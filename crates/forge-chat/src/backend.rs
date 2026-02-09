//! Chat backend with Claude API integration.

use std::sync::Arc;
use std::time::Instant;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, error, info};

use crate::audit::{AuditEntry, AuditLogger};
use crate::config::ChatConfig;
use crate::context::{ContextProvider, ContextSource, DashboardContext, MockContextSource};
use crate::error::{ChatError, Result};
use crate::rate_limit::RateLimiter;
use crate::tools::{ActionConfirmation, ToolCall, ToolRegistry, ToolResult};

/// Response from the chat backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    /// Response text from the AI.
    pub text: String,

    /// Tool calls made during processing.
    pub tool_calls: Vec<ToolCall>,

    /// Tool results.
    pub tool_results: Vec<ToolResult>,

    /// Whether confirmation is required for an action.
    pub confirmation_required: Option<ActionConfirmation>,

    /// Duration in milliseconds.
    pub duration_ms: u64,

    /// Estimated cost of this interaction.
    pub cost_usd: Option<f64>,

    /// Whether the response was successful.
    pub success: bool,

    /// Error message (if any).
    pub error: Option<String>,
}

impl ChatResponse {
    /// Create a successful response.
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tool_calls: vec![],
            tool_results: vec![],
            confirmation_required: None,
            duration_ms: 0,
            cost_usd: None,
            success: true,
            error: None,
        }
    }

    /// Create an error response.
    pub fn error(message: impl Into<String>) -> Self {
        let msg = message.into();
        Self {
            text: format!("Error: {}", msg),
            tool_calls: vec![],
            tool_results: vec![],
            confirmation_required: None,
            duration_ms: 0,
            cost_usd: None,
            success: false,
            error: Some(msg),
        }
    }

    /// Set duration.
    pub fn with_duration(mut self, duration_ms: u64) -> Self {
        self.duration_ms = duration_ms;
        self
    }

    /// Set tool calls and results.
    pub fn with_tools(mut self, calls: Vec<ToolCall>, results: Vec<ToolResult>) -> Self {
        self.tool_calls = calls;
        self.tool_results = results;
        self
    }

    /// Set confirmation required.
    pub fn with_confirmation(mut self, confirmation: ActionConfirmation) -> Self {
        self.confirmation_required = Some(confirmation);
        self
    }

    /// Set cost.
    pub fn with_cost(mut self, cost: f64) -> Self {
        self.cost_usd = Some(cost);
        self
    }
}

/// Chat backend with Claude API integration.
pub struct ChatBackend {
    config: ChatConfig,
    client: Client,
    rate_limiter: RateLimiter,
    audit_logger: AuditLogger,
    tool_registry: ToolRegistry,
    context_provider: Arc<ContextProvider>,
    conversation_history: RwLock<Vec<ConversationMessage>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConversationMessage {
    role: String,
    content: String,
}

impl ChatBackend {
    /// Create a new chat backend with the given configuration.
    pub async fn new(config: ChatConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| ChatError::ConfigError(format!("Failed to create HTTP client: {}", e)))?;

        let rate_limiter = RateLimiter::new(config.rate_limit.clone());
        let audit_logger = AuditLogger::new(config.audit.clone()).await?;
        let tool_registry = ToolRegistry::with_builtin_tools();

        // Create a mock context source for now
        // In production, this would be a real context source
        let context_source = MockContextSource::with_sample_data();
        let context_provider = Arc::new(ContextProvider::new(context_source));

        Ok(Self {
            config,
            client,
            rate_limiter,
            audit_logger,
            tool_registry,
            context_provider,
            conversation_history: RwLock::new(Vec::new()),
        })
    }

    /// Create a chat backend with a custom context source.
    pub async fn with_context_source(
        config: ChatConfig,
        source: impl ContextSource + 'static,
    ) -> Result<Self> {
        let mut backend = Self::new(config).await?;
        backend.context_provider = Arc::new(ContextProvider::new(source));
        Ok(backend)
    }

    /// Process a user command.
    pub async fn process_command(&self, input: &str) -> Result<ChatResponse> {
        let start = Instant::now();

        // Check rate limit
        self.rate_limiter.check().await?;

        info!("Processing chat command: {}", input);

        // Get current context
        let context = self.context_provider.get_context().await?;

        // Build the prompt with context
        let prompt = self.build_prompt(input, &context).await;

        // Call the Claude API
        let response = match self.call_api(&prompt, &context).await {
            Ok(resp) => resp,
            Err(e) => {
                let duration = start.elapsed().as_millis() as u64;
                let response = ChatResponse::error(e.to_string()).with_duration(duration);

                // Log the error
                let entry = AuditEntry::new(input)
                    .with_error(e.to_string())
                    .with_duration(duration);
                if let Err(log_err) = self.audit_logger.log(&entry).await {
                    error!("Failed to log audit entry: {}", log_err);
                }

                return Ok(response);
            }
        };

        // Record the command for rate limiting
        self.rate_limiter.record().await;

        let duration = start.elapsed().as_millis() as u64;
        let response = response.with_duration(duration);

        // Log the successful interaction
        let entry = AuditEntry::new(input)
            .with_response(&response.text)
            .with_tool_calls(response.tool_calls.clone())
            .with_side_effects(
                response
                    .tool_results
                    .iter()
                    .flat_map(|r| r.side_effects.clone())
                    .collect(),
            )
            .with_duration(duration);
        if let Err(log_err) = self.audit_logger.log(&entry).await {
            error!("Failed to log audit entry: {}", log_err);
        }

        Ok(response)
    }

    /// Confirm an action that requires confirmation.
    pub async fn confirm_action(&self, action: &str, confirmed: bool) -> Result<ChatResponse> {
        if !confirmed {
            return Err(ChatError::ActionCancelled);
        }

        // Re-process the action with confirmation bypass
        // In a real implementation, we'd track pending confirmations
        self.process_command(action).await
    }

    /// Clear conversation history.
    pub async fn clear_history(&self) {
        self.conversation_history.write().await.clear();
    }

    /// Get current rate limit usage.
    pub async fn rate_limit_usage(&self) -> crate::rate_limit::RateLimitUsage {
        self.rate_limiter.usage().await
    }

    async fn build_prompt(&self, input: &str, context: &DashboardContext) -> String {
        let context_summary = context.to_summary();
        let history = self.conversation_history.read().await;

        let mut prompt = String::new();

        // Add context
        prompt.push_str("Current dashboard state:\n");
        prompt.push_str(&context_summary);
        prompt.push_str("\n");

        // Add recent history (last 5 exchanges)
        let history_start = history.len().saturating_sub(10);
        for msg in &history[history_start..] {
            prompt.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        // Add current input
        prompt.push_str(&format!("User: {}\n", input));

        prompt
    }

    async fn call_api(&self, prompt: &str, context: &DashboardContext) -> Result<ChatResponse> {
        // Get API key from environment
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
            ChatError::ConfigError("ANTHROPIC_API_KEY environment variable not set".to_string())
        })?;

        let base_url = self
            .config
            .api_base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com");

        // Build the request
        let tools = self.tool_registry.tool_definitions();
        let request = ApiRequest {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            temperature: Some(self.config.temperature),
            system: SYSTEM_PROMPT.to_string(),
            messages: vec![ApiMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            tools: tools
                .iter()
                .map(|t| ApiTool {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    input_schema: t.input_schema.clone(),
                })
                .collect(),
        };

        debug!("Sending API request to {}", base_url);

        let response = self
            .client
            .post(format!("{}/v1/messages", base_url))
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ChatError::ApiError(format!(
                "API returned {}: {}",
                status, body
            )));
        }

        let api_response: ApiResponse = response.json().await?;

        // Process the response
        let mut text = String::new();
        let mut tool_calls = Vec::new();
        let mut tool_results = Vec::new();

        for content in api_response.content {
            match content {
                ContentBlock::Text { text: t } => {
                    text.push_str(&t);
                }
                ContentBlock::ToolUse { id, name, input } => {
                    let call = ToolCall {
                        name: name.clone(),
                        parameters: input.clone(),
                        id: Some(id),
                    };
                    tool_calls.push(call.clone());

                    // Execute the tool
                    match self.tool_registry.execute(&call, context).await {
                        Ok(result) => {
                            tool_results.push(result);
                        }
                        Err(ChatError::ConfirmationRequired(confirmation_json)) => {
                            // Parse confirmation and return
                            if let Ok(confirmation) =
                                serde_json::from_str::<ActionConfirmation>(&confirmation_json)
                            {
                                return Ok(ChatResponse::success(&text)
                                    .with_confirmation(confirmation)
                                    .with_tools(tool_calls, tool_results));
                            }
                        }
                        Err(e) => {
                            tool_results.push(ToolResult::error(e.to_string()));
                        }
                    }
                }
            }
        }

        // Update conversation history
        {
            let mut history = self.conversation_history.write().await;
            history.push(ConversationMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            });
            history.push(ConversationMessage {
                role: "assistant".to_string(),
                content: text.clone(),
            });

            // Keep history bounded
            const MAX_HISTORY: usize = 20;
            if history.len() > MAX_HISTORY {
                let drain_count = history.len() - MAX_HISTORY;
                history.drain(..drain_count);
            }
        }

        // Calculate cost estimate
        let cost = self.estimate_cost(&api_response.usage);

        Ok(ChatResponse::success(text)
            .with_tools(tool_calls, tool_results)
            .with_cost(cost))
    }

    fn estimate_cost(&self, usage: &ApiUsage) -> f64 {
        // Rough cost estimates based on model
        // These should be configurable or fetched from a pricing service
        let (input_cost_per_million, output_cost_per_million) = if self.config.model.contains("opus")
        {
            (15.0, 75.0)
        } else if self.config.model.contains("sonnet") {
            (3.0, 15.0)
        } else {
            (0.25, 1.25) // Haiku
        };

        let input_cost = (usage.input_tokens as f64 / 1_000_000.0) * input_cost_per_million;
        let output_cost = (usage.output_tokens as f64 / 1_000_000.0) * output_cost_per_million;

        input_cost + output_cost
    }
}

// API types

#[derive(Debug, Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    system: String,
    messages: Vec<ApiMessage>,
    tools: Vec<ApiTool>,
}


#[derive(Debug, Serialize, Deserialize)]
struct ApiMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ApiTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ApiResponse {
    id: String,
    content: Vec<ContentBlock>,
    usage: ApiUsage,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Deserialize)]
struct ApiUsage {
    input_tokens: u32,
    output_tokens: u32,
}

/// System prompt for the chat agent.
const SYSTEM_PROMPT: &str = r#"You are the conversational interface for a distributed worker control panel called FORGE.

Context:
- The user manages a pool of coding agents (workers) across multiple workspaces
- Workers process "beads" (tasks) using different LLM models (Sonnet, Opus, GLM, etc.)
- The system optimizes costs across subscriptions and pay-per-token APIs
- You have access to real-time dashboard state and can execute commands

Your role:
- Answer questions about worker status, costs, tasks, and subscriptions
- Execute commands safely (confirm destructive operations)
- Provide analysis and recommendations
- Be concise (max 5 sentences unless asked for details)
- Use tables, progress bars, and formatting when helpful
- Explain your reasoning when making recommendations

Available tools:
- get_worker_status: Get worker pool status
- get_task_queue: Get ready tasks
- get_cost_analytics: Get spending data
- get_subscription_usage: Get quota tracking
- get_activity_log: Get recent events
- spawn_worker: Spawn new workers (requires confirmation for >2)
- kill_worker: Kill a worker (requires confirmation)
- assign_task: Reassign task to different model (requires confirmation if in progress)
- pause_workers: Pause all workers
- resume_workers: Resume paused workers

Response format:
- Use markdown for formatting
- Tables for comparisons
- Status indicators: ✓ (success), ✗ (failure), ◐ (in progress)
- Progress bars: ████▌ 66%
- Keep responses under 10 lines (TUI space is limited)

Safety rules:
- Always confirm before killing workers
- Explain cost implications for model changes
- Warn about context loss when reassigning in-progress tasks
- Rate limit: User can run max 10 commands/minute
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_chat_response_success() {
        let response = ChatResponse::success("Hello, world!").with_duration(100);

        assert!(response.success);
        assert_eq!(response.text, "Hello, world!");
        assert_eq!(response.duration_ms, 100);
        assert!(response.error.is_none());
    }

    #[tokio::test]
    async fn test_chat_response_error() {
        let response = ChatResponse::error("Something went wrong");

        assert!(!response.success);
        assert!(response.text.contains("Error:"));
        assert!(response.error.is_some());
    }

    #[tokio::test]
    async fn test_cost_estimation() {
        // This would need a mock API response to test properly
        // For now, just verify the structure works
    }
}
