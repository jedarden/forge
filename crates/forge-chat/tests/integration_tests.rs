//! End-to-end integration tests for the forge-chat crate.
//!
//! These tests cover:
//! - Chat tool registry and execution
//! - Rate limiting integration
//! - Audit logging integration
//! - Context provider integration
//! - Full chat workflow (without API calls)

use chrono::{Duration, Utc};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;

use forge_chat::config::{AuditConfig, AuditLogLevel, RateLimitConfig};
use forge_chat::context::{
    ContextSource, CostAnalytics, EventInfo, ModelCost, PriorityCost, SubscriptionInfo, TaskInfo,
    WorkerInfo,
};
use forge_chat::tools::{
    ActionConfirmation, ConfirmationLevel, SideEffect, ToolCall, ToolRegistry, ToolResult,
};
use forge_chat::{
    AuditEntry, AuditLogger, ChatResponse, ContextProvider, DashboardContext, RateLimiter,
};

// ============================================================
// Chat Tool Integration Tests
// ============================================================

#[tokio::test]
async fn test_tool_registry_has_all_builtin_tools() {
    let registry = ToolRegistry::with_builtin_tools();
    let tool_names = registry.tool_names();

    // Verify all expected tools are registered
    let expected_tools = [
        "get_worker_status",
        "get_task_queue",
        "get_cost_analytics",
        "get_subscription_usage",
        "get_activity_log",
        "spawn_worker",
        "kill_worker",
        "assign_task",
        "pause_workers",
        "resume_workers",
    ];

    for tool in &expected_tools {
        assert!(
            tool_names.contains(tool),
            "Expected tool '{}' to be registered",
            tool
        );
    }

    assert_eq!(tool_names.len(), expected_tools.len());
}

#[tokio::test]
async fn test_worker_status_tool_execution() {
    let registry = ToolRegistry::with_builtin_tools();
    let context = create_rich_context();

    let call = ToolCall {
        name: "get_worker_status".to_string(),
        parameters: serde_json::json!({}),
        id: Some("test-1".to_string()),
    };

    let result = registry.execute(&call, &context).await.unwrap();
    assert!(result.success);

    let data = &result.data;
    assert_eq!(data["total"].as_u64(), Some(3));
    assert_eq!(data["healthy"].as_u64(), Some(2));
    assert_eq!(data["idle"].as_u64(), Some(1));
}

#[tokio::test]
async fn test_worker_status_tool_with_filter() {
    let registry = ToolRegistry::with_builtin_tools();
    let context = create_rich_context();

    // Filter by healthy workers
    let call = ToolCall {
        name: "get_worker_status".to_string(),
        parameters: serde_json::json!({"status_filter": "healthy"}),
        id: None,
    };

    let result = registry.execute(&call, &context).await.unwrap();
    assert!(result.success);

    let workers = result.data["workers"].as_array().unwrap();
    assert_eq!(workers.len(), 2);

    // Filter by idle workers
    let call = ToolCall {
        name: "get_worker_status".to_string(),
        parameters: serde_json::json!({"status_filter": "idle"}),
        id: None,
    };

    let result = registry.execute(&call, &context).await.unwrap();
    assert!(result.success);

    let workers = result.data["workers"].as_array().unwrap();
    assert_eq!(workers.len(), 1);
}

#[tokio::test]
async fn test_task_queue_tool_execution() {
    let registry = ToolRegistry::with_builtin_tools();
    let context = create_rich_context();

    let call = ToolCall {
        name: "get_task_queue".to_string(),
        parameters: serde_json::json!({}),
        id: None,
    };

    let result = registry.execute(&call, &context).await.unwrap();
    assert!(result.success);
    assert!(result.data["total_ready"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn test_task_queue_tool_with_priority_filter() {
    let registry = ToolRegistry::with_builtin_tools();
    let context = create_rich_context();

    let call = ToolCall {
        name: "get_task_queue".to_string(),
        parameters: serde_json::json!({"priority": "P0"}),
        id: None,
    };

    let result = registry.execute(&call, &context).await.unwrap();
    assert!(result.success);

    // All returned tasks should be P0
    if let Some(beads) = result.data["beads"].as_array() {
        for bead in beads {
            assert_eq!(bead["priority"].as_str(), Some("P0"));
        }
    }
}

#[tokio::test]
async fn test_cost_analytics_tool_execution() {
    let registry = ToolRegistry::with_builtin_tools();
    let context = create_rich_context();

    let call = ToolCall {
        name: "get_cost_analytics".to_string(),
        parameters: serde_json::json!({"timeframe": "today"}),
        id: None,
    };

    let result = registry.execute(&call, &context).await.unwrap();
    assert!(result.success);
    assert!(result.message.contains("today"));
}

#[tokio::test]
async fn test_subscription_usage_tool_execution() {
    let registry = ToolRegistry::with_builtin_tools();
    let context = create_rich_context();

    let call = ToolCall {
        name: "get_subscription_usage".to_string(),
        parameters: serde_json::json!({}),
        id: None,
    };

    let result = registry.execute(&call, &context).await.unwrap();
    assert!(result.success);
    assert!(result.message.contains("subscriptions"));
}

#[tokio::test]
async fn test_activity_log_tool_execution() {
    let registry = ToolRegistry::with_builtin_tools();
    let context = create_rich_context();

    let call = ToolCall {
        name: "get_activity_log".to_string(),
        parameters: serde_json::json!({"hours": 1}),
        id: None,
    };

    let result = registry.execute(&call, &context).await.unwrap();
    assert!(result.success);
    assert!(result.data["events"].as_array().is_some());
}

#[tokio::test]
async fn test_spawn_worker_tool_small_count_no_confirmation() {
    let registry = ToolRegistry::with_builtin_tools();
    let context = create_rich_context();

    // Spawning 1 worker should not require confirmation
    let call = ToolCall {
        name: "spawn_worker".to_string(),
        parameters: serde_json::json!({"worker_type": "sonnet", "count": 1}),
        id: None,
    };

    // Should succeed without confirmation error
    let result = registry.execute(&call, &context).await.unwrap();
    assert!(result.success);
    assert_eq!(result.data["count"].as_u64(), Some(1));
    assert!(result.side_effects.len() >= 1);
}

#[tokio::test]
async fn test_spawn_worker_tool_large_count_requires_confirmation() {
    let registry = ToolRegistry::with_builtin_tools();
    let context = create_rich_context();

    // Spawning 5 workers should require confirmation
    let call = ToolCall {
        name: "spawn_worker".to_string(),
        parameters: serde_json::json!({"worker_type": "opus", "count": 5}),
        id: None,
    };

    let result = registry.execute(&call, &context).await;

    // Should return confirmation required error
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Confirmation") || err.contains("confirmation"));
}

#[tokio::test]
async fn test_kill_worker_tool_requires_confirmation() {
    let registry = ToolRegistry::with_builtin_tools();
    let context = create_rich_context();

    let call = ToolCall {
        name: "kill_worker".to_string(),
        parameters: serde_json::json!({"session_name": "glm-alpha"}),
        id: None,
    };

    let result = registry.execute(&call, &context).await;

    // Should require confirmation for any worker kill
    assert!(result.is_err());
}

#[tokio::test]
async fn test_pause_workers_tool_short_duration_no_confirmation() {
    let registry = ToolRegistry::with_builtin_tools();
    let context = create_rich_context();

    // Short pause (under 10 min) should not require confirmation
    let call = ToolCall {
        name: "pause_workers".to_string(),
        parameters: serde_json::json!({"duration_minutes": 5}),
        id: None,
    };

    let result = registry.execute(&call, &context).await.unwrap();
    assert!(result.success);
    assert_eq!(result.data["duration_minutes"].as_u64(), Some(5));
}

#[tokio::test]
async fn test_resume_workers_tool() {
    let registry = ToolRegistry::with_builtin_tools();
    let context = create_rich_context();

    let call = ToolCall {
        name: "resume_workers".to_string(),
        parameters: serde_json::json!({}),
        id: None,
    };

    let result = registry.execute(&call, &context).await.unwrap();
    assert!(result.success);
    assert!(result.data["resumed"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn test_unknown_tool_returns_error() {
    let registry = ToolRegistry::with_builtin_tools();
    let context = create_rich_context();

    let call = ToolCall {
        name: "nonexistent_tool".to_string(),
        parameters: serde_json::json!({}),
        id: None,
    };

    let result = registry.execute(&call, &context).await;
    assert!(result.is_err());
}

// ============================================================
// Rate Limiting Integration Tests
// ============================================================

#[tokio::test]
async fn test_rate_limiter_allows_commands_under_limit() {
    let config = RateLimitConfig {
        max_per_minute: 10,
        max_per_hour: 100,
    };
    let limiter = RateLimiter::new(config);

    // Should allow all commands under limit
    for _ in 0..10 {
        assert!(limiter.check().await.is_ok());
        limiter.record().await;
    }
}

#[tokio::test]
async fn test_rate_limiter_blocks_commands_over_minute_limit() {
    let config = RateLimitConfig {
        max_per_minute: 5,
        max_per_hour: 100,
    };
    let limiter = RateLimiter::new(config);

    // Fill up the minute limit
    for _ in 0..5 {
        limiter.record().await;
    }

    // Next command should be blocked
    let result = limiter.check().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_rate_limiter_usage_tracking() {
    let config = RateLimitConfig {
        max_per_minute: 10,
        max_per_hour: 100,
    };
    let limiter = RateLimiter::new(config);

    limiter.record().await;
    limiter.record().await;
    limiter.record().await;

    let usage = limiter.usage().await;
    assert_eq!(usage.commands_last_minute, 3);
    assert_eq!(usage.remaining_minute(), 7);
    assert!(!usage.near_minute_limit());
}

#[tokio::test]
async fn test_rate_limiter_near_limit_detection() {
    let config = RateLimitConfig {
        max_per_minute: 10,
        max_per_hour: 100,
    };
    let limiter = RateLimiter::new(config);

    // Use up 85% of the limit
    for _ in 0..9 {
        limiter.record().await;
    }

    let usage = limiter.usage().await;
    assert!(usage.near_minute_limit());
}

#[tokio::test]
async fn test_rate_limiter_reset_clears_counters() {
    let config = RateLimitConfig {
        max_per_minute: 5,
        max_per_hour: 100,
    };
    let limiter = RateLimiter::new(config);

    // Fill up the limit
    for _ in 0..5 {
        limiter.record().await;
    }

    // Reset
    limiter.reset().await;

    // Should be able to use again
    let usage = limiter.usage().await;
    assert_eq!(usage.commands_last_minute, 0);
    assert!(limiter.check().await.is_ok());
}

// ============================================================
// Audit Logging Integration Tests
// ============================================================

#[tokio::test]
async fn test_audit_logger_writes_entries() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("audit.jsonl");

    let config = AuditConfig {
        enabled: true,
        log_file: log_path.clone(),
        log_level: AuditLogLevel::All,
    };

    let logger = AuditLogger::new(config).await.unwrap();

    let entry = AuditEntry::new("show workers")
        .with_response("2 workers active")
        .with_duration(150);

    logger.log(&entry).await.unwrap();

    // Verify the log file contains the entry
    let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
    assert!(contents.contains("show workers"));
    assert!(contents.contains("2 workers active"));
}

#[tokio::test]
async fn test_audit_logger_commands_only_mode() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("audit.jsonl");

    let config = AuditConfig {
        enabled: true,
        log_file: log_path.clone(),
        log_level: AuditLogLevel::CommandsOnly,
    };

    let logger = AuditLogger::new(config).await.unwrap();

    let entry = AuditEntry::new("spawn worker").with_response("Spawned sonnet-1");

    logger.log(&entry).await.unwrap();

    // Response should be stripped
    let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
    assert!(contents.contains("spawn worker"));

    let parsed: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
    assert!(parsed["response"].is_null());
}

#[tokio::test]
async fn test_audit_logger_errors_only_mode() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("audit.jsonl");

    let config = AuditConfig {
        enabled: true,
        log_file: log_path.clone(),
        log_level: AuditLogLevel::ErrorsOnly,
    };

    let logger = AuditLogger::new(config).await.unwrap();

    // Success entry should not be logged
    let success_entry = AuditEntry::new("show status");
    logger.log(&success_entry).await.unwrap();

    // Error entry should be logged
    let error_entry = AuditEntry::new("invalid command").with_error("Unknown command");
    logger.log(&error_entry).await.unwrap();

    let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
    assert!(!contents.contains("show status"));
    assert!(contents.contains("invalid command"));
    assert!(contents.contains("Unknown command"));
}

#[tokio::test]
async fn test_audit_logger_disabled_does_nothing() {
    let logger = AuditLogger::disabled();
    assert!(!logger.is_enabled());

    let entry = AuditEntry::new("test command");
    // Should not error
    logger.log(&entry).await.unwrap();
}

#[tokio::test]
async fn test_audit_logger_logs_tool_calls() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("audit.jsonl");

    let config = AuditConfig {
        enabled: true,
        log_file: log_path.clone(),
        log_level: AuditLogLevel::All,
    };

    let logger = AuditLogger::new(config).await.unwrap();

    let tool_call = ToolCall {
        name: "spawn_worker".to_string(),
        parameters: serde_json::json!({"worker_type": "sonnet", "count": 2}),
        id: Some("call-123".to_string()),
    };

    let entry = AuditEntry::new("spawn 2 sonnet workers")
        .with_tool_call(tool_call)
        .with_duration(200);

    logger.log(&entry).await.unwrap();

    let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
    assert!(contents.contains("spawn_worker"));
    assert!(contents.contains("sonnet"));
}

#[tokio::test]
async fn test_audit_logger_logs_side_effects() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("audit.jsonl");

    let config = AuditConfig {
        enabled: true,
        log_file: log_path.clone(),
        log_level: AuditLogLevel::All,
    };

    let logger = AuditLogger::new(config).await.unwrap();

    let side_effect = SideEffect {
        effect_type: "spawn".to_string(),
        description: "Spawned worker sonnet-1".to_string(),
        data: Some(serde_json::json!({"session": "sonnet-1"})),
    };

    let entry = AuditEntry::new("spawn sonnet worker").with_side_effects(vec![side_effect]);

    logger.log(&entry).await.unwrap();

    let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
    assert!(contents.contains("sonnet-1"));
    assert!(contents.contains("spawn"));
}

// ============================================================
// Context Provider Integration Tests
// ============================================================

#[tokio::test]
async fn test_context_provider_caches_context() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let source = CountingContextSource {
        context: create_rich_context(),
        call_count: call_count.clone(),
    };

    let provider = ContextProvider::new(source).with_cache_duration(60);

    // First call should hit the source
    let _ = provider.get_context().await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Second call should use cache
    let _ = provider.get_context().await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_context_provider_refresh_bypasses_cache() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let source = CountingContextSource {
        context: create_rich_context(),
        call_count: call_count.clone(),
    };

    let provider = ContextProvider::new(source).with_cache_duration(60);

    // First call
    let _ = provider.get_context().await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Refresh should bypass cache
    let _ = provider.refresh().await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_context_provider_invalidate_clears_cache() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let source = CountingContextSource {
        context: create_rich_context(),
        call_count: call_count.clone(),
    };

    let provider = ContextProvider::new(source).with_cache_duration(60);

    // First call
    let _ = provider.get_context().await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Invalidate cache
    provider.invalidate().await;

    // Next call should hit source again
    let _ = provider.get_context().await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_context_summary_generation() {
    let context = create_rich_context();
    let summary = context.to_summary();

    // Should contain worker count
    assert!(summary.contains("Workers:"));
    assert!(summary.contains("healthy"));

    // Should contain task count
    assert!(summary.contains("Tasks:"));

    // Should contain costs
    assert!(summary.contains("Costs today:"));

    // Should contain subscription info
    assert!(summary.contains("Claude Pro"));
}

// ============================================================
// Chat Response Integration Tests
// ============================================================

#[tokio::test]
async fn test_chat_response_builder_success() {
    let response = ChatResponse::success("Worker pool is healthy")
        .with_duration(150)
        .with_cost(0.02);

    assert!(response.success);
    assert_eq!(response.text, "Worker pool is healthy");
    assert_eq!(response.duration_ms, 150);
    assert_eq!(response.cost_usd, Some(0.02));
    assert!(response.error.is_none());
}

#[tokio::test]
async fn test_chat_response_builder_error() {
    let response = ChatResponse::error("Rate limit exceeded").with_duration(10);

    assert!(!response.success);
    assert!(response.text.contains("Error:"));
    assert!(response.error.is_some());
}

#[tokio::test]
async fn test_chat_response_with_tools() {
    let tool_call = ToolCall {
        name: "get_worker_status".to_string(),
        parameters: serde_json::json!({}),
        id: Some("test-call".to_string()),
    };

    let tool_result = ToolResult::success(
        serde_json::json!({"total": 5, "healthy": 4}),
        "Found 5 workers",
    );

    let response = ChatResponse::success("5 workers found, 4 are healthy")
        .with_tools(vec![tool_call], vec![tool_result]);

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_results.len(), 1);
    assert_eq!(response.tool_calls[0].name, "get_worker_status");
}

#[tokio::test]
async fn test_chat_response_with_confirmation() {
    let confirmation = ActionConfirmation {
        title: "Kill worker?".to_string(),
        description: "Worker glm-alpha will be terminated".to_string(),
        level: ConfirmationLevel::Warning,
        cost_impact: None,
        affected_items: vec!["glm-alpha".to_string()],
        reversible: true,
    };

    let response = ChatResponse::success("Confirmation required").with_confirmation(confirmation);

    assert!(response.confirmation_required.is_some());
    let conf = response.confirmation_required.unwrap();
    assert_eq!(conf.title, "Kill worker?");
    assert_eq!(conf.level, ConfirmationLevel::Warning);
}

// ============================================================
// Full Workflow Integration Tests
// ============================================================

#[tokio::test]
async fn test_full_workflow_read_only_query() {
    // This test simulates a complete read-only workflow:
    // 1. Get context
    // 2. Execute read-only tool
    // 3. Log audit entry
    // 4. Record rate limit

    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("audit.jsonl");

    // Setup components
    let context = create_rich_context();
    let registry = ToolRegistry::with_builtin_tools();
    let rate_limiter = RateLimiter::new(RateLimitConfig {
        max_per_minute: 10,
        max_per_hour: 100,
    });
    let logger = AuditLogger::new(AuditConfig {
        enabled: true,
        log_file: log_path.clone(),
        log_level: AuditLogLevel::All,
    })
    .await
    .unwrap();

    // Simulate workflow
    assert!(rate_limiter.check().await.is_ok());

    let call = ToolCall {
        name: "get_worker_status".to_string(),
        parameters: serde_json::json!({}),
        id: None,
    };

    let result = registry.execute(&call, &context).await.unwrap();
    assert!(result.success);

    let entry = AuditEntry::new("show workers")
        .with_response(&result.message)
        .with_duration(100);

    logger.log(&entry).await.unwrap();
    rate_limiter.record().await;

    // Verify rate limit was recorded
    let usage = rate_limiter.usage().await;
    assert_eq!(usage.commands_last_minute, 1);

    // Verify audit was logged
    let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
    assert!(contents.contains("show workers"));
}

#[tokio::test]
async fn test_full_workflow_action_with_confirmation() {
    // This test simulates a workflow requiring confirmation:
    // 1. Check rate limit
    // 2. Execute action tool (triggers confirmation)
    // 3. Handle confirmation error

    let context = create_rich_context();
    let registry = ToolRegistry::with_builtin_tools();

    let call = ToolCall {
        name: "kill_worker".to_string(),
        parameters: serde_json::json!({"session_name": "glm-alpha"}),
        id: None,
    };

    let result = registry.execute(&call, &context).await;

    // Should return confirmation required error
    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_str = error.to_string();
    assert!(
        error_str.contains("Confirmation") || error_str.contains("confirmation"),
        "Expected confirmation error, got: {}",
        error_str
    );
}

#[tokio::test]
async fn test_full_workflow_rate_limit_exceeded() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("audit.jsonl");

    // Setup with very low rate limit
    let rate_limiter = RateLimiter::new(RateLimitConfig {
        max_per_minute: 2,
        max_per_hour: 100,
    });
    let logger = AuditLogger::new(AuditConfig {
        enabled: true,
        log_file: log_path.clone(),
        log_level: AuditLogLevel::All,
    })
    .await
    .unwrap();

    // Fill up the rate limit
    rate_limiter.record().await;
    rate_limiter.record().await;

    // Next command should be rate limited
    let result = rate_limiter.check().await;
    assert!(result.is_err());

    // Log the rate limit error
    let entry = AuditEntry::new("show costs").with_error("Rate limit exceeded");

    logger.log(&entry).await.unwrap();

    // Verify error was logged
    let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
    assert!(contents.contains("Rate limit exceeded"));
}

#[tokio::test]
async fn test_multiple_concurrent_operations() {
    // Test that multiple operations can run concurrently
    let context = create_rich_context();
    let registry = Arc::new(ToolRegistry::with_builtin_tools());

    let mut handles = vec![];

    for i in 0..5 {
        let reg = registry.clone();
        let ctx = context.clone();
        let handle = tokio::spawn(async move {
            let call = ToolCall {
                name: "get_worker_status".to_string(),
                parameters: serde_json::json!({}),
                id: Some(format!("concurrent-{}", i)),
            };
            reg.execute(&call, &ctx).await
        });
        handles.push(handle);
    }

    // All operations should succeed
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
        assert!(result.unwrap().success);
    }
}

// ============================================================
// Helper Functions and Types
// ============================================================

fn create_rich_context() -> DashboardContext {
    DashboardContext {
        workers: vec![
            WorkerInfo {
                session_name: "glm-alpha".to_string(),
                worker_type: "glm".to_string(),
                workspace: "/home/coder/ardenone-cluster".to_string(),
                is_healthy: true,
                is_idle: false,
                current_task: Some("bd-123".to_string()),
                uptime_secs: 3600,
                last_activity: Some(Utc::now()),
                beads_completed: 5,
            },
            WorkerInfo {
                session_name: "sonnet-bravo".to_string(),
                worker_type: "sonnet".to_string(),
                workspace: "/home/coder/forge".to_string(),
                is_healthy: true,
                is_idle: true,
                current_task: None,
                uptime_secs: 1800,
                last_activity: Some(Utc::now()),
                beads_completed: 3,
            },
            WorkerInfo {
                session_name: "opus-charlie".to_string(),
                worker_type: "opus".to_string(),
                workspace: "/home/coder/botburrow".to_string(),
                is_healthy: false,
                is_idle: false,
                current_task: Some("bd-456".to_string()),
                uptime_secs: 500,
                last_activity: Some(Utc::now() - Duration::minutes(10)),
                beads_completed: 0,
            },
        ],
        tasks: vec![
            TaskInfo {
                id: "bd-456".to_string(),
                title: "Implement feature X".to_string(),
                priority: "P0".to_string(),
                workspace: "/home/coder/forge".to_string(),
                in_progress: true,
                assigned_model: Some("opus".to_string()),
                estimated_tokens: Some(50000),
            },
            TaskInfo {
                id: "bd-789".to_string(),
                title: "Fix bug in login".to_string(),
                priority: "P1".to_string(),
                workspace: "/home/coder/forge".to_string(),
                in_progress: false,
                assigned_model: Some("sonnet".to_string()),
                estimated_tokens: Some(20000),
            },
            TaskInfo {
                id: "bd-012".to_string(),
                title: "Add tests".to_string(),
                priority: "P2".to_string(),
                workspace: "/home/coder/ardenone-cluster".to_string(),
                in_progress: false,
                assigned_model: None,
                estimated_tokens: Some(15000),
            },
        ],
        costs_today: CostAnalytics {
            timeframe: "today".to_string(),
            total_cost_usd: 25.43,
            by_model: vec![
                ModelCost {
                    model: "opus".to_string(),
                    cost_usd: 18.24,
                    percentage: 72.0,
                },
                ModelCost {
                    model: "sonnet".to_string(),
                    cost_usd: 6.19,
                    percentage: 24.0,
                },
                ModelCost {
                    model: "glm".to_string(),
                    cost_usd: 1.00,
                    percentage: 4.0,
                },
            ],
            by_priority: vec![
                PriorityCost {
                    priority: "P0".to_string(),
                    cost_usd: 18.00,
                },
                PriorityCost {
                    priority: "P1".to_string(),
                    cost_usd: 7.43,
                },
            ],
        },
        costs_projected: CostAnalytics {
            timeframe: "month".to_string(),
            total_cost_usd: 762.90,
            by_model: vec![],
            by_priority: vec![],
        },
        subscriptions: vec![
            SubscriptionInfo {
                name: "Claude Pro".to_string(),
                quota_used: 328,
                quota_limit: Some(500),
                reset_time: "16d 9h".to_string(),
                status: "on_pace".to_string(),
            },
            SubscriptionInfo {
                name: "ChatGPT Plus".to_string(),
                quota_used: 12,
                quota_limit: Some(40),
                reset_time: "2h 30m".to_string(),
                status: "accelerate".to_string(),
            },
        ],
        recent_events: vec![
            EventInfo {
                timestamp: Utc::now(),
                event_type: "completions".to_string(),
                description: "Completed bd-123".to_string(),
                worker: Some("glm-alpha".to_string()),
                bead_id: Some("bd-123".to_string()),
            },
            EventInfo {
                timestamp: Utc::now() - Duration::minutes(5),
                event_type: "spawns".to_string(),
                description: "Spawned worker opus-charlie".to_string(),
                worker: Some("opus-charlie".to_string()),
                bead_id: None,
            },
            EventInfo {
                timestamp: Utc::now() - Duration::minutes(10),
                event_type: "errors".to_string(),
                description: "Worker opus-charlie failed health check".to_string(),
                worker: Some("opus-charlie".to_string()),
                bead_id: None,
            },
        ],
        timestamp: Utc::now(),
    }
}

/// Context source that counts calls for testing caching.
struct CountingContextSource {
    context: DashboardContext,
    call_count: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl ContextSource for CountingContextSource {
    async fn gather(&self) -> forge_chat::Result<DashboardContext> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(self.context.clone())
    }
}
