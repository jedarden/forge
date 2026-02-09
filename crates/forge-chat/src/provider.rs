//! Pluggable chat provider trait and implementations.
//!
//! This module defines the [`ChatProvider`] trait which abstracts different
//! AI backends (CLI tools, HTTP APIs, etc.) for the chat system.
//!
//! ## Implementations
//!
//! - [`ClaudeApiProvider`] - Direct Anthropic API via HTTP (reqwest) (see [`claude_api`])
//! - [`ClaudeCliProvider`] - claude-cli via stdin/stdout (tokio::process)
//! - [`MockProvider`] - Testing mock that returns predefined responses
//!
//! ## Example
//!
//! ```no_run
//! use forge_chat::{provider::{ChatProvider}, config::ClaudeApiConfig, context::DashboardContext};
//! use forge_chat::claude_api::ClaudeApiProvider;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let config = ClaudeApiConfig::default();
//! let provider = ClaudeApiProvider::from_config(config)?;
//!
//! let context = DashboardContext::default();
//! let tools = vec![];
//! let response = provider.process("Hello!", &context, &tools).await?;
//! println!("{}", response.text);
//! # Ok(())
//! # }
//! ```

use crate::claude_api::ClaudeApiProvider;

use ::async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use tracing::{debug, info};

use crate::config::{
    ChatConfig, ClaudeApiConfig, ClaudeCliConfig, MockConfig, ProviderConfig,
};
use crate::context::DashboardContext;
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

    /// Reason why the response ended.
    pub finish_reason: FinishReason,

    /// Token usage information (if available).
    pub usage: Option<TokenUsage>,
}

impl ProviderResponse {
    /// Create a new basic response.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tool_calls: vec![],
            duration_ms: 0,
            cost_usd: None,
            finish_reason: FinishReason::Stop,
            usage: None,
        }
    }

    /// Add tool calls to the response.
    pub fn with_tool_calls(mut self, calls: Vec<ToolCall>) -> Self {
        self.tool_calls = calls;
        self.finish_reason = if !self.tool_calls.is_empty() {
            FinishReason::ToolCall
        } else {
            FinishReason::Stop
        };
        self
    }

    /// Set the finish reason.
    pub fn with_finish_reason(mut self, reason: FinishReason) -> Self {
        self.finish_reason = reason;
        self
    }

    /// Set the token usage.
    pub fn with_usage(mut self, usage: TokenUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Set the duration.
    pub fn with_duration(mut self, duration_ms: u64) -> Self {
        self.duration_ms = duration_ms;
        self
    }

    /// Set the cost.
    pub fn with_cost(mut self, cost: f64) -> Self {
        self.cost_usd = Some(cost);
        self
    }
}

/// Reason why the provider's response ended.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FinishReason {
    /// Normal stop sequence.
    Stop,
    /// Tool calls were made.
    ToolCall,
    /// Max tokens reached.
    MaxTokens,
    /// Error occurred.
    Error(String),
}

impl Default for FinishReason {
    fn default() -> Self {
        Self::Stop
    }
}

/// Token usage information for a provider response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Input tokens consumed.
    pub input_tokens: u32,

    /// Output tokens consumed.
    pub output_tokens: u32,

    /// Cache read tokens (prompt caching hits).
    pub cache_read_tokens: u32,

    /// Cache creation tokens (new cache entries).
    pub cache_creation_tokens: u32,
}

impl TokenUsage {
    /// Total tokens consumed (input + output).
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens as u64 + self.output_tokens as u64
    }

    /// Total cache tokens (read + creation).
    pub fn total_cache_tokens(&self) -> u64 {
        self.cache_read_tokens as u64 + self.cache_creation_tokens as u64
    }

    /// Create zero usage.
    pub fn zero() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        }
    }
}

impl Default for TokenUsage {
    fn default() -> Self {
        Self::zero()
    }
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
    /// Process a prompt with context and tools.
    ///
    /// The provider may invoke tools during processing. Tool results are
    /// NOT included in the response - callers must handle tool execution
    /// separately based on the returned tool_calls.
    ///
    /// # Arguments
    /// * `prompt` - The user's prompt/command
    /// * `context` - Current dashboard context for contextual awareness
    /// * `tools` - Available tools that the provider may invoke
    async fn process(
        &self,
        prompt: &str,
        context: &DashboardContext,
        tools: &[ProviderTool],
    ) -> Result<ProviderResponse>;

    /// Get the provider name for logging/debugging.
    fn name(&self) -> &str;

    /// Whether this provider supports streaming responses.
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Estimated cost per request (if known).
    ///
    /// Returns None if cost estimation is not available for this provider.
    /// This is useful for cost tracking and budget management.
    fn estimated_cost(&self) -> Option<f64> {
        None
    }

    /// Get the model name being used.
    fn model(&self) -> &str;
}

// ============ Claude CLI Provider ============
// Note: Claude API provider has been moved to claude_api.rs module

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

    /// Create a provider from a ChatConfig (deprecated - use ProviderConfig directly).
    #[deprecated(note = "Use ProviderConfig::ClaudeCli directly instead")]
    pub fn from_chat_config(_config: &ChatConfig) -> Self {
        Self::new(ClaudeCliConfig::default())
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
    async fn process(
        &self,
        prompt: &str,
        context: &DashboardContext,
        tools: &[ProviderTool],
    ) -> Result<ProviderResponse> {
        let start = std::time::Instant::now();

        // Build the enhanced prompt with context
        let enhanced_prompt = format!(
            "{}\n\nCurrent dashboard state:\n{}",
            prompt,
            context.to_summary()
        );

        let cli_response = self.send_request(&enhanced_prompt, tools).await?;

        let duration = start.elapsed().as_millis() as u64;

        // Convert tool calls
        let tool_calls: Vec<ToolCall> = cli_response
            .tool_calls
            .into_iter()
            .map(|tc| ToolCall {
                name: tc.name,
                parameters: tc.parameters,
                id: tc.id,
            })
            .collect();

        let finish_reason = if !tool_calls.is_empty() {
            FinishReason::ToolCall
        } else {
            FinishReason::Stop
        };

        Ok(ProviderResponse {
            text: cli_response.text,
            tool_calls,
            duration_ms: duration,
            cost_usd: Some(0.0), // CLI provider doesn't report cost
            finish_reason,
            usage: None, // CLI doesn't provide token usage
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
        Self::from_config(MockConfig::default())
    }

    /// Create a mock provider from config.
    pub fn from_config(config: MockConfig) -> Self {
        Self {
            model: config.model,
            response_text: config.response,
            response_delay_ms: config.delay_ms,
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
    async fn process(
        &self,
        _prompt: &str,
        _context: &DashboardContext,
        _tools: &[ProviderTool],
    ) -> Result<ProviderResponse> {
        if self.response_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.response_delay_ms)).await;
        }

        Ok(ProviderResponse {
            text: self.response_text.clone(),
            tool_calls: vec![],
            duration_ms: self.response_delay_ms,
            cost_usd: Some(0.0),
            finish_reason: FinishReason::Stop,
            usage: None,
        })
    }

    fn name(&self) -> &str {
        "mock"
    }

    fn estimated_cost(&self) -> Option<f64> {
        Some(0.0)
    }

    fn model(&self) -> &str {
        &self.model
    }
}

// ============ Provider Factory ============

/// Create a provider from configuration.
///
/// This factory function creates the appropriate provider based on the
/// `ProviderConfig` in `ChatConfig`.
///
/// # Example
///
/// ```no_run
/// use forge_chat::{ChatConfig, provider::create_provider};
///
/// # fn main() -> anyhow::Result<()> {
/// let config = ChatConfig::default();
/// let provider = create_provider(&config)?;
/// # Ok(())
/// # }
/// ```
pub fn create_provider(config: &ChatConfig) -> Result<Box<dyn ChatProvider>> {
    // Check for environment variable override (for backward compatibility)
    if let Ok(provider_type) = std::env::var("FORGE_CHAT_PROVIDER") {
        info!(
            "Using FORGE_CHAT_PROVIDER environment variable: {}",
            provider_type
        );
        return create_provider_by_type(&provider_type, config);
    }

    // Use the provider from config
    match &config.provider {
        ProviderConfig::ClaudeApi(api_config) => {
            info!("Creating claude-api provider");
            Ok(Box::new(ClaudeApiProvider::from_config(api_config.clone())?))
        }
        ProviderConfig::ClaudeCli(cli_config) => {
            info!("Creating claude-cli provider");
            Ok(Box::new(ClaudeCliProvider::new(cli_config.clone())))
        }
        ProviderConfig::Mock(mock_config) => {
            info!("Creating mock provider");
            Ok(Box::new(MockProvider::from_config(mock_config.clone())))
        }
    }
}

/// Create a provider by type string (for backward compatibility with env var).
fn create_provider_by_type(
    provider_type: &str,
    _config: &ChatConfig,
) -> Result<Box<dyn ChatProvider>> {
    match provider_type {
        "claude-cli" => {
            info!("Creating claude-cli provider from env var");
            Ok(Box::new(ClaudeCliProvider::new(ClaudeCliConfig::default())))
        }
        "mock" => {
            info!("Creating mock provider from env var");
            Ok(Box::new(MockProvider::new()))
        }
        "claude-api" | "" | _ => {
            info!("Creating claude-api provider from env var");
            Ok(Box::new(ClaudeApiProvider::from_config(
                ClaudeApiConfig::default(),
            )?))
        }
    }
}

// ============ Helper Functions ============

// Helper functions and API types have been moved to claude_api and claude_api_types modules
// See claude_api.rs for SYSTEM_PROMPT and provider implementation
// See claude_api_types.rs for ApiRequest, ApiResponse, ContentBlock, etc.

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_provider() {
        use crate::context::DashboardContext;

        let provider = MockProvider::new()
            .with_response("Test response")
            .with_delay(10);

        assert_eq!(provider.name(), "mock");
        assert_eq!(provider.model(), "mock-model");
        assert!(!provider.supports_streaming());
        assert_eq!(provider.estimated_cost(), Some(0.0));

        let context = DashboardContext::default();
        let response = provider.process("Hello", &context, &[]).await.unwrap();
        assert_eq!(response.text, "Test response");
        assert_eq!(response.tool_calls.len(), 0);
        assert!(response.duration_ms >= 10);
        assert_eq!(response.cost_usd, Some(0.0));
        assert_eq!(response.finish_reason, FinishReason::Stop);
        assert!(response.usage.is_none());
    }

    #[test]
    fn test_claude_cli_config_default() {
        let config = ClaudeCliConfig::default();
        assert_eq!(config.binary_path, "claude-code");
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

    // ============ Claude CLI Provider Tests ============

    #[test]
    fn test_claude_cli_provider_creation() {
        let config = ClaudeCliConfig {
            binary_path: "/usr/bin/claude".to_string(),
            model: "opus".to_string(),
            config_dir: None,
            timeout_secs: 30,
            headless: true,
            extra_args: vec!["--debug".to_string()],
        };

        let provider = ClaudeCliProvider::new(config.clone());
        assert_eq!(provider.name(), "claude-cli");
        assert_eq!(provider.model(), "opus");
        assert!(!provider.supports_streaming());
    }

    #[test]
    fn test_claude_cli_from_chat_config() {
        use crate::config::{ChatConfig, ProviderConfig};

        let chat_config = ChatConfig {
            provider: ProviderConfig::ClaudeCli(crate::config::ClaudeCliConfig {
                model: "opus".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        };

        // Verify the provider config is set correctly
        match &chat_config.provider {
            ProviderConfig::ClaudeCli(cli_config) => {
                assert_eq!(cli_config.model, "opus");
            }
            _ => panic!("Expected ClaudeCli provider"),
        }
    }

    #[test]
    fn test_claude_cli_request_serialization() {
        let request = CliRequest {
            prompt: "Hello, world!".to_string(),
            tools: vec![ProviderTool {
                name: "test_tool".to_string(),
                description: "A test tool".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            }],
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("Hello, world!"));
        assert!(json.contains("test_tool"));

        // CliRequest is Serialize only (for sending to CLI), not Deserialize
        // We verify the JSON structure is valid
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["prompt"], "Hello, world!");
        assert_eq!(parsed["tools"][0]["name"], "test_tool");
    }

    #[test]
    fn test_claude_cli_response_parsing() {
        // Test basic text response
        let json = r#"{"text": "Hello there!", "tool_calls": []}"#;
        let response: CliResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.text, "Hello there!");
        assert!(response.tool_calls.is_empty());

        // Test response with tool calls
        let json_with_tools = r#"{
            "text": "I'll help you with that.",
            "tool_calls": [
                {
                    "name": "get_worker_status",
                    "parameters": {"worker_id": "worker-1"},
                    "id": "call_123"
                }
            ]
        }"#;
        let response_with_tools: CliResponse = serde_json::from_str(json_with_tools).unwrap();
        assert_eq!(response_with_tools.text, "I'll help you with that.");
        assert_eq!(response_with_tools.tool_calls.len(), 1);
        assert_eq!(response_with_tools.tool_calls[0].name, "get_worker_status");
        assert_eq!(response_with_tools.tool_calls[0].id, Some("call_123".to_string()));
    }

    #[test]
    fn test_claude_cli_response_tool_call_parsing() {
        // Test tool call without ID
        let json = r#"{
            "text": "Executing tool",
            "tool_calls": [
                {
                    "name": "spawn_worker",
                    "parameters": {"worker_type": "sonnet", "count": 2}
                }
            ]
        }"#;
        let response: CliResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.tool_calls[0].name, "spawn_worker");
        assert!(response.tool_calls[0].id.is_none());
        assert_eq!(
            response.tool_calls[0].parameters["worker_type"],
            "sonnet"
        );
        assert_eq!(response.tool_calls[0].parameters["count"], 2);
    }

    #[test]
    fn test_claude_cli_response_default_tool_calls() {
        // Test that tool_calls defaults to empty array
        let json = r#"{"text": "Simple response"}"#;
        let response: CliResponse = serde_json::from_str(json).unwrap();
        assert!(response.tool_calls.is_empty());
    }

    #[test]
    fn test_claude_cli_response_malformed_json() {
        // Test malformed JSON handling
        let json = r#"{"text": "Incomplete"#;
        let result: std::result::Result<CliResponse, serde_json::Error> =
            serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_provider_response_builder() {
        let response = ProviderResponse::new("Test response")
            .with_duration(100)
            .with_cost(0.5)
            .with_finish_reason(FinishReason::ToolCall)
            .with_usage(TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
            });

        assert_eq!(response.text, "Test response");
        assert_eq!(response.duration_ms, 100);
        assert_eq!(response.cost_usd, Some(0.5));
        assert_eq!(response.finish_reason, FinishReason::ToolCall);
        assert!(response.usage.is_some());
        assert_eq!(response.usage.unwrap().total_tokens(), 150);
    }

    #[test]
    fn test_token_usage_calculations() {
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 200,
            cache_creation_tokens: 100,
        };

        assert_eq!(usage.total_tokens(), 1500);
        assert_eq!(usage.total_cache_tokens(), 300);
    }

    #[test]
    fn test_finish_reason_display() {
        assert_eq!(FinishReason::Stop, FinishReason::Stop);
        assert_eq!(FinishReason::ToolCall, FinishReason::ToolCall);
        assert_eq!(FinishReason::MaxTokens, FinishReason::MaxTokens);
        assert_eq!(
            FinishReason::Error("test".to_string()),
            FinishReason::Error("test".to_string())
        );
    }

    #[tokio::test]
    async fn test_claude_cli_process_with_dashboard_context() {
        use crate::context::DashboardContext;

        let config = ClaudeCliConfig::default();
        let provider = ClaudeCliProvider::new(config);

        // Note: This test will fail if claude-cli is not installed
        // In a real test environment, you would mock the subprocess
        let context = DashboardContext::default();
        let tools: Vec<ProviderTool> = vec![];

        // This will attempt to spawn the real claude-cli process
        // In production tests, this should be mocked
        let result = provider.process("Hello", &context, &tools).await;

        // We expect this to fail in test environment without claude-cli
        // But we can verify the error type
        match result {
            Ok(_) => {
                // If claude-cli is installed and working, that's fine too
            }
            Err(ChatError::ConfigError(_)) => {
                // Expected: claude-cli not found
            }
            Err(ChatError::ApiError(_)) => {
                // Also possible: claude-cli failed to start
            }
            Err(other) => {
                panic!("Unexpected error type: {:?}", other);
            }
        }
    }
}
