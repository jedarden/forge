//! Rate limiting for chat commands.

use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::config::RateLimitConfig;
use crate::error::{ChatError, Result};

/// Rate limiter for chat commands.
///
/// Uses a sliding window algorithm to enforce rate limits.
pub struct RateLimiter {
    /// Configuration
    config: RateLimitConfig,
    /// Timestamps of recent commands (for per-minute limit)
    minute_window: Mutex<VecDeque<Instant>>,
    /// Timestamps of recent commands (for per-hour limit)
    hour_window: Mutex<VecDeque<Instant>>,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            minute_window: Mutex::new(VecDeque::new()),
            hour_window: Mutex::new(VecDeque::new()),
        }
    }

    /// Check if a command is allowed under rate limits.
    ///
    /// Returns Ok(()) if allowed, or Err with time to wait if rate limited.
    pub async fn check(&self) -> Result<()> {
        let now = Instant::now();
        let one_minute = Duration::from_secs(60);
        let one_hour = Duration::from_secs(3600);

        // Check per-minute limit
        {
            let mut window = self.minute_window.lock().await;

            // Remove entries older than 1 minute
            while let Some(front) = window.front() {
                if now.duration_since(*front) > one_minute {
                    window.pop_front();
                } else {
                    break;
                }
            }

            if window.len() >= self.config.max_per_minute as usize {
                // Calculate time until oldest entry expires
                if let Some(oldest) = window.front() {
                    let wait_time = one_minute
                        .checked_sub(now.duration_since(*oldest))
                        .unwrap_or(Duration::ZERO);
                    return Err(ChatError::RateLimitExceeded(
                        self.config.max_per_minute,
                        wait_time.as_secs() + 1,
                    ));
                }
            }
        }

        // Check per-hour limit
        {
            let mut window = self.hour_window.lock().await;

            // Remove entries older than 1 hour
            while let Some(front) = window.front() {
                if now.duration_since(*front) > one_hour {
                    window.pop_front();
                } else {
                    break;
                }
            }

            if window.len() >= self.config.max_per_hour as usize
                && let Some(oldest) = window.front()
            {
                let wait_time = one_hour
                    .checked_sub(now.duration_since(*oldest))
                    .unwrap_or(Duration::ZERO);
                return Err(ChatError::RateLimitExceeded(
                    self.config.max_per_hour,
                    wait_time.as_secs() + 1,
                ));
            }
        }

        Ok(())
    }

    /// Record a command execution.
    ///
    /// Call this after successfully processing a command.
    pub async fn record(&self) {
        let now = Instant::now();

        {
            let mut window = self.minute_window.lock().await;
            window.push_back(now);
        }

        {
            let mut window = self.hour_window.lock().await;
            window.push_back(now);
        }
    }

    /// Get current usage statistics.
    pub async fn usage(&self) -> RateLimitUsage {
        let now = Instant::now();
        let one_minute = Duration::from_secs(60);
        let one_hour = Duration::from_secs(3600);

        let minute_count = {
            let window = self.minute_window.lock().await;
            window
                .iter()
                .filter(|t| now.duration_since(**t) <= one_minute)
                .count() as u32
        };

        let hour_count = {
            let window = self.hour_window.lock().await;
            window
                .iter()
                .filter(|t| now.duration_since(**t) <= one_hour)
                .count() as u32
        };

        RateLimitUsage {
            commands_last_minute: minute_count,
            commands_last_hour: hour_count,
            max_per_minute: self.config.max_per_minute,
            max_per_hour: self.config.max_per_hour,
        }
    }

    /// Reset the rate limiter (clear all windows).
    pub async fn reset(&self) {
        self.minute_window.lock().await.clear();
        self.hour_window.lock().await.clear();
    }
}

/// Rate limit usage statistics.
#[derive(Debug, Clone)]
pub struct RateLimitUsage {
    /// Commands in the last minute
    pub commands_last_minute: u32,
    /// Commands in the last hour
    pub commands_last_hour: u32,
    /// Maximum commands per minute
    pub max_per_minute: u32,
    /// Maximum commands per hour
    pub max_per_hour: u32,
}

impl RateLimitUsage {
    /// Check if near the per-minute limit (>80% used).
    pub fn near_minute_limit(&self) -> bool {
        self.commands_last_minute as f32 / self.max_per_minute as f32 > 0.8
    }

    /// Check if near the per-hour limit (>80% used).
    pub fn near_hour_limit(&self) -> bool {
        self.commands_last_hour as f32 / self.max_per_hour as f32 > 0.8
    }

    /// Remaining commands in the current minute.
    pub fn remaining_minute(&self) -> u32 {
        self.max_per_minute
            .saturating_sub(self.commands_last_minute)
    }

    /// Remaining commands in the current hour.
    pub fn remaining_hour(&self) -> u32 {
        self.max_per_hour.saturating_sub(self.commands_last_hour)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_allows_under_limit() {
        let config = RateLimitConfig {
            max_per_minute: 5,
            max_per_hour: 100,
        };
        let limiter = RateLimiter::new(config);

        // First 5 should be allowed
        for _ in 0..5 {
            assert!(limiter.check().await.is_ok());
            limiter.record().await;
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_over_limit() {
        let config = RateLimitConfig {
            max_per_minute: 3,
            max_per_hour: 100,
        };
        let limiter = RateLimiter::new(config);

        // Use up the limit
        for _ in 0..3 {
            limiter.record().await;
        }

        // Next should be blocked
        let result = limiter.check().await;
        assert!(result.is_err());

        if let Err(ChatError::RateLimitExceeded(limit, _)) = result {
            assert_eq!(limit, 3);
        } else {
            panic!("Expected RateLimitExceeded error");
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_usage() {
        let config = RateLimitConfig {
            max_per_minute: 10,
            max_per_hour: 100,
        };
        let limiter = RateLimiter::new(config);

        limiter.record().await;
        limiter.record().await;

        let usage = limiter.usage().await;
        assert_eq!(usage.commands_last_minute, 2);
        assert_eq!(usage.remaining_minute(), 8);
    }

    #[tokio::test]
    async fn test_rate_limiter_reset() {
        let config = RateLimitConfig {
            max_per_minute: 3,
            max_per_hour: 100,
        };
        let limiter = RateLimiter::new(config);

        // Fill up the limit
        for _ in 0..3 {
            limiter.record().await;
        }

        // Reset should clear
        limiter.reset().await;

        let usage = limiter.usage().await;
        assert_eq!(usage.commands_last_minute, 0);
    }
}
