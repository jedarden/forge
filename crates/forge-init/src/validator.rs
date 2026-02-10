//! Configuration validation.
//!
//! Validates generated configuration by testing the chat backend connection.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during validation.
#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Config file not found: {0}")]
    ConfigNotFound(String),

    #[error("Chat backend test failed: {0}")]
    ChatBackendFailed(String),

    #[error("Launcher script not executable: {0}")]
    LauncherNotExecutable(String),
}

/// Result type for validation operations.
pub type Result<T> = std::result::Result<T, ValidationError>;

/// Validate config.yaml by testing the chat backend.
pub fn validate_config(_config_path: &PathBuf) -> Result<()> {
    // TODO: Implement in fg-ss3
    todo!()
}
