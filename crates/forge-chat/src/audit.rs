//! Audit logging for chat commands.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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

    /// Model used for the request.
    pub model: Option<String>,
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
            model: None,
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

    /// Set the model.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
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
        let Some(file) = &self.file else {
            return Ok(());
        };

        let json = serde_json::to_string(entry)?;
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
}
