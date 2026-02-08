//! Error types for FORGE operations.
//!
//! This module defines [`ForgeError`], a comprehensive error enum that covers
//! all error cases across the FORGE system. Following ADR 0014, errors are
//! designed for visibility - no silent failures, clear actionable messages.

use std::path::PathBuf;
use thiserror::Error;

/// Result type alias using [`ForgeError`].
pub type Result<T> = std::result::Result<T, ForgeError>;

/// Comprehensive error type for all FORGE operations.
///
/// Following ADR 0014 (Error Handling Strategy):
/// - No automatic retry - user decides if/when to retry
/// - No silent failures - every error surfaced to user
/// - Clear actionable messages with guidance
#[derive(Debug, Error)]
pub enum ForgeError {
    // =========================================================================
    // Configuration Errors
    // =========================================================================
    /// Configuration file not found
    #[error("Configuration not found at {path}")]
    ConfigNotFound {
        path: PathBuf,
        #[source]
        source: Option<std::io::Error>,
    },

    /// Configuration file is invalid YAML
    #[error("Invalid configuration at {path}: {message}")]
    ConfigInvalid { path: PathBuf, message: String },

    /// Configuration validation failed
    #[error("Configuration validation failed: {message}")]
    ConfigValidation { message: String },

    /// Missing required configuration field
    #[error("Missing required config field: {field}")]
    ConfigMissingField { field: String },

    // =========================================================================
    // I/O Errors
    // =========================================================================
    /// Generic I/O error with context
    #[error("I/O error {operation}: {path}")]
    Io {
        operation: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// File not found
    #[error("File not found: {path}")]
    FileNotFound { path: PathBuf },

    /// Workspace directory not found
    #[error("Workspace not found: {path}")]
    WorkspaceNotFound { path: PathBuf },

    /// Permission denied
    #[error("Permission denied: {path}")]
    PermissionDenied { path: PathBuf },

    /// Directory creation failed
    #[error("Failed to create directory: {path}")]
    DirectoryCreation {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    // =========================================================================
    // Parsing Errors
    // =========================================================================
    /// JSON parsing error
    #[error("JSON parse error in {context}: {message}")]
    JsonParse {
        context: String,
        message: String,
        #[source]
        source: Option<serde_json::Error>,
    },

    /// YAML parsing error
    #[error("YAML parse error in {context}: {message}")]
    YamlParse { context: String, message: String },

    /// Log entry parsing error (non-fatal, per ADR 0014 we skip malformed entries)
    #[error("Malformed log entry: {message}")]
    LogParse { message: String, line_number: usize },

    /// Status file parsing error
    #[error("Invalid status file {path}: {message}")]
    StatusFileParse { path: PathBuf, message: String },

    // =========================================================================
    // Worker Errors
    // =========================================================================
    /// Worker not found
    #[error("Worker not found: {worker_id}")]
    WorkerNotFound { worker_id: String },

    /// Worker spawn failed
    #[error("Failed to spawn worker {worker_id}: {message}")]
    WorkerSpawn { worker_id: String, message: String },

    /// Worker process exited unexpectedly
    #[error("Worker {worker_id} exited with code {exit_code:?}")]
    WorkerExit {
        worker_id: String,
        exit_code: Option<i32>,
    },

    /// Worker health check failed
    #[error("Worker {worker_id} health check failed: {reason}")]
    WorkerHealth { worker_id: String, reason: String },

    /// Worker status update failed
    #[error("Failed to update worker {worker_id} status: {message}")]
    WorkerStatusUpdate { worker_id: String, message: String },

    // =========================================================================
    // Launcher Errors
    // =========================================================================
    /// Launcher not found
    #[error("Launcher not found: {path}")]
    LauncherNotFound { path: PathBuf },

    /// Launcher not executable
    #[error("Launcher not executable: {path}")]
    LauncherNotExecutable { path: PathBuf },

    /// Launcher execution failed
    #[error("Launcher failed for {model}: {message}")]
    LauncherExecution { model: String, message: String },

    /// Launcher returned invalid output
    #[error("Launcher returned invalid output: {message}")]
    LauncherOutput { message: String },

    /// Launcher timeout
    #[error("Launcher timed out after {timeout_secs}s")]
    LauncherTimeout { timeout_secs: u64 },

    // =========================================================================
    // Backend Errors (Chat Backend)
    // =========================================================================
    /// Backend not configured
    #[error("No chat backend configured")]
    BackendNotConfigured,

    /// Backend process failed to start
    #[error("Failed to start chat backend: {message}")]
    BackendStart { message: String },

    /// Backend communication error
    #[error("Backend communication error: {message}")]
    BackendCommunication { message: String },

    /// Backend returned invalid response
    #[error("Invalid backend response: {message}")]
    BackendResponse { message: String },

    /// Backend timeout
    #[error("Backend timed out after {timeout_secs}s")]
    BackendTimeout { timeout_secs: u64 },

    // =========================================================================
    // Database Errors
    // =========================================================================
    /// Database connection failed
    #[error("Database connection failed: {path}")]
    DatabaseConnection {
        path: PathBuf,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Database query failed
    #[error("Database query failed: {message}")]
    DatabaseQuery { message: String },

    /// Database migration failed
    #[error("Database migration failed: {message}")]
    DatabaseMigration { message: String },

    // =========================================================================
    // Bead/Task Errors
    // =========================================================================
    /// Bead not found
    #[error("Bead not found: {bead_id}")]
    BeadNotFound { bead_id: String },

    /// Bead JSONL file not found
    #[error("Bead file not found: {path}")]
    BeadFileNotFound { path: PathBuf },

    /// Invalid bead entry
    #[error("Invalid bead entry in {path}: {message}")]
    BeadInvalid { path: PathBuf, message: String },

    /// Bead dependency cycle detected
    #[error("Dependency cycle detected: {cycle}")]
    BeadCycle { cycle: String },

    // =========================================================================
    // Tool Errors
    // =========================================================================
    /// Unknown tool
    #[error("Unknown tool: {tool_name}")]
    ToolUnknown { tool_name: String },

    /// Tool validation failed (missing/invalid parameters)
    #[error("Tool validation failed for {tool_name}: {message}")]
    ToolValidation { tool_name: String, message: String },

    /// Tool execution failed
    #[error("Tool execution failed for {tool_name}: {message}")]
    ToolExecution { tool_name: String, message: String },

    /// Tool rate limited
    #[error("Tool {tool_name} rate limited: try again in {retry_after_secs}s")]
    ToolRateLimited {
        tool_name: String,
        retry_after_secs: u64,
    },

    // =========================================================================
    // File Watching Errors
    // =========================================================================
    /// File watcher initialization failed
    #[error("Failed to initialize file watcher: {message}")]
    WatcherInit { message: String },

    /// File watcher error
    #[error("File watcher error: {message}")]
    WatcherError { message: String },

    // =========================================================================
    // TUI Errors
    // =========================================================================
    /// Terminal initialization failed
    #[error("Terminal initialization failed: {message}")]
    TerminalInit { message: String },

    /// Terminal restore failed
    #[error("Failed to restore terminal: {message}")]
    TerminalRestore { message: String },

    // =========================================================================
    // Internal Errors
    // =========================================================================
    /// Internal error (bug in FORGE)
    #[error("Internal error: {message}")]
    Internal { message: String },

    /// Feature not implemented
    #[error("Not implemented: {feature}")]
    NotImplemented { feature: String },
}

impl ForgeError {
    // =========================================================================
    // Constructor helpers for common error patterns
    // =========================================================================

    /// Create a ConfigNotFound error
    pub fn config_not_found(path: impl Into<PathBuf>) -> Self {
        Self::ConfigNotFound {
            path: path.into(),
            source: None,
        }
    }

    /// Create a ConfigNotFound error with source
    pub fn config_not_found_with_source(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::ConfigNotFound {
            path: path.into(),
            source: Some(source),
        }
    }

    /// Create an I/O error
    pub fn io(operation: impl Into<String>, path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            operation: operation.into(),
            path: path.into(),
            source,
        }
    }

    /// Create a JSON parse error
    pub fn json_parse(context: impl Into<String>, source: serde_json::Error) -> Self {
        Self::JsonParse {
            context: context.into(),
            message: source.to_string(),
            source: Some(source),
        }
    }

    /// Create a generic parse error
    pub fn parse(message: impl Into<String>) -> Self {
        Self::Internal {
            message: format!("Parse error: {}", message.into()),
        }
    }

    /// Create a worker spawn error
    pub fn worker_spawn(worker_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::WorkerSpawn {
            worker_id: worker_id.into(),
            message: message.into(),
        }
    }

    /// Create a launcher not found error
    pub fn launcher_not_found(path: impl Into<PathBuf>) -> Self {
        Self::LauncherNotFound { path: path.into() }
    }

    /// Create an internal error
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    // =========================================================================
    // Error classification helpers
    // =========================================================================

    /// Returns true if this error is recoverable (user can retry)
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::LauncherTimeout { .. }
                | Self::BackendTimeout { .. }
                | Self::ToolRateLimited { .. }
                | Self::WatcherError { .. }
        )
    }

    /// Returns true if this error is fatal (should exit application)
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            Self::TerminalInit { .. } | Self::Internal { .. } | Self::DatabaseMigration { .. }
        )
    }

    /// Returns true if this is a configuration error
    pub fn is_config_error(&self) -> bool {
        matches!(
            self,
            Self::ConfigNotFound { .. }
                | Self::ConfigInvalid { .. }
                | Self::ConfigValidation { .. }
                | Self::ConfigMissingField { .. }
        )
    }

    /// Returns true if this is a worker-related error
    pub fn is_worker_error(&self) -> bool {
        matches!(
            self,
            Self::WorkerNotFound { .. }
                | Self::WorkerSpawn { .. }
                | Self::WorkerExit { .. }
                | Self::WorkerHealth { .. }
                | Self::WorkerStatusUpdate { .. }
        )
    }

    /// Returns actionable guidance for the user
    pub fn guidance(&self) -> Option<&'static str> {
        match self {
            Self::ConfigNotFound { .. } => Some("Run 'forge init' to create a configuration file"),
            Self::ConfigInvalid { .. } => {
                Some("Check YAML syntax - try 'forge validate' to see detailed errors")
            }
            Self::LauncherNotFound { .. } => {
                Some("Check that the launcher path is correct in ~/.forge/config.yaml")
            }
            Self::LauncherNotExecutable { .. } => Some("Run 'chmod +x' on the launcher script"),
            Self::BackendNotConfigured => {
                Some("Configure a chat backend in ~/.forge/config.yaml or run 'forge init'")
            }
            Self::WorkerHealth { .. } => {
                Some("Check worker logs in ~/.forge/logs/ for details")
            }
            Self::ToolRateLimited { .. } => Some("Wait and try again"),
            Self::TerminalInit { .. } => Some("Try running in a different terminal"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_not_found_error() {
        let err = ForgeError::config_not_found("/home/user/.forge/config.yaml");
        assert!(err.to_string().contains("Configuration not found"));
        assert!(err.is_config_error());
        assert!(!err.is_fatal());
        assert!(err.guidance().is_some());
    }

    #[test]
    fn test_worker_spawn_error() {
        let err = ForgeError::worker_spawn("sonnet-alpha", "tmux session failed");
        assert!(err.to_string().contains("sonnet-alpha"));
        assert!(err.is_worker_error());
    }

    #[test]
    fn test_error_classification() {
        assert!(ForgeError::LauncherTimeout { timeout_secs: 30 }.is_recoverable());
        assert!(ForgeError::Internal {
            message: "bug".into()
        }
        .is_fatal());
    }

    #[test]
    fn test_error_guidance() {
        let err = ForgeError::LauncherNotExecutable {
            path: "/some/launcher".into(),
        };
        assert_eq!(err.guidance(), Some("Run 'chmod +x' on the launcher script"));
    }
}
