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
pub mod status;
pub mod types;
pub mod watcher;

// Re-export main types for convenience
pub use error::{ForgeError, Result};
pub use logging::{LogGuard, init_logging};
pub use status::{StatusReader, WorkerStatusInfo};
pub use watcher::{StatusEvent, StatusWatcher, WatcherConfig};
