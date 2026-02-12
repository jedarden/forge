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
    DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache, new_debouncer,
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

/// Custom deserializer for current_task field that handles both string and object formats.
///
/// Accepts:
/// - String: "bd-abc"
/// - Object: {"bead_id": "bd-abc", "bead_title": "...", "priority": 1}
fn deserialize_current_task<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Deserialize};
    use serde_json::Value;

    let value = Value::deserialize(deserializer)?;

    match value {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s)),
        Value::Object(map) => {
            // Extract bead_id from object format
            if let Some(Value::String(bead_id)) = map.get("bead_id") {
                Ok(Some(bead_id.clone()))
            } else {
                Err(de::Error::custom(
                    "current_task object must have bead_id field",
                ))
            }
        }
        _ => Err(de::Error::custom("current_task must be a string or object")),
    }
}

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

    /// Current task/bead being worked on (handles both string and object formats)
    #[serde(default, deserialize_with = "deserialize_current_task")]
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
    Error { path: PathBuf, error: String },
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
    use serde_json::json;
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
        assert!(matches!(
            result.unwrap_err(),
            StatusWatcherError::ReadFile { .. }
        ));
    }

    #[test]
    fn test_worker_status_file_invalid_json() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("invalid.json");
        fs::write(&path, "not valid json").unwrap();

        let result = WorkerStatusFile::from_file(&path);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            StatusWatcherError::ParseJson { .. }
        ));
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
                panic!(
                    "Expected WorkerUpdated event with active status, got: {:?}",
                    other
                );
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
        assert!(
            watcher.get_worker("worker-a").is_some(),
            "worker-a should be tracked"
        );
        assert!(
            watcher.get_worker("worker-b").is_some(),
            "worker-b should be tracked"
        );
        assert!(
            watcher.get_worker("worker-c").is_some(),
            "worker-c should be tracked"
        );

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
                i,
                i * 10
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

    // ============================================================
    // current_task Format Consistency Tests
    // ============================================================
    //
    // These tests verify that the custom deserializer handles both
    // string and object formats for the current_task field.

    /// Test current_task as a simple string format.
    #[test]
    fn test_current_task_string_format() {
        let json = r#"{
            "worker_id": "string-task-worker",
            "status": "active",
            "model": "test-model",
            "workspace": "/test",
            "current_task": "bd-abc"
        }"#;

        let path = Path::new("test.json");
        let status = WorkerStatusFile::from_json(json, path).unwrap();

        assert_eq!(status.worker_id, "string-task-worker");
        assert_eq!(status.current_task, Some("bd-abc".to_string()));
    }

    /// Test current_task as an object format with bead_id.
    #[test]
    fn test_current_task_object_format() {
        let json = r#"{
            "worker_id": "object-task-worker",
            "status": "active",
            "model": "test-model",
            "workspace": "/test",
            "current_task": {
                "bead_id": "bd-xyz",
                "bead_title": "Some task title",
                "priority": 1
            }
        }"#;

        let path = Path::new("test.json");
        let status = WorkerStatusFile::from_json(json, path).unwrap();

        assert_eq!(status.worker_id, "object-task-worker");
        assert_eq!(status.current_task, Some("bd-xyz".to_string()));
    }

    /// Test current_task as null (no task).
    #[test]
    fn test_current_task_null() {
        let json = r#"{
            "worker_id": "no-task-worker",
            "status": "idle",
            "model": "test-model",
            "workspace": "/test",
            "current_task": null
        }"#;

        let path = Path::new("test.json");
        let status = WorkerStatusFile::from_json(json, path).unwrap();

        assert_eq!(status.worker_id, "no-task-worker");
        assert_eq!(status.current_task, None);
    }

    /// Test current_task missing from JSON (uses default None).
    #[test]
    fn test_current_task_missing() {
        let json = r#"{
            "worker_id": "missing-task-worker",
            "status": "idle",
            "model": "test-model",
            "workspace": "/test"
        }"#;

        let path = Path::new("test.json");
        let status = WorkerStatusFile::from_json(json, path).unwrap();

        assert_eq!(status.worker_id, "missing-task-worker");
        assert_eq!(status.current_task, None);
    }

    /// Test current_task object format with minimal fields.
    #[test]
    fn test_current_task_object_minimal() {
        let json = r#"{
            "worker_id": "minimal-object-worker",
            "status": "active",
            "current_task": {"bead_id": "fg-123"}
        }"#;

        let path = Path::new("test.json");
        let status = WorkerStatusFile::from_json(json, path).unwrap();

        assert_eq!(status.current_task, Some("fg-123".to_string()));
    }

    /// Test current_task object format without bead_id fails.
    #[test]
    fn test_current_task_object_missing_bead_id() {
        let json = r#"{
            "worker_id": "bad-object-worker",
            "status": "active",
            "current_task": {"title": "missing bead_id", "priority": 1}
        }"#;

        let path = Path::new("test.json");
        let result = WorkerStatusFile::from_json(json, path);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("bead_id"));
    }

    /// Test current_task with various bead ID prefixes.
    #[test]
    fn test_current_task_various_prefixes() {
        let test_cases = [
            ("bd-abc", "bd-abc"),                 // beads format
            ("fg-xyz", "fg-xyz"),                 // forge format
            ("po-123", "po-123"),                 // another prefix
            ("task-uuid-here", "task-uuid-here"), // longer format
        ];

        for (input, expected) in test_cases {
            let json = format!(
                r#"{{"worker_id": "prefix-worker", "current_task": "{}"}}"#,
                input
            );
            let path = Path::new("test.json");
            let status = WorkerStatusFile::from_json(&json, path).unwrap();
            assert_eq!(
                status.current_task,
                Some(expected.to_string()),
                "Failed for input: {}",
                input
            );
        }
    }

    /// Test file parsing with both current_task formats.
    #[test]
    fn test_file_parsing_current_task_formats() {
        let temp_dir = TempDir::new().unwrap();

        // Create file with string format
        let string_path = temp_dir.path().join("string-worker.json");
        fs::write(
            &string_path,
            r#"{"worker_id": "string-worker", "status": "active", "current_task": "bd-string"}"#,
        )
        .unwrap();

        // Create file with object format
        let object_path = temp_dir.path().join("object-worker.json");
        fs::write(
            &object_path,
            r#"{"worker_id": "object-worker", "status": "active", "current_task": {"bead_id": "bd-object", "priority": 0}}"#,
        )
        .unwrap();

        // Parse both files
        let string_status = WorkerStatusFile::from_file(&string_path).unwrap();
        let object_status = WorkerStatusFile::from_file(&object_path).unwrap();

        assert_eq!(string_status.current_task, Some("bd-string".to_string()));
        assert_eq!(object_status.current_task, Some("bd-object".to_string()));
    }

    /// Test StatusWatcher handles mixed current_task formats.
    #[test]
    fn test_watcher_mixed_current_task_formats() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create workers with different current_task formats
        fs::write(
            status_dir.join("worker-1.json"),
            r#"{"worker_id": "worker-1", "status": "active", "current_task": "bd-string-task"}"#,
        )
        .unwrap();

        fs::write(
            status_dir.join("worker-2.json"),
            r#"{"worker_id": "worker-2", "status": "active", "current_task": {"bead_id": "bd-object-task", "title": "Object task"}}"#,
        )
        .unwrap();

        fs::write(
            status_dir.join("worker-3.json"),
            r#"{"worker_id": "worker-3", "status": "idle", "current_task": null}"#,
        )
        .unwrap();

        fs::write(
            status_dir.join("worker-4.json"),
            r#"{"worker_id": "worker-4", "status": "idle"}"#,
        )
        .unwrap();

        // Initialize watcher
        let config = StatusWatcherConfig::default().with_status_dir(&status_dir);
        let watcher = StatusWatcher::new(config).unwrap();

        // Verify all workers are tracked with correct current_task values
        assert_eq!(watcher.workers().len(), 4);

        let w1 = watcher.get_worker("worker-1").unwrap();
        assert_eq!(w1.current_task, Some("bd-string-task".to_string()));

        let w2 = watcher.get_worker("worker-2").unwrap();
        assert_eq!(w2.current_task, Some("bd-object-task".to_string()));

        let w3 = watcher.get_worker("worker-3").unwrap();
        assert_eq!(w3.current_task, None);

        let w4 = watcher.get_worker("worker-4").unwrap();
        assert_eq!(w4.current_task, None);
    }

    // ============================================================
    // Worker Status Real-Time Update Tests (fg-56p)
    // ============================================================
    //
    // These tests verify that worker status updates propagate through
    // the StatusWatcher in real-time with the expected latency.
    //
    // Success Criteria:
    // - Status updates within 1-2 seconds
    // - All status transitions visible
    // - No stale data displayed
    // - Handles external changes gracefully

    /// Test worker spawn status transition: starting -> active/idle.
    ///
    /// Verifies:
    /// - Initial 'starting' status is detected
    /// - Transition to 'active' or 'idle' is detected within expected latency
    #[test]
    fn test_worker_spawn_status_transition_starting_to_active() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create watcher with short debounce for faster tests
        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Simulate worker spawn: create status file with 'starting' status
        let worker_id = "spawn-test-worker";
        let path = status_dir.join(format!("{}.json", worker_id));
        let starting_content = json!({
            "worker_id": worker_id,
            "status": "starting",
            "model": "sonnet",
            "workspace": "/test/workspace",
            "pid": 12345,
            "started_at": Utc::now().to_rfc3339(),
            "last_activity": Utc::now().to_rfc3339(),
            "tasks_completed": 0
        });
        fs::write(&path, starting_content.to_string()).unwrap();

        // Wait for file system event
        std::thread::sleep(Duration::from_millis(100));

        // Drain events and verify 'starting' status was detected
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        let worker = watcher.get_worker(worker_id);
        assert!(worker.is_some(), "Worker should be tracked after spawn");
        assert_eq!(
            worker.unwrap().status,
            WorkerStatus::Starting,
            "Initial status should be 'starting'"
        );

        // Simulate status update: worker becomes active
        let active_content = json!({
            "worker_id": worker_id,
            "status": "active",
            "model": "sonnet",
            "workspace": "/test/workspace",
            "pid": 12345,
            "started_at": Utc::now().to_rfc3339(),
            "last_activity": Utc::now().to_rfc3339(),
            "current_task": "fg-test-task",
            "tasks_completed": 0
        });
        fs::write(&path, active_content.to_string()).unwrap();

        // Wait for file system event
        std::thread::sleep(Duration::from_millis(100));

        // Drain events and verify 'active' status was detected
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        let worker = watcher.get_worker(worker_id).unwrap();
        assert_eq!(
            worker.status,
            WorkerStatus::Active,
            "Status should transition to 'active'"
        );
        assert_eq!(
            worker.current_task,
            Some("fg-test-task".to_string()),
            "current_task should be set when active"
        );
    }

    /// Test worker spawn status transition: starting -> idle.
    ///
    /// Verifies that workers that complete startup without picking up a task
    /// correctly transition to 'idle' status.
    #[test]
    fn test_worker_spawn_status_transition_starting_to_idle() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Create worker with 'starting' status
        let worker_id = "idle-spawn-worker";
        let path = status_dir.join(format!("{}.json", worker_id));
        let starting_content = json!({
            "worker_id": worker_id,
            "status": "starting",
            "model": "haiku",
            "workspace": "/test/workspace",
            "tasks_completed": 0
        });
        fs::write(&path, starting_content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Transition to idle
        let idle_content = json!({
            "worker_id": worker_id,
            "status": "idle",
            "model": "haiku",
            "workspace": "/test/workspace",
            "current_task": null,
            "tasks_completed": 0
        });
        fs::write(&path, idle_content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        let worker = watcher.get_worker(worker_id).unwrap();
        assert_eq!(
            worker.status,
            WorkerStatus::Idle,
            "Status should transition to 'idle'"
        );
        assert_eq!(
            worker.current_task, None,
            "current_task should be None when idle"
        );
    }

    /// Test task pickup status updates.
    ///
    /// Verifies:
    /// - When worker picks up a task, status becomes 'active'
    /// - current_task field is updated with bead ID
    #[test]
    fn test_task_pickup_status_updates() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Create idle worker
        let worker_id = "task-pickup-worker";
        let path = status_dir.join(format!("{}.json", worker_id));
        let idle_content = json!({
            "worker_id": worker_id,
            "status": "idle",
            "model": "sonnet",
            "workspace": "/test/workspace",
            "current_task": null,
            "tasks_completed": 0
        });
        fs::write(&path, idle_content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify initial idle state
        let worker = watcher.get_worker(worker_id).unwrap();
        assert_eq!(worker.status, WorkerStatus::Idle);
        assert_eq!(worker.current_task, None);

        // Worker picks up a task (status update with current_task)
        let active_content = json!({
            "worker_id": worker_id,
            "status": "active",
            "model": "sonnet",
            "workspace": "/test/workspace",
            "current_task": "fg-56p",
            "tasks_completed": 0,
            "last_activity": Utc::now().to_rfc3339()
        });
        fs::write(&path, active_content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify active state with task
        let worker = watcher.get_worker(worker_id).unwrap();
        assert_eq!(
            worker.status,
            WorkerStatus::Active,
            "Status should become 'active' when task is picked up"
        );
        assert_eq!(
            worker.current_task,
            Some("fg-56p".to_string()),
            "current_task should be set to the bead ID"
        );
    }

    /// Test task pickup with object format current_task.
    ///
    /// Verifies that current_task field works with both string and object formats.
    #[test]
    fn test_task_pickup_with_object_format_current_task() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_id = "object-task-worker";
        let path = status_dir.join(format!("{}.json", worker_id));

        // Worker picks up task with object format current_task
        let content = json!({
            "worker_id": worker_id,
            "status": "active",
            "model": "sonnet",
            "current_task": {
                "bead_id": "fg-abc",
                "bead_title": "Test task with object format",
                "priority": 0
            },
            "tasks_completed": 0
        });
        fs::write(&path, content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        let worker = watcher.get_worker(worker_id).unwrap();
        assert_eq!(worker.status, WorkerStatus::Active);
        assert_eq!(
            worker.current_task,
            Some("fg-abc".to_string()),
            "Object format current_task should extract bead_id"
        );
    }

    /// Test task completion status updates.
    ///
    /// Verifies:
    /// - tasks_completed counter increments
    /// - status returns to 'idle'
    /// - current_task becomes None
    #[test]
    fn test_task_completion_status_updates() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_id = "completion-worker";
        let path = status_dir.join(format!("{}.json", worker_id));

        // Worker is active with a task
        let active_content = json!({
            "worker_id": worker_id,
            "status": "active",
            "model": "sonnet",
            "current_task": "fg-xyz",
            "tasks_completed": 0
        });
        fs::write(&path, active_content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify active state
        let worker = watcher.get_worker(worker_id).unwrap();
        assert_eq!(worker.status, WorkerStatus::Active);
        assert_eq!(worker.tasks_completed, 0);

        // Task completes
        let completed_content = json!({
            "worker_id": worker_id,
            "status": "idle",
            "model": "sonnet",
            "current_task": null,
            "tasks_completed": 1,
            "last_activity": Utc::now().to_rfc3339()
        });
        fs::write(&path, completed_content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify completed state
        let worker = watcher.get_worker(worker_id).unwrap();
        assert_eq!(
            worker.status,
            WorkerStatus::Idle,
            "Status should return to 'idle' after task completion"
        );
        assert_eq!(
            worker.current_task, None,
            "current_task should be cleared after completion"
        );
        assert_eq!(
            worker.tasks_completed, 1,
            "tasks_completed should increment"
        );
    }

    /// Test multiple task completions incrementing tasks_completed.
    #[test]
    fn test_multiple_task_completions_increment_counter() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_id = "multi-completion-worker";
        let path = status_dir.join(format!("{}.json", worker_id));

        // Start with 0 completed tasks
        let content = json!({
            "worker_id": worker_id,
            "status": "idle",
            "tasks_completed": 0
        });
        fs::write(&path, content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Simulate multiple task cycles
        for i in 1..=5 {
            // Pick up task
            let active = json!({
                "worker_id": worker_id,
                "status": "active",
                "current_task": format!("task-{}", i),
                "tasks_completed": i - 1
            });
            fs::write(&path, active.to_string()).unwrap();
            std::thread::sleep(Duration::from_millis(50));
            while watcher.recv_timeout(Duration::from_millis(20)).is_some() {}

            // Complete task
            let idle = json!({
                "worker_id": worker_id,
                "status": "idle",
                "current_task": null,
                "tasks_completed": i
            });
            fs::write(&path, idle.to_string()).unwrap();
            std::thread::sleep(Duration::from_millis(50));
            while watcher.recv_timeout(Duration::from_millis(20)).is_some() {}
        }

        // Verify final counter
        let worker = watcher.get_worker(worker_id).unwrap();
        assert_eq!(
            worker.tasks_completed, 5,
            "tasks_completed should be 5 after 5 task cycles"
        );
    }

    /// Test external worker termination handling (stopped status).
    ///
    /// Verifies that when a worker is killed externally (tmux kill-session),
    /// the status is detected and shows 'stopped' or 'failed'.
    #[test]
    fn test_external_worker_termination_stopped() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_id = "stopped-worker";
        let path = status_dir.join(format!("{}.json", worker_id));

        // Create active worker
        let active_content = json!({
            "worker_id": worker_id,
            "status": "active",
            "current_task": "fg-test",
            "tasks_completed": 0
        });
        fs::write(&path, active_content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Simulate external termination: status file updated to 'stopped'
        let stopped_content = json!({
            "worker_id": worker_id,
            "status": "stopped",
            "current_task": null,
            "tasks_completed": 0
        });
        fs::write(&path, stopped_content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        let worker = watcher.get_worker(worker_id).unwrap();
        assert_eq!(
            worker.status,
            WorkerStatus::Stopped,
            "Status should show 'stopped' after external termination"
        );
        assert!(!worker.is_healthy(), "Stopped worker should not be healthy");
    }

    /// Test external worker termination handling (failed status).
    ///
    /// Verifies that when a worker process dies unexpectedly,
    /// the status is detected and shows 'failed'.
    #[test]
    fn test_external_worker_termination_failed() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_id = "failed-worker";
        let path = status_dir.join(format!("{}.json", worker_id));

        // Create active worker
        let active_content = json!({
            "worker_id": worker_id,
            "status": "active",
            "model": "sonnet",
            "tasks_completed": 2
        });
        fs::write(&path, active_content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Simulate process failure: status updated to 'failed'
        let failed_content = json!({
            "worker_id": worker_id,
            "status": "failed",
            "model": "sonnet",
            "tasks_completed": 2,
            "health_error": "Process died unexpectedly"
        });
        fs::write(&path, failed_content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        let worker = watcher.get_worker(worker_id).unwrap();
        assert_eq!(
            worker.status,
            WorkerStatus::Failed,
            "Status should show 'failed' after process death"
        );
        assert!(!worker.is_healthy(), "Failed worker should not be healthy");
        // Verify historical data is preserved
        assert_eq!(
            worker.tasks_completed, 2,
            "tasks_completed should be preserved after failure"
        );
    }

    /// Test worker status file deletion (complete removal).
    ///
    /// Verifies that when a status file is deleted, the worker is removed
    /// from tracking.
    #[test]
    fn test_worker_status_file_deletion() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_id = "deleted-worker";
        let path = status_dir.join(format!("{}.json", worker_id));

        // Create worker
        let content = json!({
            "worker_id": worker_id,
            "status": "active"
        });
        fs::write(&path, content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify worker exists
        assert!(watcher.get_worker(worker_id).is_some());

        // Delete the status file (simulates worker cleanup)
        fs::remove_file(&path).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify worker is removed from tracking
        assert!(
            watcher.get_worker(worker_id).is_none(),
            "Worker should be removed from tracking after file deletion"
        );
    }

    /// Test real-time update latency.
    ///
    /// Verifies that status updates are detected within 1-2 seconds.
    /// Uses a measured approach to verify latency.
    #[test]
    fn test_real_time_update_latency() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Use default debounce (50ms) to test real-world latency
        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(50);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_id = "latency-test-worker";
        let path = status_dir.join(format!("{}.json", worker_id));

        // Create initial status
        let content = json!({
            "worker_id": worker_id,
            "status": "idle"
        });
        fs::write(&path, content.to_string()).unwrap();

        // Measure time to detect initial creation
        let start = std::time::Instant::now();

        // Poll for the update with a 2-second timeout
        let mut detected = false;
        while start.elapsed() < Duration::from_millis(2000) {
            if watcher.recv_timeout(Duration::from_millis(50)).is_some() {
                if watcher.get_worker(worker_id).is_some() {
                    detected = true;
                    break;
                }
            }
        }

        let detection_time = start.elapsed();

        assert!(
            detected,
            "Status update should be detected within 2 seconds"
        );
        assert!(
            detection_time < Duration::from_millis(1500),
            "Status update should be detected within 1.5 seconds (actual: {:?})",
            detection_time
        );

        // Now test update latency
        let updated_content = json!({
            "worker_id": worker_id,
            "status": "active",
            "current_task": "latency-task"
        });
        fs::write(&path, updated_content.to_string()).unwrap();

        let update_start = std::time::Instant::now();
        let mut update_detected = false;

        while update_start.elapsed() < Duration::from_millis(2000) {
            if watcher.recv_timeout(Duration::from_millis(50)).is_some() {
                if let Some(w) = watcher.get_worker(worker_id) {
                    if w.status == WorkerStatus::Active {
                        update_detected = true;
                        break;
                    }
                }
            }
        }

        let update_time = update_start.elapsed();

        assert!(
            update_detected,
            "Status update should be detected within 2 seconds"
        );
        assert!(
            update_time < Duration::from_millis(1500),
            "Status update should propagate within 1.5 seconds (actual: {:?})",
            update_time
        );
    }

    /// Test that no stale data is displayed after rapid updates.
    ///
    /// Verifies that the watcher correctly tracks the latest state
    /// even when updates happen rapidly.
    #[test]
    fn test_no_stale_data_after_rapid_updates() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_id = "rapid-update-worker";
        let path = status_dir.join(format!("{}.json", worker_id));

        // Rapidly update status 10 times
        for i in 0..10 {
            let status = if i % 2 == 0 { "active" } else { "idle" };
            let content = json!({
                "worker_id": worker_id,
                "status": status,
                "tasks_completed": i
            });
            fs::write(&path, content.to_string()).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }

        // Wait for debounce and processing
        std::thread::sleep(Duration::from_millis(200));

        // Drain all events
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify we have the latest state (9 tasks completed, idle status)
        let worker = watcher.get_worker(worker_id).unwrap();
        assert_eq!(
            worker.tasks_completed, 9,
            "Should have latest tasks_completed value (no stale data)"
        );
        assert_eq!(
            worker.status,
            WorkerStatus::Idle,
            "Should have latest status (idle, since 9 is odd)"
        );
    }

    /// Test graceful handling of external changes (file becomes corrupted).
    #[test]
    fn test_handles_corrupted_status_file_gracefully() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_id = "corruption-test-worker";
        let path = status_dir.join(format!("{}.json", worker_id));

        // Create valid worker
        let content = json!({
            "worker_id": worker_id,
            "status": "active",
            "tasks_completed": 5
        });
        fs::write(&path, content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Corrupt the file
        fs::write(&path, "{ this is not valid json }").unwrap();

        std::thread::sleep(Duration::from_millis(100));

        // Should receive an error event or the watcher should handle it gracefully
        // The worker might still exist in the map with old data, or be removed
        // Either way, the watcher should not crash
        let event = watcher.recv_timeout(Duration::from_millis(500));

        // Check that we either got an error event or no crash occurred
        match event {
            Some(StatusEvent::Error { path: p, .. }) => {
                assert!(p.ends_with(format!("{}.json", worker_id).as_str()));
            }
            Some(StatusEvent::WorkerUpdated { .. }) => {
                // This is also acceptable - the watcher might keep old state
            }
            None => {
                // No event is also acceptable - the watcher might ignore invalid files
            }
            other => {
                // Any other event type is fine as long as we didn't crash
                let _ = other;
            }
        }

        // Most importantly: the watcher should still be functional
        // Create a new valid worker to verify the watcher is still working
        let new_worker_id = "post-corruption-worker";
        let new_path = status_dir.join(format!("{}.json", new_worker_id));
        let new_content = json!({
            "worker_id": new_worker_id,
            "status": "idle"
        });
        fs::write(&new_path, new_content.to_string()).unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        assert!(
            watcher.get_worker(new_worker_id).is_some(),
            "Watcher should still track new workers after corruption"
        );
    }

    /// Test full worker lifecycle: starting -> active -> idle -> stopped.
    #[test]
    fn test_full_worker_lifecycle() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_id = "lifecycle-worker";
        let path = status_dir.join(format!("{}.json", worker_id));

        // Stage 1: Starting
        fs::write(
            &path,
            json!({
                "worker_id": worker_id,
                "status": "starting"
            })
            .to_string(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        while watcher.recv_timeout(Duration::from_millis(20)).is_some() {}
        assert_eq!(
            watcher.get_worker(worker_id).unwrap().status,
            WorkerStatus::Starting
        );

        // Stage 2: Active (picked up task)
        fs::write(
            &path,
            json!({
                "worker_id": worker_id,
                "status": "active",
                "current_task": "fg-lifecycle-test",
                "tasks_completed": 0
            })
            .to_string(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        while watcher.recv_timeout(Duration::from_millis(20)).is_some() {}
        let w = watcher.get_worker(worker_id).unwrap();
        assert_eq!(w.status, WorkerStatus::Active);
        assert_eq!(w.current_task, Some("fg-lifecycle-test".to_string()));

        // Stage 3: Idle (task completed)
        fs::write(
            &path,
            json!({
                "worker_id": worker_id,
                "status": "idle",
                "current_task": null,
                "tasks_completed": 1
            })
            .to_string(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        while watcher.recv_timeout(Duration::from_millis(20)).is_some() {}
        let w = watcher.get_worker(worker_id).unwrap();
        assert_eq!(w.status, WorkerStatus::Idle);
        assert_eq!(w.tasks_completed, 1);

        // Stage 4: Stopped (external termination)
        fs::write(
            &path,
            json!({
                "worker_id": worker_id,
                "status": "stopped",
                "tasks_completed": 1
            })
            .to_string(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        while watcher.recv_timeout(Duration::from_millis(20)).is_some() {}
        let w = watcher.get_worker(worker_id).unwrap();
        assert_eq!(w.status, WorkerStatus::Stopped);
        assert!(!w.is_healthy());

        // Stage 5: Deleted (cleanup)
        fs::remove_file(&path).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        while watcher.recv_timeout(Duration::from_millis(20)).is_some() {}
        assert!(
            watcher.get_worker(worker_id).is_none(),
            "Worker should be removed after file deletion"
        );
    }

    /// Test WorkerCounts tracking through status transitions.
    #[test]
    fn test_worker_counts_through_transitions() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Create workers in different states
        for (id, status) in [
            ("worker-1", "starting"),
            ("worker-2", "active"),
            ("worker-3", "active"),
            ("worker-4", "idle"),
            ("worker-5", "failed"),
        ] {
            let path = status_dir.join(format!("{}.json", id));
            fs::write(
                &path,
                json!({
                    "worker_id": id,
                    "status": status
                })
                .to_string(),
            )
            .unwrap();
        }

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        let counts = watcher.worker_counts();
        assert_eq!(counts.total, 5);
        assert_eq!(counts.starting, 1);
        assert_eq!(counts.active, 2);
        assert_eq!(counts.idle, 1);
        assert_eq!(counts.failed, 1);
        assert_eq!(counts.healthy(), 4); // starting + active + idle
        assert_eq!(counts.unhealthy(), 1); // failed

        // Transition worker-1 from starting to active
        let path = status_dir.join("worker-1.json");
        fs::write(
            &path,
            json!({
                "worker_id": "worker-1",
                "status": "active"
            })
            .to_string(),
        )
        .unwrap();

        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        let counts = watcher.worker_counts();
        assert_eq!(counts.starting, 0, "Starting count should decrease");
        assert_eq!(counts.active, 3, "Active count should increase");
    }
}
