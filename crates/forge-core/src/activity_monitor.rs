//! Activity monitoring for worker health tracking.
//!
//! This module provides comprehensive activity monitoring that distinguishes between:
//! - **Idle**: Worker waiting for work (no task assigned)
//! - **Active**: Worker working on a task with recent activity
//! - **Stuck**: Worker has a task but no activity for > 15 minutes
//!
//! ## Heartbeat Files
//!
//! Workers write heartbeat files to `~/.forge/heartbeat/<worker_id>.heartbeat`
//! which are updated every 30 seconds during active work. This provides:
//! - Independent verification of worker activity (separate from status files)
//! - More granular activity tracking during long operations
//! - Detection of workers that crashed without updating status
//!
//! ## Activity Classification
//!
//! | Status | Has Task | Last Activity | Classification |
//! |--------|----------|---------------|----------------|
//! | Active | Yes      | < 15 min      | Working        |
//! | Active | Yes      | >= 15 min     | **Stuck**      |
//! | Idle   | No       | Any           | Idle           |
//! | Active | No       | Any           | Idle (finishing)|

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use crate::{ForgeError, Result};

/// Default activity timeout threshold (15 minutes in seconds).
pub const DEFAULT_ACTIVITY_TIMEOUT_SECS: i64 = 900;

/// Default heartbeat interval (30 seconds).
pub const DEFAULT_HEARTBEAT_INTERVAL_SECS: u64 = 30;

/// Default heartbeat stale threshold (2 minutes - 4x heartbeat interval).
pub const DEFAULT_HEARTBEAT_STALE_SECS: i64 = 120;

/// Activity state of a worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityState {
    /// Worker is idle, waiting for tasks
    Idle,
    /// Worker is actively working with recent activity
    Working,
    /// Worker has a task but no recent activity (potentially stuck)
    Stuck,
    /// Worker has no heartbeat (potentially crashed)
    Unresponsive,
    /// Worker state is unknown
    Unknown,
}

impl ActivityState {
    /// Returns true if the worker needs attention.
    pub fn needs_attention(&self) -> bool {
        matches!(self, Self::Stuck | Self::Unresponsive)
    }

    /// Returns the display indicator for TUI.
    pub fn indicator(&self) -> &'static str {
        match self {
            Self::Idle => "💤",
            Self::Working => "⟳",
            Self::Stuck => "⚠️",
            Self::Unresponsive => "❌",
            Self::Unknown => "?",
        }
    }

    /// Returns a short label for display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Working => "Working",
            Self::Stuck => "Stuck",
            Self::Unresponsive => "Unresponsive",
            Self::Unknown => "Unknown",
        }
    }
}

impl std::fmt::Display for ActivityState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Detailed activity status for a worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerActivity {
    /// Worker identifier
    pub worker_id: String,
    /// Current activity state
    pub state: ActivityState,
    /// Time since last activity
    pub time_since_activity: Option<Duration>,
    /// Time since last heartbeat
    pub time_since_heartbeat: Option<Duration>,
    /// Whether the worker has a task assigned
    pub has_task: bool,
    /// Current task ID if any
    pub current_task: Option<String>,
    /// Last activity timestamp
    pub last_activity: Option<DateTime<Utc>>,
    /// Last heartbeat timestamp
    pub last_heartbeat: Option<DateTime<Utc>>,
    /// Recommended action
    pub recommendation: Option<String>,
}

impl WorkerActivity {
    /// Create a new worker activity record.
    pub fn new(worker_id: impl Into<String>) -> Self {
        Self {
            worker_id: worker_id.into(),
            state: ActivityState::Unknown,
            time_since_activity: None,
            time_since_heartbeat: None,
            has_task: false,
            current_task: None,
            last_activity: None,
            last_heartbeat: None,
            recommendation: None,
        }
    }

    /// Check if this worker needs attention.
    pub fn needs_attention(&self) -> bool {
        self.state.needs_attention()
    }

    /// Get the time since activity as a human-readable string.
    pub fn activity_age_string(&self) -> String {
        match self.time_since_activity {
            Some(d) if d.num_hours() >= 1 => format!("{}h {}m", d.num_hours(), d.num_minutes() % 60),
            Some(d) if d.num_minutes() >= 1 => format!("{}m", d.num_minutes()),
            Some(d) => format!("{}s", d.num_seconds()),
            None => "N/A".to_string(),
        }
    }
}

/// Configuration for activity monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityMonitorConfig {
    /// Timeout threshold for considering a worker stuck (default: 15 minutes).
    pub activity_timeout_secs: i64,
    /// Heartbeat stale threshold (default: 2 minutes).
    pub heartbeat_stale_secs: i64,
    /// Path to heartbeat directory.
    pub heartbeat_dir: PathBuf,
    /// Path to status directory.
    pub status_dir: PathBuf,
}

impl Default for ActivityMonitorConfig {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let forge_dir = PathBuf::from(&home).join(".forge");

        Self {
            activity_timeout_secs: DEFAULT_ACTIVITY_TIMEOUT_SECS,
            heartbeat_stale_secs: DEFAULT_HEARTBEAT_STALE_SECS,
            heartbeat_dir: forge_dir.join("heartbeat"),
            status_dir: forge_dir.join("status"),
        }
    }
}

/// Heartbeat file content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatData {
    /// Worker identifier
    pub worker_id: String,
    /// Timestamp of this heartbeat
    pub timestamp: DateTime<Utc>,
    /// Current task if any
    pub current_task: Option<String>,
    /// Current operation description
    pub operation: Option<String>,
    /// Additional metrics
    #[serde(default)]
    pub metrics: HeartbeatMetrics,
}

/// Metrics included in heartbeat.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HeartbeatMetrics {
    /// API calls in the last interval
    #[serde(default)]
    pub api_calls: u32,
    /// Tokens processed in the last interval
    #[serde(default)]
    pub tokens_processed: u64,
    /// Memory usage in MB
    #[serde(default)]
    pub memory_mb: u64,
}

impl HeartbeatData {
    /// Create a new heartbeat.
    pub fn new(worker_id: impl Into<String>) -> Self {
        Self {
            worker_id: worker_id.into(),
            timestamp: Utc::now(),
            current_task: None,
            operation: None,
            metrics: HeartbeatMetrics::default(),
        }
    }

    /// Set the current task.
    pub fn with_task(mut self, task: impl Into<String>) -> Self {
        self.current_task = Some(task.into());
        self
    }

    /// Set the current operation.
    pub fn with_operation(mut self, op: impl Into<String>) -> Self {
        self.operation = Some(op.into());
        self
    }

    /// Set metrics.
    pub fn with_metrics(mut self, metrics: HeartbeatMetrics) -> Self {
        self.metrics = metrics;
        self
    }
}

/// Heartbeat writer for workers to use.
pub struct HeartbeatWriter {
    worker_id: String,
    heartbeat_dir: PathBuf,
}

impl HeartbeatWriter {
    /// Create a new heartbeat writer.
    pub fn new(worker_id: impl Into<String>, heartbeat_dir: impl Into<PathBuf>) -> Result<Self> {
        let heartbeat_dir = heartbeat_dir.into();

        // Create heartbeat directory if it doesn't exist
        if !heartbeat_dir.exists() {
            fs::create_dir_all(&heartbeat_dir).map_err(|e| {
                ForgeError::io("creating heartbeat directory", &heartbeat_dir, e)
            })?;
        }

        Ok(Self {
            worker_id: worker_id.into(),
            heartbeat_dir,
        })
    }

    /// Create a heartbeat writer with default config.
    pub fn with_defaults(worker_id: impl Into<String>) -> Result<Self> {
        let config = ActivityMonitorConfig::default();
        Self::new(worker_id, config.heartbeat_dir)
    }

    /// Write a heartbeat.
    pub fn write(&self, data: &HeartbeatData) -> Result<()> {
        let path = self.heartbeat_path();
        let json = serde_json::to_string_pretty(data).map_err(|e| {
            ForgeError::parse(&format!("Failed to serialize heartbeat: {}", e))
        })?;

        // Write atomically using a temp file
        let temp_path = path.with_extension("tmp");
        let mut file = fs::File::create(&temp_path).map_err(|e| {
            ForgeError::io("creating heartbeat temp file", &temp_path, e)
        })?;
        file.write_all(json.as_bytes()).map_err(|e| {
            ForgeError::io("writing heartbeat", &temp_path, e)
        })?;
        file.sync_all().map_err(|e| {
            ForgeError::io("syncing heartbeat", &temp_path, e)
        })?;
        drop(file);

        fs::rename(&temp_path, &path).map_err(|e| {
            ForgeError::io("renaming heartbeat file", &path, e)
        })?;

        debug!(worker_id = %self.worker_id, "Heartbeat written");
        Ok(())
    }

    /// Write a simple heartbeat (just timestamp).
    pub fn beat(&self) -> Result<()> {
        let data = HeartbeatData::new(&self.worker_id);
        self.write(&data)
    }

    /// Write a heartbeat with task info.
    pub fn beat_with_task(&self, task: impl Into<String>) -> Result<()> {
        let data = HeartbeatData::new(&self.worker_id).with_task(task);
        self.write(&data)
    }

    /// Get the heartbeat file path.
    pub fn heartbeat_path(&self) -> PathBuf {
        self.heartbeat_dir.join(format!("{}.heartbeat", self.worker_id))
    }

    /// Remove the heartbeat file (on clean shutdown).
    pub fn remove(&self) -> Result<()> {
        let path = self.heartbeat_path();
        if path.exists() {
            fs::remove_file(&path).map_err(|e| {
                ForgeError::io("removing heartbeat file", &path, e)
            })?;
            debug!(worker_id = %self.worker_id, "Heartbeat file removed");
        }
        Ok(())
    }
}

/// Activity monitor that combines status files and heartbeats.
pub struct ActivityMonitor {
    config: ActivityMonitorConfig,
}

impl ActivityMonitor {
    /// Create a new activity monitor.
    pub fn new(config: ActivityMonitorConfig) -> Self {
        Self { config }
    }

    /// Create with default config.
    pub fn with_defaults() -> Self {
        Self::new(ActivityMonitorConfig::default())
    }

    /// Read heartbeat data for a worker.
    pub fn read_heartbeat(&self, worker_id: &str) -> Option<HeartbeatData> {
        let path = self.config.heartbeat_dir.join(format!("{}.heartbeat", worker_id));

        if !path.exists() {
            return None;
        }

        match fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(data) => Some(data),
                Err(e) => {
                    warn!(worker_id, error = %e, "Failed to parse heartbeat file");
                    None
                }
            },
            Err(e) => {
                warn!(worker_id, error = %e, "Failed to read heartbeat file");
                None
            }
        }
    }

    /// Classify the activity state of a worker.
    pub fn classify_activity(
        &self,
        worker_id: &str,
        has_task: bool,
        last_activity: Option<DateTime<Utc>>,
        worker_status: &str,
    ) -> ActivityState {
        let now = Utc::now();
        let heartbeat = self.read_heartbeat(worker_id);
        let timeout = Duration::seconds(self.config.activity_timeout_secs);
        let heartbeat_stale = Duration::seconds(self.config.heartbeat_stale_secs);

        // Check heartbeat staleness
        if let Some(ref hb) = heartbeat {
            let hb_age = now.signed_duration_since(hb.timestamp);
            if hb_age > heartbeat_stale {
                // Heartbeat is stale - worker might be unresponsive
                if has_task {
                    return ActivityState::Stuck;
                }
            }
        }

        // If worker has no task, it's idle
        if !has_task {
            return ActivityState::Idle;
        }

        // Worker has a task - check activity
        if let Some(activity) = last_activity {
            let activity_age = now.signed_duration_since(activity);

            if activity_age > timeout {
                // No activity for > 15 minutes with a task = stuck
                return ActivityState::Stuck;
            } else {
                // Recent activity = working
                return ActivityState::Working;
            }
        }

        // Has task but no activity timestamp - check heartbeat
        if let Some(hb) = heartbeat {
            let hb_age = now.signed_duration_since(hb.timestamp);
            if hb_age <= heartbeat_stale {
                return ActivityState::Working;
            } else {
                return ActivityState::Stuck;
            }
        }

        // Has task, no activity, no heartbeat - likely stuck or starting
        if worker_status == "starting" {
            ActivityState::Working
        } else {
            ActivityState::Unknown
        }
    }

    /// Get detailed activity status for a worker.
    pub fn get_activity(
        &self,
        worker_id: &str,
        has_task: bool,
        current_task: Option<String>,
        last_activity: Option<DateTime<Utc>>,
        worker_status: &str,
    ) -> WorkerActivity {
        let now = Utc::now();
        let heartbeat = self.read_heartbeat(worker_id);

        let time_since_activity = last_activity.map(|t| now.signed_duration_since(t));
        let time_since_heartbeat = heartbeat.as_ref().map(|hb| now.signed_duration_since(hb.timestamp));

        let state = self.classify_activity(worker_id, has_task, last_activity, worker_status);

        let recommendation = match state {
            ActivityState::Stuck => Some(format!(
                "Worker has been stuck for {}. Consider restarting.",
                time_since_activity
                    .map(|d| format!("{} minutes", d.num_minutes()))
                    .unwrap_or_else(|| "unknown time".to_string())
            )),
            ActivityState::Unresponsive => Some("Worker is not responding. Check process health.".to_string()),
            _ => None,
        };

        WorkerActivity {
            worker_id: worker_id.to_string(),
            state,
            time_since_activity,
            time_since_heartbeat,
            has_task,
            current_task,
            last_activity,
            last_heartbeat: heartbeat.map(|hb| hb.timestamp),
            recommendation,
        }
    }

    /// Scan all heartbeat files and return activity data.
    pub fn scan_all_heartbeats(&self) -> HashMap<String, HeartbeatData> {
        let mut heartbeats = HashMap::new();

        if !self.config.heartbeat_dir.exists() {
            return heartbeats;
        }

        if let Ok(entries) = fs::read_dir(&self.config.heartbeat_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "heartbeat") {
                    if let Some(stem) = path.file_stem() {
                        let worker_id = stem.to_string_lossy().to_string();
                        if let Some(hb) = self.read_heartbeat(&worker_id) {
                            heartbeats.insert(worker_id, hb);
                        }
                    }
                }
            }
        }

        heartbeats
    }

    /// Find workers that are potentially stuck.
    pub fn find_stuck_workers(&self, workers: &[(String, bool, Option<DateTime<Utc>>, String)]) -> Vec<WorkerActivity> {
        workers
            .iter()
            .filter_map(|(worker_id, has_task, last_activity, status)| {
                let activity = self.get_activity(
                    worker_id,
                    *has_task,
                    None, // We don't have task ID here
                    *last_activity,
                    status,
                );
                if activity.needs_attention() {
                    Some(activity)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Clean up stale heartbeat files (workers that no longer exist).
    pub fn cleanup_stale_heartbeats(&self, active_worker_ids: &[String]) -> Result<usize> {
        let mut cleaned = 0;

        if !self.config.heartbeat_dir.exists() {
            return Ok(0);
        }

        if let Ok(entries) = fs::read_dir(&self.config.heartbeat_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "heartbeat") {
                    if let Some(stem) = path.file_stem() {
                        let worker_id = stem.to_string_lossy().to_string();
                        if !active_worker_ids.contains(&worker_id) {
                            // Check if heartbeat is old enough to clean up
                            if let Some(hb) = self.read_heartbeat(&worker_id) {
                                let age = Utc::now().signed_duration_since(hb.timestamp);
                                // Clean up heartbeats older than 1 hour for non-existent workers
                                if age.num_hours() >= 1 {
                                    if let Err(e) = fs::remove_file(&path) {
                                        warn!(path = ?path, error = %e, "Failed to clean up stale heartbeat");
                                    } else {
                                        info!(worker_id = %worker_id, "Cleaned up stale heartbeat file");
                                        cleaned += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(cleaned)
    }

    /// Get the heartbeat directory path.
    pub fn heartbeat_dir(&self) -> &Path {
        &self.config.heartbeat_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_config(temp_dir: &TempDir) -> ActivityMonitorConfig {
        ActivityMonitorConfig {
            activity_timeout_secs: 900,
            heartbeat_stale_secs: 120,
            heartbeat_dir: temp_dir.path().join("heartbeat"),
            status_dir: temp_dir.path().join("status"),
        }
    }

    #[test]
    fn test_activity_state_display() {
        assert_eq!(ActivityState::Idle.label(), "Idle");
        assert_eq!(ActivityState::Working.label(), "Working");
        assert_eq!(ActivityState::Stuck.label(), "Stuck");
        assert_eq!(ActivityState::Unresponsive.label(), "Unresponsive");
    }

    #[test]
    fn test_activity_state_needs_attention() {
        assert!(!ActivityState::Idle.needs_attention());
        assert!(!ActivityState::Working.needs_attention());
        assert!(ActivityState::Stuck.needs_attention());
        assert!(ActivityState::Unresponsive.needs_attention());
    }

    #[test]
    fn test_heartbeat_writer() {
        let temp_dir = TempDir::new().unwrap();
        let heartbeat_dir = temp_dir.path().join("heartbeat");

        let writer = HeartbeatWriter::new("test-worker", &heartbeat_dir).unwrap();

        // Write a heartbeat
        writer.beat().unwrap();
        assert!(writer.heartbeat_path().exists());

        // Write a heartbeat with task
        writer.beat_with_task("bd-123").unwrap();

        // Read it back
        let content = fs::read_to_string(writer.heartbeat_path()).unwrap();
        let data: HeartbeatData = serde_json::from_str(&content).unwrap();
        assert_eq!(data.worker_id, "test-worker");
        assert_eq!(data.current_task, Some("bd-123".to_string()));

        // Remove heartbeat
        writer.remove().unwrap();
        assert!(!writer.heartbeat_path().exists());
    }

    #[test]
    fn test_activity_classification_idle() {
        let temp_dir = TempDir::new().unwrap();
        let config = test_config(&temp_dir);
        let monitor = ActivityMonitor::new(config);

        // Worker with no task = Idle
        let state = monitor.classify_activity(
            "worker-1",
            false, // no task
            Some(Utc::now()),
            "idle",
        );
        assert_eq!(state, ActivityState::Idle);
    }

    #[test]
    fn test_activity_classification_working() {
        let temp_dir = TempDir::new().unwrap();
        let config = test_config(&temp_dir);
        let monitor = ActivityMonitor::new(config);

        // Worker with task and recent activity = Working
        let state = monitor.classify_activity(
            "worker-1",
            true, // has task
            Some(Utc::now()), // recent activity
            "active",
        );
        assert_eq!(state, ActivityState::Working);
    }

    #[test]
    fn test_activity_classification_stuck() {
        let temp_dir = TempDir::new().unwrap();
        let config = test_config(&temp_dir);
        let monitor = ActivityMonitor::new(config);

        // Worker with task but old activity = Stuck
        let old_activity = Utc::now() - Duration::minutes(20); // > 15 min timeout
        let state = monitor.classify_activity(
            "worker-1",
            true, // has task
            Some(old_activity),
            "active",
        );
        assert_eq!(state, ActivityState::Stuck);
    }

    #[test]
    fn test_get_activity_details() {
        let temp_dir = TempDir::new().unwrap();
        let config = test_config(&temp_dir);
        let monitor = ActivityMonitor::new(config);

        let old_activity = Utc::now() - Duration::minutes(20);
        let activity = monitor.get_activity(
            "worker-1",
            true,
            Some("bd-123".to_string()),
            Some(old_activity),
            "active",
        );

        assert_eq!(activity.state, ActivityState::Stuck);
        assert!(activity.needs_attention());
        assert!(activity.recommendation.is_some());
        assert!(activity.time_since_activity.unwrap().num_minutes() >= 20);
    }

    #[test]
    fn test_scan_heartbeats() {
        let temp_dir = TempDir::new().unwrap();
        let config = test_config(&temp_dir);

        // Create heartbeat directory and files
        fs::create_dir_all(&config.heartbeat_dir).unwrap();

        let data1 = HeartbeatData::new("worker-1").with_task("bd-1");
        let data2 = HeartbeatData::new("worker-2").with_task("bd-2");

        fs::write(
            config.heartbeat_dir.join("worker-1.heartbeat"),
            serde_json::to_string(&data1).unwrap(),
        ).unwrap();
        fs::write(
            config.heartbeat_dir.join("worker-2.heartbeat"),
            serde_json::to_string(&data2).unwrap(),
        ).unwrap();

        let monitor = ActivityMonitor::new(config);
        let heartbeats = monitor.scan_all_heartbeats();

        assert_eq!(heartbeats.len(), 2);
        assert!(heartbeats.contains_key("worker-1"));
        assert!(heartbeats.contains_key("worker-2"));
    }

    #[test]
    fn test_worker_activity_age_string() {
        let mut activity = WorkerActivity::new("test");

        // No activity
        assert_eq!(activity.activity_age_string(), "N/A");

        // Seconds
        activity.time_since_activity = Some(Duration::seconds(30));
        assert_eq!(activity.activity_age_string(), "30s");

        // Minutes
        activity.time_since_activity = Some(Duration::minutes(5));
        assert_eq!(activity.activity_age_string(), "5m");

        // Hours
        activity.time_since_activity = Some(Duration::hours(2) + Duration::minutes(30));
        assert_eq!(activity.activity_age_string(), "2h 30m");
    }
}
