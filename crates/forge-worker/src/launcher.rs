//! Worker launcher implementation using tokio::process.
//!
//! This module provides the [`WorkerLauncher`] type for spawning worker processes
//! in tmux sessions using configurable launcher scripts, or as Docker
//! containers managed directly (see [`crate::docker`]).

use crate::docker;
use crate::tmux;
use crate::types::{LaunchConfig, LauncherOutput, SpawnRequest, WorkerBackend, WorkerHandle};
use forge_core::types::WorkerStatus;
use forge_core::{ForgeError, Result};
use forge_cost::CostDatabase;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio::time::timeout;
use tracing::{debug, error, info, instrument, warn};

/// Build the command-line arguments passed to a launcher script.
///
/// Standard launcher-protocol arguments are always present; the bead-aware
/// extension adds `--bead-ref=<bead-id>` when the launch config carries a
/// bead assignment (see `docs/BEAD_LAUNCHER_PROTOCOL.md`).
fn build_launcher_args(config: &LaunchConfig, session_name: &str) -> Vec<String> {
    let mut args = vec![
        format!("--model={}", config.model),
        format!("--workspace={}", config.workspace.display()),
        format!("--session-name={}", session_name),
    ];

    if let Some(ref bead_id) = config.bead_id {
        args.push(format!("--bead-ref={}", bead_id));
    }

    args
}

/// Worker launcher for spawning and managing worker processes.
///
/// The default backend uses external launcher scripts to spawn workers in
/// tmux sessions; launcher scripts must output JSON to stdout with worker
/// information. The Docker backend spawns containers directly and needs no
/// script — set `WorkerBackend::Docker` plus a pinned image on the
/// [`LaunchConfig`].
#[derive(Debug)]
pub struct WorkerLauncher {
    /// Active worker handles keyed by worker ID
    workers: Arc<RwLock<HashMap<String, WorkerHandle>>>,
    /// Session name prefix for all workers
    session_prefix: String,
    /// Cost database used to persist task predictions before launch.
    cost_db: Option<CostDatabase>,
    /// Docker CLI binary used by the Docker backend.
    docker_bin: PathBuf,
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
            cost_db: Self::default_cost_database(),
            docker_bin: PathBuf::from(docker::DEFAULT_DOCKER_BIN),
        }
    }

    /// Create a new worker launcher with a custom session prefix.
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            workers: Arc::new(RwLock::new(HashMap::new())),
            session_prefix: prefix.into(),
            cost_db: Self::default_cost_database(),
            docker_bin: PathBuf::from(docker::DEFAULT_DOCKER_BIN),
        }
    }

    /// Use an explicit cost database, primarily for isolated callers/tests.
    pub fn with_cost_database(mut self, db: CostDatabase) -> Self {
        self.cost_db = Some(db);
        self
    }

    /// Override the docker CLI binary, primarily for isolated callers/tests.
    pub fn with_docker_binary(mut self, bin: impl Into<PathBuf>) -> Self {
        self.docker_bin = bin.into();
        self
    }

    /// Open the standard FORGE cost database when available.
    fn default_cost_database() -> Option<CostDatabase> {
        let home = std::env::var_os("HOME")?;
        let forge_dir = PathBuf::from(home).join(".forge");
        std::fs::create_dir_all(&forge_dir).ok()?;
        CostDatabase::open(forge_dir.join("costs.db")).ok()
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
        // Persist the prediction before any launcher work. A failed launch still
        // represents an assignment attempt, and successful launches will have
        // their later API-call rows correlated by bead_id.
        if let (Some(db), Some(assignment)) = (&self.cost_db, request.task_assignment.as_ref())
            && let Err(error) = db.insert_task_assignment(assignment)
        {
            warn!(
                bead_id = %assignment.bead_id,
                error = %error,
                "Failed to persist task assignment prediction"
            );
        }

        let config = &request.config;
        let worker_id = &request.worker_id;

        // Docker workers bypass launcher scripts entirely.
        if config.is_docker() {
            return self.spawn_docker(worker_id, config).await;
        }

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
            let bead_title = launcher_output
                .bead_title
                .clone()
                .unwrap_or_else(|| bead_id.clone());
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

    /// Spawn a worker as a Docker container.
    ///
    /// Mirrors the tmux flow: kill any stale container of the same name,
    /// start the container from the configured pinned image with the
    /// workspace bind-mounted, verify it is actually running, then track the
    /// handle. The workspace must already exist — unlike `docker run`, the
    /// tmux path fails fast on a missing working directory, and silently
    /// creating a stray host directory would hide config mistakes.
    async fn spawn_docker(&self, worker_id: &str, config: &LaunchConfig) -> Result<WorkerHandle> {
        let image = config
            .image
            .as_deref()
            .ok_or_else(|| ForgeError::WorkerSpawn {
                worker_id: worker_id.to_string(),
                message: "Docker backend requires an image (use with_docker_image)".into(),
            })?;
        docker::validate_image_ref(image)?;

        if !config.workspace.exists() {
            return Err(ForgeError::WorkerSpawn {
                worker_id: worker_id.to_string(),
                message: format!(
                    "Workspace {} does not exist; cannot bind-mount it into the container",
                    config.workspace.display()
                ),
            });
        }

        info!(
            "Spawning worker {} with model {} in container image {}",
            worker_id, config.model, image
        );

        let container_name = format!("{}{}", self.session_prefix, config.session_name);

        // Check if a stale container already exists and remove it
        if docker::container_exists(&self.docker_bin, &container_name).await? {
            warn!("Container {} already exists, removing it", container_name);
            docker::remove_container(&self.docker_bin, &container_name).await?;
        }

        docker::run_container(&self.docker_bin, config, &container_name, worker_id).await?;

        // Verify the container is actually running before reporting success
        let state = docker::inspect_state(&self.docker_bin, &container_name)
            .await?
            .ok_or_else(|| ForgeError::WorkerSpawn {
                worker_id: worker_id.to_string(),
                message: format!(
                    "Container '{}' was created but cannot be inspected",
                    container_name
                ),
            })?;

        if state.status != "running" {
            return Err(ForgeError::WorkerSpawn {
                worker_id: worker_id.to_string(),
                message: format!(
                    "Container '{}' is not running after launch (state: {})",
                    container_name, state.status
                ),
            });
        }

        // The container's init PID as seen from the host, when available
        let mut handle = WorkerHandle::new(
            worker_id.to_string(),
            state.pid.unwrap_or(0),
            container_name,
            self.docker_bin.clone(),
            config.model.clone(),
            config.tier,
            config.workspace.clone(),
        )
        .with_backend(WorkerBackend::Docker);

        // Add bead assignment if present in the launch config (the Docker
        // backend has no launcher-script output to carry one)
        if let Some(ref bead_id) = config.bead_id {
            handle = handle.with_bead(bead_id.clone(), bead_id.clone());
        }

        {
            let mut workers = self.workers.write().await;
            workers.insert(worker_id.to_string(), handle.clone());
        }

        info!(
            "Worker {} spawned successfully (container: {}, backend: docker)",
            worker_id, handle.session_name
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
            let metadata = tokio::fs::metadata(path)
                .await
                .map_err(|e| ForgeError::io("checking launcher permissions", path, e))?;
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

        // Standard arguments plus the bead-aware extension
        let args = build_launcher_args(config, session_name);
        if config.has_bead() {
            debug!(
                "Launching bead-aware worker for bead: {}",
                config.bead_id.as_deref().unwrap_or_default()
            );
        }
        for arg in args {
            cmd.arg(arg);
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

        debug!("Executing launcher: {}", config.launcher_path.display());

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
    async fn parse_launcher_output(&self, worker_id: &str, output: &str) -> Result<LauncherOutput> {
        // Find JSON in the output (launcher might emit other text before JSON)
        let json_start = output.find('{');
        let json_end = output.rfind('}');

        match (json_start, json_end) {
            (Some(start), Some(end)) if end >= start => {
                let json_str = &output[start..=end];
                serde_json::from_str(json_str).map_err(|e| ForgeError::LauncherOutput {
                    message: format!(
                        "Invalid JSON in launcher output: {} (input: {})",
                        e, json_str
                    ),
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

                let pid = tmux::get_session_pid(session_name).await?.unwrap_or(0);

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
                info!(
                    "Stopping worker {} (session: {}, backend: {})",
                    worker_id, handle.session_name, handle.backend
                );
                // Cleanup on kill: tmux workers lose their session; Docker
                // workers are force-removed so no exited container lingers.
                match handle.backend {
                    WorkerBackend::Docker => {
                        docker::remove_container(&self.docker_bin, &handle.session_name).await?;
                    }
                    WorkerBackend::Tmux => {
                        tmux::kill_session(&handle.session_name).await?;
                    }
                }

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
            Some(handle) => match handle.backend {
                WorkerBackend::Docker => {
                    // Map the container's observed state onto worker status;
                    // a missing container means the worker has stopped.
                    match docker::inspect_state(&self.docker_bin, &handle.session_name).await? {
                        Some(state) => Ok(docker::state_to_worker_status(&state)),
                        None => Ok(WorkerStatus::Stopped),
                    }
                }
                WorkerBackend::Tmux => {
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
            },
            None => Err(ForgeError::WorkerNotFound {
                worker_id: worker_id.into(),
            }),
        }
    }

    /// Capture recent output from a worker's terminal.
    ///
    /// The backend-aware log path, mirroring [`Self::check_status`]'s
    /// dispatch: tmux workers are read through `tmux capture-pane`, Docker
    /// workers through `docker logs`. `lines` caps the capture to the most
    /// recent N lines; `None` returns everything available.
    #[instrument(level = "debug", skip(self), fields(worker_id = %worker_id))]
    pub async fn worker_logs(&self, worker_id: &str, lines: Option<u32>) -> Result<String> {
        let handle = {
            let workers = self.workers.read().await;
            workers.get(worker_id).cloned()
        };

        match handle {
            Some(handle) => match handle.backend {
                WorkerBackend::Docker => {
                    docker::container_logs(&self.docker_bin, &handle.session_name, lines).await
                }
                WorkerBackend::Tmux => tmux::capture_pane(&handle.session_name, lines).await,
            },
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
    use forge_cost::{CostDatabase, TaskAssignment};
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    #[test]
    fn test_launcher_creation() {
        let launcher = WorkerLauncher::new();
        assert_eq!(launcher.session_prefix, "forge-");
    }

    #[test]
    fn test_launcher_args_standard_protocol() {
        let config = LaunchConfig::new("/path/to/launcher.sh", "session", "/workspace", "sonnet");

        let args = build_launcher_args(&config, "forge-session");
        assert_eq!(
            args,
            vec![
                "--model=sonnet".to_string(),
                "--workspace=/workspace".to_string(),
                "--session-name=forge-session".to_string(),
            ]
        );
    }

    #[test]
    fn test_launcher_args_include_bead_ref() {
        // The bead-aware launcher protocol extension: a configured bead is
        // forwarded to the launcher script as --bead-ref=<bead-id>.
        let config = LaunchConfig::new("/path/to/launcher.sh", "session", "/workspace", "sonnet")
            .with_bead("fg-1qo");

        let args = build_launcher_args(&config, "forge-fg-1qo-sonnet");
        assert_eq!(args.len(), 4);
        assert!(args.contains(&"--bead-ref=fg-1qo".to_string()));
        // The extension comes after the standard arguments.
        assert_eq!(args.last().unwrap(), "--bead-ref=fg-1qo");
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
    async fn test_task_assignment_is_persisted_before_launch() {
        let db = CostDatabase::open_in_memory().unwrap();
        let launcher = WorkerLauncher::new().with_cost_database(db.clone());
        let config = LaunchConfig::new(
            "/path/to/missing-launcher.sh",
            "test-session",
            "/workspace",
            "claude-opus",
        )
        .with_bead("bd-123");
        let request = SpawnRequest::new("worker-1", config)
            .with_task_assignment(TaskAssignment::new("bd-123", 72, "premium", "claude-opus"));

        // The launch fails after the assignment persistence step, proving the
        // prediction is recorded before launcher validation/execution.
        assert!(launcher.spawn(request).await.is_err());

        let conn = db.connection();
        let conn = conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_assignments WHERE bead_id = 'bd-123'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
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

    // ------------------------------------------------------------
    // Docker backend lifecycle (spawn / kill / status) via a fake docker CLI
    // ------------------------------------------------------------

    fn docker_test_config(session: &str, workspace: &Path, image: Option<&str>) -> LaunchConfig {
        let mut config = LaunchConfig::new("/unused/launcher.sh", session, workspace, "sonnet");
        config.backend = WorkerBackend::Docker;
        if let Some(image) = image {
            config = config.with_docker_image(image);
        }
        config
    }

    fn docker_test_launcher(tmp: &TempDir) -> WorkerLauncher {
        let fake = crate::docker::fake::install(tmp.path(), None);
        WorkerLauncher::new()
            .with_cost_database(CostDatabase::open_in_memory().unwrap())
            .with_docker_binary(fake)
    }

    #[tokio::test]
    async fn test_spawn_status_and_kill_docker_worker() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let launcher = docker_test_launcher(&tmp);

        // Spawn: handle records the docker backend and container identity
        let handle = launcher
            .spawn(SpawnRequest::new(
                "worker-d1",
                docker_test_config(
                    "claude-code-sonnet-alpha",
                    &workspace,
                    Some("example/agent:1.2.3"),
                ),
            ))
            .await
            .unwrap();
        assert_eq!(handle.backend, WorkerBackend::Docker);
        assert_eq!(handle.session_name, "forge-claude-code-sonnet-alpha");
        assert_eq!(handle.pid, 4242);
        assert!(launcher.get("worker-d1").await.is_some());

        // Health: status check reports Active from the running container
        let status = launcher.check_status("worker-d1").await.unwrap();
        assert_eq!(status, WorkerStatus::Active);

        // Kill: container is removed entirely, nothing left behind
        launcher.stop("worker-d1").await.unwrap();
        assert!(!crate::docker::fake::container_exists(tmp.path()));
        assert!(launcher.get("worker-d1").await.is_none());

        // Status after stop: worker no longer tracked
        assert!(matches!(
            launcher.check_status("worker-d1").await,
            Err(ForgeError::WorkerNotFound { .. })
        ));
    }

    #[tokio::test]
    async fn test_check_status_docker_worker_exited() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let launcher = docker_test_launcher(&tmp);

        launcher
            .spawn(SpawnRequest::new(
                "worker-d2",
                docker_test_config(
                    "claude-code-sonnet-alpha",
                    &workspace,
                    Some("example/agent:1.2.3"),
                ),
            ))
            .await
            .unwrap();

        // Container dies (e.g. OOM-kill): status check notices
        crate::docker::fake::set_state(tmp.path(), "exited");
        let status = launcher.check_status("worker-d2").await.unwrap();
        assert_eq!(status, WorkerStatus::Stopped);

        // Container vanishes entirely (removed out-of-band): also Stopped
        crate::docker::fake::clear_state(tmp.path());
        let status = launcher.check_status("worker-d2").await.unwrap();
        assert_eq!(status, WorkerStatus::Stopped);
    }

    #[tokio::test]
    async fn test_spawn_docker_requires_pinned_image() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let launcher = docker_test_launcher(&tmp);

        // No image configured
        let err = launcher
            .spawn(SpawnRequest::new(
                "worker-d3",
                docker_test_config("s1", &workspace, None),
            ))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("requires an image"), "{err}");

        // Floating latest tag is rejected
        let err = launcher
            .spawn(SpawnRequest::new(
                "worker-d4",
                docker_test_config("s2", &workspace, Some("example/agent:latest")),
            ))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("latest"), "{err}");

        // Nothing was spawned
        assert!(launcher.list().await.is_empty());
        assert!(!crate::docker::fake::container_exists(tmp.path()));
    }

    #[tokio::test]
    async fn test_spawn_docker_replaces_stale_container() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        // Pre-seed a stale container of the same name
        let fake = crate::docker::fake::install(tmp.path(), Some("exited"));
        let launcher = WorkerLauncher::new()
            .with_cost_database(CostDatabase::open_in_memory().unwrap())
            .with_docker_binary(fake);

        launcher
            .spawn(SpawnRequest::new(
                "worker-d5",
                docker_test_config(
                    "claude-code-sonnet-alpha",
                    &workspace,
                    Some("example/agent:1.2.3"),
                ),
            ))
            .await
            .unwrap();

        // The stale container was removed and a running one took its place
        let status = launcher.check_status("worker-d5").await.unwrap();
        assert_eq!(status, WorkerStatus::Active);
    }

    #[tokio::test]
    async fn test_spawn_docker_missing_workspace_fails() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let launcher = docker_test_launcher(&tmp);

        let err = launcher
            .spawn(SpawnRequest::new(
                "worker-d6",
                docker_test_config("s3", &missing, Some("example/agent:1.2.3")),
            ))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
        assert!(!crate::docker::fake::container_exists(tmp.path()));
    }

    #[tokio::test]
    async fn test_worker_logs_docker_backend() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let launcher = docker_test_launcher(&tmp);

        launcher
            .spawn(SpawnRequest::new(
                "worker-d7",
                docker_test_config(
                    "claude-code-sonnet-alpha",
                    &workspace,
                    Some("example/agent:1.2.3"),
                ),
            ))
            .await
            .unwrap();

        // Log capture flows through the backend-aware entry point.
        let logs = launcher.worker_logs("worker-d7", Some(2)).await.unwrap();
        assert_eq!(logs.lines().collect::<Vec<_>>(), vec!["working", "done"]);
        let logs = launcher.worker_logs("worker-d7", None).await.unwrap();
        assert!(logs.contains("forge-worker starting"), "{logs}");

        // Unknown worker is the standard not-found error.
        let err = launcher
            .worker_logs("no-such-worker", None)
            .await
            .unwrap_err();
        assert!(matches!(err, ForgeError::WorkerNotFound { .. }));
    }
}
