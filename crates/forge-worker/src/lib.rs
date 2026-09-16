//! Worker management for FORGE.
//!
//! This crate handles spawning, monitoring, and managing AI coding workers
//! running in tmux sessions or Docker containers.
//!
//! # Overview
//!
//! Workers are autonomous AI coding agents. This crate
//! provides the infrastructure to:
//!
//! - Spawn workers using configurable launcher scripts (tmux) or directly
//!   from a pinned image (Docker, see [`docker`])
//! - Track worker PIDs and session names
//! - Parse JSON output from launchers
//! - Manage worker lifecycle (start, stop, status check)
//! - **Discover active workers** from existing tmux sessions and Docker
//!   containers
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
//!     ┌──────┴───────┐
//!     ▼              ▼
//! ┌────────────┐  ┌──────────────────────┐
//! │  docker.rs │  │   Launcher Script    │
//! │ (container)│  │  (JSON output)       │
//! └─────┬──────┘  └──────────┬───────────┘
//!       │                    │
//!       ▼                    ▼
//! ┌────────────┐  ┌──────────────────────┐
//! │  Container │  │   tmux Session       │
//! │ (bind-mount│  │  (worker process)    │
//! │ workspace) │  └──────────────────────┘
//! └────────────┘
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
/// Cross-process bead-rs claims and fencing-token release support.
pub mod bead_claim;
pub mod bead_queue;
pub mod bead_scheduler;
pub mod complexity;
pub mod crash_recovery;
pub mod discovery;
pub mod docker;
pub mod health;
pub mod launcher;
#[cfg(test)]
mod lifecycle_tests;
pub mod memory;
pub mod pause;
pub mod pool;
pub mod response_time;
pub mod router;
pub mod scorer;
pub mod tmux;
pub mod types;

// Re-export main types for convenience
pub use auto_recovery::{
    AutoRecoveryManager, RecoveryAction, RecoveryActionType, RecoveryConfig, RecoveryPolicy,
};
pub use bead_claim::{
    BeadClaimBackend, ClaimOutcome, ClaimVerification, MemoryClaimStore, StoredClaim,
};
pub use bead_queue::{BeadAllocation, BeadQueueManager, BeadQueueReader, QueuedBead};
pub use bead_scheduler::{
    BeadScheduler, BeadStatusAction, BeadStatusBackend, BeadStatusUpdate, CompletionRecord,
    FORGE_BEAD_ID_ENV, FORGE_TASK_PROMPT_ENV, WorkerBeadAssignment, build_bead_prompt,
    priority_label,
};
pub use complexity::{
    CalibrationError, CalibrationReport, CalibrationResult, ComplexityCalibrationEvent,
    ComplexityCalibrationJob, ComplexityConfig, ComplexityScore, ComplexityScorer, ComplexityTier,
    DEFAULT_BUDGET_THRESHOLD, DEFAULT_CALIBRATION_INTERVAL_SECS, DEFAULT_CALIBRATION_MIN_SAMPLES,
    DEFAULT_STANDARD_THRESHOLD, TaskContext, ThresholdChange,
};
pub use crash_recovery::{
    CRASH_WINDOW_SECS, CrashAction, CrashRecord, CrashRecoveryConfig, CrashRecoveryManager,
    MAX_CRASHES_IN_WINDOW,
};
pub use discovery::{
    DiscoveredWorker, DiscoveryResult, WorkerType, discover_docker_workers, discover_workers,
};
pub use docker::{
    CONTAINER_NAME_PREFIX, ContainerState, ContainerSummary, DEFAULT_DOCKER_BIN, WORKER_LABEL,
    container_logs, list_worker_containers, state_to_worker_status, validate_image_ref,
};
pub use health::{
    DEFAULT_CHECK_INTERVAL_SECS, DEFAULT_MAX_RECOVERY_ATTEMPTS, DEFAULT_MEMORY_KILL_LIMIT_MB,
    DEFAULT_MEMORY_LIMIT_MB, DEFAULT_STALE_THRESHOLD_SECS, HealthCheckResult, HealthCheckType,
    HealthErrorType, HealthLevel, HealthMonitor, HealthMonitorConfig, WorkerHealthStatus,
};
pub use launcher::WorkerLauncher;
pub use memory::{MemoryConfig, MemoryMonitor, MemorySeverity, WorkerMemoryStats};
pub use pause::{
    DEFAULT_PAUSE_CHECK_INTERVAL_SECS, Pausable, PauseConfig, PauseSignalHandler, is_any_paused,
    pause_all, resume_all,
};
pub use pool::{
    LauncherPoolSpawner, PoolEvent, PoolRecoveryPolicy, PoolSpawner, PoolTierSummary, PoolWorker,
    PoolWorkerState, ProbeOutcome, WorkerPool, backoff_delay_secs,
};
pub use response_time::{
    DEFAULT_FAILURE_THRESHOLD, DEFAULT_PING_INTERVAL_SECS, DEFAULT_PING_TIMEOUT_MS, PingResult,
    ResponseTimeConfig, ResponseTimeTracker, WorkerResponseState,
};
pub use router::{
    FallbackOption, ModelAvailability, ModelConfig, ModelHealth, Router, RouterConfig, RouterError,
    RouterStats, RoutingDecision, RoutingReason, SubscriptionQuota, TaskMetadata,
};
pub use scorer::{ScoreComponents, ScoredBead, ScoringConfig, TaskScorer};
pub use types::{LaunchConfig, LauncherOutput, SpawnRequest, WorkerBackend, WorkerHandle};
