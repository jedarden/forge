//! Data models for cost tracking.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// Represents a single API call with token usage and cost.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiCall {
    /// Unique identifier for this API call (generated from timestamp + hash)
    pub id: Option<i64>,

    /// Timestamp of the API call
    pub timestamp: DateTime<Utc>,

    /// Worker identifier (e.g., "claude-code-opus-alpha")
    pub worker_id: String,

    /// Session identifier from the log
    pub session_id: Option<String>,

    /// Model identifier (e.g., "claude-opus-4-5-20251101", "glm-4.7")
    pub model: String,

    /// Number of input tokens
    pub input_tokens: i64,

    /// Number of output tokens
    pub output_tokens: i64,

    /// Cache creation input tokens (Anthropic-specific)
    pub cache_creation_tokens: i64,

    /// Cache read input tokens (Anthropic-specific)
    pub cache_read_tokens: i64,

    /// Total cost in USD
    pub cost_usd: f64,

    /// Associated bead ID (if any)
    pub bead_id: Option<String>,

    /// Event type (result, assistant, etc.)
    pub event_type: String,
}

impl ApiCall {
    /// Create a new ApiCall with required fields.
    pub fn new(
        timestamp: DateTime<Utc>,
        worker_id: impl Into<String>,
        model: impl Into<String>,
        input_tokens: i64,
        output_tokens: i64,
        cost_usd: f64,
    ) -> Self {
        Self {
            id: None,
            timestamp,
            worker_id: worker_id.into(),
            session_id: None,
            model: model.into(),
            input_tokens,
            output_tokens,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            cost_usd,
            bead_id: None,
            event_type: "result".to_string(),
        }
    }

    /// Set cache tokens.
    pub fn with_cache(mut self, creation: i64, read: i64) -> Self {
        self.cache_creation_tokens = creation;
        self.cache_read_tokens = read;
        self
    }

    /// Set bead ID.
    pub fn with_bead(mut self, bead_id: impl Into<String>) -> Self {
        self.bead_id = Some(bead_id.into());
        self
    }

    /// Set session ID.
    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Total tokens (input + output + cache).
    pub fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

/// Cost breakdown by model for a time period.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostBreakdown {
    /// Model name
    pub model: String,

    /// Number of API calls
    pub call_count: i64,

    /// Total input tokens
    pub input_tokens: i64,

    /// Total output tokens
    pub output_tokens: i64,

    /// Total cache creation tokens
    pub cache_creation_tokens: i64,

    /// Total cache read tokens
    pub cache_read_tokens: i64,

    /// Total cost in USD
    pub total_cost_usd: f64,
}

/// Daily cost aggregation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyCost {
    /// Date of the costs
    pub date: NaiveDate,

    /// Total cost in USD
    pub total_cost_usd: f64,

    /// Number of API calls
    pub call_count: i64,

    /// Total tokens used
    pub total_tokens: i64,

    /// Cost breakdown by model
    pub by_model: Vec<CostBreakdown>,
}

/// Monthly cost aggregation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyCost {
    /// Year
    pub year: i32,

    /// Month (1-12)
    pub month: u32,

    /// Total cost in USD
    pub total_cost_usd: f64,

    /// Number of API calls
    pub call_count: i64,

    /// Daily breakdown
    pub by_day: Vec<DailyCost>,

    /// Model breakdown
    pub by_model: Vec<CostBreakdown>,
}

/// Cost per model for aggregation queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCost {
    /// Model name
    pub model: String,

    /// Total cost in USD
    pub total_cost_usd: f64,

    /// Number of API calls
    pub call_count: i64,

    /// Average cost per call
    pub avg_cost_per_call: f64,

    /// Total tokens
    pub total_tokens: i64,
}

/// Projected costs based on current usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedCost {
    /// Days remaining in projection period
    pub days_remaining: i32,

    /// Current spending rate (USD per day)
    pub daily_rate: f64,

    /// Projected total cost for the period
    pub projected_total: f64,

    /// Cost so far in the period
    pub current_total: f64,

    /// Confidence level (0-1)
    pub confidence: f64,
}

impl ProjectedCost {
    /// Calculate projected cost from current spending.
    pub fn calculate(current_total: f64, days_elapsed: i32, days_remaining: i32) -> Self {
        let daily_rate = if days_elapsed > 0 {
            current_total / days_elapsed as f64
        } else {
            0.0
        };

        let projected_total = current_total + (daily_rate * days_remaining as f64);

        // Confidence decreases with fewer days of data
        let confidence = (days_elapsed as f64 / 7.0).min(1.0);

        Self {
            days_remaining,
            daily_rate,
            projected_total,
            current_total,
            confidence,
        }
    }
}

/// Subscription type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionType {
    /// Fixed monthly quota (e.g., Claude Pro: 500 messages)
    FixedQuota,
    /// Unlimited usage (e.g., unlimited plan)
    Unlimited,
    /// Pay-per-use API (no fixed cost, variable pricing)
    PayPerUse,
}

impl Default for SubscriptionType {
    fn default() -> Self {
        Self::FixedQuota
    }
}

/// Subscription quota status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaStatus {
    /// On track to use quota appropriately
    OnPace,
    /// Under-utilized, should accelerate usage
    Accelerate,
    /// Nearly exhausted, should max out
    MaxOut,
    /// Depleted, quota exhausted
    Depleted,
}

impl Default for QuotaStatus {
    fn default() -> Self {
        Self::OnPace
    }
}

/// Represents a subscription service for tracking usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    /// Unique identifier for the subscription
    pub id: Option<i64>,

    /// Subscription name (e.g., "Claude Pro", "ChatGPT Plus")
    pub name: String,

    /// Associated model or service (e.g., "claude-sonnet", "gpt-4")
    pub model: Option<String>,

    /// Subscription type
    pub subscription_type: SubscriptionType,

    /// Monthly cost in USD
    pub monthly_cost: f64,

    /// Quota limit (messages, tokens, or requests per billing period)
    pub quota_limit: Option<i64>,

    /// Current quota used
    pub quota_used: i64,

    /// Billing period start date
    pub billing_start: DateTime<Utc>,

    /// Billing period end date (reset date)
    pub billing_end: DateTime<Utc>,

    /// Whether the subscription is active
    pub active: bool,

    /// Last updated timestamp
    pub updated_at: DateTime<Utc>,
}

impl Subscription {
    /// Create a new subscription with required fields.
    pub fn new(
        name: impl Into<String>,
        subscription_type: SubscriptionType,
        monthly_cost: f64,
        billing_start: DateTime<Utc>,
        billing_end: DateTime<Utc>,
    ) -> Self {
        Self {
            id: None,
            name: name.into(),
            model: None,
            subscription_type,
            monthly_cost,
            quota_limit: None,
            quota_used: 0,
            billing_start,
            billing_end,
            active: true,
            updated_at: Utc::now(),
        }
    }

    /// Set the associated model.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the quota limit.
    pub fn with_quota(mut self, limit: i64) -> Self {
        self.quota_limit = Some(limit);
        self
    }

    /// Calculate usage percentage (0.0 - 100.0).
    pub fn usage_percentage(&self) -> f64 {
        match self.quota_limit {
            Some(limit) if limit > 0 => (self.quota_used as f64 / limit as f64) * 100.0,
            _ => 0.0,
        }
    }

    /// Get remaining quota.
    pub fn remaining_quota(&self) -> Option<i64> {
        self.quota_limit.map(|limit| (limit - self.quota_used).max(0))
    }

    /// Calculate time remaining until quota reset.
    pub fn time_until_reset(&self) -> chrono::Duration {
        self.billing_end.signed_duration_since(Utc::now())
    }

    /// Format time until reset as human-readable string.
    pub fn reset_time_display(&self) -> String {
        let duration = self.time_until_reset();
        let days = duration.num_days();
        let hours = duration.num_hours() % 24;

        if days > 0 {
            format!("{}d {}h", days, hours)
        } else if hours > 0 {
            format!("{}h", hours)
        } else {
            let minutes = duration.num_minutes() % 60;
            format!("{}m", minutes.max(0))
        }
    }

    /// Calculate quota status based on usage and time remaining.
    pub fn quota_status(&self) -> QuotaStatus {
        let Some(limit) = self.quota_limit else {
            return QuotaStatus::OnPace;
        };

        let usage_pct = self.usage_percentage();
        let total_period = self.billing_end.signed_duration_since(self.billing_start);
        let elapsed = Utc::now().signed_duration_since(self.billing_start);

        // Time elapsed as percentage of billing period
        let time_pct = if total_period.num_seconds() > 0 {
            (elapsed.num_seconds() as f64 / total_period.num_seconds() as f64) * 100.0
        } else {
            100.0
        };

        // If quota is depleted
        if self.quota_used >= limit {
            return QuotaStatus::Depleted;
        }

        // Calculate expected usage vs actual
        let expected_usage_pct = time_pct;
        let usage_diff = usage_pct - expected_usage_pct;

        if usage_diff > 20.0 {
            // Usage significantly ahead of schedule - on pace or ahead
            QuotaStatus::OnPace
        } else if usage_diff < -30.0 && time_pct > 50.0 {
            // More than halfway through period but significantly under-utilized
            QuotaStatus::Accelerate
        } else if time_pct > 80.0 && usage_pct < 70.0 {
            // Near end of period with lots of quota remaining - max out
            QuotaStatus::MaxOut
        } else {
            QuotaStatus::OnPace
        }
    }

    /// Get recommended action based on quota status.
    pub fn recommended_action(&self) -> &'static str {
        match self.quota_status() {
            QuotaStatus::OnPace => "📊 On-Pace",
            QuotaStatus::Accelerate => "🚀 Accelerate",
            QuotaStatus::MaxOut => "⚠️ Max Out",
            QuotaStatus::Depleted => "❌ Depleted",
        }
    }
}

/// Usage record for tracking subscription consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionUsageRecord {
    /// Unique identifier
    pub id: Option<i64>,

    /// Subscription ID this record belongs to
    pub subscription_id: i64,

    /// Timestamp of the usage
    pub timestamp: DateTime<Utc>,

    /// Number of units consumed (messages, tokens, requests)
    pub units: i64,

    /// Associated worker ID (if applicable)
    pub worker_id: Option<String>,

    /// Associated bead ID (if applicable)
    pub bead_id: Option<String>,

    /// API call ID this usage corresponds to (if tracked)
    pub api_call_id: Option<i64>,
}

impl SubscriptionUsageRecord {
    /// Create a new usage record.
    pub fn new(subscription_id: i64, units: i64) -> Self {
        Self {
            id: None,
            subscription_id,
            timestamp: Utc::now(),
            units,
            worker_id: None,
            bead_id: None,
            api_call_id: None,
        }
    }

    /// Set worker ID.
    pub fn with_worker(mut self, worker_id: impl Into<String>) -> Self {
        self.worker_id = Some(worker_id.into());
        self
    }

    /// Set bead ID.
    pub fn with_bead(mut self, bead_id: impl Into<String>) -> Self {
        self.bead_id = Some(bead_id.into());
        self
    }

    /// Set API call ID.
    pub fn with_api_call(mut self, api_call_id: i64) -> Self {
        self.api_call_id = Some(api_call_id);
        self
    }
}

/// Summary of subscription status for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSummary {
    /// Subscription name
    pub name: String,

    /// Associated model (if any)
    pub model: Option<String>,

    /// Usage percentage (0.0 - 100.0)
    pub usage_percentage: f64,

    /// Quota used
    pub quota_used: i64,

    /// Quota limit (None for unlimited/pay-per-use)
    pub quota_limit: Option<i64>,

    /// Time until reset (formatted)
    pub reset_time: String,

    /// Quota status
    pub status: QuotaStatus,

    /// Recommended action
    pub recommended_action: String,

    /// Monthly cost
    pub monthly_cost: f64,

    /// Estimated savings vs API pricing
    pub estimated_savings: f64,
}

impl From<&Subscription> for SubscriptionSummary {
    fn from(sub: &Subscription) -> Self {
        Self {
            name: sub.name.clone(),
            model: sub.model.clone(),
            usage_percentage: sub.usage_percentage(),
            quota_used: sub.quota_used,
            quota_limit: sub.quota_limit,
            reset_time: sub.reset_time_display(),
            status: sub.quota_status(),
            recommended_action: sub.recommended_action().to_string(),
            monthly_cost: sub.monthly_cost,
            estimated_savings: 0.0, // Calculated separately based on API pricing
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_api_call_total_tokens() {
        let call = ApiCall::new(Utc::now(), "worker-1", "claude-opus", 100, 50, 0.01)
            .with_cache(200, 300);

        assert_eq!(call.total_tokens(), 650); // 100 + 50 + 200 + 300
    }

    #[test]
    fn test_projected_cost() {
        let proj = ProjectedCost::calculate(100.0, 10, 20);

        assert_eq!(proj.daily_rate, 10.0);
        assert_eq!(proj.projected_total, 300.0); // 100 + 10*20
        assert_eq!(proj.current_total, 100.0);
        assert_eq!(proj.confidence, 1.0); // 10 days is enough
    }

    #[test]
    fn test_projected_cost_low_confidence() {
        let proj = ProjectedCost::calculate(21.0, 3, 27);

        assert_eq!(proj.daily_rate, 7.0);
        assert!((proj.confidence - 3.0 / 7.0).abs() < 0.001);
    }

    #[test]
    fn test_subscription_usage_percentage() {
        use chrono::Duration;

        let start = Utc::now() - Duration::days(15);
        let end = Utc::now() + Duration::days(15);

        let sub = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
            .with_quota(500);

        // 0% used
        assert_eq!(sub.usage_percentage(), 0.0);

        // 50% used
        let mut sub2 = sub.clone();
        sub2.quota_used = 250;
        assert!((sub2.usage_percentage() - 50.0).abs() < 0.001);

        // 100% used
        let mut sub3 = sub.clone();
        sub3.quota_used = 500;
        assert!((sub3.usage_percentage() - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_subscription_remaining_quota() {
        use chrono::Duration;

        let start = Utc::now() - Duration::days(1);
        let end = Utc::now() + Duration::days(29);

        let sub = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
            .with_quota(500);

        assert_eq!(sub.remaining_quota(), Some(500));

        let mut sub2 = sub.clone();
        sub2.quota_used = 328;
        assert_eq!(sub2.remaining_quota(), Some(172));
    }

    #[test]
    fn test_subscription_quota_status_on_pace() {
        use chrono::Duration;

        // Halfway through billing period with ~50% usage = on pace
        let start = Utc::now() - Duration::days(15);
        let end = Utc::now() + Duration::days(15);

        let mut sub =
            Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
                .with_quota(500);
        sub.quota_used = 250; // 50% used, 50% of time elapsed

        assert_eq!(sub.quota_status(), QuotaStatus::OnPace);
    }

    #[test]
    fn test_subscription_quota_status_accelerate() {
        use chrono::Duration;

        // 80% through billing period with only 30% usage = accelerate
        let start = Utc::now() - Duration::days(24);
        let end = Utc::now() + Duration::days(6);

        let mut sub =
            Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
                .with_quota(500);
        sub.quota_used = 150; // 30% used, 80% of time elapsed

        assert_eq!(sub.quota_status(), QuotaStatus::Accelerate);
    }

    #[test]
    fn test_subscription_quota_status_max_out() {
        use chrono::Duration;

        // 85% through billing period with 65% usage = max out
        // time_pct > 80%, usage_pct < 70% triggers MaxOut
        let start = Utc::now() - Duration::days(26);
        let end = Utc::now() + Duration::days(4);

        let mut sub =
            Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
                .with_quota(500);
        sub.quota_used = 300; // 60% used, ~87% of time elapsed

        assert_eq!(sub.quota_status(), QuotaStatus::MaxOut);
    }

    #[test]
    fn test_subscription_quota_status_depleted() {
        use chrono::Duration;

        let start = Utc::now() - Duration::days(15);
        let end = Utc::now() + Duration::days(15);

        let mut sub =
            Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
                .with_quota(500);
        sub.quota_used = 500; // 100% used

        assert_eq!(sub.quota_status(), QuotaStatus::Depleted);
    }

    #[test]
    fn test_subscription_reset_time_display() {
        use chrono::Duration;

        // 16 days 9 hours remaining
        let start = Utc::now() - Duration::days(14);
        let end = Utc::now() + Duration::days(16) + Duration::hours(9);

        let sub =
            Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end);

        let display = sub.reset_time_display();
        assert!(display.starts_with("16d"));
    }

    #[test]
    fn test_subscription_summary_from() {
        use chrono::Duration;

        let start = Utc::now() - Duration::days(15);
        let end = Utc::now() + Duration::days(15);

        let mut sub =
            Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
                .with_quota(500)
                .with_model("claude-sonnet-4.5");
        sub.quota_used = 328;

        let summary = SubscriptionSummary::from(&sub);

        assert_eq!(summary.name, "Claude Pro");
        assert_eq!(summary.model, Some("claude-sonnet-4.5".to_string()));
        assert_eq!(summary.quota_used, 328);
        assert_eq!(summary.quota_limit, Some(500));
        assert!((summary.usage_percentage - 65.6).abs() < 0.1);
    }

    #[test]
    fn test_usage_record_builder() {
        let record = SubscriptionUsageRecord::new(1, 5)
            .with_worker("glm-alpha")
            .with_bead("bd-123")
            .with_api_call(42);

        assert_eq!(record.subscription_id, 1);
        assert_eq!(record.units, 5);
        assert_eq!(record.worker_id, Some("glm-alpha".to_string()));
        assert_eq!(record.bead_id, Some("bd-123".to_string()));
        assert_eq!(record.api_call_id, Some(42));
    }
}
