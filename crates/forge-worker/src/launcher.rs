//! Worker launcher implementation using tokio::process.
//!
//! This module provides the [`WorkerLauncher`] type for spawning worker processes
//! in tmux sessions using configurable launcher scripts.

use crate::tmux;
use crate::types::{LaunchConfig, LauncherOutput, SpawnRequest, WorkerHandle};
use forge_core::{ForgeError, Result};
use forge_core::types::WorkerStatus;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio::time::timeout;
use tracing::{debug, error, info, instrument, warn};

/// Worker launcher for spawning and managing worker processes.
///
/// The launcher uses external launcher scripts to spawn workers in tmux sessions.
/// Launcher scripts must output JSON to stdout with worker information.
#[derive(Debug)]
pub struct WorkerLauncher {
    /// Active worker handles keyed by worker ID
    workers: Arc<RwLock<HashMap<String, WorkerHandle>>>,
    /// Session name prefix for all workers
    session_prefix: String,
}

impl Default for WorkerLauncher {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerLauncher {
    /// Create a new worker launcher.
    pub fn new() -> Self {
        Self {
            workers: Arc::new(RwLock::new(HashMap::new())),
            session_prefix: "forge-".into(),
        }
    }

    /// Create a new worker launcher with a custom session prefix.
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            workers: Arc::new(RwLock::new(HashMap::new())),
            session_prefix: prefix.into(),
        }
    }

    /// Spawn a new worker using the provided configuration.
    ///
    /// This will:
    /// 1. Validate the launcher script exists and is executable
    /// 2. Execute the launcher script with appropriate environment
    /// 3. Parse the JSON output to get PID and session info
    /// 4. Create a WorkerHandle and track it
    #[instrument(level = "info", skip(self), fields(worker_id = %request.worker_id, model = %request.config.model))]
    pub async fn spawn(&self, request: SpawnRequest) -> Result<WorkerHandle> {
        let config = &request.config;
        let worker_id = &request.worker_id;

        // Validate launcher exists
        self.validate_launcher(&config.launcher_path).await?;

        info!(
            "Spawning worker {} with model {} in {}",
            worker_id,
            config.model,
            config.workspace.display()
        );

        // Check if session already exists and kill it
        let session_name = format!("{}{}", self.session_prefix, config.session_name);
        if tmux::session_exists(&session_name).await? {
            warn!("Session {} already exists, killing it", session_name);
            tmux::kill_session(&session_name).await?;
        }

        // Execute the launcher script
        let output = self
            .execute_launcher(worker_id, config, &session_name)
            .await?;

        // Parse launcher output
        let launcher_output = self.parse_launcher_output(worker_id, &output).await?;

        // Validate the launcher succeeded
        if !launcher_output.is_success() {
            return Err(ForgeError::LauncherExecution {
                model: config.model.clone(),
                message: launcher_output
                    .error
                    .unwrap_or_else(|| "Unknown launcher error".into()),
            });
        }

        // Verify the session was created
        if !tmux::session_exists(&launcher_output.session).await? {
            return Err(ForgeError::WorkerSpawn {
                worker_id: worker_id.clone(),
                message: format!(
                    "Launcher claimed to create session '{}' but it doesn't exist",
                    launcher_output.session
                ),
            });
        }

        // Create worker handle with optional bead assignment
        let mut handle = WorkerHandle::new(
            worker_id.clone(),
            launcher_output.pid,
            launcher_output.session.clone(),
            config.launcher_path.clone(),
            if launcher_output.model.is_empty() {
                config.model.clone()
            } else {
                launcher_output.model.clone()
            },
            config.tier,
            config.workspace.clone(),
        );

        // Add bead assignment if present in launcher output or config
        if let Some(ref bead_id) = launcher_output.bead_id {
            let bead_title = launcher_output.bead_title.clone().unwrap_or_else(|| bead_id.clone());
            handle = handle.with_bead(bead_id.clone(), bead_title);
        } else if let Some(ref bead_id) = config.bead_id {
            handle = handle.with_bead(bead_id.clone(), bead_id.clone());
        }

        // Store the handle
        {
            let mut workers = self.workers.write().await;
            workers.insert(worker_id.clone(), handle.clone());
        }

        info!(
            "Worker {} spawned successfully (PID: {}, session: {})",
            worker_id, launcher_output.pid, launcher_output.session
        );

        Ok(handle)
    }

    /// Validate that a launcher script exists and is executable.
    async fn validate_launcher(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Err(ForgeError::launcher_not_found(path));
        }

        // Check if executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = tokio::fs::metadata(path).await.map_err(|e| {
                ForgeError::io("checking launcher permissions", path, e)
            })?;
            let permissions = metadata.permissions();
            if permissions.mode() & 0o111 == 0 {
                return Err(ForgeError::LauncherNotExecutable { path: path.into() });
            }
        }

        debug!("Launcher validated: {}", path.display());
        Ok(())
    }

    /// Execute the launcher script and capture output.
    async fn execute_launcher(
        &self,
        worker_id: &str,
        config: &LaunchConfig,
        session_name: &str,
    ) -> Result<String> {
        let mut cmd = Command::new(&config.launcher_path);

        // Pass standard arguments
        cmd.arg(format!("--model={}", config.model))
            .arg(format!("--workspace={}", config.workspace.display()))
            .arg(format!("--session-name={}", session_name));

        // Pass bead-ref if we have a bead assignment (bead-aware launcher protocol)
        if let Some(ref bead_id) = config.bead_id {
            cmd.arg(format!("--bead-ref={}", bead_id));
            debug!("Launching bead-aware worker for bead: {}", bead_id);
        }

        // Set working directory
        cmd.current_dir(&config.workspace);

        // Set environment variables
        cmd.env("FORGE_WORKER_ID", worker_id);
        cmd.env("FORGE_SESSION", session_name);
        cmd.env("FORGE_MODEL", &config.model);
        cmd.env("FORGE_WORKSPACE", &config.workspace);

        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        debug!(
            "Executing launcher: {}",
            config.launcher_path.display()
        );

        // Execute with timeout
        let timeout_duration = Duration::from_secs(config.timeout_secs);
        let result = timeout(timeout_duration, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    error!(
                        "Launcher failed with status {}: stdout={}, stderr={}",
                        output.status, stdout, stderr
                    );
                    return Err(ForgeError::LauncherExecution {
                        model: config.model.clone(),
                        message: format!(
                            "Exit code: {:?}, stderr: {}",
                            output.status.code(),
                            stderr.trim()
                        ),
                    });
                }

                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                debug!("Launcher output: {}", stdout.trim());
                Ok(stdout)
            }
            Ok(Err(e)) => Err(ForgeError::LauncherExecution {
                model: config.model.clone(),
                message: format!("Failed to execute launcher: {}", e),
            }),
            Err(_) => Err(ForgeError::LauncherTimeout {
                timeout_secs: config.timeout_secs,
            }),
        }
    }

    /// Parse JSON output from a launcher script.
    async fn parse_launcher_output(
        &self,
        worker_id: &str,
        output: &str,
    ) -> Result<LauncherOutput> {
        // Find JSON in the output (launcher might emit other text before JSON)
        let json_start = output.find('{');
        let json_end = output.rfind('}');

        match (json_start, json_end) {
            (Some(start), Some(end)) if end >= start => {
                let json_str = &output[start..=end];
                serde_json::from_str(json_str).map_err(|e| {
                    ForgeError::LauncherOutput {
                        message: format!("Invalid JSON in launcher output: {} (input: {})", e, json_str),
                    }
                })
            }
            _ => {
                // If no JSON found, try to construct output from stdout
                // This supports simple launchers that just output the session name
                warn!(
                    "No JSON found in launcher output for {}, attempting fallback parse",
                    worker_id
                );

                // Try to get the PID from tmux
                let session_name = output.trim();
                if session_name.is_empty() {
                    return Err(ForgeError::LauncherOutput {
                        message: "Launcher produced no output".into(),
                    });
                }

                let pid = tmux::get_session_pid(session_name)
                    .await?
                    .unwrap_or(0);

                Ok(LauncherOutput {
                    pid,
                    session: session_name.to_string(),
                    model: String::new(),
                    message: None,
                    error: None,
                    bead_id: None,
                    bead_title: None,
                })
            }
        }
    }

    /// Get a worker handle by ID.
    pub async fn get(&self, worker_id: &str) -> Option<WorkerHandle> {
        let workers = self.workers.read().await;
        workers.get(worker_id).cloned()
    }

    /// Get all active worker handles.
    pub async fn list(&self) -> Vec<WorkerHandle> {
        let workers = self.workers.read().await;
        workers.values().cloned().collect()
    }

    /// Stop a worker by ID.
    #[instrument(level = "info", skip(self), fields(worker_id = %worker_id))]
    pub async fn stop(&self, worker_id: &str) -> Result<()> {
        let handle = {
            let workers = self.workers.read().await;
            workers.get(worker_id).cloned()
        };

        match handle {
            Some(handle) => {
                info!("Stopping worker {} (session: {})", worker_id, handle.session_name);
                tmux::kill_session(&handle.session_name).await?;

                // Remove from active workers
                {
                    let mut workers = self.workers.write().await;
                    workers.remove(worker_id);
                }

                info!("Worker {} stopped", worker_id);
                Ok(())
            }
            None => Err(ForgeError::WorkerNotFound {
                worker_id: worker_id.into(),
            }),
        }
    }

    /// Stop all workers.
    #[instrument(level = "info", skip(self))]
    pub async fn stop_all(&self) -> Result<()> {
        let worker_ids: Vec<String> = {
            let workers = self.workers.read().await;
            workers.keys().cloned().collect()
        };

        for worker_id in worker_ids {
            if let Err(e) = self.stop(&worker_id).await {
                warn!("Failed to stop worker {}: {}", worker_id, e);
            }
        }

        Ok(())
    }

    /// Check the status of a worker.
    #[instrument(level = "debug", skip(self), fields(worker_id = %worker_id))]
    pub async fn check_status(&self, worker_id: &str) -> Result<WorkerStatus> {
        let handle = {
            let workers = self.workers.read().await;
            workers.get(worker_id).cloned()
        };

        match handle {
            Some(handle) => {
                // Check if the tmux session still exists
                if tmux::session_exists(&handle.session_name).await? {
                    // Check if the process is still running
                    let pid = tmux::get_session_pid(&handle.session_name).await?;
                    match pid {
                        Some(_) => Ok(WorkerStatus::Active),
                        None => Ok(WorkerStatus::Failed),
                    }
                } else {
                    // Session gone, worker has stopped
                    Ok(WorkerStatus::Stopped)
                }
            }
            None => Err(ForgeError::WorkerNotFound {
                worker_id: worker_id.into(),
            }),
        }
    }

    /// Update the status of a worker in the internal map.
    pub async fn update_status(&self, worker_id: &str, status: WorkerStatus) -> Result<()> {
        let mut workers = self.workers.write().await;
        match workers.get_mut(worker_id) {
            Some(handle) => {
                handle.status = status;
                Ok(())
            }
            None => Err(ForgeError::WorkerNotFound {
                worker_id: worker_id.into(),
            }),
        }
    }

    /// Refresh status for all workers.
    pub async fn refresh_all_status(&self) -> Result<()> {
        let worker_ids: Vec<String> = {
            let workers = self.workers.read().await;
            workers.keys().cloned().collect()
        };

        for worker_id in worker_ids {
            match self.check_status(&worker_id).await {
                Ok(status) => {
                    let _ = self.update_status(&worker_id, status).await;
                }
                Err(e) => {
                    warn!("Failed to check status for worker {}: {}", worker_id, e);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LaunchConfig;
    use forge_core::types::WorkerTier;
    use std::path::PathBuf;

    #[test]
    fn test_launcher_creation() {
        let launcher = WorkerLauncher::new();
        assert_eq!(launcher.session_prefix, "forge-");
    }

    #[test]
    fn test_launcher_with_prefix() {
        let launcher = WorkerLauncher::with_prefix("test-");
        assert_eq!(launcher.session_prefix, "test-");
    }

    #[tokio::test]
    async fn test_launcher_empty_workers() {
        let launcher = WorkerLauncher::new();
        let workers = launcher.list().await;
        assert!(workers.is_empty());
    }

    #[tokio::test]
    async fn test_launcher_get_nonexistent() {
        let launcher = WorkerLauncher::new();
        let worker = launcher.get("nonexistent").await;
        assert!(worker.is_none());
    }

    #[tokio::test]
    async fn test_stop_nonexistent_worker() {
        let launcher = WorkerLauncher::new();
        let result = launcher.stop("nonexistent").await;
        assert!(matches!(result, Err(ForgeError::WorkerNotFound { .. })));
    }

    #[test]
    fn test_launch_config_creation() {
        let config = LaunchConfig::new(
            PathBuf::from("/path/to/launcher.sh"),
            "test-session",
            PathBuf::from("/workspace"),
            "sonnet",
        );

        assert_eq!(config.session_name, "test-session");
        assert_eq!(config.model, "sonnet");
        assert_eq!(config.tier, WorkerTier::Standard);
        assert_eq!(config.timeout_secs, 30);
    }

    #[tokio::test]
    async fn test_parse_launcher_json_output() {
        let launcher = WorkerLauncher::new();

        let json_output = r#"{"pid": 12345, "session": "forge-test", "model": "sonnet"}"#;
        let result = launcher.parse_launcher_output("test", json_output).await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.pid, 12345);
        assert_eq!(output.session, "forge-test");
        assert_eq!(output.model, "sonnet");
    }

    #[tokio::test]
    async fn test_parse_launcher_json_with_prefix() {
        let launcher = WorkerLauncher::new();

        // Launcher might output logging before JSON
        let output = r#"Starting worker...
Initializing model...
{"pid": 54321, "session": "forge-worker-1", "model": "opus", "message": "Started"}"#;

        let result = launcher.parse_launcher_output("test", output).await;

        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.pid, 54321);
        assert_eq!(parsed.session, "forge-worker-1");
        assert_eq!(parsed.message, Some("Started".into()));
    }

    #[tokio::test]
    async fn test_parse_launcher_error_output() {
        let launcher = WorkerLauncher::new();

        let output = r#"{"pid": 0, "session": "", "error": "Failed to start: no API key"}"#;
        let result = launcher.parse_launcher_output("test", output).await;

        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert!(!parsed.is_success());
        assert_eq!(parsed.error, Some("Failed to start: no API key".into()));
    }

    #[tokio::test]
    async fn test_parse_invalid_json() {
        let launcher = WorkerLauncher::new();

        let output = "not valid json at all";
        let result = launcher.parse_launcher_output("test", output).await;

        // Should try fallback parsing since no JSON found
        // This will fail because there's no valid session name
        assert!(result.is_err() || !result.unwrap().is_success());
    }
}
