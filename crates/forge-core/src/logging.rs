//! Logging infrastructure for FORGE.
//!
//! This module provides structured logging using the `tracing` ecosystem.
//! Per ADR 0014, FORGE maintains its own logs separate from worker logs,
//! enabling developers to iterate on test FORGE instances in different terminals.
//!
//! ## Features
//!
//! - JSON lines format for machine parsing
//! - File output to `~/.forge/logs/forge.log`
//! - Console output with configurable verbosity
//! - `--debug` flag support for verbose logging
//!
//! ## Example
//!
//! ```no_run
//! use forge_core::logging;
//!
//! // Initialize logging (call once at startup)
//! let _guard = logging::init_logging(None, false).expect("logging init");
//!
//! // Use tracing macros
//! tracing::info!("FORGE started");
//! tracing::debug!(worker_id = "sonnet-alpha", "spawning worker");
//! ```

use std::path::PathBuf;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

use crate::error::{ForgeError, Result};

/// Guard that must be held to ensure log flushing on shutdown.
///
/// When this guard is dropped, it flushes any pending log entries.
/// Keep this guard alive for the lifetime of the application.
pub struct LogGuard {
    _file_guard: Option<WorkerGuard>,
}

/// Initialize the FORGE logging system.
///
/// This sets up:
/// - File logging to `~/.forge/logs/forge.log` (JSON lines format)
/// - Console logging to stderr (human-readable format)
///
/// # Arguments
///
/// * `log_dir` - Optional custom log directory. Defaults to `~/.forge/logs/`
/// * `verbose` - If true, sets log level to DEBUG. Otherwise uses INFO.
///
/// # Returns
///
/// A [`LogGuard`] that must be held for the application lifetime to ensure
/// logs are properly flushed on shutdown.
///
/// # Example
///
/// ```no_run
/// use forge_core::logging;
///
/// fn main() -> forge_core::Result<()> {
///     let _guard = logging::init_logging(None, false)?;
///     tracing::info!("Application started");
///     Ok(())
/// }
/// ```
pub fn init_logging(log_dir: Option<PathBuf>, verbose: bool) -> Result<LogGuard> {
    // Determine log directory
    let log_dir = match log_dir {
        Some(dir) => dir,
        None => default_log_dir()?,
    };

    // Ensure log directory exists
    std::fs::create_dir_all(&log_dir).map_err(|e| ForgeError::DirectoryCreation {
        path: log_dir.clone(),
        source: e,
    })?;

    // Set up file appender for JSON logs
    let file_appender = tracing_appender::rolling::daily(&log_dir, "forge.log");
    let (non_blocking_file, file_guard) = tracing_appender::non_blocking(file_appender);

    // Determine log level based on verbose flag and environment
    let default_level = if verbose { "debug" } else { "info" };
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("forge={default_level}")));

    // JSON layer for file output
    let file_layer = fmt::layer()
        .with_writer(non_blocking_file)
        .with_ansi(false)
        .json()
        .with_span_events(FmtSpan::CLOSE)
        .with_current_span(true)
        .with_span_list(true);

    // Human-readable layer for console output
    let console_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(verbose)
        .with_line_number(verbose)
        .compact();

    // Combine layers with filter
    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(console_layer)
        .init();

    tracing::debug!(log_dir = %log_dir.display(), verbose, "logging initialized");

    Ok(LogGuard {
        _file_guard: Some(file_guard),
    })
}

/// Initialize minimal console-only logging for testing.
///
/// This is a simpler alternative to [`init_logging`] that only logs to stderr.
/// Useful for tests and development.
pub fn init_test_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("debug"))
        .with_test_writer()
        .try_init();
}

/// Get the default log directory path.
///
/// Returns `~/.forge/logs/`
pub fn default_log_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| ForgeError::Internal {
        message: "HOME environment variable not set".into(),
    })?;

    Ok(PathBuf::from(home).join(".forge").join("logs"))
}

/// Get the default FORGE log file path.
///
/// Returns `~/.forge/logs/forge.log`
pub fn default_log_file() -> Result<PathBuf> {
    Ok(default_log_dir()?.join("forge.log"))
}

/// Convenience macro for logging worker-related events.
///
/// # Example
///
/// ```ignore
/// log_worker_event!("sonnet-alpha", "spawned");
/// log_worker_event!("sonnet-alpha", "failed", reason = "timeout");
/// ```
#[macro_export]
macro_rules! log_worker_event {
    ($worker_id:expr, $event:expr) => {
        tracing::info!(
            worker_id = $worker_id,
            event = $event,
            target: "forge::worker",
            "worker event"
        )
    };
    ($worker_id:expr, $event:expr, $($field:tt)*) => {
        tracing::info!(
            worker_id = $worker_id,
            event = $event,
            target: "forge::worker",
            $($field)*,
            "worker event"
        )
    };
}

/// Convenience macro for logging tool execution.
///
/// # Example
///
/// ```ignore
/// log_tool_call!("spawn_worker", success = true);
/// log_tool_call!("kill_worker", success = false, error = "not found");
/// ```
#[macro_export]
macro_rules! log_tool_call {
    ($tool_name:expr, $($field:tt)*) => {
        tracing::info!(
            tool = $tool_name,
            target: "forge::tool",
            $($field)*,
            "tool call"
        )
    };
}

/// Convenience macro for logging cost events.
///
/// # Example
///
/// ```ignore
/// log_cost_event!(worker_id = "sonnet-alpha", model = "sonnet", tokens = 1000, cost_usd = 0.003);
/// ```
#[macro_export]
macro_rules! log_cost_event {
    ($($field:tt)*) => {
        tracing::info!(
            target: "forge::cost",
            $($field)*,
            "cost event"
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_log_dir() {
        // Set HOME for test (unsafe in Rust 2024 due to potential data races)
        // SAFETY: We are in a test context and this is the only test modifying HOME
        unsafe { std::env::set_var("HOME", "/tmp/test-home") };
        let dir = default_log_dir().unwrap();
        assert_eq!(dir, PathBuf::from("/tmp/test-home/.forge/logs"));
    }

    #[test]
    fn test_default_log_file() {
        // SAFETY: We are in a test context
        unsafe { std::env::set_var("HOME", "/tmp/test-home") };
        let file = default_log_file().unwrap();
        assert_eq!(file, PathBuf::from("/tmp/test-home/.forge/logs/forge.log"));
    }

    #[test]
    fn test_init_test_logging() {
        // Should not panic
        init_test_logging();
    }
}
