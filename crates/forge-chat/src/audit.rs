//! Audit logging for chat commands and the tool calls they trigger.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::config::{AuditConfig, AuditLogLevel};
use crate::error::{ChatError, Result};
use crate::tools::{SideEffect, ToolCall};

/// Audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Timestamp of the command.
    pub timestamp: DateTime<Utc>,

    /// User input command.
    pub command: String,

    /// Agent response text.
    pub response: Option<String>,

    /// Tool calls made during processing.
    pub tool_calls: Vec<ToolCall>,

    /// Side effects from tool executions.
    pub side_effects: Vec<SideEffect>,

    /// Total cost of the API call (if tracked).
    pub cost_usd: Option<f64>,

    /// Duration in milliseconds.
    pub duration_ms: u64,

    /// Whether the command was successful.
    pub success: bool,

    /// Error message (if any).
    pub error: Option<String>,
}

impl AuditEntry {
    /// Create a new audit entry for a command.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            timestamp: Utc::now(),
            command: command.into(),
            response: None,
            tool_calls: vec![],
            side_effects: vec![],
            cost_usd: None,
            duration_ms: 0,
            success: true,
            error: None,
        }
    }

    /// Set the response.
    pub fn with_response(mut self, response: impl Into<String>) -> Self {
        self.response = Some(response.into());
        self
    }

    /// Add a tool call.
    pub fn with_tool_call(mut self, call: ToolCall) -> Self {
        self.tool_calls.push(call);
        self
    }

    /// Add tool calls.
    pub fn with_tool_calls(mut self, calls: Vec<ToolCall>) -> Self {
        self.tool_calls.extend(calls);
        self
    }

    /// Add side effects.
    pub fn with_side_effects(mut self, effects: Vec<SideEffect>) -> Self {
        self.side_effects.extend(effects);
        self
    }

    /// Set the cost.
    pub fn with_cost(mut self, cost: f64) -> Self {
        self.cost_usd = Some(cost);
        self
    }

    /// Set the duration.
    pub fn with_duration(mut self, duration_ms: u64) -> Self {
        self.duration_ms = duration_ms;
        self
    }

    /// Mark as failed with error.
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.success = false;
        self.error = Some(error.into());
        self
    }
}

/// How confirmation was resolved for a tool invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationOutcome {
    /// The tool does not require confirmation; it executed directly.
    NotRequired,

    /// The tool requires confirmation; it did not execute and awaits approval.
    Required,

    /// The tool executed after the user approved the confirmation.
    Approved,

    /// The user declined the confirmation; the tool did not execute.
    Declined,
}

/// Execution result recorded for a tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAuditResult {
    /// Whether the tool execution succeeded.
    pub success: bool,

    /// Human-readable result or error message.
    pub message: String,
}

/// Audit record for a single tool call triggered by a chat command.
///
/// Written as one JSONL line to the configured audit log, alongside (and
/// distinguishable from) the per-command [`AuditEntry`] records via the
/// `record_type` discriminator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAuditEntry {
    /// Record type discriminator, always `"tool_call"`.
    #[serde(default = "default_tool_record_type")]
    pub record_type: String,

    /// When the tool invocation happened.
    pub timestamp: DateTime<Utc>,

    /// The user command that triggered the invocation.
    pub command: String,

    /// Provider that produced the tool call (e.g. "claude-api", "mock").
    pub provider: String,

    /// Name of the tool that was invoked.
    pub tool: String,

    /// Arguments the tool was invoked with.
    pub arguments: serde_json::Value,

    /// Execution result; absent when the tool never ran (e.g. pending or
    /// declined confirmation).
    pub result: Option<ToolAuditResult>,

    /// How confirmation was resolved for this invocation.
    pub confirmation: ConfirmationOutcome,
}

fn default_tool_record_type() -> String {
    "tool_call".to_string()
}

impl ToolAuditEntry {
    /// Create a new tool-call audit record.
    pub fn new(
        command: impl Into<String>,
        provider: impl Into<String>,
        tool: impl Into<String>,
        arguments: serde_json::Value,
    ) -> Self {
        Self {
            record_type: default_tool_record_type(),
            timestamp: Utc::now(),
            command: command.into(),
            provider: provider.into(),
            tool: tool.into(),
            arguments,
            result: None,
            confirmation: ConfirmationOutcome::NotRequired,
        }
    }

    /// Record an execution result.
    pub fn with_result(mut self, success: bool, message: impl Into<String>) -> Self {
        self.result = Some(ToolAuditResult {
            success,
            message: message.into(),
        });
        self
    }

    /// Set the confirmation outcome.
    pub fn with_confirmation(mut self, confirmation: ConfirmationOutcome) -> Self {
        self.confirmation = confirmation;
        self
    }
}

/// Audit logger for chat commands.
pub struct AuditLogger {
    config: AuditConfig,
    file: Option<Mutex<File>>,
}

impl AuditLogger {
    /// Create a new audit logger.
    pub async fn new(config: AuditConfig) -> Result<Self> {
        if !config.enabled {
            return Ok(Self { config, file: None });
        }

        // Ensure the parent directory exists
        if let Some(parent) = config.log_file.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ChatError::AuditError(format!(
                    "Failed to create audit log directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        // Open the file for appending
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&config.log_file)
            .await
            .map_err(|e| {
                ChatError::AuditError(format!(
                    "Failed to open audit log {}: {}",
                    config.log_file.display(),
                    e
                ))
            })?;

        Ok(Self {
            config,
            file: Some(Mutex::new(file)),
        })
    }

    /// Create a disabled audit logger.
    pub fn disabled() -> Self {
        Self {
            config: AuditConfig {
                enabled: false,
                log_file: PathBuf::new(),
                log_level: AuditLogLevel::All,
            },
            file: None,
        }
    }

    /// Log an audit entry.
    pub async fn log(&self, entry: &AuditEntry) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Check log level
        match self.config.log_level {
            AuditLogLevel::ErrorsOnly if entry.success => return Ok(()),
            AuditLogLevel::CommandsOnly => {
                // Log without response
                let mut stripped = entry.clone();
                stripped.response = None;
                return self.write_entry(&stripped).await;
            }
            _ => {}
        }

        self.write_entry(entry).await
    }

    async fn write_entry(&self, entry: &AuditEntry) -> Result<()> {
        self.write_line(serde_json::to_string(entry)?).await
    }

    /// Log a tool-call audit record.
    ///
    /// Honors the same [`AuditConfig`] switches as [`AuditLogger::log`]:
    /// disabled loggers drop the record, `ErrorsOnly` keeps only failed
    /// executions, and `CommandsOnly` strips the execution result.
    pub async fn log_tool_call(&self, entry: &ToolAuditEntry) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Check log level
        match self.config.log_level {
            AuditLogLevel::ErrorsOnly => {
                // Only failed executions are interesting in errors-only mode
                if !entry.result.as_ref().is_some_and(|r| !r.success) {
                    return Ok(());
                }
            }
            AuditLogLevel::CommandsOnly => {
                // Log the invocation without its result payload
                let mut stripped = entry.clone();
                stripped.result = None;
                return self.write_tool_entry(&stripped).await;
            }
            AuditLogLevel::All => {}
        }

        self.write_tool_entry(entry).await
    }

    async fn write_tool_entry(&self, entry: &ToolAuditEntry) -> Result<()> {
        self.write_line(serde_json::to_string(entry)?).await
    }

    async fn write_line(&self, json: String) -> Result<()> {
        let Some(file) = &self.file else {
            return Ok(());
        };

        let line = format!("{}\n", json);

        let mut file = file.lock().await;
        file.write_all(line.as_bytes())
            .await
            .map_err(|e| ChatError::AuditError(format!("Failed to write audit log: {}", e)))?;
        file.flush()
            .await
            .map_err(|e| ChatError::AuditError(format!("Failed to flush audit log: {}", e)))?;

        Ok(())
    }

    /// Read the tool-call records from the configured audit log.
    ///
    /// Returns an empty vector when logging is disabled.
    pub async fn read_tool_entries(&self) -> Result<Vec<ToolAuditEntry>> {
        match self.log_file() {
            Some(path) => read_tool_entries_from(path).await,
            None => Ok(vec![]),
        }
    }

    /// Get the log file path.
    pub fn log_file(&self) -> Option<&PathBuf> {
        if self.config.enabled {
            Some(&self.config.log_file)
        } else {
            None
        }
    }

    /// Check if logging is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

/// Read tool-call records from a JSONL audit log file.
///
/// Blank lines, malformed JSON, and records of the other entry kind (e.g.
/// per-command [`AuditEntry`] lines sharing the same file) are skipped rather
/// than returned as errors, so a truncated or partially corrupted log never
/// breaks reads of the records that do parse.
pub async fn read_tool_entries_from(path: &Path) -> Result<Vec<ToolAuditEntry>> {
    let contents = match tokio::fs::read_to_string(path).await {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };

    Ok(contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_audit_entry_builder() {
        let entry = AuditEntry::new("test command")
            .with_response("test response")
            .with_duration(100)
            .with_cost(0.01);

        assert_eq!(entry.command, "test command");
        assert_eq!(entry.response, Some("test response".to_string()));
        assert_eq!(entry.duration_ms, 100);
        assert_eq!(entry.cost_usd, Some(0.01));
        assert!(entry.success);
    }

    #[tokio::test]
    async fn test_audit_entry_error() {
        let entry = AuditEntry::new("test command").with_error("something went wrong");

        assert!(!entry.success);
        assert_eq!(entry.error, Some("something went wrong".to_string()));
    }

    #[tokio::test]
    async fn test_audit_logger_disabled() {
        let logger = AuditLogger::disabled();
        assert!(!logger.is_enabled());

        let entry = AuditEntry::new("test");
        // Should not error even when disabled
        logger.log(&entry).await.unwrap();
    }

    #[tokio::test]
    async fn test_audit_logger_writes() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("audit.jsonl");

        let config = AuditConfig {
            enabled: true,
            log_file: log_path.clone(),
            log_level: AuditLogLevel::All,
        };

        let logger = AuditLogger::new(config).await.unwrap();

        let entry = AuditEntry::new("test command")
            .with_response("test response")
            .with_duration(100);

        logger.log(&entry).await.unwrap();

        // Read the file and verify
        let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
        assert!(contents.contains("test command"));
        assert!(contents.contains("test response"));
    }

    #[tokio::test]
    async fn test_audit_logger_commands_only() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("audit.jsonl");

        let config = AuditConfig {
            enabled: true,
            log_file: log_path.clone(),
            log_level: AuditLogLevel::CommandsOnly,
        };

        let logger = AuditLogger::new(config).await.unwrap();

        let entry = AuditEntry::new("test command")
            .with_response("test response")
            .with_duration(100);

        logger.log(&entry).await.unwrap();

        // Read the file and verify response is stripped
        let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
        assert!(contents.contains("test command"));
        // Response should be null in the JSON
        let parsed: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
        assert!(parsed["response"].is_null());
    }

    #[tokio::test]
    async fn test_audit_logger_errors_only() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("audit.jsonl");

        let config = AuditConfig {
            enabled: true,
            log_file: log_path.clone(),
            log_level: AuditLogLevel::ErrorsOnly,
        };

        let logger = AuditLogger::new(config).await.unwrap();

        // Success entry should not be logged
        let success_entry = AuditEntry::new("success command");
        logger.log(&success_entry).await.unwrap();

        // Error entry should be logged
        let error_entry = AuditEntry::new("error command").with_error("something went wrong");
        logger.log(&error_entry).await.unwrap();

        // Read the file and verify only error is logged
        let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
        assert!(!contents.contains("success command"));
        assert!(contents.contains("error command"));
    }

    fn tool_entry(command: &str, tool: &str) -> ToolAuditEntry {
        ToolAuditEntry::new(
            command,
            "mock",
            tool,
            serde_json::json!({"session_name": "glm-delta"}),
        )
    }

    #[tokio::test]
    async fn test_tool_audit_entries_appended() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("chat-audit.jsonl");

        let config = AuditConfig {
            enabled: true,
            log_file: log_path.clone(),
            log_level: AuditLogLevel::All,
        };
        let logger = AuditLogger::new(config).await.unwrap();

        // Two invocations append two lines, in order, without clobbering
        logger
            .log_tool_call(
                &tool_entry("spawn a worker", "get_worker_status")
                    .with_result(true, "Found 3 workers (2 healthy, 1 idle)"),
            )
            .await
            .unwrap();
        logger
            .log_tool_call(
                &tool_entry("kill glm-delta", "kill_worker")
                    .with_confirmation(ConfirmationOutcome::Required),
            )
            .await
            .unwrap();

        let entries = read_tool_entries_from(&log_path).await.unwrap();
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].record_type, "tool_call");
        assert_eq!(entries[0].command, "spawn a worker");
        assert_eq!(entries[0].provider, "mock");
        assert_eq!(entries[0].tool, "get_worker_status");
        assert_eq!(
            entries[0].arguments,
            serde_json::json!({"session_name": "glm-delta"})
        );
        assert_eq!(entries[0].confirmation, ConfirmationOutcome::NotRequired);
        let result = entries[0].result.as_ref().unwrap();
        assert!(result.success);
        assert_eq!(result.message, "Found 3 workers (2 healthy, 1 idle)");

        // A pending confirmation records that the tool never ran
        assert_eq!(entries[1].tool, "kill_worker");
        assert_eq!(entries[1].confirmation, ConfirmationOutcome::Required);
        assert!(entries[1].result.is_none());
    }

    #[tokio::test]
    async fn test_read_skips_malformed_lines() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("chat-audit.jsonl");

        // A realistic mix: valid records interleaved with truncated writes,
        // garbage, blank lines, and a valid command-level entry
        let valid = serde_json::to_string(&tool_entry("first command", "get_task_queue")).unwrap();
        tokio::fs::write(
            &log_path,
            format!(
                "{}\nnot json at all\n{{\"truncated\":\n\n{{\"record_type\":\"command\"}}\n{}\n",
                valid,
                serde_json::to_string(
                    &tool_entry("second command", "kill_worker")
                        .with_confirmation(ConfirmationOutcome::Declined)
                )
                .unwrap()
            ),
        )
        .await
        .unwrap();

        let entries = read_tool_entries_from(&log_path).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].command, "first command");
        assert_eq!(entries[0].tool, "get_task_queue");
        assert_eq!(entries[1].command, "second command");
        assert_eq!(entries[1].confirmation, ConfirmationOutcome::Declined);
    }

    #[tokio::test]
    async fn test_read_missing_file_is_empty() {
        let temp_dir = TempDir::new().unwrap();
        let entries = read_tool_entries_from(&temp_dir.path().join("absent.jsonl"))
            .await
            .unwrap();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn test_tool_audit_disabled_writes_nothing() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("chat-audit.jsonl");

        // AuditConfig::default honors `enabled`; a disabled logger must not
        // create or write the file
        let config = AuditConfig {
            enabled: false,
            log_file: log_path.clone(),
            log_level: AuditLogLevel::All,
        };
        let logger = AuditLogger::new(config).await.unwrap();
        assert!(!logger.is_enabled());
        assert!(logger.log_file().is_none());

        logger
            .log_tool_call(&tool_entry("test", "get_worker_status").with_result(true, "ok"))
            .await
            .unwrap();

        assert!(!log_path.exists());
        assert!(logger.read_tool_entries().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_tool_audit_log_levels() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("chat-audit.jsonl");

        // ErrorsOnly keeps only failed executions
        let config = AuditConfig {
            enabled: true,
            log_file: log_path.clone(),
            log_level: AuditLogLevel::ErrorsOnly,
        };
        let logger = AuditLogger::new(config).await.unwrap();
        logger
            .log_tool_call(&tool_entry("ok", "get_worker_status").with_result(true, "fine"))
            .await
            .unwrap();
        logger
            .log_tool_call(&tool_entry("bad", "spawn_worker").with_result(false, "boom"))
            .await
            .unwrap();

        let entries = read_tool_entries_from(&log_path).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, "bad");

        // CommandsOnly strips the result payload but keeps the invocation
        tokio::fs::remove_file(&log_path).await.unwrap();
        let config = AuditConfig {
            enabled: true,
            log_file: log_path.clone(),
            log_level: AuditLogLevel::CommandsOnly,
        };
        let logger = AuditLogger::new(config).await.unwrap();
        logger
            .log_tool_call(&tool_entry("cmd", "get_worker_status").with_result(true, "fine"))
            .await
            .unwrap();

        let entries = read_tool_entries_from(&log_path).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, "cmd");
        assert!(entries[0].result.is_none());
    }
}
