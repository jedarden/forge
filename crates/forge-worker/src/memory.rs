//! Memory monitoring for workers.
//!
//! This module provides comprehensive memory tracking for AI coding workers:
//!
//! - **RSS tracking**: Monitor resident set size per worker
//! - **Growth rate logging**: Track memory growth over time
//! - **Configurable limits**: Alert at 4GB, kill at 8GB by default
//! - **Runaway detection**: Automatically kill workers exceeding 8GB
//!
//! ## Usage
//!
//! ```no_run
//! use forge_worker::memory::{MemoryMonitor, MemoryConfig};
//!
//! fn main() -> forge_core::Result<()> {
//!     let config = MemoryConfig::default();
//!     let mut monitor = MemoryMonitor::new(config);
//!
//!     // Check memory for a specific worker PID
//!     if let Some(stats) = monitor.check_worker_memory(12345, "worker-1")? {
//!         println!("Worker {} RSS: {} MB", stats.worker_id, stats.rss_mb);
//!         println!("Growth rate: {:.2} MB/min", stats.growth_rate_mb_per_min);
//!
//!         if stats.exceeds_kill_limit {
//!             println!("CRITICAL: Worker exceeds 8GB limit!");
//!         }
//!     }
//!
//!     Ok(())
//! }
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;
use tracing::{debug, error, info, warn};

use forge_core::Result;

use crate::health::{DEFAULT_MEMORY_KILL_LIMIT_MB, DEFAULT_MEMORY_LIMIT_MB};

/// Configuration for memory monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Memory warning threshold in MB (default: 4GB).
    /// Workers exceeding this will trigger warnings.
    pub warning_limit_mb: u64,

    /// Memory kill threshold in MB (default: 8GB).
    /// Workers exceeding this will be forcefully terminated.
    pub kill_limit_mb: u64,

    /// Enable growth rate calculation.
    pub track_growth_rate: bool,

    /// Number of samples to keep for growth rate calculation.
    pub sample_history_size: usize,

    /// Minimum time between samples (seconds) for growth rate.
    pub min_sample_interval_secs: u64,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            warning_limit_mb: DEFAULT_MEMORY_LIMIT_MB,  // 4GB
            kill_limit_mb: DEFAULT_MEMORY_KILL_LIMIT_MB, // 8GB
            track_growth_rate: true,
            sample_history_size: 10,
            min_sample_interval_secs: 30,
        }
    }
}

/// Memory sample for a worker at a point in time.
#[derive(Debug, Clone)]
struct MemorySample {
    /// RSS in MB.
    rss_mb: u64,
    /// Timestamp of the sample.
    timestamp: DateTime<Utc>,
}

/// Memory statistics for a worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMemoryStats {
    /// Worker identifier.
    pub worker_id: String,

    /// Process ID.
    pub pid: u32,

    /// Current RSS in MB.
    pub rss_mb: u64,

    /// Current RSS in bytes (for precision).
    pub rss_bytes: u64,

    /// Virtual memory size in MB.
    pub vms_mb: u64,

    /// Memory growth rate in MB per minute.
    /// Calculated from recent samples. Positive = growing, negative = shrinking.
    pub growth_rate_mb_per_min: f64,

    /// Whether the worker exceeds the warning limit.
    pub exceeds_warning_limit: bool,

    /// Whether the worker exceeds the kill limit (runaway).
    pub exceeds_kill_limit: bool,

    /// Timestamp of this measurement.
    pub timestamp: DateTime<Utc>,

    /// Time since worker started (if known), in seconds.
    pub uptime_secs: Option<u64>,
}

impl WorkerMemoryStats {
    /// Format RSS for display (e.g., "1.5 GB" or "512 MB").
    pub fn format_rss(&self) -> String {
        if self.rss_mb >= 1024 {
            format!("{:.1} GB", self.rss_mb as f64 / 1024.0)
        } else {
            format!("{} MB", self.rss_mb)
        }
    }

    /// Format growth rate for display.
    pub fn format_growth_rate(&self) -> String {
        if self.growth_rate_mb_per_min.abs() < 0.1 {
            "stable".to_string()
        } else if self.growth_rate_mb_per_min > 0.0 {
            format!("+{:.1} MB/min", self.growth_rate_mb_per_min)
        } else {
            format!("{:.1} MB/min", self.growth_rate_mb_per_min)
        }
    }

    /// Get a severity level for display/alerting.
    pub fn severity(&self) -> MemorySeverity {
        if self.exceeds_kill_limit {
            MemorySeverity::Critical
        } else if self.exceeds_warning_limit {
            MemorySeverity::Warning
        } else if self.rss_mb > 2048 {
            // Above 2GB, show elevated status
            MemorySeverity::Elevated
        } else {
            MemorySeverity::Normal
        }
    }
}

/// Severity level for memory usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySeverity {
    /// Normal memory usage.
    Normal,
    /// Elevated but not concerning (2-4GB).
    Elevated,
    /// Warning level - exceeds warning limit (4-8GB).
    Warning,
    /// Critical - exceeds kill limit (>8GB), worker should be terminated.
    Critical,
}

impl std::fmt::Display for MemorySeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "normal"),
            Self::Elevated => write!(f, "elevated"),
            Self::Warning => write!(f, "warning"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

/// Memory monitor for tracking worker memory usage.
#[derive(Debug)]
pub struct MemoryMonitor {
    /// Configuration.
    config: MemoryConfig,

    /// Sample history per worker (keyed by worker_id).
    sample_history: HashMap<String, Vec<MemorySample>>,

    /// Latest stats per worker.
    latest_stats: HashMap<String, WorkerMemoryStats>,
}

impl MemoryMonitor {
    /// Create a new memory monitor.
    pub fn new(config: MemoryConfig) -> Self {
        Self {
            config,
            sample_history: HashMap::new(),
            latest_stats: HashMap::new(),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(MemoryConfig::default())
    }

    /// Check memory for a worker by PID.
    ///
    /// Returns `None` if the PID doesn't exist or memory info is unavailable.
    pub fn check_worker_memory(
        &mut self,
        pid: u32,
        worker_id: &str,
    ) -> Result<Option<WorkerMemoryStats>> {
        // Read memory info from /proc/<pid>/status
        let (rss_kb, vms_kb) = match self.read_proc_memory(pid) {
            Some(mem) => mem,
            None => {
                debug!(pid, worker_id, "Could not read memory info from /proc");
                return Ok(None);
            }
        };

        let rss_mb = rss_kb / 1024;
        let rss_bytes = rss_kb * 1024;
        let vms_mb = vms_kb / 1024;
        let now = Utc::now();

        // Store sample for growth rate calculation
        let sample = MemorySample {
            rss_mb,
            timestamp: now,
        };

        let history = self
            .sample_history
            .entry(worker_id.to_string())
            .or_insert_with(Vec::new);

        // Check if enough time has passed since last sample
        let should_add_sample = history.last().map_or(true, |last| {
            let elapsed = now.signed_duration_since(last.timestamp);
            elapsed.num_seconds() >= self.config.min_sample_interval_secs as i64
        });

        if should_add_sample {
            history.push(sample);
            // Trim to max size
            while history.len() > self.config.sample_history_size {
                history.remove(0);
            }
        }

        // Calculate growth rate (take a slice to avoid borrow conflict)
        let growth_rate = Self::calculate_growth_rate_static(&self.config, history);

        // Check limits
        let exceeds_warning = rss_mb > self.config.warning_limit_mb;
        let exceeds_kill = rss_mb > self.config.kill_limit_mb;

        // Log memory status
        if exceeds_kill {
            error!(
                worker_id,
                pid,
                rss_mb,
                kill_limit = self.config.kill_limit_mb,
                "CRITICAL: Worker memory exceeds kill limit! Worker should be terminated."
            );
        } else if exceeds_warning {
            warn!(
                worker_id,
                pid,
                rss_mb,
                warning_limit = self.config.warning_limit_mb,
                growth_rate_mb_per_min = growth_rate,
                "Worker memory exceeds warning limit"
            );
        } else if self.config.track_growth_rate && growth_rate.abs() > 10.0 {
            // Log significant growth (> 10 MB/min)
            info!(
                worker_id,
                pid,
                rss_mb,
                growth_rate_mb_per_min = growth_rate,
                "Worker memory growing significantly"
            );
        }

        let stats = WorkerMemoryStats {
            worker_id: worker_id.to_string(),
            pid,
            rss_mb,
            rss_bytes,
            vms_mb,
            growth_rate_mb_per_min: growth_rate,
            exceeds_warning_limit: exceeds_warning,
            exceeds_kill_limit: exceeds_kill,
            timestamp: now,
            uptime_secs: None, // Could be enhanced to read from /proc/<pid>/stat
        };

        // Store latest stats
        self.latest_stats
            .insert(worker_id.to_string(), stats.clone());

        Ok(Some(stats))
    }

    /// Read memory info from /proc/<pid>/status.
    ///
    /// Returns (rss_kb, vms_kb) or None if unavailable.
    fn read_proc_memory(&self, pid: u32) -> Option<(u64, u64)> {
        let status_path = format!("/proc/{}/status", pid);
        let content = std::fs::read_to_string(&status_path).ok()?;

        let mut rss_kb = 0u64;
        let mut vms_kb = 0u64;

        for line in content.lines() {
            if line.starts_with("VmRSS:") {
                // Parse "VmRSS: 12345 kB"
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    rss_kb = parts[1].parse().unwrap_or(0);
                }
            } else if line.starts_with("VmSize:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    vms_kb = parts[1].parse().unwrap_or(0);
                }
            }
        }

        if rss_kb > 0 {
            Some((rss_kb, vms_kb))
        } else {
            None
        }
    }

    /// Calculate memory growth rate from sample history.
    ///
    /// Returns MB/minute. Positive = growing, negative = shrinking.
    /// Used internally for test coverage.
    #[cfg(test)]
    fn calculate_growth_rate(&self, history: &[MemorySample]) -> f64 {
        Self::calculate_growth_rate_static(&self.config, history)
    }

    /// Static version of calculate_growth_rate to avoid borrow conflicts.
    fn calculate_growth_rate_static(config: &MemoryConfig, history: &[MemorySample]) -> f64 {
        if !config.track_growth_rate || history.len() < 2 {
            return 0.0;
        }

        // Use linear regression or simple first/last comparison
        let first = &history[0];
        let last = history.last().unwrap();

        let duration_mins = last
            .timestamp
            .signed_duration_since(first.timestamp)
            .num_seconds() as f64
            / 60.0;

        if duration_mins < 0.5 {
            // Not enough time for meaningful rate
            return 0.0;
        }

        let delta_mb = last.rss_mb as f64 - first.rss_mb as f64;
        delta_mb / duration_mins
    }

    /// Get the latest stats for a worker.
    pub fn get_latest_stats(&self, worker_id: &str) -> Option<&WorkerMemoryStats> {
        self.latest_stats.get(worker_id)
    }

    /// Get all latest stats.
    pub fn get_all_stats(&self) -> &HashMap<String, WorkerMemoryStats> {
        &self.latest_stats
    }

    /// Get workers that exceed the kill limit (runaway workers).
    pub fn get_runaway_workers(&self) -> Vec<&WorkerMemoryStats> {
        self.latest_stats
            .values()
            .filter(|s| s.exceeds_kill_limit)
            .collect()
    }

    /// Get workers that exceed the warning limit.
    pub fn get_warning_workers(&self) -> Vec<&WorkerMemoryStats> {
        self.latest_stats
            .values()
            .filter(|s| s.exceeds_warning_limit && !s.exceeds_kill_limit)
            .collect()
    }

    /// Clear stats for a worker (e.g., after termination).
    pub fn clear_worker(&mut self, worker_id: &str) {
        self.sample_history.remove(worker_id);
        self.latest_stats.remove(worker_id);
    }

    /// Get configuration.
    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }

    /// Kill a runaway worker process.
    ///
    /// This sends SIGKILL to the process. Use with caution.
    pub fn kill_runaway_worker(&self, pid: u32, worker_id: &str) -> Result<bool> {
        info!(
            worker_id,
            pid, "Killing runaway worker (memory > {} MB)",
            self.config.kill_limit_mb
        );

        let output = Command::new("kill")
            .arg("-9") // SIGKILL
            .arg(pid.to_string())
            .output();

        match output {
            Ok(output) => {
                if output.status.success() {
                    info!(worker_id, pid, "Runaway worker killed successfully");
                    Ok(true)
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!(
                        worker_id,
                        pid,
                        stderr = %stderr,
                        "Failed to kill runaway worker"
                    );
                    Ok(false)
                }
            }
            Err(e) => {
                error!(worker_id, pid, error = %e, "Error executing kill command");
                Err(forge_core::ForgeError::ToolExecution {
                    tool_name: "kill".to_string(),
                    message: e.to_string(),
                })
            }
        }
    }

    /// Kill the tmux session for a runaway worker.
    ///
    /// This is a cleaner way to terminate a worker than just killing the PID.
    pub fn kill_worker_session(&self, session_name: &str, worker_id: &str) -> Result<bool> {
        info!(
            worker_id,
            session_name, "Killing worker tmux session (memory exceeded kill limit)"
        );

        let output = Command::new("tmux")
            .arg("kill-session")
            .arg("-t")
            .arg(session_name)
            .output();

        match output {
            Ok(output) => {
                if output.status.success() {
                    info!(
                        worker_id,
                        session_name, "Worker session killed successfully"
                    );
                    Ok(true)
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!(
                        worker_id,
                        session_name,
                        stderr = %stderr,
                        "Failed to kill worker session"
                    );
                    Ok(false)
                }
            }
            Err(e) => {
                error!(
                    worker_id,
                    session_name,
                    error = %e,
                    "Error executing tmux kill-session"
                );
                Err(forge_core::ForgeError::ToolExecution {
                    tool_name: "tmux".to_string(),
                    message: e.to_string(),
                })
            }
        }
    }
}

impl Default for MemoryMonitor {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_config_default() {
        let config = MemoryConfig::default();
        assert_eq!(config.warning_limit_mb, 4096); // 4GB
        assert_eq!(config.kill_limit_mb, 8192); // 8GB
        assert!(config.track_growth_rate);
    }

    #[test]
    fn test_memory_stats_format_rss() {
        let stats = WorkerMemoryStats {
            worker_id: "test".to_string(),
            pid: 123,
            rss_mb: 512,
            rss_bytes: 512 * 1024 * 1024,
            vms_mb: 1024,
            growth_rate_mb_per_min: 0.0,
            exceeds_warning_limit: false,
            exceeds_kill_limit: false,
            timestamp: Utc::now(),
            uptime_secs: None,
        };

        assert_eq!(stats.format_rss(), "512 MB");

        let large_stats = WorkerMemoryStats {
            rss_mb: 2048,
            ..stats.clone()
        };
        assert_eq!(large_stats.format_rss(), "2.0 GB");

        let very_large_stats = WorkerMemoryStats {
            rss_mb: 5632,
            ..stats
        };
        assert_eq!(very_large_stats.format_rss(), "5.5 GB");
    }

    #[test]
    fn test_memory_stats_format_growth_rate() {
        let mut stats = WorkerMemoryStats {
            worker_id: "test".to_string(),
            pid: 123,
            rss_mb: 1024,
            rss_bytes: 1024 * 1024 * 1024,
            vms_mb: 2048,
            growth_rate_mb_per_min: 0.0,
            exceeds_warning_limit: false,
            exceeds_kill_limit: false,
            timestamp: Utc::now(),
            uptime_secs: None,
        };

        assert_eq!(stats.format_growth_rate(), "stable");

        stats.growth_rate_mb_per_min = 5.5;
        assert_eq!(stats.format_growth_rate(), "+5.5 MB/min");

        stats.growth_rate_mb_per_min = -3.2;
        assert_eq!(stats.format_growth_rate(), "-3.2 MB/min");
    }

    #[test]
    fn test_memory_stats_severity() {
        let mut stats = WorkerMemoryStats {
            worker_id: "test".to_string(),
            pid: 123,
            rss_mb: 1024,
            rss_bytes: 1024 * 1024 * 1024,
            vms_mb: 2048,
            growth_rate_mb_per_min: 0.0,
            exceeds_warning_limit: false,
            exceeds_kill_limit: false,
            timestamp: Utc::now(),
            uptime_secs: None,
        };

        assert_eq!(stats.severity(), MemorySeverity::Normal);

        stats.rss_mb = 3000;
        assert_eq!(stats.severity(), MemorySeverity::Elevated);

        stats.exceeds_warning_limit = true;
        assert_eq!(stats.severity(), MemorySeverity::Warning);

        stats.exceeds_kill_limit = true;
        assert_eq!(stats.severity(), MemorySeverity::Critical);
    }

    #[test]
    fn test_memory_monitor_creation() {
        let monitor = MemoryMonitor::with_defaults();
        assert_eq!(monitor.config.warning_limit_mb, 4096);
        assert_eq!(monitor.config.kill_limit_mb, 8192);
    }

    #[test]
    fn test_growth_rate_calculation() {
        let monitor = MemoryMonitor::with_defaults();

        // Empty history
        let history: Vec<MemorySample> = vec![];
        assert_eq!(monitor.calculate_growth_rate(&history), 0.0);

        // Single sample
        let history = vec![MemorySample {
            rss_mb: 1000,
            timestamp: Utc::now(),
        }];
        assert_eq!(monitor.calculate_growth_rate(&history), 0.0);

        // Two samples with growth
        let now = Utc::now();
        let history = vec![
            MemorySample {
                rss_mb: 1000,
                timestamp: now - chrono::Duration::minutes(5),
            },
            MemorySample {
                rss_mb: 1050,
                timestamp: now,
            },
        ];
        let rate = monitor.calculate_growth_rate(&history);
        assert!((rate - 10.0).abs() < 0.1); // 50MB / 5 min = 10 MB/min
    }

    #[test]
    fn test_memory_severity_display() {
        assert_eq!(MemorySeverity::Normal.to_string(), "normal");
        assert_eq!(MemorySeverity::Elevated.to_string(), "elevated");
        assert_eq!(MemorySeverity::Warning.to_string(), "warning");
        assert_eq!(MemorySeverity::Critical.to_string(), "critical");
    }

    #[test]
    fn test_clear_worker() {
        let mut monitor = MemoryMonitor::with_defaults();

        // Add some fake history
        monitor.sample_history.insert(
            "worker-1".to_string(),
            vec![MemorySample {
                rss_mb: 1000,
                timestamp: Utc::now(),
            }],
        );
        monitor.latest_stats.insert(
            "worker-1".to_string(),
            WorkerMemoryStats {
                worker_id: "worker-1".to_string(),
                pid: 123,
                rss_mb: 1000,
                rss_bytes: 1000 * 1024 * 1024,
                vms_mb: 2000,
                growth_rate_mb_per_min: 0.0,
                exceeds_warning_limit: false,
                exceeds_kill_limit: false,
                timestamp: Utc::now(),
                uptime_secs: None,
            },
        );

        assert!(monitor.sample_history.contains_key("worker-1"));
        assert!(monitor.latest_stats.contains_key("worker-1"));

        monitor.clear_worker("worker-1");

        assert!(!monitor.sample_history.contains_key("worker-1"));
        assert!(!monitor.latest_stats.contains_key("worker-1"));
    }

    #[test]
    fn test_get_runaway_workers() {
        let mut monitor = MemoryMonitor::with_defaults();

        monitor.latest_stats.insert(
            "worker-normal".to_string(),
            WorkerMemoryStats {
                worker_id: "worker-normal".to_string(),
                pid: 1,
                rss_mb: 1000,
                rss_bytes: 1000 * 1024 * 1024,
                vms_mb: 2000,
                growth_rate_mb_per_min: 0.0,
                exceeds_warning_limit: false,
                exceeds_kill_limit: false,
                timestamp: Utc::now(),
                uptime_secs: None,
            },
        );

        monitor.latest_stats.insert(
            "worker-warning".to_string(),
            WorkerMemoryStats {
                worker_id: "worker-warning".to_string(),
                pid: 2,
                rss_mb: 5000,
                rss_bytes: 5000 * 1024 * 1024,
                vms_mb: 6000,
                growth_rate_mb_per_min: 5.0,
                exceeds_warning_limit: true,
                exceeds_kill_limit: false,
                timestamp: Utc::now(),
                uptime_secs: None,
            },
        );

        monitor.latest_stats.insert(
            "worker-runaway".to_string(),
            WorkerMemoryStats {
                worker_id: "worker-runaway".to_string(),
                pid: 3,
                rss_mb: 9000,
                rss_bytes: 9000 * 1024 * 1024,
                vms_mb: 10000,
                growth_rate_mb_per_min: 20.0,
                exceeds_warning_limit: true,
                exceeds_kill_limit: true,
                timestamp: Utc::now(),
                uptime_secs: None,
            },
        );

        let runaway = monitor.get_runaway_workers();
        assert_eq!(runaway.len(), 1);
        assert_eq!(runaway[0].worker_id, "worker-runaway");

        let warning = monitor.get_warning_workers();
        assert_eq!(warning.len(), 1);
        assert_eq!(warning[0].worker_id, "worker-warning");
    }
}
