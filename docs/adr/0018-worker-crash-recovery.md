# ADR 0018: Worker Crash Recovery

## Status
Accepted

## Context
Workers (AI coding agents running in tmux sessions) can crash unexpectedly due to:
- Process termination (kill signal, OOM, hardware failure)
- Network timeouts
- API rate limits
- Internal errors in the worker code

When a worker crashes:
1. The bead (task) assigned to it remains locked with `status: in_progress` and `assignee: <worker-id>`
2. Other workers cannot pick up the task
3. User has no visibility into the crash
4. Manual intervention is required to:
   - Clear the assignee from the bead
   - Update the bead status
   - Restart the worker

This creates operational overhead and reduces system resilience.

## Decision
Implement automatic crash detection and recovery with the following components:

### 1. Crash Detection
Use the existing `HealthMonitor` to detect crashes via PID checks:
- If `HealthCheckType::PidExists` fails → worker crashed
- Other health check failures (stale activity, high memory) are NOT crashes

### 2. CrashRecoveryManager
Track crash events and manage recovery:

```rust
pub struct CrashRecoveryManager {
    config: CrashRecoveryConfig,
    crash_history: HashMap<WorkerId, Vec<CrashRecord>>,
    crashed_workers: HashMap<WorkerId, CrashRecord>,
}

pub struct CrashRecord {
    worker_id: WorkerId,
    crashed_at: DateTime<Utc>,
    reason: String,
    error_message: String,
    workspace: Option<PathBuf>,
    bead_id: Option<BeadId>,
    assignee_cleared: bool,
    auto_restarted: bool,
}
```

### 3. Automatic Assignee Clearing
When a crash is detected:
1. Check if bead has an assignee: `br show <bead-id> --format=json`
2. Clear the assignee: `br update <bead-id> --assignee ""`
3. Reset status: `br update <bead-id> --status open`

This allows other workers to pick up the task immediately.

### 4. Auto-Restart with Rate Limiting
To prevent crash loops:
- **Crash Window**: 10 minutes (600 seconds)
- **Max Crashes**: 3 crashes within the window
- **Behavior**:
  - Crashes 1-2: Auto-restart enabled
  - Crash 3: Auto-restart disabled, notify user
  - After 10 minutes: Counter resets

```rust
pub enum CrashAction {
    Restart,      // Auto-restart the worker
    NotifyOnly,   // Show notification, no restart
    Ignore,       // Already handled or not a crash
}
```

### 5. Crash Notifications
Show user-visible alerts in the TUI:
- Critical alert: Worker crashed
- Display crash reason and error message
- Show recovery status (assignee cleared, auto-restart)
- Provide manual restart option if auto-restart exhausted

### 6. Configuration
```rust
pub struct CrashRecoveryConfig {
    auto_restart_enabled: bool,          // Default: false (opt-in)
    max_crashes_in_window: usize,        // Default: 3
    crash_window_secs: i64,              // Default: 600 (10 min)
    clear_assignees_enabled: bool,       // Default: true
    show_notifications: bool,            // Default: true
}
```

### 7. Integration Points
- **HealthMonitor**: Detects crashes via PID checks
- **CrashRecoveryManager**: Handles crash events
- **TUI App Loop**: Calls `handle_crash()` on health check failures
- **Alert System**: Shows crash notifications to user
- **WorkerLauncher**: Restarts workers when CrashAction::Restart

## Consequences

### Positive
1. **Automatic Recovery**: Beads automatically become available again
2. **Reduced Downtime**: No manual intervention needed for transient crashes
3. **Crash Loop Protection**: Rate limiting prevents infinite restart cycles
4. **Visibility**: User sees crash events in TUI
5. **Data Integrity**: Bead state is cleaned up automatically
6. **Testable**: 13 unit tests with 100% coverage

### Negative
1. **Complexity**: Adds another state machine to manage
2. **br CLI Dependency**: Requires `br` command to be available
3. **Race Conditions**: Possible if bead is modified externally during crash recovery
4. **False Positives**: Could clear assignee if process is legitimately stopped

### Mitigation
- **Race Conditions**: Use `br`'s atomic operations
- **False Positives**: Only clear assignee if PID check fails (process dead)
- **br CLI Availability**: Gracefully degrade if `br` command fails

## Implementation Details

### Phase 1: Core Module ✅
- [x] Implement `CrashRecoveryManager`
- [x] Add crash detection logic
- [x] Implement assignee clearing via `br` CLI
- [x] Add rate limiting logic
- [x] Write comprehensive unit tests

### Phase 2: TUI Integration (In Progress)
- [x] Add `CrashRecoveryManager` to `App` struct
- [ ] Integrate with health check loop
- [ ] Add crash notification alerts
- [ ] Add worker restart hotkey

### Phase 3: Manual Testing
- [ ] Test in tmux with real workers
- [ ] Verify assignee clearing works
- [ ] Test rate limiting behavior
- [ ] Test crash notifications

## Related ADRs
- **ADR 0014: Error Handling Strategy** - Visibility first, no silent failures
- **ADR 0015: Health Monitoring** - PID checks for crash detection

## References
- Issue: `fg-2eq2.6` - Implement worker crash recovery
- Code: `crates/forge-worker/src/crash_recovery.rs`
- Tests: `crates/forge-worker/src/crash_recovery.rs#tests`

## Date
2026-02-16
