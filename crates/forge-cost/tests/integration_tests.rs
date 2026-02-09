//! Integration tests for forge-cost with mock log files.

use chrono::Utc;
use forge_cost::{ApiCall, CostDatabase, CostQuery, LogParser};
use std::io::Write;
use tempfile::{tempdir, NamedTempFile};

/// Create a mock log file with various API events.
fn create_mock_log_file(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::with_suffix(".log").unwrap();
    file.write_all(content.as_bytes()).unwrap();
    file.flush().unwrap();
    file
}

/// Mock log content for Anthropic Claude format.
const CLAUDE_LOG_CONTENT: &str = r#"[2026-02-08 14:40:22] [INFO] Log rotated
{"type":"system","subtype":"init","cwd":"/home/coder/forge","session_id":"session-1"}
{"type":"assistant","message":{"model":"claude-opus-4-5-20251101","id":"msg_01","type":"message","role":"assistant","content":[],"usage":{"input_tokens":1000,"output_tokens":500,"cache_creation_input_tokens":2000,"cache_read_input_tokens":10000}},"session_id":"session-1"}
{"type":"assistant","message":{"model":"claude-opus-4-5-20251101","id":"msg_02","type":"message","role":"assistant","content":[],"usage":{"input_tokens":500,"output_tokens":200,"cache_creation_input_tokens":0,"cache_read_input_tokens":5000}},"session_id":"session-1"}
{"type":"result","subtype":"success","is_error":false,"duration_ms":100000,"total_cost_usd":1.5,"usage":{"input_tokens":1500,"output_tokens":700,"cache_creation_input_tokens":2000,"cache_read_input_tokens":15000},"session_id":"session-1"}
"#;

/// Mock log content for GLM (z.ai proxy) format.
const GLM_LOG_CONTENT: &str = r#"{"type":"system","subtype":"init","cwd":"/home/coder/botburrow-hub","session_id":"session-2"}
{"type":"assistant","message":{"model":"glm-4.7","id":"msg_01","type":"message","role":"assistant","content":[],"usage":{"input_tokens":5000,"output_tokens":2000,"cache_creation_input_tokens":0,"cache_read_input_tokens":50000}},"session_id":"session-2"}
{"type":"result","subtype":"success","total_cost_usd":0.35,"usage":{"input_tokens":5000,"output_tokens":2000,"cache_creation_input_tokens":0,"cache_read_input_tokens":50000},"modelUsage":{"glm-4.7":{"inputTokens":5000,"outputTokens":2000}},"session_id":"session-2"}
"#;

#[test]
fn test_parse_claude_log_file() {
    let log_file = create_mock_log_file(CLAUDE_LOG_CONTENT);
    let parser = LogParser::new();

    let calls = parser.parse_file(log_file.path()).unwrap();

    // Should have 3 events: 2 assistant + 1 result
    assert_eq!(calls.len(), 3);

    // First assistant event
    assert_eq!(calls[0].model, "claude-opus-4-5-20251101");
    assert_eq!(calls[0].input_tokens, 1000);
    assert_eq!(calls[0].output_tokens, 500);
    assert_eq!(calls[0].event_type, "assistant");

    // Result event (last)
    assert_eq!(calls[2].event_type, "result");
    assert!((calls[2].cost_usd - 1.5).abs() < 0.0001);
}

#[test]
fn test_parse_glm_log_file() {
    let log_file = create_mock_log_file(GLM_LOG_CONTENT);
    let parser = LogParser::new();

    let calls = parser.parse_file(log_file.path()).unwrap();

    // Should have 2 events: 1 assistant + 1 result
    assert_eq!(calls.len(), 2);

    // Result event should extract model from modelUsage
    let result_event = calls.iter().find(|c| c.event_type == "result").unwrap();
    assert_eq!(result_event.model, "glm-4.7");
    assert!((result_event.cost_usd - 0.35).abs() < 0.0001);
}

#[test]
fn test_parse_directory_multiple_files() {
    let dir = tempdir().unwrap();

    // Create worker log files
    let opus_log_path = dir.path().join("claude-code-opus-alpha.log");
    std::fs::write(&opus_log_path, CLAUDE_LOG_CONTENT).unwrap();

    let glm_log_path = dir.path().join("claude-code-glm-47-alpha.log");
    std::fs::write(&glm_log_path, GLM_LOG_CONTENT).unwrap();

    let parser = LogParser::new();
    let calls = parser.parse_directory(dir.path()).unwrap();

    // Should have 5 total events (3 from Claude + 2 from GLM)
    assert_eq!(calls.len(), 5);

    // Verify worker IDs extracted from filenames
    let opus_calls: Vec<_> = calls.iter().filter(|c| c.worker_id == "claude-code-opus-alpha").collect();
    let glm_calls: Vec<_> = calls.iter().filter(|c| c.worker_id == "claude-code-glm-47-alpha").collect();

    assert_eq!(opus_calls.len(), 3);
    assert_eq!(glm_calls.len(), 2);
}

#[test]
fn test_full_pipeline_log_to_query() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("costs.db");

    // Create mock log
    let log_path = dir.path().join("test-worker.log");
    std::fs::write(&log_path, CLAUDE_LOG_CONTENT).unwrap();

    // Parse logs
    let parser = LogParser::new();
    let calls = parser.parse_file(&log_path).unwrap();

    // Insert into database
    let db = CostDatabase::open(&db_path).unwrap();
    let inserted = db.insert_api_calls(&calls).unwrap();
    assert_eq!(inserted, 3);

    // Query costs
    let query = CostQuery::new(&db);
    let today = query.get_today_costs().unwrap();

    assert_eq!(today.call_count, 3);
    assert!(today.total_cost_usd > 0.0);
}

#[test]
fn test_insert_performance() {
    use std::time::Instant;

    let db = CostDatabase::open_in_memory().unwrap();

    // Create 1000 API calls
    let calls: Vec<ApiCall> = (0..1000)
        .map(|i| {
            ApiCall::new(
                Utc::now(),
                format!("worker-{}", i % 10),
                if i % 3 == 0 { "claude-opus" } else { "claude-sonnet" },
                (100 + i * 10) as i64,
                (50 + i * 5) as i64,
                0.01 + (i as f64 * 0.001),
            )
        })
        .collect();

    let start = Instant::now();
    let inserted = db.insert_api_calls(&calls).unwrap();
    let duration = start.elapsed();

    assert_eq!(inserted, 1000);

    // Acceptance criteria: <100ms per API call (1000 calls should be <100s)
    // In practice, batched insert should be much faster
    assert!(duration.as_millis() < 5000, "Insert took too long: {:?}", duration);
}

#[test]
fn test_query_performance() {
    use std::time::Instant;

    let db = CostDatabase::open_in_memory().unwrap();

    // Insert sample data
    let calls: Vec<ApiCall> = (0..500)
        .map(|i| {
            ApiCall::new(
                Utc::now(),
                format!("worker-{}", i % 10),
                "claude-opus",
                1000,
                500,
                1.0,
            )
        })
        .collect();
    db.insert_api_calls(&calls).unwrap();

    let query = CostQuery::new(&db);

    // Measure get_today_costs
    let start = Instant::now();
    let _today = query.get_today_costs().unwrap();
    let duration = start.elapsed();

    // Acceptance criteria: <50ms for dashboard queries
    assert!(duration.as_millis() < 50, "Query took too long: {:?}", duration);
}

#[test]
fn test_log_rotation_handling() {
    // Log file with rotation message at the start
    let content = r#"[2026-02-08 14:40:22] [INFO] [worker] Log rotated (size: 10602201 bytes)
[2026-02-08 14:40:22] [SUCCESS] [worker] Bead bd-nkk7 completed successfully
{"type":"assistant","message":{"model":"claude-opus-4-5-20251101","id":"msg_01","type":"message","role":"assistant","content":[],"usage":{"input_tokens":100,"output_tokens":50}},"session_id":"session-1"}
{"type":"result","subtype":"success","total_cost_usd":0.01,"usage":{"input_tokens":100,"output_tokens":50},"session_id":"session-1"}
"#;

    let log_file = create_mock_log_file(content);
    let parser = LogParser::new();

    let calls = parser.parse_file(log_file.path()).unwrap();

    // Should skip non-JSON lines and parse the 2 valid events
    assert_eq!(calls.len(), 2);
}

#[test]
fn test_corrupted_json_handling() {
    let content = r#"{"type":"assistant","message":{"model":"claude-opus-4-5-20251101","usage":{"input_tokens":100,"output_tokens":50}},"session_id":"session-1"}
{"type":"assistant","message":CORRUPTED_JSON}
{"type":"result","subtype":"success","total_cost_usd":0.01,"usage":{"input_tokens":100,"output_tokens":50},"session_id":"session-1"}
"#;

    let log_file = create_mock_log_file(content);
    let parser = LogParser::new();

    let calls = parser.parse_file(log_file.path()).unwrap();

    // Should parse 2 valid events, skipping corrupted one
    assert_eq!(calls.len(), 2);
}

#[test]
fn test_bead_cost_tracking() {
    let db = CostDatabase::open_in_memory().unwrap();

    let calls = vec![
        ApiCall::new(Utc::now(), "worker-1", "claude-opus", 1000, 500, 1.0)
            .with_bead("fg-a8z"),
        ApiCall::new(Utc::now(), "worker-1", "claude-opus", 2000, 1000, 2.0)
            .with_bead("fg-a8z"),
        ApiCall::new(Utc::now(), "worker-2", "claude-sonnet", 500, 250, 0.1)
            .with_bead("fg-b9y"),
    ];
    db.insert_api_calls(&calls).unwrap();

    let query = CostQuery::new(&db);

    // Check cost for fg-a8z
    let task_cost = query.get_cost_per_task("fg-a8z").unwrap();
    assert_eq!(task_cost.call_count, 2);
    assert!((task_cost.total_cost_usd - 3.0).abs() < 0.0001);

    // Check cost for fg-b9y
    let task_cost = query.get_cost_per_task("fg-b9y").unwrap();
    assert_eq!(task_cost.call_count, 1);
    assert!((task_cost.total_cost_usd - 0.1).abs() < 0.0001);
}

#[test]
fn test_model_aggregation() {
    let db = CostDatabase::open_in_memory().unwrap();

    let calls = vec![
        ApiCall::new(Utc::now(), "worker-1", "claude-opus", 1000, 500, 10.0),
        ApiCall::new(Utc::now(), "worker-1", "claude-opus", 1000, 500, 10.0),
        ApiCall::new(Utc::now(), "worker-1", "claude-sonnet", 1000, 500, 1.0),
        ApiCall::new(Utc::now(), "worker-2", "claude-haiku", 1000, 500, 0.1),
    ];
    db.insert_api_calls(&calls).unwrap();

    let query = CostQuery::new(&db);
    let model_costs = query.get_model_costs(None, None).unwrap();

    assert_eq!(model_costs.len(), 3);

    // Should be sorted by cost descending
    assert_eq!(model_costs[0].model, "claude-opus");
    assert!((model_costs[0].total_cost_usd - 20.0).abs() < 0.0001);
    assert_eq!(model_costs[0].call_count, 2);

    assert_eq!(model_costs[1].model, "claude-sonnet");
    assert_eq!(model_costs[2].model, "claude-haiku");
}

#[test]
fn test_projected_costs_calculation() {
    let db = CostDatabase::open_in_memory().unwrap();

    // Insert $100 of costs for today
    let calls = vec![
        ApiCall::new(Utc::now(), "worker-1", "claude-opus", 1000, 500, 100.0),
    ];
    db.insert_api_calls(&calls).unwrap();

    let query = CostQuery::new(&db);
    let projected = query.get_projected_costs(Some(20)).unwrap();

    // With 20 days remaining, should project based on current daily rate
    assert_eq!(projected.current_total, 100.0);
    assert_eq!(projected.days_remaining, 20);
    assert!(projected.projected_total >= projected.current_total);
}

#[test]
fn test_cache_token_tracking() {
    let db = CostDatabase::open_in_memory().unwrap();

    let calls = vec![
        ApiCall::new(Utc::now(), "worker-1", "claude-opus", 100, 50, 0.5)
            .with_cache(1000, 5000),
    ];
    db.insert_api_calls(&calls).unwrap();

    let query = CostQuery::new(&db);
    let today = query.get_today_costs().unwrap();

    assert_eq!(today.call_count, 1);

    let model = &today.by_model[0];
    assert_eq!(model.input_tokens, 100);
    assert_eq!(model.output_tokens, 50);
    assert_eq!(model.cache_creation_tokens, 1000);
    assert_eq!(model.cache_read_tokens, 5000);
}

#[test]
fn test_empty_database_queries() {
    let db = CostDatabase::open_in_memory().unwrap();
    let query = CostQuery::new(&db);

    // Should handle empty database gracefully
    let today = query.get_today_costs().unwrap();
    assert_eq!(today.call_count, 0);
    assert_eq!(today.total_cost_usd, 0.0);
    assert!(today.by_model.is_empty());

    let model_costs = query.get_model_costs(None, None).unwrap();
    assert!(model_costs.is_empty());

    let top_workers = query.get_top_workers(10).unwrap();
    assert!(top_workers.is_empty());
}

// ============================================================
// Extended Integration Tests for Comprehensive Coverage
// ============================================================

/// Test database schema migration on fresh database.
#[test]
fn test_database_migration_from_scratch() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("new_db.db");

    // Create new database - should run all migrations
    let db = CostDatabase::open(&db_path).unwrap();

    // Verify we can insert and query
    let calls = vec![ApiCall::new(
        Utc::now(),
        "test-worker",
        "claude-opus",
        100,
        50,
        1.0,
    )];
    let inserted = db.insert_api_calls(&calls).unwrap();
    assert_eq!(inserted, 1);

    let query = CostQuery::new(&db);
    let today = query.get_today_costs().unwrap();
    assert_eq!(today.call_count, 1);
}

/// Test database reopening preserves data.
#[test]
fn test_database_persistence() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("persistent.db");

    // Create and insert
    {
        let db = CostDatabase::open(&db_path).unwrap();
        let calls = vec![
            ApiCall::new(Utc::now(), "worker-1", "claude-opus", 100, 50, 10.0),
            ApiCall::new(Utc::now(), "worker-2", "claude-sonnet", 200, 100, 2.0),
        ];
        db.insert_api_calls(&calls).unwrap();
    }

    // Reopen and verify data persists
    {
        let db = CostDatabase::open(&db_path).unwrap();
        let query = CostQuery::new(&db);
        let today = query.get_today_costs().unwrap();

        assert_eq!(today.call_count, 2);
        assert!((today.total_cost_usd - 12.0).abs() < 0.0001);
    }
}

/// Test duplicate API call handling (idempotency).
#[test]
fn test_insert_duplicate_handling() {
    let db = CostDatabase::open_in_memory().unwrap();

    let timestamp = Utc::now();
    let calls = vec![
        ApiCall::new(timestamp, "worker-1", "claude-opus", 100, 50, 1.0),
        ApiCall::new(timestamp, "worker-1", "claude-opus", 100, 50, 1.0),
    ];

    // Both should be inserted (no unique constraint on timestamp+worker)
    let inserted = db.insert_api_calls(&calls).unwrap();
    assert_eq!(inserted, 2);
}

/// Test large batch insert performance.
#[test]
fn test_large_batch_insert() {
    use std::time::Instant;

    let db = CostDatabase::open_in_memory().unwrap();

    // Create 5000 API calls
    let calls: Vec<ApiCall> = (0..5000)
        .map(|i| {
            ApiCall::new(
                Utc::now(),
                &format!("worker-{}", i % 50),
                if i % 4 == 0 {
                    "claude-opus"
                } else if i % 4 == 1 {
                    "claude-sonnet"
                } else if i % 4 == 2 {
                    "claude-haiku"
                } else {
                    "glm-4.7"
                },
                (100 + i * 5) as i64,
                (50 + i * 2) as i64,
                0.01 + (i as f64 * 0.001),
            )
        })
        .collect();

    let start = Instant::now();
    let inserted = db.insert_api_calls(&calls).unwrap();
    let duration = start.elapsed();

    assert_eq!(inserted, 5000);
    // Should complete in under 10 seconds (usually much faster)
    assert!(
        duration.as_secs() < 10,
        "Large batch insert took too long: {:?}",
        duration
    );
}

/// Test queries with date range filtering.
#[test]
fn test_date_range_queries() {
    use chrono::Duration;

    let db = CostDatabase::open_in_memory().unwrap();

    let now = Utc::now();
    let yesterday = now - Duration::days(1);
    let last_week = now - Duration::days(7);

    let calls = vec![
        ApiCall::new(now, "worker-1", "claude-opus", 100, 50, 10.0),
        ApiCall::new(yesterday, "worker-1", "claude-opus", 100, 50, 20.0),
        ApiCall::new(last_week, "worker-1", "claude-opus", 100, 50, 30.0),
    ];
    db.insert_api_calls(&calls).unwrap();

    let query = CostQuery::new(&db);

    // Today only
    let today = query.get_today_costs().unwrap();
    assert_eq!(today.call_count, 1);
    assert!((today.total_cost_usd - 10.0).abs() < 0.0001);

    // Specific date
    let yesterday_costs = query.get_costs_for_date(yesterday.date_naive()).unwrap();
    assert_eq!(yesterday_costs.call_count, 1);
    assert!((yesterday_costs.total_cost_usd - 20.0).abs() < 0.0001);
}

/// Test monthly cost aggregation.
#[test]
fn test_monthly_costs() {
    use chrono::Duration;

    let db = CostDatabase::open_in_memory().unwrap();

    let now = Utc::now();
    let calls = vec![
        ApiCall::new(now, "worker-1", "claude-opus", 100, 50, 100.0),
        ApiCall::new(now - Duration::days(1), "worker-1", "claude-sonnet", 200, 100, 50.0),
        ApiCall::new(now - Duration::days(2), "worker-2", "claude-haiku", 500, 250, 10.0),
    ];
    db.insert_api_calls(&calls).unwrap();

    let query = CostQuery::new(&db);
    let monthly = query.get_current_month_costs().unwrap();

    assert_eq!(monthly.call_count, 3);
    assert!((monthly.total_cost_usd - 160.0).abs() < 0.0001);
}

/// Test worker ranking by cost.
#[test]
fn test_worker_ranking() {
    let db = CostDatabase::open_in_memory().unwrap();

    let calls = vec![
        // Worker A: $100 total
        ApiCall::new(Utc::now(), "worker-A", "claude-opus", 100, 50, 50.0),
        ApiCall::new(Utc::now(), "worker-A", "claude-opus", 100, 50, 50.0),
        // Worker B: $10 total
        ApiCall::new(Utc::now(), "worker-B", "claude-haiku", 100, 50, 10.0),
        // Worker C: $25 total
        ApiCall::new(Utc::now(), "worker-C", "claude-sonnet", 100, 50, 25.0),
    ];
    db.insert_api_calls(&calls).unwrap();

    let query = CostQuery::new(&db);
    let workers = query.get_top_workers(10).unwrap();

    assert_eq!(workers.len(), 3);
    assert_eq!(workers[0].0, "worker-A");
    assert!((workers[0].1 - 100.0).abs() < 0.0001);
    assert_eq!(workers[1].0, "worker-C");
    assert_eq!(workers[2].0, "worker-B");
}

/// Test multi-bead cost tracking.
#[test]
fn test_multi_bead_tracking() {
    let db = CostDatabase::open_in_memory().unwrap();

    let calls = vec![
        // Bead 1: complex task, multiple API calls
        ApiCall::new(Utc::now(), "worker-1", "claude-opus", 1000, 500, 5.0).with_bead("fg-001"),
        ApiCall::new(Utc::now(), "worker-1", "claude-opus", 2000, 1000, 10.0).with_bead("fg-001"),
        ApiCall::new(Utc::now(), "worker-1", "claude-opus", 500, 250, 2.5).with_bead("fg-001"),
        // Bead 2: simple task
        ApiCall::new(Utc::now(), "worker-2", "claude-haiku", 100, 50, 0.1).with_bead("fg-002"),
        // Bead 3: medium task
        ApiCall::new(Utc::now(), "worker-1", "claude-sonnet", 500, 250, 1.0).with_bead("fg-003"),
    ];
    db.insert_api_calls(&calls).unwrap();

    let query = CostQuery::new(&db);

    // Bead 1: should be most expensive
    let bead1 = query.get_cost_per_task("fg-001").unwrap();
    assert_eq!(bead1.call_count, 3);
    assert!((bead1.total_cost_usd - 17.5).abs() < 0.0001);
    assert_eq!(bead1.model, "claude-opus");

    // Bead 2: cheapest
    let bead2 = query.get_cost_per_task("fg-002").unwrap();
    assert_eq!(bead2.call_count, 1);
    assert!((bead2.total_cost_usd - 0.1).abs() < 0.0001);

    // Non-existent bead
    let non_existent = query.get_cost_per_task("fg-999").unwrap();
    assert_eq!(non_existent.call_count, 0);
    assert_eq!(non_existent.total_cost_usd, 0.0);
}

/// Test cache token tracking and reporting.
#[test]
fn test_cache_efficiency_tracking() {
    let db = CostDatabase::open_in_memory().unwrap();

    let calls = vec![
        // High cache hit ratio
        ApiCall::new(Utc::now(), "worker-1", "claude-opus", 100, 50, 0.5)
            .with_cache(500, 10000), // 500 created, 10000 read
        // Low cache hit ratio
        ApiCall::new(Utc::now(), "worker-2", "claude-opus", 1000, 500, 5.0)
            .with_cache(5000, 0), // 5000 created, 0 read
    ];
    db.insert_api_calls(&calls).unwrap();

    let query = CostQuery::new(&db);
    let today = query.get_today_costs().unwrap();

    // Verify total tokens include cache tokens
    assert_eq!(today.call_count, 2);

    // Check model breakdown includes cache metrics
    assert!(!today.by_model.is_empty());
    let model = &today.by_model[0];
    assert!(model.cache_creation_tokens > 0);
}

/// Test projected costs calculation.
#[test]
fn test_projected_costs_scenarios() {
    let db = CostDatabase::open_in_memory().unwrap();

    // Insert some costs for today
    let calls = vec![ApiCall::new(
        Utc::now(),
        "worker-1",
        "claude-opus",
        100,
        50,
        50.0,
    )];
    db.insert_api_calls(&calls).unwrap();

    let query = CostQuery::new(&db);

    // With 10 days remaining
    let projected = query.get_projected_costs(Some(10)).unwrap();
    assert_eq!(projected.current_total, 50.0);
    assert_eq!(projected.days_remaining, 10);
    assert!(projected.projected_total >= projected.current_total);

    // With 1 day remaining
    let projected_end = query.get_projected_costs(Some(1)).unwrap();
    assert!(projected_end.projected_total <= projected.projected_total);
}

/// Test model cost comparison.
#[test]
fn test_model_cost_comparison() {
    let db = CostDatabase::open_in_memory().unwrap();

    let calls = vec![
        // Expensive model
        ApiCall::new(Utc::now(), "worker-1", "claude-opus-4.5", 1000, 500, 50.0),
        ApiCall::new(Utc::now(), "worker-1", "claude-opus-4.5", 1000, 500, 50.0),
        // Mid-tier model
        ApiCall::new(Utc::now(), "worker-2", "claude-sonnet-4.5", 2000, 1000, 10.0),
        ApiCall::new(Utc::now(), "worker-2", "claude-sonnet-4.5", 2000, 1000, 10.0),
        // Cheap model
        ApiCall::new(Utc::now(), "worker-3", "claude-haiku-3", 5000, 2500, 1.0),
    ];
    db.insert_api_calls(&calls).unwrap();

    let query = CostQuery::new(&db);
    let model_costs = query.get_model_costs(None, None).unwrap();

    // Should be sorted by cost descending
    assert_eq!(model_costs.len(), 3);
    assert_eq!(model_costs[0].model, "claude-opus-4.5");
    assert!((model_costs[0].total_cost_usd - 100.0).abs() < 0.0001);
    assert_eq!(model_costs[1].model, "claude-sonnet-4.5");
    assert!((model_costs[1].total_cost_usd - 20.0).abs() < 0.0001);
    assert_eq!(model_costs[2].model, "claude-haiku-3");
}

/// Test mixed log format parsing.
#[test]
fn test_mixed_log_format_parsing() {
    let dir = tempdir().unwrap();

    // Create log with mixed valid and invalid entries
    let mixed_content = r#"[2026-02-08 10:00:00] Starting worker session
{"type":"system","subtype":"init","cwd":"/project","session_id":"session-1"}
{"type":"assistant","message":{"model":"claude-opus-4-5-20251101","id":"msg_01","type":"message","role":"assistant","content":[],"usage":{"input_tokens":500,"output_tokens":250}},"session_id":"session-1"}
RANDOM_TEXT_NOT_JSON
{"type":"result","subtype":"success","total_cost_usd":0.75,"usage":{"input_tokens":500,"output_tokens":250},"session_id":"session-1"}
[2026-02-08 10:05:00] Worker completed task
Another non-JSON line
{"type":"assistant","message":{"model":"claude-sonnet-4-5-20251101","id":"msg_02","type":"message","role":"assistant","content":[],"usage":{"input_tokens":1000,"output_tokens":500}},"session_id":"session-1"}
"#;

    let log_path = dir.path().join("mixed-worker.log");
    std::fs::write(&log_path, mixed_content).unwrap();

    let parser = LogParser::new();
    let calls = parser.parse_file(&log_path).unwrap();

    // Should parse the 3 valid JSON events
    assert_eq!(calls.len(), 3);
}

/// Test log parsing with different model formats.
#[test]
fn test_log_parsing_model_formats() {
    let dir = tempdir().unwrap();

    // Anthropic format
    let anthropic_log = r#"{"type":"assistant","message":{"model":"claude-opus-4-5-20251101","id":"msg_01","type":"message","role":"assistant","content":[],"usage":{"input_tokens":100,"output_tokens":50}},"session_id":"s1"}
{"type":"result","subtype":"success","total_cost_usd":0.10,"usage":{"input_tokens":100,"output_tokens":50},"session_id":"s1"}"#;

    let anthropic_path = dir.path().join("anthropic.log");
    std::fs::write(&anthropic_path, anthropic_log).unwrap();

    // GLM format with modelUsage
    let glm_log = r#"{"type":"assistant","message":{"model":"glm-4.7","id":"msg_01","type":"message","role":"assistant","content":[],"usage":{"input_tokens":1000,"output_tokens":500}},"session_id":"s2"}
{"type":"result","subtype":"success","total_cost_usd":0.0,"usage":{"input_tokens":1000,"output_tokens":500},"modelUsage":{"glm-4.7":{"inputTokens":1000,"outputTokens":500}},"session_id":"s2"}"#;

    let glm_path = dir.path().join("glm.log");
    std::fs::write(&glm_path, glm_log).unwrap();

    let parser = LogParser::new();

    let anthropic_calls = parser.parse_file(&anthropic_path).unwrap();
    assert_eq!(anthropic_calls.len(), 2);
    assert!(anthropic_calls[0].model.contains("claude"));

    let glm_calls = parser.parse_file(&glm_path).unwrap();
    assert_eq!(glm_calls.len(), 2);
    assert!(glm_calls.iter().any(|c| c.model.contains("glm")));
}

/// Test concurrent database access.
#[test]
fn test_concurrent_database_operations() {
    use std::sync::Arc;
    use std::thread;

    let db = Arc::new(CostDatabase::open_in_memory().unwrap());

    let handles: Vec<_> = (0..4)
        .map(|thread_id| {
            let db = Arc::clone(&db);
            thread::spawn(move || {
                // Each thread inserts 100 calls
                let calls: Vec<ApiCall> = (0..100)
                    .map(|_i| {
                        ApiCall::new(
                            Utc::now(),
                            &format!("worker-thread-{}", thread_id),
                            "claude-sonnet",
                            100,
                            50,
                            0.01,
                        )
                    })
                    .collect();
                db.insert_api_calls(&calls).unwrap()
            })
        })
        .collect();

    let total_inserted: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();

    // All 400 calls should be inserted
    assert_eq!(total_inserted, 400);

    // Verify with query
    let query = CostQuery::new(&db);
    let today = query.get_today_costs().unwrap();
    assert_eq!(today.call_count, 400);
}

/// Test subscription creation and querying.
#[test]
fn test_subscription_lifecycle() {
    use chrono::Duration;
    use forge_cost::{Subscription, SubscriptionType};

    let db = CostDatabase::open_in_memory().unwrap();

    let now = Utc::now();
    let start = now - Duration::days(5);
    let end = now + Duration::days(25);

    // Create subscription
    let sub = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
        .with_quota(500)
        .with_model("claude-sonnet-4.5");

    db.upsert_subscription(&sub).unwrap();

    // Query subscription
    let query = CostQuery::new(&db);
    let summaries = query.get_subscription_summaries().unwrap();

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].name, "Claude Pro");
    assert_eq!(summaries[0].quota_limit, Some(500));
    assert_eq!(summaries[0].monthly_cost, 20.0);
}

/// Test subscription usage tracking.
#[test]
fn test_subscription_usage_tracking() {
    use chrono::Duration;
    use forge_cost::{Subscription, SubscriptionType};

    let db = CostDatabase::open_in_memory().unwrap();

    let now = Utc::now();
    let start = now - Duration::days(10);
    let end = now + Duration::days(20);

    // Create subscription with 50% usage
    let mut sub =
        Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
            .with_quota(500);
    sub.quota_used = 250;

    db.upsert_subscription(&sub).unwrap();

    let query = CostQuery::new(&db);
    let summary = query.get_subscription_summary("Claude Pro").unwrap().unwrap();

    assert_eq!(summary.quota_used, 250);
    assert!((summary.usage_percentage - 50.0).abs() < 0.1);
}

/// Test subscription optimization recommendations.
#[test]
fn test_subscription_recommendations() {
    use chrono::Duration;
    use forge_cost::{Subscription, SubscriptionType};

    let db = CostDatabase::open_in_memory().unwrap();

    let now = Utc::now();

    // Over-utilized subscription (90% used, only 33% through period)
    let start1 = now - Duration::days(10);
    let end1 = now + Duration::days(20);
    let mut sub1 =
        Subscription::new("High Usage", SubscriptionType::FixedQuota, 20.0, start1, end1)
            .with_quota(500);
    sub1.quota_used = 450;

    // Under-utilized subscription (10% used, 80% through period)
    let start2 = now - Duration::days(24);
    let end2 = now + Duration::days(6);
    let mut sub2 =
        Subscription::new("Low Usage", SubscriptionType::FixedQuota, 20.0, start2, end2)
            .with_quota(500);
    sub2.quota_used = 50;

    db.upsert_subscription(&sub1).unwrap();
    db.upsert_subscription(&sub2).unwrap();

    let query = CostQuery::new(&db);
    let report = query.get_subscription_optimization_report().unwrap();

    assert_eq!(report.subscription_count, 2);
    assert!(!report.recommendations.is_empty());
    assert_eq!(report.total_quota_used, 500); // 450 + 50
}

/// Test aggregator with sample data.
#[tokio::test]
async fn test_aggregator_with_data() {
    use forge_cost::Aggregator;
    use std::sync::Arc;

    let db = Arc::new(CostDatabase::open_in_memory().unwrap());

    // Insert sample data
    let calls = vec![
        ApiCall::new(Utc::now(), "worker-1", "claude-opus", 1000, 500, 10.0),
        ApiCall::new(Utc::now(), "worker-2", "claude-sonnet", 2000, 1000, 5.0),
    ];
    db.insert_api_calls(&calls).unwrap();

    // Run aggregation
    let aggregator = Aggregator::new(Arc::clone(&db));
    aggregator.run_once().unwrap();

    // Verify queries still work after aggregation
    let query = CostQuery::new(&db);
    let today = query.get_today_costs().unwrap();
    assert_eq!(today.call_count, 2);
}

/// Test empty log file handling.
#[test]
fn test_empty_log_file() {
    let file = create_mock_log_file("");
    let parser = LogParser::new();

    let calls = parser.parse_file(file.path()).unwrap();
    assert!(calls.is_empty());
}

/// Test whitespace-only log file.
#[test]
fn test_whitespace_log_file() {
    let file = create_mock_log_file("   \n\n   \t\n   ");
    let parser = LogParser::new();

    let calls = parser.parse_file(file.path()).unwrap();
    assert!(calls.is_empty());
}

/// Test log file with only non-JSON lines.
#[test]
fn test_non_json_only_log_file() {
    let content = r#"[2026-02-08 10:00:00] Starting worker
[2026-02-08 10:01:00] Processing task
[2026-02-08 10:02:00] Task completed
"#;
    let file = create_mock_log_file(content);
    let parser = LogParser::new();

    let calls = parser.parse_file(file.path()).unwrap();
    assert!(calls.is_empty());
}

/// Test directory with nested log files.
#[test]
fn test_nested_directory_parsing() {
    let dir = tempdir().unwrap();

    // Create nested structure
    let subdir = dir.path().join("workers");
    std::fs::create_dir(&subdir).unwrap();

    // Log in root
    std::fs::write(dir.path().join("worker-1.log"), CLAUDE_LOG_CONTENT).unwrap();

    // Log in subdirectory
    std::fs::write(subdir.join("worker-2.log"), GLM_LOG_CONTENT).unwrap();

    let parser = LogParser::new();
    let calls = parser.parse_directory(dir.path()).unwrap();

    // Should find logs in both locations
    assert!(calls.len() >= 2);
}

/// Test API call with all optional fields.
#[test]
fn test_api_call_full_fields() {
    let db = CostDatabase::open_in_memory().unwrap();

    let call = ApiCall::new(Utc::now(), "worker-1", "claude-opus", 1000, 500, 15.0)
        .with_bead("fg-abc")
        .with_cache(2000, 5000)
        .with_session("session-123");

    db.insert_api_calls(&[call]).unwrap();

    let query = CostQuery::new(&db);
    let bead_cost = query.get_cost_per_task("fg-abc").unwrap();

    assert_eq!(bead_cost.call_count, 1);
    assert_eq!(bead_cost.input_tokens, 1000);
    assert_eq!(bead_cost.cache_creation_tokens, 2000);
    assert_eq!(bead_cost.cache_read_tokens, 5000);
}
