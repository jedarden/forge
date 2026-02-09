//! Error types for the cost tracking module.

use thiserror::Error;

/// Cost tracking errors.
#[derive(Error, Debug)]
pub enum CostError {
    /// Database error
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// JSON parsing error
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    /// IO error (file reading)
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Invalid log line format
    #[error("invalid log line format: {0}")]
    InvalidLogFormat(String),

    /// Migration error
    #[error("migration error: {0}")]
    Migration(String),

    /// Query error
    #[error("query error: {0}")]
    Query(String),
}

/// Result type for cost tracking operations.
pub type Result<T> = std::result::Result<T, CostError>;
