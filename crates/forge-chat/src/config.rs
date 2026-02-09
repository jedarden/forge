//! Configuration for the chat backend.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Chat backend configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatConfig {
    /// Model to use for chat (e.g., "claude-sonnet-4-5-20250929")
    pub model: String,

    /// Maximum tokens for responses
    pub max_tokens: u32,

    /// Temperature for responses (0.0 - 1.0)
    pub temperature: f32,

    /// Rate limit configuration
    pub rate_limit: RateLimitConfig,

    /// Audit logging configuration
    pub audit: AuditConfig,

    /// Tool confirmations configuration
    pub confirmations: ConfirmationConfig,

    /// API base URL (defaults to Anthropic API)
    pub api_base_url: Option<String>,

    /// Response timeout in seconds
    pub timeout_secs: u64,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-5-20250929".to_string(),
            max_tokens: 1000,
            temperature: 0.2,
            rate_limit: RateLimitConfig::default(),
            audit: AuditConfig::default(),
            confirmations: ConfirmationConfig::default(),
            api_base_url: None,
            timeout_secs: 30,
        }
    }
}

/// Rate limiting configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum commands per minute
    pub max_per_minute: u32,

    /// Maximum commands per hour
    pub max_per_hour: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_per_minute: 10,
            max_per_hour: 100,
        }
    }
}

/// Audit logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    /// Whether audit logging is enabled
    pub enabled: bool,

    /// Path to the audit log file
    pub log_file: PathBuf,

    /// Log level (all, commands_only, errors_only)
    pub log_level: AuditLogLevel,
}

impl Default for AuditConfig {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            enabled: true,
            log_file: home.join(".forge").join("chat-audit.jsonl"),
            log_level: AuditLogLevel::All,
        }
    }
}

/// Audit log level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditLogLevel {
    /// Log all commands and responses
    All,
    /// Log only commands (not responses)
    CommandsOnly,
    /// Log only errors
    ErrorsOnly,
}

/// Confirmation settings for destructive operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationConfig {
    /// Actions that require confirmation
    pub required_for: Vec<String>,

    /// Cost threshold for requiring confirmation (USD)
    pub high_cost_threshold: f64,

    /// Count threshold for bulk operations
    pub bulk_operation_threshold: u32,
}

impl Default for ConfirmationConfig {
    fn default() -> Self {
        Self {
            required_for: vec![
                "kill_worker".to_string(),
                "kill_all_workers".to_string(),
                "pause_workers".to_string(),
            ],
            high_cost_threshold: 10.0,
            bulk_operation_threshold: 5,
        }
    }
}

impl ChatConfig {
    /// Create a new config with custom model.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Create a new config with custom rate limit.
    pub fn with_rate_limit(mut self, max_per_minute: u32) -> Self {
        self.rate_limit.max_per_minute = max_per_minute;
        self
    }

    /// Create a new config with custom audit log path.
    pub fn with_audit_log(mut self, path: impl Into<PathBuf>) -> Self {
        self.audit.log_file = path.into();
        self
    }

    /// Disable audit logging.
    pub fn disable_audit(mut self) -> Self {
        self.audit.enabled = false;
        self
    }

    /// Set custom API base URL.
    pub fn with_api_base_url(mut self, url: impl Into<String>) -> Self {
        self.api_base_url = Some(url.into());
        self
    }
}

// Use dirs crate for home directory
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| std::env::var("USERPROFILE").ok().map(PathBuf::from))
    }
}
