//! Audit logging for compliance and security tracking.
//!
//! This module provides structured, append-only audit trails for all mutating
//! operations in FORGE, including worker lifecycle, bead status changes, and
//! configuration modifications.
//!
//! ## Features
//!
//! - Immutable append-only log stored in SQLite
//! - Comprehensive event tracking for compliance
//! - Queryable by time range, entity, actor, and event type
//! - Export to JSON/CSV formats
//! - Configurable retention policy
//!
//! ## Example
//!
//! ```no_run
//! use forge_core::audit::{AuditLogger, AuditEvent, EventType};
//!
//! # fn main() -> forge_core::Result<()> {
//! let logger = AuditLogger::open("~/.forge/audit.db")?;
//!
//! logger.log(AuditEvent {
//!     event_type: EventType::WorkerSpawn,
//!     actor: "user".to_string(),
//!     entity_type: "worker".to_string(),
//!     entity_id: "worker-1".to_string(),
//!     old_value: None,
//!     new_value: Some(r#"{"status":"active"}"#.to_string()),
//!     metadata: None,
//!     severity: Severity::Info,
//! })?;
//! # Ok(())
//! # }
//! ```

use crate::{ForgeError, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

/// Error type for parse failures that implements std::error::Error.
#[derive(Debug)]
struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// Current audit schema version.
const AUDIT_SCHEMA_VERSION: i32 = 1;

/// Default retention period for audit logs (90 days).
const DEFAULT_RETENTION_DAYS: i64 = 90;

/// Maximum number of records to return in a single query.
const MAX_QUERY_RESULTS: i64 = 10000;

/// Types of auditable events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// Worker spawned/created
    WorkerSpawn,
    /// Worker killed/stopped
    WorkerKill,
    /// Worker paused
    WorkerPause,
    /// Worker resumed
    WorkerResume,
    /// Bead status changed
    BeadStatusChange,
    /// Configuration value changed
    ConfigChange,
    /// Database schema migrated
    SchemaMigration,
    /// API call recorded (cost tracking)
    ApiCallRecorded,
    /// Cost aggregation performed
    CostAggregation,
    /// Subscription created/updated
    SubscriptionChange,
    /// Task event recorded
    TaskEvent,
    /// Manual user action
    UserAction,
    /// System automated action
    SystemAction,
    /// Error/failure event
    Error,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WorkerSpawn => write!(f, "worker_spawn"),
            Self::WorkerKill => write!(f, "worker_kill"),
            Self::WorkerPause => write!(f, "worker_pause"),
            Self::WorkerResume => write!(f, "worker_resume"),
            Self::BeadStatusChange => write!(f, "bead_status_change"),
            Self::ConfigChange => write!(f, "config_change"),
            Self::SchemaMigration => write!(f, "schema_migration"),
            Self::ApiCallRecorded => write!(f, "api_call_recorded"),
            Self::CostAggregation => write!(f, "cost_aggregation"),
            Self::SubscriptionChange => write!(f, "subscription_change"),
            Self::TaskEvent => write!(f, "task_event"),
            Self::UserAction => write!(f, "user_action"),
            Self::SystemAction => write!(f, "system_action"),
            Self::Error => write!(f, "error"),
        }
    }
}

impl std::str::FromStr for EventType {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "worker_spawn" => Ok(Self::WorkerSpawn),
            "worker_kill" => Ok(Self::WorkerKill),
            "worker_pause" => Ok(Self::WorkerPause),
            "worker_resume" => Ok(Self::WorkerResume),
            "bead_status_change" => Ok(Self::BeadStatusChange),
            "config_change" => Ok(Self::ConfigChange),
            "schema_migration" => Ok(Self::SchemaMigration),
            "api_call_recorded" => Ok(Self::ApiCallRecorded),
            "cost_aggregation" => Ok(Self::CostAggregation),
            "subscription_change" => Ok(Self::SubscriptionChange),
            "task_event" => Ok(Self::TaskEvent),
            "user_action" => Ok(Self::UserAction),
            "system_action" => Ok(Self::SystemAction),
            "error" => Ok(Self::Error),
            _ => Err(format!("unknown event type: {}", s)),
        }
    }
}

/// Severity level for audit events.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational event
    Info,
    /// Warning condition
    Warning,
    /// Error condition
    Error,
    /// Critical failure
    Critical,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

impl std::str::FromStr for Severity {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "info" => Ok(Self::Info),
            "warning" => Ok(Self::Warning),
            "error" => Ok(Self::Error),
            "critical" => Ok(Self::Critical),
            _ => Err(format!("unknown severity: {}", s)),
        }
    }
}

/// A single audit event record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// When the event occurred
    pub timestamp: DateTime<Utc>,
    /// Type of event
    pub event_type: EventType,
    /// Who performed the action (user ID, "system", worker_id, etc.)
    pub actor: String,
    /// Type of entity affected (worker, bead, config, etc.)
    pub entity_type: String,
    /// ID of the specific entity
    pub entity_id: String,
    /// Previous state (JSON encoded)
    pub old_value: Option<String>,
    /// New state (JSON encoded)
    pub new_value: Option<String>,
    /// Additional context (JSON encoded)
    pub metadata: Option<String>,
    /// Event severity
    pub severity: Severity,
}

impl AuditEvent {
    /// Create a new audit event with the current timestamp.
    pub fn new(
        event_type: EventType,
        actor: impl Into<String>,
        entity_type: impl Into<String>,
        entity_id: impl Into<String>,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            event_type,
            actor: actor.into(),
            entity_type: entity_type.into(),
            entity_id: entity_id.into(),
            old_value: None,
            new_value: None,
            metadata: None,
            severity: Severity::Info,
        }
    }

    /// Set the old value (previous state).
    pub fn with_old_value(mut self, value: impl Into<String>) -> Self {
        self.old_value = Some(value.into());
        self
    }

    /// Set the new value (new state).
    pub fn with_new_value(mut self, value: impl Into<String>) -> Self {
        self.new_value = Some(value.into());
        self
    }

    /// Set the metadata (additional context).
    pub fn with_metadata(mut self, value: impl Into<String>) -> Self {
        self.metadata = Some(value.into());
        self
    }

    /// Set the severity level.
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// Create a worker lifecycle event.
    pub fn worker_lifecycle(
        event_type: EventType,
        worker_id: impl Into<String>,
        actor: impl Into<String>,
    ) -> Self {
        Self::new(event_type, actor, "worker", worker_id)
    }

    /// Create a bead status change event.
    pub fn bead_status_change(
        bead_id: impl Into<String>,
        old_status: impl Into<String>,
        new_status: impl Into<String>,
        actor: impl Into<String>,
    ) -> Self {
        Self::new(EventType::BeadStatusChange, actor, "bead", bead_id)
            .with_old_value(old_status.into())
            .with_new_value(new_status.into())
    }

    /// Create a config change event.
    pub fn config_change(
        key: impl Into<String>,
        old_value: impl Into<String>,
        new_value: impl Into<String>,
        actor: impl Into<String>,
    ) -> Self {
        Self::new(EventType::ConfigChange, actor, "config", key)
            .with_old_value(old_value.into())
            .with_new_value(new_value.into())
    }
}

/// Filter criteria for querying audit logs.
#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    /// Start of time range (inclusive)
    pub start_time: Option<DateTime<Utc>>,
    /// End of time range (inclusive)
    pub end_time: Option<DateTime<Utc>>,
    /// Filter by event types
    pub event_types: Option<Vec<EventType>>,
    /// Filter by actor
    pub actor: Option<String>,
    /// Filter by entity type
    pub entity_type: Option<String>,
    /// Filter by entity ID
    pub entity_id: Option<String>,
    /// Filter by severity
    pub severity: Option<Severity>,
    /// Maximum number of results
    pub limit: Option<i64>,
    /// Offset for pagination
    pub offset: Option<i64>,
}

impl AuditFilter {
    /// Create a new empty filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set time range.
    pub fn with_time_range(mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        self.start_time = Some(start);
        self.end_time = Some(end);
        self
    }

    /// Set event types filter.
    pub fn with_event_types(mut self, types: Vec<EventType>) -> Self {
        self.event_types = Some(types);
        self
    }

    /// Set actor filter.
    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = Some(actor.into());
        self
    }

    /// Set entity filter.
    pub fn with_entity(mut self, entity_type: impl Into<String>, entity_id: impl Into<String>) -> Self {
        self.entity_type = Some(entity_type.into());
        self.entity_id = Some(entity_id.into());
        self
    }

    /// Set severity filter.
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = Some(severity);
        self
    }

    /// Set pagination.
    pub fn with_pagination(mut self, limit: i64, offset: i64) -> Self {
        self.limit = Some(limit);
        self.offset = Some(offset);
        self
    }

    /// Build SQL WHERE clause and parameters from this filter.
    fn build_where_clause(&self) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(start) = &self.start_time {
            conditions.push("timestamp >= ?".to_string());
            params.push(Box::new(start.to_rfc3339()));
        }
        if let Some(end) = &self.end_time {
            conditions.push("timestamp <= ?".to_string());
            params.push(Box::new(end.to_rfc3339()));
        }
        if let Some(types) = &self.event_types {
            let placeholders: Vec<String> = (0..types.len()).map(|_| "?".to_string()).collect();
            conditions.push(format!("event_type IN ({})", placeholders.join(", ")));
            for t in types {
                params.push(Box::new(t.to_string()));
            }
        }
        if let Some(actor) = &self.actor {
            conditions.push("actor = ?".to_string());
            params.push(Box::new(actor.clone()));
        }
        if let Some(entity_type) = &self.entity_type {
            conditions.push("entity_type = ?".to_string());
            params.push(Box::new(entity_type.clone()));
        }
        if let Some(entity_id) = &self.entity_id {
            conditions.push("entity_id = ?".to_string());
            params.push(Box::new(entity_id.clone()));
        }
        if let Some(severity) = &self.severity {
            conditions.push("severity = ?".to_string());
            params.push(Box::new(severity.to_string()));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        (where_clause, params)
    }
}

/// Retention policy for audit logs.
#[derive(Debug, Clone)]
pub struct RetentionPolicy {
    /// Number of days to retain logs
    pub days: i64,
    /// Whether to apply retention on startup
    pub apply_on_startup: bool,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            days: DEFAULT_RETENTION_DAYS,
            apply_on_startup: true,
        }
    }
}

impl RetentionPolicy {
    /// Create a new retention policy.
    pub fn new(days: i64, apply_on_startup: bool) -> Self {
        Self {
            days: days.max(1), // At least 1 day
            apply_on_startup,
        }
    }

    /// Get the cutoff timestamp for this policy.
    pub fn cutoff(&self) -> DateTime<Utc> {
        Utc::now() - chrono::Duration::days(self.days)
    }
}

/// SQLite-backed audit logger.
#[derive(Clone)]
pub struct AuditLogger {
    conn: Arc<Mutex<Connection>>,
    db_path: PathBuf,
    retention: RetentionPolicy,
}

impl AuditLogger {
    /// Open or create an audit database at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db_path = path.as_ref().to_path_buf();

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| ForgeError::io("create_dir_all", parent, e))?;
        }

        let conn = Connection::open(&db_path)
            .map_err(|e| ForgeError::Audit { message: format!("failed to open database: {}", e) })?;

        // Enable WAL mode for better concurrent access
        // Returns the new mode, so we need to use query_row
        conn.query_row("PRAGMA journal_mode=WAL", [], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| ForgeError::Audit { message: format!("failed to enable WAL: {}", e) })?;

        // Set a busy timeout for concurrent access
        conn.query_row("PRAGMA busy_timeout=5000", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|e| ForgeError::Audit { message: format!("failed to set busy timeout: {}", e) })?;

        let mut logger = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
            retention: RetentionPolicy::default(),
        };

        logger.migrate()?;

        // Apply retention if configured
        if logger.retention.apply_on_startup {
            logger.apply_retention()?;
        }

        Ok(logger)
    }

    /// Open with a custom retention policy.
    pub fn open_with_retention<P: AsRef<Path>>(
        path: P,
        retention: RetentionPolicy,
    ) -> Result<Self> {
        let mut logger = Self::open(path)?;
        logger.retention = retention.clone();
        if retention.apply_on_startup {
            logger.apply_retention()?;
        }
        Ok(logger)
    }

    /// Get the database path.
    pub fn path(&self) -> &Path {
        &self.db_path
    }

    /// Get the current retention policy.
    pub fn retention(&self) -> &RetentionPolicy {
        &self.retention
    }

    /// Update the retention policy.
    pub fn set_retention(&mut self, retention: RetentionPolicy) -> Result<()> {
        self.retention = retention.clone();
        self.apply_retention().map(|_| ())
    }

    /// Run database migrations.
    fn migrate(&mut self) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            ForgeError::audit_error(format!("failed to acquire lock: {}", e))
        })?;

        // Create schema version table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS audit_schema_version (
                version INTEGER PRIMARY KEY
            )",
            [],
        ).map_err(|e| ForgeError::audit_error(format!("failed to create schema table: {}", e)))?;

        // Get current version
        let current_version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM audit_schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if current_version < AUDIT_SCHEMA_VERSION {
            info!(
                current = current_version,
                target = AUDIT_SCHEMA_VERSION,
                "Running audit database migrations"
            );

            // Create audit logs table
            conn.execute(
                "CREATE TABLE IF NOT EXISTS audit_logs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL,
                    event_type TEXT NOT NULL,
                    actor TEXT NOT NULL,
                    entity_type TEXT NOT NULL,
                    entity_id TEXT NOT NULL,
                    old_value TEXT,
                    new_value TEXT,
                    metadata TEXT,
                    severity TEXT NOT NULL DEFAULT 'info'
                )",
                [],
            ).map_err(|e| ForgeError::audit_error(format!("failed to create audit_logs table: {}", e)))?;

            // Create indexes for efficient queries
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_audit_logs_timestamp
                 ON audit_logs(timestamp DESC)",
                [],
            ).map_err(|e| ForgeError::audit_error(format!("failed to create timestamp index: {}", e)))?;

            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_audit_logs_event_type
                 ON audit_logs(event_type)",
                [],
            ).map_err(|e| ForgeError::audit_error(format!("failed to create event_type index: {}", e)))?;

            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_audit_logs_actor
                 ON audit_logs(actor)",
                [],
            ).map_err(|e| ForgeError::audit_error(format!("failed to create actor index: {}", e)))?;

            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_audit_logs_entity
                 ON audit_logs(entity_type, entity_id)",
                [],
            ).map_err(|e| ForgeError::audit_error(format!("failed to create entity index: {}", e)))?;

            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_audit_logs_severity
                 ON audit_logs(severity)",
                [],
            ).map_err(|e| ForgeError::audit_error(format!("failed to create severity index: {}", e)))?;

            // Record schema version
            conn.execute(
                "INSERT OR REPLACE INTO audit_schema_version (version) VALUES (?)",
                params![AUDIT_SCHEMA_VERSION],
            ).map_err(|e| ForgeError::audit_error(format!("failed to record schema version: {}", e)))?;

            info!("Audit database initialized at schema version {}", AUDIT_SCHEMA_VERSION);
        }

        Ok(())
    }

    /// Log an audit event.
    pub fn log(&self, event: AuditEvent) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| {
            ForgeError::audit_error(format!("failed to acquire lock: {}", e))
        })?;

        debug!(
            event_type = %event.event_type,
            actor = %event.actor,
            entity = %format!("{}:{}", event.entity_type, event.entity_id),
            "Logging audit event"
        );

        conn.execute(
            "INSERT INTO audit_logs (
                timestamp, event_type, actor, entity_type, entity_id,
                old_value, new_value, metadata, severity
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                event.timestamp.to_rfc3339(),
                event.event_type.to_string(),
                event.actor,
                event.entity_type,
                event.entity_id,
                event.old_value,
                event.new_value,
                event.metadata,
                event.severity.to_string(),
            ],
        )
        .map_err(|e| ForgeError::audit_error(format!("failed to insert audit log: {}", e)))?;

        Ok(conn.last_insert_rowid())
    }

    /// Query audit logs with optional filters.
    pub fn query(&self, filter: &AuditFilter) -> Result<Vec<AuditEvent>> {
        let conn = self.conn.lock().map_err(|e| {
            ForgeError::audit_error(format!("failed to acquire lock: {}", e))
        })?;

        let (where_clause, params) = filter.build_where_clause();
        let limit = filter.limit.unwrap_or(MAX_QUERY_RESULTS);
        let offset = filter.offset.unwrap_or(0);

        let sql = format!(
            "SELECT timestamp, event_type, actor, entity_type, entity_id,
                    old_value, new_value, metadata, severity
             FROM audit_logs
             {}
             ORDER BY timestamp DESC
             LIMIT {} OFFSET {}",
            where_clause, limit, offset
        );

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| ForgeError::audit_error(format!("failed to prepare query: {}", e)))?;

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        // Helper to parse timestamp from string
        let parse_timestamp = |s: String| -> std::result::Result<DateTime<Utc>, String> {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| format!("invalid timestamp: {}", e))
        };

        // Helper to parse event type from string
        let parse_event_type = |s: String| -> std::result::Result<EventType, String> {
            s.parse().map_err(|e| format!("invalid event type: {}", e))
        };

        // Helper to parse severity from string
        let parse_severity = |s: String| -> std::result::Result<Severity, String> {
            s.parse().map_err(|e| format!("invalid severity: {}", e))
        };

        let events = stmt
            .query_map(param_refs.as_slice(), |row| {
                let ts_str: String = row.get(0)?;
                let event_type_str: String = row.get(1)?;
                let severity_str: String = row.get(8)?;

                Ok(AuditEvent {
                    timestamp: parse_timestamp(ts_str).map_err(|e| {
                        rusqlite::Error::ToSqlConversionFailure(
                            Box::new(ParseError(e))
                        )
                    })?,
                    event_type: parse_event_type(event_type_str).map_err(|e| {
                        rusqlite::Error::ToSqlConversionFailure(
                            Box::new(ParseError(e))
                        )
                    })?,
                    actor: row.get(2)?,
                    entity_type: row.get(3)?,
                    entity_id: row.get(4)?,
                    old_value: row.get(5)?,
                    new_value: row.get(6)?,
                    metadata: row.get(7)?,
                    severity: parse_severity(severity_str).map_err(|e| {
                        rusqlite::Error::ToSqlConversionFailure(
                            Box::new(ParseError(e))
                        )
                    })?,
                })
            })
            .map_err(|e| ForgeError::audit_error(format!("failed to execute query: {}", e)))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| ForgeError::audit_error(format!("failed to parse results: {}", e)))?;

        Ok(events)
    }

    /// Get a count of audit logs matching the filter.
    pub fn count(&self, filter: &AuditFilter) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| {
            ForgeError::audit_error(format!("failed to acquire lock: {}", e))
        })?;

        let (where_clause, params) = filter.build_where_clause();

        let sql = format!(
            "SELECT COUNT(*) FROM audit_logs {}",
            where_clause
        );

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| ForgeError::audit_error(format!("failed to prepare count query: {}", e)))?;

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let count: i64 = stmt
            .query_row(param_refs.as_slice(), |row| row.get(0))
            .map_err(|e| ForgeError::audit_error(format!("failed to execute count query: {}", e)))?;

        Ok(count)
    }

    /// Get audit logs for a specific entity.
    pub fn get_entity_history(
        &self,
        entity_type: &str,
        entity_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<AuditEvent>> {
        let filter = AuditFilter::new()
            .with_entity(entity_type, entity_id)
            .with_pagination(limit.unwrap_or(1000), 0);

        self.query(&filter)
    }

    /// Apply retention policy - delete old logs.
    pub fn apply_retention(&self) -> Result<usize> {
        let cutoff = self.retention.cutoff();
        let cutoff_str = cutoff.to_rfc3339();

        info!(
            cutoff = %cutoff_str,
            days = self.retention.days,
            "Applying audit log retention"
        );

        let conn = self.conn.lock().map_err(|e| {
            ForgeError::audit_error(format!("failed to acquire lock: {}", e))
        })?;

        let affected = conn
            .execute(
                "DELETE FROM audit_logs WHERE timestamp < ?",
                params![cutoff_str],
            )
            .map_err(|e| ForgeError::audit_error(format!("failed to apply retention: {}", e)))?;

        if affected > 0 {
            info!(
                deleted = affected,
                "Deleted audit logs past retention period"
            );
        }

        Ok(affected)
    }

    /// Export audit logs to JSON.
    pub fn export_json(&self, filter: &AuditFilter, output: &Path) -> Result<()> {
        let events = self.query(filter)?;

        let json = serde_json::to_string_pretty(&events)
            .map_err(|e| ForgeError::audit_error(format!("failed to serialize JSON: {}", e)))?;

        fs::write(output, json)
            .map_err(|e| ForgeError::audit_error(format!("failed to write export: {}", e)))?;

        info!(
            count = events.len(),
            path = %output.display(),
            "Exported audit logs to JSON"
        );

        Ok(())
    }

    /// Export audit logs to CSV.
    pub fn export_csv(&self, filter: &AuditFilter, output: &Path) -> Result<()> {
        let events = self.query(filter)?;

        let mut wtr = csv::Writer::from_path(output)
            .map_err(|e| ForgeError::audit_error(format!("failed to create CSV writer: {}", e)))?;

        // Write header
        wtr.write_record(&[
            "timestamp", "event_type", "actor", "entity_type", "entity_id",
            "old_value", "new_value", "metadata", "severity",
        ])
        .map_err(|e| ForgeError::audit_error(format!("failed to write CSV header: {}", e)))?;

        // Write records
        for event in &events {
            wtr.write_record(&[
                event.timestamp.to_rfc3339().as_str(),
                &event.event_type.to_string(),
                &event.actor,
                &event.entity_type,
                &event.entity_id,
                event.old_value.as_deref().unwrap_or(""),
                event.new_value.as_deref().unwrap_or(""),
                event.metadata.as_deref().unwrap_or(""),
                &event.severity.to_string(),
            ])
            .map_err(|e| ForgeError::audit_error(format!("failed to write CSV record: {}", e)))?;
        }

        wtr.flush()
            .map_err(|e| ForgeError::audit_error(format!("failed to flush CSV: {}", e)))?;

        info!(
            count = events.len(),
            path = %output.display(),
            "Exported audit logs to CSV"
        );

        Ok(())
    }

    /// Get statistics about the audit log.
    pub fn stats(&self) -> Result<AuditStats> {
        let conn = self.conn.lock().map_err(|e| {
            ForgeError::audit_error(format!("failed to acquire lock: {}", e))
        })?;

        let total_events: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_logs", [], |row| row.get(0))
            .unwrap_or(0);

        let oldest_event: Option<String> = conn
            .query_row(
                "SELECT MIN(timestamp) FROM audit_logs",
                [],
                |row| row.get(0),
            )
            .unwrap_or(None);

        let newest_event: Option<String> = conn
            .query_row(
                "SELECT MAX(timestamp) FROM audit_logs",
                [],
                |row| row.get(0),
            )
            .unwrap_or(None);

        // Count by event type
        let mut type_counts = std::collections::HashMap::new();
        let mut stmt = conn
            .prepare(
                "SELECT event_type, COUNT(*) as count FROM audit_logs GROUP BY event_type"
            )
            .map_err(|e| ForgeError::audit_error(format!("failed to prepare stats query: {}", e)))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?
                ))
            })
            .map_err(|e| ForgeError::audit_error(format!("failed to execute stats query: {}", e)))?;

        for row in rows {
            if let Ok((event_type, count)) = row {
                type_counts.insert(event_type, count);
            }
        }

        Ok(AuditStats {
            total_events,
            oldest_timestamp: oldest_event.and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            newest_timestamp: newest_event.and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            event_type_counts: type_counts,
        })
    }

    /// Vacuum the database to reclaim space.
    pub fn vacuum(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            ForgeError::audit_error(format!("failed to acquire lock: {}", e))
        })?;

        info!("Vacuuming audit database");

        conn.execute("VACUUM", [])
            .map_err(|e| ForgeError::audit_error(format!("failed to vacuum: {}", e)))?;

        Ok(())
    }
}

/// Statistics about the audit log.
#[derive(Debug, Clone)]
pub struct AuditStats {
    /// Total number of events
    pub total_events: i64,
    /// Oldest event timestamp
    pub oldest_timestamp: Option<DateTime<Utc>>,
    /// Newest event timestamp
    pub newest_timestamp: Option<DateTime<Utc>>,
    /// Count of events by type
    pub event_type_counts: std::collections::HashMap<String, i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_audit_event_creation() {
        let event = AuditEvent::worker_lifecycle(
            EventType::WorkerSpawn,
            "worker-1",
            "user"
        )
        .with_new_value(r#"{"status":"active"}"#)
        .with_severity(Severity::Info);

        assert_eq!(event.event_type, EventType::WorkerSpawn);
        assert_eq!(event.actor, "user");
        assert_eq!(event.entity_type, "worker");
        assert_eq!(event.entity_id, "worker-1");
        assert_eq!(event.severity, Severity::Info);
    }

    #[test]
    fn test_audit_logger() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test-audit.db");

        let logger = AuditLogger::open(&db_path).unwrap();

        // Log an event
        let event = AuditEvent::worker_lifecycle(
            EventType::WorkerSpawn,
            "worker-test",
            "test"
        );
        let id = logger.log(event).unwrap();
        assert!(id > 0);

        // Query it back
        let filter = AuditFilter::new()
            .with_entity("worker", "worker-test");
        let results = logger.query(&filter).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_type, EventType::WorkerSpawn);
        assert_eq!(results[0].entity_id, "worker-test");
    }

    #[test]
    fn test_filter_building() {
        let filter = AuditFilter::new()
            .with_time_range(
                DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").unwrap().with_timezone(&Utc),
                DateTime::parse_from_rfc3339("2024-12-31T23:59:59Z").unwrap().with_timezone(&Utc),
            )
            .with_actor("user")
            .with_entity("worker", "worker-1");

        let (where_clause, params) = filter.build_where_clause();

        assert!(where_clause.contains("timestamp >="));
        assert!(where_clause.contains("timestamp <="));
        assert!(where_clause.contains("actor ="));
        assert!(where_clause.contains("entity_type ="));
        assert!(where_clause.contains("entity_id ="));
        assert_eq!(params.len(), 5);
    }

    #[test]
    fn test_event_type_from_str() {
        assert_eq!(
            "worker_spawn".parse::<EventType>().unwrap(),
            EventType::WorkerSpawn
        );
        assert_eq!(
            "config_change".parse::<EventType>().unwrap(),
            EventType::ConfigChange
        );
        assert!("unknown".parse::<EventType>().is_err());
    }

    #[test]
    fn test_severity_from_str() {
        assert_eq!("info".parse::<Severity>().unwrap(), Severity::Info);
        assert_eq!("critical".parse::<Severity>().unwrap(), Severity::Critical);
        assert!("unknown".parse::<Severity>().is_err());
    }

    #[test]
    fn test_retention_policy() {
        let policy = RetentionPolicy::new(30, false);
        assert_eq!(policy.days, 30);
        assert!(!policy.apply_on_startup);

        let cutoff = policy.cutoff();
        let now = Utc::now();
        let days_diff = (now - cutoff).num_days();
        assert!((days_diff - 30).abs() <= 1); // Allow 1 day tolerance
    }

    #[test]
    fn test_stats() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test-stats.db");

        let logger = AuditLogger::open(&db_path).unwrap();

        // Log some events
        logger.log(AuditEvent::worker_lifecycle(EventType::WorkerSpawn, "w1", "u1")).unwrap();
        logger.log(AuditEvent::worker_lifecycle(EventType::WorkerSpawn, "w2", "u1")).unwrap();
        logger.log(AuditEvent::worker_lifecycle(EventType::WorkerKill, "w1", "u1")).unwrap();

        let stats = logger.stats().unwrap();
        assert_eq!(stats.total_events, 3);
        assert!(stats.oldest_timestamp.is_some());
        assert!(stats.newest_timestamp.is_some());
        assert_eq!(stats.event_type_counts.get("worker_spawn").map(|v| *v as usize), Some(2));
        assert_eq!(stats.event_type_counts.get("worker_kill").map(|v| *v as usize), Some(1));
    }

    #[test]
    fn test_bead_status_change_helper() {
        let event = AuditEvent::bead_status_change(
            "bd-123",
            "open",
            "in_progress",
            "worker-1"
        );

        assert_eq!(event.event_type, EventType::BeadStatusChange);
        assert_eq!(event.entity_id, "bd-123");
        assert_eq!(event.old_value, Some("open".to_string()));
        assert_eq!(event.new_value, Some("in_progress".to_string()));
        assert_eq!(event.actor, "worker-1");
    }

    #[test]
    fn test_config_change_helper() {
        let event = AuditEvent::config_change(
            "max_workers",
            "5",
            "10",
            "user"
        );

        assert_eq!(event.event_type, EventType::ConfigChange);
        assert_eq!(event.entity_id, "max_workers");
        assert_eq!(event.old_value, Some("5".to_string()));
        assert_eq!(event.new_value, Some("10".to_string()));
        assert_eq!(event.actor, "user");
    }
}
