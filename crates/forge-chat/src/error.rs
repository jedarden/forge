//! Error types for the chat backend.

use thiserror::Error;

/// Chat backend errors.
#[derive(Debug, Error)]
pub enum ChatError {
    /// Rate limit exceeded
    #[error("Rate limit exceeded: {0} commands/minute. Try again in {1}s")]
    RateLimitExceeded(u32, u64),

    /// API request failed
    #[error("API request failed: {0}")]
    ApiError(String),

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

/// Result type for chat operations.
pub type Result<T> = std::result::Result<T, ChatError>;
