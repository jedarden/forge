# FORGE Database Schema

## Overview

FORGE uses SQLite as its primary data storage for cost tracking and analytics. The database is stored at `~/.forge/costs.db` and uses the bundled version of SQLite via the `rusqlite` crate.

## Database Location

```bash
~/.forge/costs.db
```

The database is automatically created on first run if it doesn't exist.

## Schema Version

Current schema version: **3**

Schema migrations are handled automatically by `CostDatabase::open()`. The `schema_version` table tracks the current version and applies migrations as needed.

## Core Tables

### api_calls

The primary table storing individual API call records.

```sql
CREATE TABLE api_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,        -- ISO 8601 datetime (UTC)
    worker_id TEXT NOT NULL,         -- e.g., "claude-code-opus-alpha"
    session_id TEXT,                 -- Optional session identifier
    model TEXT NOT NULL,             -- e.g., "claude-opus-4-5-20250101"
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_creation_tokens INTEGER DEFAULT 0,
    cache_read_tokens INTEGER DEFAULT 0,
    cost_usd REAL NOT NULL,         -- Cost in USD
    bead_id TEXT,                    -- Associated task ID (optional)
    event_type TEXT DEFAULT 'result'  -- Event type (result, assistant, etc.)
);
```

**Indexes:**
- `idx_api_calls_timestamp`: ON api_calls(timestamp DESC) - for time-range queries
- `idx_api_calls_worker`: ON api_calls(worker_id) - for worker filtering
- `idx_api_calls_model`: ON api_calls(model) - for model analytics
- `idx_api_calls_bead`: ON api_calls(bead_id) - for task cost attribution

**Notes:**
- `cache_creation_tokens` and `cache_read_tokens` are Anthropic-specific
- `event_type` distinguishes between different API event types
- `bead_id` allows cost attribution to specific tasks

### schema_version

Tracks the current database schema version for migrations.

```sql
CREATE TABLE schema_version (
    version INTEGER PRIMARY KEY
);
```

## Aggregation Tables

These tables are pre-computed for performance optimization.

### daily_costs

Daily cost summaries by model.

```sql
CREATE TABLE daily_costs (
    date TEXT NOT NULL,              -- ISO date (YYYY-MM-DD)
    model TEXT NOT NULL,
    call_count INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_creation_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER NOT NULL,
    total_tokens INTEGER NOT NULL,
    total_cost_usd REAL NOT NULL,
    avg_cost_per_call REAL NOT NULL,
    PRIMARY KEY (date, model)
);
```

### model_costs

Model-specific cost breakdowns.

```sql
CREATE TABLE model_costs (
    model TEXT PRIMARY KEY,
    call_count INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_creation_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER NOT NULL,
    total_cost_usd REAL NOT NULL,
    last_used TEXT NOT NULL
);
```

### hourly_stats

Hourly performance statistics.

```sql
CREATE TABLE hourly_stats (
    hour TEXT NOT NULL,             -- ISO datetime hour (YYYY-MM-DDTHH)
    model TEXT NOT NULL,
    call_count INTEGER NOT NULL,
    total_tokens INTEGER NOT NULL,
    total_cost_usd REAL NOT NULL,
    avg_tokens_per_call REAL NOT NULL,
    PRIMARY KEY (hour, model)
);
```

### daily_stats

Daily performance statistics.

```sql
CREATE TABLE daily_stats (
    date TEXT NOT NULL,
    call_count INTEGER NOT NULL,
    total_tokens INTEGER NOT NULL,
    total_cost_usd REAL NOT NULL,
    avg_cost_per_call REAL NOT NULL,
    peak_concurrent_calls INTEGER,
    PRIMARY KEY (date)
);
```

### worker_efficiency

Worker performance metrics.

```sql
CREATE TABLE worker_efficiency (
    worker_id TEXT PRIMARY KEY,
    call_count INTEGER NOT NULL,
    total_cost_usd REAL NOT NULL,
    total_tokens INTEGER NOT NULL,
    avg_cost_per_call REAL NOT NULL,
    avg_tokens_per_call REAL NOT NULL,
    efficiency_score REAL NOT NULL,
    last_active TEXT NOT NULL,
    tier TEXT NOT NULL               -- premium, standard, budget
);
```

### model_performance

Per-model performance analytics.

```sql
CREATE TABLE model_performance (
    model TEXT PRIMARY KEY,
    call_count INTEGER NOT NULL,
    total_tokens INTEGER NOT NULL,
    total_cost_usd REAL NOT NULL,
    avg_cost_per_1k_tokens REAL NOT NULL,
    avg_latency_ms REAL,
    cache_hit_rate REAL,
    last_updated TEXT NOT NULL
);
```

## Subscription Tracking Tables

### subscriptions

```sql
CREATE TABLE subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,         -- e.g., "anthropic", "openai"
    subscription_type TEXT NOT NULL,  -- e.g., "build", "pro", "team"
    quota_limit_i64 REAL NOT NULL,    -- Token limit
    renewal_date TEXT,                -- ISO date
    is_active INTEGER NOT NULL DEFAULT 1
);
```

### subscription_usage

```sql
CREATE TABLE subscription_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subscription_id INTEGER NOT NULL,
    period TEXT NOT NULL,            -- e.g., "2025-01"
    tokens_used_i64 NOT NULL,
    cost_usd REAL NOT NULL,
    recorded_at TEXT NOT NULL,
    FOREIGN KEY (subscription_id) REFERENCES subscriptions(id)
);
```

## Data Types Reference

### ApiCall

```rust
pub struct ApiCall {
    pub id: Option<i64>,
    pub timestamp: DateTime<Utc>,
    pub worker_id: String,
    pub session_id: Option<String>,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub cost_usd: f64,
    pub bead_id: Option<String>,
    pub event_type: String,
}
```

### CostBreakdown

```rust
pub struct CostBreakdown {
    pub model: String,
    pub call_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_cost_usd: f64,
}
```

### DailyCost

```rust
pub struct DailyCost {
    pub date: NaiveDate,
    pub model: String,
    pub call_count: i64,
    pub total_cost_usd: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
}
```

### WorkerCostBreakdown

```rust
pub struct WorkerCostBreakdown {
    pub worker_id: String,
    pub session_id: Option<String>,
    pub bead_id: Option<String>,
    pub model: String,
    pub call_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_cost_usd: f64,
    pub avg_cost_per_call: f64,
    pub is_expensive: bool,
}
```

### ModelPerformance

```rust
pub struct ModelPerformance {
    pub model: String,
    pub call_count: i64,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub avg_cost_per_1k_tokens: f64,
    pub avg_latency_ms: Option<f64>,
    pub cache_hit_rate: Option<f64>,
}
```

### Subscription

```rust
pub struct Subscription {
    pub id: i64,
    pub provider: String,
    pub subscription_type: SubscriptionType,
    pub quota_limit_i64: i64,
    pub renewal_date: Option<NaiveDate>,
    pub is_active: bool,
}
```

### SubscriptionUsageRecord

```rust
pub struct SubscriptionUsageRecord {
    pub id: i64,
    pub subscription_id: i64,
    pub period: String,
    pub tokens_used_i64: i64,
    pub cost_usd: f64,
    pub recorded_at: DateTime<Utc>,
}
```

## Database Operations

### Opening the Database

```rust
use forge_cost::CostDatabase;

// Open or create database at default path
let db = CostDatabase::open("~/.forge/costs.db")?;

// Open in-memory database for testing
let db = CostDatabase::open_in_memory()?;
```

### Inserting API Calls

```rust
use forge_cost::LogParser;
use forge_cost::models::ApiCall;

// Parse log files
let parser = LogParser::new();
let api_calls = parser.parse_directory("~/.forge/logs/")?;

// Insert into database
db.insert_api_calls(&api_calls)?;
```

### Querying Costs

```rust
use forge_cost::CostQuery;

let query = CostQuery::new(&db);

// Today's costs
let today = query.get_today_costs()?;

// Current month costs
let monthly = query.get_current_month_costs()?;

// Cost breakdown by model
let by_model = query.get_model_breakdown(start_date, end_date)?;

// Worker-specific costs
let worker_costs = query.get_worker_costs(start_date, end_date)?;

// Bead-attributed costs
let bead_costs = query.get_bead_costs(bead_id)?;
```

## Concurrency Handling

The database uses `Arc<Mutex<Connection>>` for thread-safe access:

```rust
pub struct CostDatabase {
    conn: Arc<Mutex<Connection>>,
}
```

### Lock Retry Logic

```rust
const DB_LOCK_MAX_TRIES: u32 = 5;
const DB_LOCK_INITIAL_DELAY_MS: u64 = 50;
const DB_LOCK_MAX_DELAY: Duration = Duration::from_secs(5);
```

When the database is locked, operations automatically retry with exponential backoff.

## Supported API Formats

### Anthropic (Claude)

```json
{
  "type": "result",
  "input_tokens": 1000,
  "output_tokens": 500,
  "cache_creation_input_tokens": 0,
  "cache_read_input_tokens": 100
}
```

### OpenAI

```json
{
  "prompt_tokens": 1000,
  "completion_tokens": 500
}
```

### DeepSeek

```json
{
  "input_tokens": 1000,
  "output_tokens": 500
}
```

### GLM (via z.ai proxy)

```json
{
  "type": "result",
  "modelUsage": {
    "input_tokens": 1000,
    "output_tokens": 500
  }
}
```

## Cost Calculations

### Token Cost Formula

```rust
total_cost = (input_tokens / 1_000_000 * input_cost_per_million)
          + (output_tokens / 1_000_000 * output_cost_per_million)
          + (cache_creation_tokens / 1_000_000 * cache_write_cost_per_million)
          + (cache_read_tokens / 1_000_000 * cache_read_cost_per_million)
```

### Example Pricing (USD per million tokens)

| Model | Input | Output | Cache Write | Cache Read |
|--------|--------|---------|--------------|--------------|
| Claude Opus | $15.00 | $75.00 | $18.75 | $1.50 |
| Claude Sonnet | $3.00 | $15.00 | $3.75 | $0.30 |
| Claude Haiku | $0.80 | $4.00 | $1.00 | $0.08 |
| GLM-4.7 | $1.00 | $2.00 | - | - |

## Performance Optimizations

### Indexes

Indexes on frequently queried columns:
- Time-based queries (timestamp)
- Worker filtering (worker_id)
- Model analytics (model)
- Task attribution (bead_id)

### Aggregation Tables

Pre-computed aggregations avoid expensive GROUP BY operations:
- Daily summaries for quick dashboard loading
- Model performance for optimization recommendations
- Worker efficiency for resource allocation

### Query Patterns

```rust
// Efficient time range query
"SELECT * FROM api_calls
 WHERE timestamp >= ? AND timestamp <= ?
 ORDER BY timestamp DESC"

// Efficient aggregation
"SELECT
     model,
     SUM(input_tokens) as input_tokens,
     SUM(output_tokens) as output_tokens,
     SUM(cost_usd) as total_cost
 FROM api_calls
 WHERE date >= ?
 GROUP BY model"
```

## Maintenance

### Vacuum

SQLite databases benefit from periodic VACUUM to reclaim space:

```bash
sqlite3 ~/.forge/costs.db "VACUUM;"
```

### Analyze

Check database health:

```bash
sqlite3 ~/.forge/costs.db "PRAGMA integrity_check;"
sqlite3 ~/.forge/costs.db "ANALYZE;"
```

### Backup

Simple file-based backup:

```bash
cp ~/.forge/costs.db ~/.forge/costs.db.backup
```

## Error Handling

### Database Locked

```rust
use forge_cost::CostError;

match db.insert_api_calls(&calls) {
    Ok(()) => {},
    Err(CostError::DatabaseLocked { retries }) => {
        eprintln!("Database locked after {} retries", retries);
    }
    Err(e) => {
        eprintln!("Insert failed: {}", e);
    }
}
```

### Migration Errors

Schema migrations are transactional - on error, the database rolls back:

```rust
db.migrate()?; // Atomic schema update
```

## Related Documentation

- [Architecture Overview](./ARCHITECTURE.md) - System design
- [Cost Tracking](../crates/forge-cost/src/lib.rs) - Implementation details
- [Worker System](./WORKERS.md) - Worker lifecycle
