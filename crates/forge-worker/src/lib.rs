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
//! - **Discover active workers** from existing tmux sessions
//! - **Read bead queues** from workspaces for task allocation
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
//! # Session Discovery
//!
//! The [`discovery`] module provides utilities for discovering active worker sessions:
//!
//! ```no_run
//! use forge_worker::discovery::{discover_workers, WorkerType};
//!
//! #[tokio::main]
//! async fn main() -> forge_core::Result<()> {
//!     let result = discover_workers().await?;
//!
//!     println!("Found {} workers:", result.workers.len());
//!     for worker in &result.workers {
//!         println!("  {} ({}) - {}",
//!             worker.session_name,
//!             worker.worker_type,
//!             if worker.is_attached { "attached" } else { "detached" }
//!         );
//!     }
//!
//!     // Filter by type
//!     let opus_workers = result.workers_of_type(WorkerType::Opus);
//!     println!("Opus workers: {}", opus_workers.len());
//!
//!     Ok(())
//! }
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

pub mod auto_recovery;
pub mod bead_queue;
pub mod crash_recovery;
pub mod discovery;
pub mod health;
pub mod launcher;
#[cfg(test)]
mod lifecycle_tests;
pub mod memory;
pub mod router;
pub mod scorer;
pub mod tmux;
pub mod types;

// Re-export main types for convenience
pub use auto_recovery::{
    AutoRecoveryManager, RecoveryAction, RecoveryActionType, RecoveryConfig, RecoveryPolicy,
};
pub use bead_queue::{BeadAllocation, BeadQueueManager, BeadQueueReader, QueuedBead};
pub use crash_recovery::{
    CrashAction, CrashRecord, CrashRecoveryConfig, CrashRecoveryManager, CRASH_WINDOW_SECS,
    MAX_CRASHES_IN_WINDOW,
};
pub use discovery::{DiscoveredWorker, DiscoveryResult, WorkerType, discover_workers};
pub use health::{
    HealthCheckResult, HealthCheckType, HealthErrorType, HealthLevel, HealthMonitor,
    HealthMonitorConfig, WorkerHealthStatus, DEFAULT_CHECK_INTERVAL_SECS,
    DEFAULT_MAX_RECOVERY_ATTEMPTS, DEFAULT_MEMORY_KILL_LIMIT_MB, DEFAULT_MEMORY_LIMIT_MB,
    DEFAULT_STALE_THRESHOLD_SECS,
};
pub use memory::{MemoryConfig, MemoryMonitor, MemorySeverity, WorkerMemoryStats};
pub use launcher::WorkerLauncher;
pub use router::{
    FallbackOption, ModelAvailability, ModelConfig, ModelHealth, Router, RouterConfig,
    RouterError, RouterStats, RoutingDecision, RoutingReason, SubscriptionQuota, TaskMetadata,
};
pub use scorer::{ScoredBead, ScoreComponents, ScoringConfig, TaskScorer};
pub use types::{LaunchConfig, LauncherOutput, SpawnRequest, WorkerHandle};
