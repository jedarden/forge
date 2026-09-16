//! Rate limiting for chat commands.

use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::config::RateLimitConfig;
use crate::error::{ChatError, Result};

/// Sliding window length for the per-minute limit.
const MINUTE_WINDOW: Duration = Duration::from_secs(60);

/// Sliding window length for the per-hour limit.
const HOUR_WINDOW: Duration = Duration::from_secs(3600);

/// Drop entries that have slid out of the window.
fn prune(window: &mut VecDeque<Instant>, now: Instant, window_len: Duration) {
    while let Some(front) = window.front() {
        if now.duration_since(*front) > window_len {
            window.pop_front();
        } else {
            break;
        }
    }
}

/// Build the error for a full window: the limit and the whole seconds until
/// the oldest entry slides out (rounded up, minimum 1).
fn limit_error(
    oldest: Option<Instant>,
    now: Instant,
    window_len: Duration,
    limit: u32,
) -> ChatError {
    let wait = oldest
        .map(|oldest| {
            window_len
                .checked_sub(now.duration_since(oldest))
                .unwrap_or(Duration::ZERO)
        })
        .unwrap_or(window_len);
    ChatError::RateLimitExceeded(limit, wait.as_secs() + 1)
}

/// Rate limiter for chat commands.
///
/// Uses a sliding window algorithm to enforce rate limits. A window limit of
/// `0` disables that window entirely (no check, no bookkeeping).
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
        self.check_at(Instant::now()).await
    }

    /// Check the rate limit at a specific instant.
    async fn check_at(&self, now: Instant) -> Result<()> {
        {
            let mut window = self.minute_window.lock().await;

            // Remove entries older than 1 minute
            prune(&mut window, now, MINUTE_WINDOW);

            if self.minute_limit_active() && window.len() >= self.config.max_per_minute as usize {
                return Err(limit_error(
                    window.front().copied(),
                    now,
                    MINUTE_WINDOW,
                    self.config.max_per_minute,
                ));
            }
        }

        // Check per-hour limit
        {
            let mut window = self.hour_window.lock().await;

            // Remove entries older than 1 hour
            prune(&mut window, now, HOUR_WINDOW);

            if self.hour_limit_active() && window.len() >= self.config.max_per_hour as usize {
                return Err(limit_error(
                    window.front().copied(),
                    now,
                    HOUR_WINDOW,
                    self.config.max_per_hour,
                ));
            }
        }

        Ok(())
    }

    /// Atomically check the rate limit and, if allowed, record the command.
    ///
    /// Unlike calling [`check`] followed by [`record`], both windows are
    /// checked and updated under their locks, so concurrent callers cannot all
    /// pass the check before any of them records.
    pub async fn check_and_record(&self) -> Result<()> {
        self.check_and_record_at(Instant::now()).await
    }

    /// Atomically check and record at a specific instant.
    ///
    /// Locks are always taken minute-then-hour and no other method holds both,
    /// so this cannot deadlock with [`check`], [`record`] or [`usage`].
    async fn check_and_record_at(&self, now: Instant) -> Result<()> {
        let mut minute = self.minute_window.lock().await;
        let mut hour = self.hour_window.lock().await;

        prune(&mut minute, now, MINUTE_WINDOW);
        prune(&mut hour, now, HOUR_WINDOW);

        if self.minute_limit_active() && minute.len() >= self.config.max_per_minute as usize {
            return Err(limit_error(
                minute.front().copied(),
                now,
                MINUTE_WINDOW,
                self.config.max_per_minute,
            ));
        }
        if self.hour_limit_active() && hour.len() >= self.config.max_per_hour as usize {
            return Err(limit_error(
                hour.front().copied(),
                now,
                HOUR_WINDOW,
                self.config.max_per_hour,
            ));
        }

        self.record_into(&mut minute, &mut hour, now);
        Ok(())
    }

    /// Record a command execution.
    ///
    /// Call this after successfully processing a command.
    pub async fn record(&self) {
        self.record_at(Instant::now()).await
    }

    /// Record a command at a specific instant.
    ///
    /// Disabled windows (limit 0) are not written to, so they stay empty
    /// instead of growing without bound.
    async fn record_at(&self, now: Instant) {
        let mut minute = self.minute_window.lock().await;
        let mut hour = self.hour_window.lock().await;
        self.record_into(&mut minute, &mut hour, now);
    }

    /// Push `now` into both windows (caller holds the locks).
    fn record_into(
        &self,
        minute: &mut VecDeque<Instant>,
        hour: &mut VecDeque<Instant>,
        now: Instant,
    ) {
        if self.minute_limit_active() {
            minute.push_back(now);
        }
        if self.hour_limit_active() {
            hour.push_back(now);
        }
    }

    /// Whether the per-minute limit is enabled (0 disables it).
    fn minute_limit_active(&self) -> bool {
        self.config.max_per_minute != 0
    }

    /// Whether the per-hour limit is enabled (0 disables it).
    fn hour_limit_active(&self) -> bool {
        self.config.max_per_hour != 0
    }

    /// Get current usage statistics.
    pub async fn usage(&self) -> RateLimitUsage {
        let now = Instant::now();

        let minute_count = {
            let window = self.minute_window.lock().await;
            window
                .iter()
                .filter(|t| now.duration_since(**t) <= MINUTE_WINDOW)
                .count() as u32
        };

        let hour_count = {
            let window = self.hour_window.lock().await;
            window
                .iter()
                .filter(|t| now.duration_since(**t) <= HOUR_WINDOW)
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

    /// Timestamp `secs` before `base`, for seeding windows at controlled ages.
    fn ago(base: Instant, secs: u64) -> Instant {
        base.checked_sub(Duration::from_secs(secs)).unwrap()
    }

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

    // --- Window edge cases (deterministic via injected timestamps) ---

    #[tokio::test]
    async fn test_entry_expires_after_window() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_per_minute: 2,
            max_per_hour: 100,
        });
        let base = Instant::now();

        // Fill the minute window
        limiter.record_at(base).await;
        limiter.record_at(base).await;
        assert!(limiter.check_at(base).await.is_err());

        // Past the window, both entries have slid out — allowed again
        assert!(
            limiter
                .check_at(base + MINUTE_WINDOW + Duration::from_secs(1))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_entry_at_exact_boundary_is_still_counted() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_per_minute: 1,
            max_per_hour: 100,
        });
        let base = Instant::now();

        // Pruning drops entries strictly older than the window, so an entry
        // exactly 60s old is still inside it and still blocks.
        limiter.record_at(ago(base, 60)).await;
        assert!(limiter.check_at(base).await.is_err());

        // One microsecond-multiple later it has slid out
        assert!(
            limiter
                .check_at(base + Duration::from_nanos(1))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_window_slides_as_entries_expire() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_per_minute: 3,
            max_per_hour: 100,
        });
        let base = Instant::now();

        // Three commands staggered across the first 20 seconds
        limiter.record_at(base).await;
        limiter.record_at(base + Duration::from_secs(10)).await;
        limiter.record_at(base + Duration::from_secs(20)).await;

        // At t+30s all three are still inside the window — blocked, and the
        // wait is measured until the oldest entry slides out
        let t30 = base + Duration::from_secs(30);
        match limiter.check_at(t30).await {
            Err(ChatError::RateLimitExceeded(limit, wait)) => {
                assert_eq!(limit, 3);
                assert_eq!(wait, MINUTE_WINDOW.as_secs() - 30 + 1);
            }
            other => panic!("Expected RateLimitExceeded at t+30s, got {:?}", other),
        }

        // At t+61s the oldest entry has slid out — one slot free
        assert!(
            limiter
                .check_at(base + MINUTE_WINDOW + Duration::from_secs(1))
                .await
                .is_ok()
        );
        limiter
            .record_at(base + MINUTE_WINDOW + Duration::from_secs(1))
            .await;

        // Immediately after taking that slot the window is full again
        assert!(
            limiter
                .check_at(base + MINUTE_WINDOW + Duration::from_secs(1))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_wait_counts_down_to_oldest_expiry() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_per_minute: 1,
            max_per_hour: 100,
        });
        let base = Instant::now();

        limiter.record_at(base).await;

        // 50s in, the sole entry needs 10 more seconds; the wait rounds up
        match limiter.check_at(base + Duration::from_secs(50)).await {
            Err(ChatError::RateLimitExceeded(_, wait)) => assert_eq!(wait, 11),
            other => panic!("Expected RateLimitExceeded, got {:?}", other),
        }

        // Just past the boundary the command is allowed with no wait
        assert!(limiter.check_at(base + MINUTE_WINDOW).await.is_err());
        assert!(
            limiter
                .check_at(base + MINUTE_WINDOW + Duration::from_secs(1))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_hour_window_blocks_when_minute_is_clear() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_per_minute: 10,
            max_per_hour: 3,
        });
        let base = Instant::now();
        let minute = Duration::from_secs(60);

        // Three commands, each over a minute apart: never more than one in
        // the minute window, but the hour window fills up
        limiter.record_at(base).await;
        limiter.record_at(base + 2 * minute).await;
        limiter.record_at(base + 4 * minute).await;

        // Minute window holds only the last command; hour window is full
        let t = base + 4 * minute;
        match limiter.check_at(t).await {
            Err(ChatError::RateLimitExceeded(limit, wait)) => {
                assert_eq!(limit, 3);
                // The hour limit is reached, so the wait runs until the first
                // entry (at `base`) leaves the hour window, not the minute one
                assert_eq!(wait, HOUR_WINDOW.as_secs() - 4 * 60 + 1);
            }
            other => panic!(
                "Expected RateLimitExceeded from hour window, got {:?}",
                other
            ),
        }

        // The minute window is not the constraint: even at t+300s, with only
        // one entry left in it, the hour window is still full
        assert!(limiter.check_at(base + 5 * minute).await.is_err());

        // Past the hour, everything has slid out of both windows
        assert!(
            limiter
                .check_at(base + HOUR_WINDOW + Duration::from_secs(1))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_zero_limit_disables_window() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_per_minute: 0,
            max_per_hour: 100,
        });

        // A disabled window never blocks and never accumulates entries
        for _ in 0..25 {
            assert!(limiter.check_and_record().await.is_ok());
        }
        let usage = limiter.usage().await;
        assert_eq!(usage.commands_last_minute, 0);
        assert_eq!(usage.commands_last_hour, 25);
    }

    #[tokio::test]
    async fn test_check_and_record_blocks_at_limit() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_per_minute: 2,
            max_per_hour: 100,
        });

        assert!(limiter.check_and_record().await.is_ok());
        assert!(limiter.check_and_record().await.is_ok());
        assert!(limiter.check_and_record().await.is_err());

        // The rejected command must not consume window capacity
        let usage = limiter.usage().await;
        assert_eq!(usage.commands_last_minute, 2);
    }
}
