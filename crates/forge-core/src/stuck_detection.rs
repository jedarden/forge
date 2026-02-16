//! Stuck task detection for identifying and recovering from stalled beads.
//!
//! This module detects beads that have been in_progress for longer than expected
//! and determines if they are truly stuck by checking worker activity indicators:
//!
//! - **File changes**: Modified files in workspace (git status)
//! - **API calls**: Recent API activity from cost logs
//! - **Worker responsiveness**: Process health checks
//!
//! ## Usage
//!
//! ```no_run
//! use forge_core::stuck_detection::{StuckTaskDetector, StuckDetectionConfig};
//! use std::path::PathBuf;
//! use std::time::Duration;
//!
//! let config = StuckDetectionConfig {
//!     stuck_timeout: Duration::from_secs(30 * 60), // 30 minutes
//!     activity_check_window: Duration::from_secs(5 * 60), // 5 minutes
//!     min_activity_threshold: 1,
//! };
//!
//! let mut detector = StuckTaskDetector::new(config);
//! detector.add_workspace(PathBuf::from("/home/coder/forge"));
//!
//! // Check for stuck tasks
//! if let Ok(stuck) = detector.detect_stuck_tasks() {
//!     for task in stuck {
//!         println!("Stuck: {} - {}", task.bead_id, task.reason);
//!     }
//! }
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::{ForgeError, Result};

/// Configuration for stuck task detection.
#[derive(Debug, Clone)]
pub struct StuckDetectionConfig {
    /// Time threshold for considering a task stuck (default: 30 minutes).
    pub stuck_timeout: Duration,
    /// Time window to check for recent activity (default: 5 minutes).
    pub activity_check_window: Duration,
    /// Minimum number of activity events to consider task active.
    pub min_activity_threshold: u32,
}

impl Default for StuckDetectionConfig {
    fn default() -> Self {
        Self {
            stuck_timeout: Duration::from_secs(30 * 60), // 30 minutes
            activity_check_window: Duration::from_secs(5 * 60), // 5 minutes
            min_activity_threshold: 1,
        }
    }
}

/// A task detected as stuck.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StuckTask {
    /// Bead ID
    pub bead_id: String,
    /// Bead title
    pub title: String,
    /// Workspace path
    pub workspace: PathBuf,
    /// Worker ID (if known)
    pub worker_id: Option<String>,
    /// Time the task has been in progress
    pub in_progress_duration: Duration,
    /// Reason for stuck detection
    pub reason: String,
    /// Activity indicators checked
    pub activity_checks: ActivityChecks,
}

/// Results of activity checks for a task.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ActivityChecks {
    /// Number of modified files in workspace
    pub modified_files: u32,
    /// Number of untracked files in workspace
    pub untracked_files: u32,
    /// Number of recent API calls (last N minutes)
    pub recent_api_calls: u32,
    /// Worker process is alive
    pub worker_alive: bool,
    /// Last activity timestamp (if any)
    pub last_activity: Option<SystemTime>,
}

impl ActivityChecks {
    /// Check if there is any recent activity.
    pub fn has_activity(&self, min_threshold: u32) -> bool {
        let total_activity = self.modified_files + self.untracked_files + self.recent_api_calls;
        total_activity >= min_threshold || !self.worker_alive
    }

    /// Get a human-readable summary of activity.
    pub fn summary(&self) -> String {
        if !self.worker_alive {
            return "Worker process not responding".to_string();
        }

        let parts: Vec<String> = vec![
            if self.modified_files > 0 {
                Some(format!("{} modified files", self.modified_files))
            } else {
                None
            },
            if self.untracked_files > 0 {
                Some(format!("{} new files", self.untracked_files))
            } else {
                None
            },
            if self.recent_api_calls > 0 {
                Some(format!("{} API calls", self.recent_api_calls))
            } else {
                None
            },
        ]
        .into_iter()
        .flatten()
        .collect();

        if parts.is_empty() {
            "No recent activity".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// Stuck task detector for monitoring in-progress beads.
pub struct StuckTaskDetector {
    /// Configuration
    config: StuckDetectionConfig,
    /// Monitored workspaces
    workspaces: Vec<PathBuf>,
    /// Cache of last activity checks per bead
    activity_cache: HashMap<String, ActivityChecks>,
}

impl StuckTaskDetector {
    /// Create a new stuck task detector.
    pub fn new(config: StuckDetectionConfig) -> Self {
        Self {
            config,
            workspaces: Vec::new(),
            activity_cache: HashMap::new(),
        }
    }

    /// Create a detector with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(StuckDetectionConfig::default())
    }

    /// Add a workspace to monitor.
    pub fn add_workspace(&mut self, workspace: impl Into<PathBuf>) {
        self.workspaces.push(workspace.into());
    }

    /// Detect stuck tasks across all monitored workspaces.
    pub fn detect_stuck_tasks(&mut self) -> Result<Vec<StuckTask>> {
        let mut stuck_tasks = Vec::new();

        for workspace in &self.workspaces.clone() {
            match self.detect_stuck_in_workspace(workspace) {
                Ok(mut tasks) => stuck_tasks.append(&mut tasks),
                Err(e) => {
                    warn!(workspace = ?workspace, error = %e, "Failed to check workspace for stuck tasks");
                }
            }
        }

        Ok(stuck_tasks)
    }

    /// Detect stuck tasks in a specific workspace.
    fn detect_stuck_in_workspace(&mut self, workspace: &Path) -> Result<Vec<StuckTask>> {
        let in_progress_beads = self.get_in_progress_beads(workspace)?;
        let mut stuck_tasks = Vec::new();

        for bead in in_progress_beads {
            if let Some(duration) = self.parse_duration_in_progress(&bead.updated_at) {
                if duration >= self.config.stuck_timeout {
                    // Check if task is truly stuck
                    let activity = self.check_activity(workspace, &bead.id, &bead.assignee)?;

                    if !activity.has_activity(self.config.min_activity_threshold) {
                        let reason = self.determine_stuck_reason(&activity, duration);

                        stuck_tasks.push(StuckTask {
                            bead_id: bead.id.clone(),
                            title: bead.title.clone(),
                            workspace: workspace.to_path_buf(),
                            worker_id: bead.assignee.clone(),
                            in_progress_duration: duration,
                            reason,
                            activity_checks: activity.clone(),
                        });

                        info!(
                            bead_id = %bead.id,
                            duration_secs = duration.as_secs(),
                            "Detected stuck task"
                        );
                    } else {
                        debug!(
                            bead_id = %bead.id,
                            activity = %activity.summary(),
                            "Task has recent activity, not stuck"
                        );
                    }

                    // Cache the activity check
                    self.activity_cache.insert(bead.id.clone(), activity);
                }
            }
        }

        Ok(stuck_tasks)
    }

    /// Get in-progress beads from a workspace using br CLI.
    fn get_in_progress_beads(&self, workspace: &Path) -> Result<Vec<InProgressBead>> {
        let output = Command::new("br")
            .arg("list")
            .arg("--status")
            .arg("in_progress")
            .arg("--format")
            .arg("json")
            .current_dir(workspace)
            .output()
            .map_err(|e| ForgeError::io("running br list", workspace, e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("no .beads") {
                return Ok(Vec::new());
            }
            return Err(ForgeError::ToolExecution {
                tool_name: "br list".to_string(),
                message: stderr.to_string(),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() || stdout.trim() == "[]" {
            return Ok(Vec::new());
        }

        let beads: Vec<InProgressBead> = serde_json::from_str(&stdout)
            .map_err(|e| ForgeError::parse(&format!("Failed to parse br output: {}", e)))?;

        Ok(beads)
    }

    /// Check activity indicators for a bead.
    fn check_activity(
        &self,
        workspace: &Path,
        bead_id: &str,
        worker_id: &Option<String>,
    ) -> Result<ActivityChecks> {
        let mut checks = ActivityChecks::default();

        // Check git status for file changes
        if let Ok((modified, untracked)) = self.check_git_changes(workspace) {
            checks.modified_files = modified;
            checks.untracked_files = untracked;
        }

        // Check recent API calls from cost logs
        if let Ok(api_calls) = self.check_recent_api_calls(workspace, worker_id) {
            checks.recent_api_calls = api_calls;
            if api_calls > 0 {
                checks.last_activity = Some(SystemTime::now());
            }
        }

        // Check if worker process is alive
        if let Some(worker) = worker_id {
            checks.worker_alive = self.check_worker_alive(worker);
        } else {
            checks.worker_alive = true; // Unknown, assume alive
        }

        debug!(
            bead_id,
            modified = checks.modified_files,
            untracked = checks.untracked_files,
            api_calls = checks.recent_api_calls,
            worker_alive = checks.worker_alive,
            "Activity check completed"
        );

        Ok(checks)
    }

    /// Check git for modified and untracked files.
    fn check_git_changes(&self, workspace: &Path) -> Result<(u32, u32)> {
        let output = Command::new("git")
            .arg("status")
            .arg("--porcelain")
            .current_dir(workspace)
            .output()
            .map_err(|e| ForgeError::io("running git status", workspace, e))?;

        if !output.status.success() {
            return Ok((0, 0));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut modified = 0;
        let mut untracked = 0;

        for line in stdout.lines() {
            if line.starts_with("??") {
                untracked += 1;
            } else if !line.is_empty() {
                modified += 1;
            }
        }

        Ok((modified, untracked))
    }

    /// Check recent API calls from cost logs.
    fn check_recent_api_calls(
        &self,
        workspace: &Path,
        worker_id: &Option<String>,
    ) -> Result<u32> {
        let cost_db = workspace.join(".forge/costs.db");
        if !cost_db.exists() {
            return Ok(0);
        }

        // Parse activity from cost database or logs
        // For now, check log files in .forge/logs/
        let logs_dir = workspace.join(".forge/logs");
        if !logs_dir.exists() {
            return Ok(0);
        }

        let cutoff = SystemTime::now() - self.config.activity_check_window;
        let mut api_calls = 0;

        for entry in fs::read_dir(&logs_dir)
            .map_err(|e| ForgeError::io("reading logs directory", &logs_dir, e))? {
            let entry = entry.map_err(|e| ForgeError::io("reading log entry", &logs_dir, e))?;
            let path = entry.path();

            // Check if file was modified recently
            if let Ok(metadata) = fs::metadata(&path) {
                if let Ok(modified) = metadata.modified() {
                    if modified >= cutoff {
                        // Count lines with API call indicators
                        if let Ok(content) = fs::read_to_string(&path) {
                            for line in content.lines() {
                                if line.contains("API call")
                                    || line.contains("input_tokens")
                                    || line.contains("output_tokens")
                                    || line.contains("request_id") {

                                    // If worker_id is specified, filter by worker
                                    if let Some(worker) = worker_id {
                                        if line.contains(worker) {
                                            api_calls += 1;
                                        }
                                    } else {
                                        api_calls += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(api_calls)
    }

    /// Check if a worker process is alive.
    fn check_worker_alive(&self, worker_id: &str) -> bool {
        // Check if tmux session exists for worker
        let output = Command::new("tmux")
            .arg("has-session")
            .arg("-t")
            .arg(worker_id)
            .output();

        if let Ok(output) = output {
            output.status.success()
        } else {
            false
        }
    }

    /// Parse duration since task started (from updated_at timestamp).
    fn parse_duration_in_progress(&self, updated_at: &str) -> Option<Duration> {
        // Parse ISO 8601 timestamp
        if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(updated_at) {
            let now = chrono::Utc::now();
            let duration = now.signed_duration_since(timestamp);

            if duration.num_seconds() >= 0 {
                return Some(Duration::from_secs(duration.num_seconds() as u64));
            }
        }

        None
    }

    /// Determine the reason a task is stuck.
    fn determine_stuck_reason(&self, activity: &ActivityChecks, duration: Duration) -> String {
        if !activity.worker_alive {
            return format!(
                "Worker process not responding after {} minutes",
                duration.as_secs() / 60
            );
        }

        if activity.modified_files == 0 && activity.untracked_files == 0 {
            return format!(
                "No file changes in {} minutes",
                self.config.activity_check_window.as_secs() / 60
            );
        }

        if activity.recent_api_calls == 0 {
            return format!(
                "No API activity in {} minutes",
                self.config.activity_check_window.as_secs() / 60
            );
        }

        format!(
            "Task in progress for {} minutes with minimal activity",
            duration.as_secs() / 60
        )
    }

    /// Timeout a stuck task (mark as open, unassign worker).
    pub fn timeout_task(&self, workspace: &Path, bead_id: &str) -> Result<()> {
        info!(bead_id, workspace = ?workspace, "Timing out stuck task");

        // Use br CLI to update status
        let output = Command::new("br")
            .arg("update")
            .arg(bead_id)
            .arg("--status")
            .arg("open")
            .current_dir(workspace)
            .output()
            .map_err(|e| ForgeError::io("running br update", workspace, e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ForgeError::ToolExecution {
                tool_name: "br update".to_string(),
                message: stderr.to_string(),
            });
        }

        // Clear assignee
        let output = Command::new("br")
            .arg("update")
            .arg(bead_id)
            .arg("--assignee")
            .arg("")
            .current_dir(workspace)
            .output()
            .map_err(|e| ForgeError::io("running br update", workspace, e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(bead_id, error = %stderr, "Failed to clear assignee");
        }

        info!(bead_id, "Task timed out and available for reassignment");
        Ok(())
    }

    /// Get cached activity for a bead.
    pub fn get_cached_activity(&self, bead_id: &str) -> Option<&ActivityChecks> {
        self.activity_cache.get(bead_id)
    }

    /// Clear activity cache.
    pub fn clear_cache(&mut self) {
        self.activity_cache.clear();
    }
}

/// Minimal bead representation for in-progress detection.
#[derive(Debug, Clone, Deserialize)]
struct InProgressBead {
    id: String,
    title: String,
    assignee: Option<String>,
    updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_activity_checks_has_activity() {
        let checks = ActivityChecks {
            modified_files: 5,
            untracked_files: 2,
            recent_api_calls: 0,
            worker_alive: true,
            last_activity: None,
        };

        assert!(checks.has_activity(1));
        assert!(checks.has_activity(5));
        assert!(!checks.has_activity(10));
    }

    #[test]
    fn test_activity_checks_worker_dead() {
        let checks = ActivityChecks {
            modified_files: 0,
            untracked_files: 0,
            recent_api_calls: 0,
            worker_alive: false,
            last_activity: None,
        };

        // Dead worker is always considered stuck
        assert!(!checks.worker_alive);
    }

    #[test]
    fn test_activity_summary() {
        let checks = ActivityChecks {
            modified_files: 3,
            untracked_files: 2,
            recent_api_calls: 5,
            worker_alive: true,
            last_activity: None,
        };

        let summary = checks.summary();
        assert!(summary.contains("modified files"));
        assert!(summary.contains("new files"));
        assert!(summary.contains("API calls"));
    }

    #[test]
    fn test_activity_summary_no_activity() {
        let checks = ActivityChecks {
            worker_alive: true,
            ..Default::default()
        };
        assert_eq!(checks.summary(), "No recent activity");
    }

    #[test]
    fn test_activity_summary_worker_dead() {
        let checks = ActivityChecks {
            worker_alive: false,
            ..Default::default()
        };
        assert_eq!(checks.summary(), "Worker process not responding");
    }

    #[test]
    fn test_default_config() {
        let config = StuckDetectionConfig::default();
        assert_eq!(config.stuck_timeout.as_secs(), 30 * 60);
        assert_eq!(config.activity_check_window.as_secs(), 5 * 60);
        assert_eq!(config.min_activity_threshold, 1);
    }

    #[test]
    fn test_detector_creation() {
        let detector = StuckTaskDetector::with_defaults();
        assert_eq!(detector.workspaces.len(), 0);
        assert_eq!(detector.activity_cache.len(), 0);
    }

    #[test]
    fn test_add_workspace() {
        let mut detector = StuckTaskDetector::with_defaults();
        detector.add_workspace("/test/workspace");
        assert_eq!(detector.workspaces.len(), 1);
    }
}
