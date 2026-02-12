//! Advanced health monitoring for workers with auto-recovery.
//!
//! This module implements comprehensive health monitoring for AI coding workers
//! with configurable health metrics, recovery policies, and visual indicators.
//!
//! ## Health Metrics
//!
//! 1. **Process Health**: PID exists, not zombie
//! 2. **Activity Health**: Last activity within threshold
//! 3. **Memory Health**: RSS below limit (configurable)
//! 4. **Response Health**: Responds to ping within timeout
//! 5. **Task Health**: No stuck tasks > 30 minutes
//!
//! ## Recovery Actions
//!
//! Per ADR 0014: No automatic recovery - visibility first, user decides.
//! Health status is displayed prominently but actions require user confirmation.
//!
//! ## Usage
//!
//! ```no_run
//! use forge_worker::health::{HealthMonitor, HealthMonitorConfig};
//! use forge_core::status::StatusReader;
//!
//! fn main() -> forge_core::Result<()> {
//!     let config = HealthMonitorConfig::default();
//!     let monitor = HealthMonitor::new(config)?;
//!
//!     // Check health of all workers
//!     let results = monitor.check_all_health()?;
//!     for (worker_id, health) in results {
//!         println!("{}: healthy={} score={:.2}",
//!             worker_id, health.is_healthy, health.health_score);
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
use tracing::{debug, info};

use forge_core::status::{StatusReader, WorkerStatusInfo};
use forge_core::types::WorkerStatus;

/// Default health check interval in seconds.
pub const DEFAULT_CHECK_INTERVAL_SECS: u64 = 30;

/// Default stale activity threshold in seconds (5 minutes).
pub const DEFAULT_STALE_THRESHOLD_SECS: i64 = 300;

/// Default memory limit in MB (1GB).
pub const DEFAULT_MEMORY_LIMIT_MB: u64 = 1024;

/// Default maximum recovery attempts.
pub const DEFAULT_MAX_RECOVERY_ATTEMPTS: u8 = 3;

/// Configuration for health monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthMonitorConfig {
    /// Health check interval in seconds
    pub check_interval_secs: u64,

    /// Stale activity threshold in seconds
    pub stale_activity_threshold_secs: i64,

    /// Memory limit in MB (0 = no limit)
    pub memory_limit_mb: u64,

    /// Maximum recovery attempts before escalation
    pub max_recovery_attempts: u8,

    /// Enable PID existence check
    pub enable_pid_check: bool,

    /// Enable log activity check
    pub enable_activity_check: bool,

    /// Enable memory check
    pub enable_memory_check: bool,

    /// Enable task stuck check
    pub enable_task_check: bool,

    /// Task stuck threshold in minutes
    pub task_stuck_threshold_mins: i64,
}

impl Default for HealthMonitorConfig {
    fn default() -> Self {
        Self {
            check_interval_secs: DEFAULT_CHECK_INTERVAL_SECS,
            stale_activity_threshold_secs: DEFAULT_STALE_THRESHOLD_SECS,
            memory_limit_mb: DEFAULT_MEMORY_LIMIT_MB,
            max_recovery_attempts: DEFAULT_MAX_RECOVERY_ATTEMPTS,
            enable_pid_check: true,
            enable_activity_check: true,
            enable_memory_check: false, // Disabled by default - requires procfs
            enable_task_check: true,
            task_stuck_threshold_mins: 30,
        }
    }
}

/// Types of health checks that can be performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthCheckType {
    /// Check if PID exists and is not a zombie
    PidExists,
    /// Check if last activity is within threshold
    ActivityFresh,
    /// Check if memory usage is below limit
    MemoryUsage,
    /// Check if current task is not stuck
    TaskProgress,
    /// Check if tmux session exists
    TmuxSession,
}

impl std::fmt::Display for HealthCheckType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PidExists => write!(f, "PID"),
            Self::ActivityFresh => write!(f, "Activity"),
            Self::MemoryUsage => write!(f, "Memory"),
            Self::TaskProgress => write!(f, "Task"),
            Self::TmuxSession => write!(f, "Session"),
        }
    }
}

/// Types of health errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthErrorType {
    /// Process has died
    DeadProcess,
    /// No activity for too long
    StaleActivity,
    /// Memory usage too high
    HighMemory,
    /// Task stuck for too long
    StuckTask,
    /// Tmux session missing
    MissingSession,
    /// Unknown error
    Unknown,
}

impl std::fmt::Display for HealthErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeadProcess => write!(f, "dead process"),
            Self::StaleActivity => write!(f, "stale activity"),
            Self::HighMemory => write!(f, "high memory"),
            Self::StuckTask => write!(f, "stuck task"),
            Self::MissingSession => write!(f, "missing session"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Result of a single health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    /// Type of check performed
    pub check_type: HealthCheckType,
    /// Whether the check passed
    pub passed: bool,
    /// Error type if check failed
    pub error_type: Option<HealthErrorType>,
    /// Human-readable error message
    pub error_message: Option<String>,
    /// Timestamp of the check
    pub timestamp: DateTime<Utc>,
}

impl HealthCheckResult {
    /// Create a passing health check result.
    pub fn passed(check_type: HealthCheckType) -> Self {
        Self {
            check_type,
            passed: true,
            error_type: None,
            error_message: None,
            timestamp: Utc::now(),
        }
    }

    /// Create a failing health check result.
    pub fn failed(
        check_type: HealthCheckType,
        error_type: HealthErrorType,
        message: impl Into<String>,
    ) -> Self {
        Self {
            check_type,
            passed: false,
            error_type: Some(error_type),
            error_message: Some(message.into()),
            timestamp: Utc::now(),
        }
    }

    /// Create a skipped health check result (check disabled).
    pub fn skipped(check_type: HealthCheckType) -> Self {
        Self {
            check_type,
            passed: true, // Skipped counts as passing
            error_type: None,
            error_message: Some("Check disabled".to_string()),
            timestamp: Utc::now(),
        }
    }
}

/// Aggregated health status for a worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerHealthStatus {
    /// Worker identifier
    pub worker_id: String,
    /// Overall health status
    pub is_healthy: bool,
    /// Health score (0.0 - 1.0)
    pub health_score: f32,
    /// Individual check results
    pub check_results: Vec<HealthCheckResult>,
    /// Failed check types
    pub failed_checks: Vec<HealthCheckType>,
    /// Primary error message
    pub primary_error: Option<String>,
    /// Actionable guidance for user
    pub guidance: Vec<String>,
    /// Last health check timestamp
    pub last_checked: DateTime<Utc>,
    /// Recovery attempts count
    pub recovery_attempts: u8,
}

impl WorkerHealthStatus {
    /// Create a new health status for a worker.
    pub fn new(worker_id: impl Into<String>) -> Self {
        Self {
            worker_id: worker_id.into(),
            is_healthy: true,
            health_score: 1.0,
            check_results: Vec::new(),
            failed_checks: Vec::new(),
            primary_error: None,
            guidance: Vec::new(),
            last_checked: Utc::now(),
            recovery_attempts: 0,
        }
    }

    /// Add a check result and update health score.
    pub fn add_result(&mut self, result: HealthCheckResult) {
        if !result.passed {
            self.failed_checks.push(result.check_type);
            self.is_healthy = false;
        }
        self.check_results.push(result);
        self.recalculate_score();
    }

    /// Recalculate health score based on check results.
    fn recalculate_score(&mut self) {
        if self.check_results.is_empty() {
            self.health_score = 1.0;
            return;
        }

        let passed = self.check_results.iter().filter(|r| r.passed).count();
        let total = self.check_results.len();
        self.health_score = passed as f32 / total as f32;
    }

    /// Get health indicator emoji for TUI display.
    pub fn health_indicator(&self) -> &'static str {
        if self.health_score >= 0.8 {
            "●" // Green - healthy
        } else if self.health_score >= 0.5 {
            "◐" // Yellow - degraded
        } else {
            "○" // Red - unhealthy
        }
    }

    /// Get health level for coloring.
    pub fn health_level(&self) -> HealthLevel {
        if self.health_score >= 0.8 {
            HealthLevel::Healthy
        } else if self.health_score >= 0.5 {
            HealthLevel::Degraded
        } else {
            HealthLevel::Unhealthy
        }
    }

    /// Generate actionable guidance based on failed checks.
    pub fn generate_guidance(&mut self) {
        self.guidance.clear();

        for check_type in &self.failed_checks {
            match check_type {
                HealthCheckType::PidExists => {
                    self.guidance.push("Process died - restart the worker".to_string());
                }
                HealthCheckType::ActivityFresh => {
                    self.guidance.push("Worker may be stuck - check logs".to_string());
                }
                HealthCheckType::MemoryUsage => {
                    self.guidance.push("Memory usage high - consider restart".to_string());
                }
                HealthCheckType::TaskProgress => {
                    self.guidance.push("Task may be stuck - verify progress".to_string());
                }
                HealthCheckType::TmuxSession => {
                    self.guidance.push("Session missing - restart required".to_string());
                }
            }
        }

        // Set primary error
        if let Some(first_failed) = self.check_results.iter().find(|r| !r.passed) {
            self.primary_error = first_failed.error_message.clone();
        }
    }
}

/// Health level for coloring and display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HealthLevel {
    /// Healthy (green)
    Healthy,
    /// Degraded (yellow)
    Degraded,
    /// Unhealthy (red)
    Unhealthy,
}

/// The health monitoring engine.
#[derive(Debug)]
pub struct HealthMonitor {
    /// Configuration
    config: HealthMonitorConfig,
    /// Status reader for worker status files
    status_reader: StatusReader,
    /// Log directory for activity checks
    log_dir: PathBuf,
    /// Recovery attempt tracking per worker
    recovery_attempts: HashMap<String, u8>,
}

impl HealthMonitor {
    /// Create a new health monitor with default configuration.
    pub fn new(config: HealthMonitorConfig) -> forge_core::Result<Self> {
        let status_reader = StatusReader::new(None)?;

        let home = std::env::var("HOME").map_err(|_| forge_core::ForgeError::ConfigMissingField {
            field: "HOME environment variable".to_string(),
        })?;
        let log_dir = PathBuf::from(home).join(".forge").join("logs");

        Ok(Self {
            config,
            status_reader,
            log_dir,
            recovery_attempts: HashMap::new(),
        })
    }

    /// Create a health monitor with custom directories.
    pub fn with_dirs(
        config: HealthMonitorConfig,
        status_dir: PathBuf,
        log_dir: PathBuf,
    ) -> forge_core::Result<Self> {
        let status_reader = StatusReader::new(Some(status_dir))?;

        Ok(Self {
            config,
            status_reader,
            log_dir,
            recovery_attempts: HashMap::new(),
        })
    }

    /// Check health of all known workers.
    pub fn check_all_health(&mut self) -> forge_core::Result<HashMap<String, WorkerHealthStatus>> {
        let workers = self.status_reader.read_all()?;
        let mut results = HashMap::new();

        for worker in workers {
            let health = self.check_worker_health(&worker);
            results.insert(worker.worker_id.clone(), health);
        }

        Ok(results)
    }

    /// Check health of a specific worker.
    pub fn check_worker_health(&mut self, worker: &WorkerStatusInfo) -> WorkerHealthStatus {
        let mut status = WorkerHealthStatus::new(&worker.worker_id);

        // Carry over recovery attempts
        if let Some(&attempts) = self.recovery_attempts.get(&worker.worker_id) {
            status.recovery_attempts = attempts;
        }

        // Run health checks
        if self.config.enable_pid_check {
            let result = self.check_pid_exists(worker);
            status.add_result(result);
        }

        if self.config.enable_activity_check {
            let result = self.check_activity_fresh(worker);
            status.add_result(result);
        }

        if self.config.enable_memory_check {
            let result = self.check_memory_usage(worker);
            status.add_result(result);
        }

        if self.config.enable_task_check {
            let result = self.check_task_progress(worker);
            status.add_result(result);
        }

        // Generate guidance based on results
        status.generate_guidance();
        status.last_checked = Utc::now();

        debug!(
            worker_id = %worker.worker_id,
            is_healthy = status.is_healthy,
            health_score = status.health_score,
            failed_checks = ?status.failed_checks,
            "Health check completed"
        );

        status
    }

    /// Check if worker's PID exists and is not a zombie.
    fn check_pid_exists(&self, worker: &WorkerStatusInfo) -> HealthCheckResult {
        let Some(pid) = worker.pid else {
            return HealthCheckResult::failed(
                HealthCheckType::PidExists,
                HealthErrorType::Unknown,
                "No PID recorded in status file",
            );
        };

        // Check if process exists using kill -0
        let output = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .output();

        match output {
            Ok(output) if output.status.success() => {
                // Process exists - check if it's a zombie
                let stat_path = format!("/proc/{}/stat", pid);
                if let Ok(stat) = std::fs::read_to_string(&stat_path) {
                    // Third field is state: Z = zombie
                    let fields: Vec<&str> = stat.split_whitespace().collect();
                    if fields.len() > 2 && fields[2] == "Z" {
                        return HealthCheckResult::failed(
                            HealthCheckType::PidExists,
                            HealthErrorType::DeadProcess,
                            format!("Process {} is a zombie", pid),
                        );
                    }
                }
                HealthCheckResult::passed(HealthCheckType::PidExists)
            }
            _ => {
                HealthCheckResult::failed(
                    HealthCheckType::PidExists,
                    HealthErrorType::DeadProcess,
                    format!("Process {} does not exist", pid),
                )
            }
        }
    }

    /// Check if worker's last activity is within threshold.
    fn check_activity_fresh(&self, worker: &WorkerStatusInfo) -> HealthCheckResult {
        let Some(last_activity) = worker.last_activity else {
            // No activity recorded - skip if worker is starting
            if worker.status == WorkerStatus::Starting {
                return HealthCheckResult::passed(HealthCheckType::ActivityFresh);
            }
            return HealthCheckResult::failed(
                HealthCheckType::ActivityFresh,
                HealthErrorType::StaleActivity,
                "No activity timestamp recorded",
            );
        };

        let now = Utc::now();
        let elapsed = now.signed_duration_since(last_activity);
        let elapsed_secs = elapsed.num_seconds();

        if elapsed_secs > self.config.stale_activity_threshold_secs {
            let elapsed_mins = elapsed_secs / 60;
            return HealthCheckResult::failed(
                HealthCheckType::ActivityFresh,
                HealthErrorType::StaleActivity,
                format!(
                    "No activity for {} minutes (threshold: {} mins)",
                    elapsed_mins,
                    self.config.stale_activity_threshold_secs / 60
                ),
            );
        }

        HealthCheckResult::passed(HealthCheckType::ActivityFresh)
    }

    /// Check memory usage (Linux only, uses /proc).
    fn check_memory_usage(&self, worker: &WorkerStatusInfo) -> HealthCheckResult {
        let Some(pid) = worker.pid else {
            return HealthCheckResult::skipped(HealthCheckType::MemoryUsage);
        };

        if self.config.memory_limit_mb == 0 {
            return HealthCheckResult::skipped(HealthCheckType::MemoryUsage);
        }

        // Read RSS from /proc/<pid>/status
        let status_path = format!("/proc/{}/status", pid);
        if let Ok(content) = std::fs::read_to_string(&status_path) {
            for line in content.lines() {
                if line.starts_with("VmRSS:") {
                    // Parse "VmRSS: 12345 kB"
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        if let Ok(rss_kb) = parts[1].parse::<u64>() {
                            let rss_mb = rss_kb / 1024;
                            if rss_mb > self.config.memory_limit_mb {
                                return HealthCheckResult::failed(
                                    HealthCheckType::MemoryUsage,
                                    HealthErrorType::HighMemory,
                                    format!(
                                        "Memory usage {}MB exceeds limit {}MB",
                                        rss_mb, self.config.memory_limit_mb
                                    ),
                                );
                            }
                            return HealthCheckResult::passed(HealthCheckType::MemoryUsage);
                        }
                    }
                }
            }
        }

        // Could not read memory info - skip
        HealthCheckResult::skipped(HealthCheckType::MemoryUsage)
    }

    /// Check if task has been stuck for too long.
    fn check_task_progress(&self, worker: &WorkerStatusInfo) -> HealthCheckResult {
        // Only check active workers with a task
        if worker.status != WorkerStatus::Active {
            return HealthCheckResult::passed(HealthCheckType::TaskProgress);
        }

        let Some(last_activity) = worker.last_activity else {
            return HealthCheckResult::passed(HealthCheckType::TaskProgress);
        };

        let now = Utc::now();
        let elapsed = now.signed_duration_since(last_activity);
        let elapsed_mins = elapsed.num_minutes();

        // If worker is active with a task but no activity for task_stuck_threshold_mins
        if elapsed_mins > self.config.task_stuck_threshold_mins && worker.current_task.is_some() {
            return HealthCheckResult::failed(
                HealthCheckType::TaskProgress,
                HealthErrorType::StuckTask,
                format!(
                    "Task {} stuck for {} minutes",
                    worker.current_task.as_deref().unwrap_or("unknown"),
                    elapsed_mins
                ),
            );
        }

        HealthCheckResult::passed(HealthCheckType::TaskProgress)
    }

    /// Record a recovery attempt for a worker.
    pub fn record_recovery_attempt(&mut self, worker_id: &str) {
        let attempts = self.recovery_attempts.entry(worker_id.to_string()).or_insert(0);
        *attempts = attempts.saturating_add(1);
        info!(
            worker_id = %worker_id,
            attempts = *attempts,
            max = self.config.max_recovery_attempts,
            "Recorded recovery attempt"
        );
    }

    /// Check if worker has exceeded max recovery attempts.
    pub fn is_recovery_exhausted(&self, worker_id: &str) -> bool {
        self.recovery_attempts
            .get(worker_id)
            .map(|&attempts| attempts >= self.config.max_recovery_attempts)
            .unwrap_or(false)
    }

    /// Reset recovery attempts for a worker (after successful recovery).
    pub fn reset_recovery_attempts(&mut self, worker_id: &str) {
        self.recovery_attempts.remove(worker_id);
        debug!(worker_id = %worker_id, "Reset recovery attempts");
    }

    /// Get the configuration.
    pub fn config(&self) -> &HealthMonitorConfig {
        &self.config
    }

    /// Get the log directory.
    pub fn log_dir(&self) -> &PathBuf {
        &self.log_dir
    }
}

/// Convenience function to check health of a single worker.
pub fn check_worker_health(
    worker_id: &str,
    status_dir: &PathBuf,
    log_dir: &PathBuf,
) -> forge_core::Result<WorkerHealthStatus> {
    let config = HealthMonitorConfig::default();
    let mut monitor = HealthMonitor::with_dirs(config, status_dir.clone(), log_dir.clone())?;

    let worker = monitor
        .status_reader
        .read_worker(worker_id)?
        .ok_or_else(|| forge_core::ForgeError::StatusFileParse {
            path: status_dir.join(format!("{}.json", worker_id)),
            message: "Worker not found".to_string(),
        })?;

    Ok(monitor.check_worker_health(&worker))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_status_file(dir: &PathBuf, worker_id: &str, content: &str) {
        std::fs::write(dir.join(format!("{}.json", worker_id)), content).unwrap();
    }

    #[test]
    fn test_health_monitor_config_default() {
        let config = HealthMonitorConfig::default();
        assert_eq!(config.check_interval_secs, 30);
        assert_eq!(config.stale_activity_threshold_secs, 300);
        assert!(config.enable_pid_check);
        assert!(config.enable_activity_check);
    }

    #[test]
    fn test_health_check_result_passed() {
        let result = HealthCheckResult::passed(HealthCheckType::PidExists);
        assert!(result.passed);
        assert!(result.error_type.is_none());
    }

    #[test]
    fn test_health_check_result_failed() {
        let result = HealthCheckResult::failed(
            HealthCheckType::PidExists,
            HealthErrorType::DeadProcess,
            "Process 123 does not exist",
        );
        assert!(!result.passed);
        assert_eq!(result.error_type, Some(HealthErrorType::DeadProcess));
    }

    #[test]
    fn test_worker_health_status_initial() {
        let status = WorkerHealthStatus::new("test-worker");
        assert!(status.is_healthy);
        assert_eq!(status.health_score, 1.0);
        assert!(status.check_results.is_empty());
    }

    #[test]
    fn test_worker_health_status_add_passed_result() {
        let mut status = WorkerHealthStatus::new("test-worker");
        status.add_result(HealthCheckResult::passed(HealthCheckType::PidExists));

        assert!(status.is_healthy);
        assert_eq!(status.health_score, 1.0);
        assert!(status.failed_checks.is_empty());
    }

    #[test]
    fn test_worker_health_status_add_failed_result() {
        let mut status = WorkerHealthStatus::new("test-worker");
        status.add_result(HealthCheckResult::failed(
            HealthCheckType::PidExists,
            HealthErrorType::DeadProcess,
            "Process died",
        ));

        assert!(!status.is_healthy);
        assert_eq!(status.health_score, 0.0);
        assert!(status.failed_checks.contains(&HealthCheckType::PidExists));
    }

    #[test]
    fn test_worker_health_status_mixed_results() {
        let mut status = WorkerHealthStatus::new("test-worker");
        status.add_result(HealthCheckResult::passed(HealthCheckType::PidExists));
        status.add_result(HealthCheckResult::passed(HealthCheckType::ActivityFresh));
        status.add_result(HealthCheckResult::failed(
            HealthCheckType::MemoryUsage,
            HealthErrorType::HighMemory,
            "Memory high",
        ));
        status.add_result(HealthCheckResult::passed(HealthCheckType::TaskProgress));

        assert!(!status.is_healthy);
        assert!((status.health_score - 0.75).abs() < 0.01);
        assert_eq!(status.failed_checks.len(), 1);
    }

    #[test]
    fn test_health_indicator() {
        let mut status = WorkerHealthStatus::new("test-worker");

        // 100% - green
        status.health_score = 1.0;
        assert_eq!(status.health_indicator(), "●");

        // 80% - green
        status.health_score = 0.8;
        assert_eq!(status.health_indicator(), "●");

        // 60% - yellow
        status.health_score = 0.6;
        assert_eq!(status.health_indicator(), "◐");

        // 40% - red
        status.health_score = 0.4;
        assert_eq!(status.health_indicator(), "○");
    }

    #[test]
    fn test_health_level() {
        let mut status = WorkerHealthStatus::new("test-worker");

        status.health_score = 1.0;
        assert_eq!(status.health_level(), HealthLevel::Healthy);

        status.health_score = 0.6;
        assert_eq!(status.health_level(), HealthLevel::Degraded);

        status.health_score = 0.3;
        assert_eq!(status.health_level(), HealthLevel::Unhealthy);
    }

    #[test]
    fn test_generate_guidance() {
        let mut status = WorkerHealthStatus::new("test-worker");
        status.add_result(HealthCheckResult::failed(
            HealthCheckType::PidExists,
            HealthErrorType::DeadProcess,
            "Process died",
        ));
        status.generate_guidance();

        assert!(!status.guidance.is_empty());
        assert!(status.guidance[0].contains("restart"));
        assert!(status.primary_error.is_some());
    }

    #[test]
    fn test_recovery_attempts() {
        let config = HealthMonitorConfig::default();
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().to_path_buf();
        let log_dir = temp_dir.path().to_path_buf();

        let mut monitor =
            HealthMonitor::with_dirs(config, status_dir, log_dir).expect("Failed to create monitor");

        let worker_id = "test-worker";

        // Initially not exhausted
        assert!(!monitor.is_recovery_exhausted(worker_id));

        // Record attempts
        monitor.record_recovery_attempt(worker_id);
        monitor.record_recovery_attempt(worker_id);
        assert!(!monitor.is_recovery_exhausted(worker_id));

        monitor.record_recovery_attempt(worker_id);
        assert!(monitor.is_recovery_exhausted(worker_id));

        // Reset
        monitor.reset_recovery_attempts(worker_id);
        assert!(!monitor.is_recovery_exhausted(worker_id));
    }

    #[test]
    fn test_check_activity_fresh_recent() {
        let config = HealthMonitorConfig {
            stale_activity_threshold_secs: 300,
            ..Default::default()
        };
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().to_path_buf();
        let log_dir = temp_dir.path().to_path_buf();

        let monitor =
            HealthMonitor::with_dirs(config, status_dir, log_dir).expect("Failed to create monitor");

        let worker = WorkerStatusInfo {
            worker_id: "test".to_string(),
            status: WorkerStatus::Active,
            last_activity: Some(Utc::now()),
            ..Default::default()
        };

        let result = monitor.check_activity_fresh(&worker);
        assert!(result.passed);
    }

    #[test]
    fn test_check_activity_fresh_stale() {
        let config = HealthMonitorConfig {
            stale_activity_threshold_secs: 300,
            ..Default::default()
        };
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().to_path_buf();
        let log_dir = temp_dir.path().to_path_buf();

        let monitor =
            HealthMonitor::with_dirs(config, status_dir, log_dir).expect("Failed to create monitor");

        let worker = WorkerStatusInfo {
            worker_id: "test".to_string(),
            status: WorkerStatus::Active,
            last_activity: Some(Utc::now() - chrono::Duration::seconds(600)),
            ..Default::default()
        };

        let result = monitor.check_activity_fresh(&worker);
        assert!(!result.passed);
        assert_eq!(result.error_type, Some(HealthErrorType::StaleActivity));
    }

    #[test]
    fn test_check_task_progress_no_task() {
        let config = HealthMonitorConfig {
            task_stuck_threshold_mins: 30,
            ..Default::default()
        };
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().to_path_buf();
        let log_dir = temp_dir.path().to_path_buf();

        let monitor =
            HealthMonitor::with_dirs(config, status_dir, log_dir).expect("Failed to create monitor");

        // Idle worker - should pass
        let worker = WorkerStatusInfo {
            worker_id: "test".to_string(),
            status: WorkerStatus::Idle,
            last_activity: Some(Utc::now() - chrono::Duration::minutes(60)),
            ..Default::default()
        };

        let result = monitor.check_task_progress(&worker);
        assert!(result.passed);
    }

    #[test]
    fn test_check_task_progress_stuck() {
        let config = HealthMonitorConfig {
            task_stuck_threshold_mins: 30,
            ..Default::default()
        };
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().to_path_buf();
        let log_dir = temp_dir.path().to_path_buf();

        let monitor =
            HealthMonitor::with_dirs(config, status_dir, log_dir).expect("Failed to create monitor");

        // Active worker with stuck task
        let worker = WorkerStatusInfo {
            worker_id: "test".to_string(),
            status: WorkerStatus::Active,
            last_activity: Some(Utc::now() - chrono::Duration::minutes(45)),
            current_task: Some("bd-123".to_string()),
            ..Default::default()
        };

        let result = monitor.check_task_progress(&worker);
        assert!(!result.passed);
        assert_eq!(result.error_type, Some(HealthErrorType::StuckTask));
    }

    #[test]
    fn test_health_check_type_display() {
        assert_eq!(HealthCheckType::PidExists.to_string(), "PID");
        assert_eq!(HealthCheckType::ActivityFresh.to_string(), "Activity");
        assert_eq!(HealthCheckType::MemoryUsage.to_string(), "Memory");
        assert_eq!(HealthCheckType::TaskProgress.to_string(), "Task");
        assert_eq!(HealthCheckType::TmuxSession.to_string(), "Session");
    }

    #[test]
    fn test_health_error_type_display() {
        assert_eq!(HealthErrorType::DeadProcess.to_string(), "dead process");
        assert_eq!(HealthErrorType::StaleActivity.to_string(), "stale activity");
        assert_eq!(HealthErrorType::HighMemory.to_string(), "high memory");
        assert_eq!(HealthErrorType::StuckTask.to_string(), "stuck task");
        assert_eq!(HealthErrorType::MissingSession.to_string(), "missing session");
    }
}
