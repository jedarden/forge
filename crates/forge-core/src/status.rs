//! Status file reader for worker status from `~/.forge/status/*.json`.
//!
//! This module provides functionality to read and parse worker status files,
//! handling missing files, invalid JSON, and partial data gracefully.
//!
//! ## Example
//!
//! ```no_run
//! use forge_core::status::{StatusReader, WorkerStatusInfo};
//!
//! fn main() -> forge_core::Result<()> {
//!     let reader = StatusReader::new(None)?;
//!
//!     // Read all worker statuses
//!     let workers = reader.read_all()?;
//!     for worker in workers {
//!         println!("{}: {}", worker.worker_id, worker.status);
//!     }
//!
//!     // Read a specific worker's status
//!     if let Some(worker) = reader.read_worker("my-worker")? {
//!         println!("Worker {} is {}", worker.worker_id, worker.status);
//!     }
//!
//!     Ok(())
//! }
//! ```

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::error::{ForgeError, Result};
use crate::types::WorkerStatus;

/// Custom deserializer for current_task field that handles both string and object formats.
///
/// Accepts:
/// - String: "bd-abc"
/// - Object: {"bead_id": "bd-abc", "bead_title": "...", "priority": 1}
fn deserialize_current_task<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
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

/// Complete worker status information as stored in status files.
///
/// This struct represents the full contents of a worker's status file
/// at `~/.forge/status/<worker_id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerStatusInfo {
    /// Unique identifier for the worker
    pub worker_id: String,

    /// Current status of the worker
    #[serde(default)]
    pub status: WorkerStatus,

    /// Model being used by this worker (e.g., "sonnet", "opus", "haiku")
    #[serde(default)]
    pub model: Option<String>,

    /// Workspace directory the worker is operating in
    #[serde(default)]
    pub workspace: Option<PathBuf>,

    /// Process ID of the worker
    #[serde(default)]
    pub pid: Option<u32>,

    /// Timestamp when the worker was started
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,

    /// Timestamp of the worker's last activity
    #[serde(default)]
    pub last_activity: Option<DateTime<Utc>>,

    /// ID of the bead/task currently being processed (handles both string and object formats)
    #[serde(default, deserialize_with = "deserialize_current_task")]
    pub current_task: Option<String>,

    /// Number of tasks completed by this worker
    #[serde(default)]
    pub tasks_completed: u32,
}

impl WorkerStatusInfo {
    /// Create a new WorkerStatusInfo with just an ID and status.
    ///
    /// This is useful for creating placeholder entries when a status file
    /// is missing or corrupted.
    pub fn new(worker_id: impl Into<String>, status: WorkerStatus) -> Self {
        Self {
            worker_id: worker_id.into(),
            status,
            model: None,
            workspace: None,
            pid: None,
            started_at: None,
            last_activity: None,
            current_task: None,
            tasks_completed: 0,
        }
    }

    /// Create an error placeholder for a worker with a corrupted or unreadable status file.
    pub fn error(worker_id: impl Into<String>) -> Self {
        Self::new(worker_id, WorkerStatus::Error)
    }

    /// Returns true if the worker is considered healthy.
    pub fn is_healthy(&self) -> bool {
        self.status.is_healthy()
    }

    /// Returns true if the worker appears to be stale (no activity for a long time).
    ///
    /// A worker is considered stale if its last_activity is more than 5 minutes ago.
    pub fn is_stale(&self, threshold_secs: i64) -> bool {
        match self.last_activity {
            Some(last) => {
                let now = Utc::now();
                let elapsed = now.signed_duration_since(last);
                elapsed.num_seconds() > threshold_secs
            }
            None => false, // No activity data, can't determine staleness
        }
    }
}

impl Default for WorkerStatusInfo {
    fn default() -> Self {
        Self::new("unknown", WorkerStatus::Idle)
    }
}

/// Reader for worker status files from `~/.forge/status/`.
///
/// The reader provides methods to read individual worker statuses or all
/// workers at once. It handles errors gracefully, returning Error status
/// for workers with corrupted or unreadable files.
#[derive(Debug, Clone)]
pub struct StatusReader {
    /// Directory containing status files
    status_dir: PathBuf,
}

impl StatusReader {
    /// Create a new StatusReader.
    ///
    /// If `status_dir` is None, uses the default `~/.forge/status/` directory.
    pub fn new(status_dir: Option<PathBuf>) -> Result<Self> {
        let status_dir = match status_dir {
            Some(dir) => dir,
            None => Self::default_status_dir()?,
        };

        debug!("StatusReader initialized with directory: {:?}", status_dir);

        Ok(Self { status_dir })
    }

    /// Get the default status directory (`~/.forge/status/`).
    pub fn default_status_dir() -> Result<PathBuf> {
        let home = std::env::var("HOME").map_err(|_| ForgeError::ConfigMissingField {
            field: "HOME environment variable".to_string(),
        })?;

        Ok(PathBuf::from(home).join(".forge").join("status"))
    }

    /// Get the path to a worker's status file.
    pub fn status_file_path(&self, worker_id: &str) -> PathBuf {
        self.status_dir.join(format!("{}.json", worker_id))
    }

    /// Read a specific worker's status file.
    ///
    /// Returns `None` if the file doesn't exist.
    /// Returns `Some(WorkerStatusInfo)` with Error status if the file is corrupted.
    pub fn read_worker(&self, worker_id: &str) -> Result<Option<WorkerStatusInfo>> {
        let path = self.status_file_path(worker_id);

        if !path.exists() {
            debug!("Status file not found: {:?}", path);
            return Ok(None);
        }

        match self.parse_status_file(&path) {
            Ok(status) => Ok(Some(status)),
            Err(e) => {
                warn!("Failed to parse status file {:?}: {}", path, e);
                Ok(Some(WorkerStatusInfo::error(worker_id)))
            }
        }
    }

    /// Read all worker status files in the status directory.
    ///
    /// Returns a list of all workers found. Workers with corrupted files
    /// will have Error status.
    ///
    /// If the status directory doesn't exist, returns an empty list.
    pub fn read_all(&self) -> Result<Vec<WorkerStatusInfo>> {
        if !self.status_dir.exists() {
            debug!("Status directory does not exist: {:?}", self.status_dir);
            return Ok(Vec::new());
        }

        let entries = std::fs::read_dir(&self.status_dir).map_err(|e| ForgeError::Io {
            operation: "reading status directory".to_string(),
            path: self.status_dir.clone(),
            source: e,
        })?;

        let mut workers = Vec::new();

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warn!("Failed to read directory entry: {}", e);
                    continue;
                }
            };

            let path = entry.path();

            // Only process .json files
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let worker_id = match path.file_stem().and_then(|s| s.to_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };

            match self.parse_status_file(&path) {
                Ok(status) => workers.push(status),
                Err(e) => {
                    warn!("Failed to parse status file {:?}: {}", path, e);
                    workers.push(WorkerStatusInfo::error(worker_id));
                }
            }
        }

        // Sort by worker_id for consistent ordering
        workers.sort_by(|a, b| a.worker_id.cmp(&b.worker_id));

        debug!("Read {} worker statuses", workers.len());
        Ok(workers)
    }

    /// Parse a status file into a WorkerStatusInfo.
    fn parse_status_file(&self, path: &Path) -> Result<WorkerStatusInfo> {
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

    /// Get list of worker IDs from status files (without parsing).
    ///
    /// This is faster than `read_all()` when you only need the worker IDs.
    pub fn list_workers(&self) -> Result<Vec<String>> {
        if !self.status_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = std::fs::read_dir(&self.status_dir).map_err(|e| ForgeError::Io {
            operation: "listing status directory".to_string(),
            path: self.status_dir.clone(),
            source: e,
        })?;

        let mut worker_ids = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
                worker_ids.push(id.to_string());
            }
        }

        worker_ids.sort();
        Ok(worker_ids)
    }

    /// Check if the status directory exists.
    pub fn status_dir_exists(&self) -> bool {
        self.status_dir.exists()
    }
}

/// Writer for worker status files to `~/.forge/status/`.
///
/// The writer provides methods to update individual worker statuses or
/// perform batch operations on multiple workers.
#[derive(Debug, Clone)]
pub struct StatusWriter {
    /// Directory containing status files
    status_dir: PathBuf,
}

impl StatusWriter {
    /// Create a new StatusWriter.
    ///
    /// If `status_dir` is None, uses the default `~/.forge/status/` directory.
    pub fn new(status_dir: Option<PathBuf>) -> Result<Self> {
        let status_dir = match status_dir {
            Some(dir) => dir,
            None => StatusReader::default_status_dir()?,
        };

        // Ensure the status directory exists
        if !status_dir.exists() {
            std::fs::create_dir_all(&status_dir).map_err(|e| ForgeError::Io {
                operation: "creating status directory".to_string(),
                path: status_dir.clone(),
                source: e,
            })?;
        }

        debug!("StatusWriter initialized with directory: {:?}", status_dir);

        Ok(Self { status_dir })
    }

    /// Get the path to a worker's status file.
    pub fn status_file_path(&self, worker_id: &str) -> PathBuf {
        self.status_dir.join(format!("{}.json", worker_id))
    }

    /// Update a worker's status in their status file.
    ///
    /// If the status file exists, it reads the existing data and updates only the status.
    /// If the file doesn't exist, it creates a new status file with the given status.
    pub fn update_status(&self, worker_id: &str, status: WorkerStatus) -> Result<()> {
        let path = self.status_file_path(worker_id);

        // Read existing status if file exists, otherwise create new
        let mut info = if path.exists() {
            let content = std::fs::read_to_string(&path).map_err(|e| ForgeError::Io {
                operation: "reading status file".to_string(),
                path: path.clone(),
                source: e,
            })?;

            serde_json::from_str(&content).unwrap_or_else(|_| {
                WorkerStatusInfo::new(worker_id, status)
            })
        } else {
            WorkerStatusInfo::new(worker_id, status)
        };

        // Update the status
        info.status = status;
        info.last_activity = Some(Utc::now());

        // Write atomically using temp file
        self.write_status(&path, &info)
    }

    /// Write a complete worker status info to file.
    pub fn write_worker(&self, info: &WorkerStatusInfo) -> Result<()> {
        let path = self.status_file_path(&info.worker_id);
        self.write_status(&path, info)
    }

    /// Write status to file atomically.
    fn write_status(&self, path: &Path, info: &WorkerStatusInfo) -> Result<()> {
        let json = serde_json::to_string_pretty(info).map_err(|e| ForgeError::StatusFileParse {
            path: path.to_path_buf(),
            message: format!("Failed to serialize status: {}", e),
        })?;

        // Write to temp file first, then rename for atomicity
        let temp_path = path.with_extension("json.tmp");
        std::fs::write(&temp_path, &json).map_err(|e| ForgeError::Io {
            operation: "writing status file".to_string(),
            path: temp_path.clone(),
            source: e,
        })?;

        std::fs::rename(&temp_path, path).map_err(|e| ForgeError::Io {
            operation: "renaming status file".to_string(),
            path: path.to_path_buf(),
            source: e,
        })?;

        debug!("Wrote status file: {:?}", path);
        Ok(())
    }

    /// Pause a worker by setting their status to Paused.
    pub fn pause_worker(&self, worker_id: &str) -> Result<()> {
        self.update_status(worker_id, WorkerStatus::Paused)
    }

    /// Resume a worker by setting their status to Idle.
    pub fn resume_worker(&self, worker_id: &str) -> Result<()> {
        self.update_status(worker_id, WorkerStatus::Idle)
    }

    /// Pause all workers with status files in the status directory.
    ///
    /// Returns the number of workers paused.
    pub fn pause_all(&self) -> Result<usize> {
        let reader = StatusReader::new(Some(self.status_dir.clone()))?;
        let workers = reader.list_workers()?;
        let mut count = 0;

        for worker_id in workers {
            if let Ok(Some(info)) = reader.read_worker(&worker_id) {
                // Only pause if not already paused, stopped, or failed
                if info.status != WorkerStatus::Paused
                    && info.status != WorkerStatus::Stopped
                    && info.status != WorkerStatus::Failed
                {
                    self.pause_worker(&worker_id)?;
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// Resume all paused workers in the status directory.
    ///
    /// Returns the number of workers resumed.
    pub fn resume_all(&self) -> Result<usize> {
        let reader = StatusReader::new(Some(self.status_dir.clone()))?;
        let workers = reader.list_workers()?;
        let mut count = 0;

        for worker_id in workers {
            if let Ok(Some(info)) = reader.read_worker(&worker_id) {
                // Only resume if currently paused
                if info.status == WorkerStatus::Paused {
                    self.resume_worker(&worker_id)?;
                    count += 1;
                }
            }
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_status_file(dir: &Path, worker_id: &str, content: &str) {
        let path = dir.join(format!("{}.json", worker_id));
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn test_worker_status_info_new() {
        let status = WorkerStatusInfo::new("test-worker", WorkerStatus::Active);
        assert_eq!(status.worker_id, "test-worker");
        assert_eq!(status.status, WorkerStatus::Active);
        assert!(status.model.is_none());
        assert!(status.workspace.is_none());
    }

    #[test]
    fn test_worker_status_info_error() {
        let status = WorkerStatusInfo::error("broken-worker");
        assert_eq!(status.worker_id, "broken-worker");
        assert_eq!(status.status, WorkerStatus::Error);
    }

    #[test]
    fn test_worker_status_info_is_healthy() {
        assert!(WorkerStatusInfo::new("w", WorkerStatus::Active).is_healthy());
        assert!(WorkerStatusInfo::new("w", WorkerStatus::Idle).is_healthy());
        assert!(WorkerStatusInfo::new("w", WorkerStatus::Starting).is_healthy());
        assert!(!WorkerStatusInfo::new("w", WorkerStatus::Failed).is_healthy());
        assert!(!WorkerStatusInfo::new("w", WorkerStatus::Error).is_healthy());
    }

    #[test]
    fn test_worker_status_info_is_stale() {
        let mut status = WorkerStatusInfo::new("w", WorkerStatus::Active);

        // No last_activity - not stale
        assert!(!status.is_stale(300));

        // Recent activity - not stale
        status.last_activity = Some(Utc::now());
        assert!(!status.is_stale(300));

        // Old activity - stale
        status.last_activity = Some(Utc::now() - chrono::Duration::seconds(600));
        assert!(status.is_stale(300));
    }

    #[test]
    fn test_status_reader_empty_dir() {
        let tmp_dir = TempDir::new().unwrap();
        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();

        let workers = reader.read_all().unwrap();
        assert!(workers.is_empty());

        let worker = reader.read_worker("nonexistent").unwrap();
        assert!(worker.is_none());
    }

    #[test]
    fn test_status_reader_nonexistent_dir() {
        let reader = StatusReader::new(Some(PathBuf::from("/nonexistent/path/to/status"))).unwrap();

        let workers = reader.read_all().unwrap();
        assert!(workers.is_empty());
    }

    #[test]
    fn test_status_reader_valid_file() {
        let tmp_dir = TempDir::new().unwrap();
        let status_json = r#"{
            "worker_id": "test-worker",
            "status": "active",
            "model": "sonnet",
            "workspace": "/home/user/project",
            "pid": 12345,
            "started_at": "2026-02-08T10:00:00Z",
            "last_activity": "2026-02-08T10:30:00Z",
            "current_task": "bd-123",
            "tasks_completed": 5
        }"#;

        create_test_status_file(tmp_dir.path(), "test-worker", status_json);

        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        let worker = reader.read_worker("test-worker").unwrap().unwrap();

        assert_eq!(worker.worker_id, "test-worker");
        assert_eq!(worker.status, WorkerStatus::Active);
        assert_eq!(worker.model, Some("sonnet".to_string()));
        assert_eq!(worker.workspace, Some(PathBuf::from("/home/user/project")));
        assert_eq!(worker.pid, Some(12345));
        assert_eq!(worker.current_task, Some("bd-123".to_string()));
        assert_eq!(worker.tasks_completed, 5);
    }

    #[test]
    fn test_status_reader_partial_file() {
        let tmp_dir = TempDir::new().unwrap();
        // Minimal valid JSON - only required worker_id
        let status_json = r#"{"worker_id": "minimal-worker"}"#;

        create_test_status_file(tmp_dir.path(), "minimal-worker", status_json);

        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        let worker = reader.read_worker("minimal-worker").unwrap().unwrap();

        assert_eq!(worker.worker_id, "minimal-worker");
        assert_eq!(worker.status, WorkerStatus::Idle); // Default
        assert!(worker.model.is_none());
        assert!(worker.workspace.is_none());
        assert_eq!(worker.tasks_completed, 0); // Default
    }

    #[test]
    fn test_status_reader_invalid_json() {
        let tmp_dir = TempDir::new().unwrap();
        create_test_status_file(tmp_dir.path(), "bad-worker", "not valid json {");

        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        let worker = reader.read_worker("bad-worker").unwrap().unwrap();

        // Should return Error status for corrupted files
        assert_eq!(worker.worker_id, "bad-worker");
        assert_eq!(worker.status, WorkerStatus::Error);
    }

    #[test]
    fn test_status_reader_read_all() {
        let tmp_dir = TempDir::new().unwrap();

        create_test_status_file(
            tmp_dir.path(),
            "worker-a",
            r#"{"worker_id": "worker-a", "status": "active"}"#,
        );
        create_test_status_file(
            tmp_dir.path(),
            "worker-b",
            r#"{"worker_id": "worker-b", "status": "idle"}"#,
        );
        create_test_status_file(tmp_dir.path(), "worker-c", "invalid json");

        // Non-JSON file should be ignored
        std::fs::write(tmp_dir.path().join("readme.txt"), "ignore me").unwrap();

        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        let workers = reader.read_all().unwrap();

        assert_eq!(workers.len(), 3);

        // Should be sorted by worker_id
        assert_eq!(workers[0].worker_id, "worker-a");
        assert_eq!(workers[0].status, WorkerStatus::Active);

        assert_eq!(workers[1].worker_id, "worker-b");
        assert_eq!(workers[1].status, WorkerStatus::Idle);

        assert_eq!(workers[2].worker_id, "worker-c");
        assert_eq!(workers[2].status, WorkerStatus::Error);
    }

    #[test]
    fn test_status_reader_list_workers() {
        let tmp_dir = TempDir::new().unwrap();

        create_test_status_file(tmp_dir.path(), "alpha", r#"{"worker_id": "alpha"}"#);
        create_test_status_file(tmp_dir.path(), "beta", r#"{"worker_id": "beta"}"#);
        std::fs::write(tmp_dir.path().join("readme.txt"), "ignore").unwrap();

        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        let ids = reader.list_workers().unwrap();

        assert_eq!(ids, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_status_reader_status_file_path() {
        let reader = StatusReader::new(Some(PathBuf::from("/tmp/status"))).unwrap();
        let path = reader.status_file_path("my-worker");

        assert_eq!(path, PathBuf::from("/tmp/status/my-worker.json"));
    }

    #[test]
    fn test_worker_status_info_deserialize_all_statuses() {
        let test_cases = [
            (
                r#"{"worker_id": "w", "status": "active"}"#,
                WorkerStatus::Active,
            ),
            (
                r#"{"worker_id": "w", "status": "idle"}"#,
                WorkerStatus::Idle,
            ),
            (
                r#"{"worker_id": "w", "status": "failed"}"#,
                WorkerStatus::Failed,
            ),
            (
                r#"{"worker_id": "w", "status": "stopped"}"#,
                WorkerStatus::Stopped,
            ),
            (
                r#"{"worker_id": "w", "status": "error"}"#,
                WorkerStatus::Error,
            ),
            (
                r#"{"worker_id": "w", "status": "starting"}"#,
                WorkerStatus::Starting,
            ),
        ];

        for (json, expected_status) in test_cases {
            let info: WorkerStatusInfo = serde_json::from_str(json).unwrap();
            assert_eq!(info.status, expected_status, "Failed for JSON: {}", json);
        }
    }

    #[test]
    fn test_worker_status_info_serialize_roundtrip() {
        let original = WorkerStatusInfo {
            worker_id: "test-worker".to_string(),
            status: WorkerStatus::Active,
            model: Some("opus".to_string()),
            workspace: Some(PathBuf::from("/home/user/project")),
            pid: Some(42),
            started_at: Some(Utc::now()),
            last_activity: Some(Utc::now()),
            current_task: Some("fg-123".to_string()),
            tasks_completed: 10,
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: WorkerStatusInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(original.worker_id, deserialized.worker_id);
        assert_eq!(original.status, deserialized.status);
        assert_eq!(original.model, deserialized.model);
        assert_eq!(original.workspace, deserialized.workspace);
        assert_eq!(original.pid, deserialized.pid);
        assert_eq!(original.current_task, deserialized.current_task);
        assert_eq!(original.tasks_completed, deserialized.tasks_completed);
    }

    // ============================================================
    // current_task Custom Deserializer Tests (fg-3bq)
    // ============================================================

    /// Test current_task as a simple string format.
    #[test]
    fn test_current_task_string_format() {
        let json = r#"{"worker_id": "w", "current_task": "bd-abc"}"#;
        let info: WorkerStatusInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.current_task, Some("bd-abc".to_string()));
    }

    /// Test current_task as object format with bead_id.
    #[test]
    fn test_current_task_object_format() {
        let json = r#"{"worker_id": "w", "current_task": {"bead_id": "fg-123", "bead_title": "Test task", "priority": 0}}"#;
        let info: WorkerStatusInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.current_task, Some("fg-123".to_string()));
    }

    /// Test current_task as null.
    #[test]
    fn test_current_task_null() {
        let json = r#"{"worker_id": "w", "current_task": null}"#;
        let info: WorkerStatusInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.current_task, None);
    }

    /// Test current_task absent (uses default).
    #[test]
    fn test_current_task_absent() {
        let json = r#"{"worker_id": "w"}"#;
        let info: WorkerStatusInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.current_task, None);
    }

    /// Test current_task object format with minimal fields.
    #[test]
    fn test_current_task_object_minimal() {
        let json = r#"{"worker_id": "w", "current_task": {"bead_id": "bd-xyz"}}"#;
        let info: WorkerStatusInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.current_task, Some("bd-xyz".to_string()));
    }

    /// Test current_task object format without bead_id fails.
    #[test]
    fn test_current_task_object_missing_bead_id() {
        let json = r#"{"worker_id": "w", "current_task": {"title": "no bead_id"}}"#;
        let result: std::result::Result<WorkerStatusInfo, _> = serde_json::from_str(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("bead_id"),
            "Error should mention bead_id: {}",
            err
        );
    }

    /// Test current_task invalid type (number) fails.
    #[test]
    fn test_current_task_invalid_number() {
        let json = r#"{"worker_id": "w", "current_task": 12345}"#;
        let result: std::result::Result<WorkerStatusInfo, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    /// Test current_task invalid type (array) fails.
    #[test]
    fn test_current_task_invalid_array() {
        let json = r#"{"worker_id": "w", "current_task": ["bd-a", "bd-b"]}"#;
        let result: std::result::Result<WorkerStatusInfo, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    /// Test StatusReader handles both current_task formats.
    #[test]
    fn test_status_reader_mixed_current_task_formats() {
        let tmp_dir = TempDir::new().unwrap();

        // Create worker with string format
        create_test_status_file(
            tmp_dir.path(),
            "worker-string",
            r#"{"worker_id": "worker-string", "status": "active", "current_task": "bd-string"}"#,
        );

        // Create worker with object format
        create_test_status_file(
            tmp_dir.path(),
            "worker-object",
            r#"{"worker_id": "worker-object", "status": "active", "current_task": {"bead_id": "fg-object", "priority": 1}}"#,
        );

        // Create worker with null
        create_test_status_file(
            tmp_dir.path(),
            "worker-null",
            r#"{"worker_id": "worker-null", "status": "idle", "current_task": null}"#,
        );

        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        let workers = reader.read_all().unwrap();

        assert_eq!(workers.len(), 3);

        // Find each worker by id
        let w_string = workers
            .iter()
            .find(|w| w.worker_id == "worker-string")
            .unwrap();
        assert_eq!(w_string.current_task, Some("bd-string".to_string()));

        let w_object = workers
            .iter()
            .find(|w| w.worker_id == "worker-object")
            .unwrap();
        assert_eq!(w_object.current_task, Some("fg-object".to_string()));

        let w_null = workers
            .iter()
            .find(|w| w.worker_id == "worker-null")
            .unwrap();
        assert_eq!(w_null.current_task, None);
    }

    // ============================================================
    // StatusWriter Tests (bd-2y6q)
    // ============================================================

    #[test]
    fn test_status_writer_new_creates_directory() {
        let tmp_dir = TempDir::new().unwrap();
        let status_dir = tmp_dir.path().join("status");

        // Directory doesn't exist yet
        assert!(!status_dir.exists());

        // Writer should create it
        let _writer = StatusWriter::new(Some(status_dir.clone())).unwrap();
        assert!(status_dir.exists());
    }

    #[test]
    fn test_status_writer_pause_worker() {
        let tmp_dir = TempDir::new().unwrap();

        // Create an active worker status file
        create_test_status_file(
            tmp_dir.path(),
            "test-worker",
            r#"{"worker_id": "test-worker", "status": "active", "model": "sonnet"}"#,
        );

        let writer = StatusWriter::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        writer.pause_worker("test-worker").unwrap();

        // Read back and verify
        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        let worker = reader.read_worker("test-worker").unwrap().unwrap();

        assert_eq!(worker.status, WorkerStatus::Paused);
        assert_eq!(worker.model, Some("sonnet".to_string())); // Preserved
    }

    #[test]
    fn test_status_writer_resume_worker() {
        let tmp_dir = TempDir::new().unwrap();

        // Create a paused worker status file
        create_test_status_file(
            tmp_dir.path(),
            "paused-worker",
            r#"{"worker_id": "paused-worker", "status": "paused", "model": "opus"}"#,
        );

        let writer = StatusWriter::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        writer.resume_worker("paused-worker").unwrap();

        // Read back and verify
        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        let worker = reader.read_worker("paused-worker").unwrap().unwrap();

        assert_eq!(worker.status, WorkerStatus::Idle);
        assert_eq!(worker.model, Some("opus".to_string())); // Preserved
    }

    #[test]
    fn test_status_writer_pause_creates_file_if_missing() {
        let tmp_dir = TempDir::new().unwrap();

        let writer = StatusWriter::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        writer.pause_worker("new-worker").unwrap();

        // File should be created
        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        let worker = reader.read_worker("new-worker").unwrap().unwrap();

        assert_eq!(worker.worker_id, "new-worker");
        assert_eq!(worker.status, WorkerStatus::Paused);
    }

    #[test]
    fn test_status_writer_pause_all() {
        let tmp_dir = TempDir::new().unwrap();

        // Create multiple workers with different statuses
        create_test_status_file(
            tmp_dir.path(),
            "worker-active",
            r#"{"worker_id": "worker-active", "status": "active"}"#,
        );
        create_test_status_file(
            tmp_dir.path(),
            "worker-idle",
            r#"{"worker_id": "worker-idle", "status": "idle"}"#,
        );
        create_test_status_file(
            tmp_dir.path(),
            "worker-paused",
            r#"{"worker_id": "worker-paused", "status": "paused"}"#,
        );
        create_test_status_file(
            tmp_dir.path(),
            "worker-stopped",
            r#"{"worker_id": "worker-stopped", "status": "stopped"}"#,
        );

        let writer = StatusWriter::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        let count = writer.pause_all().unwrap();

        // Should pause active and idle, skip paused and stopped
        assert_eq!(count, 2);

        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        let workers = reader.read_all().unwrap();

        for worker in workers {
            match worker.worker_id.as_str() {
                "worker-active" | "worker-idle" | "worker-paused" => {
                    assert_eq!(worker.status, WorkerStatus::Paused);
                }
                "worker-stopped" => {
                    assert_eq!(worker.status, WorkerStatus::Stopped);
                }
                _ => panic!("Unexpected worker: {}", worker.worker_id),
            }
        }
    }

    #[test]
    fn test_status_writer_resume_all() {
        let tmp_dir = TempDir::new().unwrap();

        // Create paused and non-paused workers
        create_test_status_file(
            tmp_dir.path(),
            "worker-paused1",
            r#"{"worker_id": "worker-paused1", "status": "paused"}"#,
        );
        create_test_status_file(
            tmp_dir.path(),
            "worker-paused2",
            r#"{"worker_id": "worker-paused2", "status": "paused"}"#,
        );
        create_test_status_file(
            tmp_dir.path(),
            "worker-active",
            r#"{"worker_id": "worker-active", "status": "active"}"#,
        );

        let writer = StatusWriter::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        let count = writer.resume_all().unwrap();

        // Should only resume paused workers
        assert_eq!(count, 2);

        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();

        let p1 = reader.read_worker("worker-paused1").unwrap().unwrap();
        assert_eq!(p1.status, WorkerStatus::Idle);

        let p2 = reader.read_worker("worker-paused2").unwrap().unwrap();
        assert_eq!(p2.status, WorkerStatus::Idle);

        let active = reader.read_worker("worker-active").unwrap().unwrap();
        assert_eq!(active.status, WorkerStatus::Active); // Unchanged
    }

    #[test]
    fn test_status_writer_updates_last_activity() {
        let tmp_dir = TempDir::new().unwrap();

        // Create a worker without last_activity
        create_test_status_file(
            tmp_dir.path(),
            "test-worker",
            r#"{"worker_id": "test-worker", "status": "active"}"#,
        );

        let writer = StatusWriter::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        writer.pause_worker("test-worker").unwrap();

        let reader = StatusReader::new(Some(tmp_dir.path().to_path_buf())).unwrap();
        let worker = reader.read_worker("test-worker").unwrap().unwrap();

        // Should have last_activity set
        assert!(worker.last_activity.is_some());
    }
}
