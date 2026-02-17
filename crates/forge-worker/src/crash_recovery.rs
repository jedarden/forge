//! Worker crash recovery with auto-restart and assignee management.
//!
//! This module implements comprehensive crash detection and recovery for worker processes:
//!
//! ## Features
//!
//! 1. **Crash Detection**: Detects when worker processes die unexpectedly
//! 2. **Assignee Clearing**: Automatically clears stale assignees from beads via br CLI
//! 3. **Status Updates**: Updates bead status from in_progress to open
//! 4. **Crash Notifications**: Shows user-visible alerts about crashes
//! 5. **Auto-Restart**: Optionally restarts workers with rate limiting
//!
//! ## Rate Limiting
//!
//! Auto-restart is rate-limited to prevent crash loops:
//! - Maximum 3 crashes within 10 minutes
//! - After 3 crashes, auto-restart is disabled
//! - User must manually intervene
//!
//! ## Usage
//!
//! ```no_run
//! use forge_worker::crash_recovery::{CrashRecoveryManager, CrashAction};
//! use forge_worker::health::HealthMonitor;
//!
//! #[tokio::main]
//! async fn main() -> forge_core::Result<()> {
//!     let mut recovery_manager = CrashRecoveryManager::new();
//!     let mut health_monitor = HealthMonitor::new(Default::default())?;
//!
//!     // In your main loop
//!     loop {
//!         // Check health of all workers
//!         let health_results = health_monitor.check_all_health()?;
//!
//!         // Detect crashes and recover
//!         for (worker_id, health) in health_results {
//!             if !health.is_healthy {
//!                 // Process crash and determine if restart needed
//!                 let action = recovery_manager.handle_crash(
//!                     &worker_id,
//!                     &health,
//!                     Some("/path/to/workspace".into()),
//!                     Some("bead-id".into()),
//!                 ).await?;
//!
//!                 match action {
//!                     CrashAction::Restart => {
//!                         // Restart the worker
//!                     }
//!                     CrashAction::NotifyOnly => {
//!                         // Just show notification
//!                     }
//!                     CrashAction::Ignore => {
//!                         // Do nothing
//!                     }
//!                 }
//!             }
//!         }
//!
//!         tokio::time::sleep(std::time::Duration::from_secs(30)).await;
//!     }
//! }
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use tracing::{debug, error, info, warn};

use forge_core::types::{BeadId, WorkerId};
use forge_core::Result;

use crate::health::{HealthCheckType, HealthErrorType, WorkerHealthStatus};

/// Time window for crash rate limiting (10 minutes).
pub const CRASH_WINDOW_SECS: i64 = 600;

/// Maximum crashes allowed within the crash window.
pub const MAX_CRASHES_IN_WINDOW: usize = 3;

/// Default auto-restart enabled flag.
pub const DEFAULT_AUTO_RESTART_ENABLED: bool = false;

/// Record of a single crash event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashRecord {
    /// Worker that crashed
    pub worker_id: WorkerId,
    /// Timestamp of the crash
    pub crashed_at: DateTime<Utc>,
    /// Reason for crash (health check failure type)
    pub reason: String,
    /// Error message
    pub error_message: String,
    /// Workspace where worker was running
    pub workspace: Option<PathBuf>,
    /// Bead that was assigned (if any)
    pub bead_id: Option<BeadId>,
    /// Whether assignee was cleared
    pub assignee_cleared: bool,
    /// Whether worker was auto-restarted
    pub auto_restarted: bool,
}

impl CrashRecord {
    /// Create a new crash record.
    pub fn new(
        worker_id: impl Into<WorkerId>,
        reason: impl Into<String>,
        error_message: impl Into<String>,
        workspace: Option<PathBuf>,
        bead_id: Option<BeadId>,
    ) -> Self {
        Self {
            worker_id: worker_id.into(),
            crashed_at: Utc::now(),
            reason: reason.into(),
            error_message: error_message.into(),
            workspace,
            bead_id,
            assignee_cleared: false,
            auto_restarted: false,
        }
    }

    /// Get age of crash in seconds.
    pub fn age_secs(&self) -> i64 {
        Utc::now().signed_duration_since(self.crashed_at).num_seconds()
    }

    /// Check if crash is within the rate limit window.
    pub fn is_within_window(&self) -> bool {
        self.age_secs() < CRASH_WINDOW_SECS
    }

    /// Format crash for display.
    pub fn format(&self) -> String {
        format!(
            "{} crashed: {} ({})",
            self.worker_id, self.reason, self.error_message
        )
    }
}

/// Action to take after detecting a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashAction {
    /// Restart the worker automatically
    Restart,
    /// Show notification only (no restart)
    NotifyOnly,
    /// Ignore (not a real crash or already handled)
    Ignore,
}

/// Configuration for crash recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashRecoveryConfig {
    /// Enable automatic worker restart after crash
    pub auto_restart_enabled: bool,
    /// Maximum crashes before disabling auto-restart
    pub max_crashes_in_window: usize,
    /// Time window for crash counting (seconds)
    pub crash_window_secs: i64,
    /// Automatically clear bead assignees on crash
    pub clear_assignees_enabled: bool,
    /// Show crash notifications in UI
    pub show_notifications: bool,
}

impl Default for CrashRecoveryConfig {
    fn default() -> Self {
        Self {
            auto_restart_enabled: DEFAULT_AUTO_RESTART_ENABLED,
            max_crashes_in_window: MAX_CRASHES_IN_WINDOW,
            crash_window_secs: CRASH_WINDOW_SECS,
            clear_assignees_enabled: true,
            show_notifications: true,
        }
    }
}

/// Manager for crash detection and recovery.
pub struct CrashRecoveryManager {
    /// Configuration
    config: CrashRecoveryConfig,
    /// Crash history per worker
    crash_history: HashMap<WorkerId, Vec<CrashRecord>>,
    /// Workers currently marked as crashed
    crashed_workers: HashMap<WorkerId, CrashRecord>,
}

impl CrashRecoveryManager {
    /// Create a new crash recovery manager with default configuration.
    pub fn new() -> Self {
        Self::with_config(CrashRecoveryConfig::default())
    }

    /// Create a crash recovery manager with custom configuration.
    pub fn with_config(config: CrashRecoveryConfig) -> Self {
        Self {
            config,
            crash_history: HashMap::new(),
            crashed_workers: HashMap::new(),
        }
    }

    /// Handle a potential crash detected via health monitoring.
    ///
    /// Returns the action that should be taken (restart, notify, or ignore).
    pub async fn handle_crash(
        &mut self,
        worker_id: &str,
        health: &WorkerHealthStatus,
        workspace: Option<PathBuf>,
        bead_id: Option<BeadId>,
    ) -> Result<CrashAction> {
        // Check if this is actually a crash (process died)
        if !Self::is_crash(health) {
            return Ok(CrashAction::Ignore);
        }

        // Check if we've already handled this crash
        if self.crashed_workers.contains_key(worker_id) {
            debug!("Crash for {} already handled", worker_id);
            return Ok(CrashAction::Ignore);
        }

        // Extract crash details from health status
        let (reason, error_message) = Self::extract_crash_details(health);

        // Create crash record
        let mut record = CrashRecord::new(
            worker_id,
            reason,
            error_message,
            workspace.clone(),
            bead_id.clone(),
        );

        info!(
            "Worker {} crashed: {} (bead: {:?})",
            worker_id, record.reason, record.bead_id
        );

        // Clear bead assignee if configured
        if self.config.clear_assignees_enabled {
            if let Some(ref bead) = record.bead_id {
                if let Some(ref workspace_path) = workspace {
                    match self.clear_bead_assignee(workspace_path, bead).await {
                        Ok(true) => {
                            record.assignee_cleared = true;
                            info!("Cleared assignee for bead {}", bead);
                        }
                        Ok(false) => {
                            debug!("No assignee to clear for bead {}", bead);
                        }
                        Err(e) => {
                            warn!("Failed to clear assignee for bead {}: {}", bead, e);
                        }
                    }
                }
            }
        }

        // Record crash in history
        let history = self.crash_history.entry(worker_id.to_string()).or_insert_with(Vec::new);
        history.push(record.clone());

        // Clean up old crashes outside the window
        self.cleanup_old_crashes(worker_id);

        // Mark worker as crashed
        self.crashed_workers.insert(worker_id.to_string(), record.clone());

        // Determine if we should auto-restart
        let action = if self.should_auto_restart(worker_id) {
            info!("Auto-restart enabled for {}", worker_id);
            CrashAction::Restart
        } else {
            let recent_crashes = self.recent_crash_count(worker_id);
            if recent_crashes >= self.config.max_crashes_in_window {
                warn!(
                    "Worker {} has crashed {} times in {} seconds, auto-restart disabled",
                    worker_id, recent_crashes, self.config.crash_window_secs
                );
            }
            CrashAction::NotifyOnly
        };

        Ok(action)
    }

    /// Check if health status indicates a crash (process died).
    fn is_crash(health: &WorkerHealthStatus) -> bool {
        // Look for PID check failure (dead process)
        health.failed_checks.contains(&HealthCheckType::PidExists)
    }

    /// Extract crash reason and error message from health status.
    fn extract_crash_details(health: &WorkerHealthStatus) -> (String, String) {
        // Find the first failed check related to process death
        for result in &health.check_results {
            if !result.passed && result.check_type == HealthCheckType::PidExists {
                let reason = if let Some(ref error_type) = result.error_type {
                    match error_type {
                        HealthErrorType::DeadProcess => "process died".to_string(),
                        _ => error_type.to_string(),
                    }
                } else {
                    "unknown".to_string()
                };

                let message = result.error_message.clone().unwrap_or_else(|| "No error message".to_string());
                return (reason, message);
            }
        }

        // Fallback
        ("unknown".to_string(), health.primary_error.clone().unwrap_or_else(|| "No details".to_string()))
    }

    /// Clear bead assignee using br CLI.
    ///
    /// Returns Ok(true) if assignee was cleared, Ok(false) if no assignee was set.
    async fn clear_bead_assignee(&self, workspace: &PathBuf, bead_id: &str) -> Result<bool> {
        debug!("Clearing assignee for bead {} in {:?}", bead_id, workspace);

        // Check if bead exists and has an assignee
        let has_assignee = self.check_bead_has_assignee(workspace, bead_id).await?;
        if !has_assignee {
            return Ok(false);
        }

        // Clear the assignee: br update <bead-id> --assignee ""
        let output = Command::new("br")
            .arg("update")
            .arg(bead_id)
            .arg("--assignee")
            .arg("")
            .current_dir(workspace)
            .output()
            .map_err(|e| forge_core::ForgeError::ToolExecution {
                tool_name: "br".to_string(),
                message: format!("Failed to execute br update {}: {}", bead_id, e),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("Failed to clear assignee for {}: {}", bead_id, stderr);
            return Err(forge_core::ForgeError::ToolExecution {
                tool_name: "br".to_string(),
                message: format!("br update failed for {}: {}", bead_id, stderr),
            });
        }

        // Also update status back to open if it was in_progress
        let _ = Command::new("br")
            .arg("update")
            .arg(bead_id)
            .arg("--status")
            .arg("open")
            .current_dir(workspace)
            .output();

        Ok(true)
    }

    /// Check if a bead has an assignee.
    async fn check_bead_has_assignee(&self, workspace: &PathBuf, bead_id: &str) -> Result<bool> {
        // Use br show to get bead details
        let output = Command::new("br")
            .arg("show")
            .arg(bead_id)
            .arg("--format=json")
            .current_dir(workspace)
            .output()
            .map_err(|e| forge_core::ForgeError::ToolExecution {
                tool_name: "br".to_string(),
                message: format!("Failed to execute br show {}: {}", bead_id, e),
            })?;

        if !output.status.success() {
            // Bead might not exist
            return Ok(false);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse JSON and check for assignee field
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
            if let Some(assignee) = json.get("assignee") {
                if let Some(assignee_str) = assignee.as_str() {
                    return Ok(!assignee_str.is_empty());
                }
            }
        }

        Ok(false)
    }

    /// Determine if worker should be auto-restarted after crash.
    fn should_auto_restart(&self, worker_id: &str) -> bool {
        if !self.config.auto_restart_enabled {
            return false;
        }

        // Check crash count within window
        let recent_crashes = self.recent_crash_count(worker_id);
        recent_crashes < self.config.max_crashes_in_window
    }

    /// Count recent crashes for a worker within the time window.
    fn recent_crash_count(&self, worker_id: &str) -> usize {
        self.crash_history
            .get(worker_id)
            .map(|history| history.iter().filter(|c| c.is_within_window()).count())
            .unwrap_or(0)
    }

    /// Clean up old crash records outside the time window.
    fn cleanup_old_crashes(&mut self, worker_id: &str) {
        if let Some(history) = self.crash_history.get_mut(worker_id) {
            history.retain(|c| c.is_within_window());
        }
    }

    /// Mark a worker as recovered (no longer crashed).
    pub fn mark_recovered(&mut self, worker_id: &str) {
        if self.crashed_workers.remove(worker_id).is_some() {
            info!("Worker {} marked as recovered", worker_id);
        }
    }

    /// Get crash record for a worker if it's currently crashed.
    pub fn get_crash(&self, worker_id: &str) -> Option<&CrashRecord> {
        self.crashed_workers.get(worker_id)
    }

    /// Get crash history for a worker.
    pub fn get_crash_history(&self, worker_id: &str) -> Option<&Vec<CrashRecord>> {
        self.crash_history.get(worker_id)
    }

    /// Get all currently crashed workers.
    pub fn get_crashed_workers(&self) -> Vec<(WorkerId, CrashRecord)> {
        self.crashed_workers
            .iter()
            .map(|(id, record)| (id.clone(), record.clone()))
            .collect()
    }

    /// Get recent crash count for a worker.
    pub fn get_recent_crash_count(&self, worker_id: &str) -> usize {
        self.recent_crash_count(worker_id)
    }

    /// Check if auto-restart is exhausted for a worker.
    pub fn is_auto_restart_exhausted(&self, worker_id: &str) -> bool {
        self.recent_crash_count(worker_id) >= self.config.max_crashes_in_window
    }

    /// Get configuration.
    pub fn config(&self) -> &CrashRecoveryConfig {
        &self.config
    }

    /// Update configuration.
    pub fn set_config(&mut self, config: CrashRecoveryConfig) {
        self.config = config;
    }

    /// Clear all crash history (for testing).
    #[cfg(test)]
    pub fn clear_history(&mut self) {
        self.crash_history.clear();
        self.crashed_workers.clear();
    }
}

impl Default for CrashRecoveryManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::{HealthCheckResult, WorkerHealthStatus};

    fn create_crashed_health() -> WorkerHealthStatus {
        let mut health = WorkerHealthStatus::new("test-worker");
        health.add_result(HealthCheckResult::failed(
            HealthCheckType::PidExists,
            HealthErrorType::DeadProcess,
            "Process 12345 does not exist",
        ));
        health
    }

    fn create_healthy_health() -> WorkerHealthStatus {
        let mut health = WorkerHealthStatus::new("test-worker");
        health.add_result(HealthCheckResult::passed(HealthCheckType::PidExists));
        health
    }

    #[test]
    fn test_is_crash_detection() {
        let crashed = create_crashed_health();
        assert!(CrashRecoveryManager::is_crash(&crashed));

        let healthy = create_healthy_health();
        assert!(!CrashRecoveryManager::is_crash(&healthy));
    }

    #[test]
    fn test_extract_crash_details() {
        let crashed = create_crashed_health();
        let (reason, message) = CrashRecoveryManager::extract_crash_details(&crashed);

        assert_eq!(reason, "process died");
        assert!(message.contains("12345"));
    }

    #[test]
    fn test_crash_record_age() {
        let record = CrashRecord::new(
            "test-worker",
            "process died",
            "PID 12345 does not exist",
            None,
            None,
        );

        assert!(record.age_secs() < 5); // Should be recent
        assert!(record.is_within_window());
    }

    #[test]
    fn test_crash_record_outside_window() {
        let mut record = CrashRecord::new(
            "test-worker",
            "process died",
            "PID 12345 does not exist",
            None,
            None,
        );

        // Simulate crash from 11 minutes ago
        record.crashed_at = Utc::now() - chrono::Duration::seconds(660);

        assert!(!record.is_within_window());
        assert!(record.age_secs() > CRASH_WINDOW_SECS);
    }

    #[tokio::test]
    async fn test_crash_recovery_manager_creation() {
        let manager = CrashRecoveryManager::new();
        assert_eq!(manager.config.auto_restart_enabled, false); // Disabled by default
        assert_eq!(manager.config.max_crashes_in_window, 3);
    }

    #[tokio::test]
    async fn test_handle_crash_first_time() {
        let mut manager = CrashRecoveryManager::new();
        let health = create_crashed_health();

        let action = manager
            .handle_crash("worker-1", &health, None, None)
            .await
            .unwrap();

        // Auto-restart disabled by default, so should notify only
        assert_eq!(action, CrashAction::NotifyOnly);
        assert!(manager.get_crash("worker-1").is_some());
    }

    #[tokio::test]
    async fn test_handle_crash_with_auto_restart() {
        let config = CrashRecoveryConfig {
            auto_restart_enabled: true,
            ..Default::default()
        };
        let mut manager = CrashRecoveryManager::with_config(config);
        let health = create_crashed_health();

        let action = manager
            .handle_crash("worker-1", &health, None, None)
            .await
            .unwrap();

        // First crash should trigger restart
        assert_eq!(action, CrashAction::Restart);
    }

    #[tokio::test]
    async fn test_crash_rate_limiting() {
        let config = CrashRecoveryConfig {
            auto_restart_enabled: true,
            max_crashes_in_window: 2, // Allow only 2 crashes
            ..Default::default()
        };
        let mut manager = CrashRecoveryManager::with_config(config);
        let health = create_crashed_health();

        // First crash - should restart
        manager.handle_crash("worker-1", &health, None, None).await.unwrap();
        manager.mark_recovered("worker-1");

        // Second crash - should restart
        manager.handle_crash("worker-1", &health, None, None).await.unwrap();
        manager.mark_recovered("worker-1");

        // Third crash - should NOT restart (limit exceeded)
        let action = manager
            .handle_crash("worker-1", &health, None, None)
            .await
            .unwrap();

        assert_eq!(action, CrashAction::NotifyOnly);
        assert!(manager.is_auto_restart_exhausted("worker-1"));
    }

    #[tokio::test]
    async fn test_ignore_already_handled_crash() {
        let mut manager = CrashRecoveryManager::new();
        let health = create_crashed_health();

        // Handle crash once
        let action1 = manager
            .handle_crash("worker-1", &health, None, None)
            .await
            .unwrap();
        assert_ne!(action1, CrashAction::Ignore);

        // Try to handle again - should be ignored
        let action2 = manager
            .handle_crash("worker-1", &health, None, None)
            .await
            .unwrap();
        assert_eq!(action2, CrashAction::Ignore);
    }

    #[tokio::test]
    async fn test_mark_recovered() {
        let mut manager = CrashRecoveryManager::new();
        let health = create_crashed_health();

        manager.handle_crash("worker-1", &health, None, None).await.unwrap();
        assert!(manager.get_crash("worker-1").is_some());

        manager.mark_recovered("worker-1");
        assert!(manager.get_crash("worker-1").is_none());
    }

    #[tokio::test]
    async fn test_cleanup_old_crashes() {
        let mut manager = CrashRecoveryManager::new();

        // Add old crash manually
        let mut old_record = CrashRecord::new(
            "worker-1",
            "process died",
            "Old crash",
            None,
            None,
        );
        old_record.crashed_at = Utc::now() - chrono::Duration::seconds(700); // 11+ minutes ago

        manager.crash_history.entry("worker-1".to_string()).or_insert_with(Vec::new).push(old_record);

        // Add recent crash
        let health = create_crashed_health();
        manager.handle_crash("worker-1", &health, None, None).await.unwrap();

        // Cleanup should remove old crash
        manager.cleanup_old_crashes("worker-1");

        let history = manager.get_crash_history("worker-1").unwrap();
        assert_eq!(history.len(), 1); // Only recent crash remains
    }

    #[test]
    fn test_recent_crash_count() {
        let mut manager = CrashRecoveryManager::new();

        // Add 2 recent crashes
        let mut history = Vec::new();
        for i in 0..2 {
            let record = CrashRecord::new(
                "worker-1",
                "process died",
                format!("Crash {}", i),
                None,
                None,
            );
            history.push(record);
        }

        // Add 1 old crash
        let mut old_record = CrashRecord::new(
            "worker-1",
            "process died",
            "Old crash",
            None,
            None,
        );
        old_record.crashed_at = Utc::now() - chrono::Duration::seconds(700);
        history.push(old_record);

        manager.crash_history.insert("worker-1".to_string(), history);

        // Should only count recent crashes
        assert_eq!(manager.recent_crash_count("worker-1"), 2);
    }

    #[tokio::test]
    async fn test_ignore_non_crash_health_issues() {
        let mut manager = CrashRecoveryManager::new();

        // Create health status with non-crash failure (e.g., stale activity)
        let mut health = WorkerHealthStatus::new("test-worker");
        health.add_result(HealthCheckResult::failed(
            HealthCheckType::ActivityFresh,
            HealthErrorType::StaleActivity,
            "No activity for 20 minutes",
        ));

        let action = manager
            .handle_crash("worker-1", &health, None, None)
            .await
            .unwrap();

        // Should ignore - not a crash
        assert_eq!(action, CrashAction::Ignore);
        assert!(manager.get_crash("worker-1").is_none());
    }
}
