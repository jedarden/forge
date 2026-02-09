//! Pluggable chat provider trait and implementations.
//!
//! This module defines the [`ChatProvider`] trait which abstracts different
//! AI backends (CLI tools, HTTP APIs, etc.) for the chat system.
//!
//! ## Implementations
//!
//! - [`ClaudeApiProvider`] - Direct Anthropic API via HTTP (reqwest)
//! - [`ClaudeCliProvider`] - claude-cli via stdin/stdout (tokio::process)
//! - [`MockProvider`] - Testing mock that returns predefined responses
//!
//! ## Example
//!
//! ```no_run
//! use forge_chat::{ChatConfig, provider::{ChatProvider, ClaudeApiProvider}};
//!
//! # async fn example() -> anyhow::Result<()> {
//! let config = ChatConfig::default();
//! let provider = ClaudeApiProvider::new(config)?;
//!
//! let tools = vec![];
//! let response = provider.process("Hello!", &tools).await?;
//! println!("{}", response.text);
//! # Ok(())
//! # }
//! ```

use ::async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use tracing::{debug, info};

use crate::config::ChatConfig;
use crate::error::{ChatError, Result};
use crate::tools::{ToolCall, ToolDefinition};

/// Response from a chat provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    /// Response text from the AI.
    pub text: String,

    /// Tool calls made during processing.
    pub tool_calls: Vec<ToolCall>,

    /// Duration in milliseconds.
    pub duration_ms: u64,

    /// Estimated cost of this interaction (optional).
    pub cost_usd: Option<f64>,
}

/// Tool definition passed to providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderTool {
    /// Tool name.
    pub name: String,

    /// Tool description.
    pub description: String,

    /// Input schema (JSON Schema).
    pub input_schema: serde_json::Value,
}

impl From<ToolDefinition> for ProviderTool {
    fn from(tool: ToolDefinition) -> Self {
        Self {
            name: tool.name,
            description: tool.description,
            input_schema: tool.input_schema,
        }
    }
}

/// Trait for AI chat providers.
///
/// Implementations can use different backends:
/// - HTTP APIs (Anthropic, OpenAI, etc.)
/// - CLI tools (claude-cli, aider, etc.)
/// - Mock providers for testing
#[async_trait]
pub trait ChatProvider: Send + Sync {
    /// Process a prompt and return a response.
    ///
    /// The provider may invoke tools during processing. Tool results are
    /// NOT included in the response - callers must handle tool execution
    /// separately based on the returned tool_calls.
    async fn process(
        &self,
        prompt: &str,
        tools: &[ProviderTool],
    ) -> Result<ProviderResponse>;

    /// Get the provider name.
    fn name(&self) -> &str;

    /// Whether this provider supports streaming responses.
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Get the model name being used.
    fn model(&self) -> &str;
}

// ============ Claude API Provider ============

/// Claude API provider using direct HTTP requests.
///
/// This provider uses the reqwest client to make direct API calls to
/// Anthropic's Claude API.
pub struct ClaudeApiProvider {
    config: ChatConfig,
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl ClaudeApiProvider {
    /// Create a new Claude API provider.
    pub fn new(config: ChatConfig) -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
            ChatError::ConfigError("ANTHROPIC_API_KEY environment variable not set".to_string())
        })?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| ChatError::ConfigError(format!("Failed to create HTTP client: {}", e)))?;

        let base_url = config
            .api_base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com")
            .to_string();

        Ok(Self {
            config,
            client,
            api_key,
            base_url,
        })
    }

    /// Create a provider with a custom API key.
    pub fn with_api_key(config: ChatConfig, api_key: impl Into<String>) -> Result<Self> {
        let api_key = api_key.into();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| ChatError::ConfigError(format!("Failed to create HTTP client: {}", e)))?;

        let base_url = config
            .api_base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com")
            .to_string();

        Ok(Self {
            config,
            client,
            api_key,
            base_url,
        })
    }
}

#[async_trait]
impl ChatProvider for ClaudeApiProvider {
    async fn process(&self, prompt: &str, tools: &[ProviderTool]) -> Result<ProviderResponse> {
        let start = std::time::Instant::now();

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

        debug!("Sending API request to {}", self.base_url);

        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
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

        for content in api_response.content {
            match content {
                ContentBlock::Text { text: t } => {
                    text.push_str(&t);
                }
                ContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCall {
                        name: name.clone(),
                        parameters: input.clone(),
                        id: Some(id),
                    });
                }
            }
        }

        let duration = start.elapsed().as_millis() as u64;
        let cost = estimate_cost(&self.config.model, &api_response.usage);

        Ok(ProviderResponse {
            text,
            tool_calls,
            duration_ms: duration,
            cost_usd: Some(cost),
        })
    }

    fn name(&self) -> &str {
        "claude-api"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn model(&self) -> &str {
        &self.config.model
    }
}

// ============ Claude CLI Provider ============

/// Configuration for the claude-cli provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCliConfig {
    /// Path to the claude-cli binary.
    pub binary_path: String,

    /// Model to use (sonnet, opus, haiku).
    pub model: String,

    /// Whether to run in headless mode.
    pub headless: bool,

    /// Additional arguments to pass to claude-cli.
    pub extra_args: Vec<String>,
}

impl Default for ClaudeCliConfig {
    fn default() -> Self {
        Self {
            binary_path: "claude".to_string(),
            model: "sonnet".to_string(),
            headless: true,
            extra_args: vec![],
        }
    }
}

/// Claude CLI provider using stdin/stdout communication.
///
/// This provider spawns a claude-cli process and communicates via JSON messages.
pub struct ClaudeCliProvider {
    config: ClaudeCliConfig,
    #[allow(dead_code)]
    process: Mutex<Option<CliProcess>>,
}

/// Represents a running claude-cli process.
struct CliProcess {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
}

impl ClaudeCliProvider {
    /// Create a new Claude CLI provider.
    pub fn new(config: ClaudeCliConfig) -> Self {
        Self {
            config,
            process: Mutex::new(None),
        }
    }

    /// Create a provider from a ChatConfig.
    pub fn from_chat_config(config: &ChatConfig) -> Self {
        let cli_config = ClaudeCliConfig {
            model: model_name_from_id(&config.model),
            ..Default::default()
        };
        Self::new(cli_config)
    }

    /// Spawn the claude-cli process.
    async fn spawn_process(&self) -> Result<CliProcess> {
        let mut cmd = Command::new(&self.config.binary_path);

        // Add model flag
        cmd.arg("--model").arg(&self.config.model);

        // Add headless flag if configured
        if self.config.headless {
            cmd.arg("--headless");
        }

        // Add extra args
        cmd.args(&self.config.extra_args);

        // Set up stdin/stdout/stderr for JSON communication
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        info!("Spawning claude-cli: {:?}", cmd);

        let mut child = cmd.spawn().map_err(|e| {
            ChatError::ConfigError(format!("Failed to spawn claude-cli: {}", e))
        })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            ChatError::ConfigError("Failed to open stdin for claude-cli".to_string())
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            ChatError::ConfigError("Failed to open stdout for claude-cli".to_string())
        })?;

        let stderr = child.stderr.take().ok_or_else(|| {
            ChatError::ConfigError("Failed to open stderr for claude-cli".to_string())
        })?;

        Ok(CliProcess {
            child,
            stdin,
            stdout,
            stderr,
        })
    }

    /// Send a request to claude-cli and read the response.
    async fn send_request(&self, prompt: &str, tools: &[ProviderTool]) -> Result<CliResponse> {
        // Spawn a new process for each request (simpler reliability)
        let proc = self.spawn_process().await?;

        let CliProcess {
            mut child,
            mut stdin,
            stdout,
            mut stderr,
        } = proc;

        // Build the request JSON
        let request = CliRequest {
            prompt: prompt.to_string(),
            tools: tools.to_vec(),
        };

        let request_json =
            serde_json::to_string(&request).map_err(|e| ChatError::JsonError(e))?;

        // Send request
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(request_json.as_bytes())
            .await
            .map_err(|e| ChatError::IoError(e))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|e| ChatError::IoError(e))?;
        stdin
            .shutdown()
            .await
            .map_err(|e| ChatError::IoError(e))?;

        // Read response with timeout
        let response_bytes = timeout(Duration::from_secs(30), async {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(stdout);
            let mut line = String::new();
            reader.read_line(&mut line).await?;
            Ok::<Vec<u8>, ChatError>(line.into_bytes())
        })
        .await
        .map_err(|_| {
            // Kill the child process on timeout
            let _ = child.kill();
            ChatError::ApiError("claude-cli timeout".to_string())
        })??;

        // Also capture stderr for debugging
        let stderr_output = timeout(Duration::from_secs(1), async {
            use tokio::io::AsyncReadExt;
            let mut buf = vec![];
            stderr.read_to_end(&mut buf).await?;
            Ok::<Vec<u8>, ChatError>(buf)
        })
        .await
        .unwrap_or(Ok(vec![]))?;

        if !stderr_output.is_empty() {
            let stderr_str = String::from_utf8_lossy(&stderr_output);
            debug!("claude-cli stderr: {}", stderr_str);
        }

        // Wait for process to exit
        let status = timeout(Duration::from_secs(5), child.wait())
            .await
            .map_err(|_| ChatError::ApiError("claude-cli hang on exit".to_string()))??;

        if !status.success() {
            return Err(ChatError::ApiError(format!(
                "claude-cli exited with status: {:?}",
                status
            )));
        }

        // Parse response
        let response_str = String::from_utf8(response_bytes).map_err(|e| {
            ChatError::ApiError(format!("Invalid UTF-8 from claude-cli: {}", e))
        })?;

        debug!("claude-cli response: {}", response_str);

        let response: CliResponse =
            serde_json::from_str(&response_str).map_err(|e| {
                ChatError::ApiError(format!("Failed to parse claude-cli response: {}", e))
            })?;

        Ok(response)
    }
}

#[async_trait]
impl ChatProvider for ClaudeCliProvider {
    async fn process(&self, prompt: &str, tools: &[ProviderTool]) -> Result<ProviderResponse> {
        let start = std::time::Instant::now();

        let cli_response = self.send_request(prompt, tools).await?;

        let duration = start.elapsed().as_millis() as u64;

        // Convert tool calls
        let tool_calls = cli_response
            .tool_calls
            .into_iter()
            .map(|tc| ToolCall {
                name: tc.name,
                parameters: tc.parameters,
                id: tc.id,
            })
            .collect();

        Ok(ProviderResponse {
            text: cli_response.text,
            tool_calls,
            duration_ms: duration,
            cost_usd: Some(0.0), // CLI provider doesn't report cost
        })
    }

    fn name(&self) -> &str {
        "claude-cli"
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn model(&self) -> &str {
        &self.config.model
    }
}

/// Request sent to claude-cli.
#[derive(Debug, Serialize)]
struct CliRequest {
    prompt: String,
    tools: Vec<ProviderTool>,
}

/// Response received from claude-cli.
#[derive(Debug, Deserialize)]
struct CliResponse {
    text: String,
    #[serde(default)]
    tool_calls: Vec<CliToolCall>,
}

/// Tool call from claude-cli.
#[derive(Debug, Deserialize)]
struct CliToolCall {
    name: String,
    parameters: serde_json::Value,
    #[serde(default)]
    id: Option<String>,
}

// ============ Mock Provider ============

/// Mock provider for testing.
///
/// Returns predefined responses without making any external calls.
pub struct MockProvider {
    model: String,
    response_text: String,
    response_delay_ms: u64,
}

impl MockProvider {
    /// Create a new mock provider with default responses.
    pub fn new() -> Self {
        Self {
            model: "mock-model".to_string(),
            response_text: "This is a mock response.".to_string(),
            response_delay_ms: 0,
        }
    }

    /// Set the response text.
    pub fn with_response(mut self, text: impl Into<String>) -> Self {
        self.response_text = text.into();
        self
    }

    /// Set the response delay.
    pub fn with_delay(mut self, delay_ms: u64) -> Self {
        self.response_delay_ms = delay_ms;
        self
    }

    /// Set the model name.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for MockProvider {
    async fn process(&self, _prompt: &str, _tools: &[ProviderTool]) -> Result<ProviderResponse> {
        if self.response_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.response_delay_ms)).await;
        }

        Ok(ProviderResponse {
            text: self.response_text.clone(),
            tool_calls: vec![],
            duration_ms: self.response_delay_ms,
            cost_usd: Some(0.0),
        })
    }

    fn name(&self) -> &str {
        "mock"
    }

    fn model(&self) -> &str {
        &self.model
    }
}

// ============ Provider Factory ============

/// Create a provider from configuration.
pub fn create_provider(config: &ChatConfig) -> Result<Box<dyn ChatProvider>> {
    let provider_type = std::env::var("FORGE_CHAT_PROVIDER")
        .unwrap_or_else(|_| "claude-api".to_string());

    match provider_type.as_str() {
        "claude-cli" => {
            info!("Creating claude-cli provider");
            Ok(Box::new(ClaudeCliProvider::from_chat_config(config)))
        }
        "mock" => {
            info!("Creating mock provider");
            Ok(Box::new(MockProvider::new()))
        }
        "claude-api" | "" | _ => {
            info!("Creating claude-api provider");
            Ok(Box::new(ClaudeApiProvider::new(config.clone())?))
        }
    }
}

// ============ Helper Functions ============

/// Extract model name from model ID.
fn model_name_from_id(model_id: &str) -> String {
    if model_id.contains("opus") {
        "opus".to_string()
    } else if model_id.contains("sonnet") {
        "sonnet".to_string()
    } else if model_id.contains("haiku") {
        "haiku".to_string()
    } else {
        "sonnet".to_string()
    }
}

/// Estimate cost based on usage.
fn estimate_cost(model: &str, usage: &ApiUsage) -> f64 {
    let (input_cost_per_million, output_cost_per_million) = if model.contains("opus") {
        (15.0, 75.0)
    } else if model.contains("sonnet") {
        (3.0, 15.0)
    } else {
        (0.25, 1.25) // Haiku
    };

    let input_cost = (usage.input_tokens as f64 / 1_000_000.0) * input_cost_per_million;
    let output_cost = (usage.output_tokens as f64 / 1_000_000.0) * output_cost_per_million;

    input_cost + output_cost
}

// ============ API Types ============

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
struct ApiResponse {
    #[allow(dead_code)]
    id: String,
    content: Vec<ContentBlock>,
    usage: ApiUsage,
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

    #[test]
    fn test_model_name_from_id() {
        assert_eq!(model_name_from_id("claude-opus-4-5"), "opus");
        assert_eq!(model_name_from_id("claude-sonnet-4-5"), "sonnet");
        assert_eq!(model_name_from_id("claude-haiku-4-5"), "haiku");
        assert_eq!(model_name_from_id("unknown"), "sonnet");
    }

    #[test]
    fn test_estimate_cost() {
        let usage = ApiUsage {
            input_tokens: 1000,
            output_tokens: 500,
        };

        let opus_cost = estimate_cost("claude-opus-4", &usage);
        assert!(opus_cost > 0.0);

        let sonnet_cost = estimate_cost("claude-sonnet-4", &usage);
        assert!(sonnet_cost < opus_cost);

        let haiku_cost = estimate_cost("claude-haiku-4", &usage);
        assert!(haiku_cost < sonnet_cost);
    }

    #[tokio::test]
    async fn test_mock_provider() {
        let provider = MockProvider::new()
            .with_response("Test response")
            .with_delay(10);

        assert_eq!(provider.name(), "mock");
        assert_eq!(provider.model(), "mock-model");
        assert!(!provider.supports_streaming());

        let response = provider.process("Hello", &[]).await.unwrap();
        assert_eq!(response.text, "Test response");
        assert_eq!(response.tool_calls.len(), 0);
        assert!(response.duration_ms >= 10);
        assert_eq!(response.cost_usd, Some(0.0));
    }

    #[test]
    fn test_claude_cli_config_default() {
        let config = ClaudeCliConfig::default();
        assert_eq!(config.binary_path, "claude");
        assert_eq!(config.model, "sonnet");
        assert!(config.headless);
        assert!(config.extra_args.is_empty());
    }

    #[test]
    fn test_provider_tool_conversion() {
        let tool_def = ToolDefinition {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        };

        let provider_tool: ProviderTool = tool_def.into();
        assert_eq!(provider_tool.name, "test_tool");
        assert_eq!(provider_tool.description, "A test tool");
    }
}
