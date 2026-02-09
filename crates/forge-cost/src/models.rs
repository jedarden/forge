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

// ============ Performance Metrics Models ============

/// Hourly statistics for performance tracking.
///
/// Aggregated metrics for a specific hour, providing granular performance data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyStat {
    /// Unique identifier
    pub id: Option<i64>,

    /// Hour timestamp (truncated to hour)
    pub hour: DateTime<Utc>,

    /// Total API calls in this hour
    pub total_calls: i64,

    /// Total cost in USD for this hour
    pub total_cost_usd: f64,

    /// Total input tokens
    pub total_input_tokens: i64,

    /// Total output tokens
    pub total_output_tokens: i64,

    /// Number of tasks started
    pub tasks_started: i64,

    /// Number of tasks completed
    pub tasks_completed: i64,

    /// Number of tasks failed
    pub tasks_failed: i64,

    /// Number of active workers (max during the hour)
    pub active_workers: i64,

    /// Average response time in milliseconds (if tracked)
    pub avg_response_time_ms: Option<f64>,

    /// Tokens per minute throughput
    pub tokens_per_minute: f64,

    /// When this stat was last updated
    pub last_updated: DateTime<Utc>,
}

impl HourlyStat {
    /// Create a new hourly stat for the given hour.
    pub fn new(hour: DateTime<Utc>) -> Self {
        Self {
            id: None,
            hour,
            total_calls: 0,
            total_cost_usd: 0.0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            tasks_started: 0,
            tasks_completed: 0,
            tasks_failed: 0,
            active_workers: 0,
            avg_response_time_ms: None,
            tokens_per_minute: 0.0,
            last_updated: Utc::now(),
        }
    }

    /// Calculate tokens per minute based on total tokens.
    pub fn calculate_throughput(&mut self) {
        let total_tokens = self.total_input_tokens + self.total_output_tokens;
        // Assuming this represents a full hour (60 minutes)
        self.tokens_per_minute = total_tokens as f64 / 60.0;
    }

    /// Task success rate (0.0 - 1.0).
    pub fn success_rate(&self) -> f64 {
        let total = self.tasks_completed + self.tasks_failed;
        if total == 0 {
            1.0
        } else {
            self.tasks_completed as f64 / total as f64
        }
    }

    /// Average cost per task.
    pub fn cost_per_task(&self) -> f64 {
        let total_tasks = self.tasks_completed + self.tasks_failed;
        if total_tasks == 0 {
            0.0
        } else {
            self.total_cost_usd / total_tasks as f64
        }
    }
}

/// Daily statistics for performance tracking.
///
/// Aggregated metrics for a specific day, providing high-level performance overview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyStat {
    /// Unique identifier
    pub id: Option<i64>,

    /// Date of the statistics
    pub date: NaiveDate,

    /// Total API calls for the day
    pub total_calls: i64,

    /// Total cost in USD for the day
    pub total_cost_usd: f64,

    /// Total input tokens
    pub total_input_tokens: i64,

    /// Total output tokens
    pub total_output_tokens: i64,

    /// Total cache creation tokens
    pub total_cache_creation_tokens: i64,

    /// Total cache read tokens
    pub total_cache_read_tokens: i64,

    /// Number of tasks started
    pub tasks_started: i64,

    /// Number of tasks completed
    pub tasks_completed: i64,

    /// Number of tasks failed
    pub tasks_failed: i64,

    /// Peak number of concurrent workers
    pub peak_workers: i64,

    /// Average tokens per minute throughput
    pub avg_tokens_per_minute: f64,

    /// Task success rate (0.0 - 1.0)
    pub success_rate: f64,

    /// Average cost per completed task
    pub avg_cost_per_task: f64,

    /// When this stat was last updated
    pub last_updated: DateTime<Utc>,
}

impl DailyStat {
    /// Create a new daily stat for the given date.
    pub fn new(date: NaiveDate) -> Self {
        Self {
            id: None,
            date,
            total_calls: 0,
            total_cost_usd: 0.0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cache_creation_tokens: 0,
            total_cache_read_tokens: 0,
            tasks_started: 0,
            tasks_completed: 0,
            tasks_failed: 0,
            peak_workers: 0,
            avg_tokens_per_minute: 0.0,
            success_rate: 1.0,
            avg_cost_per_task: 0.0,
            last_updated: Utc::now(),
        }
    }

    /// Calculate derived metrics.
    pub fn calculate_derived_metrics(&mut self) {
        // Success rate
        let total_tasks = self.tasks_completed + self.tasks_failed;
        self.success_rate = if total_tasks == 0 {
            1.0
        } else {
            self.tasks_completed as f64 / total_tasks as f64
        };

        // Average cost per task
        self.avg_cost_per_task = if self.tasks_completed == 0 {
            0.0
        } else {
            self.total_cost_usd / self.tasks_completed as f64
        };

        // Tokens per minute (assuming 24 hours of activity, normalized)
        let total_tokens = self.total_input_tokens
            + self.total_output_tokens
            + self.total_cache_creation_tokens
            + self.total_cache_read_tokens;
        // Divide by 1440 (minutes in a day) for average throughput
        self.avg_tokens_per_minute = total_tokens as f64 / 1440.0;
    }

    /// Total tokens including cache tokens.
    pub fn total_tokens(&self) -> i64 {
        self.total_input_tokens
            + self.total_output_tokens
            + self.total_cache_creation_tokens
            + self.total_cache_read_tokens
    }
}

/// Worker efficiency statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerEfficiency {
    /// Worker identifier
    pub worker_id: String,

    /// Date of the statistics
    pub date: NaiveDate,

    /// Total API calls made by this worker
    pub total_calls: i64,

    /// Total cost incurred by this worker
    pub total_cost_usd: f64,

    /// Total tokens used
    pub total_tokens: i64,

    /// Number of tasks completed
    pub tasks_completed: i64,

    /// Number of tasks failed
    pub tasks_failed: i64,

    /// Average cost per task
    pub avg_cost_per_task: f64,

    /// Success rate (0.0 - 1.0)
    pub success_rate: f64,

    /// Model used by this worker
    pub model: Option<String>,

    /// Total active time in seconds
    pub active_time_secs: i64,
}

impl WorkerEfficiency {
    /// Create a new worker efficiency record.
    pub fn new(worker_id: impl Into<String>, date: NaiveDate) -> Self {
        Self {
            worker_id: worker_id.into(),
            date,
            total_calls: 0,
            total_cost_usd: 0.0,
            total_tokens: 0,
            tasks_completed: 0,
            tasks_failed: 0,
            avg_cost_per_task: 0.0,
            success_rate: 1.0,
            model: None,
            active_time_secs: 0,
        }
    }

    /// Calculate derived metrics.
    pub fn calculate_derived_metrics(&mut self) {
        let total_tasks = self.tasks_completed + self.tasks_failed;
        self.success_rate = if total_tasks == 0 {
            1.0
        } else {
            self.tasks_completed as f64 / total_tasks as f64
        };

        self.avg_cost_per_task = if self.tasks_completed == 0 {
            0.0
        } else {
            self.total_cost_usd / self.tasks_completed as f64
        };
    }

    /// Cost efficiency score (lower is better).
    ///
    /// Calculated as cost per task adjusted for success rate.
    pub fn efficiency_score(&self) -> f64 {
        if self.success_rate == 0.0 {
            f64::MAX
        } else {
            self.avg_cost_per_task / self.success_rate
        }
    }
}

/// Model performance statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPerformance {
    /// Model identifier
    pub model: String,

    /// Date of the statistics
    pub date: NaiveDate,

    /// Total API calls for this model
    pub total_calls: i64,

    /// Total cost for this model
    pub total_cost_usd: f64,

    /// Total input tokens
    pub total_input_tokens: i64,

    /// Total output tokens
    pub total_output_tokens: i64,

    /// Total cache creation tokens
    pub total_cache_creation_tokens: i64,

    /// Total cache read tokens
    pub total_cache_read_tokens: i64,

    /// Number of tasks completed with this model
    pub tasks_completed: i64,

    /// Number of tasks failed with this model
    pub tasks_failed: i64,

    /// Average cost per task
    pub avg_cost_per_task: f64,

    /// Success rate (0.0 - 1.0)
    pub success_rate: f64,

    /// Average tokens per call
    pub avg_tokens_per_call: f64,

    /// Cache hit rate (cache_read / total_input)
    pub cache_hit_rate: f64,
}

impl ModelPerformance {
    /// Create a new model performance record.
    pub fn new(model: impl Into<String>, date: NaiveDate) -> Self {
        Self {
            model: model.into(),
            date,
            total_calls: 0,
            total_cost_usd: 0.0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cache_creation_tokens: 0,
            total_cache_read_tokens: 0,
            tasks_completed: 0,
            tasks_failed: 0,
            avg_cost_per_task: 0.0,
            success_rate: 1.0,
            avg_tokens_per_call: 0.0,
            cache_hit_rate: 0.0,
        }
    }

    /// Calculate derived metrics.
    pub fn calculate_derived_metrics(&mut self) {
        // Success rate
        let total_tasks = self.tasks_completed + self.tasks_failed;
        self.success_rate = if total_tasks == 0 {
            1.0
        } else {
            self.tasks_completed as f64 / total_tasks as f64
        };

        // Average cost per task
        self.avg_cost_per_task = if self.tasks_completed == 0 {
            0.0
        } else {
            self.total_cost_usd / self.tasks_completed as f64
        };

        // Average tokens per call
        let total_tokens = self.total_input_tokens
            + self.total_output_tokens
            + self.total_cache_creation_tokens
            + self.total_cache_read_tokens;
        self.avg_tokens_per_call = if self.total_calls == 0 {
            0.0
        } else {
            total_tokens as f64 / self.total_calls as f64
        };

        // Cache hit rate
        let total_input = self.total_input_tokens + self.total_cache_read_tokens;
        self.cache_hit_rate = if total_input == 0 {
            0.0
        } else {
            self.total_cache_read_tokens as f64 / total_input as f64
        };
    }

    /// Total tokens.
    pub fn total_tokens(&self) -> i64 {
        self.total_input_tokens
            + self.total_output_tokens
            + self.total_cache_creation_tokens
            + self.total_cache_read_tokens
    }
}

/// Performance summary for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSummary {
    /// Time period start
    pub period_start: DateTime<Utc>,

    /// Time period end
    pub period_end: DateTime<Utc>,

    /// Total API calls
    pub total_calls: i64,

    /// Total cost
    pub total_cost_usd: f64,

    /// Total tokens
    pub total_tokens: i64,

    /// Tasks completed
    pub tasks_completed: i64,

    /// Tasks failed
    pub tasks_failed: i64,

    /// Overall success rate
    pub success_rate: f64,

    /// Average cost per task
    pub avg_cost_per_task: f64,

    /// Average tokens per minute
    pub avg_tokens_per_minute: f64,

    /// Most used model
    pub top_model: Option<String>,

    /// Most efficient worker
    pub top_worker: Option<String>,
}

impl PerformanceSummary {
    /// Create a new performance summary.
    pub fn new(period_start: DateTime<Utc>, period_end: DateTime<Utc>) -> Self {
        Self {
            period_start,
            period_end,
            total_calls: 0,
            total_cost_usd: 0.0,
            total_tokens: 0,
            tasks_completed: 0,
            tasks_failed: 0,
            success_rate: 1.0,
            avg_cost_per_task: 0.0,
            avg_tokens_per_minute: 0.0,
            top_model: None,
            top_worker: None,
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

    // ============ Performance Metrics Tests ============

    #[test]
    fn test_hourly_stat_new() {
        let now = Utc::now();
        let stat = HourlyStat::new(now);

        assert_eq!(stat.hour, now);
        assert_eq!(stat.total_calls, 0);
        assert_eq!(stat.total_cost_usd, 0.0);
        assert_eq!(stat.tasks_completed, 0);
    }

    #[test]
    fn test_hourly_stat_success_rate() {
        let mut stat = HourlyStat::new(Utc::now());

        // No tasks = 100% success rate
        assert!((stat.success_rate() - 1.0).abs() < 0.001);

        // 8 completed, 2 failed = 80% success rate
        stat.tasks_completed = 8;
        stat.tasks_failed = 2;
        assert!((stat.success_rate() - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_hourly_stat_throughput() {
        let mut stat = HourlyStat::new(Utc::now());
        stat.total_input_tokens = 30000;
        stat.total_output_tokens = 30000;
        stat.calculate_throughput();

        // 60000 tokens / 60 minutes = 1000 tokens/min
        assert!((stat.tokens_per_minute - 1000.0).abs() < 0.001);
    }

    #[test]
    fn test_daily_stat_derived_metrics() {
        let today = Utc::now().date_naive();
        let mut stat = DailyStat::new(today);

        stat.total_cost_usd = 10.0;
        stat.tasks_completed = 5;
        stat.tasks_failed = 0;
        stat.total_input_tokens = 100000;
        stat.total_output_tokens = 50000;
        stat.calculate_derived_metrics();

        assert!((stat.success_rate - 1.0).abs() < 0.001);
        assert!((stat.avg_cost_per_task - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_worker_efficiency_score() {
        let today = Utc::now().date_naive();
        let mut eff = WorkerEfficiency::new("worker-1", today);

        eff.total_cost_usd = 10.0;
        eff.tasks_completed = 10;
        eff.tasks_failed = 0;
        eff.calculate_derived_metrics();

        // Cost per task = 1.0, success rate = 1.0, score = 1.0
        assert!((eff.efficiency_score() - 1.0).abs() < 0.001);

        // With 50% failure rate
        eff.tasks_failed = 10;
        eff.calculate_derived_metrics();
        // Cost per task = 1.0, success rate = 0.5, score = 2.0
        assert!((eff.efficiency_score() - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_model_performance_cache_hit_rate() {
        let today = Utc::now().date_naive();
        let mut perf = ModelPerformance::new("claude-opus", today);

        perf.total_input_tokens = 80000;
        perf.total_cache_read_tokens = 20000;
        perf.calculate_derived_metrics();

        // cache_read / (input + cache_read) = 20000 / 100000 = 0.2
        assert!((perf.cache_hit_rate - 0.2).abs() < 0.001);
    }
}
