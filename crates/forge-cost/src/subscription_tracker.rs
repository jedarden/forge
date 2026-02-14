//! Subscription tracking and management.
//!
//! This module provides subscription tracking functionality including:
//! - Loading subscription configurations from YAML
//! - Tracking usage against quotas
//! - Calculating days until renewal
//! - Generating alerts when quota is high or renewal is approaching

use chrono::{DateTime, Datelike, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info, warn};

use crate::db::CostDatabase;
use crate::error::{CostError, Result};
use crate::models::{Subscription, SubscriptionSummary, SubscriptionType};

/// Configuration for a subscription (loaded from YAML).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionConfig {
    /// Provider name (e.g., "anthropic", "openai")
    pub provider: String,
    /// Plan name (e.g., "max_5x", "plus")
    pub plan: String,
    /// Monthly token quota (for token-based plans)
    #[serde(default)]
    pub monthly_tokens: Option<i64>,
    /// Monthly cost in USD
    #[serde(default)]
    pub monthly_cost: Option<f64>,
    /// Renewal date (day of month or ISO date)
    #[serde(default)]
    pub renewal_date: Option<String>,
    /// Current usage (if known)
    #[serde(default)]
    pub current_usage: Option<i64>,
    /// Associated model (if applicable)
    #[serde(default)]
    pub model: Option<String>,
    /// Subscription type (fixed_quota, unlimited, pay_per_use)
    #[serde(default = "default_subscription_type")]
    pub subscription_type: String,
    /// Whether the subscription is active
    #[serde(default = "default_true")]
    pub active: bool,
}

fn default_subscription_type() -> String {
    "fixed_quota".to_string()
}

fn default_true() -> bool {
    true
}

/// Root configuration file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionConfigFile {
    /// List of subscription configurations
    pub subscriptions: Vec<SubscriptionConfig>,
}

/// Alert level for subscription status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubscriptionAlert {
    /// No alert - everything is fine
    None,
    /// Warning - quota usage > 80% or renewal within 3 days
    Warning,
    /// Critical - quota usage > 90% or renewal within 1 day
    Critical,
    /// Depleted - quota exhausted
    Depleted,
}

impl SubscriptionAlert {
    /// Get icon for the alert level.
    pub fn icon(&self) -> &'static str {
        match self {
            SubscriptionAlert::None => "",
            SubscriptionAlert::Warning => "⚠",
            SubscriptionAlert::Critical => "🔴",
            SubscriptionAlert::Depleted => "❌",
        }
    }

    /// Check if this is an alerting state.
    pub fn is_alert(&self) -> bool {
        !matches!(self, SubscriptionAlert::None)
    }
}

/// Subscription tracker that manages multiple subscriptions.
pub struct SubscriptionTracker {
    /// Loaded subscriptions by name
    subscriptions: HashMap<String, Subscription>,
    /// Path to config file
    config_path: Option<std::path::PathBuf>,
    /// Last load time
    last_loaded: Option<DateTime<Utc>>,
    /// Cache of subscription summaries for display
    summary_cache: Vec<SubscriptionSummary>,
}

impl SubscriptionTracker {
    /// Create a new empty subscription tracker.
    pub fn new() -> Self {
        Self {
            subscriptions: HashMap::new(),
            config_path: None,
            last_loaded: None,
            summary_cache: Vec::new(),
        }
    }

    /// Create a tracker with configuration loaded from a file.
    pub fn from_config_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut tracker = Self::new();
        tracker.config_path = Some(path.as_ref().to_path_buf());
        tracker.reload()?;
        Ok(tracker)
    }

    /// Create a tracker with default config path (~/.forge/subscriptions.yaml).
    pub fn with_default_config() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/coder".to_string());
        let config_path = Path::new(&home).join(".forge").join("subscriptions.yaml");

        if config_path.exists() {
            match Self::from_config_file(&config_path) {
                Ok(tracker) => {
                    info!("Loaded {} subscriptions from {}", tracker.len(), config_path.display());
                    return tracker;
                }
                Err(e) => {
                    warn!("Failed to load subscriptions from {}: {}", config_path.display(), e);
                }
            }
        }

        // Return empty tracker if no config found
        let mut tracker = Self::new();
        tracker.config_path = Some(config_path);
        tracker
    }

    /// Reload subscriptions from the config file.
    pub fn reload(&mut self) -> Result<()> {
        let config_path = self.config_path.as_ref().ok_or_else(|| {
            CostError::Config("No config path set".to_string())
        })?;

        if !config_path.exists() {
            debug!("Subscription config file does not exist: {}", config_path.display());
            return Ok(());
        }

        let config_str = std::fs::read_to_string(config_path)?;
        let config: SubscriptionConfigFile = serde_yaml::from_str(&config_str)?;

        self.subscriptions.clear();

        for sub_config in config.subscriptions {
            let subscription = self.config_to_subscription(&sub_config)?;
            let name = subscription.name.clone();
            self.subscriptions.insert(name, subscription);
        }

        self.last_loaded = Some(Utc::now());
        self.update_summary_cache();

        info!("Loaded {} subscriptions from config", self.subscriptions.len());
        Ok(())
    }

    /// Convert a config entry to a Subscription model.
    fn config_to_subscription(&self, config: &SubscriptionConfig) -> Result<Subscription> {
        let now = Utc::now();

        // Parse subscription type
        let subscription_type = match config.subscription_type.as_str() {
            "fixed_quota" | "fixed" => SubscriptionType::FixedQuota,
            "unlimited" => SubscriptionType::Unlimited,
            "pay_per_use" | "payperuse" => SubscriptionType::PayPerUse,
            _ => SubscriptionType::FixedQuota,
        };

        // Parse renewal date
        let (billing_start, billing_end) = self.parse_billing_period(&config.renewal_date, now)?;

        // Build subscription name from provider and plan
        let name = format!("{} {}", config.provider, config.plan);

        let mut subscription = Subscription::new(
            name,
            subscription_type,
            config.monthly_cost.unwrap_or(0.0),
            billing_start,
            billing_end,
        );

        // Set model if provided
        if let Some(ref model) = config.model {
            subscription = subscription.with_model(model);
        }

        // Set quota limit if provided
        if let Some(tokens) = config.monthly_tokens {
            subscription = subscription.with_quota(tokens);
        }

        // Set current usage if provided
        if let Some(usage) = config.current_usage {
            subscription.quota_used = usage;
        }

        // Set active status
        subscription.active = config.active;

        Ok(subscription)
    }

    /// Parse billing period from renewal date string.
    fn parse_billing_period(
        &self,
        renewal_date: &Option<String>,
        now: DateTime<Utc>,
    ) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
        let renewal = renewal_date.clone().unwrap_or_else(|| {
            // Default to end of current month
            format!("{}-{:02}-28", now.year(), now.month())
        });

        // Try to parse as ISO date (YYYY-MM-DD)
        if let Ok(date) = chrono::NaiveDate::parse_from_str(&renewal, "%Y-%m-%d") {
            let billing_end = date.and_hms_opt(23, 59, 59).unwrap().and_utc();
            // Billing period is typically one month
            let billing_start = billing_end - Duration::days(30);
            return Ok((billing_start, billing_end));
        }

        // Try to parse as day of month (e.g., "15" for 15th)
        if let Ok(day) = renewal.parse::<u32>() {
            let day = day.min(28); // Clamp to 28 to avoid month issues

            // Calculate next renewal date
            let mut renewal_month = now.month();
            let mut renewal_year = now.year();

            // If the day has passed this month, use next month
            if now.day() >= day {
                renewal_month += 1;
                if renewal_month > 12 {
                    renewal_month = 1;
                    renewal_year += 1;
                }
            }

            let billing_end = chrono::NaiveDate::from_ymd_opt(renewal_year, renewal_month, day)
                .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(renewal_year, renewal_month, 28).unwrap())
                .and_hms_opt(23, 59, 59)
                .unwrap()
                .and_utc();

            let billing_start = billing_end - Duration::days(30);
            return Ok((billing_start, billing_end));
        }

        // Default: 30 days from now
        let billing_end = now + Duration::days(30);
        let billing_start = now;
        Ok((billing_start, billing_end))
    }

    /// Update the summary cache.
    fn update_summary_cache(&mut self) {
        self.summary_cache = self.subscriptions
            .values()
            .map(|s| SubscriptionSummary::from(s))
            .collect();
    }

    /// Get all subscriptions.
    pub fn get_subscriptions(&self) -> Vec<&Subscription> {
        self.subscriptions.values().collect()
    }

    /// Get subscription summaries for display.
    pub fn get_summaries(&self) -> &[SubscriptionSummary] {
        &self.summary_cache
    }

    /// Get a subscription by name.
    pub fn get_subscription(&self, name: &str) -> Option<&Subscription> {
        self.subscriptions.get(name)
    }

    /// Get subscription count.
    pub fn len(&self) -> usize {
        self.subscriptions.len()
    }

    /// Check if there are no subscriptions.
    pub fn is_empty(&self) -> bool {
        self.subscriptions.is_empty()
    }

    /// Check if we have any active subscriptions.
    pub fn has_active(&self) -> bool {
        self.subscriptions.values().any(|s| s.active)
    }

    /// Sync subscriptions with the database.
    pub fn sync_to_database(&self, db: &CostDatabase) -> Result<()> {
        for subscription in self.subscriptions.values() {
            db.upsert_subscription(subscription)?;
        }
        Ok(())
    }

    /// Load subscriptions from the database.
    pub fn load_from_database(&mut self, db: &CostDatabase) -> Result<()> {
        let db_subscriptions = db.get_active_subscriptions()?;

        for sub in db_subscriptions {
            let name = sub.name.clone();
            self.subscriptions.insert(name, sub);
        }

        self.update_summary_cache();
        Ok(())
    }

    /// Calculate days until renewal for a subscription.
    pub fn days_until_renewal(&self, name: &str) -> Option<i64> {
        self.subscriptions.get(name).map(|s| {
            let duration = s.time_until_reset();
            duration.num_days().max(0)
        })
    }

    /// Get alert level for a subscription.
    pub fn get_alert(&self, name: &str) -> SubscriptionAlert {
        let Some(sub) = self.subscriptions.get(name) else {
            return SubscriptionAlert::None;
        };

        let usage_pct = sub.usage_percentage();
        let days_left = sub.time_until_reset().num_days();

        // Check if depleted
        if usage_pct >= 100.0 {
            return SubscriptionAlert::Depleted;
        }

        // Check if critical (>90% usage or <1 day to renewal)
        if usage_pct >= 90.0 || days_left <= 1 {
            return SubscriptionAlert::Critical;
        }

        // Check if warning (>80% usage or <3 days to renewal)
        if usage_pct >= 80.0 || days_left <= 3 {
            return SubscriptionAlert::Warning;
        }

        SubscriptionAlert::None
    }

    /// Get all subscriptions with alerts.
    pub fn get_alerts(&self) -> Vec<(String, SubscriptionAlert)> {
        self.subscriptions
            .keys()
            .map(|name| {
                let alert = self.get_alert(name);
                (name.clone(), alert)
            })
            .filter(|(_, alert)| alert.is_alert())
            .collect()
    }

    /// Check if any subscription has a critical or depleted alert.
    pub fn has_critical_alert(&self) -> bool {
        self.subscriptions.keys().any(|name| {
            matches!(
                self.get_alert(name),
                SubscriptionAlert::Critical | SubscriptionAlert::Depleted
            )
        })
    }

    /// Update usage for a subscription (typically from database).
    pub fn update_usage(&mut self, name: &str, quota_used: i64) {
        if let Some(sub) = self.subscriptions.get_mut(name) {
            sub.quota_used = quota_used;
            sub.updated_at = Utc::now();
            self.update_summary_cache();
        }
    }

    /// Increment usage for a subscription.
    pub fn increment_usage(&mut self, name: &str, units: i64) {
        if let Some(sub) = self.subscriptions.get_mut(name) {
            sub.quota_used += units;
            sub.updated_at = Utc::now();
            self.update_summary_cache();
        }
    }

    /// Find subscription by model name.
    pub fn find_subscription_for_model(&self, model: &str) -> Option<String> {
        for (name, sub) in &self.subscriptions {
            if sub.model.as_ref().map(|m| m == model).unwrap_or(false) {
                return Some(name.clone());
            }
        }
        None
    }

    /// Check and reset billing periods that have ended.
    /// Returns the number of subscriptions that were reset.
    pub fn check_and_reset_billing(&mut self, _db: &CostDatabase) -> Result<usize> {
        // Check all subscriptions for expired billing periods
        let now = Utc::now();
        let mut reset_count = 0;

        for sub in self.subscriptions.values_mut() {
            if sub.billing_end <= now && sub.active {
                // Reset billing period
                let new_start = sub.billing_end;
                let new_end = new_start + Duration::days(30);
                sub.billing_start = new_start;
                sub.billing_end = new_end;
                sub.quota_used = 0;
                sub.updated_at = now;
                reset_count += 1;
            }
        }

        if reset_count > 0 {
            self.update_summary_cache();
        }

        Ok(reset_count)
    }

    /// Create demo data for testing.
    pub fn with_demo_data() -> Self {
        let now = Utc::now();

        let subscriptions = vec![
            // Claude Pro: 328/500 messages, resets in 16 days
            Subscription::new(
                "anthropic max_5x",
                SubscriptionType::FixedQuota,
                100.0,
                now - Duration::days(14),
                now + Duration::days(16),
            )
            .with_quota(45_000_000) // 45M tokens
            .with_model("claude-opus-4.6"),
            // OpenAI Plus: monthly subscription
            Subscription::new(
                "openai plus",
                SubscriptionType::FixedQuota,
                20.0,
                now - Duration::days(10),
                now + Duration::days(20),
            )
            .with_quota(1_000_000), // 1M tokens
        ];

        let mut tracker = Self::new();
        for sub in subscriptions {
            let name = sub.name.clone();
            tracker.subscriptions.insert(name, sub);
        }

        // Set some usage
        if let Some(sub) = tracker.subscriptions.get_mut("anthropic max_5x") {
            sub.quota_used = 12_500_000; // 12.5M tokens used
        }
        if let Some(sub) = tracker.subscriptions.get_mut("openai plus") {
            sub.quota_used = 250_000; // 250K tokens used
        }

        tracker.update_summary_cache();
        tracker
    }
}

impl Default for SubscriptionTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_tracker_new() {
        let tracker = SubscriptionTracker::new();
        assert!(tracker.is_empty());
        assert!(!tracker.has_active());
    }

    #[test]
    fn test_subscription_tracker_demo_data() {
        let tracker = SubscriptionTracker::with_demo_data();
        assert_eq!(tracker.len(), 2);
        assert!(tracker.has_active());

        let summaries = tracker.get_summaries();
        assert_eq!(summaries.len(), 2);
    }

    #[test]
    fn test_days_until_renewal() {
        let tracker = SubscriptionTracker::with_demo_data();
        let days = tracker.days_until_renewal("anthropic max_5x");
        assert!(days.is_some());
        // Demo data sets renewal 16 days from now, but num_days() truncates
        // so the result could be 15 or 16 depending on the current time
        assert!(days.unwrap() >= 15 && days.unwrap() <= 17);
    }

    #[test]
    fn test_alert_levels() {
        let now = Utc::now();

        // Create subscription with high usage
        let mut sub = Subscription::new(
            "test",
            SubscriptionType::FixedQuota,
            20.0,
            now - Duration::days(25),
            now + Duration::days(5),
        )
        .with_quota(100);

        let mut tracker = SubscriptionTracker::new();

        // Low usage - no alert
        sub.quota_used = 10;
        tracker.subscriptions.insert("test".to_string(), sub.clone());
        tracker.update_summary_cache();
        assert_eq!(tracker.get_alert("test"), SubscriptionAlert::None);

        // 85% usage - warning
        sub.quota_used = 85;
        tracker.subscriptions.insert("test".to_string(), sub.clone());
        tracker.update_summary_cache();
        assert_eq!(tracker.get_alert("test"), SubscriptionAlert::Warning);

        // 95% usage - critical
        sub.quota_used = 95;
        tracker.subscriptions.insert("test".to_string(), sub.clone());
        tracker.update_summary_cache();
        assert_eq!(tracker.get_alert("test"), SubscriptionAlert::Critical);

        // 100% usage - depleted
        sub.quota_used = 100;
        tracker.subscriptions.insert("test".to_string(), sub);
        tracker.update_summary_cache();
        assert_eq!(tracker.get_alert("test"), SubscriptionAlert::Depleted);
    }

    #[test]
    fn test_parse_billing_period_iso_date() {
        let tracker = SubscriptionTracker::new();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 2, 12).unwrap().and_hms_opt(12, 0, 0).unwrap().and_utc();

        let (start, end) = tracker.parse_billing_period(&Some("2026-02-28".to_string()), now).unwrap();

        assert_eq!(end.day(), 28);
        assert_eq!(end.month(), 2);
        assert_eq!(end.year(), 2026);
    }

    #[test]
    fn test_parse_billing_period_day_of_month() {
        let tracker = SubscriptionTracker::new();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 2, 12).unwrap().and_hms_opt(12, 0, 0).unwrap().and_utc();

        let (start, end) = tracker.parse_billing_period(&Some("15".to_string()), now).unwrap();

        // Should be 15th of current or next month
        assert!(end.day() == 15);
    }

    #[test]
    fn test_config_to_subscription() {
        let tracker = SubscriptionTracker::new();

        let config = SubscriptionConfig {
            provider: "anthropic".to_string(),
            plan: "max_5x".to_string(),
            monthly_tokens: Some(45_000_000),
            monthly_cost: Some(100.0),
            renewal_date: Some("15".to_string()),
            current_usage: Some(12_500_000),
            model: Some("claude-opus-4.6".to_string()),
            subscription_type: "fixed_quota".to_string(),
            active: true,
        };

        let sub = tracker.config_to_subscription(&config).unwrap();

        assert_eq!(sub.name, "anthropic max_5x");
        assert_eq!(sub.quota_limit, Some(45_000_000));
        assert_eq!(sub.quota_used, 12_500_000);
        assert_eq!(sub.model, Some("claude-opus-4.6".to_string()));
        assert!(sub.active);
    }

    #[test]
    fn test_update_usage() {
        let mut tracker = SubscriptionTracker::with_demo_data();

        tracker.update_usage("anthropic max_5x", 20_000_000);

        let sub = tracker.get_subscription("anthropic max_5x").unwrap();
        assert_eq!(sub.quota_used, 20_000_000);
    }

    #[test]
    fn test_increment_usage() {
        let mut tracker = SubscriptionTracker::with_demo_data();

        let initial = tracker.get_subscription("anthropic max_5x").unwrap().quota_used;
        tracker.increment_usage("anthropic max_5x", 1_000_000);

        let sub = tracker.get_subscription("anthropic max_5x").unwrap();
        assert_eq!(sub.quota_used, initial + 1_000_000);
    }

    #[test]
    fn test_get_alerts() {
        let now = Utc::now();

        let mut tracker = SubscriptionTracker::new();

        // Create subscriptions with different alert levels
        let sub1 = Subscription::new(
            "low_usage",
            SubscriptionType::FixedQuota,
            20.0,
            now - Duration::days(10),
            now + Duration::days(20),
        )
        .with_quota(100);
        tracker.subscriptions.insert("low_usage".to_string(), sub1);

        let mut sub2 = Subscription::new(
            "high_usage",
            SubscriptionType::FixedQuota,
            20.0,
            now - Duration::days(10),
            now + Duration::days(20),
        )
        .with_quota(100);
        sub2.quota_used = 95;
        tracker.subscriptions.insert("high_usage".to_string(), sub2);

        tracker.update_summary_cache();

        let alerts = tracker.get_alerts();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].0, "high_usage");
        assert_eq!(alerts[0].1, SubscriptionAlert::Critical);
    }
}
