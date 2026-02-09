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
