# Database Retry Logic Implementation

## Overview

Implemented comprehensive retry logic with exponential backoff for all database operations in the `forge-cost` crate to handle SQLite BUSY/LOCKED errors gracefully.

## Changes

### Constants Updated

- `DB_LOCK_INITIAL_DELAY_MS`: Changed from 50ms to 100ms
- Retry sequence: 100ms → 200ms → 400ms → 800ms → 1600ms (5 retries total)
- Maximum delay cap: 5 seconds

### Methods Refactored (33 total)

All database methods now use `self.with_retry("method_name", || { ... })` wrapper:

#### Core API Methods (4)
1. `insert_api_calls` - Insert API calls with aggregation
2. `get_daily_cost` - Get daily cost statistics
3. `get_last_timestamp` - Get last processed timestamp
4. `exists` - Check if API call exists (deduplication)

#### Subscription Methods (10)
5. `upsert_subscription` - Insert/update subscription
6. `get_subscription` - Get subscription by name
7. `get_active_subscriptions` - Get all active subscriptions
8. `get_all_subscriptions` - Get all subscriptions
9. `update_subscription_usage` - Update quota usage
10. `increment_subscription_usage` - Increment quota usage
11. `record_subscription_usage` - Record usage event
12. `get_subscription_usage` - Get usage records
13. `get_subscription_period_usage` - Get period usage
14. `deactivate_subscription` - Deactivate subscription

#### Task Event Methods (1)
15. `record_task_event` - Record task events

#### Statistics Aggregation Methods (4)
16. `aggregate_hourly_stats` - Aggregate hourly statistics
17. `aggregate_daily_stats` - Aggregate daily statistics
18. `aggregate_worker_efficiency` - Aggregate worker efficiency
19. `aggregate_model_performance` - Aggregate model performance

#### Statistics Query Methods (14)
20. `get_hourly_stat` - Get hourly statistics
21. `get_daily_stat` - Get daily statistics
22. `get_worker_efficiency` - Get worker efficiency stats
23. `get_model_performance` - Get model performance stats
24. `get_recent_hourly_stats` - Get recent hourly stats
25. `get_recent_daily_stats` - Get recent daily stats
26. `get_7day_task_trend` - Get 7-day task trend
27. `get_7day_cost_trend` - Get 7-day cost trend
28. `get_tasks_per_hour` - Get tasks per hour
29. `get_model_performance_7day` - Get 7-day model performance
30. `get_worker_efficiency_7day` - Get 7-day worker efficiency
31. `get_avg_cost_per_task_by_model` - Get average cost per task
32. `get_api_calls_since` - Get API calls since timestamp
33. `get_subscription_id` - Get subscription ID by name

## Retry Logic Behavior

### Exponential Backoff

```rust
attempt | delay
--------|-------
1       | 100ms
2       | 200ms
3       | 400ms
4       | 800ms
5       | 1600ms
```

### Error Detection

The retry logic detects SQLite database locked errors:
- `SQLITE_BUSY` (error code 5)
- `SQLITE_LOCKED` (error code 6)

### Logging

- **Info**: Successful retry after lock
- **Warn**: Lock detected, retrying with backoff
- **Warn**: Operation failed after all retries

## Testing

All tests pass successfully:
- **Unit tests**: 89 tests passed
- **Integration tests**: 36 tests passed
- **Doc tests**: 3 tests passed
- **Concurrent operations test**: ✅ Passed (4 threads inserting 100 calls each)

## Benefits

1. **Resilience**: Automatic handling of transient database lock errors
2. **Consistency**: All database operations use the same retry mechanism
3. **Observability**: Structured logging for retry events
4. **Performance**: Exponential backoff reduces contention
5. **Reliability**: Tested with concurrent operations

## Related Files

- `crates/forge-cost/src/db.rs` - Database implementation
- `crates/forge-cost/src/error.rs` - Error types and detection
- `crates/forge-cost/tests/integration_tests.rs` - Test coverage

## Implementation Date

2026-02-16 (Bead: fg-2eq2.1)
