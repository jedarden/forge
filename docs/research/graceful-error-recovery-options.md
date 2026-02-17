# Research: Graceful Error Recovery Options

**Bead:** bd-2cw7 (Alternative for fg-2eq2)
**Date:** 2026-02-17
**Status:** Research Complete
**Author:** Claude Worker

---

## Executive Summary

This document analyzes approaches to graceful error recovery for FORGE, a terminal-based Agent Orchestration Dashboard. The analysis examines what has already been implemented (bd-1xdv epic - COMPLETED), identifies gaps, and proposes options for further improvement.

**Key Finding:** The existing implementation (ADR 0014 + bd-1xdv epic with 6 child tasks) is comprehensive and follows best practices. No major gaps require immediate attention. This document serves as a reference for the design decisions made and potential future enhancements.

---

## Table of Contents

1. [Current Implementation Analysis](#current-implementation-analysis)
2. [Option 1: Current Implementation (No Changes)](#option-1-current-implementation-no-changes)
3. [Option 2: Enhanced Automatic Recovery](#option-2-enhanced-automatic-recovery)
4. [Option 3: Hybrid Approach](#option-3-hybrid-approach)
5. [Option 4: Full Resilience Engineering](#option-4-full-resilience-engineering)
6. [Comparison Matrix](#comparison-matrix)
7. [Recommendation](#recommendation)
8. [Appendix: Error Recovery Patterns](#appendix-error-recovery-patterns)

---

## Current Implementation Analysis

### What's Already Built

The bd-1xdv epic ("Implement graceful error recovery") has been COMPLETED with all 6 child tasks:

| Task | Status | Implementation |
|------|--------|----------------|
| bd-2ku0: Database lock handling | CLOSED | `CostError::DatabaseLocked` with exponential backoff |
| bd-16hv: API rate limit handling | CLOSED | `ChatError::ApiRateLimitExceeded` with retry-after parsing |
| bd-1a92: Worker crash recovery | CLOSED | `WorkerCrash` error + ADR 0018 + CrashRecoveryManager |
| bd-a6yr: Invalid config handling | CLOSED | `ConfigInvalid` + validator.rs |
| bd-24vt: Network timeout recovery | CLOSED | Network banner + status tracking + retry support |
| bd-2oum: Missing dependency detection | CLOSED | `forge_core::deps` + startup checks |

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    FORGE Error Recovery Stack                │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────────┐  ┌──────────────────┐                 │
│  │ ErrorRecovery    │  │ CrashRecovery    │                 │
│  │ Manager          │  │ Manager          │                 │
│  │ (forge-tui)      │  │ (forge-worker)   │                 │
│  └────────┬─────────┘  └────────┬─────────┘                 │
│           │                     │                            │
│           ▼                     ▼                            │
│  ┌──────────────────────────────────────────┐               │
│  │              Error Types                  │               │
│  │  ForgeError (core) + ChatError (chat)    │               │
│  │  + CostError (cost)                       │               │
│  └──────────────────────────────────────────┘               │
│           │                                                  │
│           ▼                                                  │
│  ┌──────────────────────────────────────────┐               │
│  │           TUI Rendering                   │               │
│  │  - Error banners (network, degraded)     │               │
│  │  - Alert notifications                    │               │
│  │  - Component degradation indicators       │               │
│  └──────────────────────────────────────────┘               │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### Design Philosophy (ADR 0014)

The current implementation follows these principles:

1. **Visibility First** - Show errors clearly in TUI
2. **No Silent Failures** - Every error is visible to user
3. **No Automatic Retry** - User decides if/when to retry (with exceptions)
4. **Degrade Gracefully** - Broken component doesn't crash entire app
5. **Clear Error Messages** - Actionable guidance, not technical jargon

### Error Categories Implemented

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

### Error Severity Levels

```rust
pub enum ErrorSeverity {
    Info,     // Informational, not really an error
    Warning,  // Something went wrong but operation continues
    Error,    // Component failed, degraded mode
    Fatal,    // App cannot continue
}
```

---

## Option 1: Current Implementation (No Changes)

### Description

Keep the existing implementation as-is. The bd-1xdv epic is complete with comprehensive error handling across all major failure scenarios.

### Pros

| Benefit | Description |
|---------|-------------|
| **Proven** | Already implemented and tested with 13+ unit tests |
| **Follows ADR 0014** | Consistent with documented architecture decisions |
| **Zero Risk** | No new code = no new bugs |
| **Developer-Focused** | Visibility over convenience (appropriate for dev tools) |
| **Simple** | No complex retry/fallback logic to maintain |
| **Documented** | ADRs 0014 and 0018 explain all decisions |

### Cons

| Drawback | Description |
|----------|-------------|
| **Manual Recovery** | User must fix most errors manually |
| **No Auto-Healing** | Transient failures require user action |
| **Learning Curve** | Users need to understand error messages |
| **Verbose** | Many notifications during multi-failure scenarios |

### Implementation Effort

**None** - Already complete.

### When to Choose

- When stability is more important than convenience
- When users are developers who prefer explicit control
- When the current implementation meets all requirements

---

## Option 2: Enhanced Automatic Recovery

### Description

Add automatic retry with exponential backoff for transient errors, while keeping visibility for user.

### Key Changes

1. **Retry Manager** - Centralized retry logic with configurable policies
2. **Automatic Backoff** - For network, API, and database errors
3. **Silent Recovery** - Auto-recover without user interruption (with notification)
4. **Retry Budget** - Prevent infinite retry loops

### Implementation Design

```rust
pub struct RetryPolicy {
    max_retries: u32,
    initial_delay_ms: u64,
    max_delay_ms: u64,
    backoff_multiplier: f64,
    jitter: bool,
}

pub struct RetryManager {
    policies: HashMap<ErrorCategory, RetryPolicy>,
    active_retries: HashMap<String, RetryState>,
    budget: RetryBudget,
}

impl RetryManager {
    pub async fn with_retry<T, F, Fut>(
        &mut self,
        operation: &str,
        category: ErrorCategory,
        f: F,
    ) -> Result<T, ForgeError>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T, ForgeError>>,
    {
        let policy = self.policies.get(&category).cloned()
            .unwrap_or_default();

        for attempt in 0..=policy.max_retries {
            match f().await {
                Ok(result) => return Ok(result),
                Err(e) if e.is_recoverable() && attempt < policy.max_retries => {
                    let delay = self.calculate_delay(&policy, attempt);
                    tokio::time::sleep(delay).await;
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!()
    }
}
```

### Pros

| Benefit | Description |
|---------|-------------|
| **Reduced Manual Work** | Transient failures auto-resolve |
| **Better UX** | Fewer interruptions for temporary issues |
| **Resilient** | Handles network flakiness gracefully |
| **Configurable** | Per-category retry policies |
| **Standard Pattern** | Well-established in industry |

### Cons

| Drawback | Description |
|----------|-------------|
| **Complexity** | More code to maintain |
| **Hidden Failures** | Retries can mask underlying issues |
| **Violates ADR 0014** | Goes against "no automatic retry" principle |
| **Resource Usage** | Retries consume API calls/compute |
| **Testing** | More edge cases to test |

### Implementation Effort

**Medium** (2-3 days)
- RetryManager module: 1 day
- Integration with existing error handling: 1 day
- Testing and documentation: 0.5-1 day

### When to Choose

- When users expect "just works" behavior
- When network/API flakiness is common
- When transient failures are significantly impacting productivity

---

## Option 3: Hybrid Approach

### Description

Keep ADR 0014 philosophy but add **opt-in** automatic recovery for specific error types. User controls which errors auto-retry.

### Key Changes

1. **Recovery Mode Config** - User selects recovery behavior per category
2. **Visual Retry Indicator** - Show when auto-retry is in progress
3. **Easy Opt-Out** - Cancel ongoing retries
4. **Audit Log** - Record all automatic actions

### Configuration Design

```yaml
# ~/.forge/config.yaml
error_recovery:
  # Per-category settings
  network:
    auto_retry: true
    max_retries: 3
    notify_on_retry: true
  database:
    auto_retry: true
    max_retries: 5
    backoff_ms: 100
  api_rate_limit:
    auto_retry: true
    wait_for_reset: true
  worker_crash:
    auto_restart: false  # Opt-in, dangerous
    max_restarts_per_hour: 3
  config:
    auto_retry: false  # User must fix
    show_guidance: true
```

### UI Integration

```
┌─ FORGE ─────────────────────────────────────────────────────┐
│                                                              │
│  ⟳ Network retry in progress (attempt 2/3)... [Cancel]     │
│                                                              │
│  ┌─ WORKERS ────────────────────────────────────────────┐   │
│  │ sonnet-alpha  │ active  │ bd-123  │ 10:45:23        │   │
│  │ haiku-beta    │ idle    │ -       │ 10:44:15        │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### Pros

| Benefit | Description |
|---------|-------------|
| **User Control** | Each user configures their preference |
| **Best of Both** | Automation where wanted, control where needed |
| **Respects ADR 0014** | Visibility maintained, auto-retry is opt-in |
| **Transparent** | Visual indicators for all automatic actions |
| **Flexible** | Different policies per environment/project |

### Cons

| Drawback | Description |
|----------|-------------|
| **Config Complexity** | More settings to understand |
| **Documentation** | Must explain all options |
| **Testing Matrix** | Many configuration combinations |
| **Decision Fatigue** | User must decide what to enable |

### Implementation Effort

**Medium-High** (3-4 days)
- Config schema and validation: 0.5 day
- RecoveryPolicyManager: 1-1.5 days
- UI retry indicators: 1 day
- Testing and documentation: 1 day

### When to Choose

- When different users have different needs
- When you want to preserve ADR 0014 principles
- When transparency is important

---

## Option 4: Full Resilience Engineering

### Description

Implement comprehensive resilience patterns: circuit breakers, bulkheads, health endpoints, and chaos testing.

### Key Components

1. **Circuit Breaker** - Stop calling failing services
2. **Bulkhead** - Isolate failures to prevent cascade
3. **Health Endpoints** - Expose system health metrics
4. **Chaos Testing** - Inject failures for testing
5. **Observability** - Metrics, traces, structured logs

### Circuit Breaker Design

```rust
pub enum CircuitState {
    Closed,       // Normal operation
    Open,         // Failing, reject requests
    HalfOpen,     // Testing if service recovered
}

pub struct CircuitBreaker {
    state: CircuitState,
    failure_count: u32,
    failure_threshold: u32,
    success_count: u32,
    success_threshold: u32,
    timeout: Duration,
    last_failure: Option<Instant>,
}

impl CircuitBreaker {
    pub fn call<T>(&mut self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        match self.state {
            CircuitState::Open => {
                if self.should_attempt_reset() {
                    self.state = CircuitState::HalfOpen;
                } else {
                    return Err("Circuit open, service unavailable");
                }
            }
            _ => {}
        }

        match f() {
            Ok(result) => {
                self.on_success();
                Ok(result)
            }
            Err(e) => {
                self.on_failure();
                Err(e)
            }
        }
    }
}
```

### Bulkhead Pattern

```rust
pub struct Bulkhead {
    name: String,
    max_concurrent: usize,
    semaphore: Arc<Semaphore>,
}

impl Bulkhead {
    pub async fn execute<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let _permit = self.semaphore.acquire().await?;
        f().await
    }
}
```

### Health Endpoints

```rust
pub struct HealthCheck {
    name: String,
    checker: Box<dyn Fn() -> HealthStatus>,
    timeout: Duration,
}

pub enum HealthStatus {
    Healthy,
    Degraded { reason: String },
    Unhealthy { reason: String },
}

pub struct HealthEndpoint {
    checks: Vec<HealthCheck>,
}

impl HealthEndpoint {
    pub async fn check_all(&self) -> SystemHealth {
        let results = join_all(self.checks.iter().map(|c| c.check())).await;
        SystemHealth::aggregate(results)
    }
}
```

### Pros

| Benefit | Description |
|---------|-------------|
| **Production-Grade** | Industry-standard resilience patterns |
| **Prevents Cascade** | One failure doesn't bring down system |
| **Self-Healing** | System recovers without intervention |
| **Observable** | Clear visibility into system health |
| **Testable** | Chaos testing validates resilience |

### Cons

| Drawback | Description |
|----------|-------------|
| **Over-Engineering** | May be overkill for TUI app |
| **Complexity** | Significant code increase |
| **Learning Curve** | Team must understand patterns |
| **Maintenance** | More code = more maintenance |
| **Performance** | Overhead from wrappers |

### Implementation Effort

**High** (1-2 weeks)
- Circuit breaker module: 2 days
- Bulkhead isolation: 1 day
- Health endpoints: 1 day
- Observability integration: 2 days
- Chaos testing framework: 2 days
- Testing and documentation: 2 days

### When to Choose

- When building a distributed system
- When uptime SLA is critical
- When you have dedicated SRE capacity
- When failures have high business impact

---

## Comparison Matrix

| Criterion | Option 1 (Current) | Option 2 (Auto) | Option 3 (Hybrid) | Option 4 (Full) |
|-----------|-------------------|-----------------|-------------------|-----------------|
| **Implementation Effort** | None | Medium | Medium-High | High |
| **Complexity** | Low | Medium | Medium | High |
| **User Control** | High | Low | High | Medium |
| **Automatic Recovery** | Minimal | High | Configurable | High |
| **ADR 0014 Compliance** | Full | Partial | Full (opt-in) | Partial |
| **Maintenance** | Low | Medium | Medium | High |
| **Risk** | None | Medium | Low | Medium |
| **Appropriate for TUI** | Yes | Yes | Yes | Overkill |

### Scoring (1-5, higher is better for FORGE context)

| Factor | Weight | Opt 1 | Opt 2 | Opt 3 | Opt 4 |
|--------|--------|-------|-------|-------|-------|
| Simplicity | 25% | 5 | 3 | 3 | 1 |
| User Control | 20% | 5 | 2 | 5 | 3 |
| Resilience | 20% | 3 | 4 | 4 | 5 |
| Effort | 20% | 5 | 3 | 2 | 1 |
| ADR Compliance | 15% | 5 | 2 | 4 | 2 |
| **Weighted Score** | 100% | **4.6** | **2.9** | **3.6** | **2.3** |

---

## Recommendation

### Primary Recommendation: Option 1 (Keep Current Implementation)

**Rationale:**

1. **Already Comprehensive** - The bd-1xdv epic covered all major error categories
2. **Follows Architecture** - Compliant with ADR 0014 design decisions
3. **Zero Risk** - No new code means no new bugs
4. **Appropriate Scope** - FORGE is a developer TUI, not a distributed system
5. **User Preference** - Developers prefer explicit control over magic

### Secondary Recommendation: Option 3 (If User Feedback Demands)

If users consistently complain about manual recovery burden:

1. Add opt-in auto-retry for network errors only
2. Keep all other error handling as-is
3. Implement visual retry indicator
4. Maintain ADR 0014 philosophy with transparency

### Not Recommended: Options 2 and 4

- **Option 2** violates ADR 0014 without user control
- **Option 4** is over-engineering for a TUI application

---

## Appendix: Error Recovery Patterns

### A. Exponential Backoff

```rust
fn calculate_delay(attempt: u32, base_ms: u64, max_ms: u64) -> Duration {
    let delay = base_ms * 2_u64.pow(attempt);
    let capped = delay.min(max_ms);
    let jitter = rand::random::<u64>() % (capped / 4);
    Duration::from_millis(capped + jitter)
}
```

### B. Circuit Breaker State Machine

```
           success threshold reached
                    ↓
    ┌───────┐    ┌──────────┐    ┌────────┐
    │ CLOSED│───→│ HALF-OPEN│───→│  OPEN  │
    │       │    │          │    │        │
    └───────┘    └──────────┘    └────────┘
        ↑               │            │
        │   failure     │   timeout  │
        └───────────────┴────────────┘
```

### C. Error Classification Tree

```
Error
├── Recoverable (can retry)
│   ├── Transient (will likely succeed on retry)
│   │   ├── Network timeout
│   │   ├── API rate limit
│   │   └── Database locked
│   └── Permanent (requires user action)
│       ├── Invalid config
│       ├── Missing dependency
│       └── Permission denied
└── Fatal (cannot continue)
    ├── Terminal init failure
    ├── Database migration error
    └── Internal error (bug)
```

### D. References

- **ADR 0014**: Error Handling Strategy
- **ADR 0018**: Worker Crash Recovery
- **bd-1xdv Epic**: Implement graceful error recovery (COMPLETED)
- **Implementation Summary**: docs/IMPLEMENTATION_SUMMARY_bd-1xdv.md

---

## Document History

| Date | Author | Change |
|------|--------|--------|
| 2026-02-17 | Claude Worker | Initial research document |
