//! Worker types and data structures.
//!
//! This module defines the core types used for worker management,
//! including worker handles, launcher output, and process information.

use chrono::{DateTime, Utc};
use forge_core::types::{BeadId, WorkerId, WorkerStatus, WorkerTier};
use forge_cost::TaskAssignment;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The execution backend hosting a worker.
///
/// Re-exported from [`forge_core::types`] so handles, discovery results, and
/// launch configs across the workspace share one definition: FORGE workers
/// run either in a tmux session on the host (the default, driven by launcher
/// scripts) or in a Docker container managed directly by [`crate::docker`].
pub use forge_core::types::WorkerBackend;

/// Handle to a running worker process.
///
/// Contains all information needed to track and manage a worker.
#[derive(Debug, Clone)]
pub struct WorkerHandle {
    /// Unique worker identifier
    pub id: WorkerId,
    /// Process ID of the worker (or tmux server)
    pub pid: u32,
    /// Name of the tmux session
    pub session_name: String,
    /// Path to the launcher script used
    pub launcher_path: PathBuf,
    /// Model being used by this worker
    pub model: String,
    /// Worker tier classification
    pub tier: WorkerTier,
    /// Current worker status
    pub status: WorkerStatus,
    /// When the worker was started
    pub started_at: DateTime<Utc>,
    /// Working directory for the worker
    pub workspace: PathBuf,
    /// Optional bead ID if this worker is assigned to a specific bead
    pub bead_id: Option<BeadId>,
    /// Optional bead title for display
    pub bead_title: Option<String>,
    /// Backend hosting this worker (tmux session or Docker container)
    pub backend: WorkerBackend,
}

impl WorkerHandle {
    /// Create a new worker handle.
    pub fn new(
        id: impl Into<WorkerId>,
        pid: u32,
        session_name: impl Into<String>,
        launcher_path: impl Into<PathBuf>,
        model: impl Into<String>,
        tier: WorkerTier,
        workspace: impl Into<PathBuf>,
    ) -> Self {
        Self {
            id: id.into(),
            pid,
            session_name: session_name.into(),
            launcher_path: launcher_path.into(),
            model: model.into(),
            tier,
            status: WorkerStatus::Starting,
            started_at: Utc::now(),
            workspace: workspace.into(),
            bead_id: None,
            bead_title: None,
            backend: WorkerBackend::default(),
        }
    }

    /// Set the bead assignment for this worker.
    pub fn with_bead(mut self, bead_id: impl Into<BeadId>, bead_title: impl Into<String>) -> Self {
        self.bead_id = Some(bead_id.into());
        self.bead_title = Some(bead_title.into());
        self
    }

    /// Set the backend hosting this worker.
    pub fn with_backend(mut self, backend: WorkerBackend) -> Self {
        self.backend = backend;
        self
    }

    /// Check if the worker is still running.
    pub fn is_running(&self) -> bool {
        self.status.is_healthy()
    }

    /// Check if this worker is assigned to a bead.
    pub fn has_bead(&self) -> bool {
        self.bead_id.is_some()
    }

    /// Get the session name for tmux commands.
    pub fn tmux_session(&self) -> &str {
        &self.session_name
    }
}

/// Output from a launcher script (parsed from JSON).
///
/// Launchers emit JSON to stdout with worker information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherOutput {
    /// Process ID of the spawned worker
    pub pid: u32,
    /// Name of the tmux session created
    pub session: String,
    /// Model identifier being used
    #[serde(default)]
    pub model: String,
    /// Optional status message
    #[serde(default)]
    pub message: Option<String>,
    /// Optional error if launch failed
    #[serde(default)]
    pub error: Option<String>,
    /// Optional bead ID if this worker is assigned to a bead
    #[serde(default)]
    pub bead_id: Option<String>,
    /// Optional bead title for display
    #[serde(default)]
    pub bead_title: Option<String>,
}

impl LauncherOutput {
    /// Check if the launcher output indicates success.
    pub fn is_success(&self) -> bool {
        self.error.is_none() && self.pid > 0
    }
}

/// Configuration for launching a worker.
#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// Path to the launcher script
    pub launcher_path: PathBuf,
    /// Session name for tmux
    pub session_name: String,
    /// Working directory for the worker
    pub workspace: PathBuf,
    /// Model to use
    pub model: String,
    /// Worker tier
    pub tier: WorkerTier,
    /// Environment variables to set
    pub env: Vec<(String, String)>,
    /// Timeout for launcher in seconds
    pub timeout_secs: u64,
    /// Optional bead ID to assign this worker to
    pub bead_id: Option<BeadId>,
    /// Execution backend (tmux launcher script by default, or Docker)
    pub backend: WorkerBackend,
    /// Docker image reference for the Docker backend. Must be pinned to an
    /// explicit tag or digest — validated at spawn time.
    pub image: Option<String>,
    /// Command to run inside a Docker container. When unset, the container
    /// stays alive with a keepalive command so the orchestrator manages its
    /// lifecycle.
    pub container_command: Option<String>,
}

impl LaunchConfig {
    /// Create a new launch configuration with defaults.
    pub fn new(
        launcher_path: impl Into<PathBuf>,
        session_name: impl Into<String>,
        workspace: impl Into<PathBuf>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            launcher_path: launcher_path.into(),
            session_name: session_name.into(),
            workspace: workspace.into(),
            model: model.into(),
            tier: WorkerTier::Standard,
            env: Vec::new(),
            timeout_secs: 30,
            bead_id: None,
            backend: WorkerBackend::default(),
            image: None,
            container_command: None,
        }
    }

    /// Set the worker tier.
    pub fn with_tier(mut self, tier: WorkerTier) -> Self {
        self.tier = tier;
        self
    }

    /// Add an environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Set the timeout.
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    /// Set the bead assignment.
    pub fn with_bead(mut self, bead_id: impl Into<BeadId>) -> Self {
        self.bead_id = Some(bead_id.into());
        self
    }

    /// Set the execution backend.
    pub fn with_backend(mut self, backend: WorkerBackend) -> Self {
        self.backend = backend;
        self
    }

    /// Set the pinned Docker image for the Docker backend.
    pub fn with_docker_image(mut self, image: impl Into<String>) -> Self {
        self.image = Some(image.into());
        self
    }

    /// Set the command to run inside a Docker container.
    pub fn with_container_command(mut self, command: impl Into<String>) -> Self {
        self.container_command = Some(command.into());
        self
    }

    /// Check if this launch config has a bead assignment.
    pub fn has_bead(&self) -> bool {
        self.bead_id.is_some()
    }

    /// Check if this config targets the Docker backend.
    pub fn is_docker(&self) -> bool {
        self.backend.is_docker()
    }
}

/// Worker spawn request with all necessary information.
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    /// Unique ID for this worker
    pub worker_id: WorkerId,
    /// Launch configuration
    pub config: LaunchConfig,
    /// Complexity prediction to persist before launching, if this is a bead task.
    pub task_assignment: Option<TaskAssignment>,
}

impl SpawnRequest {
    /// Create a new spawn request.
    pub fn new(worker_id: impl Into<WorkerId>, config: LaunchConfig) -> Self {
        Self {
            worker_id: worker_id.into(),
            config,
            task_assignment: None,
        }
    }

    /// Attach the complexity prediction that led to this spawn request.
    pub fn with_task_assignment(mut self, assignment: TaskAssignment) -> Self {
        self.task_assignment = Some(assignment);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_handle_creation() {
        let handle = WorkerHandle::new(
            "worker-1",
            12345,
            "forge-worker-1",
            "/path/to/launcher.sh",
            "sonnet",
            WorkerTier::Standard,
            "/home/user/project",
        );

        assert_eq!(handle.id, "worker-1");
        assert_eq!(handle.pid, 12345);
        assert_eq!(handle.session_name, "forge-worker-1");
        assert_eq!(handle.status, WorkerStatus::Starting);
        assert!(handle.is_running());
    }

    #[test]
    fn test_launcher_output_success() {
        let output = LauncherOutput {
            pid: 12345,
            session: "forge-test".into(),
            model: "sonnet".into(),
            message: Some("Started successfully".into()),
            error: None,
            bead_id: None,
            bead_title: None,
        };

        assert!(output.is_success());
    }

    #[test]
    fn test_launcher_output_failure() {
        let output = LauncherOutput {
            pid: 0,
            session: String::new(),
            model: String::new(),
            message: None,
            error: Some("Failed to start".into()),
            bead_id: None,
            bead_title: None,
        };

        assert!(!output.is_success());
    }

    #[test]
    fn test_launch_config_builder() {
        let config =
            LaunchConfig::new("/path/to/launcher.sh", "test-session", "/workspace", "opus")
                .with_tier(WorkerTier::Premium)
                .with_env("FORGE_DEBUG", "1")
                .with_timeout(60);

        assert_eq!(config.tier, WorkerTier::Premium);
        assert_eq!(config.timeout_secs, 60);
        assert_eq!(config.env.len(), 1);
        assert_eq!(config.backend, WorkerBackend::Tmux);
        assert!(!config.is_docker());
    }

    #[test]
    fn test_worker_backend_default_and_display() {
        assert_eq!(WorkerBackend::default(), WorkerBackend::Tmux);
        assert_eq!(WorkerBackend::Tmux.to_string(), "tmux");
        assert_eq!(WorkerBackend::Docker.to_string(), "docker");
        assert!(WorkerBackend::Docker.is_docker());
        assert!(!WorkerBackend::Tmux.is_docker());
    }

    #[test]
    fn test_docker_launch_config_builder() {
        let config = LaunchConfig::new("/unused.sh", "test-session", "/workspace", "sonnet")
            .with_backend(WorkerBackend::Docker)
            .with_docker_image("example/agent:1.2.3")
            .with_container_command("sleep infinity");

        assert!(config.is_docker());
        assert_eq!(config.image.as_deref(), Some("example/agent:1.2.3"));
        assert_eq!(config.container_command.as_deref(), Some("sleep infinity"));
    }

    #[test]
    fn test_worker_handle_backend_defaults_to_tmux() {
        let handle = WorkerHandle::new(
            "worker-1",
            1,
            "forge-worker-1",
            "/path/to/launcher.sh",
            "sonnet",
            WorkerTier::Standard,
            "/home/user/project",
        )
        .with_backend(WorkerBackend::Docker);

        assert_eq!(handle.backend, WorkerBackend::Docker);

        let handle = WorkerHandle::new(
            "worker-2",
            2,
            "forge-worker-2",
            "/path/to/launcher.sh",
            "sonnet",
            WorkerTier::Standard,
            "/home/user/project",
        );
        assert_eq!(handle.backend, WorkerBackend::Tmux);
    }
}
