//! Log parser for extracting API usage from worker logs.
//!
//! Supports multiple API formats:
//! - Anthropic (Claude): input_tokens, output_tokens, cache tokens
//! - OpenAI: prompt_tokens, completion_tokens
//! - DeepSeek: Uses OpenAI-compatible format
//! - GLM (z.ai proxy): Anthropic-compatible with modelUsage

use crate::error::{CostError, Result};
use crate::models::ApiCall;
use chrono::Utc;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use tracing::{debug, trace, warn};

/// Model pricing in USD per million tokens.
/// Based on January 2026 pricing.
#[derive(Debug, Clone)]
pub struct ModelPricing {
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub cache_creation_per_million: f64,
    pub cache_read_per_million: f64,
}

impl ModelPricing {
    pub fn new(input: f64, output: f64) -> Self {
        Self {
            input_per_million: input,
            output_per_million: output,
            // Default cache pricing: creation same as input, read is 10%
            cache_creation_per_million: input,
            cache_read_per_million: input * 0.1,
        }
    }

    pub fn with_cache(mut self, creation: f64, read: f64) -> Self {
        self.cache_creation_per_million = creation;
        self.cache_read_per_million = read;
        self
    }

    /// Calculate cost from token counts.
    pub fn calculate_cost(
        &self,
        input: i64,
        output: i64,
        cache_creation: i64,
        cache_read: i64,
    ) -> f64 {
        (input as f64 * self.input_per_million / 1_000_000.0)
            + (output as f64 * self.output_per_million / 1_000_000.0)
            + (cache_creation as f64 * self.cache_creation_per_million / 1_000_000.0)
            + (cache_read as f64 * self.cache_read_per_million / 1_000_000.0)
    }
}

/// Default model pricing configuration.
pub fn default_pricing() -> HashMap<String, ModelPricing> {
    let mut pricing = HashMap::new();

    // Anthropic Claude models (January 2026 pricing)
    // Opus 4.5/4.6
    pricing.insert(
        "claude-opus".to_string(),
        ModelPricing::new(15.0, 75.0).with_cache(18.75, 1.50),
    );

    // Sonnet 4.5
    pricing.insert(
        "claude-sonnet".to_string(),
        ModelPricing::new(3.0, 15.0).with_cache(3.75, 0.30),
    );

    // Haiku 4.5
    pricing.insert(
        "claude-haiku".to_string(),
        ModelPricing::new(0.80, 4.0).with_cache(1.0, 0.08),
    );

    // GLM models (via z.ai proxy) - approximate pricing
    pricing.insert(
        "glm-4.7".to_string(),
        ModelPricing::new(1.0, 2.0).with_cache(1.0, 0.10),
    );

    // OpenAI models
    pricing.insert("gpt-4-turbo".to_string(), ModelPricing::new(10.0, 30.0));

    pricing.insert("gpt-4o".to_string(), ModelPricing::new(5.0, 15.0));

    // DeepSeek
    pricing.insert("deepseek-chat".to_string(), ModelPricing::new(0.14, 0.28));

    pricing.insert("deepseek-coder".to_string(), ModelPricing::new(0.14, 0.28));

    pricing
}

/// Log parser for extracting API usage events.
pub struct LogParser {
    pricing: HashMap<String, ModelPricing>,
}

impl LogParser {
    /// Create a new parser with default pricing.
    pub fn new() -> Self {
        Self {
            pricing: default_pricing(),
        }
    }

    /// Create a parser with custom pricing.
    pub fn with_pricing(pricing: HashMap<String, ModelPricing>) -> Self {
        Self { pricing }
    }

    /// Add or update pricing for a model.
    pub fn set_pricing(&mut self, model: &str, pricing: ModelPricing) {
        self.pricing.insert(model.to_string(), pricing);
    }

    /// Parse all log files in a directory.
    pub fn parse_directory<P: AsRef<Path>>(&self, dir: P) -> Result<Vec<ApiCall>> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            return Err(CostError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Directory not found: {}", dir.display()),
            )));
        }

        let mut all_calls = Vec::new();

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|e| e == "log") {
                match self.parse_file(&path) {
                    Ok(calls) => {
                        debug!(file = %path.display(), count = calls.len(), "Parsed log file");
                        all_calls.extend(calls);
                    }
                    Err(e) => {
                        warn!(file = %path.display(), error = %e, "Failed to parse log file");
                    }
                }
            }
        }

        Ok(all_calls)
    }

    /// Parse a single log file.
    pub fn parse_file<P: AsRef<Path>>(&self, path: P) -> Result<Vec<ApiCall>> {
        let path = path.as_ref();
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        // Extract worker ID from filename
        let worker_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let mut calls = Vec::new();
        let mut line_number = 0;

        for line in reader.lines() {
            line_number += 1;
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    trace!(line = line_number, error = %e, "Failed to read line");
                    continue;
                }
            };

            // Skip non-JSON lines (like log rotation messages)
            if !line.starts_with('{') {
                continue;
            }

            match self.parse_line(&line, &worker_id) {
                Ok(Some(call)) => calls.push(call),
                Ok(None) => {} // Line didn't contain usage data
                Err(e) => {
                    trace!(line = line_number, error = %e, "Failed to parse line");
                }
            }
        }

        Ok(calls)
    }

    /// Parse a single JSON log line.
    pub fn parse_line(&self, line: &str, worker_id: &str) -> Result<Option<ApiCall>> {
        let value: Value = serde_json::from_str(line)?;

        // Check event type
        let event_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            "result" => self.parse_result_event(&value, worker_id),
            "assistant" => self.parse_assistant_event(&value, worker_id),
            _ => Ok(None),
        }
    }

    /// Parse a "result" event (session summary with total cost).
    fn parse_result_event(&self, value: &Value, worker_id: &str) -> Result<Option<ApiCall>> {
        // Check for usage data
        let usage = match value.get("usage") {
            Some(u) => u,
            None => return Ok(None),
        };

        let session_id = value.get("session_id").and_then(|v| v.as_str());

        // Extract bead_id (task ID) if present in the log event
        let bead_id = value.get("bead_id").and_then(|v| v.as_str());

        // Try to extract cost directly (Claude Code provides this)
        let cost_usd = value.get("total_cost_usd").and_then(|v| v.as_f64());

        // Extract model - check modelUsage first (GLM format), then look for model in usage
        let model = self.extract_model_from_result(value);

        // Parse token usage
        let input_tokens = usage
            .get("input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let output_tokens = usage
            .get("output_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let cache_creation = usage
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let cache_read = usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        // Skip if no tokens (empty result)
        if input_tokens == 0 && output_tokens == 0 && cache_creation == 0 && cache_read == 0 {
            return Ok(None);
        }

        // Calculate cost if not provided
        let final_cost = cost_usd.unwrap_or_else(|| {
            self.calculate_cost(
                &model,
                input_tokens,
                output_tokens,
                cache_creation,
                cache_read,
            )
        });

        // Try to extract timestamp from uuid field or use current time
        let timestamp = Utc::now(); // Result events are session summaries

        let mut call = ApiCall::new(
            timestamp,
            worker_id,
            &model,
            input_tokens,
            output_tokens,
            final_cost,
        )
        .with_cache(cache_creation, cache_read);

        if let Some(sid) = session_id {
            call = call.with_session(sid);
        }

        if let Some(bid) = bead_id {
            call = call.with_bead(bid);
        }

        call.event_type = "result".to_string();

        Ok(Some(call))
    }

    /// Parse an "assistant" event (individual API call).
    fn parse_assistant_event(&self, value: &Value, worker_id: &str) -> Result<Option<ApiCall>> {
        let message = match value.get("message") {
            Some(m) => m,
            None => return Ok(None),
        };

        let usage = match message.get("usage") {
            Some(u) => u,
            None => return Ok(None),
        };

        let session_id = value.get("session_id").and_then(|v| v.as_str());

        // Extract bead_id (task ID) if present in the log event
        let bead_id = value.get("bead_id").and_then(|v| v.as_str());

        let model = message
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        // Parse token usage - handle both Anthropic and OpenAI formats
        let (input_tokens, output_tokens, cache_creation, cache_read) =
            self.parse_usage_tokens(usage);

        // Skip if no tokens
        if input_tokens == 0 && output_tokens == 0 {
            return Ok(None);
        }

        // Calculate cost
        let cost = self.calculate_cost(
            model,
            input_tokens,
            output_tokens,
            cache_creation,
            cache_read,
        );

        let timestamp = Utc::now();

        let mut call = ApiCall::new(
            timestamp,
            worker_id,
            model,
            input_tokens,
            output_tokens,
            cost,
        )
        .with_cache(cache_creation, cache_read);

        if let Some(sid) = session_id {
            call = call.with_session(sid);
        }

        if let Some(bid) = bead_id {
            call = call.with_bead(bid);
        }

        call.event_type = "assistant".to_string();

        Ok(Some(call))
    }

    /// Extract model name from result event.
    fn extract_model_from_result(&self, value: &Value) -> String {
        // Try modelUsage first (GLM/z.ai format)
        if let Some(model_usage) = value.get("modelUsage")
            && let Some(obj) = model_usage.as_object()
            && let Some(model) = obj.keys().next()
        {
            return model.clone();
        }

        // Try usage.model
        if let Some(model) = value
            .get("usage")
            .and_then(|u| u.get("model"))
            .and_then(|m| m.as_str())
        {
            return model.to_string();
        }

        // Fall back to examining the result text for model hints
        "unknown".to_string()
    }

    /// Parse token counts from usage object, handling different formats.
    fn parse_usage_tokens(&self, usage: &Value) -> (i64, i64, i64, i64) {
        // Anthropic format
        let input = usage
            .get("input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let output = usage
            .get("output_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let cache_creation = usage
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let cache_read = usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        // OpenAI format (if Anthropic fields are missing)
        let input = if input == 0 {
            usage
                .get("prompt_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
        } else {
            input
        };

        let output = if output == 0 {
            usage
                .get("completion_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
        } else {
            output
        };

        (input, output, cache_creation, cache_read)
    }

    /// Calculate cost for a model and token usage.
    fn calculate_cost(
        &self,
        model: &str,
        input: i64,
        output: i64,
        cache_creation: i64,
        cache_read: i64,
    ) -> f64 {
        // Normalize model name to find pricing
        let normalized = self.normalize_model_name(model);

        let pricing = self.pricing.get(&normalized).cloned().unwrap_or_else(|| {
            // Default pricing for unknown models
            warn!(model = model, normalized = %normalized, "Unknown model, using default pricing");
            ModelPricing::new(3.0, 15.0) // Default to Sonnet-like pricing
        });

        pricing.calculate_cost(input, output, cache_creation, cache_read)
    }

    /// Normalize model name to match pricing keys.
    fn normalize_model_name(&self, model: &str) -> String {
        let model = model.to_lowercase();

        // Anthropic Claude models
        if model.contains("opus") {
            return "claude-opus".to_string();
        }
        if model.contains("sonnet") {
            return "claude-sonnet".to_string();
        }
        if model.contains("haiku") {
            return "claude-haiku".to_string();
        }

        // GLM models
        if model.contains("glm") {
            return "glm-4.7".to_string();
        }

        // OpenAI models
        if model.contains("gpt-4-turbo") || model.contains("gpt-4-1106") {
            return "gpt-4-turbo".to_string();
        }
        if model.contains("gpt-4o") || model.contains("gpt-4-o") {
            return "gpt-4o".to_string();
        }

        // DeepSeek models
        if model.contains("deepseek-coder") {
            return "deepseek-coder".to_string();
        }
        if model.contains("deepseek") {
            return "deepseek-chat".to_string();
        }

        // Return normalized lowercase version
        model
    }
}

impl Default for LogParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse result events only from a log file (for cost summaries).
pub fn parse_results_only<P: AsRef<Path>>(path: P) -> Result<Vec<ApiCall>> {
    let parser = LogParser::new();
    let calls = parser.parse_file(path)?;
    Ok(calls
        .into_iter()
        .filter(|c| c.event_type == "result")
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_result_line() {
        let parser = LogParser::new();
        let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":356775,"duration_api_ms":309143,"num_turns":45,"result":"Done","stop_reason":null,"session_id":"d52fbd7d-2d77-4048-9b3a-58740d54b4e6","total_cost_usd":2.5879285,"usage":{"input_tokens":2,"cache_creation_input_tokens":92308,"cache_read_input_tokens":3072787,"output_tokens":18984}}"#;

        let call = parser.parse_line(line, "test-worker").unwrap().unwrap();

        assert_eq!(call.worker_id, "test-worker");
        assert_eq!(call.input_tokens, 2);
        assert_eq!(call.output_tokens, 18984);
        assert_eq!(call.cache_creation_tokens, 92308);
        assert_eq!(call.cache_read_tokens, 3072787);
        assert!((call.cost_usd - 2.5879285).abs() < 0.0001);
        assert_eq!(
            call.session_id,
            Some("d52fbd7d-2d77-4048-9b3a-58740d54b4e6".to_string())
        );
    }

    #[test]
    fn test_parse_assistant_line() {
        let parser = LogParser::new();
        let line = r#"{"type":"assistant","message":{"model":"claude-opus-4-5-20251101","id":"msg_01","type":"message","role":"assistant","content":[],"usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":200,"cache_read_input_tokens":300}},"session_id":"sess-123"}"#;

        let call = parser.parse_line(line, "test-worker").unwrap().unwrap();

        assert_eq!(call.worker_id, "test-worker");
        assert_eq!(call.model, "claude-opus-4-5-20251101");
        assert_eq!(call.input_tokens, 100);
        assert_eq!(call.output_tokens, 50);
        assert_eq!(call.cache_creation_tokens, 200);
        assert_eq!(call.cache_read_tokens, 300);
        assert_eq!(call.event_type, "assistant");
    }

    #[test]
    fn test_parse_glm_result_line() {
        let parser = LogParser::new();
        let line = r#"{"type":"result","subtype":"success","total_cost_usd":0.3594570000000001,"usage":{"input_tokens":10549,"cache_creation_input_tokens":0,"cache_read_input_tokens":727040,"output_tokens":5509},"modelUsage":{"glm-4.7":{"inputTokens":15836,"outputTokens":5923}},"session_id":"60e69c73"}"#;

        let call = parser.parse_line(line, "glm-worker").unwrap().unwrap();

        assert_eq!(call.worker_id, "glm-worker");
        assert_eq!(call.model, "glm-4.7");
        assert!((call.cost_usd - 0.3594570000000001).abs() < 0.0001);
    }

    #[test]
    fn test_normalize_model_name() {
        let parser = LogParser::new();

        assert_eq!(
            parser.normalize_model_name("claude-opus-4-5-20251101"),
            "claude-opus"
        );
        assert_eq!(
            parser.normalize_model_name("claude-sonnet-4-5-20250929"),
            "claude-sonnet"
        );
        assert_eq!(
            parser.normalize_model_name("claude-haiku-4-5-20251001"),
            "claude-haiku"
        );
        assert_eq!(parser.normalize_model_name("glm-4.7"), "glm-4.7");
        assert_eq!(parser.normalize_model_name("gpt-4o-2024-08-06"), "gpt-4o");
        assert_eq!(
            parser.normalize_model_name("deepseek-coder-v2"),
            "deepseek-coder"
        );
    }

    #[test]
    fn test_skip_non_usage_events() {
        let parser = LogParser::new();

        // System event (no usage)
        let line = r#"{"type":"system","subtype":"init","cwd":"/home/coder/forge"}"#;
        assert!(parser.parse_line(line, "test").unwrap().is_none());

        // User event
        let line = r#"{"type":"user","message":"hello"}"#;
        assert!(parser.parse_line(line, "test").unwrap().is_none());
    }

    #[test]
    fn test_calculate_cost() {
        let parser = LogParser::new();

        // Opus pricing: $15/$75 per million
        let cost = parser.calculate_cost("claude-opus", 1_000_000, 1_000_000, 0, 0);
        assert!((cost - 90.0).abs() < 0.01); // 15 + 75

        // Sonnet pricing: $3/$15 per million
        let cost = parser.calculate_cost("claude-sonnet", 1_000_000, 1_000_000, 0, 0);
        assert!((cost - 18.0).abs() < 0.01); // 3 + 15
    }

    #[test]
    fn test_parse_result_with_bead_id() {
        let parser = LogParser::new();
        let line = r#"{"type":"result","subtype":"success","total_cost_usd":0.05,"usage":{"input_tokens":100,"output_tokens":50},"session_id":"sess-123","bead_id":"fg-3nck"}"#;

        let call = parser.parse_line(line, "test-worker").unwrap().unwrap();

        assert_eq!(call.worker_id, "test-worker");
        assert_eq!(call.session_id, Some("sess-123".to_string()));
        assert_eq!(call.bead_id, Some("fg-3nck".to_string()));
        assert!((call.cost_usd - 0.05).abs() < 0.0001);
    }

    #[test]
    fn test_parse_assistant_with_bead_id() {
        let parser = LogParser::new();
        let line = r#"{"type":"assistant","message":{"model":"claude-sonnet","usage":{"input_tokens":100,"output_tokens":50}},"session_id":"sess-456","bead_id":"fg-abc"}"#;

        let call = parser.parse_line(line, "test-worker").unwrap().unwrap();

        assert_eq!(call.worker_id, "test-worker");
        assert_eq!(call.session_id, Some("sess-456".to_string()));
        assert_eq!(call.bead_id, Some("fg-abc".to_string()));
        assert_eq!(call.event_type, "assistant");
    }
}
