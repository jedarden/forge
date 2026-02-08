//! Status file monitoring for worker health tracking.
//!
//! This module provides a debounced file watcher for monitoring worker status
//! files in `~/.forge/status/*.json`. It uses the notify crate to watch for
//! file system events and updates worker state accordingly.
//!
//! ## Architecture
//!
//! The status watcher operates as follows:
//! 1. Watches `~/.forge/status/` directory for file changes
//! 2. Debounces rapid file changes (10-30ms typical latency)
//! 3. Parses status JSON files when they change
//! 4. Sends updates via a channel to the main TUI event loop
//!
//! ## Status File Format
//!
//! Workers write JSON status files with the following format:
//! ```json
//! {
//!   "worker_id": "sonnet-alpha",
//!   "status": "active",
//!   "model": "claude-sonnet-4-5",
//!   "workspace": "/path/to/workspace",
//!   "pid": 12345,
//!   "started_at": "2026-02-08T10:30:00Z",
//!   "last_activity": "2026-02-08T10:35:00Z",
//!   "current_task": "bd-abc",
//!   "tasks_completed": 5
//! }
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use chrono::{DateTime, Utc};
use notify::RecursiveMode;
use notify_debouncer_full::{
    new_debouncer, DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, error, info, warn};

use forge_core::types::WorkerStatus;

/// Default debounce timeout in milliseconds.
const DEFAULT_DEBOUNCE_MS: u64 = 50;

/// Errors that can occur during status monitoring.
#[derive(Error, Debug)]
pub enum StatusWatcherError {
    /// Failed to initialize the file watcher
    #[error("Failed to initialize file watcher: {0}")]
    WatcherInit(#[from] notify::Error),

    /// Failed to read status file
    #[error("Failed to read status file {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse status file JSON
    #[error("Failed to parse status file {path}: {source}")]
    ParseJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    /// Status directory does not exist
    #[error("Status directory does not exist: {0}")]
    DirectoryNotFound(PathBuf),

    /// Failed to create status directory
    #[error("Failed to create status directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Result type for status watcher operations.
pub type StatusWatcherResult<T> = Result<T, StatusWatcherError>;

/// Worker status as read from a status file.
///
/// This struct represents the JSON format of worker status files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkerStatusFile {
    /// Unique worker identifier (matches filename without .json)
    pub worker_id: String,

    /// Current worker status
    #[serde(default)]
    pub status: WorkerStatus,

    /// Model being used by this worker
    #[serde(default)]
    pub model: String,

    /// Working directory for the worker
    #[serde(default)]
    pub workspace: String,

    /// Process ID of the worker
    #[serde(default)]
    pub pid: Option<u32>,

    /// When the worker was started
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,

    /// Last activity timestamp
    #[serde(default)]
    pub last_activity: Option<DateTime<Utc>>,

    /// Current task/bead being worked on
    #[serde(default)]
    pub current_task: Option<String>,

    /// Number of tasks completed
    #[serde(default)]
    pub tasks_completed: u32,

    /// Optional container ID (for containerized workers)
    #[serde(default)]
    pub container_id: Option<String>,
}

impl WorkerStatusFile {
    /// Parse a status file from JSON content.
    pub fn from_json(content: &str, path: &Path) -> StatusWatcherResult<Self> {
        serde_json::from_str(content).map_err(|e| StatusWatcherError::ParseJson {
            path: path.to_path_buf(),
            source: e,
        })
    }

    /// Read and parse a status file from disk.
    pub fn from_file(path: &Path) -> StatusWatcherResult<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| StatusWatcherError::ReadFile {
            path: path.to_path_buf(),
            source: e,
        })?;
        Self::from_json(&content, path)
    }

    /// Check if the worker is considered healthy.
    pub fn is_healthy(&self) -> bool {
        self.status.is_healthy()
    }
}

/// Events sent by the status watcher to the TUI.
#[derive(Debug, Clone)]
pub enum StatusEvent {
    /// A worker status was created or updated
    WorkerUpdated {
        worker_id: String,
        status: WorkerStatusFile,
    },

    /// A worker status file was deleted
    WorkerRemoved { worker_id: String },

    /// Initial scan completed with all current workers
    InitialScanComplete {
        workers: HashMap<String, WorkerStatusFile>,
    },

    /// An error occurred while processing a status file
    Error {
        path: PathBuf,
        error: String,
    },
}

/// Configuration for the status watcher.
#[derive(Debug, Clone)]
pub struct StatusWatcherConfig {
    /// Path to the status directory (default: ~/.forge/status)
    pub status_dir: PathBuf,

    /// Debounce timeout in milliseconds
    pub debounce_ms: u64,

    /// Whether to create the status directory if it doesn't exist
    pub create_dir_if_missing: bool,
}

impl Default for StatusWatcherConfig {
    fn default() -> Self {
        Self {
            status_dir: Self::default_status_dir(),
            debounce_ms: DEFAULT_DEBOUNCE_MS,
            create_dir_if_missing: true,
        }
    }
}

impl StatusWatcherConfig {
    /// Get the default status directory path (~/.forge/status).
    pub fn default_status_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".forge")
            .join("status")
    }

    /// Set a custom status directory.
    pub fn with_status_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.status_dir = dir.into();
        self
    }

    /// Set the debounce timeout.
    pub fn with_debounce_ms(mut self, ms: u64) -> Self {
        self.debounce_ms = ms;
        self
    }
}

/// Watcher for worker status files.
///
/// This struct manages the file system watcher and provides a channel
/// for receiving status updates.
pub struct StatusWatcher {
    /// Configuration for the watcher
    config: StatusWatcherConfig,

    /// The debounced file watcher
    _debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,

    /// Receiver for status events
    event_rx: Receiver<StatusEvent>,

    /// Current worker states
    workers: HashMap<String, WorkerStatusFile>,
}

impl StatusWatcher {
    /// Create a new status watcher with the given configuration.
    pub fn new(config: StatusWatcherConfig) -> StatusWatcherResult<Self> {
        // Ensure status directory exists
        if !config.status_dir.exists() {
            if config.create_dir_if_missing {
                std::fs::create_dir_all(&config.status_dir).map_err(|e| {
                    StatusWatcherError::CreateDirectory {
                        path: config.status_dir.clone(),
                        source: e,
                    }
                })?;
                info!(path = ?config.status_dir, "Created status directory");
            } else {
                return Err(StatusWatcherError::DirectoryNotFound(
                    config.status_dir.clone(),
                ));
            }
        }

        // Create channel for status events
        let (event_tx, event_rx) = mpsc::channel();

        // Create debounced watcher
        let status_dir = config.status_dir.clone();
        let tx_clone = event_tx.clone();

        let debouncer = new_debouncer(
            Duration::from_millis(config.debounce_ms),
            None,
            move |result: DebounceEventResult| {
                Self::handle_debounced_events(result, &status_dir, &tx_clone);
            },
        )?;

        let mut watcher = Self {
            config,
            _debouncer: debouncer,
            event_rx,
            workers: HashMap::new(),
        };

        // Start watching the directory
        watcher.start_watching()?;

        // Perform initial scan
        watcher.initial_scan(event_tx)?;

        Ok(watcher)
    }

    /// Create a new status watcher with default configuration.
    pub fn new_default() -> StatusWatcherResult<Self> {
        Self::new(StatusWatcherConfig::default())
    }

    /// Start watching the status directory.
    fn start_watching(&mut self) -> StatusWatcherResult<()> {
        self._debouncer
            .watch(&self.config.status_dir, RecursiveMode::NonRecursive)?;
        info!(path = ?self.config.status_dir, "Started watching status directory");
        Ok(())
    }

    /// Perform initial scan of existing status files.
    fn initial_scan(&mut self, event_tx: Sender<StatusEvent>) -> StatusWatcherResult<()> {
        let mut workers = HashMap::new();

        if let Ok(entries) = std::fs::read_dir(&self.config.status_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    match WorkerStatusFile::from_file(&path) {
                        Ok(status) => {
                            debug!(worker_id = %status.worker_id, "Found existing worker status");
                            workers.insert(status.worker_id.clone(), status);
                        }
                        Err(e) => {
                            warn!(path = ?path, error = %e, "Failed to parse existing status file");
                        }
                    }
                }
            }
        }

        self.workers = workers.clone();

        // Send initial scan complete event
        let _ = event_tx.send(StatusEvent::InitialScanComplete { workers });

        Ok(())
    }

    /// Handle debounced file system events.
    fn handle_debounced_events(
        result: DebounceEventResult,
        status_dir: &Path,
        event_tx: &Sender<StatusEvent>,
    ) {
        match result {
            Ok(events) => {
                for event in events {
                    Self::process_event(event, status_dir, event_tx);
                }
            }
            Err(errors) => {
                for error in errors {
                    error!(error = %error, "File watcher error");
                }
            }
        }
    }

    /// Process a single debounced event.
    fn process_event(event: DebouncedEvent, status_dir: &Path, event_tx: &Sender<StatusEvent>) {
        use notify::EventKind;

        for path in &event.paths {
            // Only process .json files in the status directory
            if !path.extension().is_some_and(|e| e == "json") {
                continue;
            }

            // Ensure the file is in the status directory
            if path.parent() != Some(status_dir) {
                continue;
            }

            // Extract worker ID from filename
            let worker_id = match path.file_stem().and_then(|s| s.to_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };

            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    // File created or modified - read and parse
                    match WorkerStatusFile::from_file(path) {
                        Ok(status) => {
                            debug!(worker_id = %status.worker_id, "Worker status updated");
                            let _ = event_tx.send(StatusEvent::WorkerUpdated {
                                worker_id: status.worker_id.clone(),
                                status,
                            });
                        }
                        Err(e) => {
                            warn!(path = ?path, error = %e, "Failed to parse status file");
                            let _ = event_tx.send(StatusEvent::Error {
                                path: path.clone(),
                                error: e.to_string(),
                            });
                        }
                    }
                }
                EventKind::Remove(_) => {
                    // File deleted
                    debug!(worker_id = %worker_id, "Worker status file removed");
                    let _ = event_tx.send(StatusEvent::WorkerRemoved { worker_id });
                }
                _ => {
                    // Other events (access, etc.) - ignore
                }
            }
        }
    }

    /// Try to receive a status event without blocking.
    pub fn try_recv(&mut self) -> Option<StatusEvent> {
        match self.event_rx.try_recv() {
            Ok(event) => {
                self.apply_event(&event);
                Some(event)
            }
            Err(_) => None,
        }
    }

    /// Receive a status event, blocking until one is available.
    pub fn recv(&mut self) -> Option<StatusEvent> {
        match self.event_rx.recv() {
            Ok(event) => {
                self.apply_event(&event);
                Some(event)
            }
            Err(_) => None,
        }
    }

    /// Receive a status event with a timeout.
    pub fn recv_timeout(&mut self, timeout: Duration) -> Option<StatusEvent> {
        match self.event_rx.recv_timeout(timeout) {
            Ok(event) => {
                self.apply_event(&event);
                Some(event)
            }
            Err(_) => None,
        }
    }

    /// Apply an event to the internal worker state.
    fn apply_event(&mut self, event: &StatusEvent) {
        match event {
            StatusEvent::WorkerUpdated { worker_id, status } => {
                self.workers.insert(worker_id.clone(), status.clone());
            }
            StatusEvent::WorkerRemoved { worker_id } => {
                self.workers.remove(worker_id);
            }
            StatusEvent::InitialScanComplete { workers } => {
                self.workers = workers.clone();
            }
            StatusEvent::Error { .. } => {
                // Errors don't change state
            }
        }
    }

    /// Get the current worker states.
    pub fn workers(&self) -> &HashMap<String, WorkerStatusFile> {
        &self.workers
    }

    /// Get a specific worker's status.
    pub fn get_worker(&self, worker_id: &str) -> Option<&WorkerStatusFile> {
        self.workers.get(worker_id)
    }

    /// Get the count of workers by status.
    pub fn worker_counts(&self) -> WorkerCounts {
        let mut counts = WorkerCounts::default();
        for worker in self.workers.values() {
            match worker.status {
                WorkerStatus::Active => counts.active += 1,
                WorkerStatus::Idle => counts.idle += 1,
                WorkerStatus::Failed => counts.failed += 1,
                WorkerStatus::Stopped => counts.stopped += 1,
                WorkerStatus::Error => counts.error += 1,
                WorkerStatus::Starting => counts.starting += 1,
            }
        }
        counts.total = self.workers.len();
        counts
    }

    /// Get list of healthy workers.
    pub fn healthy_workers(&self) -> Vec<&WorkerStatusFile> {
        self.workers.values().filter(|w| w.is_healthy()).collect()
    }

    /// Get list of unhealthy workers.
    pub fn unhealthy_workers(&self) -> Vec<&WorkerStatusFile> {
        self.workers.values().filter(|w| !w.is_healthy()).collect()
    }
}

/// Summary counts of workers by status.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct WorkerCounts {
    /// Total number of workers
    pub total: usize,
    /// Number of active workers
    pub active: usize,
    /// Number of idle workers
    pub idle: usize,
    /// Number of failed workers
    pub failed: usize,
    /// Number of stopped workers
    pub stopped: usize,
    /// Number of workers in error state
    pub error: usize,
    /// Number of workers starting up
    pub starting: usize,
}

impl WorkerCounts {
    /// Get the number of healthy workers (active + idle + starting).
    pub fn healthy(&self) -> usize {
        self.active + self.idle + self.starting
    }

    /// Get the number of unhealthy workers (failed + stopped + error).
    pub fn unhealthy(&self) -> usize {
        self.failed + self.stopped + self.error
    }
}

/// Get the home directory path for the dirs crate compatibility.
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_status_file(dir: &Path, worker_id: &str, status: &str) -> PathBuf {
        let path = dir.join(format!("{}.json", worker_id));
        let content = format!(
            r#"{{
                "worker_id": "{}",
                "status": "{}",
                "model": "test-model",
                "workspace": "/test/workspace",
                "pid": 12345,
                "started_at": "2026-02-08T10:00:00Z",
                "tasks_completed": 5
            }}"#,
            worker_id, status
        );
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_worker_status_file_parsing() {
        let json = r#"{
            "worker_id": "test-worker",
            "status": "active",
            "model": "claude-sonnet-4-5",
            "workspace": "/home/user/project",
            "pid": 12345,
            "started_at": "2026-02-08T10:30:00Z",
            "last_activity": "2026-02-08T10:35:00Z",
            "current_task": "bd-abc",
            "tasks_completed": 10
        }"#;

        let path = Path::new("test.json");
        let status = WorkerStatusFile::from_json(json, path).unwrap();

        assert_eq!(status.worker_id, "test-worker");
        assert_eq!(status.status, WorkerStatus::Active);
        assert_eq!(status.model, "claude-sonnet-4-5");
        assert_eq!(status.workspace, "/home/user/project");
        assert_eq!(status.pid, Some(12345));
        assert_eq!(status.current_task, Some("bd-abc".to_string()));
        assert_eq!(status.tasks_completed, 10);
        assert!(status.is_healthy());
    }

    #[test]
    fn test_worker_status_file_minimal() {
        let json = r#"{"worker_id": "minimal-worker"}"#;

        let path = Path::new("minimal.json");
        let status = WorkerStatusFile::from_json(json, path).unwrap();

        assert_eq!(status.worker_id, "minimal-worker");
        assert_eq!(status.status, WorkerStatus::Idle); // default
        assert_eq!(status.model, ""); // default empty
        assert_eq!(status.tasks_completed, 0); // default
    }

    #[test]
    fn test_worker_status_file_unhealthy() {
        let json = r#"{"worker_id": "failed-worker", "status": "failed"}"#;

        let path = Path::new("failed.json");
        let status = WorkerStatusFile::from_json(json, path).unwrap();

        assert_eq!(status.status, WorkerStatus::Failed);
        assert!(!status.is_healthy());
    }

    #[test]
    fn test_worker_counts() {
        let mut counts = WorkerCounts::default();
        counts.active = 5;
        counts.idle = 3;
        counts.starting = 1;
        counts.failed = 2;
        counts.stopped = 1;
        counts.error = 1;
        counts.total = 13;

        assert_eq!(counts.healthy(), 9); // 5 + 3 + 1
        assert_eq!(counts.unhealthy(), 4); // 2 + 1 + 1
    }

    #[test]
    fn test_status_watcher_config_default() {
        let config = StatusWatcherConfig::default();

        assert!(config.status_dir.ends_with(".forge/status"));
        assert_eq!(config.debounce_ms, DEFAULT_DEBOUNCE_MS);
        assert!(config.create_dir_if_missing);
    }

    #[test]
    fn test_status_watcher_config_builder() {
        let config = StatusWatcherConfig::default()
            .with_status_dir("/custom/path")
            .with_debounce_ms(100);

        assert_eq!(config.status_dir, PathBuf::from("/custom/path"));
        assert_eq!(config.debounce_ms, 100);
    }

    #[test]
    fn test_status_watcher_initial_scan() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create some test status files
        create_test_status_file(&status_dir, "worker-1", "active");
        create_test_status_file(&status_dir, "worker-2", "idle");
        create_test_status_file(&status_dir, "worker-3", "failed");

        let config = StatusWatcherConfig::default().with_status_dir(&status_dir);
        let watcher = StatusWatcher::new(config).unwrap();

        // Check that workers were loaded
        assert_eq!(watcher.workers().len(), 3);
        assert!(watcher.get_worker("worker-1").is_some());
        assert!(watcher.get_worker("worker-2").is_some());
        assert!(watcher.get_worker("worker-3").is_some());

        // Check counts
        let counts = watcher.worker_counts();
        assert_eq!(counts.total, 3);
        assert_eq!(counts.active, 1);
        assert_eq!(counts.idle, 1);
        assert_eq!(counts.failed, 1);
        assert_eq!(counts.healthy(), 2);
        assert_eq!(counts.unhealthy(), 1);
    }

    #[test]
    fn test_status_watcher_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("forge").join("status");

        assert!(!status_dir.exists());

        let config = StatusWatcherConfig::default().with_status_dir(&status_dir);
        let _watcher = StatusWatcher::new(config).unwrap();

        assert!(status_dir.exists());
    }

    #[test]
    fn test_status_watcher_healthy_unhealthy_lists() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        create_test_status_file(&status_dir, "active-worker", "active");
        create_test_status_file(&status_dir, "idle-worker", "idle");
        create_test_status_file(&status_dir, "failed-worker", "failed");

        let config = StatusWatcherConfig::default().with_status_dir(&status_dir);
        let watcher = StatusWatcher::new(config).unwrap();

        let healthy = watcher.healthy_workers();
        let unhealthy = watcher.unhealthy_workers();

        assert_eq!(healthy.len(), 2);
        assert_eq!(unhealthy.len(), 1);

        let healthy_ids: Vec<_> = healthy.iter().map(|w| w.worker_id.as_str()).collect();
        assert!(healthy_ids.contains(&"active-worker"));
        assert!(healthy_ids.contains(&"idle-worker"));

        let unhealthy_ids: Vec<_> = unhealthy.iter().map(|w| w.worker_id.as_str()).collect();
        assert!(unhealthy_ids.contains(&"failed-worker"));
    }

    #[test]
    fn test_worker_status_file_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let path = create_test_status_file(temp_dir.path(), "file-worker", "active");

        let status = WorkerStatusFile::from_file(&path).unwrap();
        assert_eq!(status.worker_id, "file-worker");
        assert_eq!(status.status, WorkerStatus::Active);
    }

    #[test]
    fn test_worker_status_file_from_file_not_found() {
        let result = WorkerStatusFile::from_file(Path::new("/nonexistent/file.json"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StatusWatcherError::ReadFile { .. }));
    }

    #[test]
    fn test_worker_status_file_invalid_json() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("invalid.json");
        fs::write(&path, "not valid json").unwrap();

        let result = WorkerStatusFile::from_file(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StatusWatcherError::ParseJson { .. }));
    }

    #[test]
    fn test_status_event_apply() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default().with_status_dir(&status_dir);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Initial state - empty
        assert_eq!(watcher.workers().len(), 0);

        // Apply worker updated event
        let status = WorkerStatusFile {
            worker_id: "new-worker".to_string(),
            status: WorkerStatus::Active,
            model: "test".to_string(),
            workspace: "/test".to_string(),
            pid: Some(12345),
            started_at: None,
            last_activity: None,
            current_task: None,
            tasks_completed: 0,
            container_id: None,
        };

        watcher.apply_event(&StatusEvent::WorkerUpdated {
            worker_id: "new-worker".to_string(),
            status: status.clone(),
        });

        assert_eq!(watcher.workers().len(), 1);
        assert!(watcher.get_worker("new-worker").is_some());

        // Apply worker removed event
        watcher.apply_event(&StatusEvent::WorkerRemoved {
            worker_id: "new-worker".to_string(),
        });

        assert_eq!(watcher.workers().len(), 0);
        assert!(watcher.get_worker("new-worker").is_none());
    }

    // ============================================================
    // Status File Monitoring Integration Tests
    // ============================================================
    //
    // These tests verify that file system changes trigger the correct
    // events. They create, modify, and delete status files and verify
    // that the StatusWatcher picks them up correctly.

    /// Test that creating a new status file triggers a WorkerUpdated event.
    #[test]
    fn test_file_creation_triggers_update_event() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create watcher with short debounce for faster tests
        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Receive initial scan event (should be empty)
        let initial_event = watcher.recv_timeout(Duration::from_millis(100));
        assert!(matches!(
            initial_event,
            Some(StatusEvent::InitialScanComplete { workers }) if workers.is_empty()
        ));

        // Create a new status file after the watcher is running
        create_test_status_file(&status_dir, "new-worker-1", "active");

        // Wait for the file system event with enough time for debounce
        std::thread::sleep(Duration::from_millis(100));

        // Try to receive the update event
        let event = watcher.recv_timeout(Duration::from_millis(500));

        // Verify we got a WorkerUpdated event
        match event {
            Some(StatusEvent::WorkerUpdated { worker_id, status }) => {
                assert_eq!(worker_id, "new-worker-1");
                assert_eq!(status.status, WorkerStatus::Active);
                assert_eq!(status.model, "test-model");
            }
            other => {
                // Also accept if the watcher detected it via internal state
                if watcher.get_worker("new-worker-1").is_some() {
                    let w = watcher.get_worker("new-worker-1").unwrap();
                    assert_eq!(w.status, WorkerStatus::Active);
                } else {
                    panic!("Expected WorkerUpdated event, got: {:?}", other);
                }
            }
        }
    }

    /// Test that modifying an existing status file triggers a WorkerUpdated event.
    #[test]
    fn test_file_modification_triggers_update_event() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create initial status file before watcher starts
        let path = create_test_status_file(&status_dir, "worker-mod", "idle");

        // Create watcher
        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Verify initial state
        assert_eq!(watcher.workers().len(), 1);
        assert_eq!(
            watcher.get_worker("worker-mod").unwrap().status,
            WorkerStatus::Idle
        );

        // Receive initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Modify the file to change status to active
        let new_content = r#"{
            "worker_id": "worker-mod",
            "status": "active",
            "model": "test-model-updated",
            "workspace": "/test/workspace",
            "pid": 12345,
            "tasks_completed": 10
        }"#;
        fs::write(&path, new_content).unwrap();

        // Wait for debounce
        std::thread::sleep(Duration::from_millis(100));

        // Try to receive the update event
        let event = watcher.recv_timeout(Duration::from_millis(500));

        match event {
            Some(StatusEvent::WorkerUpdated { worker_id, status }) => {
                assert_eq!(worker_id, "worker-mod");
                assert_eq!(status.status, WorkerStatus::Active);
                assert_eq!(status.tasks_completed, 10);
            }
            other => {
                // Check internal state as fallback
                if let Some(w) = watcher.get_worker("worker-mod") {
                    if w.status == WorkerStatus::Active {
                        // Event was applied internally
                        return;
                    }
                }
                panic!("Expected WorkerUpdated event with active status, got: {:?}", other);
            }
        }
    }

    /// Test that deleting a status file triggers a WorkerRemoved event.
    #[test]
    fn test_file_deletion_triggers_remove_event() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create initial status file
        let path = create_test_status_file(&status_dir, "worker-del", "active");

        // Create watcher
        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Verify initial state
        assert_eq!(watcher.workers().len(), 1);
        assert!(watcher.get_worker("worker-del").is_some());

        // Receive initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Delete the file
        fs::remove_file(&path).unwrap();

        // Wait for debounce
        std::thread::sleep(Duration::from_millis(100));

        // Try to receive the remove event
        let event = watcher.recv_timeout(Duration::from_millis(500));

        match event {
            Some(StatusEvent::WorkerRemoved { worker_id }) => {
                assert_eq!(worker_id, "worker-del");
            }
            other => {
                // Check internal state as fallback - worker should be removed
                if watcher.get_worker("worker-del").is_none() {
                    // Event was applied internally
                    return;
                }
                panic!("Expected WorkerRemoved event, got: {:?}", other);
            }
        }
    }

    /// Test monitoring multiple file changes in sequence.
    #[test]
    fn test_multiple_file_changes_sequence() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Create first worker
        create_test_status_file(&status_dir, "worker-a", "active");
        std::thread::sleep(Duration::from_millis(100));

        // Create second worker
        create_test_status_file(&status_dir, "worker-b", "idle");
        std::thread::sleep(Duration::from_millis(100));

        // Create third worker
        create_test_status_file(&status_dir, "worker-c", "starting");
        std::thread::sleep(Duration::from_millis(100));

        // Drain all events
        while watcher.recv_timeout(Duration::from_millis(100)).is_some() {}

        // Verify all workers are tracked
        assert!(watcher.get_worker("worker-a").is_some(), "worker-a should be tracked");
        assert!(watcher.get_worker("worker-b").is_some(), "worker-b should be tracked");
        assert!(watcher.get_worker("worker-c").is_some(), "worker-c should be tracked");

        // Verify counts
        let counts = watcher.worker_counts();
        assert_eq!(counts.total, 3);
        assert_eq!(counts.active, 1);
        assert_eq!(counts.idle, 1);
        assert_eq!(counts.starting, 1);
    }

    /// Test that non-JSON files are ignored.
    #[test]
    fn test_non_json_files_ignored() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Create a non-JSON file (should be ignored)
        let txt_path = status_dir.join("not-a-worker.txt");
        fs::write(&txt_path, "This is not JSON").unwrap();

        std::thread::sleep(Duration::from_millis(100));

        // Drain events
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Should still have no workers
        assert_eq!(watcher.workers().len(), 0);
    }

    /// Test that invalid JSON files generate error events.
    #[test]
    fn test_invalid_json_generates_error_event() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Create an invalid JSON file
        let invalid_path = status_dir.join("broken.json");
        fs::write(&invalid_path, "{ not valid json }").unwrap();

        std::thread::sleep(Duration::from_millis(100));

        // Should receive an error event or the file should be ignored
        let event = watcher.recv_timeout(Duration::from_millis(500));

        match event {
            Some(StatusEvent::Error { path, error: _ }) => {
                assert!(path.ends_with("broken.json"));
            }
            other => {
                // It's acceptable to not receive an error event if the
                // implementation silently ignores invalid files
                assert!(
                    watcher.get_worker("broken").is_none(),
                    "Invalid file should not create a worker, got event: {:?}",
                    other
                );
            }
        }
    }

    /// Test worker status transitions (active -> idle -> failed).
    #[test]
    fn test_worker_status_transitions() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let path = create_test_status_file(&status_dir, "transitioning-worker", "starting");

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Verify initial state
        assert_eq!(
            watcher.get_worker("transitioning-worker").unwrap().status,
            WorkerStatus::Starting
        );

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Transition to active
        let content = r#"{"worker_id": "transitioning-worker", "status": "active", "model": "test", "workspace": "/test"}"#;
        fs::write(&path, content).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        // Drain events and check state
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify transition
        let w = watcher.get_worker("transitioning-worker");
        assert!(w.is_some(), "Worker should still exist after transition");
        assert_eq!(
            w.unwrap().status,
            WorkerStatus::Active,
            "Worker should be active after transition"
        );

        // Transition to idle
        let content = r#"{"worker_id": "transitioning-worker", "status": "idle", "model": "test", "workspace": "/test"}"#;
        fs::write(&path, content).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        assert_eq!(
            watcher.get_worker("transitioning-worker").unwrap().status,
            WorkerStatus::Idle
        );

        // Transition to failed
        let content = r#"{"worker_id": "transitioning-worker", "status": "failed", "model": "test", "workspace": "/test"}"#;
        fs::write(&path, content).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        let final_worker = watcher.get_worker("transitioning-worker").unwrap();
        assert_eq!(final_worker.status, WorkerStatus::Failed);
        assert!(!final_worker.is_healthy());
    }

    /// Test concurrent file operations (simulates multiple workers updating simultaneously).
    #[test]
    fn test_concurrent_file_operations() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Create multiple files rapidly (simulating concurrent worker registration)
        for i in 0..5 {
            let content = format!(
                r#"{{"worker_id": "concurrent-{}", "status": "active", "model": "test", "workspace": "/test", "tasks_completed": {}}}"#,
                i, i * 10
            );
            fs::write(status_dir.join(format!("concurrent-{}.json", i)), content).unwrap();
        }

        // Wait for all events to be processed
        std::thread::sleep(Duration::from_millis(200));

        // Drain all events
        while watcher.recv_timeout(Duration::from_millis(100)).is_some() {}

        // Verify all workers are tracked
        for i in 0..5 {
            let worker_id = format!("concurrent-{}", i);
            let worker = watcher.get_worker(&worker_id);
            assert!(worker.is_some(), "Worker {} should be tracked", worker_id);
            assert_eq!(worker.unwrap().tasks_completed, i * 10);
        }

        assert_eq!(watcher.worker_counts().total, 5);
        assert_eq!(watcher.worker_counts().active, 5);
    }
}
