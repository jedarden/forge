//! Chat backend with pluggable provider support.

use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::audit::{AuditEntry, AuditLogger};
use crate::config::ChatConfig;
use crate::context::{ContextProvider, ContextSource, DashboardContext, RealContextSource};
use crate::error::{ChatError, Result};
use crate::provider::{ChatProvider, ProviderTool, create_provider};
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

    /// Provider type that was used.
    pub provider: String,
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
            provider: "unknown".to_string(),
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
            provider: "unknown".to_string(),
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

    /// Set provider.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }
}

/// Chat backend with pluggable provider support.
pub struct ChatBackend {
    #[allow(dead_code)]
    config: ChatConfig,
    provider: Box<dyn ChatProvider>,
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
        let provider = create_provider(&config)?;

        let rate_limiter = RateLimiter::new(config.rate_limit.clone());
        let audit_logger = AuditLogger::new(config.audit.clone()).await?;
        let tool_registry = ToolRegistry::with_builtin_tools();

        // Use RealContextSource for live dashboard data
        let context_source = RealContextSource::new();
        let context_provider = Arc::new(ContextProvider::new(context_source));

        Ok(Self {
            config,
            provider,
            rate_limiter,
            audit_logger,
            tool_registry,
            context_provider,
            conversation_history: RwLock::new(Vec::new()),
        })
    }

    /// Create a chat backend with a custom provider.
    pub async fn with_provider(
        config: ChatConfig,
        provider: Box<dyn ChatProvider>,
    ) -> Result<Self> {
        let rate_limiter = RateLimiter::new(config.rate_limit.clone());
        let audit_logger = AuditLogger::new(config.audit.clone()).await?;
        let tool_registry = ToolRegistry::with_builtin_tools();

        // Use RealContextSource for live dashboard data
        let context_source = RealContextSource::new();
        let context_provider = Arc::new(ContextProvider::new(context_source));

        Ok(Self {
            config,
            provider,
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
        let provider = create_provider(&config)?;

        let rate_limiter = RateLimiter::new(config.rate_limit.clone());
        let audit_logger = AuditLogger::new(config.audit.clone()).await?;
        let tool_registry = ToolRegistry::with_builtin_tools();

        let context_provider = Arc::new(ContextProvider::new(source));

        Ok(Self {
            config,
            provider,
            rate_limiter,
            audit_logger,
            tool_registry,
            context_provider,
            conversation_history: RwLock::new(Vec::new()),
        })
    }

    /// Get the provider being used.
    pub fn provider_name(&self) -> &str {
        self.provider.name()
    }

    /// Get the model being used.
    pub fn model(&self) -> &str {
        self.provider.model()
    }

    /// Process a user command.
    pub async fn process_command(&self, input: &str) -> Result<ChatResponse> {
        let start = Instant::now();
        let provider_name = self.provider.name().to_string();

        // Check rate limit
        self.rate_limiter.check().await?;

        info!("Processing chat command with {}: {}", provider_name, input);

        // Get current context
        let context = self.context_provider.get_context().await?;

        // Build the prompt with context
        let prompt = self.build_prompt(input, &context).await;

        // Get tool definitions
        let tools: Vec<ProviderTool> = self
            .tool_registry
            .tool_definitions()
            .into_iter()
            .map(|t| t.into())
            .collect();

        // Call the provider with context
        let response = match self.provider.process(&prompt, &context, &tools).await {
            Ok(resp) => resp,
            Err(e) => {
                let duration = start.elapsed().as_millis() as u64;
                let response = ChatResponse::error(e.to_string())
                    .with_duration(duration)
                    .with_provider(&provider_name);

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

        // Execute tool calls
        let mut tool_results = Vec::new();
        let mut confirmation_required = None;

        for call in &response.tool_calls {
            match self.tool_registry.execute(call, &context).await {
                Ok(result) => {
                    tool_results.push(result);
                }
                Err(ChatError::ConfirmationRequired(confirmation_json)) => {
                    // Parse confirmation and return
                    if let Ok(confirmation) =
                        serde_json::from_str::<ActionConfirmation>(&confirmation_json)
                    {
                        confirmation_required = Some(confirmation);
                        break;
                    }
                }
                Err(e) => {
                    tool_results.push(ToolResult::error(e.to_string()));
                }
            }
        }

        // Record the command for rate limiting
        self.rate_limiter.record().await;

        let duration = start.elapsed().as_millis() as u64;

        // Build the response
        let mut chat_response = ChatResponse::success(response.text)
            .with_tools(response.tool_calls.clone(), tool_results)
            .with_duration(duration)
            .with_provider(&provider_name);

        if let Some(cost) = response.cost_usd {
            chat_response = chat_response.with_cost(cost);
        }

        if let Some(confirmation) = confirmation_required {
            chat_response = chat_response.with_confirmation(confirmation);
        }

        // Log the successful interaction
        let entry = AuditEntry::new(input)
            .with_response(&chat_response.text)
            .with_tool_calls(chat_response.tool_calls.clone())
            .with_side_effects(
                chat_response
                    .tool_results
                    .iter()
                    .flat_map(|r| r.side_effects.clone())
                    .collect(),
            )
            .with_duration(duration);
        if let Err(log_err) = self.audit_logger.log(&entry).await {
            error!("Failed to log audit entry: {}", log_err);
        }

        Ok(chat_response)
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
        prompt.push('\n');

        // Add recent history (last 5 exchanges)
        let history_start = history.len().saturating_sub(10);
        for msg in &history[history_start..] {
            prompt.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        // Add current input
        prompt.push_str(&format!("User: {}\n", input));

        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_chat_response_success() {
        let response = ChatResponse::success("Hello, world!")
            .with_duration(100)
            .with_provider("test-provider");

        assert!(response.success);
        assert_eq!(response.text, "Hello, world!");
        assert_eq!(response.duration_ms, 100);
        assert!(response.error.is_none());
        assert_eq!(response.provider, "test-provider");
    }

    #[tokio::test]
    async fn test_chat_response_error() {
        let response = ChatResponse::error("Something went wrong");

        assert!(!response.success);
        assert!(response.text.contains("Error:"));
        assert!(response.error.is_some());
    }

    #[tokio::test]
    async fn test_chat_backend_with_mock_provider() {
        use crate::provider::MockProvider;

        let config = ChatConfig::default();
        let mock_provider = MockProvider::new()
            .with_response("Mock response for testing")
            .with_model("test-model");

        let backend = ChatBackend::with_provider(config, Box::new(mock_provider))
            .await
            .unwrap();

        assert_eq!(backend.provider_name(), "mock");
        assert_eq!(backend.model(), "test-model");

        let response = backend.process_command("test input").await.unwrap();
        assert!(response.success);
        assert_eq!(response.text, "Mock response for testing");
        assert_eq!(response.provider, "mock");
    }

    #[tokio::test]
    async fn test_chat_backend_clear_history() {
        use crate::provider::MockProvider;

        let config = ChatConfig::default();
        let mock_provider = MockProvider::new();

        let backend = ChatBackend::with_provider(config, Box::new(mock_provider))
            .await
            .unwrap();

        // Clear history should not panic
        backend.clear_history().await;
    }
}
