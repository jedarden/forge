//! Worker spawner abstraction for chat tools.

use async_trait::async_trait;
use forge_core::types::WorkerTier;
use forge_worker::{LaunchConfig, SpawnRequest, WorkerLauncher};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{error, info};

/// Result from spawning a worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnResult {
    /// Unique worker ID.
    pub worker_id: String,
    /// Tmux session name.
    pub session_name: String,
    /// Process ID.
    pub pid: u32,
    /// Worker type/model.
    pub worker_type: String,
    /// Workspace path.
    pub workspace: PathBuf,
}

/// Trait for spawning workers.
///
/// This allows chat tools to spawn workers without depending directly
/// on the WorkerLauncher implementation.
#[async_trait]
pub trait WorkerSpawner: Send + Sync {
    /// Spawn a new worker.
    ///
    /// # Arguments
    /// * `worker_type` - The model/type (e.g., "glm", "sonnet", "opus")
    /// * `count` - Number of workers to spawn
    /// * `workspace` - Optional workspace path
    ///
    /// # Returns
    /// Vector of spawn results, one per worker.
    async fn spawn_workers(
        &self,
        worker_type: &str,
        count: usize,
        workspace: Option<&PathBuf>,
    ) -> Result<Vec<SpawnResult>, String>;

    /// Check if the spawner is available.
    fn is_available(&self) -> bool {
        true
    }
}

/// No-op worker spawner that returns errors.
///
/// Used when no real spawner is configured (e.g., in tests or
/// when the chat backend is used without the TUI).
#[derive(Debug, Clone)]
pub struct NoOpWorkerSpawner;

#[async_trait]
impl WorkerSpawner for NoOpWorkerSpawner {
    async fn spawn_workers(
        &self,
        worker_type: &str,
        _count: usize,
        _workspace: Option<&PathBuf>,
    ) -> Result<Vec<SpawnResult>, String> {
        Err(format!(
            "Worker spawning not available: NoOpWorkerSpawner configured. \
            Please use the FORGE TUI to spawn {} workers.",
            worker_type
        ))
    }

    fn is_available(&self) -> bool {
        false
    }
}

/// Real worker spawner that uses WorkerLauncher.
///
/// This wraps the forge-worker::WorkerLauncher and provides
/// the WorkerSpawner trait interface.
#[derive(Debug)]
pub struct RealWorkerSpawner {
    /// Worker launcher from forge-worker crate.
    launcher: WorkerLauncher,
    /// Launcher script path.
    launcher_path: PathBuf,
    /// Workspace path.
    workspace: PathBuf,
}

impl RealWorkerSpawner {
    /// Create a new real worker spawner.
    ///
    /// # Arguments
    /// * `launcher_path` - Path to the launcher script
    /// * `workspace` - Workspace path
    pub fn new(launcher_path: PathBuf, workspace: PathBuf) -> Self {
        Self {
            launcher: WorkerLauncher::new(),
            launcher_path,
            workspace,
        }
    }

    /// Find the default launcher script.
    ///
    /// Searches in order:
    /// 1. $FORGE_SRC/scripts/launchers/bead-worker-launcher.sh
    /// 2. $HOME/.forge/launchers/bead-worker-launcher.sh
    /// 3. $FORGE_SRC/test/example-launchers/claude-code-launcher.sh
    pub fn find_launcher() -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        let forge_src = std::env::var("FORGE_SRC")
            .unwrap_or_else(|_| format!("{}/forge", home));

        let paths = vec![
            PathBuf::from(&forge_src).join("scripts/launchers/bead-worker-launcher.sh"),
            PathBuf::from(&home).join(".forge/launchers/bead-worker-launcher.sh"),
            PathBuf::from(&forge_src).join("test/example-launchers/claude-code-launcher.sh"),
        ];

        paths.into_iter().find(|p| p.exists())
    }

    /// Create with default launcher detection.
    ///
    /// Returns None if no launcher script can be found.
    pub fn with_default_launcher(workspace: PathBuf) -> Option<Self> {
        let launcher_path = Self::find_launcher()?;
        Some(Self::new(launcher_path, workspace))
    }

    /// Generate a unique worker ID.
    fn generate_worker_id(&self, model: &str) -> String {
        use chrono::Local;

        let timestamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
        let random = rand::random::<u16>();
        format!("{}-{}-{:04}", model, timestamp, random)
    }

    /// Map worker type to model string and tier.
    fn map_worker_type(&self, worker_type: &str) -> (String, WorkerTier) {
        match worker_type {
            "glm" => ("glm-4.7".to_string(), WorkerTier::Budget),
            "sonnet" => ("sonnet".to_string(), WorkerTier::Standard),
            "opus" => ("opus".to_string(), WorkerTier::Premium),
            "haiku" => ("haiku".to_string(), WorkerTier::Budget),
            _ => (worker_type.to_string(), WorkerTier::Standard),
        }
    }

    /// Spawn a single worker.
    async fn spawn_single(&self, worker_type: &str) -> Result<SpawnResult, String> {
        let (model, tier) = self.map_worker_type(worker_type);

        // Generate unique IDs
        let worker_id = self.generate_worker_id(&model);
        let session_name = worker_id.clone(); // WorkerLauncher adds prefix
        let workspace = self.workspace.clone();

        info!(
            "Spawning {} worker: id={}, session={}, workspace={}",
            model,
            worker_id,
            session_name,
            workspace.display()
        );

        // Create launch config
        let config = LaunchConfig::new(&self.launcher_path, &session_name, &workspace, &model)
            .with_tier(tier)
            .with_timeout(60);

        // Create spawn request
        let request = SpawnRequest::new(&worker_id, config);

        // Spawn the worker using WorkerLauncher
        match self.launcher.spawn(request).await {
            Ok(handle) => {
                info!(
                    "Worker spawned successfully: {} (PID: {}, session: {})",
                    worker_id, handle.pid, handle.session_name
                );

                Ok(SpawnResult {
                    worker_id,
                    session_name: handle.session_name,
                    pid: handle.pid,
                    worker_type: worker_type.to_string(),
                    workspace,
                })
            }
            Err(e) => {
                error!("Failed to spawn worker {}: {}", worker_id, e);
                Err(format!("Failed to spawn worker: {}", e))
            }
        }
    }
}

#[async_trait]
impl WorkerSpawner for RealWorkerSpawner {
    async fn spawn_workers(
        &self,
        worker_type: &str,
        count: usize,
        _workspace: Option<&PathBuf>,
    ) -> Result<Vec<SpawnResult>, String> {
        let mut results = Vec::new();

        for i in 0..count {
            match self.spawn_single(worker_type).await {
                Ok(result) => results.push(result),
                Err(e) => {
                    // If we spawned some workers successfully, return those
                    if !results.is_empty() {
                        error!(
                            "Failed to spawn worker {}/{}: {}",
                            i + 1,
                            count,
                            e
                        );
                        break;
                    }
                    return Err(e);
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_worker_id() {
        let spawner = RealWorkerSpawner::new(
            PathBuf::from("/tmp/test-launcher.sh"),
            PathBuf::from("/tmp/workspace"),
        );

        let id1 = spawner.generate_worker_id("sonnet");
        let id2 = spawner.generate_worker_id("sonnet");

        // IDs should be unique
        assert_ne!(id1, id2);

        // IDs should contain the model name
        assert!(id1.starts_with("sonnet-"));
    }

    #[test]
    fn test_map_worker_type() {
        let spawner = RealWorkerSpawner::new(
            PathBuf::from("/tmp/test-launcher.sh"),
            PathBuf::from("/tmp/workspace"),
        );

        let (model, tier) = spawner.map_worker_type("glm");
        assert_eq!(model, "glm-4.7");
        assert_eq!(tier, WorkerTier::Budget);

        let (model, tier) = spawner.map_worker_type("sonnet");
        assert_eq!(model, "sonnet");
        assert_eq!(tier, WorkerTier::Standard);

        let (model, tier) = spawner.map_worker_type("opus");
        assert_eq!(model, "opus");
        assert_eq!(tier, WorkerTier::Premium);
    }

    #[test]
    fn test_noop_spawner_unavailable() {
        let spawner = NoOpWorkerSpawner;
        assert!(!spawner.is_available());
    }

    #[tokio::test]
    async fn test_noop_spawner_returns_error() {
        let spawner = NoOpWorkerSpawner;
        let result = spawner
            .spawn_workers("sonnet", 1, None)
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("not available"));
        assert!(err.contains("sonnet"));
    }
}
