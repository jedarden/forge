//! SQLite database layer for cost tracking.

use crate::error::{CostError, Result};
use crate::models::{ApiCall, CostBreakdown, DailyCost};
use chrono::{NaiveDate, Utc};
use rusqlite::{params, Connection, Transaction};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

/// Current schema version for migrations.
const SCHEMA_VERSION: i32 = 1;

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
}
