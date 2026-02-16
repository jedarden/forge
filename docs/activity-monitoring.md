# Activity Monitoring in FORGE

## Overview

FORGE implements comprehensive activity monitoring for worker agents to detect when workers become unresponsive, idle, or stuck on tasks.

## Monitoring Thresholds

### Activity Freshness (15 minutes)
- **Threshold**: 15 minutes (900 seconds)
- **What it tracks**: Last activity timestamp in worker status files (`~/.forge/status/<worker-id>.json`)
- **Applies to**: All workers regardless of status
- **Alert type**: `WorkerStale` (Warning severity)
- **Trigger**: When `last_activity` timestamp is more than 15 minutes old

### Task Progress (30 minutes)
- **Threshold**: 30 minutes
- **What it tracks**: Progress on current task for active workers
- **Applies to**: Only workers with `status == "active"` and a `current_task` assigned
- **Alert type**: `TaskStuck` (Warning severity)
- **Trigger**: When worker is active with a task but no activity for 30+ minutes

## Idle vs Stuck Detection

FORGE distinguishes between two states:

### Idle Worker
- Worker has no activity for 15+ minutes
- Worker is NOT actively working on a task (`current_task` is null or status != "active")
- **Interpretation**: Worker may be waiting for work or has completed its task
- **Alert**: `WorkerStale` - "No activity detected from worker"
- **Severity**: Warning

### Stuck Worker
- Worker is actively working on a task (`status == "active"` and `current_task` is set)
- No activity for 30+ minutes
- **Interpretation**: Worker may be blocked, hung, or experiencing an issue
- **Alert**: `TaskStuck` - "Current task has been running too long"
- **Severity**: Warning

## Activity Timestamp Updates

Workers update their `last_activity` timestamp in the status file at:

1. **Worker launch**: Initial timestamp set by launcher script
2. **Status updates**: When worker updates its status file (manual or automated)
3. **Log entries**: Activity can be inferred from log file timestamps

**Note**: The current implementation relies on workers updating their status files. If a worker is running but not updating its status, it will be flagged as stale after 15 minutes.

## Health Monitoring Architecture

### Components

1. **HealthMonitor** (`crates/forge-worker/src/health.rs`)
   - Runs health checks on all workers
   - Configurable thresholds and check types
   - Tracks consecutive failures and recovery attempts

2. **AlertManager** (`crates/forge-tui/src/alert.rs`)
   - Raises, tracks, and resolves alerts
   - Deduplicates alerts per worker
   - Provides badge counts for UI display

3. **DataManager** (`crates/forge-tui/src/data.rs`)
   - Polls health monitor periodically
   - Generates alerts from health check results
   - Updates activity log and UI indicators

### Health Check Flow

```
1. StatusWatcher reads ~/.forge/status/*.json files
2. HealthMonitor performs health checks on each worker:
   - PID exists (process is running)
   - Activity fresh (last_activity within 15 mins)
   - Task progress (active tasks making progress)
   - Memory usage (optional, requires /proc)
   - Response health (optional, requires signal handling)
3. Failed checks generate HealthCheckResult with error type
4. DataManager creates alerts based on failed checks:
   - Priority: PID > Activity > Task > Memory > Response
5. Alerts displayed in UI:
   - Badge count in header
   - Alert list in Overview panel
   - Worker panel shows health indicators
```

## Configuration

### Default Configuration

```rust
HealthMonitorConfig {
    check_interval_secs: 30,           // Check every 30 seconds
    stale_activity_threshold_secs: 900, // 15 minutes
    task_stuck_threshold_mins: 30,      // 30 minutes
    enable_pid_check: true,
    enable_activity_check: true,
    enable_memory_check: false,
    enable_task_check: true,
    enable_response_check: false,
    enable_auto_recovery: false,        // Manual recovery by default
    auto_restart_after_failures: 2,
}
```

### Customization

To customize thresholds, modify `DEFAULT_STALE_THRESHOLD_SECS` in `crates/forge-worker/src/health.rs`:

```rust
/// Default stale activity threshold in seconds (15 minutes).
pub const DEFAULT_STALE_THRESHOLD_SECS: i64 = 900;
```

Or create a custom config:

```rust
let config = HealthMonitorConfig {
    stale_activity_threshold_secs: 600, // 10 minutes
    task_stuck_threshold_mins: 45,       // 45 minutes
    ..Default::default()
};
let monitor = HealthMonitor::new(config)?;
```

## Alert Behavior

### Alert Lifecycle

1. **Raised**: When health check fails
2. **Incremented**: Duplicate alerts increase occurrence count
3. **Acknowledged**: User marks alert as seen (doesn't resolve)
4. **Resolved**: When worker becomes healthy again
5. **Auto-cleared**: Resolved alerts automatically cleared after acknowledgment

### Alert Deduplication

Alerts are deduplicated per `(worker_id, alert_type)` pair. If the same worker triggers the same alert type multiple times, the occurrence count increments rather than creating duplicate alerts.

### UI Indicators

- **Badge count**: Shows unacknowledged warning + critical alerts in header
- **Health indicator**: Shows per-worker health level (●/◐/○)
- **Alert list**: Displays active alerts with severity icons (ℹ/⚠/✖)
- **Activity log**: Records health warnings with timestamps

## Recovery Guidance

When workers become unhealthy, the system provides actionable guidance:

- **WorkerStale**: "Worker may be stuck - check logs"
- **TaskStuck**: "Task may be stuck - verify progress"
- **WorkerCrashed**: "Process died - restart the worker"

## Best Practices

### For Worker Implementations

1. **Update status regularly**: Workers should update their `last_activity` timestamp periodically (e.g., every 5 minutes)
2. **Log activity**: Write to log files to provide evidence of activity
3. **Handle signals**: Optionally implement SIGUSR1 handler for response checks

### For Operators

1. **Monitor alerts**: Check the Overview panel regularly for health alerts
2. **Investigate stale workers**: Workers idle for 15+ minutes may need attention
3. **Check stuck tasks**: Workers active for 30+ minutes without progress may be blocked
4. **Review logs**: Use the Logs view to investigate worker behavior
5. **Manual intervention**: Current default is manual recovery - restart workers as needed

## Future Enhancements

- **Auto-restart**: Configurable automatic restart after consecutive failures
- **Heartbeat files**: Dedicated heartbeat mechanism independent of status files
- **Smart thresholds**: Adaptive thresholds based on task complexity
- **Recovery actions**: Automated recovery strategies (restart, escalate, alert)
- **Notification integration**: Slack/Discord notifications for critical alerts
