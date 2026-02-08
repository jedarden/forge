//! Real-time file watching for worker status updates.
//!
//! This module provides file system watching functionality using the `notify` crate
//! with debouncing to avoid excessive updates. It watches `~/.forge/status/*.json`
//! files and triggers UI updates on file creation, modification, and deletion.
//!
//! ## Architecture
//!
//! Per ADR 0008, this module implements:
//! - inotify-based file watching (Linux) with fallback to polling
//! - Debouncing to coalesce rapid file changes
//! - Async event channel for integration with async runtimes
//! - Graceful error handling for corrupted or unreadable files
//!
//! ## Example
//!
//! ```no_run
//! use forge_core::watcher::{StatusWatcher, StatusEvent};
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> forge_core::Result<()> {
//!     let (watcher, mut rx) = StatusWatcher::new(None)?;
//!
//!     // Process events
//!     while let Some(event) = rx.recv().await {
//!         match event {
//!             StatusEvent::Created { worker_id, status } => {
//!                 println!("Worker {} created with status {:?}", worker_id, status);
//!             }
//!             StatusEvent::Modified { worker_id, status } => {
//!                 println!("Worker {} modified: {:?}", worker_id, status);
//!             }
//!             StatusEvent::Removed { worker_id } => {
//!                 println!("Worker {} removed", worker_id);
//!             }
//!             StatusEvent::Error { worker_id, error } => {
//!                 eprintln!("Error reading worker {}: {}", worker_id, error);
//!             }
//!         }
//!     }
//!
//!     Ok(())
//! }
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::error::{ForgeError, Result};
use crate::status::{StatusReader, WorkerStatusInfo};

/// Default debounce duration in milliseconds.
///
/// Per ADR 0008: Status updates should have <100ms latency.
/// 50ms debounce provides good balance between responsiveness and efficiency.
pub const DEFAULT_DEBOUNCE_MS: u64 = 50;

/// Default channel buffer size for events.
///
/// Large enough to handle bursts of events without blocking.
pub const DEFAULT_CHANNEL_BUFFER: usize = 256;

/// Event types emitted by the status watcher.
#[derive(Debug, Clone)]
pub enum StatusEvent {
    /// A new worker status file was created.
    Created {
        /// ID of the worker (filename stem)
        worker_id: String,
        /// Parsed status information
        status: WorkerStatusInfo,
    },

    /// An existing worker status file was modified.
    Modified {
        /// ID of the worker (filename stem)
        worker_id: String,
        /// Updated status information
        status: WorkerStatusInfo,
    },

    /// A worker status file was removed.
    Removed {
        /// ID of the worker that was removed
        worker_id: String,
    },

    /// An error occurred while processing a status file.
    ///
    /// This is non-fatal; other workers continue to be monitored.
    Error {
        /// ID of the worker (if determinable)
        worker_id: String,
        /// Error message
        error: String,
    },
}

impl StatusEvent {
    /// Get the worker ID associated with this event.
    pub fn worker_id(&self) -> &str {
        match self {
            Self::Created { worker_id, .. } => worker_id,
            Self::Modified { worker_id, .. } => worker_id,
            Self::Removed { worker_id } => worker_id,
            Self::Error { worker_id, .. } => worker_id,
        }
    }

    /// Returns true if this is an error event.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }
}

/// Configuration for the status watcher.
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// Directory to watch (defaults to ~/.forge/status/)
    pub status_dir: PathBuf,

    /// Debounce duration for coalescing rapid changes
    pub debounce_duration: Duration,

    /// Channel buffer size for events
    pub channel_buffer: usize,

    /// Whether to emit initial state on startup
    pub emit_initial_state: bool,
}

impl WatcherConfig {
    /// Create a new config with the given status directory.
    pub fn new(status_dir: PathBuf) -> Self {
        Self {
            status_dir,
            debounce_duration: Duration::from_millis(DEFAULT_DEBOUNCE_MS),
            channel_buffer: DEFAULT_CHANNEL_BUFFER,
            emit_initial_state: true,
        }
    }

    /// Create a config with the default status directory.
    pub fn default_config() -> Result<Self> {
        let status_dir = StatusReader::default_status_dir()?;
        Ok(Self::new(status_dir))
    }

    /// Set the debounce duration.
    pub fn with_debounce(mut self, duration: Duration) -> Self {
        self.debounce_duration = duration;
        self
    }

    /// Set the channel buffer size.
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.channel_buffer = size;
        self
    }

    /// Set whether to emit initial state on startup.
    pub fn with_initial_state(mut self, emit: bool) -> Self {
        self.emit_initial_state = emit;
        self
    }
}

/// Real-time file watcher for worker status files.
///
/// Uses the `notify` crate with debouncing to watch `~/.forge/status/*.json`
/// and emit [`StatusEvent`]s when files change.
pub struct StatusWatcher {
    /// The underlying debounced watcher
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,

    /// Status directory being watched
    status_dir: PathBuf,

    /// Sender for stop signal (kept to prevent channel from closing)
    _stop_tx: mpsc::Sender<()>,
}

impl StatusWatcher {
    /// Create a new status watcher with default configuration.
    ///
    /// Returns the watcher and a receiver for status events.
    ///
    /// # Arguments
    ///
    /// * `status_dir` - Optional status directory. If None, uses `~/.forge/status/`.
    ///
    /// # Returns
    ///
    /// A tuple of (StatusWatcher, mpsc::Receiver<StatusEvent>).
    pub fn new(status_dir: Option<PathBuf>) -> Result<(Self, mpsc::Receiver<StatusEvent>)> {
        let config = match status_dir {
            Some(dir) => WatcherConfig::new(dir),
            None => WatcherConfig::default_config()?,
        };

        Self::with_config(config)
    }

    /// Create a new status watcher with custom configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration for the watcher
    ///
    /// # Returns
    ///
    /// A tuple of (StatusWatcher, mpsc::Receiver<StatusEvent>).
    pub fn with_config(config: WatcherConfig) -> Result<(Self, mpsc::Receiver<StatusEvent>)> {
        let (event_tx, event_rx) = mpsc::channel(config.channel_buffer);
        let (stop_tx, _stop_rx) = mpsc::channel::<()>(1);

        // Create the status directory if it doesn't exist
        if !config.status_dir.exists() {
            std::fs::create_dir_all(&config.status_dir).map_err(|e| ForgeError::Io {
                operation: "creating status directory".to_string(),
                path: config.status_dir.clone(),
                source: e,
            })?;
            info!("Created status directory: {:?}", config.status_dir);
        }

        let status_dir = config.status_dir.clone();
        let status_reader = StatusReader::new(Some(status_dir.clone()))?;

        // Track known files for create vs modify detection
        let known_files = Arc::new(std::sync::Mutex::new(HashSet::new()));

        // Initialize known files
        if let Ok(workers) = status_reader.list_workers() {
            let mut files = known_files.lock().unwrap();
            for worker_id in workers {
                files.insert(worker_id);
            }
        }

        // Clone for the closure
        let known_files_clone = Arc::clone(&known_files);
        let event_tx_clone = event_tx.clone();
        let status_dir_clone = status_dir.clone();

        // Create debouncer with event handler
        let debouncer = new_debouncer(
            config.debounce_duration,
            None, // Use default tick rate
            move |result: DebounceEventResult| {
                match result {
                    Ok(events) => {
                        for event in events {
                            if let Err(e) = process_event(
                                &event.event,
                                &status_dir_clone,
                                &known_files_clone,
                                &event_tx_clone,
                            ) {
                                warn!("Error processing file event: {}", e);
                            }
                        }
                    }
                    Err(errors) => {
                        for error in errors {
                            error!("File watcher error: {:?}", error);
                            let _ = event_tx_clone.blocking_send(StatusEvent::Error {
                                worker_id: "watcher".to_string(),
                                error: format!("{:?}", error),
                            });
                        }
                    }
                }
            },
        )
        .map_err(|e| ForgeError::WatcherInit {
            message: format!("Failed to create debouncer: {}", e),
        })?;

        // Get mutable reference to add watch path
        let mut debouncer = debouncer;
        debouncer
            .watch(&config.status_dir, RecursiveMode::NonRecursive)
            .map_err(|e| ForgeError::WatcherInit {
                message: format!("Failed to watch directory {:?}: {}", config.status_dir, e),
            })?;

        info!("Started watching status directory: {:?}", config.status_dir);

        // Emit initial state if configured
        if config.emit_initial_state {
            let reader = StatusReader::new(Some(config.status_dir.clone()))?;
            if let Ok(workers) = reader.read_all() {
                for worker in workers {
                    // Use try_send to avoid blocking - if channel is full, log warning
                    if event_tx
                        .try_send(StatusEvent::Created {
                            worker_id: worker.worker_id.clone(),
                            status: worker,
                        })
                        .is_err()
                    {
                        warn!("Event channel full during initial state emission");
                    }
                }
            }
        }

        Ok((
            Self {
                _debouncer: debouncer,
                status_dir,
                _stop_tx: stop_tx,
            },
            event_rx,
        ))
    }

    /// Get the status directory being watched.
    pub fn status_dir(&self) -> &Path {
        &self.status_dir
    }
}

/// Process a file system event and emit appropriate StatusEvent.
fn process_event(
    event: &Event,
    _status_dir: &Path,
    known_files: &Arc<std::sync::Mutex<HashSet<String>>>,
    tx: &mpsc::Sender<StatusEvent>,
) -> Result<()> {
    for path in &event.paths {
        // Only process .json files
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        // Extract worker ID from filename
        let worker_id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };

        debug!("Processing event {:?} for worker {}", event.kind, worker_id);

        let status_event = match event.kind {
            EventKind::Create(_) => {
                // File created
                {
                    let mut files = known_files.lock().unwrap();
                    files.insert(worker_id.clone());
                }

                match read_status_file(path) {
                    Ok(status) => StatusEvent::Created {
                        worker_id: worker_id.clone(),
                        status,
                    },
                    Err(e) => StatusEvent::Error {
                        worker_id: worker_id.clone(),
                        error: e.to_string(),
                    },
                }
            }

            EventKind::Modify(_) => {
                // File modified - check if it's actually a create (file might be new)
                let is_new = {
                    let mut files = known_files.lock().unwrap();
                    if files.contains(&worker_id) {
                        false
                    } else {
                        files.insert(worker_id.clone());
                        true
                    }
                };

                match read_status_file(path) {
                    Ok(status) => {
                        if is_new {
                            StatusEvent::Created {
                                worker_id: worker_id.clone(),
                                status,
                            }
                        } else {
                            StatusEvent::Modified {
                                worker_id: worker_id.clone(),
                                status,
                            }
                        }
                    }
                    Err(e) => StatusEvent::Error {
                        worker_id: worker_id.clone(),
                        error: e.to_string(),
                    },
                }
            }

            EventKind::Remove(_) => {
                // File removed
                {
                    let mut files = known_files.lock().unwrap();
                    files.remove(&worker_id);
                }

                StatusEvent::Removed {
                    worker_id: worker_id.clone(),
                }
            }

            _ => {
                // Ignore other event types (access, etc.)
                debug!("Ignoring event kind {:?}", event.kind);
                continue;
            }
        };

        // Send the event (non-blocking, drop if channel full)
        if tx.blocking_send(status_event).is_err() {
            warn!("Event channel full, dropping event for worker {}", worker_id);
        }
    }

    Ok(())
}

/// Read and parse a status file.
fn read_status_file(path: &Path) -> Result<WorkerStatusInfo> {
    let content = std::fs::read_to_string(path).map_err(|e| ForgeError::Io {
        operation: "reading status file".to_string(),
        path: path.to_path_buf(),
        source: e,
    })?;

    serde_json::from_str(&content).map_err(|e| ForgeError::StatusFileParse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;
    use tempfile::TempDir;

    /// Create a test status file.
    fn create_status_file(dir: &Path, worker_id: &str, status: &str) {
        let path = dir.join(format!("{}.json", worker_id));
        let content = format!(
            r#"{{"worker_id": "{}", "status": "{}"}}"#,
            worker_id, status
        );
        fs::write(path, content).unwrap();
    }

    #[test]
    fn test_watcher_config_default() {
        // Skip if HOME is not set
        if std::env::var("HOME").is_err() {
            return;
        }

        let config = WatcherConfig::default_config().unwrap();
        assert!(config.status_dir.ends_with("status"));
        assert_eq!(config.debounce_duration, Duration::from_millis(50));
        assert!(config.emit_initial_state);
    }

    #[test]
    fn test_watcher_config_custom() {
        let config = WatcherConfig::new(PathBuf::from("/tmp/test"))
            .with_debounce(Duration::from_millis(100))
            .with_buffer_size(512)
            .with_initial_state(false);

        assert_eq!(config.status_dir, PathBuf::from("/tmp/test"));
        assert_eq!(config.debounce_duration, Duration::from_millis(100));
        assert_eq!(config.channel_buffer, 512);
        assert!(!config.emit_initial_state);
    }

    #[test]
    fn test_status_event_worker_id() {
        let created = StatusEvent::Created {
            worker_id: "test".to_string(),
            status: WorkerStatusInfo::new("test", crate::types::WorkerStatus::Active),
        };
        assert_eq!(created.worker_id(), "test");
        assert!(!created.is_error());

        let error = StatusEvent::Error {
            worker_id: "bad".to_string(),
            error: "failed".to_string(),
        };
        assert_eq!(error.worker_id(), "bad");
        assert!(error.is_error());
    }

    #[tokio::test]
    async fn test_watcher_creates_directory() {
        let tmp_dir = TempDir::new().unwrap();
        let status_dir = tmp_dir.path().join("status");

        // Directory doesn't exist yet
        assert!(!status_dir.exists());

        let config = WatcherConfig::new(status_dir.clone()).with_initial_state(false);

        let (_watcher, _rx) = StatusWatcher::with_config(config).unwrap();

        // Directory should now exist
        assert!(status_dir.exists());
    }

    #[tokio::test]
    async fn test_watcher_emits_initial_state() {
        let tmp_dir = TempDir::new().unwrap();
        let status_dir = tmp_dir.path().to_path_buf();

        // Create some status files first
        create_status_file(&status_dir, "worker-a", "active");
        create_status_file(&status_dir, "worker-b", "idle");

        let config = WatcherConfig::new(status_dir).with_initial_state(true);

        let (_watcher, mut rx) = StatusWatcher::with_config(config).unwrap();

        // Wait for initial events
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should receive two Created events
        let mut worker_ids = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let StatusEvent::Created { worker_id, .. } = event {
                worker_ids.push(worker_id);
            }
        }

        assert!(worker_ids.contains(&"worker-a".to_string()));
        assert!(worker_ids.contains(&"worker-b".to_string()));
    }

    #[tokio::test]
    async fn test_watcher_detects_file_creation() {
        let tmp_dir = TempDir::new().unwrap();
        let status_dir = tmp_dir.path().to_path_buf();

        let config = WatcherConfig::new(status_dir.clone())
            .with_debounce(Duration::from_millis(10))
            .with_initial_state(false);

        let (_watcher, mut rx) = StatusWatcher::with_config(config).unwrap();

        // Wait for watcher to be ready
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Create a new status file
        create_status_file(&status_dir, "new-worker", "starting");

        // Wait for event
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("Timeout waiting for event")
            .expect("Channel closed");

        match event {
            StatusEvent::Created { worker_id, status } | StatusEvent::Modified { worker_id, status } => {
                assert_eq!(worker_id, "new-worker");
                assert_eq!(status.worker_id, "new-worker");
            }
            other => panic!("Unexpected event: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_watcher_detects_file_modification() {
        let tmp_dir = TempDir::new().unwrap();
        let status_dir = tmp_dir.path().to_path_buf();

        // Create initial file
        create_status_file(&status_dir, "worker-x", "idle");

        let config = WatcherConfig::new(status_dir.clone())
            .with_debounce(Duration::from_millis(10))
            .with_initial_state(false);

        let (_watcher, mut rx) = StatusWatcher::with_config(config).unwrap();

        // Wait for watcher to be ready and drain initial event if any
        tokio::time::sleep(Duration::from_millis(100)).await;
        while let Ok(_) = rx.try_recv() {}

        // Modify the file
        create_status_file(&status_dir, "worker-x", "active");

        // Wait for event
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("Timeout waiting for event")
            .expect("Channel closed");

        match event {
            StatusEvent::Modified { worker_id, status } => {
                assert_eq!(worker_id, "worker-x");
                assert_eq!(status.status, crate::types::WorkerStatus::Active);
            }
            StatusEvent::Created { worker_id, status } => {
                // Some systems emit create instead of modify
                assert_eq!(worker_id, "worker-x");
                assert_eq!(status.status, crate::types::WorkerStatus::Active);
            }
            other => panic!("Unexpected event: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_watcher_detects_file_deletion() {
        let tmp_dir = TempDir::new().unwrap();
        let status_dir = tmp_dir.path().to_path_buf();

        // Create initial file
        create_status_file(&status_dir, "worker-del", "active");

        let config = WatcherConfig::new(status_dir.clone())
            .with_debounce(Duration::from_millis(10))
            .with_initial_state(false);

        let (_watcher, mut rx) = StatusWatcher::with_config(config).unwrap();

        // Wait for watcher to be ready and drain initial events
        tokio::time::sleep(Duration::from_millis(100)).await;
        while let Ok(_) = rx.try_recv() {}

        // Delete the file
        fs::remove_file(status_dir.join("worker-del.json")).unwrap();

        // Wait for event
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("Timeout waiting for event")
            .expect("Channel closed");

        match event {
            StatusEvent::Removed { worker_id } => {
                assert_eq!(worker_id, "worker-del");
            }
            other => panic!("Unexpected event: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_watcher_handles_invalid_json() {
        let tmp_dir = TempDir::new().unwrap();
        let status_dir = tmp_dir.path().to_path_buf();

        let config = WatcherConfig::new(status_dir.clone())
            .with_debounce(Duration::from_millis(10))
            .with_initial_state(false);

        let (_watcher, mut rx) = StatusWatcher::with_config(config).unwrap();

        // Wait for watcher to be ready
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Create an invalid JSON file
        let path = status_dir.join("bad-worker.json");
        fs::write(&path, "not valid json {").unwrap();

        // Wait for event
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("Timeout waiting for event")
            .expect("Channel closed");

        match event {
            StatusEvent::Error { worker_id, error } => {
                assert_eq!(worker_id, "bad-worker");
                assert!(!error.is_empty());
            }
            other => panic!("Unexpected event: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_watcher_ignores_non_json_files() {
        let tmp_dir = TempDir::new().unwrap();
        let status_dir = tmp_dir.path().to_path_buf();

        let config = WatcherConfig::new(status_dir.clone())
            .with_debounce(Duration::from_millis(10))
            .with_initial_state(false);

        let (_watcher, mut rx) = StatusWatcher::with_config(config).unwrap();

        // Wait for watcher to be ready
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Create a non-JSON file
        fs::write(status_dir.join("readme.txt"), "ignore me").unwrap();

        // Should not receive any events
        let result = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;

        assert!(result.is_err(), "Should not receive event for non-JSON file");
    }

    #[test]
    fn test_read_status_file_valid() {
        let tmp_dir = TempDir::new().unwrap();
        let path = tmp_dir.path().join("test.json");
        fs::write(
            &path,
            r#"{"worker_id": "test", "status": "active", "model": "sonnet"}"#,
        )
        .unwrap();

        let status = read_status_file(&path).unwrap();
        assert_eq!(status.worker_id, "test");
        assert_eq!(status.status, crate::types::WorkerStatus::Active);
        assert_eq!(status.model, Some("sonnet".to_string()));
    }

    #[test]
    fn test_read_status_file_invalid() {
        let tmp_dir = TempDir::new().unwrap();
        let path = tmp_dir.path().join("bad.json");
        fs::write(&path, "not json").unwrap();

        let result = read_status_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_status_file_not_found() {
        let result = read_status_file(Path::new("/nonexistent/file.json"));
        assert!(result.is_err());
    }
}
