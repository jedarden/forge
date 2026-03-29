//! OpenCode CLI provider for forge-chat.
//!
//! This module implements [`OpencodeProvider`] which spawns the `opencode` CLI
//! subprocess and communicates via streaming JSON output.
//!
//! ## Command Format
//!
//! ```text
//! opencode run --format json --model <provider/model> "<prompt>"
//! ```
//!
//! ## Output Format
//!
//! Opencode outputs newline-delimited JSON events:
//! - `{"type":"step_start",...}` — step begins
//! - `{"type":"text","part":{"text":"Hello."},...}` — text chunk
//! - `{"type":"step_finish","part":{"reason":"stop","tokens":{...},"cost":0},...}` — step done
//! - `{"type":"error","part":{"message":"..."},...}` — error event

use std::process::Stdio;

use ::async_trait::async_trait;
use serde::Deserialize;
use tokio::process::Command;
use tokio::time::{Duration, timeout};
use tracing::{debug, info};

use crate::config::OpencodeConfig;
use crate::context::DashboardContext;
use crate::error::{ChatError, Result};
use crate::provider::{ChatProvider, FinishReason, ProviderResponse, ProviderTool, TokenUsage};

/// OpenCode CLI provider.
///
/// Spawns `opencode run --format json` as a subprocess and parses the
/// streaming JSON output to extract the response text, cost, and token usage.
pub struct OpencodeProvider {
    config: OpencodeConfig,
}

impl OpencodeProvider {
    /// Create a new OpenCode provider.
    pub fn new(config: OpencodeConfig) -> Self {
        Self { config }
    }

    /// Send a prompt to opencode and parse the streaming JSON response.
    async fn send_request(&self, prompt: &str) -> Result<OpencodeResponse> {
        let mut cmd = Command::new(&self.config.binary_path);

        // opencode run --format json --model <provider/model> "<prompt>"
        cmd.args([
            "run",
            "--format",
            "json",
            "--model",
            self.config.resolve_model(),
        ])
        .arg(prompt);

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        info!("Spawning opencode: {:?}", cmd);

        let mut child = cmd
            .spawn()
            .map_err(|e| ChatError::ConfigError(format!("Failed to spawn opencode: {}", e)))?;

        let stdout = child.stdout.take().ok_or_else(|| {
            ChatError::ConfigError("Failed to open stdout for opencode".to_string())
        })?;

        let stderr = child.stderr.take().ok_or_else(|| {
            ChatError::ConfigError("Failed to open stderr for opencode".to_string())
        })?;

        // Read all stdout with a configurable timeout.
        let stdout_bytes = timeout(Duration::from_secs(self.config.timeout_secs), async {
            use tokio::io::AsyncReadExt;
            let mut buf = vec![];
            let mut out = stdout;
            out.read_to_end(&mut buf).await?;
            Ok::<Vec<u8>, std::io::Error>(buf)
        })
        .await
        .map_err(|_| {
            drop(child.kill());
            ChatError::ApiError("opencode timeout".to_string())
        })?
        .map_err(ChatError::IoError)?;

        // Collect stderr for error diagnostics.
        let stderr_bytes = timeout(Duration::from_secs(1), async {
            use tokio::io::AsyncReadExt;
            let mut buf = vec![];
            let mut err = stderr;
            err.read_to_end(&mut buf).await?;
            Ok::<Vec<u8>, std::io::Error>(buf)
        })
        .await
        .unwrap_or(Ok(vec![]))
        .unwrap_or_default();

        // Wait for the process to exit.
        let status = timeout(Duration::from_secs(5), child.wait())
            .await
            .map_err(|_| ChatError::ApiError("opencode hang on exit".to_string()))??;

        if !status.success() {
            let stderr_str = String::from_utf8_lossy(&stderr_bytes);
            return Err(ChatError::ApiError(format!(
                "opencode exited with status: {:?}, stderr: {}",
                status, stderr_str
            )));
        }

        let stdout_str = String::from_utf8(stdout_bytes)
            .map_err(|e| ChatError::ApiError(format!("Invalid UTF-8 from opencode: {}", e)))?;

        debug!("opencode raw output: {}", stdout_str);

        self.parse_response(&stdout_str)
    }

    /// Parse newline-delimited JSON events from opencode output.
    fn parse_response(&self, output: &str) -> Result<OpencodeResponse> {
        let mut text_parts: Vec<String> = Vec::new();
        let mut total_cost = 0.0_f64;
        let mut usage = TokenUsage::zero();

        for line in output.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let event: OpencodeEvent = match serde_json::from_str(line) {
                Ok(e) => e,
                Err(e) => {
                    debug!("Skipping unparseable opencode line: {} — {}", line, e);
                    continue;
                }
            };

            match event.event_type.as_str() {
                "text" => {
                    if let Some(part) = event.part {
                        // Skip synthetic continuation messages.
                        if part.synthetic != Some(true) {
                            if let Some(text) = part.text {
                                text_parts.push(text);
                            }
                        }
                    }
                }
                "step_finish" => {
                    if let Some(part) = event.part {
                        if let Some(cost) = part.cost {
                            total_cost += cost;
                        }
                        if let Some(tokens) = part.tokens {
                            usage.input_tokens = tokens.input.unwrap_or(0) as u32;
                            usage.output_tokens = tokens.output.unwrap_or(0) as u32;
                        }
                    }
                }
                "error" => {
                    let msg = event
                        .part
                        .and_then(|p| p.message)
                        .unwrap_or_else(|| "unknown opencode error".to_string());
                    return Err(ChatError::ApiError(format!("opencode error: {}", msg)));
                }
                _ => {
                    // step_start and unknown events are ignored.
                }
            }
        }

        Ok(OpencodeResponse {
            text: text_parts.join("\n"),
            cost: total_cost,
            usage,
        })
    }
}

// ── Internal types ────────────────────────────────────────────────────────────

#[derive(Debug)]
struct OpencodeResponse {
    text: String,
    cost: f64,
    usage: TokenUsage,
}

#[derive(Debug, Deserialize)]
struct OpencodeEvent {
    #[serde(rename = "type")]
    event_type: String,
    part: Option<OpencodePart>,
}

#[derive(Debug, Deserialize)]
struct OpencodePart {
    text: Option<String>,
    #[serde(default)]
    synthetic: Option<bool>,
    cost: Option<f64>,
    tokens: Option<OpencodeTokens>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpencodeTokens {
    input: Option<u64>,
    output: Option<u64>,
}

// ── ChatProvider impl ─────────────────────────────────────────────────────────

#[async_trait]
impl ChatProvider for OpencodeProvider {
    async fn process(
        &self,
        prompt: &str,
        context: &DashboardContext,
        _tools: &[ProviderTool],
    ) -> Result<ProviderResponse> {
        let start = std::time::Instant::now();

        let enhanced_prompt = format!(
            "{}\n\nCurrent dashboard state:\n{}",
            prompt,
            context.to_summary()
        );

        let response = self.send_request(&enhanced_prompt).await?;
        let duration = start.elapsed().as_millis() as u64;

        Ok(ProviderResponse {
            text: response.text,
            tool_calls: vec![],
            duration_ms: duration,
            cost_usd: Some(response.cost),
            finish_reason: FinishReason::Stop,
            usage: Some(response.usage),
        })
    }

    fn name(&self) -> &str {
        "opencode"
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn model(&self) -> &str {
        self.config.resolve_model()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OpencodeConfig;

    fn provider() -> OpencodeProvider {
        OpencodeProvider::new(OpencodeConfig::default())
    }

    #[test]
    fn test_parse_text_events() {
        let p = provider();
        let output = r#"
{"type":"step_start","timestamp":1,"sessionID":"s1","part":{}}
{"type":"text","timestamp":2,"part":{"text":"Hello, world!"}}
{"type":"step_finish","timestamp":3,"part":{"reason":"stop","tokens":{"input":10,"output":5},"cost":0.001}}
"#;
        let resp = p.parse_response(output).unwrap();
        assert_eq!(resp.text, "Hello, world!");
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
        assert!((resp.cost - 0.001).abs() < 1e-9);
    }

    #[test]
    fn test_parse_multiple_text_events() {
        let p = provider();
        let output = r#"
{"type":"text","part":{"text":"Line one"}}
{"type":"text","part":{"text":"Line two"}}
"#;
        let resp = p.parse_response(output).unwrap();
        assert_eq!(resp.text, "Line one\nLine two");
    }

    #[test]
    fn test_skips_synthetic_text() {
        let p = provider();
        let output = r#"
{"type":"text","part":{"text":"Real text"}}
{"type":"text","part":{"text":"Continue if you have next steps","synthetic":true}}
"#;
        let resp = p.parse_response(output).unwrap();
        assert_eq!(resp.text, "Real text");
    }

    #[test]
    fn test_error_event_returns_err() {
        let p = provider();
        let output = r#"{"type":"error","part":{"message":"model not found"}}"#;
        let err = p.parse_response(output).unwrap_err();
        assert!(err.to_string().contains("model not found"));
    }

    #[test]
    fn test_skips_malformed_lines() {
        let p = provider();
        let output = r#"
not json at all
{"type":"text","part":{"text":"Good line"}}
"#;
        let resp = p.parse_response(output).unwrap();
        assert_eq!(resp.text, "Good line");
    }
}
