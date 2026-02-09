//! SQLite database layer for cost tracking.

use crate::error::{CostError, Result};
use crate::models::{ApiCall, CostBreakdown, DailyCost, Subscription, SubscriptionType, SubscriptionUsageRecord};
use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{params, Connection, Transaction};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

/// Current schema version for migrations.
const SCHEMA_VERSION: i32 = 2;

/// SQLite database for cost tracking.
pub struct CostDatabase {
    conn: Arc<Mutex<Connection>>,
}

impl CostDatabase {
    /// Open or create a cost database at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Create an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Run database migrations.
    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Migration(format!("failed to acquire lock: {}", e))
        })?;

        // Create schema version table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY
            )",
            [],
        )?;

        // Get current version
        let current_version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if current_version < SCHEMA_VERSION {
            info!(
                current = current_version,
                target = SCHEMA_VERSION,
                "Running database migrations"
            );
            self.run_migrations(&conn, current_version)?;
        }

        Ok(())
    }

    /// Run migrations from current version to target.
    fn run_migrations(&self, conn: &Connection, from_version: i32) -> Result<()> {
        if from_version < 1 {
            self.migration_v1(conn)?;
        }
        if from_version < 2 {
            self.migration_v2(conn)?;
        }

        Ok(())
    }

    /// Migration to version 1: initial schema.
    fn migration_v1(&self, conn: &Connection) -> Result<()> {
        debug!("Running migration v1: initial schema");

        // Main API calls table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS api_calls (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                worker_id TEXT NOT NULL,
                session_id TEXT,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cost_usd REAL NOT NULL,
                bead_id TEXT,
                event_type TEXT NOT NULL DEFAULT 'result'
            )",
            [],
        )?;

        // Indexes for efficient queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_api_calls_timestamp
             ON api_calls(timestamp)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_api_calls_worker
             ON api_calls(worker_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_api_calls_model
             ON api_calls(model)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_api_calls_bead
             ON api_calls(bead_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_api_calls_date
             ON api_calls(DATE(timestamp))",
            [],
        )?;

        // Daily aggregation table (materialized view pattern)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS daily_costs (
                date TEXT PRIMARY KEY,
                total_cost_usd REAL NOT NULL,
                call_count INTEGER NOT NULL,
                total_input_tokens INTEGER NOT NULL,
                total_output_tokens INTEGER NOT NULL,
                total_cache_creation_tokens INTEGER NOT NULL,
                total_cache_read_tokens INTEGER NOT NULL,
                last_updated TEXT NOT NULL
            )",
            [],
        )?;

        // Model aggregation table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS model_costs (
                date TEXT NOT NULL,
                model TEXT NOT NULL,
                cost_usd REAL NOT NULL,
                call_count INTEGER NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                cache_creation_tokens INTEGER NOT NULL,
                cache_read_tokens INTEGER NOT NULL,
                PRIMARY KEY (date, model)
            )",
            [],
        )?;

        // Record migration
        conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])?;

        info!("Migration v1 completed");
        Ok(())
    }

    /// Migration to version 2: subscription tracking tables.
    fn migration_v2(&self, conn: &Connection) -> Result<()> {
        debug!("Running migration v2: subscription tracking");

        // Subscriptions table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS subscriptions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                model TEXT,
                subscription_type TEXT NOT NULL DEFAULT 'fixed_quota',
                monthly_cost REAL NOT NULL DEFAULT 0,
                quota_limit INTEGER,
                quota_used INTEGER NOT NULL DEFAULT 0,
                billing_start TEXT NOT NULL,
                billing_end TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Subscription usage records table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS subscription_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                subscription_id INTEGER NOT NULL,
                timestamp TEXT NOT NULL,
                units INTEGER NOT NULL,
                worker_id TEXT,
                bead_id TEXT,
                api_call_id INTEGER,
                FOREIGN KEY (subscription_id) REFERENCES subscriptions(id),
                FOREIGN KEY (api_call_id) REFERENCES api_calls(id)
            )",
            [],
        )?;

        // Indexes for efficient queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_subscription_usage_subscription
             ON subscription_usage(subscription_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_subscription_usage_timestamp
             ON subscription_usage(timestamp)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_subscription_usage_worker
             ON subscription_usage(worker_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_subscriptions_active
             ON subscriptions(active)",
            [],
        )?;

        // Record migration
        conn.execute("INSERT INTO schema_version (version) VALUES (2)", [])?;

        info!("Migration v2 completed: subscription tracking");
        Ok(())
    }

    /// Insert a batch of API calls efficiently.
    pub fn insert_api_calls(&self, calls: &[ApiCall]) -> Result<usize> {
        if calls.is_empty() {
            return Ok(0);
        }

        let mut conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let tx = conn.transaction()?;
        let count = self.insert_calls_in_tx(&tx, calls)?;
        tx.commit()?;

        debug!(count, "Inserted API calls");
        Ok(count)
    }

    /// Insert calls within a transaction.
    fn insert_calls_in_tx(&self, tx: &Transaction, calls: &[ApiCall]) -> Result<usize> {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO api_calls
             (timestamp, worker_id, session_id, model, input_tokens, output_tokens,
              cache_creation_tokens, cache_read_tokens, cost_usd, bead_id, event_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
        )?;

        let mut count = 0;
        for call in calls {
            stmt.execute(params![
                call.timestamp.to_rfc3339(),
                call.worker_id,
                call.session_id,
                call.model,
                call.input_tokens,
                call.output_tokens,
                call.cache_creation_tokens,
                call.cache_read_tokens,
                call.cost_usd,
                call.bead_id,
                call.event_type,
            ])?;
            count += 1;
        }

        // Update aggregation tables
        self.update_aggregations(tx, calls)?;

        Ok(count)
    }

    /// Update aggregation tables after inserting calls.
    fn update_aggregations(&self, tx: &Transaction, calls: &[ApiCall]) -> Result<()> {
        // Group calls by date and model
        use std::collections::HashMap;
        let mut by_date_model: HashMap<(String, String), CostBreakdown> = HashMap::new();

        for call in calls {
            let date = call.timestamp.format("%Y-%m-%d").to_string();
            let key = (date, call.model.clone());

            let entry = by_date_model.entry(key).or_insert_with(|| CostBreakdown {
                model: call.model.clone(),
                ..Default::default()
            });

            entry.call_count += 1;
            entry.input_tokens += call.input_tokens;
            entry.output_tokens += call.output_tokens;
            entry.cache_creation_tokens += call.cache_creation_tokens;
            entry.cache_read_tokens += call.cache_read_tokens;
            entry.total_cost_usd += call.cost_usd;
        }

        // Upsert into model_costs
        let mut stmt = tx.prepare_cached(
            "INSERT INTO model_costs
             (date, model, cost_usd, call_count, input_tokens, output_tokens,
              cache_creation_tokens, cache_read_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(date, model) DO UPDATE SET
                cost_usd = cost_usd + excluded.cost_usd,
                call_count = call_count + excluded.call_count,
                input_tokens = input_tokens + excluded.input_tokens,
                output_tokens = output_tokens + excluded.output_tokens,
                cache_creation_tokens = cache_creation_tokens + excluded.cache_creation_tokens,
                cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens"
        )?;

        for ((date, _), breakdown) in &by_date_model {
            stmt.execute(params![
                date,
                breakdown.model,
                breakdown.total_cost_usd,
                breakdown.call_count,
                breakdown.input_tokens,
                breakdown.output_tokens,
                breakdown.cache_creation_tokens,
                breakdown.cache_read_tokens,
            ])?;
        }

        // Aggregate by date for daily_costs
        let mut by_date: HashMap<String, (f64, i64, i64, i64, i64, i64)> = HashMap::new();
        for ((date, _), breakdown) in by_date_model {
            let entry = by_date.entry(date).or_insert((0.0, 0, 0, 0, 0, 0));
            entry.0 += breakdown.total_cost_usd;
            entry.1 += breakdown.call_count;
            entry.2 += breakdown.input_tokens;
            entry.3 += breakdown.output_tokens;
            entry.4 += breakdown.cache_creation_tokens;
            entry.5 += breakdown.cache_read_tokens;
        }

        let mut daily_stmt = tx.prepare_cached(
            "INSERT INTO daily_costs
             (date, total_cost_usd, call_count, total_input_tokens, total_output_tokens,
              total_cache_creation_tokens, total_cache_read_tokens, last_updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(date) DO UPDATE SET
                total_cost_usd = total_cost_usd + excluded.total_cost_usd,
                call_count = call_count + excluded.call_count,
                total_input_tokens = total_input_tokens + excluded.total_input_tokens,
                total_output_tokens = total_output_tokens + excluded.total_output_tokens,
                total_cache_creation_tokens = total_cache_creation_tokens + excluded.total_cache_creation_tokens,
                total_cache_read_tokens = total_cache_read_tokens + excluded.total_cache_read_tokens,
                last_updated = excluded.last_updated"
        )?;

        let now = Utc::now().to_rfc3339();
        for (date, (cost, count, input, output, cache_create, cache_read)) in by_date {
            daily_stmt.execute(params![
                date,
                cost,
                count,
                input,
                output,
                cache_create,
                cache_read,
                now,
            ])?;
        }

        Ok(())
    }

    /// Get daily costs from the aggregation table.
    pub fn get_daily_cost(&self, date: NaiveDate) -> Result<Option<DailyCost>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let date_str = date.format("%Y-%m-%d").to_string();

        // Get daily aggregate
        let daily_result: Option<(f64, i64, i64)> = conn
            .query_row(
                "SELECT total_cost_usd, call_count,
                        total_input_tokens + total_output_tokens +
                        total_cache_creation_tokens + total_cache_read_tokens
                 FROM daily_costs WHERE date = ?1",
                params![date_str],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok();

        let (total_cost_usd, call_count, total_tokens) = match daily_result {
            Some(r) => r,
            None => return Ok(None),
        };

        // Get model breakdown
        let mut stmt = conn.prepare(
            "SELECT model, cost_usd, call_count, input_tokens, output_tokens,
                    cache_creation_tokens, cache_read_tokens
             FROM model_costs WHERE date = ?1"
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

        Ok(Some(DailyCost {
            date,
            total_cost_usd,
            call_count,
            total_tokens,
            by_model,
        }))
    }

    /// Get the last processed timestamp for a worker.
    pub fn get_last_timestamp(&self, worker_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let result = conn
            .query_row(
                "SELECT MAX(timestamp) FROM api_calls WHERE worker_id = ?1",
                params![worker_id],
                |row| row.get(0),
            )
            .ok();

        Ok(result)
    }

    /// Check if an API call already exists (for deduplication).
    pub fn exists(&self, worker_id: &str, timestamp: &str, session_id: Option<&str>) -> Result<bool> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM api_calls
             WHERE worker_id = ?1 AND timestamp = ?2 AND session_id IS ?3",
            params![worker_id, timestamp, session_id],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    /// Get raw database connection for advanced queries.
    pub fn connection(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.conn)
    }

    // ============ Subscription Methods ============

    /// Insert or update a subscription.
    pub fn upsert_subscription(&self, subscription: &Subscription) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let subscription_type = match subscription.subscription_type {
            SubscriptionType::FixedQuota => "fixed_quota",
            SubscriptionType::Unlimited => "unlimited",
            SubscriptionType::PayPerUse => "pay_per_use",
        };

        conn.execute(
            "INSERT INTO subscriptions
             (name, model, subscription_type, monthly_cost, quota_limit, quota_used,
              billing_start, billing_end, active, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(name) DO UPDATE SET
                model = excluded.model,
                subscription_type = excluded.subscription_type,
                monthly_cost = excluded.monthly_cost,
                quota_limit = excluded.quota_limit,
                quota_used = excluded.quota_used,
                billing_start = excluded.billing_start,
                billing_end = excluded.billing_end,
                active = excluded.active,
                updated_at = excluded.updated_at",
            params![
                subscription.name,
                subscription.model,
                subscription_type,
                subscription.monthly_cost,
                subscription.quota_limit,
                subscription.quota_used,
                subscription.billing_start.to_rfc3339(),
                subscription.billing_end.to_rfc3339(),
                subscription.active,
                Utc::now().to_rfc3339(),
            ],
        )?;

        let id = conn.last_insert_rowid();
        debug!(id, name = %subscription.name, "Upserted subscription");
        Ok(id)
    }

    /// Get a subscription by name.
    pub fn get_subscription(&self, name: &str) -> Result<Option<Subscription>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let result = conn.query_row(
            "SELECT id, name, model, subscription_type, monthly_cost, quota_limit,
                    quota_used, billing_start, billing_end, active, updated_at
             FROM subscriptions WHERE name = ?1",
            params![name],
            |row| {
                Ok(Self::row_to_subscription(row)?)
            },
        );

        match result {
            Ok(sub) => Ok(Some(sub)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CostError::Database(e)),
        }
    }

    /// Get all active subscriptions.
    pub fn get_active_subscriptions(&self) -> Result<Vec<Subscription>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let mut stmt = conn.prepare(
            "SELECT id, name, model, subscription_type, monthly_cost, quota_limit,
                    quota_used, billing_start, billing_end, active, updated_at
             FROM subscriptions WHERE active = 1
             ORDER BY name"
        )?;

        let subscriptions: Vec<Subscription> = stmt
            .query_map([], |row| Self::row_to_subscription(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(subscriptions)
    }

    /// Get all subscriptions (including inactive).
    pub fn get_all_subscriptions(&self) -> Result<Vec<Subscription>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let mut stmt = conn.prepare(
            "SELECT id, name, model, subscription_type, monthly_cost, quota_limit,
                    quota_used, billing_start, billing_end, active, updated_at
             FROM subscriptions ORDER BY active DESC, name"
        )?;

        let subscriptions: Vec<Subscription> = stmt
            .query_map([], |row| Self::row_to_subscription(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(subscriptions)
    }

    /// Update subscription quota usage.
    pub fn update_subscription_usage(&self, name: &str, quota_used: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        conn.execute(
            "UPDATE subscriptions SET quota_used = ?1, updated_at = ?2 WHERE name = ?3",
            params![quota_used, Utc::now().to_rfc3339(), name],
        )?;

        debug!(name, quota_used, "Updated subscription usage");
        Ok(())
    }

    /// Increment subscription quota usage.
    pub fn increment_subscription_usage(&self, name: &str, units: i64) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        conn.execute(
            "UPDATE subscriptions
             SET quota_used = quota_used + ?1, updated_at = ?2
             WHERE name = ?3",
            params![units, Utc::now().to_rfc3339(), name],
        )?;

        // Get the new quota_used value
        let new_usage: i64 = conn.query_row(
            "SELECT quota_used FROM subscriptions WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )?;

        debug!(name, units, new_usage, "Incremented subscription usage");
        Ok(new_usage)
    }

    /// Reset subscription billing period.
    pub fn reset_subscription_billing(&self, name: &str, new_start: DateTime<Utc>, new_end: DateTime<Utc>) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        conn.execute(
            "UPDATE subscriptions
             SET quota_used = 0, billing_start = ?1, billing_end = ?2, updated_at = ?3
             WHERE name = ?4",
            params![
                new_start.to_rfc3339(),
                new_end.to_rfc3339(),
                Utc::now().to_rfc3339(),
                name,
            ],
        )?;

        info!(name, "Reset subscription billing period");
        Ok(())
    }

    /// Record subscription usage.
    pub fn record_subscription_usage(&self, record: &SubscriptionUsageRecord) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        conn.execute(
            "INSERT INTO subscription_usage
             (subscription_id, timestamp, units, worker_id, bead_id, api_call_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.subscription_id,
                record.timestamp.to_rfc3339(),
                record.units,
                record.worker_id,
                record.bead_id,
                record.api_call_id,
            ],
        )?;

        let id = conn.last_insert_rowid();
        debug!(id, subscription_id = record.subscription_id, units = record.units, "Recorded usage");
        Ok(id)
    }

    /// Get usage records for a subscription within a date range.
    pub fn get_subscription_usage(&self, subscription_id: i64, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<SubscriptionUsageRecord>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let mut stmt = conn.prepare(
            "SELECT id, subscription_id, timestamp, units, worker_id, bead_id, api_call_id
             FROM subscription_usage
             WHERE subscription_id = ?1 AND timestamp BETWEEN ?2 AND ?3
             ORDER BY timestamp DESC"
        )?;

        let records: Vec<SubscriptionUsageRecord> = stmt
            .query_map(
                params![subscription_id, start.to_rfc3339(), end.to_rfc3339()],
                |row| {
                    let timestamp_str: String = row.get(2)?;
                    let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());

                    Ok(SubscriptionUsageRecord {
                        id: Some(row.get(0)?),
                        subscription_id: row.get(1)?,
                        timestamp,
                        units: row.get(3)?,
                        worker_id: row.get(4)?,
                        bead_id: row.get(5)?,
                        api_call_id: row.get(6)?,
                    })
                },
            )?
            .filter_map(|r| r.ok())
            .collect();

        Ok(records)
    }

    /// Get total usage for a subscription in current billing period.
    pub fn get_subscription_period_usage(&self, subscription_id: i64) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        // Get the billing period from subscriptions table
        let (billing_start, billing_end): (String, String) = conn.query_row(
            "SELECT billing_start, billing_end FROM subscriptions WHERE id = ?1",
            params![subscription_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        // Sum usage in the billing period
        let total: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(units), 0) FROM subscription_usage
                 WHERE subscription_id = ?1 AND timestamp BETWEEN ?2 AND ?3",
                params![subscription_id, billing_start, billing_end],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(total)
    }

    /// Delete a subscription (soft delete - sets active = 0).
    pub fn deactivate_subscription(&self, name: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        conn.execute(
            "UPDATE subscriptions SET active = 0, updated_at = ?1 WHERE name = ?2",
            params![Utc::now().to_rfc3339(), name],
        )?;

        info!(name, "Deactivated subscription");
        Ok(())
    }

    /// Helper to convert a database row to a Subscription.
    fn row_to_subscription(row: &rusqlite::Row) -> rusqlite::Result<Subscription> {
        let type_str: String = row.get(3)?;
        let subscription_type = match type_str.as_str() {
            "unlimited" => SubscriptionType::Unlimited,
            "pay_per_use" => SubscriptionType::PayPerUse,
            _ => SubscriptionType::FixedQuota,
        };

        let billing_start_str: String = row.get(7)?;
        let billing_end_str: String = row.get(8)?;
        let updated_at_str: String = row.get(10)?;

        let billing_start = DateTime::parse_from_rfc3339(&billing_start_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let billing_end = DateTime::parse_from_rfc3339(&billing_end_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        Ok(Subscription {
            id: Some(row.get(0)?),
            name: row.get(1)?,
            model: row.get(2)?,
            subscription_type,
            monthly_cost: row.get(4)?,
            quota_limit: row.get(5)?,
            quota_used: row.get(6)?,
            billing_start,
            billing_end,
            active: row.get::<_, i64>(9)? != 0,
            updated_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_open_in_memory() {
        let db = CostDatabase::open_in_memory().expect("Failed to open in-memory db");
        // Verify tables exist
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'").unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"api_calls".to_string()));
        assert!(tables.contains(&"daily_costs".to_string()));
        assert!(tables.contains(&"model_costs".to_string()));
    }

    #[test]
    fn test_insert_and_query() {
        let db = CostDatabase::open_in_memory().unwrap();

        let calls = vec![
            ApiCall::new(Utc::now(), "worker-1", "claude-opus", 100, 50, 0.01),
            ApiCall::new(Utc::now(), "worker-1", "claude-opus", 200, 100, 0.02),
        ];

        let count = db.insert_api_calls(&calls).unwrap();
        assert_eq!(count, 2);

        // Verify in daily_costs
        let today = Utc::now().date_naive();
        let daily = db.get_daily_cost(today).unwrap().expect("Should have daily cost");
        assert_eq!(daily.call_count, 2);
        assert!((daily.total_cost_usd - 0.03).abs() < 0.0001);
    }

    #[test]
    fn test_aggregation_by_model() {
        let db = CostDatabase::open_in_memory().unwrap();

        let calls = vec![
            ApiCall::new(Utc::now(), "worker-1", "claude-opus", 100, 50, 0.10),
            ApiCall::new(Utc::now(), "worker-1", "claude-sonnet", 200, 100, 0.02),
            ApiCall::new(Utc::now(), "worker-2", "claude-opus", 150, 75, 0.15),
        ];

        db.insert_api_calls(&calls).unwrap();

        let today = Utc::now().date_naive();
        let daily = db.get_daily_cost(today).unwrap().unwrap();

        assert_eq!(daily.by_model.len(), 2);

        let opus = daily.by_model.iter().find(|m| m.model == "claude-opus").unwrap();
        assert_eq!(opus.call_count, 2);
        assert!((opus.total_cost_usd - 0.25).abs() < 0.0001);
    }

    #[test]
    fn test_deduplication_check() {
        let db = CostDatabase::open_in_memory().unwrap();
        let now = Utc::now();
        let ts = now.to_rfc3339();

        let call = ApiCall::new(now, "worker-1", "claude-opus", 100, 50, 0.01)
            .with_session("session-123");

        db.insert_api_calls(&[call]).unwrap();

        assert!(db.exists("worker-1", &ts, Some("session-123")).unwrap());
        assert!(!db.exists("worker-1", &ts, Some("session-456")).unwrap());
        assert!(!db.exists("worker-2", &ts, Some("session-123")).unwrap());
    }

    // ============ Subscription Tests ============

    #[test]
    fn test_upsert_subscription() {
        use chrono::Duration;

        let db = CostDatabase::open_in_memory().unwrap();

        let start = Utc::now();
        let end = start + Duration::days(30);

        let sub = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
            .with_quota(500)
            .with_model("claude-sonnet-4.5");

        let id = db.upsert_subscription(&sub).unwrap();
        assert!(id > 0);

        // Verify insertion
        let loaded = db.get_subscription("Claude Pro").unwrap().unwrap();
        assert_eq!(loaded.name, "Claude Pro");
        assert_eq!(loaded.monthly_cost, 20.0);
        assert_eq!(loaded.quota_limit, Some(500));
        assert_eq!(loaded.model, Some("claude-sonnet-4.5".to_string()));
    }

    #[test]
    fn test_upsert_subscription_update() {
        use chrono::Duration;

        let db = CostDatabase::open_in_memory().unwrap();

        let start = Utc::now();
        let end = start + Duration::days(30);

        // Insert
        let sub = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
            .with_quota(500);
        db.upsert_subscription(&sub).unwrap();

        // Update with new values
        let mut sub2 = sub.clone();
        sub2.monthly_cost = 25.0;
        sub2.quota_limit = Some(750);
        db.upsert_subscription(&sub2).unwrap();

        // Verify update
        let loaded = db.get_subscription("Claude Pro").unwrap().unwrap();
        assert_eq!(loaded.monthly_cost, 25.0);
        assert_eq!(loaded.quota_limit, Some(750));
    }

    #[test]
    fn test_get_active_subscriptions() {
        use chrono::Duration;

        let db = CostDatabase::open_in_memory().unwrap();

        let start = Utc::now();
        let end = start + Duration::days(30);

        // Add multiple subscriptions
        let sub1 = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end);
        let sub2 = Subscription::new("ChatGPT Plus", SubscriptionType::FixedQuota, 20.0, start, end);
        let mut sub3 = Subscription::new("Cursor Pro", SubscriptionType::FixedQuota, 20.0, start, end);
        sub3.active = false;

        db.upsert_subscription(&sub1).unwrap();
        db.upsert_subscription(&sub2).unwrap();
        db.upsert_subscription(&sub3).unwrap();

        // Get active only
        let active = db.get_active_subscriptions().unwrap();
        assert_eq!(active.len(), 2);

        // Get all
        let all = db.get_all_subscriptions().unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_update_subscription_usage() {
        use chrono::Duration;

        let db = CostDatabase::open_in_memory().unwrap();

        let start = Utc::now();
        let end = start + Duration::days(30);

        let sub = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
            .with_quota(500);
        db.upsert_subscription(&sub).unwrap();

        // Update usage
        db.update_subscription_usage("Claude Pro", 250).unwrap();

        let loaded = db.get_subscription("Claude Pro").unwrap().unwrap();
        assert_eq!(loaded.quota_used, 250);
    }

    #[test]
    fn test_increment_subscription_usage() {
        use chrono::Duration;

        let db = CostDatabase::open_in_memory().unwrap();

        let start = Utc::now();
        let end = start + Duration::days(30);

        let sub = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
            .with_quota(500);
        db.upsert_subscription(&sub).unwrap();

        // Increment usage
        let new_usage = db.increment_subscription_usage("Claude Pro", 5).unwrap();
        assert_eq!(new_usage, 5);

        let new_usage = db.increment_subscription_usage("Claude Pro", 3).unwrap();
        assert_eq!(new_usage, 8);

        let loaded = db.get_subscription("Claude Pro").unwrap().unwrap();
        assert_eq!(loaded.quota_used, 8);
    }

    #[test]
    fn test_record_subscription_usage() {
        use chrono::Duration;

        let db = CostDatabase::open_in_memory().unwrap();

        let start = Utc::now();
        let end = start + Duration::days(30);

        let sub = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
            .with_quota(500);
        let sub_id = db.upsert_subscription(&sub).unwrap();

        // Record usage
        let record = SubscriptionUsageRecord::new(sub_id, 5)
            .with_worker("glm-alpha")
            .with_bead("bd-123");
        let usage_id = db.record_subscription_usage(&record).unwrap();
        assert!(usage_id > 0);

        // Query usage
        let records = db.get_subscription_usage(sub_id, start - Duration::days(1), end).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].units, 5);
        assert_eq!(records[0].worker_id, Some("glm-alpha".to_string()));
    }

    #[test]
    fn test_reset_subscription_billing() {
        use chrono::Duration;

        let db = CostDatabase::open_in_memory().unwrap();

        let start = Utc::now() - Duration::days(30);
        let end = Utc::now();

        let mut sub = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
            .with_quota(500);
        sub.quota_used = 450;
        db.upsert_subscription(&sub).unwrap();

        // Reset billing period
        let new_start = Utc::now();
        let new_end = new_start + Duration::days(30);
        db.reset_subscription_billing("Claude Pro", new_start, new_end).unwrap();

        let loaded = db.get_subscription("Claude Pro").unwrap().unwrap();
        assert_eq!(loaded.quota_used, 0);
        assert!(loaded.billing_end > Utc::now());
    }

    #[test]
    fn test_deactivate_subscription() {
        use chrono::Duration;

        let db = CostDatabase::open_in_memory().unwrap();

        let start = Utc::now();
        let end = start + Duration::days(30);

        let sub = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end);
        db.upsert_subscription(&sub).unwrap();

        // Verify active
        let active = db.get_active_subscriptions().unwrap();
        assert_eq!(active.len(), 1);

        // Deactivate
        db.deactivate_subscription("Claude Pro").unwrap();

        // Verify deactivated
        let active = db.get_active_subscriptions().unwrap();
        assert_eq!(active.len(), 0);

        let loaded = db.get_subscription("Claude Pro").unwrap().unwrap();
        assert!(!loaded.active);
    }

    #[test]
    fn test_subscription_type_round_trip() {
        use chrono::Duration;

        let db = CostDatabase::open_in_memory().unwrap();

        let start = Utc::now();
        let end = start + Duration::days(30);

        // Test all subscription types
        let sub1 = Subscription::new("Fixed", SubscriptionType::FixedQuota, 20.0, start, end);
        let sub2 = Subscription::new("Unlimited", SubscriptionType::Unlimited, 100.0, start, end);
        let sub3 = Subscription::new("PayPerUse", SubscriptionType::PayPerUse, 0.0, start, end);

        db.upsert_subscription(&sub1).unwrap();
        db.upsert_subscription(&sub2).unwrap();
        db.upsert_subscription(&sub3).unwrap();

        let loaded1 = db.get_subscription("Fixed").unwrap().unwrap();
        let loaded2 = db.get_subscription("Unlimited").unwrap().unwrap();
        let loaded3 = db.get_subscription("PayPerUse").unwrap().unwrap();

        assert_eq!(loaded1.subscription_type, SubscriptionType::FixedQuota);
        assert_eq!(loaded2.subscription_type, SubscriptionType::Unlimited);
        assert_eq!(loaded3.subscription_type, SubscriptionType::PayPerUse);
    }
}
