//! Claude API provider using direct HTTP requests.
//!
//! This module provides [`ClaudeApiProvider`] which makes direct API calls to
//! Anthropic's Claude API using the reqwest HTTP client.
//!
//! ## Example
//!
//! ```no_run
//! use forge_chat::{claude_api::ClaudeApiProvider, config::ClaudeApiConfig, context::DashboardContext, provider::ChatProvider};
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

use ::async_trait::async_trait;
use tokio::time::Duration;
use tracing::debug;

use crate::claude_api_types::{
    ApiMessage, ApiRequest, ApiResponse, ApiTool, ApiUsage, ContentBlock,
};
use crate::config::ClaudeApiConfig;
use crate::context::DashboardContext;
use crate::error::{ChatError, Result};
use crate::provider::{FinishReason, ProviderTool, TokenUsage};
use crate::tools::ToolCall;

/// Claude API provider using direct HTTP requests.
///
/// This provider uses the reqwest client to make direct API calls to
/// Anthropic's Claude API.
pub struct ClaudeApiProvider {
    config: ClaudeApiConfig,
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl ClaudeApiProvider {
    /// Create a new Claude API provider from config.
    pub fn from_config(config: ClaudeApiConfig) -> Result<Self> {
        let api_key = std::env::var(&config.api_key_env).map_err(|_| {
            ChatError::ConfigError(format!(
                "{} environment variable not set",
                config.api_key_env
            ))
        })?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| ChatError::ConfigError(format!("Failed to create HTTP client: {}", e)))?;

        let base_url = config.api_base_url.clone();

        Ok(Self {
            config,
            client,
            api_key,
            base_url,
        })
    }

    /// Create a provider with a custom API key.
    pub fn with_api_key(mut config: ClaudeApiConfig, api_key: impl Into<String>) -> Result<Self> {
        let api_key = api_key.into();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| ChatError::ConfigError(format!("Failed to create HTTP client: {}", e)))?;

        let base_url = config.api_base_url.clone();

        // Override the api_key_env to indicate we're using a custom key
        config.api_key_env = "<custom>".to_string();

        Ok(Self {
            config,
            client,
            api_key,
            base_url,
        })
    }

    /// Build the API request from prompt, context, and tools.
    fn build_request(
        &self,
        prompt: &str,
        context: &DashboardContext,
        tools: &[ProviderTool],
    ) -> ApiRequest {
        // Build the enhanced prompt with context
        let enhanced_prompt = format!(
            "{}\n\nCurrent dashboard state:\n{}",
            prompt,
            context.to_summary()
        );

        ApiRequest {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            temperature: Some(self.config.temperature),
            system: SYSTEM_PROMPT.to_string(),
            messages: vec![ApiMessage {
                role: "user".to_string(),
                content: enhanced_prompt,
            }],
            tools: tools
                .iter()
                .map(|t| ApiTool {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    input_schema: t.input_schema.clone(),
                })
                .collect(),
        }
    }

    /// Send the API request and parse the response.
    async fn send_request(&self, request: &ApiRequest) -> Result<ApiResponse> {
        debug!("Sending API request to {}", self.base_url);

        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ChatError::ApiError(format!(
                "API error: {} - {}",
                status, body
            )));
        }

        response.json().await.map_err(ChatError::from)
    }

    /// Parse the API response into text and tool calls.
    fn parse_response(api_response: ApiResponse) -> (String, Vec<ToolCall>) {
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

        (text, tool_calls)
    }

    /// Estimate cost based on model and usage.
    fn estimate_cost(&self, usage: &ApiUsage) -> f64 {
        let (input_cost_per_million, output_cost_per_million) =
            if self.config.model.contains("opus") {
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

#[async_trait]
impl crate::provider::ChatProvider for ClaudeApiProvider {
    async fn process(
        &self,
        prompt: &str,
        context: &DashboardContext,
        tools: &[ProviderTool],
    ) -> Result<crate::provider::ProviderResponse> {
        let start = std::time::Instant::now();

        // Build and send the request
        let request = self.build_request(prompt, context, tools);
        let api_response = self.send_request(&request).await?;

        // Parse the response
        let (text, tool_calls) = Self::parse_response(api_response.clone());

        let duration = start.elapsed().as_millis() as u64;
        let cost = self.estimate_cost(&api_response.usage);

        let finish_reason = if !tool_calls.is_empty() {
            FinishReason::ToolCall
        } else {
            FinishReason::Stop
        };

        let usage = TokenUsage {
            input_tokens: api_response.usage.input_tokens,
            output_tokens: api_response.usage.output_tokens,
            cache_read_tokens: api_response.usage.cache_read_tokens.unwrap_or(0),
            cache_creation_tokens: api_response.usage.cache_creation_tokens.unwrap_or(0),
        };

        Ok(crate::provider::ProviderResponse {
            text,
            tool_calls,
            duration_ms: duration,
            cost_usd: Some(cost),
            finish_reason,
            usage: Some(usage),
        })
    }

    fn name(&self) -> &str {
        "claude-api"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn estimated_cost(&self) -> Option<f64> {
        // Estimate based on max_tokens setting
        let (input_cost, output_cost) = if self.config.model.contains("opus") {
            (15.0, 75.0)
        } else if self.config.model.contains("sonnet") {
            (3.0, 15.0)
        } else {
            (0.25, 1.25)
        };
        // Rough estimate: assume 10k input, max_tokens output
        let estimated_input = 10_000.0;
        let estimated_output = self.config.max_tokens as f64;
        Some(
            (estimated_input / 1_000_000.0) * input_cost
                + (estimated_output / 1_000_000.0) * output_cost,
        )
    }

    fn model(&self) -> &str {
        &self.config.model
    }
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
    use crate::provider::ChatProvider;

    #[test]
    fn test_claude_api_provider_creation() {
        let config = ClaudeApiConfig {
            model: "claude-opus-4-5".to_string(),
            max_tokens: 2000,
            temperature: 0.5,
            ..Default::default()
        };

        // This will fail without ANTHROPIC_API_KEY, but we can test the structure
        let result = ClaudeApiProvider::from_config(config);
        assert!(result.is_err() || std::env::var("ANTHROPIC_API_KEY").is_ok());
    }

    #[test]
    fn test_system_prompt_is_set() {
        assert!(!SYSTEM_PROMPT.is_empty());
        assert!(SYSTEM_PROMPT.contains("FORGE"));
        assert!(SYSTEM_PROMPT.contains("worker"));
    }

    #[test]
    fn test_estimate_cost_by_model() {
        let config = ClaudeApiConfig {
            model: "claude-opus-4-5".to_string(),
            ..Default::default()
        };
        let provider = ClaudeApiProvider::with_api_key(config, "test-key").unwrap();

        let usage = ApiUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: Some(0),
            cache_creation_tokens: Some(0),
        };

        let cost = provider.estimate_cost(&usage);
        assert!(cost > 0.0);

        // Opus should be more expensive than Sonnet
        let sonnet_config = ClaudeApiConfig {
            model: "claude-sonnet-4-5".to_string(),
            ..Default::default()
        };
        let sonnet_provider = ClaudeApiProvider::with_api_key(sonnet_config, "test-key").unwrap();
        let sonnet_cost = sonnet_provider.estimate_cost(&usage);
        assert!(cost > sonnet_cost);
    }

    #[tokio::test]
    async fn test_claude_api_provider_with_custom_key() {
        let config = ClaudeApiConfig::default();
        let provider = ClaudeApiProvider::with_api_key(config, "sk-test-key-12345");

        assert!(provider.is_ok());
        let provider = provider.unwrap();
        assert_eq!(provider.model(), "claude-sonnet-4-5-20250929");
        assert_eq!(provider.name(), "claude-api");
        assert!(provider.supports_streaming());
    }

    // ============ HTTP Mocking Tests with wiremock ============

    #[cfg(test)]
    mod http_tests {
        use super::*;
        use crate::context::DashboardContext;
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

        #[tokio::test]
        async fn test_claude_api_with_mock_server() {
            // Start mock server
            let mock_server = MockServer::start().await;

            // Set up mock response
            let template = ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "msg-123",
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "Mock response from Claude API"
                    }
                ],
                "model": "claude-sonnet-4-5-20250929",
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 10,
                    "output_tokens": 5,
                    "cache_creation_input_tokens": 0,
                    "cache_read_input_tokens": 0
                }
            }));

            Mock::given(matchers::method("POST"))
                .and(matchers::path("/v1/messages"))
                .and(matchers::header("x-api-key", "test-key"))
                .respond_with(template)
                .mount(&mock_server)
                .await;

            // Create provider with mock server URL
            let config = ClaudeApiConfig {
                api_base_url: mock_server.uri(),
                model: "claude-sonnet-4-5-20250929".to_string(),
                ..Default::default()
            };
            let provider = ClaudeApiProvider::with_api_key(config, "test-key").unwrap();

            // Test the provider
            let context = DashboardContext::default();
            let response = provider.process("Hello", &context, &[]).await.unwrap();

            assert_eq!(response.text, "Mock response from Claude API");
            assert_eq!(response.finish_reason, crate::provider::FinishReason::Stop);
        }

        #[tokio::test]
        async fn test_claude_api_with_tool_call_response() {
            let mock_server = MockServer::start().await;

            let template = ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "msg-456",
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "I'll check the worker status."
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu-123",
                        "name": "get_worker_status",
                        "input": {"worker_id": "worker-1"}
                    }
                ],
                "model": "claude-sonnet-4-5-20250929",
                "stop_reason": "tool_use",
                "usage": {
                    "input_tokens": 15,
                    "output_tokens": 10
                }
            }));

            Mock::given(matchers::method("POST"))
                .and(matchers::path("/v1/messages"))
                .respond_with(template)
                .mount(&mock_server)
                .await;

            let config = ClaudeApiConfig {
                api_base_url: mock_server.uri(),
                ..Default::default()
            };
            let provider = ClaudeApiProvider::with_api_key(config, "test-key").unwrap();

            let context = DashboardContext::default();
            let response = provider
                .process("Check worker status", &context, &[])
                .await
                .unwrap();

            assert!(!response.text.is_empty());
            assert_eq!(response.tool_calls.len(), 1);
            assert_eq!(response.tool_calls[0].name, "get_worker_status");
            assert_eq!(response.tool_calls[0].id, Some("toolu-123".to_string()));
        }

        #[tokio::test]
        async fn test_claude_api_error_handling() {
            let mock_server = MockServer::start().await;

            // Simulate API error
            let template = ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "invalid_request_error",
                    "message": "Invalid request: missing required field"
                }
            }));

            Mock::given(matchers::method("POST"))
                .and(matchers::path("/v1/messages"))
                .respond_with(template)
                .mount(&mock_server)
                .await;

            let config = ClaudeApiConfig {
                api_base_url: mock_server.uri(),
                ..Default::default()
            };
            let provider = ClaudeApiProvider::with_api_key(config, "test-key").unwrap();

            let context = DashboardContext::default();
            let result = provider.process("Bad request", &context, &[]).await;

            assert!(result.is_err());
            match result {
                Err(crate::error::ChatError::ApiError(msg)) => {
                    assert!(msg.contains("API request failed") || msg.contains("400"));
                }
                _ => panic!("Expected ApiError"),
            }
        }
    }
}
