//! Error types for the cost tracking module.

use thiserror::Error;

/// Cost tracking errors.
#[derive(Error, Debug)]
pub enum CostError {
    /// Database error
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// Database locked error (retryable)
    #[error("database is locked (retry {retry_count}/{max_retries}): {message}")]
    DatabaseLocked {
        /// Retry attempt number
        retry_count: u32,
        /// Maximum retries allowed
        max_retries: u32,
        /// Human-readable message
        message: String,
    },

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

    /// Configuration error
    #[error("configuration error: {0}")]
    Config(String),

    /// YAML parsing error
    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

impl CostError {
    /// Check if this error is retryable (e.g., database locked).
    pub fn is_retryable(&self) -> bool {
        match self {
            CostError::DatabaseLocked { .. } => true,
            CostError::Database(rusqlite::Error::SqliteFailure(e, _)) => {
                e.code == rusqlite::ErrorCode::DatabaseBusy
                    || e.code == rusqlite::ErrorCode::DatabaseLocked
            }
            _ => false,
        }
    }

    /// Check if this error indicates a database lock.
    pub fn is_database_locked(&self) -> bool {
        match self {
            CostError::DatabaseLocked { .. } => true,
            CostError::Database(rusqlite::Error::SqliteFailure(e, _)) => {
                e.code == rusqlite::ErrorCode::DatabaseBusy
                    || e.code == rusqlite::ErrorCode::DatabaseLocked
            }
            _ => false,
        }
    }

    /// Create a user-friendly message for this error.
    pub fn friendly_message(&self) -> String {
        match self {
            CostError::DatabaseLocked { retry_count, max_retries, .. } => {
                format!(
                    "Database is busy (attempt {}/{}). FORGE will automatically retry.",
                    retry_count, max_retries
                )
            }
            CostError::Database(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("locked") || msg.contains("busy") {
                    "Database is temporarily locked. Please try again.".to_string()
                } else {
                    format!("Database error: {}", e)
                }
            }
            CostError::Io(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("permission") {
                    "Permission denied. Check file permissions.".to_string()
                } else if msg.contains("not found") {
                    "File or directory not found.".to_string()
                } else {
                    format!("File system error: {}", e)
                }
            }
            _ => format!("Error: {}", self),
        }
    }
}

/// Check if a CostError indicates a database lock.
pub fn is_database_locked_error(error: &CostError) -> bool {
    match error {
        CostError::DatabaseLocked { .. } => true,
        CostError::Database(rusqlite::Error::SqliteFailure(e, _)) => {
            e.code == rusqlite::ErrorCode::DatabaseBusy
                || e.code == rusqlite::ErrorCode::DatabaseLocked
        }
        _ => false,
    }
}

/// Result type for cost tracking operations.
pub type Result<T> = std::result::Result<T, CostError>;
