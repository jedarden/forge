# Implementation Summary: Graceful Error Recovery (bd-1xdv)

**Epic:** bd-1xdv - Implement graceful error recovery
**Status:** COMPLETED - All 6 child tasks implemented
**Date:** 2026-02-17

## Overview

This epic covers the implementation of a comprehensive error recovery system for FORGE to handle common failure scenarios gracefully without crashing.

## Architecture

The error recovery system follows **ADR 0014: Error Handling Strategy** with these principles:

1. **Visibility First** - Show errors clearly in TUI
2. **No Silent Failures** - Every error is visible to user
3. **No Automatic Retry** - User decides if/when to retry (with exceptions for transient failures)
4. **Degrade Gracefully** - Broken component doesn't crash entire app
5. **Clear Error Messages** - Actionable guidance, not technical jargon

### Core Components

| Component | Location | Purpose |
|-----------|----------|---------|
| `ErrorRecoveryManager` | `crates/forge-tui/src/error_recovery.rs` | Tracks errors, manages degraded components |
| `ForgeError` | `crates/forge-core/src/error.rs` | Comprehensive error enum with guidance |
| `ChatError` | `crates/forge-chat/src/error.rs` | Chat/API-specific errors with retry info |
| `CostError` | `crates/forge-cost/src/error.rs` | Database lock handling with retries |

## Child Tasks Status

| # | Category | Bead | Status | Implementation |
|---|----------|------|--------|----------------|
| 1 | Database lock handling | bd-2ku0 | CLOSED | `CostError::DatabaseLocked` with retry logic |
| 2 | API rate limit handling | bd-16hv | CLOSED | `ChatError::ApiRateLimitExceeded` with retry-after |
| 3 | Worker crash recovery | bd-1a92 | CLOSED | `WorkerCrash` error + ADR 0018 |
| 4 | Invalid config handling | bd-a6yr | CLOSED | `ConfigInvalid` + validator.rs |
| 5 | Network timeout recovery | bd-24vt | CLOSED | Network banner + retry + error tracking |
| 6 | Missing dependency detection | bd-2oum | CLOSED | `forge_core::deps` + startup checks |

## Detailed Implementation

### 1. Database Lock Handling (bd-2ku0)

**File:** `crates/forge-cost/src/error.rs`

```rust
#[error("database is locked (retry {retry_count}/{max_retries}): {message}")]
DatabaseLocked {
    retry_count: u32,
    max_retries: u32,
    message: String,
}
```

Features:
- Detects SQLite BUSY/LOCKED errors
- Exponential backoff (100ms, 200ms, 400ms, 800ms, 1600ms)
- User-friendly messages via `friendly_message()`
- Retryable classification via `is_retryable()`

### 2. API Rate Limit Handling (bd-16hv)

**File:** `crates/forge-chat/src/error.rs`

```rust
#[error("API rate limited. Retry after {0}s")]
ApiRateLimitExceeded(u64),
```

Features:
- Parses `retry-after` header (integer or HTTP-date)
- Displays countdown to user
- Classifies HTTP status codes (429, 500-504, etc.)
- Provides suggested actions

**Test Coverage:** `crates/forge-chat/tests/rate_limit_retry_tests.rs`

### 3. Worker Crash Recovery (bd-1a92)

**Files:**
- `crates/forge-core/src/error.rs` - `WorkerCrash` error variant
- `crates/forge-tui/src/alert.rs` - Crash notifications
- `crates/forge-tui/src/data.rs` - Crash detection
- `docs/adr/0018-worker-crash-recovery.md` - Architecture

Features:
- Detects crashed workers via PID checks
- Clears stale assignee from beads
- Auto-restart with rate limiting (3 crashes in 10 min)
- Critical alerts in TUI

```rust
#[error("Worker {worker_id} crashed: {reason}")]
WorkerCrash {
    worker_id: String,
    reason: String,
    last_task: Option<String>,
    recoverable: bool,
}
```

### 4. Invalid Config Handling (bd-a6yr)

**Files:**
- `crates/forge-core/src/error.rs` - Config error variants
- `crates/forge-init/src/validator.rs` - Config validation

Features:
- Detects malformed YAML/TOML
- Shows specific parse error with context
- Provides guidance for fixing

```rust
#[error("Invalid configuration at {path}: {message}")]
ConfigInvalid { path: PathBuf, message: String },
```

### 5. Network Timeout Recovery (bd-24vt) - CLOSED

**Files:**
- `crates/forge-chat/src/error.rs` - Network error types
- `crates/forge-tui/src/app.rs` - Network status tracking and banner

```rust
#[error("Network timeout after {0}s: {1}")]
Timeout(u64, String),

#[error("Connection failed: {0}")]
ConnectionFailed(String),

#[error("DNS resolution failed for {host}: {message}")]
DnsResolutionFailed { host: String, message: String },

#[error("Network unreachable: {0}")]
NetworkUnreachable(String),
```

Features:
- Network status tracking in App state (`network_available`, `network_error_message`)
- "Network unreachable" banner displayed at top of TUI when network is down
- Duration tracking (shows how long network has been unavailable)
- Automatic recovery when network becomes available
- Integration with `ErrorRecoveryManager` for degraded component tracking
- Retry prompt with 'r' key

### 6. Missing Dependency Detection (bd-2oum) - CLOSED

**Files:**
- `crates/forge-core/src/deps.rs` - Dependency checking module
- `src/main.rs` - Startup integration

```rust
pub struct Dependency {
    pub name: &'static str,
    pub required: bool,
    pub purpose: &'static str,
    pub install_instructions: &'static str,
}
```

Features:
- Checks for required dependencies at startup: tmux, git
- Checks for optional dependencies: br, jq
- Clear error messages with install instructions for each platform
- Graceful degradation for optional dependencies (warning only)
- Version detection for found dependencies
- Startup fails only if required dependencies are missing

## TUI Integration

### ErrorRecoveryManager

Thread-safe manager for tracking errors and degraded components:

```rust
pub struct SharedErrorRecoveryManager {
    inner: Arc<Mutex<ErrorRecoveryManager>>,
}

impl SharedErrorRecoveryManager {
    pub fn record_error(...) -> usize;
    pub fn mark_degraded(component, error_id);
    pub fn mark_recovered(component);
    pub fn is_degraded(component) -> bool;
    pub fn unacknowledged_errors() -> Vec<RecordedError>;
}
```

### Error Categories

```rust
pub enum ErrorCategory {
    Database,    // SQLite errors
    Config,      // YAML parsing, validation
    Network,     // HTTP, timeouts
    Worker,      // Spawn, health, crashes
    Chat,        // Backend communication
    FileSystem,  // I/O, permissions
    Terminal,    // TUI errors
    Internal,    // Bugs
}
```

### Severity Levels

```rust
pub enum ErrorSeverity {
    Info,     // Informational, not really an error
    Warning,  // Something went wrong but operation continues
    Error,    // Component failed, degraded mode
    Fatal,    // App cannot continue
}
```

## Testing

### Unit Tests
- `crates/forge-tui/src/error_recovery.rs` - Error recording, degraded components
- `crates/forge-chat/tests/rate_limit_retry_tests.rs` - Rate limit parsing
- `crates/forge-chat/tests/network_error_tests.rs` - Network error handling
- `crates/forge-core/src/error.rs` - Error classification

### Integration Tests
- `docs/adr/0018-worker-crash-recovery.md` - Phase 3 manual testing plan

## Success Criteria

| Criteria | Status |
|----------|--------|
| All error types handled gracefully | ✅ Complete (6/6) |
| User sees helpful error messages | ✅ Yes |
| System recovers automatically where possible | ✅ Yes |
| No crashes on recoverable errors | ✅ Yes |

## Related Documentation

- [ADR 0014: Error Handling Strategy](docs/adr/0014-error-handling-strategy.md)
- [ADR 0018: Worker Crash Recovery](docs/adr/0018-worker-crash-recovery.md)
- [Database Documentation](docs/DATABASE.md)

## Completion Notes

All 6 child tasks have been implemented:

1. **Database lock handling** - SQLite BUSY/LOCKED errors with exponential backoff
2. **API rate limit handling** - 429 responses with retry-after parsing
3. **Worker crash recovery** - PID monitoring, auto-restart with rate limiting
4. **Invalid config handling** - YAML validation with clear error messages
5. **Network timeout recovery** - TUI banner, status tracking, retry support
6. **Missing dependency detection** - Startup checks with install instructions

The epic can now be closed.
