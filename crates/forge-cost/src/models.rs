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
}
