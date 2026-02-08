//! # forge-core
//!
//! Core types, errors, and utilities for the FORGE orchestration system.
//!
//! This crate provides:
//! - [`ForgeError`] - Comprehensive error types for all FORGE operations
//! - [`logging`] - Tracing setup and log management utilities
//! - [`types`] - Shared type definitions used across FORGE crates
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
pub mod types;

// Re-export main types for convenience
pub use error::{ForgeError, Result};
pub use logging::{init_logging, LogGuard};
