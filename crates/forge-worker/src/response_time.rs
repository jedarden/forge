//! Response time tracking for workers via ping/pong protocol.
//!
//! This module implements a ping/pong protocol to test worker responsiveness.
//! It sends a ping command to the tmux session and expects a pong response
//! within a configurable timeout (default 5 seconds).
//!
//! ## Protocol
//!
//! 1. Send ping: Echo a unique marker to the tmux session
//! 2. Wait for pong: Capture pane output and look for the marker
//! 3. Timeout: If no response within timeout, mark worker as unresponsive
//!
//! ## Usage
//!
//! ```no_run
//! use forge_worker::response_time::{ResponseTimeTracker, PingResult};
//!
//! #[tokio::main]
//! async fn main() -> forge_core::Result<()> {
//!     let mut tracker = ResponseTimeTracker::new();
//!
//!     // Ping a worker's tmux session
//!     let result = tracker.ping("forge-worker-1").await?;
//!
//!     match result {
//!         PingResult::Responsive { response_time_ms } => {
//!             println!("Worker responded in {}ms", response_time_ms);
//!         }
//!         PingResult::Unresponsive { timeout_ms } => {
//!             println!("Worker did not respond within {}ms", timeout_ms);
//!         }
//!         PingResult::SessionNotFound => {
//!             println!("Tmux session not found");
//!         }
//!         PingResult::Error { message } => {
//!             println!("Error during ping: {}", message);
//!         }
//!     }
//!
//!     Ok(())
//! }
//! ```

use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::time::timeout;
use tracing::{debug, info, instrument, warn};
use uuid::Uuid;

use crate::tmux;
use forge_core::Result;

/// Default ping timeout in milliseconds (5 seconds).
pub const DEFAULT_PING_TIMEOUT_MS: u64 = 5000;

/// Default interval between ping checks in seconds.
pub const DEFAULT_PING_INTERVAL_SECS: u64 = 30;

/// Number of consecutive failures before marking worker as unresponsive.
pub const DEFAULT_FAILURE_THRESHOLD: u8 = 2;

/// Configuration for response time tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseTimeConfig {
    /// Timeout for ping response in milliseconds.
    pub ping_timeout_ms: u64,

    /// Interval between automatic ping checks in seconds.
    pub ping_interval_secs: u64,

    /// Number of consecutive failures before marking as unresponsive.
    pub failure_threshold: u8,

    /// Whether to enable automatic ping checks.
    pub enable_auto_ping: bool,
}

impl Default for ResponseTimeConfig {
    fn default() -> Self {
        Self {
            ping_timeout_ms: DEFAULT_PING_TIMEOUT_MS,
            ping_interval_secs: DEFAULT_PING_INTERVAL_SECS,
            failure_threshold: DEFAULT_FAILURE_THRESHOLD,
            enable_auto_ping: false, // Disabled by default
        }
    }
}

/// Result of a ping operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PingResult {
    /// Worker responded within timeout.
    Responsive {
        /// Response time in milliseconds.
        response_time_ms: u64,
    },
    /// Worker did not respond within timeout.
    Unresponsive {
        /// Timeout that was exceeded in milliseconds.
        timeout_ms: u64,
    },
    /// Tmux session was not found.
    SessionNotFound,
    /// Error occurred during ping.
    Error {
        /// Error message.
        message: String,
    },
}

impl PingResult {
    /// Check if the result indicates the worker is responsive.
    pub fn is_responsive(&self) -> bool {
        matches!(self, PingResult::Responsive { .. })
    }

    /// Get the response time if responsive.
    pub fn response_time_ms(&self) -> Option<u64> {
        match self {
            PingResult::Responsive { response_time_ms } => Some(*response_time_ms),
            _ => None,
        }
    }
}

/// State tracking for a single worker's responsiveness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResponseState {
    /// Worker/session identifier.
    pub session_name: String,

    /// Whether the worker is currently considered responsive.
    pub is_responsive: bool,

    /// Last successful response time in milliseconds.
    pub last_response_time_ms: Option<u64>,

    /// Last ping timestamp.
    pub last_ping_at: Option<DateTime<Utc>>,

    /// Last successful ping timestamp.
    pub last_success_at: Option<DateTime<Utc>>,

    /// Number of consecutive ping failures.
    pub consecutive_failures: u8,

    /// Total number of pings sent.
    pub total_pings: u64,

    /// Total number of successful pings.
    pub successful_pings: u64,

    /// Average response time in milliseconds (rolling).
    pub avg_response_time_ms: Option<f64>,
}

impl WorkerResponseState {
    /// Create a new response state for a worker.
    pub fn new(session_name: impl Into<String>) -> Self {
        Self {
            session_name: session_name.into(),
            is_responsive: true, // Assume responsive until proven otherwise
            last_response_time_ms: None,
            last_ping_at: None,
            last_success_at: None,
            consecutive_failures: 0,
            total_pings: 0,
            successful_pings: 0,
            avg_response_time_ms: None,
        }
    }

    /// Record a successful ping response.
    pub fn record_success(&mut self, response_time_ms: u64) {
        let now = Utc::now();
        self.is_responsive = true;
        self.last_response_time_ms = Some(response_time_ms);
        self.last_ping_at = Some(now);
        self.last_success_at = Some(now);
        self.consecutive_failures = 0;
        self.total_pings += 1;
        self.successful_pings += 1;

        // Update rolling average
        self.avg_response_time_ms = Some(match self.avg_response_time_ms {
            Some(avg) => {
                // Exponential moving average with alpha = 0.3
                avg * 0.7 + response_time_ms as f64 * 0.3
            }
            None => response_time_ms as f64,
        });
    }

    /// Record a failed ping attempt.
    pub fn record_failure(&mut self, failure_threshold: u8) {
        self.last_ping_at = Some(Utc::now());
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.total_pings += 1;

        // Mark as unresponsive after exceeding failure threshold
        if self.consecutive_failures >= failure_threshold {
            self.is_responsive = false;
        }
    }

    /// Get success rate as a percentage (0.0 - 100.0).
    pub fn success_rate(&self) -> f64 {
        if self.total_pings == 0 {
            100.0 // No pings yet, assume healthy
        } else {
            (self.successful_pings as f64 / self.total_pings as f64) * 100.0
        }
    }
}

/// Tracker for worker response times via ping/pong protocol.
#[derive(Debug)]
pub struct ResponseTimeTracker {
    /// Configuration.
    config: ResponseTimeConfig,

    /// Response state per worker session.
    states: HashMap<String, WorkerResponseState>,
}

impl ResponseTimeTracker {
    /// Create a new response time tracker with default configuration.
    pub fn new() -> Self {
        Self::with_config(ResponseTimeConfig::default())
    }

    /// Create a new response time tracker with custom configuration.
    pub fn with_config(config: ResponseTimeConfig) -> Self {
        Self {
            config,
            states: HashMap::new(),
        }
    }

    /// Ping a worker's tmux session and measure response time.
    ///
    /// This sends a unique marker to the session using `tmux send-keys`,
    /// then repeatedly captures the pane output looking for the marker.
    #[instrument(level = "debug", skip(self), fields(session = %session_name))]
    pub async fn ping(&mut self, session_name: &str) -> Result<PingResult> {
        // Check if session exists first
        if !tmux::session_exists(session_name).await? {
            debug!("Session {} not found", session_name);
            return Ok(PingResult::SessionNotFound);
        }

        // Generate a unique ping marker
        let ping_id = Uuid::new_v4().to_string()[..8].to_string();
        let ping_marker = format!("FORGE_PING_{}", ping_id);
        let pong_marker = format!("FORGE_PONG_{}", ping_id);

        // Send the ping command - echo a marker that will appear in output
        // We use a simple echo command that outputs a pong marker
        let ping_command = format!("echo '{}'", pong_marker);

        let start = Instant::now();

        // Send the ping
        if let Err(e) = tmux::send_command(session_name, &ping_command).await {
            warn!("Failed to send ping to {}: {}", session_name, e);
            self.record_failure(session_name);
            return Ok(PingResult::Error {
                message: format!("Failed to send ping: {}", e),
            });
        }

        debug!("Sent ping {} to session {}", ping_marker, session_name);

        // Wait for pong with timeout
        let timeout_duration = Duration::from_millis(self.config.ping_timeout_ms);

        let result = timeout(timeout_duration, async {
            self.wait_for_pong(session_name, &pong_marker, start).await
        })
        .await;

        match result {
            Ok(Ok(response_time_ms)) => {
                info!(
                    "Worker {} responded in {}ms",
                    session_name, response_time_ms
                );
                self.record_success(session_name, response_time_ms);
                Ok(PingResult::Responsive { response_time_ms })
            }
            Ok(Err(e)) => {
                warn!("Error waiting for pong from {}: {}", session_name, e);
                self.record_failure(session_name);
                Ok(PingResult::Error {
                    message: e.to_string(),
                })
            }
            Err(_) => {
                // Timeout
                warn!(
                    "Worker {} did not respond within {}ms",
                    session_name, self.config.ping_timeout_ms
                );
                self.record_failure(session_name);
                Ok(PingResult::Unresponsive {
                    timeout_ms: self.config.ping_timeout_ms,
                })
            }
        }
    }

    /// Wait for a pong response by polling the tmux pane output.
    async fn wait_for_pong(
        &self,
        session_name: &str,
        pong_marker: &str,
        start: Instant,
    ) -> Result<u64> {
        // Poll interval - start fast, then slow down
        let mut poll_interval_ms = 50;
        let max_poll_interval_ms = 200;

        loop {
            // Capture recent pane output
            let output = tmux::capture_pane(session_name, Some(20)).await?;

            // Check if pong marker is present
            if output.contains(pong_marker) {
                let elapsed = start.elapsed();
                return Ok(elapsed.as_millis() as u64);
            }

            // Wait before next poll
            tokio::time::sleep(Duration::from_millis(poll_interval_ms)).await;

            // Gradually increase poll interval
            poll_interval_ms = (poll_interval_ms * 3 / 2).min(max_poll_interval_ms);
        }
    }

    /// Record a successful ping response for a worker.
    fn record_success(&mut self, session_name: &str, response_time_ms: u64) {
        let state = self
            .states
            .entry(session_name.to_string())
            .or_insert_with(|| WorkerResponseState::new(session_name));
        state.record_success(response_time_ms);
    }

    /// Record a failed ping attempt for a worker.
    fn record_failure(&mut self, session_name: &str) {
        let state = self
            .states
            .entry(session_name.to_string())
            .or_insert_with(|| WorkerResponseState::new(session_name));
        state.record_failure(self.config.failure_threshold);
    }

    /// Get the response state for a worker.
    pub fn get_state(&self, session_name: &str) -> Option<&WorkerResponseState> {
        self.states.get(session_name)
    }

    /// Get all worker response states.
    pub fn all_states(&self) -> &HashMap<String, WorkerResponseState> {
        &self.states
    }

    /// Check if a worker is currently considered responsive.
    pub fn is_responsive(&self, session_name: &str) -> bool {
        self.states
            .get(session_name)
            .map(|s| s.is_responsive)
            .unwrap_or(true) // Assume responsive if no state exists
    }

    /// Get list of unresponsive workers.
    pub fn unresponsive_workers(&self) -> Vec<&str> {
        self.states
            .iter()
            .filter(|(_, state)| !state.is_responsive)
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Ping all workers and return results.
    #[instrument(level = "debug", skip(self))]
    pub async fn ping_all(&mut self, session_names: &[String]) -> HashMap<String, PingResult> {
        let mut results = HashMap::new();

        for session_name in session_names {
            match self.ping(session_name).await {
                Ok(result) => {
                    results.insert(session_name.clone(), result);
                }
                Err(e) => {
                    results.insert(
                        session_name.clone(),
                        PingResult::Error {
                            message: e.to_string(),
                        },
                    );
                }
            }
        }

        results
    }

    /// Reset the state for a worker (e.g., after restart).
    pub fn reset_state(&mut self, session_name: &str) {
        self.states.remove(session_name);
    }

    /// Clear all states.
    pub fn clear_states(&mut self) {
        self.states.clear();
    }

    /// Get the configuration.
    pub fn config(&self) -> &ResponseTimeConfig {
        &self.config
    }
}

impl Default for ResponseTimeTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_time_config_default() {
        let config = ResponseTimeConfig::default();
        assert_eq!(config.ping_timeout_ms, 5000);
        assert_eq!(config.ping_interval_secs, 30);
        assert_eq!(config.failure_threshold, 2);
        assert!(!config.enable_auto_ping);
    }

    #[test]
    fn test_ping_result_is_responsive() {
        assert!(PingResult::Responsive {
            response_time_ms: 100
        }
        .is_responsive());
        assert!(!PingResult::Unresponsive { timeout_ms: 5000 }.is_responsive());
        assert!(!PingResult::SessionNotFound.is_responsive());
        assert!(!PingResult::Error {
            message: "test".to_string()
        }
        .is_responsive());
    }

    #[test]
    fn test_ping_result_response_time() {
        assert_eq!(
            PingResult::Responsive {
                response_time_ms: 123
            }
            .response_time_ms(),
            Some(123)
        );
        assert_eq!(
            PingResult::Unresponsive { timeout_ms: 5000 }.response_time_ms(),
            None
        );
    }

    #[test]
    fn test_worker_response_state_new() {
        let state = WorkerResponseState::new("test-session");
        assert_eq!(state.session_name, "test-session");
        assert!(state.is_responsive);
        assert_eq!(state.consecutive_failures, 0);
        assert_eq!(state.total_pings, 0);
        assert_eq!(state.successful_pings, 0);
    }

    #[test]
    fn test_worker_response_state_record_success() {
        let mut state = WorkerResponseState::new("test-session");

        state.record_success(100);
        assert!(state.is_responsive);
        assert_eq!(state.last_response_time_ms, Some(100));
        assert_eq!(state.consecutive_failures, 0);
        assert_eq!(state.total_pings, 1);
        assert_eq!(state.successful_pings, 1);
        assert!(state.avg_response_time_ms.is_some());

        // After a failure, success should reset consecutive failures
        state.record_failure(2);
        assert_eq!(state.consecutive_failures, 1);

        state.record_success(150);
        assert_eq!(state.consecutive_failures, 0);
        assert!(state.is_responsive);
    }

    #[test]
    fn test_worker_response_state_record_failure() {
        let mut state = WorkerResponseState::new("test-session");

        // First failure - still responsive
        state.record_failure(2);
        assert!(state.is_responsive);
        assert_eq!(state.consecutive_failures, 1);

        // Second failure - now unresponsive (threshold is 2)
        state.record_failure(2);
        assert!(!state.is_responsive);
        assert_eq!(state.consecutive_failures, 2);
    }

    #[test]
    fn test_worker_response_state_success_rate() {
        let mut state = WorkerResponseState::new("test-session");

        // No pings - 100% (assume healthy)
        assert_eq!(state.success_rate(), 100.0);

        // 2 successes, 0 failures - 100%
        state.record_success(100);
        state.record_success(100);
        assert_eq!(state.success_rate(), 100.0);

        // 2 successes, 2 failures - 50%
        state.record_failure(10);
        state.record_failure(10);
        assert_eq!(state.success_rate(), 50.0);
    }

    #[test]
    fn test_response_time_tracker_new() {
        let tracker = ResponseTimeTracker::new();
        assert!(tracker.states.is_empty());
        assert_eq!(tracker.config.ping_timeout_ms, 5000);
    }

    #[test]
    fn test_response_time_tracker_is_responsive() {
        let mut tracker = ResponseTimeTracker::new();

        // Unknown session - assume responsive
        assert!(tracker.is_responsive("unknown-session"));

        // After success - responsive
        tracker.record_success("test-session", 100);
        assert!(tracker.is_responsive("test-session"));

        // After enough failures - unresponsive
        tracker.record_failure("test-session");
        tracker.record_failure("test-session");
        assert!(!tracker.is_responsive("test-session"));
    }

    #[test]
    fn test_response_time_tracker_unresponsive_workers() {
        let mut tracker = ResponseTimeTracker::new();

        // No unresponsive workers initially
        assert!(tracker.unresponsive_workers().is_empty());

        // Add some workers
        tracker.record_success("worker-1", 100);
        tracker.record_failure("worker-2");
        tracker.record_failure("worker-2"); // Now unresponsive

        let unresponsive = tracker.unresponsive_workers();
        assert_eq!(unresponsive.len(), 1);
        assert!(unresponsive.contains(&"worker-2"));
    }

    #[test]
    fn test_response_time_tracker_reset_state() {
        let mut tracker = ResponseTimeTracker::new();

        tracker.record_success("test-session", 100);
        assert!(tracker.get_state("test-session").is_some());

        tracker.reset_state("test-session");
        assert!(tracker.get_state("test-session").is_none());
    }

    #[test]
    fn test_response_time_tracker_clear_states() {
        let mut tracker = ResponseTimeTracker::new();

        tracker.record_success("worker-1", 100);
        tracker.record_success("worker-2", 100);
        assert_eq!(tracker.all_states().len(), 2);

        tracker.clear_states();
        assert!(tracker.all_states().is_empty());
    }

    #[test]
    fn test_avg_response_time_calculation() {
        let mut state = WorkerResponseState::new("test-session");

        // First response - average equals first value
        state.record_success(100);
        assert!((state.avg_response_time_ms.unwrap() - 100.0).abs() < 0.01);

        // Second response - EMA with alpha 0.3
        // new_avg = old_avg * 0.7 + new_value * 0.3
        // new_avg = 100 * 0.7 + 200 * 0.3 = 70 + 60 = 130
        state.record_success(200);
        assert!((state.avg_response_time_ms.unwrap() - 130.0).abs() < 0.01);
    }
}
