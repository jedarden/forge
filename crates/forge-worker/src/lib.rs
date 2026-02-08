//! Worker management for FORGE.
//!
//! This crate handles spawning, monitoring, and managing AI coding workers
//! running in tmux sessions.
//!
//! # Overview
//!
//! Workers are autonomous AI coding agents that run in tmux sessions. This crate
//! provides the infrastructure to:
//!
//! - Spawn workers using configurable launcher scripts
//! - Track worker PIDs and session names
//! - Parse JSON output from launchers
//! - Manage worker lifecycle (start, stop, status check)
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────┐
//! │   WorkerLauncher     │
//! │  (spawn, stop, list) │
//! └──────────┬───────────┘
//!            │
//!            ▼
//! ┌──────────────────────┐
//! │   Launcher Script    │
//! │  (JSON output)       │
//! └──────────┬───────────┘
//!            │
//!            ▼
//! ┌──────────────────────┐
//! │   tmux Session       │
//! │  (worker process)    │
//! └──────────────────────┘
//! ```
//!
//! # Example
//!
//! ```no_run
//! use forge_worker::{WorkerLauncher, LaunchConfig, SpawnRequest};
//! use forge_core::types::WorkerTier;
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> forge_core::Result<()> {
//!     let launcher = WorkerLauncher::new();
//!
//!     // Configure the worker launch
//!     let config = LaunchConfig::new(
//!         PathBuf::from("/path/to/launcher.sh"),
//!         "my-worker",
//!         PathBuf::from("/home/user/project"),
//!         "sonnet",
//!     )
//!     .with_tier(WorkerTier::Standard)
//!     .with_timeout(60);
//!
//!     // Spawn the worker
//!     let request = SpawnRequest::new("worker-1", config);
//!     let handle = launcher.spawn(request).await?;
//!
//!     println!("Worker spawned: {} (PID: {})", handle.id, handle.pid);
//!
//!     // Check status later
//!     let status = launcher.check_status(&handle.id).await?;
//!     println!("Worker status: {}", status);
//!
//!     // Stop when done
//!     launcher.stop(&handle.id).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Launcher Script Protocol
//!
//! Launcher scripts must:
//! 1. Accept a session name as the first argument
//! 2. Create a tmux session with that name
//! 3. Output JSON to stdout with worker information
//!
//! Expected JSON format:
//! ```json
//! {
//!     "pid": 12345,
//!     "session": "forge-worker-1",
//!     "model": "sonnet",
//!     "message": "Started successfully"
//! }
//! ```
//!
//! On error:
//! ```json
//! {
//!     "pid": 0,
//!     "session": "",
//!     "error": "Failed to start: API key not found"
//! }
//! ```
//!
//! Environment variables passed to launcher:
//! - `FORGE_WORKER_ID`: Unique worker identifier
//! - `FORGE_SESSION`: tmux session name
//! - `FORGE_MODEL`: Model to use
//! - `FORGE_WORKSPACE`: Working directory path

pub mod launcher;
pub mod tmux;
pub mod types;

// Re-export main types for convenience
pub use launcher::WorkerLauncher;
pub use types::{LaunchConfig, LauncherOutput, SpawnRequest, WorkerHandle};
