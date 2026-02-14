//! # forge-core
//!
//! Core types, errors, and utilities for the FORGE orchestration system.
//!
//! This crate provides:
//! - [`ForgeError`] - Comprehensive error types for all FORGE operations
//! - [`logging`] - Tracing setup and log management utilities
//! - [`types`] - Shared type definitions used across FORGE crates
//! - [`status`] - Worker status file reading
//! - [`watcher`] - Real-time file watching for status updates
//!
//! ## Example
//!
//! ```no_run
//! use forge_core::{ForgeError, Result, logging};
//!
//! fn main() -> forge_core::Result<()> {
//!     // Initialize logging
//!     let _guard = logging::init_logging(None, false)?;
//!
//!     // Use FORGE errors
//!     let config_path = std::path::Path::new("~/.forge/config.yaml");
//!     if !config_path.exists() {
//!         return Err(ForgeError::config_not_found(config_path));
//!     }
//!
//!     Ok(())
//! }
//! ```

pub mod error;
pub mod logging;
pub mod recovery;
#[cfg(feature = "self-update")]
pub mod self_update;
pub mod status;
pub mod types;
pub mod watcher;
pub mod worker_perf;

// Re-export worker performance types
pub use worker_perf::{
    TaskEvent, TaskPerfMetrics, WorkerPerfSummary, WorkerPerfTracker,
};

// Re-export main types for convenience
pub use error::{ForgeError, Result};
pub use logging::{LogGuard, init_logging};
pub use recovery::{
    friendly_error_message, retry_with_backoff, retry_with_backoff_async, RecoveryAction,
    RetryConfig, RetryResult, Retryable,
};
pub use status::{StatusReader, WorkerStatusInfo};
pub use watcher::{StatusEvent, StatusWatcher, WatcherConfig};

// Re-export self_update types when feature is enabled
#[cfg(feature = "self-update")]
pub use self_update::{
    check_for_update, perform_update, DownloadProgress, UpdateResult, UpdateStatus,
};
