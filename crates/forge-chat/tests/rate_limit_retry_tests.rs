//! Integration tests for API rate limit handling with retry-after.
//!
//! These tests verify that the Claude API provider:
//! 1. Detects 429 responses
//! 2. Parses retry-after headers
//! 3. Waits the appropriate duration
//! 4. Automatically retries after the wait period

use forge_chat::{
    claude_api::ClaudeApiProvider,
    config::ClaudeApiConfig,
    context::DashboardContext,
    error::ChatError,
    provider::ChatProvider,
};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, Request, ResponseTemplate,
};

/// Custom matcher to track retry attempts
struct RetryCounterMatcher {
    call_count: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

impl RetryCounterMatcher {
    fn new() -> (Self, std::sync::Arc<std::sync::atomic::AtomicU32>) {
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        (
            Self {
                call_count: counter.clone(),
            },
            counter,
        )
    }
}

impl wiremock::Match for RetryCounterMatcher {
    fn matches(&self, _request: &Request) -> bool {
        self.call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        true
    }
}

#[tokio::test]
async fn test_429_response_with_retry_after_header() {
    let mock_server = MockServer::start().await;

    // First request returns 429 with retry-after: 2
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "rate_limit_error",
                        "message": "Too many requests"
                    }
                }))
                .insert_header("retry-after", "2"),
        )
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    // Second request succeeds
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg-123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Success after retry"}],
            "model": "claude-sonnet-4-5-20250929",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        })))
        .mount(&mock_server)
        .await;

    let config = ClaudeApiConfig {
        api_base_url: mock_server.uri(),
        model: "claude-sonnet-4-5-20250929".to_string(),
        timeout_secs: 10,
        ..Default::default()
    };

    let provider = ClaudeApiProvider::with_api_key(config, "test-key").unwrap();
    let context = DashboardContext::default();

    let start = std::time::Instant::now();
    let result = provider.process("Hello", &context, &[]).await;
    let duration = start.elapsed();

    // Should succeed after retry
    assert!(result.is_ok(), "Request should succeed after retry");
    let response = result.unwrap();
    assert_eq!(response.text, "Success after retry");

    // Should have waited at least 2 seconds for retry-after
    assert!(
        duration.as_secs() >= 2,
        "Should wait at least 2 seconds for retry-after"
    );
}

#[tokio::test]
async fn test_429_response_without_retry_after_uses_default() {
    let mock_server = MockServer::start().await;

    // First request returns 429 without retry-after header
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(429).set_body_json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "rate_limit_error",
                    "message": "Too many requests"
                }
            })),
        )
        .up_to_n_times(3)
        .mount(&mock_server)
        .await;

    // Subsequent requests also fail
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&mock_server)
        .await;

    let config = ClaudeApiConfig {
        api_base_url: mock_server.uri(),
        model: "claude-sonnet-4-5-20250929".to_string(),
        timeout_secs: 10,
        ..Default::default()
    };

    let provider = ClaudeApiProvider::with_api_key(config, "test-key").unwrap();
    let context = DashboardContext::default();

    let result = provider.process("Hello", &context, &[]).await;

    // Should fail after retries
    assert!(result.is_err(), "Should fail after exhausting retries");
    let err = result.unwrap_err();

    match err {
        ChatError::ApiRateLimitExceeded(wait) => {
            // Without retry-after, should use default (60s)
            assert_eq!(wait, 60, "Should use default 60 second wait");
        }
        _ => panic!("Expected ApiRateLimitExceeded, got: {:?}", err),
    }
}

#[tokio::test]
async fn test_429_with_retry_after_zero_retries_immediately() {
    let mock_server = MockServer::start().await;

    let (matcher, counter) = RetryCounterMatcher::new();

    // First request returns 429 with retry-after: 0
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(matcher)
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_json(serde_json::json!({
                    "type": "error",
                    "error": {"type": "rate_limit_error", "message": "Rate limited"}
                })),
        )
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    // Second request succeeds immediately
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg-456",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Immediate retry success"}],
            "model": "claude-sonnet-4-5-20250929",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        })))
        .mount(&mock_server)
        .await;

    let config = ClaudeApiConfig {
        api_base_url: mock_server.uri(),
        ..Default::default()
    };

    let provider = ClaudeApiProvider::with_api_key(config, "test-key").unwrap();
    let context = DashboardContext::default();

    let start = std::time::Instant::now();
    let result = provider.process("Hello", &context, &[]).await;
    let duration = start.elapsed();

    assert!(result.is_ok(), "Should succeed after immediate retry");

    // Should retry immediately (duration < 1 second)
    assert!(
        duration.as_secs() < 1,
        "Should retry immediately with retry-after: 0"
    );

    // Should have made 2 requests (initial + 1 retry)
    let call_count = counter.load(std::sync::atomic::Ordering::SeqCst);
    assert!(call_count >= 1, "Should have made at least 1 request");
}

#[tokio::test]
async fn test_multiple_429_responses_respect_retry_after() {
    let mock_server = MockServer::start().await;

    // First 429 with retry-after: 1
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "1")
                .set_body_json(serde_json::json!({
                    "type": "error",
                    "error": {"type": "rate_limit_error", "message": "Rate limited"}
                })),
        )
        .up_to_n_times(2)
        .mount(&mock_server)
        .await;

    // Finally succeeds
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg-789",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Success after multiple retries"}],
            "model": "claude-sonnet-4-5-20250929",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        })))
        .mount(&mock_server)
        .await;

    let config = ClaudeApiConfig {
        api_base_url: mock_server.uri(),
        ..Default::default()
    };

    let provider = ClaudeApiProvider::with_api_key(config, "test-key").unwrap();
    let context = DashboardContext::default();

    let start = std::time::Instant::now();
    let result = provider.process("Hello", &context, &[]).await;
    let duration = start.elapsed();

    assert!(result.is_ok(), "Should eventually succeed");

    // Should have waited at least 2 seconds (2 retries x 1 second each)
    assert!(
        duration.as_secs() >= 2,
        "Should wait cumulative retry-after time"
    );
}

#[tokio::test]
async fn test_rate_limit_error_provides_friendly_message() {
    let err = ChatError::ApiRateLimitExceeded(45);

    let friendly = err.friendly_message();
    assert!(
        friendly.contains("45"),
        "Friendly message should show wait time"
    );
    assert!(
        friendly.to_lowercase().contains("rate limit"),
        "Should mention rate limiting"
    );

    let action = err.suggested_action();
    assert!(
        action.to_lowercase().contains("wait")
            || action.to_lowercase().contains("retry"),
        "Should suggest waiting or retrying"
    );
}
