//! Pause signal handling for workers.
//!
//! This module provides functionality for workers to check and respond to pause
//! signals. The pause mechanism ensures graceful behavior:
//!
//! 1. **Graceful finish**: Workers complete their current bead before pausing
//! 2. **Idle loop**: When paused, workers sleep for 60s intervals checking for unpause
//! 3. **No new claims**: Paused workers do not claim new beads until resumed
//!
//! ## Pause/Resume Protocol
//!
//! The pause state is managed via status files in `~/.forge/status/<worker_id>.json`.
//! The `StatusWriter::pause_worker()` and `StatusWriter::resume_worker()` methods
//! update these files.
//!
//! Workers check their pause status BEFORE claiming beads using `PauseSignalHandler`,
//! which ensures the current bead is completed before entering pause state.
//!
//! ## Graceful Bead Completion
//!
//! The key design principle: workers check pause status BEFORE claiming a new bead,
//! not while executing. This ensures:
//!
//! - Current bead work is never interrupted
//! - Worker completes current task fully
//! - Only then enters pause idle loop
//!
//! ## Example: Worker Main Loop Pattern
//!
//! ```no_run
//! use forge_worker::pause::{PauseSignalHandler, PauseConfig};
//! use forge_worker::bead_queue::BeadQueueReader;
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> forge_core::Result<()> {
//!     let handler = PauseSignalHandler::new("my-worker", None)?;
//!
//!     loop {
//!         // Step 1: Check pause status BEFORE claiming next bead
//!         // This ensures current bead was completed gracefully
//!         handler.check_before_claim().await?;
//!
//!         // Step 2: Claim and execute next bead
//!         // (pause check happens before this, never during)
//!         // let bead = queue.pop_ready_bead();
//!         // execute_bead(bead).await;
//!
//!         // Step 3: Bead completed, loop back to step 1
//!         // If paused, check_before_claim() will block until resumed
//!     }
//! }
//! ```
//!
//! ## Simple Example
//!
//! ```no_run
//! use forge_worker::pause::{PauseSignalHandler, PauseConfig};
//!
//! #[tokio::main]
//! async fn main() -> forge_core::Result<()> {
//!     let handler = PauseSignalHandler::new("my-worker", None)?;
//!
//!     // Before claiming a new bead
//!     if handler.is_paused()? {
//!         // Enter idle loop until unpaused
//!         handler.wait_for_unpause().await?;
//!     }
//!
//!     // Worker is unpaused, can claim next bead
//!     Ok(())
//! }
//! ```

use std::path::PathBuf;
use std::time::Duration;

use forge_core::status::{StatusReader, StatusWriter};
use forge_core::types::WorkerStatus;
use forge_core::{ForgeError, Result};
use tracing::{debug, info, warn};

/// Default idle loop sleep duration when paused (60 seconds).
pub const DEFAULT_PAUSE_CHECK_INTERVAL_SECS: u64 = 60;

/// Configuration for pause signal handling.
#[derive(Debug, Clone)]
pub struct PauseConfig {
    /// How often to check for unpause when in idle loop (seconds).
    pub check_interval_secs: u64,
    /// Maximum time to wait for unpause before giving up (0 = wait forever).
    pub max_wait_secs: u64,
}

impl Default for PauseConfig {
    fn default() -> Self {
        Self {
            check_interval_secs: DEFAULT_PAUSE_CHECK_INTERVAL_SECS,
            max_wait_secs: 0, // Wait forever by default
        }
    }
}

impl PauseConfig {
    /// Create config with custom check interval.
    pub fn with_check_interval(mut self, secs: u64) -> Self {
        self.check_interval_secs = secs;
        self
    }

    /// Create config with maximum wait time.
    pub fn with_max_wait(mut self, secs: u64) -> Self {
        self.max_wait_secs = secs;
        self
    }
}

/// Handler for pause signal checking and responding.
///
/// Workers use this handler to check if they should pause before claiming
/// new beads, and to enter an idle loop when paused.
#[derive(Debug)]
pub struct PauseSignalHandler {
    /// Worker ID
    worker_id: String,
    /// Status file reader
    status_reader: StatusReader,
    /// Configuration
    config: PauseConfig,
    /// Status directory path
    status_dir: PathBuf,
}

impl PauseSignalHandler {
    /// Create a new pause signal handler for a worker.
    ///
    /// If `status_dir` is None, uses the default `~/.forge/status/` directory.
    pub fn new(worker_id: impl Into<String>, status_dir: Option<PathBuf>) -> Result<Self> {
        let status_dir = match status_dir {
            Some(dir) => dir,
            None => StatusReader::default_status_dir()?,
        };

        let status_reader = StatusReader::new(Some(status_dir.clone()))?;

        Ok(Self {
            worker_id: worker_id.into(),
            status_reader,
            config: PauseConfig::default(),
            status_dir,
        })
    }

    /// Create a new pause signal handler with custom configuration.
    pub fn with_config(
        worker_id: impl Into<String>,
        status_dir: Option<PathBuf>,
        config: PauseConfig,
    ) -> Result<Self> {
        let mut handler = Self::new(worker_id, status_dir)?;
        handler.config = config;
        Ok(handler)
    }

    /// Check if the worker is currently paused.
    ///
    /// Returns `true` if the worker's status is `Paused`, `false` otherwise.
    /// If the status file doesn't exist or can't be read, returns `false`.
    pub fn is_paused(&self) -> Result<bool> {
        match self.status_reader.read_worker(&self.worker_id)? {
            Some(status_info) => {
                let is_paused = status_info.status == WorkerStatus::Paused;
                if is_paused {
                    debug!(worker_id = %self.worker_id, "Worker is paused");
                }
                Ok(is_paused)
            }
            None => {
                // No status file - not paused
                debug!(
                    worker_id = %self.worker_id,
                    "No status file found, worker is not paused"
                );
                Ok(false)
            }
        }
    }

    /// Check pause status before claiming a bead.
    ///
    /// This is a convenience wrapper that:
    /// 1. Checks if paused
    /// 2. If paused, waits for unpause
    /// 3. Returns `Ok(true)` when ready to claim bead
    ///
    /// Returns `Err` if waiting times out (when max_wait_secs > 0).
    pub async fn check_before_claim(&self) -> Result<bool> {
        if self.is_paused()? {
            info!(
                worker_id = %self.worker_id,
                "Worker paused, waiting for unpause before claiming bead"
            );
            self.wait_for_unpause().await?;
        }
        Ok(true)
    }

    /// Wait for the worker to be unpaused.
    ///
    /// This enters an idle loop, sleeping for `check_interval_secs` seconds
    /// between pause status checks. Exits when:
    /// - Worker is unpaused (returns `Ok(())`)
    /// - Max wait time exceeded (returns `Err` if `max_wait_secs > 0`)
    ///
    /// While waiting, the worker updates its status to show it's paused.
    pub async fn wait_for_unpause(&self) -> Result<()> {
        let check_interval = Duration::from_secs(self.config.check_interval_secs);
        let start_time = std::time::Instant::now();
        let max_wait = if self.config.max_wait_secs > 0 {
            Some(Duration::from_secs(self.config.max_wait_secs))
        } else {
            None
        };

        info!(
            worker_id = %self.worker_id,
            check_interval_secs = self.config.check_interval_secs,
            "Entering pause idle loop"
        );

        loop {
            // Sleep first (check already determined we're paused)
            tokio::time::sleep(check_interval).await;

            // Check if we're still paused
            if !self.is_paused()? {
                info!(
                    worker_id = %self.worker_id,
                    elapsed_secs = start_time.elapsed().as_secs(),
                    "Worker unpaused, resuming"
                );
                return Ok(());
            }

            // Check timeout
            if let Some(max) = max_wait {
                if start_time.elapsed() >= max {
                    warn!(
                        worker_id = %self.worker_id,
                        max_wait_secs = self.config.max_wait_secs,
                        "Pause wait timed out"
                    );
                    return Err(ForgeError::Timeout {
                        operation: format!("wait_for_unpause(worker={})", self.worker_id),
                        timeout_secs: self.config.max_wait_secs,
                    });
                }
            }

            debug!(
                worker_id = %self.worker_id,
                elapsed_secs = start_time.elapsed().as_secs(),
                "Still paused, continuing idle loop"
            );
        }
    }

    /// Signal that the worker should pause after completing current bead.
    ///
    /// This updates the worker's status to `Paused` in the status file.
    pub fn request_pause(&self) -> Result<()> {
        let writer = StatusWriter::new(Some(self.status_dir.clone()))?;
        writer.pause_worker(&self.worker_id)?;
        info!(worker_id = %self.worker_id, "Pause requested");
        Ok(())
    }

    /// Signal that the worker should resume.
    ///
    /// This updates the worker's status to `Idle` in the status file.
    pub fn request_resume(&self) -> Result<()> {
        let writer = StatusWriter::new(Some(self.status_dir.clone()))?;
        writer.resume_worker(&self.worker_id)?;
        info!(worker_id = %self.worker_id, "Resume requested");
        Ok(())
    }

    /// Get the worker ID.
    pub fn worker_id(&self) -> &str {
        &self.worker_id
    }

    /// Get the configuration.
    pub fn config(&self) -> &PauseConfig {
        &self.config
    }
}

/// Trait for types that can be paused (workers, managers, etc.).
///
/// This trait allows different worker implementations to integrate
/// pause signal handling consistently.
pub trait Pausable {
    /// Check if the entity is paused.
    fn is_paused(&self) -> Result<bool>;

    /// Wait for the entity to be unpaused.
    fn wait_for_unpause(&self) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Check before claiming a bead - blocks if paused.
    fn check_before_claim(&self) -> impl std::future::Future<Output = Result<bool>> + Send;
}

impl Pausable for PauseSignalHandler {
    fn is_paused(&self) -> Result<bool> {
        PauseSignalHandler::is_paused(self)
    }

    async fn wait_for_unpause(&self) -> Result<()> {
        PauseSignalHandler::wait_for_unpause(self).await
    }

    async fn check_before_claim(&self) -> Result<bool> {
        PauseSignalHandler::check_before_claim(self).await
    }
}

/// Check if any worker with the given ID pattern is paused.
///
/// This is useful for checking multiple workers at once.
pub fn is_any_paused(worker_ids: &[String], status_dir: Option<PathBuf>) -> Result<bool> {
    let status_reader = StatusReader::new(status_dir)?;

    for worker_id in worker_ids {
        if let Some(info) = status_reader.read_worker(worker_id)? {
            if info.status == WorkerStatus::Paused {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

/// Pause all workers with the given IDs.
pub fn pause_all(worker_ids: &[String], status_dir: Option<PathBuf>) -> Result<usize> {
    let status_dir = status_dir
        .map(Ok)
        .unwrap_or_else(StatusReader::default_status_dir)?;
    let writer = StatusWriter::new(Some(status_dir))?;

    let mut count = 0;
    for worker_id in worker_ids {
        writer.pause_worker(worker_id)?;
        count += 1;
    }

    info!(count = count, "Paused workers");
    Ok(count)
}

/// Resume all paused workers with the given IDs.
pub fn resume_all(worker_ids: &[String], status_dir: Option<PathBuf>) -> Result<usize> {
    let status_dir = status_dir
        .map(Ok)
        .unwrap_or_else(StatusReader::default_status_dir)?;
    let reader = StatusReader::new(Some(status_dir.clone()))?;
    let writer = StatusWriter::new(Some(status_dir))?;

    let mut count = 0;
    for worker_id in worker_ids {
        if let Some(info) = reader.read_worker(worker_id)? {
            if info.status == WorkerStatus::Paused {
                writer.resume_worker(worker_id)?;
                count += 1;
            }
        }
    }

    info!(count = count, "Resumed workers");
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_status_file(dir: &std::path::Path, worker_id: &str, content: &str) {
        let path = dir.join(format!("{}.json", worker_id));
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn test_pause_config_default() {
        let config = PauseConfig::default();
        assert_eq!(config.check_interval_secs, DEFAULT_PAUSE_CHECK_INTERVAL_SECS);
        assert_eq!(config.max_wait_secs, 0);
    }

    #[test]
    fn test_pause_config_builder() {
        let config = PauseConfig::default()
            .with_check_interval(30)
            .with_max_wait(3600);

        assert_eq!(config.check_interval_secs, 30);
        assert_eq!(config.max_wait_secs, 3600);
    }

    #[test]
    fn test_handler_creation() {
        let tmp_dir = TempDir::new().unwrap();
        let handler =
            PauseSignalHandler::new("test-worker", Some(tmp_dir.path().to_path_buf())).unwrap();

        assert_eq!(handler.worker_id(), "test-worker");
        assert_eq!(
            handler.config().check_interval_secs,
            DEFAULT_PAUSE_CHECK_INTERVAL_SECS
        );
    }

    #[test]
    fn test_handler_with_config() {
        let tmp_dir = TempDir::new().unwrap();
        let config = PauseConfig::default().with_check_interval(10);
        let handler = PauseSignalHandler::with_config(
            "test-worker",
            Some(tmp_dir.path().to_path_buf()),
            config,
        )
        .unwrap();

        assert_eq!(handler.config().check_interval_secs, 10);
    }

    #[test]
    fn test_is_paused_no_status_file() {
        let tmp_dir = TempDir::new().unwrap();
        let handler =
            PauseSignalHandler::new("nonexistent", Some(tmp_dir.path().to_path_buf())).unwrap();

        // No status file means not paused
        assert!(!handler.is_paused().unwrap());
    }

    #[test]
    fn test_is_paused_active_status() {
        let tmp_dir = TempDir::new().unwrap();
        create_test_status_file(
            tmp_dir.path(),
            "test-worker",
            r#"{"worker_id": "test-worker", "status": "active"}"#,
        );

        let handler =
            PauseSignalHandler::new("test-worker", Some(tmp_dir.path().to_path_buf())).unwrap();
        assert!(!handler.is_paused().unwrap());
    }

    #[test]
    fn test_is_paused_paused_status() {
        let tmp_dir = TempDir::new().unwrap();
        create_test_status_file(
            tmp_dir.path(),
            "test-worker",
            r#"{"worker_id": "test-worker", "status": "paused"}"#,
        );

        let handler =
            PauseSignalHandler::new("test-worker", Some(tmp_dir.path().to_path_buf())).unwrap();
        assert!(handler.is_paused().unwrap());
    }

    #[test]
    fn test_is_paused_idle_status() {
        let tmp_dir = TempDir::new().unwrap();
        create_test_status_file(
            tmp_dir.path(),
            "test-worker",
            r#"{"worker_id": "test-worker", "status": "idle"}"#,
        );

        let handler =
            PauseSignalHandler::new("test-worker", Some(tmp_dir.path().to_path_buf())).unwrap();
        assert!(!handler.is_paused().unwrap());
    }

    #[test]
    fn test_request_pause() {
        let tmp_dir = TempDir::new().unwrap();
        create_test_status_file(
            tmp_dir.path(),
            "test-worker",
            r#"{"worker_id": "test-worker", "status": "active"}"#,
        );

        let handler =
            PauseSignalHandler::new("test-worker", Some(tmp_dir.path().to_path_buf())).unwrap();

        // Request pause
        handler.request_pause().unwrap();

        // Now should be paused
        assert!(handler.is_paused().unwrap());
    }

    #[test]
    fn test_request_resume() {
        let tmp_dir = TempDir::new().unwrap();
        create_test_status_file(
            tmp_dir.path(),
            "test-worker",
            r#"{"worker_id": "test-worker", "status": "paused"}"#,
        );

        let handler =
            PauseSignalHandler::new("test-worker", Some(tmp_dir.path().to_path_buf())).unwrap();
        assert!(handler.is_paused().unwrap());

        // Request resume
        handler.request_resume().unwrap();

        // Now should not be paused
        assert!(!handler.is_paused().unwrap());
    }

    #[tokio::test]
    async fn test_wait_for_unpause_timeout() {
        let tmp_dir = TempDir::new().unwrap();
        create_test_status_file(
            tmp_dir.path(),
            "test-worker",
            r#"{"worker_id": "test-worker", "status": "paused"}"#,
        );

        let config = PauseConfig::default()
            .with_check_interval(1) // 1 second check interval
            .with_max_wait(2); // 2 second timeout

        let handler = PauseSignalHandler::with_config(
            "test-worker",
            Some(tmp_dir.path().to_path_buf()),
            config,
        )
        .unwrap();

        // Should timeout since worker stays paused
        let result = handler.wait_for_unpause().await;
        assert!(result.is_err());

        match result.unwrap_err() {
            ForgeError::Timeout { operation, .. } => {
                assert!(operation.contains("wait_for_unpause"));
            }
            e => panic!("Expected Timeout error, got: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_wait_for_unpause_quick_unpause() {
        let tmp_dir = TempDir::new().unwrap();
        let status_file = tmp_dir.path().join("test-worker.json");

        // Start paused
        std::fs::write(
            &status_file,
            r#"{"worker_id": "test-worker", "status": "paused"}"#,
        )
        .unwrap();

        let config = PauseConfig::default()
            .with_check_interval(1) // 1 second check interval
            .with_max_wait(10); // 10 second timeout

        let handler = PauseSignalHandler::with_config(
            "test-worker",
            Some(tmp_dir.path().to_path_buf()),
            config,
        )
        .unwrap();

        // Spawn task to unpause after short delay
        let status_file_clone = status_file.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            std::fs::write(
                status_file_clone,
                r#"{"worker_id": "test-worker", "status": "idle"}"#,
            )
            .unwrap();
        });

        // Should succeed once unpaused
        let result = handler.wait_for_unpause().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_before_claim_not_paused() {
        let tmp_dir = TempDir::new().unwrap();
        create_test_status_file(
            tmp_dir.path(),
            "test-worker",
            r#"{"worker_id": "test-worker", "status": "active"}"#,
        );

        let handler =
            PauseSignalHandler::new("test-worker", Some(tmp_dir.path().to_path_buf())).unwrap();

        // Should return true immediately (not paused)
        let result = handler.check_before_claim().await;
        assert!(result.unwrap());
    }

    #[test]
    fn test_is_any_paused() {
        let tmp_dir = TempDir::new().unwrap();

        create_test_status_file(
            tmp_dir.path(),
            "worker-1",
            r#"{"worker_id": "worker-1", "status": "active"}"#,
        );
        create_test_status_file(
            tmp_dir.path(),
            "worker-2",
            r#"{"worker_id": "worker-2", "status": "paused"}"#,
        );
        create_test_status_file(
            tmp_dir.path(),
            "worker-3",
            r#"{"worker_id": "worker-3", "status": "idle"}"#,
        );

        let worker_ids = vec![
            "worker-1".to_string(),
            "worker-2".to_string(),
            "worker-3".to_string(),
        ];

        let result = is_any_paused(&worker_ids, Some(tmp_dir.path().to_path_buf())).unwrap();
        assert!(result);

        // Without the paused worker
        let active_workers = vec!["worker-1".to_string(), "worker-3".to_string()];
        let result = is_any_paused(&active_workers, Some(tmp_dir.path().to_path_buf())).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_pause_all() {
        let tmp_dir = TempDir::new().unwrap();

        // Create status directory
        std::fs::create_dir_all(tmp_dir.path()).unwrap();

        let worker_ids = vec!["worker-a".to_string(), "worker-b".to_string()];

        let count = pause_all(&worker_ids, Some(tmp_dir.path().to_path_buf())).unwrap();
        assert_eq!(count, 2);

        // Verify both are paused
        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();

        let w_a = reader.read_worker("worker-a").unwrap().unwrap();
        assert_eq!(w_a.status, WorkerStatus::Paused);

        let w_b = reader.read_worker("worker-b").unwrap().unwrap();
        assert_eq!(w_b.status, WorkerStatus::Paused);
    }

    #[test]
    fn test_resume_all() {
        let tmp_dir = TempDir::new().unwrap();

        // Create paused workers
        create_test_status_file(
            tmp_dir.path(),
            "worker-a",
            r#"{"worker_id": "worker-a", "status": "paused"}"#,
        );
        create_test_status_file(
            tmp_dir.path(),
            "worker-b",
            r#"{"worker_id": "worker-b", "status": "paused"}"#,
        );
        create_test_status_file(
            tmp_dir.path(),
            "worker-c",
            r#"{"worker_id": "worker-c", "status": "active"}"#,
        );

        let worker_ids = vec![
            "worker-a".to_string(),
            "worker-b".to_string(),
            "worker-c".to_string(),
        ];

        // Only paused workers should be resumed
        let count = resume_all(&worker_ids, Some(tmp_dir.path().to_path_buf())).unwrap();
        assert_eq!(count, 2);

        // Verify resumed workers are now idle
        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();

        let w_a = reader.read_worker("worker-a").unwrap().unwrap();
        assert_eq!(w_a.status, WorkerStatus::Idle);

        let w_b = reader.read_worker("worker-b").unwrap().unwrap();
        assert_eq!(w_b.status, WorkerStatus::Idle);

        // Active worker should still be active (not modified)
        let w_c = reader.read_worker("worker-c").unwrap().unwrap();
        assert_eq!(w_c.status, WorkerStatus::Active);
    }
}
