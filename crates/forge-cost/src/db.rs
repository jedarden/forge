//! SQLite database layer for cost tracking.

use crate::error::{is_database_locked_error, CostError, Result};
use crate::models::{
    ApiCall, CostBreakdown, DailyCost, DailyStat, HourlyStat, ModelPerformance, Subscription,
    SubscriptionType, SubscriptionUsageRecord, WorkerEfficiency,
};
use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{Connection, Transaction, params};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{debug, info, warn};

/// Current schema version for migrations.
const SCHEMA_VERSION: i32 = 3;

/// Maximum retries for database lock errors.
const DB_LOCK_MAX_RETRIES: u32 = 5;

/// Initial delay for database lock retry (in milliseconds).
const DB_LOCK_INITIAL_DELAY_MS: u64 = 50;

/// Maximum delay for database lock retry.
const DB_LOCK_MAX_DELAY: Duration = Duration::from_secs(5);

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

    /// Execute a database operation with automatic retry on lock errors.
    ///
    /// This helper wraps database operations that may fail due to concurrent
    /// access, automatically retrying with exponential backoff.
    fn with_retry<T, F>(&self, operation: &str, mut f: F) -> Result<T>
    where
        F: FnMut() -> Result<T>,
    {
        let mut attempt = 0;
        let mut delay = Duration::from_millis(DB_LOCK_INITIAL_DELAY_MS);

        loop {
            attempt += 1;

            match f() {
                Ok(result) => {
                    if attempt > 1 {
                        info!(
                            attempt,
                            operation,
                            "Database operation succeeded after retry"
                        );
                    }
                    return Ok(result);
                }
                Err(ref e) if is_database_locked_error(e) && attempt <= DB_LOCK_MAX_RETRIES => {
                    warn!(
                        attempt,
                        max_retries = DB_LOCK_MAX_RETRIES,
                        delay_ms = delay.as_millis(),
                        operation,
                        "Database locked, retrying with backoff"
                    );

                    std::thread::sleep(delay);

                    // Exponential backoff with cap
                    delay = std::cmp::min(delay * 2, DB_LOCK_MAX_DELAY);
                }
                Err(e) => {
                    if attempt > 1 {
                        warn!(
                            attempt,
                            operation,
                            error = %e,
                            "Database operation failed after retries"
                        );
                    }
                    return Err(e);
                }
            }
        }
    }

    /// Run database migrations.
    fn migrate(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CostError::Migration(format!("failed to acquire lock: {}", e)))?;

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
        if from_version < 3 {
            self.migration_v3(conn)?;
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

    /// Migration to version 3: performance metrics tables.
    fn migration_v3(&self, conn: &Connection) -> Result<()> {
        debug!("Running migration v3: performance metrics");

        // Hourly statistics table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS hourly_stats (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                hour TEXT NOT NULL UNIQUE,
                total_calls INTEGER NOT NULL DEFAULT 0,
                total_cost_usd REAL NOT NULL DEFAULT 0,
                total_input_tokens INTEGER NOT NULL DEFAULT 0,
                total_output_tokens INTEGER NOT NULL DEFAULT 0,
                tasks_started INTEGER NOT NULL DEFAULT 0,
                tasks_completed INTEGER NOT NULL DEFAULT 0,
                tasks_failed INTEGER NOT NULL DEFAULT 0,
                active_workers INTEGER NOT NULL DEFAULT 0,
                avg_response_time_ms REAL,
                tokens_per_minute REAL NOT NULL DEFAULT 0,
                last_updated TEXT NOT NULL
            )",
            [],
        )?;

        // Index for hourly stats queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_hourly_stats_hour
             ON hourly_stats(hour)",
            [],
        )?;

        // Daily statistics table (more comprehensive than daily_costs)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS daily_stats (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                date TEXT NOT NULL UNIQUE,
                total_calls INTEGER NOT NULL DEFAULT 0,
                total_cost_usd REAL NOT NULL DEFAULT 0,
                total_input_tokens INTEGER NOT NULL DEFAULT 0,
                total_output_tokens INTEGER NOT NULL DEFAULT 0,
                total_cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                total_cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                tasks_started INTEGER NOT NULL DEFAULT 0,
                tasks_completed INTEGER NOT NULL DEFAULT 0,
                tasks_failed INTEGER NOT NULL DEFAULT 0,
                peak_workers INTEGER NOT NULL DEFAULT 0,
                avg_tokens_per_minute REAL NOT NULL DEFAULT 0,
                success_rate REAL NOT NULL DEFAULT 1.0,
                avg_cost_per_task REAL NOT NULL DEFAULT 0,
                last_updated TEXT NOT NULL
            )",
            [],
        )?;

        // Index for daily stats queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_daily_stats_date
             ON daily_stats(date)",
            [],
        )?;

        // Worker efficiency table (daily per-worker stats)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS worker_efficiency (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                worker_id TEXT NOT NULL,
                date TEXT NOT NULL,
                total_calls INTEGER NOT NULL DEFAULT 0,
                total_cost_usd REAL NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                tasks_completed INTEGER NOT NULL DEFAULT 0,
                tasks_failed INTEGER NOT NULL DEFAULT 0,
                avg_cost_per_task REAL NOT NULL DEFAULT 0,
                success_rate REAL NOT NULL DEFAULT 1.0,
                model TEXT,
                active_time_secs INTEGER NOT NULL DEFAULT 0,
                last_updated TEXT NOT NULL,
                UNIQUE(worker_id, date)
            )",
            [],
        )?;

        // Indexes for worker efficiency queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_worker_efficiency_worker
             ON worker_efficiency(worker_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_worker_efficiency_date
             ON worker_efficiency(date)",
            [],
        )?;

        // Model performance table (daily per-model stats)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS model_performance (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                model TEXT NOT NULL,
                date TEXT NOT NULL,
                total_calls INTEGER NOT NULL DEFAULT 0,
                total_cost_usd REAL NOT NULL DEFAULT 0,
                total_input_tokens INTEGER NOT NULL DEFAULT 0,
                total_output_tokens INTEGER NOT NULL DEFAULT 0,
                total_cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                total_cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                tasks_completed INTEGER NOT NULL DEFAULT 0,
                tasks_failed INTEGER NOT NULL DEFAULT 0,
                avg_cost_per_task REAL NOT NULL DEFAULT 0,
                success_rate REAL NOT NULL DEFAULT 1.0,
                avg_tokens_per_call REAL NOT NULL DEFAULT 0,
                cache_hit_rate REAL NOT NULL DEFAULT 0,
                last_updated TEXT NOT NULL,
                UNIQUE(model, date)
            )",
            [],
        )?;

        // Indexes for model performance queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_model_performance_model
             ON model_performance(model)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_model_performance_date
             ON model_performance(date)",
            [],
        )?;

        // Task events table for tracking task lifecycle
        conn.execute(
            "CREATE TABLE IF NOT EXISTS task_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bead_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                worker_id TEXT,
                model TEXT,
                cost_usd REAL NOT NULL DEFAULT 0,
                tokens_used INTEGER NOT NULL DEFAULT 0,
                error_message TEXT
            )",
            [],
        )?;

        // Indexes for task events
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_task_events_bead
             ON task_events(bead_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_task_events_timestamp
             ON task_events(timestamp)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_task_events_type
             ON task_events(event_type)",
            [],
        )?;

        // Record migration
        conn.execute("INSERT INTO schema_version (version) VALUES (3)", [])?;

        info!("Migration v3 completed: performance metrics");
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
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
                cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens",
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
             FROM model_costs WHERE date = ?1",
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
    pub fn exists(
        &self,
        worker_id: &str,
        timestamp: &str,
        session_id: Option<&str>,
    ) -> Result<bool> {
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
            Self::row_to_subscription,
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
             ORDER BY name",
        )?;

        let subscriptions: Vec<Subscription> = stmt
            .query_map([], Self::row_to_subscription)?
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
             FROM subscriptions ORDER BY active DESC, name",
        )?;

        let subscriptions: Vec<Subscription> = stmt
            .query_map([], Self::row_to_subscription)?
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
    pub fn reset_subscription_billing(
        &self,
        name: &str,
        new_start: DateTime<Utc>,
        new_end: DateTime<Utc>,
    ) -> Result<()> {
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
        debug!(
            id,
            subscription_id = record.subscription_id,
            units = record.units,
            "Recorded usage"
        );
        Ok(id)
    }

    /// Get usage records for a subscription within a date range.
    pub fn get_subscription_usage(
        &self,
        subscription_id: i64,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<SubscriptionUsageRecord>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let mut stmt = conn.prepare(
            "SELECT id, subscription_id, timestamp, units, worker_id, bead_id, api_call_id
             FROM subscription_usage
             WHERE subscription_id = ?1 AND timestamp BETWEEN ?2 AND ?3
             ORDER BY timestamp DESC",
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

    // ============ Performance Metrics Methods ============

    /// Record a task event (started, completed, failed).
    #[allow(clippy::too_many_arguments)]
    pub fn record_task_event(
        &self,
        bead_id: &str,
        event_type: &str,
        worker_id: Option<&str>,
        model: Option<&str>,
        cost_usd: f64,
        tokens_used: i64,
        error_message: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        conn.execute(
            "INSERT INTO task_events
             (bead_id, event_type, timestamp, worker_id, model, cost_usd, tokens_used, error_message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                bead_id,
                event_type,
                Utc::now().to_rfc3339(),
                worker_id,
                model,
                cost_usd,
                tokens_used,
                error_message,
            ],
        )?;

        let id = conn.last_insert_rowid();
        debug!(id, bead_id, event_type, "Recorded task event");
        Ok(id)
    }

    /// Aggregate hourly statistics from api_calls table.
    ///
    /// This should be called periodically (e.g., every 10 minutes) to update
    /// the hourly_stats table with aggregated data.
    pub fn aggregate_hourly_stats(&self, hour: DateTime<Utc>) -> Result<HourlyStat> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        // Truncate to hour
        let hour_str = hour.format("%Y-%m-%dT%H:00:00Z").to_string();
        let hour_start = format!("{}:00:00", hour.format("%Y-%m-%dT%H"));
        let hour_end = format!("{}:59:59", hour.format("%Y-%m-%dT%H"));

        // Get aggregated API call stats for this hour
        let (total_calls, total_cost_usd, total_input_tokens, total_output_tokens): (
            i64,
            f64,
            i64,
            i64,
        ) = conn
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(cost_usd), 0),
                        COALESCE(SUM(input_tokens), 0),
                        COALESCE(SUM(output_tokens), 0)
                 FROM api_calls
                 WHERE timestamp BETWEEN ?1 AND ?2",
                params![hour_start, hour_end],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap_or((0, 0.0, 0, 0));

        // Get task events for this hour
        let (tasks_started, tasks_completed, tasks_failed): (i64, i64, i64) = conn
            .query_row(
                "SELECT
                    COALESCE(SUM(CASE WHEN event_type = 'started' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN event_type = 'completed' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN event_type = 'failed' THEN 1 ELSE 0 END), 0)
                 FROM task_events
                 WHERE timestamp BETWEEN ?1 AND ?2",
                params![hour_start, hour_end],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap_or((0, 0, 0));

        // Get unique workers count
        let active_workers: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT worker_id)
                 FROM api_calls
                 WHERE timestamp BETWEEN ?1 AND ?2",
                params![hour_start, hour_end],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Calculate tokens per minute
        let tokens_per_minute = (total_input_tokens + total_output_tokens) as f64 / 60.0;

        let now = Utc::now().to_rfc3339();

        // Upsert hourly stats
        conn.execute(
            "INSERT INTO hourly_stats
             (hour, total_calls, total_cost_usd, total_input_tokens, total_output_tokens,
              tasks_started, tasks_completed, tasks_failed, active_workers,
              tokens_per_minute, last_updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(hour) DO UPDATE SET
                total_calls = excluded.total_calls,
                total_cost_usd = excluded.total_cost_usd,
                total_input_tokens = excluded.total_input_tokens,
                total_output_tokens = excluded.total_output_tokens,
                tasks_started = excluded.tasks_started,
                tasks_completed = excluded.tasks_completed,
                tasks_failed = excluded.tasks_failed,
                active_workers = excluded.active_workers,
                tokens_per_minute = excluded.tokens_per_minute,
                last_updated = excluded.last_updated",
            params![
                hour_str,
                total_calls,
                total_cost_usd,
                total_input_tokens,
                total_output_tokens,
                tasks_started,
                tasks_completed,
                tasks_failed,
                active_workers,
                tokens_per_minute,
                now,
            ],
        )?;

        debug!(hour = %hour_str, total_calls, "Aggregated hourly stats");

        Ok(HourlyStat {
            id: None,
            hour,
            total_calls,
            total_cost_usd,
            total_input_tokens,
            total_output_tokens,
            tasks_started,
            tasks_completed,
            tasks_failed,
            active_workers,
            avg_response_time_ms: None,
            tokens_per_minute,
            last_updated: Utc::now(),
        })
    }

    /// Aggregate daily statistics from hourly_stats and api_calls tables.
    pub fn aggregate_daily_stats(&self, date: NaiveDate) -> Result<DailyStat> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let date_str = date.format("%Y-%m-%d").to_string();
        let date_start = format!("{}T00:00:00", date_str);
        let date_end = format!("{}T23:59:59", date_str);

        // Get aggregated API call stats
        let (
            total_calls,
            total_cost_usd,
            total_input_tokens,
            total_output_tokens,
            total_cache_creation_tokens,
            total_cache_read_tokens,
        ): (i64, f64, i64, i64, i64, i64) = conn
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(cost_usd), 0),
                        COALESCE(SUM(input_tokens), 0),
                        COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(cache_creation_tokens), 0),
                        COALESCE(SUM(cache_read_tokens), 0)
                 FROM api_calls
                 WHERE timestamp BETWEEN ?1 AND ?2",
                params![date_start, date_end],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap_or((0, 0.0, 0, 0, 0, 0));

        // Get task events
        let (tasks_started, tasks_completed, tasks_failed): (i64, i64, i64) = conn
            .query_row(
                "SELECT
                    COALESCE(SUM(CASE WHEN event_type = 'started' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN event_type = 'completed' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN event_type = 'failed' THEN 1 ELSE 0 END), 0)
                 FROM task_events
                 WHERE timestamp BETWEEN ?1 AND ?2",
                params![date_start, date_end],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap_or((0, 0, 0));

        // Get peak workers from hourly stats
        let peak_workers: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(active_workers), 0)
                 FROM hourly_stats
                 WHERE hour LIKE ?1 || '%'",
                params![date_str],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Calculate derived metrics
        let total_tokens = total_input_tokens
            + total_output_tokens
            + total_cache_creation_tokens
            + total_cache_read_tokens;
        let avg_tokens_per_minute = total_tokens as f64 / 1440.0; // 24 * 60 minutes

        let total_tasks = tasks_completed + tasks_failed;
        let success_rate = if total_tasks == 0 {
            1.0
        } else {
            tasks_completed as f64 / total_tasks as f64
        };

        let avg_cost_per_task = if tasks_completed == 0 {
            0.0
        } else {
            total_cost_usd / tasks_completed as f64
        };

        let now = Utc::now().to_rfc3339();

        // Upsert daily stats
        conn.execute(
            "INSERT INTO daily_stats
             (date, total_calls, total_cost_usd, total_input_tokens, total_output_tokens,
              total_cache_creation_tokens, total_cache_read_tokens,
              tasks_started, tasks_completed, tasks_failed, peak_workers,
              avg_tokens_per_minute, success_rate, avg_cost_per_task, last_updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(date) DO UPDATE SET
                total_calls = excluded.total_calls,
                total_cost_usd = excluded.total_cost_usd,
                total_input_tokens = excluded.total_input_tokens,
                total_output_tokens = excluded.total_output_tokens,
                total_cache_creation_tokens = excluded.total_cache_creation_tokens,
                total_cache_read_tokens = excluded.total_cache_read_tokens,
                tasks_started = excluded.tasks_started,
                tasks_completed = excluded.tasks_completed,
                tasks_failed = excluded.tasks_failed,
                peak_workers = excluded.peak_workers,
                avg_tokens_per_minute = excluded.avg_tokens_per_minute,
                success_rate = excluded.success_rate,
                avg_cost_per_task = excluded.avg_cost_per_task,
                last_updated = excluded.last_updated",
            params![
                date_str,
                total_calls,
                total_cost_usd,
                total_input_tokens,
                total_output_tokens,
                total_cache_creation_tokens,
                total_cache_read_tokens,
                tasks_started,
                tasks_completed,
                tasks_failed,
                peak_workers,
                avg_tokens_per_minute,
                success_rate,
                avg_cost_per_task,
                now,
            ],
        )?;

        debug!(date = %date_str, total_calls, "Aggregated daily stats");

        Ok(DailyStat {
            id: None,
            date,
            total_calls,
            total_cost_usd,
            total_input_tokens,
            total_output_tokens,
            total_cache_creation_tokens,
            total_cache_read_tokens,
            tasks_started,
            tasks_completed,
            tasks_failed,
            peak_workers,
            avg_tokens_per_minute,
            success_rate,
            avg_cost_per_task,
            last_updated: Utc::now(),
        })
    }

    /// Aggregate worker efficiency stats for a specific date.
    pub fn aggregate_worker_efficiency(&self, date: NaiveDate) -> Result<Vec<WorkerEfficiency>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let date_str = date.format("%Y-%m-%d").to_string();
        let date_start = format!("{}T00:00:00", date_str);
        let date_end = format!("{}T23:59:59", date_str);

        // Get per-worker API call stats
        let mut stmt = conn.prepare(
            "SELECT worker_id,
                    COUNT(*) as total_calls,
                    COALESCE(SUM(cost_usd), 0) as total_cost,
                    COALESCE(SUM(input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens), 0) as total_tokens,
                    MAX(model) as model
             FROM api_calls
             WHERE timestamp BETWEEN ?1 AND ?2
             GROUP BY worker_id",
        )?;

        let mut workers: Vec<WorkerEfficiency> = stmt
            .query_map(params![date_start, date_end], |row| {
                Ok(WorkerEfficiency {
                    worker_id: row.get(0)?,
                    date,
                    total_calls: row.get(1)?,
                    total_cost_usd: row.get(2)?,
                    total_tokens: row.get(3)?,
                    tasks_completed: 0,
                    tasks_failed: 0,
                    avg_cost_per_task: 0.0,
                    success_rate: 1.0,
                    model: row.get(4)?,
                    active_time_secs: 0,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Get task events per worker
        for worker in &mut workers {
            let (completed, failed): (i64, i64) = conn
                .query_row(
                    "SELECT
                        COALESCE(SUM(CASE WHEN event_type = 'completed' THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN event_type = 'failed' THEN 1 ELSE 0 END), 0)
                     FROM task_events
                     WHERE worker_id = ?1 AND timestamp BETWEEN ?2 AND ?3",
                    params![worker.worker_id, date_start, date_end],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap_or((0, 0));

            worker.tasks_completed = completed;
            worker.tasks_failed = failed;
            worker.calculate_derived_metrics();
        }

        let now = Utc::now().to_rfc3339();

        // Upsert worker efficiency records
        for worker in &workers {
            conn.execute(
                "INSERT INTO worker_efficiency
                 (worker_id, date, total_calls, total_cost_usd, total_tokens,
                  tasks_completed, tasks_failed, avg_cost_per_task, success_rate,
                  model, active_time_secs, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(worker_id, date) DO UPDATE SET
                    total_calls = excluded.total_calls,
                    total_cost_usd = excluded.total_cost_usd,
                    total_tokens = excluded.total_tokens,
                    tasks_completed = excluded.tasks_completed,
                    tasks_failed = excluded.tasks_failed,
                    avg_cost_per_task = excluded.avg_cost_per_task,
                    success_rate = excluded.success_rate,
                    model = excluded.model,
                    active_time_secs = excluded.active_time_secs,
                    last_updated = excluded.last_updated",
                params![
                    worker.worker_id,
                    date_str,
                    worker.total_calls,
                    worker.total_cost_usd,
                    worker.total_tokens,
                    worker.tasks_completed,
                    worker.tasks_failed,
                    worker.avg_cost_per_task,
                    worker.success_rate,
                    worker.model,
                    worker.active_time_secs,
                    now,
                ],
            )?;
        }

        debug!(date = %date_str, count = workers.len(), "Aggregated worker efficiency");
        Ok(workers)
    }

    /// Aggregate model performance stats for a specific date.
    pub fn aggregate_model_performance(&self, date: NaiveDate) -> Result<Vec<ModelPerformance>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let date_str = date.format("%Y-%m-%d").to_string();
        let date_start = format!("{}T00:00:00", date_str);
        let date_end = format!("{}T23:59:59", date_str);

        // Get per-model API call stats
        let mut stmt = conn.prepare(
            "SELECT model,
                    COUNT(*) as total_calls,
                    COALESCE(SUM(cost_usd), 0) as total_cost,
                    COALESCE(SUM(input_tokens), 0) as input_tokens,
                    COALESCE(SUM(output_tokens), 0) as output_tokens,
                    COALESCE(SUM(cache_creation_tokens), 0) as cache_creation,
                    COALESCE(SUM(cache_read_tokens), 0) as cache_read
             FROM api_calls
             WHERE timestamp BETWEEN ?1 AND ?2
             GROUP BY model",
        )?;

        let mut models: Vec<ModelPerformance> = stmt
            .query_map(params![date_start, date_end], |row| {
                Ok(ModelPerformance {
                    model: row.get(0)?,
                    date,
                    total_calls: row.get(1)?,
                    total_cost_usd: row.get(2)?,
                    total_input_tokens: row.get(3)?,
                    total_output_tokens: row.get(4)?,
                    total_cache_creation_tokens: row.get(5)?,
                    total_cache_read_tokens: row.get(6)?,
                    tasks_completed: 0,
                    tasks_failed: 0,
                    avg_cost_per_task: 0.0,
                    success_rate: 1.0,
                    avg_tokens_per_call: 0.0,
                    cache_hit_rate: 0.0,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Get task events per model
        for model in &mut models {
            let (completed, failed): (i64, i64) = conn
                .query_row(
                    "SELECT
                        COALESCE(SUM(CASE WHEN event_type = 'completed' THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN event_type = 'failed' THEN 1 ELSE 0 END), 0)
                     FROM task_events
                     WHERE model = ?1 AND timestamp BETWEEN ?2 AND ?3",
                    params![model.model, date_start, date_end],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap_or((0, 0));

            model.tasks_completed = completed;
            model.tasks_failed = failed;
            model.calculate_derived_metrics();
        }

        let now = Utc::now().to_rfc3339();

        // Upsert model performance records
        for model in &models {
            conn.execute(
                "INSERT INTO model_performance
                 (model, date, total_calls, total_cost_usd, total_input_tokens,
                  total_output_tokens, total_cache_creation_tokens, total_cache_read_tokens,
                  tasks_completed, tasks_failed, avg_cost_per_task, success_rate,
                  avg_tokens_per_call, cache_hit_rate, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                 ON CONFLICT(model, date) DO UPDATE SET
                    total_calls = excluded.total_calls,
                    total_cost_usd = excluded.total_cost_usd,
                    total_input_tokens = excluded.total_input_tokens,
                    total_output_tokens = excluded.total_output_tokens,
                    total_cache_creation_tokens = excluded.total_cache_creation_tokens,
                    total_cache_read_tokens = excluded.total_cache_read_tokens,
                    tasks_completed = excluded.tasks_completed,
                    tasks_failed = excluded.tasks_failed,
                    avg_cost_per_task = excluded.avg_cost_per_task,
                    success_rate = excluded.success_rate,
                    avg_tokens_per_call = excluded.avg_tokens_per_call,
                    cache_hit_rate = excluded.cache_hit_rate,
                    last_updated = excluded.last_updated",
                params![
                    model.model,
                    date_str,
                    model.total_calls,
                    model.total_cost_usd,
                    model.total_input_tokens,
                    model.total_output_tokens,
                    model.total_cache_creation_tokens,
                    model.total_cache_read_tokens,
                    model.tasks_completed,
                    model.tasks_failed,
                    model.avg_cost_per_task,
                    model.success_rate,
                    model.avg_tokens_per_call,
                    model.cache_hit_rate,
                    now,
                ],
            )?;
        }

        debug!(date = %date_str, count = models.len(), "Aggregated model performance");
        Ok(models)
    }

    /// Run all aggregations for current hour and day.
    ///
    /// This is the main entry point for background aggregation.
    /// Should be called every 10 minutes.
    pub fn run_background_aggregation(&self) -> Result<()> {
        let now = Utc::now();
        let today = now.date_naive();

        info!("Running background aggregation");

        // Aggregate current hour
        self.aggregate_hourly_stats(now)?;

        // Aggregate previous hour too (in case of missed updates)
        let prev_hour = now - chrono::Duration::hours(1);
        self.aggregate_hourly_stats(prev_hour)?;

        // Aggregate daily stats
        self.aggregate_daily_stats(today)?;

        // Aggregate worker efficiency
        self.aggregate_worker_efficiency(today)?;

        // Aggregate model performance
        self.aggregate_model_performance(today)?;

        info!("Background aggregation completed");
        Ok(())
    }

    /// Get hourly stats for a specific hour.
    pub fn get_hourly_stat(&self, hour: DateTime<Utc>) -> Result<Option<HourlyStat>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let hour_str = hour.format("%Y-%m-%dT%H:00:00Z").to_string();

        let result = conn.query_row(
            "SELECT id, hour, total_calls, total_cost_usd, total_input_tokens,
                    total_output_tokens, tasks_started, tasks_completed, tasks_failed,
                    active_workers, avg_response_time_ms, tokens_per_minute, last_updated
             FROM hourly_stats WHERE hour = ?1",
            params![hour_str],
            |row| {
                let hour_str: String = row.get(1)?;
                let last_updated_str: String = row.get(12)?;

                let hour = DateTime::parse_from_rfc3339(&hour_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or(Utc::now());
                let last_updated = DateTime::parse_from_rfc3339(&last_updated_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or(Utc::now());

                Ok(HourlyStat {
                    id: Some(row.get(0)?),
                    hour,
                    total_calls: row.get(2)?,
                    total_cost_usd: row.get(3)?,
                    total_input_tokens: row.get(4)?,
                    total_output_tokens: row.get(5)?,
                    tasks_started: row.get(6)?,
                    tasks_completed: row.get(7)?,
                    tasks_failed: row.get(8)?,
                    active_workers: row.get(9)?,
                    avg_response_time_ms: row.get(10)?,
                    tokens_per_minute: row.get(11)?,
                    last_updated,
                })
            },
        );

        match result {
            Ok(stat) => Ok(Some(stat)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CostError::Database(e)),
        }
    }

    /// Get daily stats for a specific date.
    pub fn get_daily_stat(&self, date: NaiveDate) -> Result<Option<DailyStat>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let date_str = date.format("%Y-%m-%d").to_string();

        let result = conn.query_row(
            "SELECT id, date, total_calls, total_cost_usd, total_input_tokens,
                    total_output_tokens, total_cache_creation_tokens, total_cache_read_tokens,
                    tasks_started, tasks_completed, tasks_failed, peak_workers,
                    avg_tokens_per_minute, success_rate, avg_cost_per_task, last_updated
             FROM daily_stats WHERE date = ?1",
            params![date_str],
            |row| {
                let date_str: String = row.get(1)?;
                let last_updated_str: String = row.get(15)?;

                let date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                    .unwrap_or(Utc::now().date_naive());
                let last_updated = DateTime::parse_from_rfc3339(&last_updated_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or(Utc::now());

                Ok(DailyStat {
                    id: Some(row.get(0)?),
                    date,
                    total_calls: row.get(2)?,
                    total_cost_usd: row.get(3)?,
                    total_input_tokens: row.get(4)?,
                    total_output_tokens: row.get(5)?,
                    total_cache_creation_tokens: row.get(6)?,
                    total_cache_read_tokens: row.get(7)?,
                    tasks_started: row.get(8)?,
                    tasks_completed: row.get(9)?,
                    tasks_failed: row.get(10)?,
                    peak_workers: row.get(11)?,
                    avg_tokens_per_minute: row.get(12)?,
                    success_rate: row.get(13)?,
                    avg_cost_per_task: row.get(14)?,
                    last_updated,
                })
            },
        );

        match result {
            Ok(stat) => Ok(Some(stat)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CostError::Database(e)),
        }
    }

    /// Get worker efficiency stats for a specific date.
    pub fn get_worker_efficiency(&self, date: NaiveDate) -> Result<Vec<WorkerEfficiency>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let date_str = date.format("%Y-%m-%d").to_string();

        let mut stmt = conn.prepare(
            "SELECT id, worker_id, date, total_calls, total_cost_usd, total_tokens,
                    tasks_completed, tasks_failed, avg_cost_per_task, success_rate,
                    model, active_time_secs
             FROM worker_efficiency WHERE date = ?1
             ORDER BY total_cost_usd DESC",
        )?;

        let workers: Vec<WorkerEfficiency> = stmt
            .query_map(params![date_str], |row| {
                let date_str: String = row.get(2)?;
                let date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                    .unwrap_or(Utc::now().date_naive());

                Ok(WorkerEfficiency {
                    worker_id: row.get(1)?,
                    date,
                    total_calls: row.get(3)?,
                    total_cost_usd: row.get(4)?,
                    total_tokens: row.get(5)?,
                    tasks_completed: row.get(6)?,
                    tasks_failed: row.get(7)?,
                    avg_cost_per_task: row.get(8)?,
                    success_rate: row.get(9)?,
                    model: row.get(10)?,
                    active_time_secs: row.get(11)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(workers)
    }

    /// Get model performance stats for a specific date.
    pub fn get_model_performance(&self, date: NaiveDate) -> Result<Vec<ModelPerformance>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let date_str = date.format("%Y-%m-%d").to_string();

        let mut stmt = conn.prepare(
            "SELECT id, model, date, total_calls, total_cost_usd, total_input_tokens,
                    total_output_tokens, total_cache_creation_tokens, total_cache_read_tokens,
                    tasks_completed, tasks_failed, avg_cost_per_task, success_rate,
                    avg_tokens_per_call, cache_hit_rate
             FROM model_performance WHERE date = ?1
             ORDER BY total_cost_usd DESC",
        )?;

        let models: Vec<ModelPerformance> = stmt
            .query_map(params![date_str], |row| {
                let date_str: String = row.get(2)?;
                let date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                    .unwrap_or(Utc::now().date_naive());

                Ok(ModelPerformance {
                    model: row.get(1)?,
                    date,
                    total_calls: row.get(3)?,
                    total_cost_usd: row.get(4)?,
                    total_input_tokens: row.get(5)?,
                    total_output_tokens: row.get(6)?,
                    total_cache_creation_tokens: row.get(7)?,
                    total_cache_read_tokens: row.get(8)?,
                    tasks_completed: row.get(9)?,
                    tasks_failed: row.get(10)?,
                    avg_cost_per_task: row.get(11)?,
                    success_rate: row.get(12)?,
                    avg_tokens_per_call: row.get(13)?,
                    cache_hit_rate: row.get(14)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(models)
    }

    /// Get hourly stats for the last N hours.
    pub fn get_recent_hourly_stats(&self, hours: i32) -> Result<Vec<HourlyStat>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let cutoff = Utc::now() - chrono::Duration::hours(hours as i64);
        let cutoff_str = cutoff.format("%Y-%m-%dT%H:00:00Z").to_string();

        let mut stmt = conn.prepare(
            "SELECT id, hour, total_calls, total_cost_usd, total_input_tokens,
                    total_output_tokens, tasks_started, tasks_completed, tasks_failed,
                    active_workers, avg_response_time_ms, tokens_per_minute, last_updated
             FROM hourly_stats
             WHERE hour >= ?1
             ORDER BY hour DESC",
        )?;

        let stats: Vec<HourlyStat> = stmt
            .query_map(params![cutoff_str], |row| {
                let hour_str: String = row.get(1)?;
                let last_updated_str: String = row.get(12)?;

                let hour = DateTime::parse_from_rfc3339(&hour_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or(Utc::now());
                let last_updated = DateTime::parse_from_rfc3339(&last_updated_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or(Utc::now());

                Ok(HourlyStat {
                    id: Some(row.get(0)?),
                    hour,
                    total_calls: row.get(2)?,
                    total_cost_usd: row.get(3)?,
                    total_input_tokens: row.get(4)?,
                    total_output_tokens: row.get(5)?,
                    tasks_started: row.get(6)?,
                    tasks_completed: row.get(7)?,
                    tasks_failed: row.get(8)?,
                    active_workers: row.get(9)?,
                    avg_response_time_ms: row.get(10)?,
                    tokens_per_minute: row.get(11)?,
                    last_updated,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(stats)
    }

    /// Get daily stats for the last N days.
    pub fn get_recent_daily_stats(&self, days: i32) -> Result<Vec<DailyStat>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let cutoff = Utc::now().date_naive() - chrono::Days::new(days as u64);
        let cutoff_str = cutoff.format("%Y-%m-%d").to_string();

        let mut stmt = conn.prepare(
            "SELECT id, date, total_calls, total_cost_usd, total_input_tokens,
                    total_output_tokens, total_cache_creation_tokens, total_cache_read_tokens,
                    tasks_started, tasks_completed, tasks_failed, peak_workers,
                    avg_tokens_per_minute, success_rate, avg_cost_per_task, last_updated
             FROM daily_stats
             WHERE date >= ?1
             ORDER BY date DESC",
        )?;

        let stats: Vec<DailyStat> = stmt
            .query_map(params![cutoff_str], |row| {
                let date_str: String = row.get(1)?;
                let last_updated_str: String = row.get(15)?;

                let date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                    .unwrap_or(Utc::now().date_naive());
                let last_updated = DateTime::parse_from_rfc3339(&last_updated_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or(Utc::now());

                Ok(DailyStat {
                    id: Some(row.get(0)?),
                    date,
                    total_calls: row.get(2)?,
                    total_cost_usd: row.get(3)?,
                    total_input_tokens: row.get(4)?,
                    total_output_tokens: row.get(5)?,
                    total_cache_creation_tokens: row.get(6)?,
                    total_cache_read_tokens: row.get(7)?,
                    tasks_started: row.get(8)?,
                    tasks_completed: row.get(9)?,
                    tasks_failed: row.get(10)?,
                    peak_workers: row.get(11)?,
                    avg_tokens_per_minute: row.get(12)?,
                    success_rate: row.get(13)?,
                    avg_cost_per_task: row.get(14)?,
                    last_updated,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(stats)
    }

    /// Get 7-day trend data for sparklines.
    ///
    /// Returns a vector of daily task counts for the last 7 days,
    /// suitable for rendering sparkline visualizations.
    pub fn get_7day_task_trend(&self) -> Result<Vec<i64>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let today = Utc::now().date_naive();
        let start_date = today - chrono::Days::new(6);

        // Fill in missing days with zeros
        let mut trend = Vec::with_capacity(7);
        for i in 0..7 {
            let date = start_date + chrono::Days::new(i);
            let date_str = date.format("%Y-%m-%d").to_string();

            // Check if we have data for this date
            let count: i64 = conn
                .query_row(
                    "SELECT COALESCE(tasks_completed, 0) FROM daily_stats WHERE date = ?1",
                    params![date_str],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            trend.push(count);
        }

        Ok(trend)
    }

    /// Get 7-day cost trend data for sparklines.
    pub fn get_7day_cost_trend(&self) -> Result<Vec<f64>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let today = Utc::now().date_naive();
        let start_date = today - chrono::Days::new(6);

        let mut trend = Vec::with_capacity(7);
        for i in 0..7 {
            let date = start_date + chrono::Days::new(i);
            let date_str = date.format("%Y-%m-%d").to_string();

            let cost: f64 = conn
                .query_row(
                    "SELECT COALESCE(total_cost_usd, 0.0) FROM daily_stats WHERE date = ?1",
                    params![date_str],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);

            trend.push(cost);
        }

        Ok(trend)
    }

    /// Get tasks per hour for the last 24 hours (for histogram).
    pub fn get_tasks_per_hour(&self) -> Result<Vec<i64>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let cutoff = Utc::now() - chrono::Duration::hours(24);

        let mut result = vec![0i64; 24];

        let mut stmt = conn.prepare(
            "SELECT hour, tasks_completed
             FROM hourly_stats
             WHERE hour >= ?1
             ORDER BY hour ASC",
        )?;

        let cutoff_str = cutoff.format("%Y-%m-%dT%H:00:00Z").to_string();

        let rows: Vec<(String, i64)> = stmt
            .query_map(params![cutoff_str], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        for (hour_str, tasks) in rows {
            if let Ok(hour_dt) = DateTime::parse_from_rfc3339(&hour_str) {
                let hour_utc = hour_dt.with_timezone(&Utc);
                let hours_ago = (Utc::now() - hour_utc).num_hours() as usize;
                if hours_ago < 24 {
                    result[23 - hours_ago] = tasks;
                }
            }
        }

        Ok(result)
    }

    /// Get model performance aggregated over the last 7 days with extended metrics.
    pub fn get_model_performance_7day(&self) -> Result<Vec<ModelPerformance>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let start_date = Utc::now().date_naive() - chrono::Days::new(6);
        let start_str = start_date.format("%Y-%m-%d").to_string();

        let mut stmt = conn.prepare(
            "SELECT model,
                    SUM(total_calls) as total_calls,
                    SUM(total_cost_usd) as total_cost,
                    SUM(total_input_tokens) as input_tokens,
                    SUM(total_output_tokens) as output_tokens,
                    SUM(total_cache_creation_tokens) as cache_creation,
                    SUM(total_cache_read_tokens) as cache_read,
                    SUM(tasks_completed) as tasks_completed,
                    SUM(tasks_failed) as tasks_failed
             FROM model_performance
             WHERE date >= ?1
             GROUP BY model
             ORDER BY total_cost DESC",
        )?;

        let models: Vec<ModelPerformance> = stmt
            .query_map(params![start_str], |row| {
                let model: String = row.get(0)?;
                let total_calls: i64 = row.get(1)?;
                let total_cost: f64 = row.get(2)?;
                let input_tokens: i64 = row.get(3)?;
                let output_tokens: i64 = row.get(4)?;
                let cache_creation: i64 = row.get(5)?;
                let cache_read: i64 = row.get(6)?;
                let tasks_completed: i64 = row.get(7)?;
                let tasks_failed: i64 = row.get(8)?;

                let total_tasks = tasks_completed + tasks_failed;
                let success_rate = if total_tasks == 0 {
                    1.0
                } else {
                    tasks_completed as f64 / total_tasks as f64
                };

                let avg_cost_per_task = if tasks_completed == 0 {
                    0.0
                } else {
                    total_cost / tasks_completed as f64
                };

                let total_tokens = input_tokens + output_tokens + cache_creation + cache_read;
                let avg_tokens_per_call = if total_calls == 0 {
                    0.0
                } else {
                    total_tokens as f64 / total_calls as f64
                };

                let total_input = input_tokens + cache_read;
                let cache_hit_rate = if total_input == 0 {
                    0.0
                } else {
                    cache_read as f64 / total_input as f64
                };

                Ok(ModelPerformance {
                    model,
                    date: start_date, // Representing the period start
                    total_calls,
                    total_cost_usd: total_cost,
                    total_input_tokens: input_tokens,
                    total_output_tokens: output_tokens,
                    total_cache_creation_tokens: cache_creation,
                    total_cache_read_tokens: cache_read,
                    tasks_completed,
                    tasks_failed,
                    avg_cost_per_task,
                    success_rate,
                    avg_tokens_per_call,
                    cache_hit_rate,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(models)
    }

    /// Get worker efficiency aggregated over the last 7 days.
    pub fn get_worker_efficiency_7day(&self) -> Result<Vec<WorkerEfficiency>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let start_date = Utc::now().date_naive() - chrono::Days::new(6);
        let start_str = start_date.format("%Y-%m-%d").to_string();

        let mut stmt = conn.prepare(
            "SELECT worker_id,
                    SUM(total_calls) as total_calls,
                    SUM(total_cost_usd) as total_cost,
                    SUM(total_tokens) as total_tokens,
                    SUM(tasks_completed) as tasks_completed,
                    SUM(tasks_failed) as tasks_failed,
                    SUM(active_time_secs) as active_time,
                    model
             FROM worker_efficiency
             WHERE date >= ?1
             GROUP BY worker_id
             ORDER BY tasks_completed DESC",
        )?;

        let workers: Vec<WorkerEfficiency> = stmt
            .query_map(params![start_str], |row| {
                let worker_id: String = row.get(0)?;
                let total_calls: i64 = row.get(1)?;
                let total_cost: f64 = row.get(2)?;
                let total_tokens: i64 = row.get(3)?;
                let tasks_completed: i64 = row.get(4)?;
                let tasks_failed: i64 = row.get(5)?;
                let active_time: i64 = row.get(6)?;
                let model: Option<String> = row.get(7)?;

                let total_tasks = tasks_completed + tasks_failed;
                let success_rate = if total_tasks == 0 {
                    1.0
                } else {
                    tasks_completed as f64 / total_tasks as f64
                };

                let avg_cost_per_task = if tasks_completed == 0 {
                    0.0
                } else {
                    total_cost / tasks_completed as f64
                };

                Ok(WorkerEfficiency {
                    worker_id,
                    date: start_date,
                    total_calls,
                    total_cost_usd: total_cost,
                    total_tokens,
                    tasks_completed,
                    tasks_failed,
                    avg_cost_per_task,
                    success_rate,
                    model,
                    active_time_secs: active_time,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(workers)
    }

    /// Get average cost per task by model.
    pub fn get_avg_cost_per_task_by_model(&self) -> Result<Vec<(String, f64)>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let today = Utc::now().date_naive();
        let start_date = today - chrono::Days::new(6);
        let start_str = start_date.format("%Y-%m-%d").to_string();

        let mut stmt = conn.prepare(
            "SELECT model,
                    SUM(total_cost_usd) / NULLIF(SUM(tasks_completed), 0) as avg_cost
             FROM model_performance
             WHERE date >= ?1 AND tasks_completed > 0
             GROUP BY model
             ORDER BY avg_cost DESC",
        )?;

        let results: Vec<(String, f64)> = stmt
            .query_map(params![start_str], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    /// Get API calls since a specific timestamp.
    pub fn get_api_calls_since(&self, since: DateTime<Utc>) -> Result<Vec<ApiCall>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let since_str = since.to_rfc3339();
        let mut stmt = conn.prepare_cached(
            "SELECT id, timestamp, worker_id, session_id, model,
                    input_tokens, output_tokens,
                    cache_creation_tokens, cache_read_tokens,
                    cost_usd, bead_id, event_type
             FROM api_calls
             WHERE timestamp >= ?1
             ORDER BY timestamp DESC"
        )?;

        let calls: Vec<ApiCall> = stmt
            .query_map(params![since_str], |row| {
                Ok(ApiCall {
                    id: Some(row.get(0)?),
                    timestamp: DateTime::parse_from_rfc3339(&row.get::<_, String>(1)?)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?
                        .with_timezone(&Utc),
                    worker_id: row.get(2)?,
                    session_id: row.get(3)?,
                    model: row.get(4)?,
                    input_tokens: row.get(5)?,
                    output_tokens: row.get(6)?,
                    cache_creation_tokens: row.get(7)?,
                    cache_read_tokens: row.get(8)?,
                    cost_usd: row.get(9)?,
                    bead_id: row.get(10)?,
                    event_type: row.get(11)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(calls)
    }

    /// Get subscription ID by name.
    pub fn get_subscription_id(&self, name: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().map_err(|e| {
            CostError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        let mut stmt = conn.prepare_cached(
            "SELECT id FROM subscriptions WHERE name = ?1"
        )?;

        match stmt.query_row(params![name], |row| row.get(0)) {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CostError::Database(e)),
        }
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
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap();
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
        let daily = db
            .get_daily_cost(today)
            .unwrap()
            .expect("Should have daily cost");
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

        let opus = daily
            .by_model
            .iter()
            .find(|m| m.model == "claude-opus")
            .unwrap();
        assert_eq!(opus.call_count, 2);
        assert!((opus.total_cost_usd - 0.25).abs() < 0.0001);
    }

    #[test]
    fn test_deduplication_check() {
        let db = CostDatabase::open_in_memory().unwrap();
        let now = Utc::now();
        let ts = now.to_rfc3339();

        let call =
            ApiCall::new(now, "worker-1", "claude-opus", 100, 50, 0.01).with_session("session-123");

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
        let sub2 = Subscription::new(
            "ChatGPT Plus",
            SubscriptionType::FixedQuota,
            20.0,
            start,
            end,
        );
        let mut sub3 =
            Subscription::new("Cursor Pro", SubscriptionType::FixedQuota, 20.0, start, end);
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
        let records = db
            .get_subscription_usage(sub_id, start - Duration::days(1), end)
            .unwrap();
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

        let mut sub =
            Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
                .with_quota(500);
        sub.quota_used = 450;
        db.upsert_subscription(&sub).unwrap();

        // Reset billing period
        let new_start = Utc::now();
        let new_end = new_start + Duration::days(30);
        db.reset_subscription_billing("Claude Pro", new_start, new_end)
            .unwrap();

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

    // ============ Performance Metrics Tests ============

    #[test]
    fn test_record_task_event() {
        let db = CostDatabase::open_in_memory().unwrap();

        let id = db
            .record_task_event(
                "bd-123",
                "started",
                Some("worker-1"),
                Some("claude-opus"),
                0.0,
                0,
                None,
            )
            .unwrap();
        assert!(id > 0);

        let id2 = db
            .record_task_event(
                "bd-123",
                "completed",
                Some("worker-1"),
                Some("claude-opus"),
                1.50,
                5000,
                None,
            )
            .unwrap();
        assert!(id2 > id);
    }

    #[test]
    fn test_record_task_event_with_error() {
        let db = CostDatabase::open_in_memory().unwrap();

        let id = db
            .record_task_event(
                "bd-456",
                "failed",
                Some("worker-1"),
                Some("claude-opus"),
                0.50,
                2000,
                Some("API rate limit exceeded"),
            )
            .unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_aggregate_hourly_stats() {
        let db = CostDatabase::open_in_memory().unwrap();
        let now = Utc::now();

        // Insert some API calls
        let calls = vec![
            ApiCall::new(now, "worker-1", "claude-opus", 1000, 500, 0.10),
            ApiCall::new(now, "worker-2", "claude-sonnet", 2000, 1000, 0.05),
        ];
        db.insert_api_calls(&calls).unwrap();

        // Record task events
        db.record_task_event(
            "bd-1",
            "started",
            Some("worker-1"),
            Some("claude-opus"),
            0.0,
            0,
            None,
        )
        .unwrap();
        db.record_task_event(
            "bd-1",
            "completed",
            Some("worker-1"),
            Some("claude-opus"),
            0.10,
            1500,
            None,
        )
        .unwrap();

        // Aggregate hourly stats
        let stat = db.aggregate_hourly_stats(now).unwrap();

        assert_eq!(stat.total_calls, 2);
        assert!((stat.total_cost_usd - 0.15).abs() < 0.0001);
        assert_eq!(stat.total_input_tokens, 3000);
        assert_eq!(stat.total_output_tokens, 1500);
        assert_eq!(stat.tasks_started, 1);
        assert_eq!(stat.tasks_completed, 1);
        assert_eq!(stat.active_workers, 2);
    }

    #[test]
    fn test_aggregate_daily_stats() {
        let db = CostDatabase::open_in_memory().unwrap();
        let now = Utc::now();
        let today = now.date_naive();

        // Insert API calls
        let calls = vec![
            ApiCall::new(now, "worker-1", "claude-opus", 1000, 500, 0.10).with_cache(200, 100),
            ApiCall::new(now, "worker-2", "claude-sonnet", 2000, 1000, 0.05),
        ];
        db.insert_api_calls(&calls).unwrap();

        // Record task events
        db.record_task_event(
            "bd-1",
            "completed",
            Some("worker-1"),
            Some("claude-opus"),
            0.10,
            1800,
            None,
        )
        .unwrap();
        db.record_task_event(
            "bd-2",
            "failed",
            Some("worker-2"),
            Some("claude-sonnet"),
            0.05,
            3000,
            Some("Timeout"),
        )
        .unwrap();

        // Aggregate daily stats
        let stat = db.aggregate_daily_stats(today).unwrap();

        assert_eq!(stat.total_calls, 2);
        assert!((stat.total_cost_usd - 0.15).abs() < 0.0001);
        assert_eq!(stat.tasks_completed, 1);
        assert_eq!(stat.tasks_failed, 1);
        assert!((stat.success_rate - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_aggregate_worker_efficiency() {
        let db = CostDatabase::open_in_memory().unwrap();
        let now = Utc::now();
        let today = now.date_naive();

        // Insert API calls for different workers
        let calls = vec![
            ApiCall::new(now, "worker-1", "claude-opus", 1000, 500, 0.50),
            ApiCall::new(now, "worker-1", "claude-opus", 1500, 750, 0.75),
            ApiCall::new(now, "worker-2", "claude-sonnet", 2000, 1000, 0.10),
        ];
        db.insert_api_calls(&calls).unwrap();

        // Record task events
        db.record_task_event(
            "bd-1",
            "completed",
            Some("worker-1"),
            Some("claude-opus"),
            1.25,
            3750,
            None,
        )
        .unwrap();
        db.record_task_event(
            "bd-2",
            "completed",
            Some("worker-2"),
            Some("claude-sonnet"),
            0.10,
            3000,
            None,
        )
        .unwrap();

        // Aggregate worker efficiency
        let workers = db.aggregate_worker_efficiency(today).unwrap();

        assert_eq!(workers.len(), 2);

        let worker1 = workers.iter().find(|w| w.worker_id == "worker-1").unwrap();
        assert_eq!(worker1.total_calls, 2);
        assert!((worker1.total_cost_usd - 1.25).abs() < 0.0001);
        assert_eq!(worker1.tasks_completed, 1);

        let worker2 = workers.iter().find(|w| w.worker_id == "worker-2").unwrap();
        assert_eq!(worker2.total_calls, 1);
        assert!((worker2.total_cost_usd - 0.10).abs() < 0.0001);
    }

    #[test]
    fn test_aggregate_model_performance() {
        let db = CostDatabase::open_in_memory().unwrap();
        let now = Utc::now();
        let today = now.date_naive();

        // Insert API calls for different models
        let calls = vec![
            ApiCall::new(now, "worker-1", "claude-opus", 1000, 500, 0.50).with_cache(100, 200),
            ApiCall::new(now, "worker-1", "claude-opus", 1500, 750, 0.75),
            ApiCall::new(now, "worker-2", "claude-sonnet", 2000, 1000, 0.10),
        ];
        db.insert_api_calls(&calls).unwrap();

        // Record task events
        db.record_task_event(
            "bd-1",
            "completed",
            None,
            Some("claude-opus"),
            1.25,
            4050,
            None,
        )
        .unwrap();
        db.record_task_event(
            "bd-2",
            "completed",
            None,
            Some("claude-sonnet"),
            0.10,
            3000,
            None,
        )
        .unwrap();

        // Aggregate model performance
        let models = db.aggregate_model_performance(today).unwrap();

        assert_eq!(models.len(), 2);

        let opus = models.iter().find(|m| m.model == "claude-opus").unwrap();
        assert_eq!(opus.total_calls, 2);
        assert!((opus.total_cost_usd - 1.25).abs() < 0.0001);
        assert_eq!(opus.total_cache_read_tokens, 200);
        assert!(opus.cache_hit_rate > 0.0);

        let sonnet = models.iter().find(|m| m.model == "claude-sonnet").unwrap();
        assert_eq!(sonnet.total_calls, 1);
    }

    #[test]
    fn test_run_background_aggregation() {
        let db = CostDatabase::open_in_memory().unwrap();
        let now = Utc::now();

        // Insert some data
        let calls = vec![ApiCall::new(
            now,
            "worker-1",
            "claude-opus",
            1000,
            500,
            0.10,
        )];
        db.insert_api_calls(&calls).unwrap();

        db.record_task_event(
            "bd-1",
            "completed",
            Some("worker-1"),
            Some("claude-opus"),
            0.10,
            1500,
            None,
        )
        .unwrap();

        // Run background aggregation
        db.run_background_aggregation().unwrap();

        // Verify stats were created
        let today = now.date_naive();
        let daily = db.get_daily_stat(today).unwrap();
        assert!(daily.is_some());

        let workers = db.get_worker_efficiency(today).unwrap();
        assert!(!workers.is_empty());

        let models = db.get_model_performance(today).unwrap();
        assert!(!models.is_empty());
    }

    #[test]
    fn test_get_hourly_stat() {
        let db = CostDatabase::open_in_memory().unwrap();
        let now = Utc::now();

        // Aggregate stats first
        let calls = vec![ApiCall::new(
            now,
            "worker-1",
            "claude-opus",
            1000,
            500,
            0.10,
        )];
        db.insert_api_calls(&calls).unwrap();
        db.aggregate_hourly_stats(now).unwrap();

        // Query the hourly stat
        let stat = db.get_hourly_stat(now).unwrap();
        assert!(stat.is_some());

        let stat = stat.unwrap();
        assert_eq!(stat.total_calls, 1);
    }

    #[test]
    fn test_get_recent_hourly_stats() {
        let db = CostDatabase::open_in_memory().unwrap();
        let now = Utc::now();

        // Insert and aggregate
        let calls = vec![ApiCall::new(
            now,
            "worker-1",
            "claude-opus",
            1000,
            500,
            0.10,
        )];
        db.insert_api_calls(&calls).unwrap();
        db.aggregate_hourly_stats(now).unwrap();

        // Query recent stats
        let stats = db.get_recent_hourly_stats(24).unwrap();
        assert!(!stats.is_empty());
    }

    #[test]
    fn test_get_recent_daily_stats() {
        let db = CostDatabase::open_in_memory().unwrap();
        let now = Utc::now();
        let today = now.date_naive();

        // Insert and aggregate
        let calls = vec![ApiCall::new(
            now,
            "worker-1",
            "claude-opus",
            1000,
            500,
            0.10,
        )];
        db.insert_api_calls(&calls).unwrap();
        db.aggregate_daily_stats(today).unwrap();

        // Query recent stats
        let stats = db.get_recent_daily_stats(7).unwrap();
        assert!(!stats.is_empty());
    }

    #[test]
    fn test_performance_tables_exist() {
        let db = CostDatabase::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();

        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        // Verify new performance tables exist
        assert!(tables.contains(&"hourly_stats".to_string()));
        assert!(tables.contains(&"daily_stats".to_string()));
        assert!(tables.contains(&"worker_efficiency".to_string()));
        assert!(tables.contains(&"model_performance".to_string()));
        assert!(tables.contains(&"task_events".to_string()));
    }
}
