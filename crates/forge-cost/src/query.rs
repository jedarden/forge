//! Query functions for cost analysis.

use crate::db::CostDatabase;
use crate::error::{CostError, Result};
use crate::models::{
    CostBreakdown, DailyCost, ModelCost, MonthlyCost, ProjectedCost, Subscription,
    SubscriptionSummary,
};
use chrono::{Datelike, NaiveDate, Utc};
use rusqlite::params;
/// Query interface for cost analysis.
pub struct CostQuery<'a> {
    db: &'a CostDatabase,
}

impl<'a> CostQuery<'a> {
    /// Create a new query interface.
    pub fn new(db: &'a CostDatabase) -> Self {
        Self { db }
    }

    /// Get today's costs with per-model breakdown.
    pub fn get_today_costs(&self) -> Result<DailyCost> {
        let today = Utc::now().date_naive();
        self.get_costs_for_date(today)
    }

    /// Get costs for a specific date.
    pub fn get_costs_for_date(&self, date: NaiveDate) -> Result<DailyCost> {
        // Try aggregation table first
        if let Some(daily) = self.db.get_daily_cost(date)? {
            return Ok(daily);
        }

        // Fall back to querying api_calls directly
        let conn = self.db.connection();
        let conn = conn
            .lock()
            .map_err(|e| CostError::Query(format!("failed to acquire lock: {}", e)))?;

        let date_str = date.format("%Y-%m-%d").to_string();

        // Get aggregated totals
        let (total_cost_usd, call_count, total_tokens): (f64, i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(cost_usd), 0),
                        COUNT(*),
                        COALESCE(SUM(input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens), 0)
                 FROM api_calls
                 WHERE DATE(timestamp) = ?1",
                params![date_str],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;

        // Get per-model breakdown
        let mut stmt = conn.prepare(
            "SELECT model,
                    SUM(cost_usd),
                    COUNT(*),
                    SUM(input_tokens),
                    SUM(output_tokens),
                    SUM(cache_creation_tokens),
                    SUM(cache_read_tokens)
             FROM api_calls
             WHERE DATE(timestamp) = ?1
             GROUP BY model
             ORDER BY SUM(cost_usd) DESC",
        )?;

        let by_model: Vec<CostBreakdown> = stmt
            .query_map(params![date_str], |row| {
                Ok(CostBreakdown {
                    model: row.get(0)?,
                    total_cost_usd: row.get(1)?,
                    call_count: row.get(2)?,
                    input_tokens: row.get(3)?,
                    output_tokens: row.get(4)?,
                    cache_creation_tokens: row.get(5)?,
                    cache_read_tokens: row.get(6)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(DailyCost {
            date,
            total_cost_usd,
            call_count,
            total_tokens,
            by_model,
        })
    }

    /// Get monthly costs aggregated.
    pub fn get_monthly_costs(&self, year: i32, month: u32) -> Result<MonthlyCost> {
        let conn = self.db.connection();
        let conn = conn
            .lock()
            .map_err(|e| CostError::Query(format!("failed to acquire lock: {}", e)))?;

        let start_date = format!("{:04}-{:02}-01", year, month);
        let end_date = format!("{:04}-{:02}-31", year, month);

        // Get total for month
        let (total_cost_usd, call_count): (f64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_cost_usd), 0), COALESCE(SUM(call_count), 0)
                 FROM daily_costs
                 WHERE date BETWEEN ?1 AND ?2",
                params![start_date, end_date],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap_or((0.0, 0));

        // Get daily breakdown
        let mut by_day = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT date, total_cost_usd, call_count,
                    total_input_tokens + total_output_tokens +
                    total_cache_creation_tokens + total_cache_read_tokens
             FROM daily_costs
             WHERE date BETWEEN ?1 AND ?2
             ORDER BY date",
        )?;

        for row in stmt
            .query_map(params![start_date, end_date], |row| {
                let date_str: String = row.get(0)?;
                Ok((
                    date_str,
                    row.get::<_, f64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .filter_map(|r| r.ok())
        {
            if let Ok(date) = NaiveDate::parse_from_str(&row.0, "%Y-%m-%d") {
                by_day.push(DailyCost {
                    date,
                    total_cost_usd: row.1,
                    call_count: row.2,
                    total_tokens: row.3,
                    by_model: Vec::new(), // Don't populate per-day model breakdown for monthly view
                });
            }
        }

        // Get model breakdown for month
        let mut stmt = conn.prepare(
            "SELECT model,
                    SUM(cost_usd),
                    SUM(call_count),
                    SUM(input_tokens),
                    SUM(output_tokens),
                    SUM(cache_creation_tokens),
                    SUM(cache_read_tokens)
             FROM model_costs
             WHERE date BETWEEN ?1 AND ?2
             GROUP BY model
             ORDER BY SUM(cost_usd) DESC",
        )?;

        let by_model: Vec<CostBreakdown> = stmt
            .query_map(params![start_date, end_date], |row| {
                Ok(CostBreakdown {
                    model: row.get(0)?,
                    total_cost_usd: row.get(1)?,
                    call_count: row.get(2)?,
                    input_tokens: row.get(3)?,
                    output_tokens: row.get(4)?,
                    cache_creation_tokens: row.get(5)?,
                    cache_read_tokens: row.get(6)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(MonthlyCost {
            year,
            month,
            total_cost_usd,
            call_count,
            by_day,
            by_model,
        })
    }

    /// Get current month's costs.
    pub fn get_current_month_costs(&self) -> Result<MonthlyCost> {
        let now = Utc::now();
        self.get_monthly_costs(now.year(), now.month())
    }

    /// Get cost for a specific bead/task.
    pub fn get_cost_per_task(&self, bead_id: &str) -> Result<CostBreakdown> {
        let conn = self.db.connection();
        let conn = conn
            .lock()
            .map_err(|e| CostError::Query(format!("failed to acquire lock: {}", e)))?;

        let (total_cost_usd, call_count, input_tokens, output_tokens, cache_creation, cache_read):
            (f64, i64, i64, i64, i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(cost_usd), 0),
                        COUNT(*),
                        COALESCE(SUM(input_tokens), 0),
                        COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(cache_creation_tokens), 0),
                        COALESCE(SUM(cache_read_tokens), 0)
                 FROM api_calls
                 WHERE bead_id = ?1",
                params![bead_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            )?;

        // Get the primary model used for this task
        let model: String = conn
            .query_row(
                "SELECT model FROM api_calls WHERE bead_id = ?1 GROUP BY model ORDER BY COUNT(*) DESC LIMIT 1",
                params![bead_id],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| "unknown".to_string());

        Ok(CostBreakdown {
            model,
            call_count,
            input_tokens,
            output_tokens,
            cache_creation_tokens: cache_creation,
            cache_read_tokens: cache_read,
            total_cost_usd,
        })
    }

    /// Get projected costs for the rest of the month.
    pub fn get_projected_costs(&self, days_remaining: Option<i32>) -> Result<ProjectedCost> {
        let now = Utc::now();
        let today = now.date_naive();

        // Calculate days elapsed and remaining in month
        let days_in_month = Self::days_in_month(now.year(), now.month());
        let days_elapsed = today.day() as i32;
        let days_remaining = days_remaining.unwrap_or((days_in_month as i32) - days_elapsed);

        // Get current month's total
        let monthly = self.get_current_month_costs()?;

        Ok(ProjectedCost::calculate(
            monthly.total_cost_usd,
            days_elapsed,
            days_remaining,
        ))
    }

    /// Get model cost statistics.
    pub fn get_model_costs(
        &self,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
    ) -> Result<Vec<ModelCost>> {
        let conn = self.db.connection();
        let conn = conn
            .lock()
            .map_err(|e| CostError::Query(format!("failed to acquire lock: {}", e)))?;

        let start = start_date
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or("1970-01-01".to_string());
        let end = end_date
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or("2100-12-31".to_string());

        let mut stmt = conn.prepare(
            "SELECT model,
                    SUM(cost_usd) as total_cost,
                    SUM(call_count) as calls,
                    AVG(cost_usd / NULLIF(call_count, 0)) as avg_cost,
                    SUM(input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens) as tokens
             FROM model_costs
             WHERE date BETWEEN ?1 AND ?2
             GROUP BY model
             ORDER BY total_cost DESC"
        )?;

        let costs: Vec<ModelCost> = stmt
            .query_map(params![start, end], |row| {
                Ok(ModelCost {
                    model: row.get(0)?,
                    total_cost_usd: row.get(1)?,
                    call_count: row.get(2)?,
                    avg_cost_per_call: row.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
                    total_tokens: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(costs)
    }

    /// Get top spending workers.
    pub fn get_top_workers(&self, limit: usize) -> Result<Vec<(String, f64, i64)>> {
        let conn = self.db.connection();
        let conn = conn
            .lock()
            .map_err(|e| CostError::Query(format!("failed to acquire lock: {}", e)))?;

        let mut stmt = conn.prepare(
            "SELECT worker_id, SUM(cost_usd), COUNT(*)
             FROM api_calls
             GROUP BY worker_id
             ORDER BY SUM(cost_usd) DESC
             LIMIT ?1",
        )?;

        let workers: Vec<(String, f64, i64)> = stmt
            .query_map(params![limit as i64], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(workers)
    }

    /// Get days in a month.
    fn days_in_month(year: i32, month: u32) -> u32 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        }
    }

    // ============ Subscription Query Methods ============

    /// Get all active subscription summaries.
    pub fn get_subscription_summaries(&self) -> Result<Vec<SubscriptionSummary>> {
        let subscriptions = self.db.get_active_subscriptions()?;
        let summaries = subscriptions
            .iter()
            .map(|sub| self.subscription_to_summary(sub))
            .collect();
        Ok(summaries)
    }

    /// Get a specific subscription summary by name.
    pub fn get_subscription_summary(&self, name: &str) -> Result<Option<SubscriptionSummary>> {
        match self.db.get_subscription(name)? {
            Some(sub) => Ok(Some(self.subscription_to_summary(&sub))),
            None => Ok(None),
        }
    }

    /// Convert a subscription to a summary with estimated savings.
    fn subscription_to_summary(&self, sub: &Subscription) -> SubscriptionSummary {
        let mut summary = SubscriptionSummary::from(sub);

        // Calculate estimated savings vs API pricing
        // This is based on typical API costs for the associated model
        if let Some(ref model) = sub.model {
            let estimated_api_cost = self.estimate_api_cost(model, sub.quota_used);
            summary.estimated_savings = (estimated_api_cost - sub.monthly_cost).max(0.0);
        }

        summary
    }

    /// Estimate API cost for given tokens/requests.
    fn estimate_api_cost(&self, model: &str, units: i64) -> f64 {
        // Rough API pricing estimates per message/request
        // These are approximations for cost comparison
        let cost_per_unit = match model.to_lowercase().as_str() {
            m if m.contains("opus") => 0.075,       // ~$75/MTok output avg
            m if m.contains("sonnet") => 0.018,     // ~$18/MTok output avg
            m if m.contains("haiku") => 0.00125,    // ~$1.25/MTok output avg
            m if m.contains("gpt-4") => 0.06,       // ~$60/MTok output avg
            m if m.contains("gpt-3.5") => 0.002,    // ~$2/MTok output avg
            m if m.contains("deepseek") => 0.00028, // ~$0.28/MTok output avg
            m if m.contains("glm") => 0.0,          // Free tier
            _ => 0.02,                              // Default estimate
        };

        // Assume average ~5000 tokens per request for estimation
        let estimated_tokens = units * 5000;
        (estimated_tokens as f64 / 1_000_000.0) * cost_per_unit * 1000.0
    }

    /// Get subscription usage by worker.
    pub fn get_subscription_usage_by_worker(
        &self,
        subscription_name: &str,
    ) -> Result<Vec<(String, i64, f64)>> {
        let conn = self.db.connection();
        let conn = conn
            .lock()
            .map_err(|e| CostError::Query(format!("failed to acquire lock: {}", e)))?;

        // Get subscription ID
        let subscription_id: i64 = conn
            .query_row(
                "SELECT id FROM subscriptions WHERE name = ?1",
                params![subscription_name],
                |row| row.get(0),
            )
            .map_err(|_| {
                CostError::Query(format!("subscription not found: {}", subscription_name))
            })?;

        let mut stmt = conn.prepare(
            "SELECT worker_id, SUM(units), COUNT(*)
             FROM subscription_usage
             WHERE subscription_id = ?1 AND worker_id IS NOT NULL
             GROUP BY worker_id
             ORDER BY SUM(units) DESC",
        )?;

        let usage: Vec<(String, i64, f64)> = stmt
            .query_map(params![subscription_id], |row| {
                let worker_id: String = row.get(0)?;
                let total_units: i64 = row.get(1)?;
                let count: i64 = row.get(2)?;
                let avg = if count > 0 {
                    total_units as f64 / count as f64
                } else {
                    0.0
                };
                Ok((worker_id, total_units, avg))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(usage)
    }

    /// Get subscription usage by bead/task.
    pub fn get_subscription_usage_by_bead(
        &self,
        subscription_name: &str,
    ) -> Result<Vec<(String, i64)>> {
        let conn = self.db.connection();
        let conn = conn
            .lock()
            .map_err(|e| CostError::Query(format!("failed to acquire lock: {}", e)))?;

        // Get subscription ID
        let subscription_id: i64 = conn
            .query_row(
                "SELECT id FROM subscriptions WHERE name = ?1",
                params![subscription_name],
                |row| row.get(0),
            )
            .map_err(|_| {
                CostError::Query(format!("subscription not found: {}", subscription_name))
            })?;

        let mut stmt = conn.prepare(
            "SELECT bead_id, SUM(units)
             FROM subscription_usage
             WHERE subscription_id = ?1 AND bead_id IS NOT NULL
             GROUP BY bead_id
             ORDER BY SUM(units) DESC
             LIMIT 20",
        )?;

        let usage: Vec<(String, i64)> = stmt
            .query_map(params![subscription_id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(usage)
    }

    /// Get daily usage trend for a subscription.
    pub fn get_subscription_daily_trend(
        &self,
        subscription_name: &str,
        days: i32,
    ) -> Result<Vec<(NaiveDate, i64)>> {
        let conn = self.db.connection();
        let conn = conn
            .lock()
            .map_err(|e| CostError::Query(format!("failed to acquire lock: {}", e)))?;

        // Get subscription ID
        let subscription_id: i64 = conn
            .query_row(
                "SELECT id FROM subscriptions WHERE name = ?1",
                params![subscription_name],
                |row| row.get(0),
            )
            .map_err(|_| {
                CostError::Query(format!("subscription not found: {}", subscription_name))
            })?;

        let start_date = Utc::now()
            .date_naive()
            .checked_sub_days(chrono::Days::new(days as u64))
            .unwrap_or(Utc::now().date_naive())
            .format("%Y-%m-%d")
            .to_string();

        let mut stmt = conn.prepare(
            "SELECT DATE(timestamp) as date, SUM(units)
             FROM subscription_usage
             WHERE subscription_id = ?1 AND DATE(timestamp) >= ?2
             GROUP BY DATE(timestamp)
             ORDER BY date",
        )?;

        let trend: Vec<(NaiveDate, i64)> = stmt
            .query_map(params![subscription_id, start_date], |row| {
                let date_str: String = row.get(0)?;
                let date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                    .unwrap_or(Utc::now().date_naive());
                Ok((date, row.get(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(trend)
    }

    /// Get subscriptions that need attention (accelerate or max out).
    pub fn get_subscriptions_needing_attention(&self) -> Result<Vec<SubscriptionSummary>> {
        let summaries = self.get_subscription_summaries()?;
        let needing_attention: Vec<SubscriptionSummary> = summaries
            .into_iter()
            .filter(|s| {
                matches!(
                    s.status,
                    crate::models::QuotaStatus::Accelerate | crate::models::QuotaStatus::MaxOut
                )
            })
            .collect();
        Ok(needing_attention)
    }

    /// Get total subscription costs for current month.
    pub fn get_total_subscription_costs(&self) -> Result<f64> {
        let subscriptions = self.db.get_active_subscriptions()?;
        let total: f64 = subscriptions.iter().map(|s| s.monthly_cost).sum();
        Ok(total)
    }

    /// Get total estimated savings from subscriptions.
    pub fn get_total_subscription_savings(&self) -> Result<f64> {
        let summaries = self.get_subscription_summaries()?;
        let total: f64 = summaries.iter().map(|s| s.estimated_savings).sum();
        Ok(total)
    }

    /// Calculate optimal subscription allocation recommendation.
    pub fn get_subscription_optimization_report(&self) -> Result<SubscriptionOptimizationReport> {
        let subscriptions = self.db.get_active_subscriptions()?;
        let summaries = self.get_subscription_summaries()?;

        let total_monthly_cost: f64 = subscriptions.iter().map(|s| s.monthly_cost).sum();
        let total_quota_used: i64 = subscriptions.iter().map(|s| s.quota_used).sum();
        let total_quota_limit: i64 = subscriptions.iter().filter_map(|s| s.quota_limit).sum();

        let overall_utilization = if total_quota_limit > 0 {
            (total_quota_used as f64 / total_quota_limit as f64) * 100.0
        } else {
            0.0
        };

        let total_savings: f64 = summaries.iter().map(|s| s.estimated_savings).sum();

        // Generate recommendations
        let mut recommendations = Vec::new();

        for summary in &summaries {
            match summary.status {
                crate::models::QuotaStatus::Accelerate => {
                    recommendations.push(format!(
                        "📈 {} is under-utilized ({:.1}%). Consider routing more tasks to maximize value.",
                        summary.name, summary.usage_percentage
                    ));
                }
                crate::models::QuotaStatus::MaxOut => {
                    recommendations.push(format!(
                        "⚡ {} has unused quota ({:.0}/{} remaining). Use before reset in {}.",
                        summary.name,
                        summary.quota_limit.unwrap_or(0) as f64 - summary.quota_used as f64,
                        summary.quota_limit.unwrap_or(0),
                        summary.reset_time
                    ));
                }
                crate::models::QuotaStatus::Depleted => {
                    recommendations.push(format!(
                        "🔴 {} quota exhausted. Consider API fallback or upgrading plan.",
                        summary.name
                    ));
                }
                _ => {}
            }
        }

        if recommendations.is_empty() {
            recommendations
                .push("✅ All subscriptions are on-pace. No immediate action needed.".to_string());
        }

        Ok(SubscriptionOptimizationReport {
            total_monthly_cost,
            total_quota_used,
            total_quota_limit,
            overall_utilization,
            total_estimated_savings: total_savings,
            subscription_count: subscriptions.len(),
            active_count: subscriptions.iter().filter(|s| s.active).count(),
            recommendations,
            subscriptions: summaries,
        })
    }
}

/// Subscription optimization report.
#[derive(Debug, Clone)]
pub struct SubscriptionOptimizationReport {
    /// Total monthly cost of all subscriptions
    pub total_monthly_cost: f64,

    /// Total quota used across all subscriptions
    pub total_quota_used: i64,

    /// Total quota limit across all subscriptions
    pub total_quota_limit: i64,

    /// Overall utilization percentage
    pub overall_utilization: f64,

    /// Total estimated savings vs API pricing
    pub total_estimated_savings: f64,

    /// Number of subscriptions
    pub subscription_count: usize,

    /// Number of active subscriptions
    pub active_count: usize,

    /// Optimization recommendations
    pub recommendations: Vec<String>,

    /// Individual subscription summaries
    pub subscriptions: Vec<SubscriptionSummary>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ApiCall;
    use chrono::Utc;

    #[test]
    fn test_get_today_costs() {
        let db = CostDatabase::open_in_memory().unwrap();

        let calls = vec![
            ApiCall::new(Utc::now(), "worker-1", "claude-opus", 100, 50, 0.10),
            ApiCall::new(Utc::now(), "worker-1", "claude-sonnet", 200, 100, 0.02),
        ];
        db.insert_api_calls(&calls).unwrap();

        let query = CostQuery::new(&db);
        let today = query.get_today_costs().unwrap();

        assert_eq!(today.call_count, 2);
        assert!((today.total_cost_usd - 0.12).abs() < 0.0001);
        assert_eq!(today.by_model.len(), 2);
    }

    #[test]
    fn test_get_monthly_costs() {
        let db = CostDatabase::open_in_memory().unwrap();

        let calls = vec![
            ApiCall::new(Utc::now(), "worker-1", "claude-opus", 100, 50, 1.00),
            ApiCall::new(Utc::now(), "worker-2", "claude-opus", 100, 50, 2.00),
        ];
        db.insert_api_calls(&calls).unwrap();

        let query = CostQuery::new(&db);
        let now = Utc::now();
        let monthly = query.get_monthly_costs(now.year(), now.month()).unwrap();

        assert_eq!(monthly.call_count, 2);
        assert!((monthly.total_cost_usd - 3.00).abs() < 0.0001);
    }

    #[test]
    fn test_get_cost_per_task() {
        let db = CostDatabase::open_in_memory().unwrap();

        let calls = vec![
            ApiCall::new(Utc::now(), "worker-1", "claude-opus", 100, 50, 0.50).with_bead("bd-123"),
            ApiCall::new(Utc::now(), "worker-1", "claude-opus", 200, 100, 1.00).with_bead("bd-123"),
            ApiCall::new(Utc::now(), "worker-2", "claude-sonnet", 50, 25, 0.10).with_bead("bd-456"),
        ];
        db.insert_api_calls(&calls).unwrap();

        let query = CostQuery::new(&db);

        let task_cost = query.get_cost_per_task("bd-123").unwrap();
        assert_eq!(task_cost.call_count, 2);
        assert!((task_cost.total_cost_usd - 1.50).abs() < 0.0001);
        assert_eq!(task_cost.model, "claude-opus");

        let task_cost = query.get_cost_per_task("bd-456").unwrap();
        assert_eq!(task_cost.call_count, 1);
        assert!((task_cost.total_cost_usd - 0.10).abs() < 0.0001);
    }

    #[test]
    fn test_get_projected_costs() {
        let db = CostDatabase::open_in_memory().unwrap();

        let calls = vec![ApiCall::new(
            Utc::now(),
            "worker-1",
            "claude-opus",
            100,
            50,
            10.0,
        )];
        db.insert_api_calls(&calls).unwrap();

        let query = CostQuery::new(&db);
        let projected = query.get_projected_costs(Some(15)).unwrap();

        assert_eq!(projected.current_total, 10.0);
        assert!(projected.projected_total > projected.current_total);
    }

    #[test]
    fn test_get_top_workers() {
        let db = CostDatabase::open_in_memory().unwrap();

        let calls = vec![
            ApiCall::new(
                Utc::now(),
                "expensive-worker",
                "claude-opus",
                100,
                50,
                100.0,
            ),
            ApiCall::new(Utc::now(), "cheap-worker", "claude-haiku", 100, 50, 1.0),
            ApiCall::new(Utc::now(), "medium-worker", "claude-sonnet", 100, 50, 10.0),
        ];
        db.insert_api_calls(&calls).unwrap();

        let query = CostQuery::new(&db);
        let workers = query.get_top_workers(2).unwrap();

        assert_eq!(workers.len(), 2);
        assert_eq!(workers[0].0, "expensive-worker");
        assert_eq!(workers[1].0, "medium-worker");
    }

    #[test]
    fn test_days_in_month() {
        assert_eq!(CostQuery::<'_>::days_in_month(2024, 1), 31);
        assert_eq!(CostQuery::<'_>::days_in_month(2024, 2), 29); // Leap year
        assert_eq!(CostQuery::<'_>::days_in_month(2023, 2), 28); // Not leap year
        assert_eq!(CostQuery::<'_>::days_in_month(2024, 4), 30);
    }

    // ============ Subscription Query Tests ============

    #[test]
    fn test_get_subscription_summaries() {
        use crate::models::{Subscription, SubscriptionType};
        use chrono::Duration;

        let db = CostDatabase::open_in_memory().unwrap();

        let start = Utc::now() - Duration::days(15);
        let end = Utc::now() + Duration::days(15);

        // Add subscriptions
        let mut sub1 =
            Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
                .with_quota(500)
                .with_model("claude-sonnet-4.5");
        sub1.quota_used = 250;

        let mut sub2 = Subscription::new(
            "ChatGPT Plus",
            SubscriptionType::FixedQuota,
            20.0,
            start,
            end,
        )
        .with_quota(100);
        sub2.quota_used = 50;

        db.upsert_subscription(&sub1).unwrap();
        db.upsert_subscription(&sub2).unwrap();

        let query = CostQuery::new(&db);
        let summaries = query.get_subscription_summaries().unwrap();

        assert_eq!(summaries.len(), 2);

        let claude_summary = summaries.iter().find(|s| s.name == "Claude Pro").unwrap();
        assert_eq!(claude_summary.quota_used, 250);
        assert_eq!(claude_summary.quota_limit, Some(500));
        assert!((claude_summary.usage_percentage - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_get_subscription_summary_by_name() {
        use crate::models::{Subscription, SubscriptionType};
        use chrono::Duration;

        let db = CostDatabase::open_in_memory().unwrap();

        let start = Utc::now();
        let end = start + Duration::days(30);

        let sub = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
            .with_quota(500);
        db.upsert_subscription(&sub).unwrap();

        let query = CostQuery::new(&db);

        let summary = query.get_subscription_summary("Claude Pro").unwrap();
        assert!(summary.is_some());
        assert_eq!(summary.unwrap().name, "Claude Pro");

        let not_found = query.get_subscription_summary("NonExistent").unwrap();
        assert!(not_found.is_none());
    }

    #[test]
    fn test_get_subscriptions_needing_attention() {
        use crate::models::{Subscription, SubscriptionType};
        use chrono::Duration;

        let db = CostDatabase::open_in_memory().unwrap();

        // On-pace subscription (50% through, 50% used)
        let start1 = Utc::now() - Duration::days(15);
        let end1 = Utc::now() + Duration::days(15);
        let mut sub1 =
            Subscription::new("On Pace", SubscriptionType::FixedQuota, 20.0, start1, end1)
                .with_quota(500);
        sub1.quota_used = 250;

        // Under-utilized subscription (80% through, only 30% used) - should accelerate
        let start2 = Utc::now() - Duration::days(24);
        let end2 = Utc::now() + Duration::days(6);
        let mut sub2 = Subscription::new(
            "Under Utilized",
            SubscriptionType::FixedQuota,
            20.0,
            start2,
            end2,
        )
        .with_quota(500);
        sub2.quota_used = 150;

        db.upsert_subscription(&sub1).unwrap();
        db.upsert_subscription(&sub2).unwrap();

        let query = CostQuery::new(&db);
        let needing_attention = query.get_subscriptions_needing_attention().unwrap();

        // Under-utilized should need attention
        assert!(needing_attention.len() >= 1);
        assert!(needing_attention.iter().any(|s| s.name == "Under Utilized"));
    }

    #[test]
    fn test_get_total_subscription_costs() {
        use crate::models::{Subscription, SubscriptionType};
        use chrono::Duration;

        let db = CostDatabase::open_in_memory().unwrap();

        let start = Utc::now();
        let end = start + Duration::days(30);

        let sub1 = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end);
        let sub2 = Subscription::new(
            "ChatGPT Plus",
            SubscriptionType::FixedQuota,
            20.0,
            start,
            end,
        );
        let sub3 = Subscription::new("Cursor Pro", SubscriptionType::FixedQuota, 20.0, start, end);

        db.upsert_subscription(&sub1).unwrap();
        db.upsert_subscription(&sub2).unwrap();
        db.upsert_subscription(&sub3).unwrap();

        let query = CostQuery::new(&db);
        let total = query.get_total_subscription_costs().unwrap();

        assert!((total - 60.0).abs() < 0.001);
    }

    #[test]
    fn test_get_subscription_optimization_report() {
        use crate::models::{Subscription, SubscriptionType};
        use chrono::Duration;

        let db = CostDatabase::open_in_memory().unwrap();

        let start = Utc::now() - Duration::days(15);
        let end = Utc::now() + Duration::days(15);

        let mut sub1 =
            Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
                .with_quota(500)
                .with_model("claude-sonnet-4.5");
        sub1.quota_used = 250;

        let mut sub2 = Subscription::new(
            "ChatGPT Plus",
            SubscriptionType::FixedQuota,
            20.0,
            start,
            end,
        )
        .with_quota(100);
        sub2.quota_used = 50;

        db.upsert_subscription(&sub1).unwrap();
        db.upsert_subscription(&sub2).unwrap();

        let query = CostQuery::new(&db);
        let report = query.get_subscription_optimization_report().unwrap();

        assert!((report.total_monthly_cost - 40.0).abs() < 0.001);
        assert_eq!(report.total_quota_used, 300); // 250 + 50
        assert_eq!(report.total_quota_limit, 600); // 500 + 100
        assert_eq!(report.subscription_count, 2);
        assert_eq!(report.active_count, 2);
        assert!(!report.recommendations.is_empty());
        assert_eq!(report.subscriptions.len(), 2);
    }

    #[test]
    fn test_estimate_api_cost() {
        let db = CostDatabase::open_in_memory().unwrap();
        let query = CostQuery::new(&db);

        // Test various models
        let opus_cost = query.estimate_api_cost("claude-opus-4.5", 100);
        let sonnet_cost = query.estimate_api_cost("claude-sonnet-4.5", 100);
        let haiku_cost = query.estimate_api_cost("claude-haiku-3", 100);
        let glm_cost = query.estimate_api_cost("glm-4.7", 100);

        // Opus should be most expensive
        assert!(opus_cost > sonnet_cost);
        assert!(sonnet_cost > haiku_cost);
        // GLM is free
        assert_eq!(glm_cost, 0.0);
    }
}
