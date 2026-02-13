//! Error types for the chat backend.

use thiserror::Error;

/// Chat backend errors.
#[derive(Debug, Error)]
pub enum ChatError {
    /// Rate limit exceeded
    #[error("Rate limit exceeded: {0} commands/minute. Try again in {1}s")]
    RateLimitExceeded(u32, u64),

    /// API request failed (transient, retryable)
    #[error("API request failed (transient): {0}")]
    ApiTransientError(String),

    /// API request failed (permanent)
    #[error("API request failed: {0}")]
    ApiError(String),

    /// Network timeout
    #[error("Network timeout after {0}s: {1}")]
    Timeout(u64, String),

    /// Connection failed
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Tool not found
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    /// Tool execution failed
    #[error("Tool execution failed: {0}")]
    ToolExecutionFailed(String),

    /// Action requires confirmation
    #[error("Action requires confirmation: {0}")]
    ConfirmationRequired(String),

    /// Action was cancelled by user
    #[error("Action cancelled by user")]
    ActionCancelled,

    /// Context gathering failed
    #[error("Failed to gather context: {0}")]
    ContextError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Audit logging failed
    #[error("Audit logging failed: {0}")]
    AuditError(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// JSON serialization error
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// HTTP request error
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Core error
    #[error("Core error: {0}")]
    CoreError(#[from] forge_core::ForgeError),
}

impl ChatError {
    /// Check if this error is retryable (transient network/API issues).
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ChatError::ApiTransientError(_)
                | ChatError::Timeout(_, _)
                | ChatError::ConnectionFailed(_)
                | ChatError::RateLimitExceeded(_, _)
        )
    }

    /// Check if this error is a network-related error.
    pub fn is_network_error(&self) -> bool {
        matches!(
            self,
            ChatError::ApiTransientError(_)
                | ChatError::Timeout(_, _)
                | ChatError::ConnectionFailed(_)
                | ChatError::HttpError(_)
        )
    }

    /// Check if this error is a rate limit error.
    pub fn is_rate_limit(&self) -> bool {
        matches!(self, ChatError::RateLimitExceeded(_, _))
    }

    /// Get a user-friendly error message.
    pub fn friendly_message(&self) -> String {
        match self {
            ChatError::RateLimitExceeded(limit, wait) => {
                format!(
                    "Too many requests ({}/min). Please wait {} seconds.",
                    limit, wait
                )
            }
            ChatError::ApiTransientError(msg) => {
                format!("Temporary API issue: {}. Please try again.", msg)
            }
            ChatError::Timeout(secs, _) => {
                format!("Request timed out after {}s. Check your connection.", secs)
            }
            ChatError::ConnectionFailed(msg) => {
                format!("Connection failed: {}. Check your network.", msg)
            }
            ChatError::ApiError(msg) => msg.clone(),
            ChatError::ConfigError(msg) => {
                format!("Configuration error: {}", msg)
            }
            _ => format!("Error: {}", self),
        }
    }

    /// Get suggested action for this error.
    pub fn suggested_action(&self) -> &'static str {
        match self {
            ChatError::RateLimitExceeded(_, _) => "Wait a moment before sending more commands.",
            ChatError::ApiTransientError(_) => "Try again in a few seconds.",
            ChatError::Timeout(_, _) => "Check your internet connection and try again.",
            ChatError::ConnectionFailed(_) => "Verify network connectivity and API availability.",
            ChatError::ConfigError(_) => "Check your configuration file at ~/.forge/config.yaml.",
            ChatError::ApiError(msg) if msg.contains("401") || msg.contains("unauthorized") => {
                "Check your API key configuration."
            }
            ChatError::HttpError(e) if e.is_timeout() => "Request timed out. Try again.",
            ChatError::HttpError(e) if e.is_connect() => "Could not connect. Check your network.",
            _ => "Try again or check the logs for details.",
        }
    }

    /// Classify an HTTP status code into appropriate error type.
    pub fn from_http_status(status: u16, body: &str) -> Self {
        match status {
            429 => {
                // Parse retry-after if available
                ChatError::RateLimitExceeded(10, 60) // Default values
            }
            408 => ChatError::Timeout(30, "Request timeout".to_string()),
            500 | 502 | 503 | 504 => {
                ChatError::ApiTransientError(format!("Server error ({}): {}", status, body))
            }
            401 | 403 => ChatError::ApiError(format!("Authentication error ({}): {}", status, body)),
            _ => ChatError::ApiError(format!("HTTP {}: {}", status, body)),
        }
    }
}

/// Result type for chat operations.
pub type Result<T> = std::result::Result<T, ChatError>;
