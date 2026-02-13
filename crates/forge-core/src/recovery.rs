//! Error recovery utilities for FORGE.
//!
//! This module provides retry logic, exponential backoff, and graceful
//! degradation strategies for handling transient errors.
//!
//! ## Features
//!
//! - **Exponential backoff**: Retry operations with increasing delays
//! - **Jitter**: Randomize retry intervals to prevent thundering herd
//! - **Configurable limits**: Control max retries and total timeout
//! - **Error classification**: Determine if errors are retryable
//!
//! ## Example
//!
//! ```no_run
//! use forge_core::recovery::{retry_with_backoff, RetryConfig};
//! use std::time::Duration;
//!
//! let config = RetryConfig {
//!     max_retries: 3,
//!     initial_delay: Duration::from_millis(100),
//!     max_delay: Duration::from_secs(5),
//!     multiplier: 2.0,
//! };
//!
//! let result = retry_with_backoff(config, || {
//!     // Operation that might fail transiently
//!     some_network_call()
//! });
//! ```

use std::future::Future;
use std::thread;
use std::time::Duration;
use tracing::{debug, info, warn};
use rand::Rng;

/// Configuration for retry behavior with exponential backoff.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_retries: u32,
    /// Initial delay before first retry.
    pub initial_delay: Duration,
    /// Maximum delay between retries (caps exponential growth).
    pub max_delay: Duration,
    /// Multiplier for exponential backoff (e.g., 2.0 doubles each time).
    pub multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
        }
    }
}

impl RetryConfig {
    /// Create a config for database operations (aggressive retries).
    pub fn for_database() -> Self {
        Self {
            max_retries: 5,
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(5),
            multiplier: 1.5,
        }
    }

    /// Create a config for network operations (moderate retries).
    pub fn for_network() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
        }
    }

    /// Create a config for API rate limits (patient retries).
    pub fn for_rate_limit() -> Self {
        Self {
            max_retries: 5,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            multiplier: 2.0,
        }
    }

    /// Create a config for worker operations (quick retries).
    pub fn for_worker() -> Self {
        Self {
            max_retries: 2,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
            multiplier: 2.0,
        }
    }

    /// Calculate delay for a given attempt number with jitter.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base_delay = self.initial_delay.as_secs_f64()
            * self.multiplier.powi(attempt as i32);

        let capped_delay = base_delay.min(self.max_delay.as_secs_f64());

        // Add jitter (±25% of delay)
        let jitter_range = capped_delay * 0.25;
        let mut rng = rand::rng();
        let jitter = rng.random_range(-jitter_range..jitter_range);
        let final_delay = (capped_delay + jitter).max(0.0).min(self.max_delay.as_secs_f64());

        Duration::from_secs_f64(final_delay)
    }
}

/// Result of a retry operation with metadata about the attempts.
#[derive(Debug)]
pub struct RetryResult<T> {
    /// The final result (success or last error).
    pub result: T,
    /// Number of attempts made.
    pub attempts: u32,
    /// Total time spent retrying.
    pub total_duration: Duration,
}

/// Retry a synchronous operation with exponential backoff.
///
/// Returns the result of the operation if successful, or the last error
/// if all retries are exhausted.
pub fn retry_with_backoff<T, E, F>(
    config: RetryConfig,
    mut operation: F,
) -> RetryResult<Result<T, E>>
where
    F: FnMut() -> Result<T, E>,
    E: std::fmt::Debug,
{
    let start = std::time::Instant::now();
    let mut attempts = 0;
    let mut last_error: Option<E> = None;

    loop {
        attempts += 1;

        match operation() {
            Ok(value) => {
                if attempts > 1 {
                    info!(
                        attempts,
                        total_ms = start.elapsed().as_millis(),
                        "Operation succeeded after retry"
                    );
                }
                return RetryResult {
                    result: Ok(value),
                    attempts,
                    total_duration: start.elapsed(),
                };
            }
            Err(e) => {
                last_error = Some(e);

                if attempts > config.max_retries {
                    warn!(
                        attempts,
                        max_retries = config.max_retries,
                        total_ms = start.elapsed().as_millis(),
                        "Operation failed after all retries"
                    );
                    return RetryResult {
                        result: Err(last_error.unwrap()),
                        attempts,
                        total_duration: start.elapsed(),
                    };
                }

                let delay = config.delay_for_attempt(attempts - 1);
                debug!(
                    attempt = attempts,
                    delay_ms = delay.as_millis(),
                    "Operation failed, retrying with backoff"
                );

                thread::sleep(delay);
            }
        }
    }
}

/// Retry an async operation with exponential backoff.
///
/// Returns the result of the operation if successful, or the last error
/// if all retries are exhausted.
pub async fn retry_with_backoff_async<T, E, F, Fut>(
    config: RetryConfig,
    mut operation: F,
) -> RetryResult<Result<T, E>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Debug,
{
    let start = std::time::Instant::now();
    let mut attempts = 0;
    let mut last_error: Option<E> = None;

    loop {
        attempts += 1;

        match operation().await {
            Ok(value) => {
                if attempts > 1 {
                    info!(
                        attempts,
                        total_ms = start.elapsed().as_millis(),
                        "Async operation succeeded after retry"
                    );
                }
                return RetryResult {
                    result: Ok(value),
                    attempts,
                    total_duration: start.elapsed(),
                };
            }
            Err(e) => {
                last_error = Some(e);

                if attempts > config.max_retries {
                    warn!(
                        attempts,
                        max_retries = config.max_retries,
                        total_ms = start.elapsed().as_millis(),
                        "Async operation failed after all retries"
                    );
                    return RetryResult {
                        result: Err(last_error.unwrap()),
                        attempts,
                        total_duration: start.elapsed(),
                    };
                }

                let delay = config.delay_for_attempt(attempts - 1);
                debug!(
                    attempt = attempts,
                    delay_ms = delay.as_millis(),
                    "Async operation failed, retrying with backoff"
                );

                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Trait for determining if an error is retryable.
pub trait Retryable {
    /// Returns true if the operation that caused this error should be retried.
    fn is_retryable(&self) -> bool;
}

/// Error recovery action recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Retry the operation immediately.
    RetryNow,
    /// Retry after a delay (backoff).
    RetryWithBackoff,
    /// Fall back to a default value or cached data.
    UseFallback,
    /// Log the error and continue (non-critical).
    LogAndContinue,
    /// Show error to user and wait for input.
    NotifyUser,
    /// The error is fatal, cannot recover.
    Fatal,
}

impl RecoveryAction {
    /// Determine the appropriate recovery action from an error message.
    pub fn from_error_message(error: &str) -> Self {
        let error_lower = error.to_lowercase();

        // Database lock errors - always retry
        if error_lower.contains("database is locked")
            || error_lower.contains("sqlalchemy.exc.operationalerror")
            || error_lower.contains("sqlite_busy")
            || error_lower.contains("sqlite_locked")
        {
            return Self::RetryWithBackoff;
        }

        // Network/timeout errors - retry
        if error_lower.contains("timeout")
            || error_lower.contains("connection refused")
            || error_lower.contains("connection reset")
            || error_lower.contains("network")
            || error_lower.contains("broken pipe")
        {
            return Self::RetryWithBackoff;
        }

        // Rate limiting - wait and retry
        if error_lower.contains("rate limit")
            || error_lower.contains("too many requests")
            || error_lower.contains("429")
        {
            return Self::RetryWithBackoff;
        }

        // Server errors - retry (might be temporary)
        if error_lower.contains("500")
            || error_lower.contains("502")
            || error_lower.contains("503")
            || error_lower.contains("504")
            || error_lower.contains("internal server error")
            || error_lower.contains("bad gateway")
            || error_lower.contains("service unavailable")
            || error_lower.contains("gateway timeout")
        {
            return Self::RetryWithBackoff;
        }

        // Config errors - use defaults
        if error_lower.contains("config")
            && (error_lower.contains("not found") || error_lower.contains("missing"))
        {
            return Self::UseFallback;
        }

        // Invalid config syntax - notify user
        if error_lower.contains("parse error")
            || error_lower.contains("invalid yaml")
            || error_lower.contains("invalid json")
            || error_lower.contains("syntax error")
        {
            return Self::NotifyUser;
        }

        // Permission errors - notify user
        if error_lower.contains("permission denied")
            || error_lower.contains("access denied")
            || error_lower.contains("forbidden")
            || error_lower.contains("unauthorized")
        {
            return Self::NotifyUser;
        }

        // Not found errors - log and continue
        if error_lower.contains("not found") || error_lower.contains("does not exist") {
            return Self::LogAndContinue;
        }

        // Default: notify user
        Self::NotifyUser
    }
}

/// User-friendly error message generator.
pub fn friendly_error_message(error: &str) -> String {
    let error_lower = error.to_lowercase();

    // Database errors
    if error_lower.contains("database is locked") {
        return "The database is busy. FORGE will automatically retry shortly.".to_string();
    }

    // Network errors
    if error_lower.contains("timeout") || error_lower.contains("connection") {
        return "Network connection issue. Check your internet and try again.".to_string();
    }

    // Rate limiting
    if error_lower.contains("rate limit") || error_lower.contains("429") {
        return "Too many requests. Please wait a moment before trying again.".to_string();
    }

    // Config errors
    if error_lower.contains("config") && error_lower.contains("not found") {
        return "Configuration file not found. Using default settings.".to_string();
    }

    // Auth errors
    if error_lower.contains("unauthorized") || error_lower.contains("api key") {
        return "Authentication failed. Check your API key configuration.".to_string();
    }

    // Permission errors
    if error_lower.contains("permission denied") {
        return "Permission denied. Check file permissions.".to_string();
    }

    // Generic fallback
    format!("An error occurred: {}", error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_config_delay_calculation() {
        let config = RetryConfig::default();

        // First attempt should be close to initial delay
        let d1 = config.delay_for_attempt(0);
        assert!(d1.as_millis() >= 50 && d1.as_millis() <= 200);

        // Second attempt should be roughly double
        let d2 = config.delay_for_attempt(1);
        assert!(d2.as_millis() >= 150 && d2.as_millis() <= 500);

        // High attempts should cap at max_delay (with small epsilon for floating point)
        let d10 = config.delay_for_attempt(100);
        assert!(d10 <= config.max_delay + Duration::from_millis(1));
    }

    #[test]
    fn test_retry_success_first_try() {
        let config = RetryConfig::default();
        let mut call_count = 0;

        let result = retry_with_backoff(config, || {
            call_count += 1;
            Ok::<i32, &str>(42)
        });

        assert_eq!(result.result, Ok(42));
        assert_eq!(result.attempts, 1);
        assert_eq!(call_count, 1);
    }

    #[test]
    fn test_retry_success_after_failure() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
            multiplier: 1.0,
        };
        let mut call_count = 0;

        let result = retry_with_backoff(config, || {
            call_count += 1;
            if call_count < 3 {
                Err("temporary error")
            } else {
                Ok::<i32, &str>(42)
            }
        });

        assert_eq!(result.result, Ok(42));
        assert_eq!(result.attempts, 3);
    }

    #[test]
    fn test_retry_exhausted() {
        let config = RetryConfig {
            max_retries: 2,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
            multiplier: 1.0,
        };
        let mut call_count = 0;

        let result = retry_with_backoff(config, || {
            call_count += 1;
            Err::<i32, &str>("persistent error")
        });

        assert!(result.result.is_err());
        assert_eq!(result.attempts, 3); // 1 initial + 2 retries
    }

    #[test]
    fn test_recovery_action_database_lock() {
        assert_eq!(
            RecoveryAction::from_error_message("database is locked"),
            RecoveryAction::RetryWithBackoff
        );
        assert_eq!(
            RecoveryAction::from_error_message("SQLITE_BUSY: database is locked"),
            RecoveryAction::RetryWithBackoff
        );
    }

    #[test]
    fn test_recovery_action_network() {
        assert_eq!(
            RecoveryAction::from_error_message("Connection timeout"),
            RecoveryAction::RetryWithBackoff
        );
        assert_eq!(
            RecoveryAction::from_error_message("503 Service Unavailable"),
            RecoveryAction::RetryWithBackoff
        );
    }

    #[test]
    fn test_recovery_action_rate_limit() {
        assert_eq!(
            RecoveryAction::from_error_message("429 Too Many Requests"),
            RecoveryAction::RetryWithBackoff
        );
    }

    #[test]
    fn test_recovery_action_config() {
        assert_eq!(
            RecoveryAction::from_error_message("Config file not found"),
            RecoveryAction::UseFallback
        );
        assert_eq!(
            RecoveryAction::from_error_message("Invalid YAML syntax"),
            RecoveryAction::NotifyUser
        );
    }

    #[test]
    fn test_friendly_error_messages() {
        assert!(friendly_error_message("database is locked").contains("automatically retry"));
        assert!(friendly_error_message("Connection timeout").contains("Network"));
        assert!(friendly_error_message("Config file not found").contains("default settings"));
    }

    #[test]
    fn test_database_config() {
        let config = RetryConfig::for_database();
        assert_eq!(config.max_retries, 5);
        assert!(config.initial_delay < Duration::from_millis(100));
    }

    #[test]
    fn test_network_config() {
        let config = RetryConfig::for_network();
        assert_eq!(config.max_retries, 3);
        assert!(config.initial_delay >= Duration::from_millis(100));
    }
}
