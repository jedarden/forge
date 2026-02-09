//! Query functions for cost analysis.

use crate::db::CostDatabase;
use crate::error::{CostError, Result};
use crate::models::{CostBreakdown, DailyCost, ModelCost, MonthlyCost, ProjectedCost};
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
        let conn = conn.lock().map_err(|e| {
            CostError::Query(format!("failed to acquire lock: {}", e))
        })?;

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
             ORDER BY SUM(cost_usd) DESC"
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
        let conn = conn.lock().map_err(|e| {
            CostError::Query(format!("failed to acquire lock: {}", e))
        })?;

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
             ORDER BY date"
        )?;

        for row in stmt.query_map(params![start_date, end_date], |row| {
            let date_str: String = row.get(0)?;
            Ok((date_str, row.get::<_, f64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?))
        })?.filter_map(|r| r.ok()) {
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
             ORDER BY SUM(cost_usd) DESC"
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
        let conn = conn.lock().map_err(|e| {
            CostError::Query(format!("failed to acquire lock: {}", e))
        })?;

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
    pub fn get_model_costs(&self, start_date: Option<NaiveDate>, end_date: Option<NaiveDate>) -> Result<Vec<ModelCost>> {
        let conn = self.db.connection();
        let conn = conn.lock().map_err(|e| {
            CostError::Query(format!("failed to acquire lock: {}", e))
        })?;

        let start = start_date.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or("1970-01-01".to_string());
        let end = end_date.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or("2100-12-31".to_string());

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
        let conn = conn.lock().map_err(|e| {
            CostError::Query(format!("failed to acquire lock: {}", e))
        })?;

        let mut stmt = conn.prepare(
            "SELECT worker_id, SUM(cost_usd), COUNT(*)
             FROM api_calls
             GROUP BY worker_id
             ORDER BY SUM(cost_usd) DESC
             LIMIT ?1"
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
            ApiCall::new(Utc::now(), "worker-1", "claude-opus", 100, 50, 0.50)
                .with_bead("bd-123"),
            ApiCall::new(Utc::now(), "worker-1", "claude-opus", 200, 100, 1.00)
                .with_bead("bd-123"),
            ApiCall::new(Utc::now(), "worker-2", "claude-sonnet", 50, 25, 0.10)
                .with_bead("bd-456"),
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

        let calls = vec![
            ApiCall::new(Utc::now(), "worker-1", "claude-opus", 100, 50, 10.0),
        ];
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
            ApiCall::new(Utc::now(), "expensive-worker", "claude-opus", 100, 50, 100.0),
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
}
