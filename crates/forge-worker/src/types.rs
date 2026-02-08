//! Worker types and data structures.
//!
//! This module defines the core types used for worker management,
//! including worker handles, launcher output, and process information.

use chrono::{DateTime, Utc};
use forge_core::types::{BeadId, WorkerId, WorkerStatus, WorkerTier};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
        }
    }

    /// Set the bead assignment for this worker.
    pub fn with_bead(mut self, bead_id: impl Into<BeadId>, bead_title: impl Into<String>) -> Self {
        self.bead_id = Some(bead_id.into());
        self.bead_title = Some(bead_title.into());
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

    /// Check if this launch config has a bead assignment.
    pub fn has_bead(&self) -> bool {
        self.bead_id.is_some()
    }
}

/// Worker spawn request with all necessary information.
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    /// Unique ID for this worker
    pub worker_id: WorkerId,
    /// Launch configuration
    pub config: LaunchConfig,
}

impl SpawnRequest {
    /// Create a new spawn request.
    pub fn new(worker_id: impl Into<WorkerId>, config: LaunchConfig) -> Self {
        Self {
            worker_id: worker_id.into(),
            config,
        }
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
        let config = LaunchConfig::new(
            "/path/to/launcher.sh",
            "test-session",
            "/workspace",
            "opus",
        )
        .with_tier(WorkerTier::Premium)
        .with_env("FORGE_DEBUG", "1")
        .with_timeout(60);

        assert_eq!(config.tier, WorkerTier::Premium);
        assert_eq!(config.timeout_secs, 60);
        assert_eq!(config.env.len(), 1);
    }
}
