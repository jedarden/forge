//! Automated recovery system for worker health management.
//!
//! This module provides a unified `AutoRecoveryManager` that coordinates all automated
//! recovery actions based on configurable policies:
//!
//! - **Restart dead workers**: Detect and restart workers that have crashed
//! - **Kill memory-leaking workers**: Terminate workers exceeding memory limits
//! - **Timeout stuck tasks**: Reset tasks that have been in_progress too long
//! - **Clear stale assignees**: Remove worker assignments from abandoned beads
//!
//! ## Recovery Policies
//!
//! Each recovery action has a configurable policy:
//!
//! ```no_run
//! use forge_worker::auto_recovery::{AutoRecoveryManager, RecoveryConfig, RecoveryPolicy};
//! use std::time::Duration;
//!
//! let config = RecoveryConfig {
//!     dead_worker_policy: RecoveryPolicy::AutoRecover {
//!         max_attempts: 3,
//!         cooldown: Duration::from_secs(60),
//!     },
//!     memory_leak_policy: RecoveryPolicy::NotifyOnly,
//!     stuck_task_policy: RecoveryPolicy::AutoRecover {
//!         max_attempts: 1,
//!         cooldown: Duration::from_secs(300),
//!     },
//!     stale_assignee_policy: RecoveryPolicy::AutoRecover {
//!         max_attempts: 1,
//!         cooldown: Duration::from_secs(0),
//!     },
//!     ..Default::default()
//! };
//!
//! let manager = AutoRecoveryManager::new(config);
//! ```
//!
//! ## Usage
//!
//! ```no_run
//! use forge_worker::auto_recovery::AutoRecoveryManager;
//!
//! #[tokio::main]
//! async fn main() -> forge_core::Result<()> {
//!     let mut manager = AutoRecoveryManager::default();
//!
//!     // Run recovery check (typically called periodically)
//!     let actions = manager.check_and_recover().await?;
//!
//!     for action in actions {
//!         println!("Recovery action: {:?}", action);
//!     }
//!
//!     Ok(())
//! }
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use forge_core::status::StatusReader;
use forge_core::stuck_detection::{StuckDetectionConfig, StuckTask, StuckTaskDetector};
use forge_core::Result;

use crate::crash_recovery::{CrashRecoveryConfig, CrashRecoveryManager};
use crate::health::{HealthMonitor, HealthMonitorConfig, WorkerHealthStatus};
use crate::memory::{MemoryConfig, MemoryMonitor};

/// Default memory kill threshold in MB (8GB).
pub const DEFAULT_MEMORY_KILL_THRESHOLD_MB: u64 = 8192;

/// Recovery policy for different types of issues.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecoveryPolicy {
    /// Take no action (visibility only via TUI).
    Disabled,

    /// Show notification in TUI but don't take automatic action.
    NotifyOnly,

    /// Automatically attempt recovery with limits.
    AutoRecover {
        /// Maximum recovery attempts per worker/task before giving up.
        max_attempts: u8,
        /// Minimum time between recovery attempts for the same entity.
        #[serde(with = "humantime_serde")]
        cooldown: Duration,
    },
}

impl Default for RecoveryPolicy {
    fn default() -> Self {
        // Per ADR 0014: visibility first, auto-recovery is opt-in
        Self::NotifyOnly
    }
}

/// Configuration for the auto-recovery system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Enable/disable the entire auto-recovery system.
    pub enabled: bool,

    /// How often to run recovery checks (in seconds).
    pub check_interval_secs: u64,

    /// Policy for restarting dead workers.
    pub dead_worker_policy: RecoveryPolicy,

    /// Policy for killing workers with high memory usage (warning level).
    pub memory_leak_policy: RecoveryPolicy,

    /// Memory warning threshold in MB (triggers alerts, default 4GB).
    pub memory_threshold_mb: u64,

    /// Memory kill threshold in MB (auto-terminates runaway workers, default 8GB).
    /// Workers exceeding this will be forcefully terminated regardless of policy.
    pub memory_kill_threshold_mb: u64,

    /// Policy for timing out stuck tasks.
    pub stuck_task_policy: RecoveryPolicy,

    /// Time in minutes before a task is considered stuck.
    pub stuck_task_timeout_mins: i64,

    /// Policy for clearing stale assignees.
    pub stale_assignee_policy: RecoveryPolicy,

    /// Time in minutes before an assignee is considered stale.
    pub stale_assignee_timeout_mins: i64,

    /// Workspaces to monitor for stuck tasks and stale assignees.
    pub monitored_workspaces: Vec<PathBuf>,

    /// Maximum workers to restart concurrently.
    pub max_concurrent_restarts: usize,

    /// Emit alerts for recovery actions (visible in TUI).
    pub emit_alerts: bool,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval_secs: 30,
            dead_worker_policy: RecoveryPolicy::NotifyOnly,
            memory_leak_policy: RecoveryPolicy::NotifyOnly,
            memory_threshold_mb: 4096, // 4GB warning threshold (per task requirements)
            memory_kill_threshold_mb: DEFAULT_MEMORY_KILL_THRESHOLD_MB, // 8GB kill threshold
            stuck_task_policy: RecoveryPolicy::NotifyOnly,
            stuck_task_timeout_mins: 30,
            stale_assignee_policy: RecoveryPolicy::AutoRecover {
                max_attempts: 1,
                cooldown: Duration::from_secs(0),
            },
            stale_assignee_timeout_mins: 60,
            monitored_workspaces: Vec::new(),
            max_concurrent_restarts: 2,
            emit_alerts: true,
        }
    }
}

impl RecoveryConfig {
    /// Create a permissive configuration that auto-recovers everything.
    pub fn auto_recover_all() -> Self {
        Self {
            enabled: true,
            check_interval_secs: 30,
            dead_worker_policy: RecoveryPolicy::AutoRecover {
                max_attempts: 3,
                cooldown: Duration::from_secs(60),
            },
            memory_leak_policy: RecoveryPolicy::AutoRecover {
                max_attempts: 2,
                cooldown: Duration::from_secs(300),
            },
            memory_threshold_mb: 4096, // 4GB warning
            memory_kill_threshold_mb: DEFAULT_MEMORY_KILL_THRESHOLD_MB, // 8GB hard kill
            stuck_task_policy: RecoveryPolicy::AutoRecover {
                max_attempts: 1,
                cooldown: Duration::from_secs(600),
            },
            stuck_task_timeout_mins: 30,
            stale_assignee_policy: RecoveryPolicy::AutoRecover {
                max_attempts: 1,
                cooldown: Duration::from_secs(0),
            },
            stale_assignee_timeout_mins: 60,
            monitored_workspaces: Vec::new(),
            max_concurrent_restarts: 2,
            emit_alerts: true,
        }
    }

    /// Create a conservative configuration (notify only).
    pub fn notify_only() -> Self {
        Self {
            dead_worker_policy: RecoveryPolicy::NotifyOnly,
            memory_leak_policy: RecoveryPolicy::NotifyOnly,
            stuck_task_policy: RecoveryPolicy::NotifyOnly,
            stale_assignee_policy: RecoveryPolicy::NotifyOnly,
            ..Default::default()
        }
    }
}

/// Types of recovery actions that can be taken.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryActionType {
    /// Worker restart (dead worker recovery).
    RestartWorker,
    /// Worker termination (memory leak).
    TerminateWorker,
    /// Task timeout (stuck task recovery).
    TimeoutTask,
    /// Clear stale assignee.
    ClearAssignee,
}

impl std::fmt::Display for RecoveryActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RestartWorker => write!(f, "restart worker"),
            Self::TerminateWorker => write!(f, "terminate worker"),
            Self::TimeoutTask => write!(f, "timeout task"),
            Self::ClearAssignee => write!(f, "clear assignee"),
        }
    }
}

/// A recovery action that was taken or recommended.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryAction {
    /// Type of action.
    pub action_type: RecoveryActionType,
    /// Target entity (worker_id or bead_id).
    pub target: String,
    /// Description of the issue that triggered recovery.
    pub reason: String,
    /// Whether the action was executed (true) or just recommended (false).
    pub executed: bool,
    /// Result of execution (if executed).
    pub result: Option<String>,
    /// Timestamp of the action.
    pub timestamp: DateTime<Utc>,
    /// Workspace context (if applicable).
    pub workspace: Option<PathBuf>,
}

impl RecoveryAction {
    /// Create a new recovery action.
    pub fn new(
        action_type: RecoveryActionType,
        target: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            action_type,
            target: target.into(),
            reason: reason.into(),
            executed: false,
            result: None,
            timestamp: Utc::now(),
            workspace: None,
        }
    }

    /// Mark the action as executed with a result.
    pub fn with_result(mut self, result: impl Into<String>) -> Self {
        self.executed = true;
        self.result = Some(result.into());
        self
    }

    /// Add workspace context.
    pub fn with_workspace(mut self, workspace: PathBuf) -> Self {
        self.workspace = Some(workspace);
        self
    }

    /// Format for display in TUI.
    pub fn format_for_display(&self) -> String {
        let status = if self.executed {
            if self.result.as_ref().map(|r| r.contains("error")).unwrap_or(false) {
                "FAILED"
            } else {
                "DONE"
            }
        } else {
            "PENDING"
        };

        format!(
            "[{}] {} {} - {}",
            status, self.action_type, self.target, self.reason
        )
    }
}

/// Tracking data for recovery attempts per entity.
#[derive(Debug, Clone, Default)]
struct RecoveryAttemptTracker {
    /// Number of recovery attempts made.
    attempts: u8,
    /// Timestamp of last recovery attempt.
    last_attempt: Option<Instant>,
}

impl RecoveryAttemptTracker {
    /// Check if a new recovery attempt is allowed based on policy.
    fn can_attempt(&self, policy: &RecoveryPolicy) -> bool {
        match policy {
            RecoveryPolicy::Disabled | RecoveryPolicy::NotifyOnly => false,
            RecoveryPolicy::AutoRecover { max_attempts, cooldown } => {
                // Check attempt limit
                if self.attempts >= *max_attempts {
                    return false;
                }
                // Check cooldown
                if let Some(last) = self.last_attempt {
                    if last.elapsed() < *cooldown {
                        return false;
                    }
                }
                true
            }
        }
    }

    /// Record a recovery attempt.
    fn record_attempt(&mut self) {
        self.attempts = self.attempts.saturating_add(1);
        self.last_attempt = Some(Instant::now());
    }

    /// Reset tracking (after successful recovery).
    fn reset(&mut self) {
        self.attempts = 0;
        self.last_attempt = None;
    }
}

/// The main auto-recovery manager.
pub struct AutoRecoveryManager {
    /// Configuration.
    config: RecoveryConfig,
    /// Health monitor for worker health checks.
    health_monitor: HealthMonitor,
    /// Memory monitor for RSS tracking and growth rate.
    memory_monitor: MemoryMonitor,
    /// Crash recovery manager (reserved for crash history tracking).
    #[allow(dead_code)]
    crash_recovery: CrashRecoveryManager,
    /// Stuck task detector.
    stuck_detector: StuckTaskDetector,
    /// Status reader for worker status.
    status_reader: StatusReader,
    /// Recovery attempt tracking per worker.
    worker_attempts: HashMap<String, RecoveryAttemptTracker>,
    /// Recovery attempt tracking per bead.
    bead_attempts: HashMap<String, RecoveryAttemptTracker>,
    /// Last check timestamp.
    last_check: Option<Instant>,
    /// Recent actions taken (for display).
    recent_actions: Vec<RecoveryAction>,
    /// Maximum recent actions to keep.
    max_recent_actions: usize,
}

impl AutoRecoveryManager {
    /// Create a new auto-recovery manager with custom configuration.
    pub fn new(config: RecoveryConfig) -> Result<Self> {
        // Create health monitor config from recovery config
        let health_config = HealthMonitorConfig {
            enable_auto_recovery: matches!(
                config.dead_worker_policy,
                RecoveryPolicy::AutoRecover { .. }
            ),
            memory_limit_mb: config.memory_threshold_mb,
            memory_kill_limit_mb: config.memory_kill_threshold_mb,
            enable_memory_check: !matches!(config.memory_leak_policy, RecoveryPolicy::Disabled),
            task_stuck_threshold_mins: config.stuck_task_timeout_mins,
            ..Default::default()
        };

        let health_monitor = HealthMonitor::new(health_config)?;

        // Create memory monitor config
        let memory_config = MemoryConfig {
            warning_limit_mb: config.memory_threshold_mb,
            kill_limit_mb: config.memory_kill_threshold_mb,
            track_growth_rate: true,
            ..Default::default()
        };

        let memory_monitor = MemoryMonitor::new(memory_config);

        // Create crash recovery config
        let crash_config = CrashRecoveryConfig {
            auto_restart_enabled: matches!(
                config.dead_worker_policy,
                RecoveryPolicy::AutoRecover { .. }
            ),
            clear_assignees_enabled: !matches!(
                config.stale_assignee_policy,
                RecoveryPolicy::Disabled
            ),
            ..Default::default()
        };

        let crash_recovery = CrashRecoveryManager::with_config(crash_config);

        // Create stuck task detector
        let stuck_config = StuckDetectionConfig {
            stuck_timeout: Duration::from_secs(config.stuck_task_timeout_mins as u64 * 60),
            activity_check_window: Duration::from_secs(5 * 60),
            min_activity_threshold: 1,
        };

        let mut stuck_detector = StuckTaskDetector::new(stuck_config);
        for workspace in &config.monitored_workspaces {
            stuck_detector.add_workspace(workspace.clone());
        }

        let status_reader = StatusReader::new(None)?;

        Ok(Self {
            config,
            health_monitor,
            memory_monitor,
            crash_recovery,
            stuck_detector,
            status_reader,
            worker_attempts: HashMap::new(),
            bead_attempts: HashMap::new(),
            last_check: None,
            recent_actions: Vec::new(),
            max_recent_actions: 50,
        })
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Result<Self> {
        Self::new(RecoveryConfig::default())
    }

    /// Add a workspace to monitor.
    pub fn add_workspace(&mut self, workspace: impl Into<PathBuf>) {
        let path = workspace.into();
        if !self.config.monitored_workspaces.contains(&path) {
            self.config.monitored_workspaces.push(path.clone());
            self.stuck_detector.add_workspace(path);
        }
    }

    /// Get current configuration.
    pub fn config(&self) -> &RecoveryConfig {
        &self.config
    }

    /// Update configuration.
    pub fn set_config(&mut self, config: RecoveryConfig) {
        self.config = config;
    }

    /// Run a full recovery check and execute configured actions.
    ///
    /// Returns a list of actions taken or recommended.
    pub async fn check_and_recover(&mut self) -> Result<Vec<RecoveryAction>> {
        if !self.config.enabled {
            return Ok(Vec::new());
        }

        // Check if enough time has passed since last check
        if let Some(last) = self.last_check {
            if last.elapsed() < Duration::from_secs(self.config.check_interval_secs) {
                return Ok(Vec::new());
            }
        }

        self.last_check = Some(Instant::now());
        let mut actions = Vec::new();

        // 1. Check worker health (dead workers, memory leaks)
        let worker_actions = self.check_worker_health().await?;
        actions.extend(worker_actions);

        // 2. Check for stuck tasks
        let stuck_actions = self.check_stuck_tasks().await?;
        actions.extend(stuck_actions);

        // 3. Check for stale assignees
        let stale_actions = self.check_stale_assignees().await?;
        actions.extend(stale_actions);

        // Store recent actions
        for action in &actions {
            self.recent_actions.push(action.clone());
            if self.recent_actions.len() > self.max_recent_actions {
                self.recent_actions.remove(0);
            }
        }

        Ok(actions)
    }

    /// Check worker health and handle dead/unhealthy workers.
    async fn check_worker_health(&mut self) -> Result<Vec<RecoveryAction>> {
        use crate::memory::MemorySeverity;

        let mut actions = Vec::new();

        // Get all worker health statuses
        let health_results = self.health_monitor.check_all_health()?;

        // First pass: check memory and handle runaway workers (>8GB)
        // This happens regardless of policy - runaway workers are always killed
        let mut runaway_workers = Vec::new();
        for (worker_id, _health) in &health_results {
            if let Some(pid) = self.get_worker_pid(worker_id) {
                // Check memory and track growth rate
                if let Ok(Some(mem_stats)) =
                    self.memory_monitor.check_worker_memory(pid, worker_id)
                {
                    // Log memory status with growth rate
                    if mem_stats.growth_rate_mb_per_min.abs() > 1.0 {
                        info!(
                            worker_id,
                            rss_mb = mem_stats.rss_mb,
                            growth_rate = mem_stats.growth_rate_mb_per_min,
                            "Worker memory: {} (growth: {})",
                            mem_stats.format_rss(),
                            mem_stats.format_growth_rate()
                        );
                    }

                    // Handle runaway workers (>8GB) - always kill regardless of policy
                    if mem_stats.severity() == MemorySeverity::Critical {
                        runaway_workers.push((worker_id.clone(), pid, mem_stats));
                    }
                }
            }
        }

        // Kill runaway workers
        for (worker_id, pid, mem_stats) in runaway_workers {
            let action = self.handle_runaway_worker(&worker_id, pid, &mem_stats).await;
            if let Some(a) = action {
                actions.push(a);
            }
        }

        // Second pass: handle other health issues based on policy
        for (worker_id, health) in health_results {
            // Skip workers that were already handled as runaway
            if actions
                .iter()
                .any(|a| a.target == worker_id && a.action_type == RecoveryActionType::TerminateWorker)
            {
                continue;
            }

            if health.is_healthy {
                // Worker is healthy, reset tracking
                if let Some(tracker) = self.worker_attempts.get_mut(&worker_id) {
                    tracker.reset();
                }
                continue;
            }

            // Determine what kind of issue this is
            let (action_type, reason) = self.classify_health_issue(&health);

            match action_type {
                RecoveryActionType::RestartWorker => {
                    let action = self
                        .handle_dead_worker(&worker_id, &health, &reason)
                        .await;
                    if let Some(a) = action {
                        actions.push(a);
                    }
                }
                RecoveryActionType::TerminateWorker => {
                    let action = self
                        .handle_memory_leak(&worker_id, &health, &reason)
                        .await;
                    if let Some(a) = action {
                        actions.push(a);
                    }
                }
                _ => {}
            }
        }

        Ok(actions)
    }

    /// Get worker PID from status files.
    fn get_worker_pid(&self, worker_id: &str) -> Option<u32> {
        self.status_reader
            .read_worker(worker_id)
            .ok()
            .flatten()
            .and_then(|info| info.pid)
    }

    /// Handle a runaway worker (>8GB memory).
    ///
    /// This always kills the worker, regardless of policy, as runaway workers
    /// can destabilize the system.
    async fn handle_runaway_worker(
        &mut self,
        worker_id: &str,
        pid: u32,
        mem_stats: &crate::memory::WorkerMemoryStats,
    ) -> Option<RecoveryAction> {
        let reason = format!(
            "RUNAWAY: Memory {} exceeds kill limit ({}MB > {}MB), growth rate: {}",
            mem_stats.format_rss(),
            mem_stats.rss_mb,
            self.config.memory_kill_threshold_mb,
            mem_stats.format_growth_rate()
        );

        error!(
            worker_id,
            pid,
            rss_mb = mem_stats.rss_mb,
            growth_rate = mem_stats.growth_rate_mb_per_min,
            "Killing runaway worker"
        );

        let mut action =
            RecoveryAction::new(RecoveryActionType::TerminateWorker, worker_id, &reason);

        // First try to clear any bead assignment
        if let Ok(Some(info)) = self.status_reader.read_worker(worker_id) {
            if let (Some(workspace), Some(bead_id)) = (&info.workspace, &info.current_task) {
                let _ = self.clear_assignee(workspace, bead_id).await;
            }
        }

        // Kill the worker (always, regardless of policy)
        match self.memory_monitor.kill_runaway_worker(pid, worker_id) {
            Ok(true) => {
                action = action.with_result(format!(
                    "Runaway worker terminated (was using {} MB)",
                    mem_stats.rss_mb
                ));
                self.memory_monitor.clear_worker(worker_id);
                info!(worker_id, pid, "Runaway worker killed successfully");
            }
            Ok(false) => {
                action =
                    action.with_result("Failed to kill runaway worker (process may have exited)");
                warn!(worker_id, pid, "Could not kill runaway worker");
            }
            Err(e) => {
                action = action.with_result(format!("Error killing runaway worker: {}", e));
                error!(worker_id, pid, error = %e, "Error killing runaway worker");
            }
        }

        Some(action)
    }

    /// Classify a health issue into an action type.
    fn classify_health_issue(
        &self,
        health: &WorkerHealthStatus,
    ) -> (RecoveryActionType, String) {
        use crate::health::HealthCheckType;

        // Check for dead process first (highest priority)
        if health.failed_checks.contains(&HealthCheckType::PidExists) {
            return (
                RecoveryActionType::RestartWorker,
                health.primary_error.clone().unwrap_or_else(|| "Process died".to_string()),
            );
        }

        // Check for memory issues
        if health.failed_checks.contains(&HealthCheckType::MemoryUsage) {
            return (
                RecoveryActionType::TerminateWorker,
                health.primary_error.clone().unwrap_or_else(|| "Memory limit exceeded".to_string()),
            );
        }

        // Default to restart for other issues
        (
            RecoveryActionType::RestartWorker,
            health.primary_error.clone().unwrap_or_else(|| "Worker unhealthy".to_string()),
        )
    }

    /// Handle a dead worker based on policy.
    async fn handle_dead_worker(
        &mut self,
        worker_id: &str,
        _health: &WorkerHealthStatus,
        reason: &str,
    ) -> Option<RecoveryAction> {
        let tracker = self
            .worker_attempts
            .entry(worker_id.to_string())
            .or_default();

        let mut action = RecoveryAction::new(
            RecoveryActionType::RestartWorker,
            worker_id,
            reason,
        );

        if tracker.can_attempt(&self.config.dead_worker_policy) {
            // Execute restart
            tracker.record_attempt();

            match self.restart_worker(worker_id).await {
                Ok(()) => {
                    action = action.with_result("Worker restarted successfully");
                    info!(worker_id, "Auto-restarted dead worker");
                }
                Err(e) => {
                    action = action.with_result(format!("Failed to restart: {}", e));
                    error!(worker_id, error = %e, "Failed to auto-restart worker");
                }
            }
        } else {
            debug!(worker_id, "Dead worker recovery skipped (policy or limit)");
        }

        Some(action)
    }

    /// Handle a memory-leaking worker based on policy.
    async fn handle_memory_leak(
        &mut self,
        worker_id: &str,
        _health: &WorkerHealthStatus,
        reason: &str,
    ) -> Option<RecoveryAction> {
        let tracker = self
            .worker_attempts
            .entry(worker_id.to_string())
            .or_default();

        let mut action = RecoveryAction::new(
            RecoveryActionType::TerminateWorker,
            worker_id,
            reason,
        );

        if tracker.can_attempt(&self.config.memory_leak_policy) {
            tracker.record_attempt();

            match self.terminate_worker(worker_id).await {
                Ok(()) => {
                    action = action.with_result("Worker terminated due to memory leak");
                    info!(worker_id, "Terminated worker for memory leak");
                }
                Err(e) => {
                    action = action.with_result(format!("Failed to terminate: {}", e));
                    error!(worker_id, error = %e, "Failed to terminate memory-leaking worker");
                }
            }
        }

        Some(action)
    }

    /// Check for stuck tasks and handle them.
    async fn check_stuck_tasks(&mut self) -> Result<Vec<RecoveryAction>> {
        let mut actions = Vec::new();

        // Detect stuck tasks
        let stuck_tasks = self.stuck_detector.detect_stuck_tasks()?;

        for stuck in stuck_tasks {
            let action = self.handle_stuck_task(&stuck).await;
            if let Some(a) = action {
                actions.push(a);
            }
        }

        Ok(actions)
    }

    /// Handle a stuck task based on policy.
    async fn handle_stuck_task(&mut self, stuck: &StuckTask) -> Option<RecoveryAction> {
        let tracker = self
            .bead_attempts
            .entry(stuck.bead_id.clone())
            .or_default();

        let mut action = RecoveryAction::new(
            RecoveryActionType::TimeoutTask,
            &stuck.bead_id,
            &stuck.reason,
        )
        .with_workspace(stuck.workspace.clone());

        if tracker.can_attempt(&self.config.stuck_task_policy) {
            tracker.record_attempt();

            match self.timeout_task(&stuck.workspace, &stuck.bead_id).await {
                Ok(()) => {
                    action = action.with_result("Task timed out, status set to open");
                    info!(bead_id = %stuck.bead_id, "Timed out stuck task");
                }
                Err(e) => {
                    action = action.with_result(format!("Failed to timeout task: {}", e));
                    error!(bead_id = %stuck.bead_id, error = %e, "Failed to timeout stuck task");
                }
            }
        }

        Some(action)
    }

    /// Check for stale assignees and clear them.
    async fn check_stale_assignees(&mut self) -> Result<Vec<RecoveryAction>> {
        let mut actions = Vec::new();

        // Check each monitored workspace
        for workspace in self.config.monitored_workspaces.clone() {
            let stale_beads = self.find_stale_assignees(&workspace)?;

            for (bead_id, assignee, in_progress_mins) in stale_beads {
                let action = self
                    .handle_stale_assignee(&workspace, &bead_id, &assignee, in_progress_mins)
                    .await;
                if let Some(a) = action {
                    actions.push(a);
                }
            }
        }

        Ok(actions)
    }

    /// Find beads with stale assignees in a workspace.
    fn find_stale_assignees(
        &self,
        workspace: &PathBuf,
    ) -> Result<Vec<(String, String, i64)>> {
        // Use br CLI to list in_progress beads
        let output = Command::new("br")
            .arg("list")
            .arg("--status")
            .arg("in_progress")
            .arg("--format")
            .arg("json")
            .current_dir(workspace)
            .output();

        let output = match output {
            Ok(o) => o,
            Err(e) => {
                debug!(workspace = ?workspace, error = %e, "Failed to run br list");
                return Ok(Vec::new());
            }
        };

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() || stdout.trim() == "[]" {
            return Ok(Vec::new());
        }

        // Parse JSON and check for stale assignees
        let beads: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap_or_default();
        let mut stale = Vec::new();

        let threshold = self.config.stale_assignee_timeout_mins;

        for bead in beads {
            let bead_id = bead.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let assignee = bead.get("assignee").and_then(|v| v.as_str()).unwrap_or("");
            let updated_at = bead.get("updated_at").and_then(|v| v.as_str()).unwrap_or("");

            if bead_id.is_empty() || assignee.is_empty() {
                continue;
            }

            // Parse timestamp and check if stale
            if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(updated_at) {
                let now = Utc::now();
                let elapsed_mins = now
                    .signed_duration_since(timestamp)
                    .num_minutes();

                if elapsed_mins > threshold {
                    // Check if assignee (worker) is still alive
                    if !self.is_worker_alive(assignee) {
                        stale.push((bead_id.to_string(), assignee.to_string(), elapsed_mins));
                    }
                }
            }
        }

        Ok(stale)
    }

    /// Check if a worker is still alive (has a tmux session).
    fn is_worker_alive(&self, worker_id: &str) -> bool {
        let output = Command::new("tmux")
            .arg("has-session")
            .arg("-t")
            .arg(worker_id)
            .output();

        output.map(|o| o.status.success()).unwrap_or(false)
    }

    /// Handle a stale assignee based on policy.
    async fn handle_stale_assignee(
        &mut self,
        workspace: &PathBuf,
        bead_id: &str,
        assignee: &str,
        in_progress_mins: i64,
    ) -> Option<RecoveryAction> {
        let tracker = self
            .bead_attempts
            .entry(bead_id.to_string())
            .or_default();

        let reason = format!(
            "Assignee '{}' is stale ({} mins, worker not responding)",
            assignee, in_progress_mins
        );

        let mut action = RecoveryAction::new(
            RecoveryActionType::ClearAssignee,
            bead_id,
            &reason,
        )
        .with_workspace(workspace.clone());

        if tracker.can_attempt(&self.config.stale_assignee_policy) {
            tracker.record_attempt();

            match self.clear_assignee(workspace, bead_id).await {
                Ok(()) => {
                    action = action.with_result("Assignee cleared, task available for reassignment");
                    info!(bead_id, assignee, "Cleared stale assignee");
                }
                Err(e) => {
                    action = action.with_result(format!("Failed to clear assignee: {}", e));
                    error!(bead_id, error = %e, "Failed to clear stale assignee");
                }
            }
        }

        Some(action)
    }

    /// Restart a worker using tmux.
    async fn restart_worker(&self, worker_id: &str) -> Result<()> {
        // First, kill the existing session if it exists
        let kill_output = Command::new("tmux")
            .arg("kill-session")
            .arg("-t")
            .arg(worker_id)
            .output();

        if let Ok(o) = kill_output {
            if o.status.success() {
                debug!(worker_id, "Killed existing worker session");
            }
        }

        // Read worker info from status file to get config
        let worker_info = self.status_reader.read_worker(worker_id)?;

        if let Some(info) = worker_info {
            if let (Some(workspace), Some(model)) = (info.workspace, info.model) {
                // Look for launcher script
                let launcher_paths = [
                    workspace.join(".forge/launcher.sh"),
                    PathBuf::from(std::env::var("HOME").unwrap_or_default())
                        .join(".forge/launcher.sh"),
                ];

                for launcher_path in launcher_paths {
                    if launcher_path.exists() {
                        let output = Command::new(&launcher_path)
                            .arg(format!("--model={}", model))
                            .arg(format!("--workspace={}", workspace.display()))
                            .arg(format!("--session-name={}", worker_id))
                            .current_dir(&workspace)
                            .output()
                            .map_err(|e| forge_core::ForgeError::LauncherExecution {
                                model: model.clone(),
                                message: e.to_string(),
                            })?;

                        if output.status.success() {
                            info!(worker_id, "Worker restarted via launcher");
                            return Ok(());
                        } else {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            return Err(forge_core::ForgeError::LauncherExecution {
                                model,
                                message: stderr.to_string(),
                            });
                        }
                    }
                }

                return Err(forge_core::ForgeError::LauncherNotFound {
                    path: PathBuf::from("launcher.sh"),
                });
            }
        }

        Err(forge_core::ForgeError::WorkerNotFound {
            worker_id: worker_id.to_string(),
        })
    }

    /// Terminate a worker (for memory leaks).
    async fn terminate_worker(&self, worker_id: &str) -> Result<()> {
        // Read worker info to get bead assignment
        let worker_info = self.status_reader.read_worker(worker_id)?;

        // Clear assignee if worker had a bead
        if let Some(ref info) = worker_info {
            if let (Some(workspace), Some(bead_id)) = (&info.workspace, &info.current_task) {
                let _ = self.clear_assignee(workspace, bead_id).await;
            }
        }

        // Kill the tmux session
        let output = Command::new("tmux")
            .arg("kill-session")
            .arg("-t")
            .arg(worker_id)
            .output()
            .map_err(|e| forge_core::ForgeError::ToolExecution {
                tool_name: "tmux".to_string(),
                message: e.to_string(),
            })?;

        if output.status.success() {
            info!(worker_id, "Worker terminated");
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(forge_core::ForgeError::ToolExecution {
                tool_name: "tmux".to_string(),
                message: stderr.to_string(),
            })
        }
    }

    /// Timeout a stuck task.
    async fn timeout_task(&self, workspace: &PathBuf, bead_id: &str) -> Result<()> {
        self.stuck_detector.timeout_task(workspace, bead_id)
    }

    /// Clear a stale assignee from a bead.
    async fn clear_assignee(&self, workspace: &PathBuf, bead_id: &str) -> Result<()> {
        // Update status to open
        let status_output = Command::new("br")
            .arg("update")
            .arg(bead_id)
            .arg("--status")
            .arg("open")
            .current_dir(workspace)
            .output()
            .map_err(|e| forge_core::ForgeError::ToolExecution {
                tool_name: "br".to_string(),
                message: e.to_string(),
            })?;

        if !status_output.status.success() {
            let stderr = String::from_utf8_lossy(&status_output.stderr);
            warn!(bead_id, error = %stderr, "Failed to update bead status");
        }

        // Clear assignee
        let assignee_output = Command::new("br")
            .arg("update")
            .arg(bead_id)
            .arg("--assignee")
            .arg("")
            .current_dir(workspace)
            .output()
            .map_err(|e| forge_core::ForgeError::ToolExecution {
                tool_name: "br".to_string(),
                message: e.to_string(),
            })?;

        if !assignee_output.status.success() {
            let stderr = String::from_utf8_lossy(&assignee_output.stderr);
            return Err(forge_core::ForgeError::ToolExecution {
                tool_name: "br".to_string(),
                message: stderr.to_string(),
            });
        }

        Ok(())
    }

    /// Get recent recovery actions for display.
    pub fn recent_actions(&self) -> &[RecoveryAction] {
        &self.recent_actions
    }

    /// Clear recent actions.
    pub fn clear_recent_actions(&mut self) {
        self.recent_actions.clear();
    }

    /// Get recovery attempt count for a worker.
    pub fn worker_attempts(&self, worker_id: &str) -> u8 {
        self.worker_attempts
            .get(worker_id)
            .map(|t| t.attempts)
            .unwrap_or(0)
    }

    /// Get recovery attempt count for a bead.
    pub fn bead_attempts(&self, bead_id: &str) -> u8 {
        self.bead_attempts
            .get(bead_id)
            .map(|t| t.attempts)
            .unwrap_or(0)
    }

    /// Reset recovery attempts for a specific entity.
    pub fn reset_attempts(&mut self, entity_id: &str) {
        if let Some(tracker) = self.worker_attempts.get_mut(entity_id) {
            tracker.reset();
        }
        if let Some(tracker) = self.bead_attempts.get_mut(entity_id) {
            tracker.reset();
        }
    }

    /// Reset all recovery attempt tracking.
    pub fn reset_all_attempts(&mut self) {
        self.worker_attempts.clear();
        self.bead_attempts.clear();
    }
}

impl Default for AutoRecoveryManager {
    fn default() -> Self {
        Self::with_defaults().expect("Failed to create default AutoRecoveryManager")
    }
}

/// Serde support for Duration using humantime format.
mod humantime_serde {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = humantime::format_duration(*duration).to_string();
        serializer.serialize_str(&s)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        humantime::parse_duration(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recovery_policy_default() {
        let policy = RecoveryPolicy::default();
        assert_eq!(policy, RecoveryPolicy::NotifyOnly);
    }

    #[test]
    fn test_recovery_config_default() {
        let config = RecoveryConfig::default();
        assert!(config.enabled);
        assert_eq!(config.check_interval_secs, 30);
        assert_eq!(config.memory_threshold_mb, 4096); // 4GB warning threshold
        assert_eq!(config.memory_kill_threshold_mb, 8192); // 8GB kill threshold
    }

    #[test]
    fn test_recovery_config_auto_recover_all() {
        let config = RecoveryConfig::auto_recover_all();
        assert!(matches!(
            config.dead_worker_policy,
            RecoveryPolicy::AutoRecover { .. }
        ));
        assert!(matches!(
            config.memory_leak_policy,
            RecoveryPolicy::AutoRecover { .. }
        ));
    }

    #[test]
    fn test_recovery_config_notify_only() {
        let config = RecoveryConfig::notify_only();
        assert_eq!(config.dead_worker_policy, RecoveryPolicy::NotifyOnly);
        assert_eq!(config.memory_leak_policy, RecoveryPolicy::NotifyOnly);
    }

    #[test]
    fn test_recovery_action_creation() {
        let action = RecoveryAction::new(
            RecoveryActionType::RestartWorker,
            "worker-1",
            "Process died",
        );

        assert_eq!(action.target, "worker-1");
        assert!(!action.executed);
        assert!(action.result.is_none());
    }

    #[test]
    fn test_recovery_action_with_result() {
        let action = RecoveryAction::new(
            RecoveryActionType::RestartWorker,
            "worker-1",
            "Process died",
        )
        .with_result("Worker restarted successfully");

        assert!(action.executed);
        assert_eq!(action.result, Some("Worker restarted successfully".to_string()));
    }

    #[test]
    fn test_recovery_action_format() {
        let mut action = RecoveryAction::new(
            RecoveryActionType::TimeoutTask,
            "bd-123",
            "Stuck for 45 minutes",
        );

        assert!(action.format_for_display().contains("PENDING"));

        action = action.with_result("Task timed out");
        assert!(action.format_for_display().contains("DONE"));
    }

    #[test]
    fn test_recovery_attempt_tracker() {
        let mut tracker = RecoveryAttemptTracker::default();

        // Should allow first attempt
        let policy = RecoveryPolicy::AutoRecover {
            max_attempts: 3,
            cooldown: Duration::from_secs(0),
        };
        assert!(tracker.can_attempt(&policy));

        // Record attempts
        tracker.record_attempt();
        assert_eq!(tracker.attempts, 1);

        tracker.record_attempt();
        tracker.record_attempt();
        assert_eq!(tracker.attempts, 3);

        // Should not allow more attempts
        assert!(!tracker.can_attempt(&policy));

        // Reset should allow again
        tracker.reset();
        assert_eq!(tracker.attempts, 0);
        assert!(tracker.can_attempt(&policy));
    }

    #[test]
    fn test_recovery_attempt_tracker_cooldown() {
        let mut tracker = RecoveryAttemptTracker::default();

        // Long cooldown
        let policy = RecoveryPolicy::AutoRecover {
            max_attempts: 10,
            cooldown: Duration::from_secs(3600), // 1 hour
        };

        tracker.record_attempt();

        // Should not allow due to cooldown (just recorded)
        assert!(!tracker.can_attempt(&policy));
    }

    #[test]
    fn test_recovery_attempt_tracker_disabled() {
        let tracker = RecoveryAttemptTracker::default();

        assert!(!tracker.can_attempt(&RecoveryPolicy::Disabled));
        assert!(!tracker.can_attempt(&RecoveryPolicy::NotifyOnly));
    }

    #[test]
    fn test_action_type_display() {
        assert_eq!(RecoveryActionType::RestartWorker.to_string(), "restart worker");
        assert_eq!(RecoveryActionType::TerminateWorker.to_string(), "terminate worker");
        assert_eq!(RecoveryActionType::TimeoutTask.to_string(), "timeout task");
        assert_eq!(RecoveryActionType::ClearAssignee.to_string(), "clear assignee");
    }
}
