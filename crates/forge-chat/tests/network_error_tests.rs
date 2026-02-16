//! Network error handling and recovery tests for forge-chat.
//!
//! These tests verify that network errors are properly classified,
//! retried with exponential backoff, and presented with clear user guidance.

use forge_chat::error::ChatError;

#[cfg(test)]
mod network_error_classification {
    use super::*;

    #[test]
    fn test_timeout_error_is_retryable() {
        let err = ChatError::Timeout(30, "Request timed out".to_string());
        assert!(err.is_retryable(), "Timeout errors should be retryable");
        assert!(err.is_network_error(), "Timeout is a network error");
    }

    #[test]
    fn test_connection_failed_is_retryable() {
        let err = ChatError::ConnectionFailed("Could not connect".to_string());
        assert!(err.is_retryable(), "Connection failures should be retryable");
        assert!(err.is_network_error(), "Connection failure is a network error");
    }

    #[test]
    fn test_dns_resolution_failed_is_retryable() {
        let err = ChatError::DnsResolutionFailed {
            host: "api.anthropic.com".to_string(),
            message: "name or service not known".to_string(),
        };
        assert!(err.is_retryable(), "DNS resolution failures should be retryable");
        assert!(err.is_network_error(), "DNS failure is a network error");
    }

    #[test]
    fn test_network_unreachable_is_network_error() {
        let err = ChatError::NetworkUnreachable("No route to host".to_string());
        assert!(!err.is_retryable(), "Network unreachable is not auto-retryable (needs manual intervention)");
        assert!(err.is_network_error(), "Network unreachable is a network error");
    }

    #[test]
    fn test_api_transient_error_is_retryable() {
        let err = ChatError::ApiTransientError("503 Service Unavailable".to_string());
        assert!(err.is_retryable(), "Transient API errors should be retryable");
    }

    #[test]
    fn test_rate_limit_is_retryable() {
        let err = ChatError::RateLimitExceeded(10, 60);
        assert!(err.is_retryable(), "Rate limits should be retryable after waiting");
        assert!(!err.is_network_error(), "Rate limit is not a network error");
    }

    #[test]
    fn test_api_rate_limit_is_retryable() {
        let err = ChatError::ApiRateLimitExceeded(120);
        assert!(err.is_retryable(), "API rate limits should be retryable after waiting");
        assert!(!err.is_network_error(), "API rate limit is not a network error");
        assert!(err.is_rate_limit(), "Should be classified as rate limit");
    }

    #[test]
    fn test_retry_after_secs_extraction() {
        let err1 = ChatError::RateLimitExceeded(10, 60);
        assert_eq!(err1.retry_after_secs(), Some(60));

        let err2 = ChatError::ApiRateLimitExceeded(120);
        assert_eq!(err2.retry_after_secs(), Some(120));

        let err3 = ChatError::ApiError("Not a rate limit".to_string());
        assert_eq!(err3.retry_after_secs(), None);
    }

    #[test]
    fn test_api_error_is_not_retryable() {
        let err = ChatError::ApiError("400 Bad Request".to_string());
        assert!(!err.is_retryable(), "Permanent API errors should not be retryable");
    }
}

#[cfg(test)]
mod user_guidance {
    use super::*;

    #[test]
    fn test_timeout_error_friendly_message() {
        let err = ChatError::Timeout(30, "Connection timeout".to_string());
        let friendly = err.friendly_message();
        assert!(friendly.contains("timed out"), "Should mention timeout");
        assert!(friendly.contains("30s"), "Should include timeout duration");
    }

    #[test]
    fn test_dns_resolution_failed_friendly_message() {
        let err = ChatError::DnsResolutionFailed {
            host: "api.example.com".to_string(),
            message: "name or service not known".to_string(),
        };
        let friendly = err.friendly_message();
        assert!(friendly.contains("resolve"), "Should mention resolution");
        assert!(friendly.contains("api.example.com"), "Should include hostname");
    }

    #[test]
    fn test_network_unreachable_friendly_message() {
        let err = ChatError::NetworkUnreachable("No internet connection".to_string());
        let friendly = err.friendly_message();
        assert!(friendly.contains("unreachable"), "Should mention unreachable");
        assert!(friendly.contains("internet"), "Should mention connection");
    }

    #[test]
    fn test_timeout_error_suggested_action() {
        let err = ChatError::Timeout(30, "Request timeout".to_string());
        let action = err.suggested_action();
        assert!(action.contains("connection") || action.contains("again"),
                "Should suggest checking connection or retrying");
    }

    #[test]
    fn test_dns_error_suggested_action() {
        let err = ChatError::DnsResolutionFailed {
            host: "api.example.com".to_string(),
            message: "lookup failed".to_string(),
        };
        let action = err.suggested_action();
        assert!(action.contains("DNS"), "Should mention DNS");
    }

    #[test]
    fn test_network_unreachable_suggested_action() {
        let err = ChatError::NetworkUnreachable("No route".to_string());
        let action = err.suggested_action();
        assert!(action.contains("connection") || action.contains("network"),
                "Should mention checking network");
    }
}

#[cfg(test)]
mod http_error_classification {
    use super::*;

    #[test]
    fn test_http_status_429_creates_api_rate_limit() {
        let err = ChatError::from_http_status(429, "Too many requests");
        match err {
            ChatError::ApiRateLimitExceeded(_) => { /* expected */ }
            _ => panic!("Expected ApiRateLimitExceeded, got: {:?}", err),
        }
        assert!(err.is_rate_limit(), "429 should be classified as rate limit");
        assert!(err.is_retryable(), "429 should be retryable");
    }

    #[test]
    fn test_http_status_429_with_retry_after_header() {
        // Test with integer retry-after (seconds)
        let err = ChatError::from_http_response(429, "Too many requests", Some("120"));
        match err {
            ChatError::ApiRateLimitExceeded(wait) => {
                assert_eq!(wait, 120, "Should parse retry-after as 120 seconds");
            }
            _ => panic!("Expected ApiRateLimitExceeded, got: {:?}", err),
        }
    }

    #[test]
    fn test_http_status_429_without_retry_after() {
        // Test without retry-after header (should use default)
        let err = ChatError::from_http_response(429, "Too many requests", None);
        match err {
            ChatError::ApiRateLimitExceeded(wait) => {
                assert_eq!(wait, 60, "Should use default 60 seconds");
            }
            _ => panic!("Expected ApiRateLimitExceeded, got: {:?}", err),
        }
    }

    #[test]
    fn test_http_status_408_creates_timeout() {
        let err = ChatError::from_http_status(408, "Request timeout");
        match err {
            ChatError::Timeout(_, _) => { /* expected */ }
            _ => panic!("Expected Timeout, got: {:?}", err),
        }
    }

    #[test]
    fn test_http_status_503_creates_transient_error() {
        let err = ChatError::from_http_status(503, "Service unavailable");
        match err {
            ChatError::ApiTransientError(_) => { /* expected */ }
            _ => panic!("Expected ApiTransientError, got: {:?}", err),
        }
        assert!(err.is_retryable(), "503 errors should be retryable");
    }

    #[test]
    fn test_http_status_401_creates_auth_error() {
        let err = ChatError::from_http_status(401, "Unauthorized");
        assert!(!err.is_retryable(), "Auth errors should not be auto-retryable");

        match err {
            ChatError::ApiError(msg) => {
                assert!(msg.contains("401") || msg.contains("Authentication"),
                        "Should mention auth error");
            }
            _ => panic!("Expected ApiError, got: {:?}", err),
        }
    }
}

#[cfg(test)]
mod retry_behavior {
    use super::*;

    /// This test verifies the conceptual retry behavior.
    /// Actual retry logic is tested in integration tests with mock servers.
    #[test]
    fn test_retryable_errors_are_correctly_classified() {
        let retryable_errors = vec![
            ChatError::Timeout(30, "timeout".to_string()),
            ChatError::ConnectionFailed("connection failed".to_string()),
            ChatError::DnsResolutionFailed {
                host: "example.com".to_string(),
                message: "dns failed".to_string(),
            },
            ChatError::ApiTransientError("503".to_string()),
            ChatError::RateLimitExceeded(10, 60),
        ];

        for err in retryable_errors {
            assert!(
                err.is_retryable(),
                "Error {:?} should be retryable",
                err
            );
        }

        let non_retryable_errors = vec![
            ChatError::ApiError("400 Bad Request".to_string()),
            ChatError::ConfigError("Invalid config".to_string()),
            ChatError::ToolNotFound("unknown_tool".to_string()),
            ChatError::ActionCancelled,
        ];

        for err in non_retryable_errors {
            assert!(
                !err.is_retryable(),
                "Error {:?} should not be auto-retryable",
                err
            );
        }
    }

    #[test]
    fn test_network_errors_are_correctly_classified() {
        let network_errors = vec![
            ChatError::Timeout(30, "timeout".to_string()),
            ChatError::ConnectionFailed("failed".to_string()),
            ChatError::DnsResolutionFailed {
                host: "api.example.com".to_string(),
                message: "dns error".to_string(),
            },
            ChatError::NetworkUnreachable("no route".to_string()),
        ];

        for err in network_errors {
            assert!(
                err.is_network_error(),
                "Error {:?} should be classified as network error",
                err
            );
        }
    }
}

#[cfg(test)]
mod retry_after_parsing {
    use super::*;

    #[test]
    fn test_parse_retry_after_integer() {
        // Test parsing integer seconds
        assert_eq!(ChatError::parse_retry_after("60"), Some(60));
        assert_eq!(ChatError::parse_retry_after("120"), Some(120));
        assert_eq!(ChatError::parse_retry_after("0"), Some(0));
        assert_eq!(ChatError::parse_retry_after("  90  "), Some(90)); // with whitespace
    }

    #[test]
    fn test_parse_retry_after_invalid() {
        // Test invalid values
        assert_eq!(ChatError::parse_retry_after("invalid"), None);
        assert_eq!(ChatError::parse_retry_after(""), None);
        assert_eq!(ChatError::parse_retry_after("-10"), None); // negative not supported
    }

    #[test]
    fn test_parse_retry_after_http_date() {
        // Test parsing HTTP-date format
        // Note: This will return a duration from now, so we can only test it's Some
        let result = ChatError::parse_retry_after("Wed, 21 Oct 2099 07:28:00 GMT");
        assert!(result.is_some(), "Should parse valid HTTP-date");
    }

    #[test]
    fn test_from_http_response_with_various_retry_after() {
        // Test with integer retry-after
        let err1 = ChatError::from_http_response(429, "Rate limited", Some("45"));
        assert_eq!(err1.retry_after_secs(), Some(45));

        // Test without retry-after
        let err2 = ChatError::from_http_response(429, "Rate limited", None);
        assert_eq!(err2.retry_after_secs(), Some(60)); // default

        // Test with invalid retry-after (should use default)
        let err3 = ChatError::from_http_response(429, "Rate limited", Some("invalid"));
        assert_eq!(err3.retry_after_secs(), Some(60)); // default

        // Test 503 with retry-after
        let err4 = ChatError::from_http_response(503, "Service unavailable", Some("30"));
        match err4 {
            ChatError::ApiTransientError(_) => { /* expected */ }
            _ => panic!("Expected ApiTransientError for 503"),
        }
    }

    #[test]
    fn test_api_rate_limit_friendly_message() {
        let err = ChatError::ApiRateLimitExceeded(90);
        let msg = err.friendly_message();
        assert!(msg.contains("rate limit"), "Should mention rate limit");
        assert!(msg.contains("90"), "Should show wait time");
    }

    #[test]
    fn test_api_rate_limit_suggested_action() {
        let err = ChatError::ApiRateLimitExceeded(120);
        let action = err.suggested_action();
        assert!(action.contains("rate limit") || action.contains("retry"),
                "Should mention rate limit or retry");
    }
}
