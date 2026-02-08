# Data Integration Architecture for forge-tui

**Date**: 2026-02-08
**Status**: Draft
**Related**: ADR 0008 (Real-Time Update Architecture)
**Bead**: fg-29n

---

## 1. Overview

This document defines the architecture for integrating real worker data into forge-tui, replacing the current hardcoded mock data. It covers:

1. Data structures for worker status
2. Status file format (`~/.forge/status/*.json`)
3. How forge-tui reads and displays data
4. Integration patterns with existing components

---

## 2. Current State Analysis

### 2.1 Existing Components

| Component | Location | Status | Purpose |
|-----------|----------|--------|---------|
| `StatusWatcher` | `forge-tui/src/status.rs` | **Implemented** | File watching for `~/.forge/status/*.json` |
| `WorkerStatusFile` | `forge-tui/src/status.rs` | **Implemented** | Parses worker status JSON |
| `LogTailer` | `forge-tui/src/log.rs` | **Implemented** | Streams log entries from files |
| `LogBuffer` | `forge-tui/src/log.rs` | **Implemented** | Ring buffer for log entries |
| `App` | `forge-tui/src/app.rs` | **Needs Integration** | Uses hardcoded mock data |
| `WorkerStatus` | `forge-core/src/types.rs` | **Implemented** | Enum: Active, Idle, Failed, etc. |

### 2.2 Gap Analysis

The `App` struct in `app.rs` currently renders hardcoded strings in methods like:
- `draw_overview()` - Mock worker counts, subscriptions, task queue
- `draw_workers()` - Static ASCII table of workers
- `draw_logs()` - Fixed log entries
- `draw_costs()` - Hardcoded dollar amounts

**Missing**: Connection between `App` and `StatusWatcher`/`LogTailer`.

---

## 3. Data Structures

### 3.1 Worker Status File Format (`~/.forge/status/*.json`)

**Location**: `~/.forge/status/{worker_id}.json`

```json
{
  "worker_id": "sonnet-alpha",
  "status": "active",
  "model": "claude-sonnet-4-5",
  "workspace": "/home/coder/forge",
  "pid": 12345,
  "started_at": "2026-02-08T10:30:00Z",
  "last_activity": "2026-02-08T10:35:00Z",
  "current_task": "fg-29n",
  "tasks_completed": 5,
  "container_id": null
}
```

**Field Definitions**:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `worker_id` | string | **Yes** | Unique identifier matching filename (without `.json`) |
| `status` | string | No (default: `idle`) | One of: `active`, `idle`, `failed`, `stopped`, `error`, `starting` |
| `model` | string | No | Model identifier (e.g., `claude-sonnet-4-5`, `glm-4.7`) |
| `workspace` | string | No | Working directory path |
| `pid` | u32 | No | Process ID of worker or tmux server |
| `started_at` | ISO8601 | No | When worker was started |
| `last_activity` | ISO8601 | No | Last heartbeat/activity timestamp |
| `current_task` | string | No | Bead ID being worked on (null if idle) |
| `tasks_completed` | u32 | No (default: 0) | Count of completed tasks |
| `container_id` | string | No | Container ID for containerized workers |

**Status Values**:

| Status | Meaning | Healthy? |
|--------|---------|----------|
| `active` | Running and processing a task | Yes |
| `idle` | Running but waiting for work | Yes |
| `starting` | Initializing | Yes |
| `failed` | Process crashed or errored | No |
| `stopped` | Intentionally stopped | No |
| `error` | Status file corrupt or unreadable | No |

### 3.2 Extended Worker State (In-Memory)

The TUI maintains additional derived state beyond what's in the status file:

```rust
/// Extended worker state for TUI display
pub struct WorkerDisplayState {
    /// Base status from file
    pub status: WorkerStatusFile,

    /// Time since last activity (computed)
    pub idle_duration: Option<Duration>,

    /// Health indicator (derived from status)
    pub is_healthy: bool,

    /// Uptime (computed from started_at)
    pub uptime: Option<Duration>,

    /// Model tier (derived from model name)
    pub tier: WorkerTier,

    /// Session name for tmux (computed from worker_id)
    pub session_name: String,
}
```

### 3.3 Aggregated Worker Stats

For dashboard summary panels:

```rust
/// Summary statistics for worker pool
pub struct WorkerPoolStats {
    /// Total worker count
    pub total: usize,

    /// Workers by status
    pub active: usize,
    pub idle: usize,
    pub starting: usize,
    pub failed: usize,
    pub stopped: usize,

    /// Workers by model/tier
    pub by_model: HashMap<String, usize>,
    pub by_tier: HashMap<WorkerTier, usize>,

    /// Health summary
    pub healthy_count: usize,
    pub unhealthy_count: usize,

    /// Aggregate tasks
    pub total_tasks_completed: u32,
}
```

---

## 4. Log Data Structures

### 4.1 Log File Location

Workers write logs to: `~/.forge/logs/{worker_id}.log`

Alternatively, workers can be configured to write to `~/.beads-workers/{session-name}.log` (legacy location).

### 4.2 Log Entry Format

Supports both JSON and text formats:

**JSON (preferred)**:
```json
{"timestamp": "2026-02-08T14:23:45Z", "level": "info", "message": "Task completed", "worker_id": "worker-1", "bead_id": "fg-29n"}
```

**Text (fallback)**:
```
2026-02-08T14:23:45Z [INFO] Task completed
[WARN] Low memory warning
Plain text message
```

### 4.3 Log Buffer Configuration

Per ADR 0008:
- **Capacity**: 1000 entries per source (ring buffer)
- **Poll interval**: 100ms
- **Batch size**: 10 entries before UI flush

---

## 5. Integration Architecture

### 5.1 Component Diagram

```
┌────────────────────────────────────────────────────────────────────────┐
│  forge-tui App                                                          │
│                                                                          │
│  ┌─────────────────────┐     ┌─────────────────────┐                    │
│  │ StatusWatcher       │     │ LogManager          │                    │
│  │ (notify + debounce) │     │ (per-worker tailers)│                    │
│  └──────────┬──────────┘     └──────────┬──────────┘                    │
│             │                           │                                │
│             ▼                           ▼                                │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    AppState                                      │   │
│  │  ┌──────────────────┐  ┌──────────────────┐                     │   │
│  │  │ workers:         │  │ logs:            │                     │   │
│  │  │ HashMap<WorkerId,│  │ AggregateLog     │                     │   │
│  │  │ WorkerStatusFile>│  │ Buffer           │                     │   │
│  │  └──────────────────┘  └──────────────────┘                     │   │
│  │  ┌──────────────────┐  ┌──────────────────┐                     │   │
│  │  │ pool_stats:      │  │ costs:           │                     │   │
│  │  │ WorkerPoolStats  │  │ CostSummary      │                     │   │
│  │  └──────────────────┘  └──────────────────┘                     │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│             │                                                            │
│             ▼ (render on each frame)                                    │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    UI Widgets                                    │   │
│  │  WorkerPanel  TaskPanel  CostPanel  LogPanel  MetricsPanel      │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────────────────────┘
          │                    │
          ▼                    ▼
   ~/.forge/status/      ~/.forge/logs/
   ├── worker-1.json     ├── worker-1.log
   ├── worker-2.json     ├── worker-2.log
   └── worker-N.json     └── worker-N.log
```

### 5.2 Event Flow

```
1. Worker writes/updates ~/.forge/status/{id}.json
           │
           ▼
2. inotify/debouncer detects change
           │
           ▼
3. StatusWatcher.process_event() parses JSON
           │
           ▼
4. StatusEvent::WorkerUpdated sent via channel
           │
           ▼
5. App.handle_status_event() updates workers HashMap
           │
           ▼
6. App.recompute_stats() updates pool_stats
           │
           ▼
7. Next frame: draw_overview() renders new data
```

### 5.3 Proposed App State Extension

```rust
/// Extended App state with real data integration
pub struct App {
    // Existing fields...
    current_view: View,
    focus_panel: FocusPanel,
    should_quit: bool,

    // NEW: Real data sources
    status_watcher: Option<StatusWatcher>,
    log_manager: Option<AggregateLogBuffer>,

    // NEW: Derived state (updated on events)
    workers: HashMap<String, WorkerStatusFile>,
    pool_stats: WorkerPoolStats,

    // NEW: Cost tracking (future)
    cost_summary: CostSummary,
}
```

---

## 6. Display Rendering

### 6.1 Worker Pool Panel (Overview)

**Current** (hardcoded):
```
Total: 24 (18 active, 6 idle)
Unhealthy: 0

GLM-4.7:   8 active, 3 idle
Sonnet:    6 active, 2 idle
Opus:      3 active, 1 idle
Haiku:     1 active, 0 idle
```

**New** (from `WorkerPoolStats`):
```rust
fn format_worker_pool(&self) -> String {
    let stats = &self.pool_stats;

    format!(
        "Total: {} ({} active, {} idle)\n\
         Unhealthy: {}\n\n\
         {}",
        stats.total,
        stats.active,
        stats.idle,
        stats.unhealthy_count,
        stats.by_model.iter()
            .map(|(model, count)| format!("{}: {} workers", model, count))
            .collect::<Vec<_>>()
            .join("\n")
    )
}
```

### 6.2 Worker Table (Workers View)

**New** (from `self.workers`):
```rust
fn format_worker_table(&self) -> String {
    let mut rows = vec![];

    for (id, status) in &self.workers {
        rows.push(format!(
            "│ {:15} │ {:8} │ {:8} │ {:11} │",
            truncate(id, 15),
            truncate(&status.model, 8),
            status.status,
            status.current_task.as_deref().unwrap_or("-"),
        ));
    }

    format!(
        "┌─────────────────┬──────────┬──────────┬─────────────┐\n\
         │ Worker ID       │ Model    │ Status   │ Task        │\n\
         ├─────────────────┼──────────┼──────────┼─────────────┤\n\
         {}\n\
         └─────────────────┴──────────┴──────────┴─────────────┘",
        rows.join("\n")
    )
}
```

### 6.3 Activity Log (Logs View)

**New** (from `AggregateLogBuffer`):
```rust
fn format_activity_log(&self) -> String {
    if let Some(log_manager) = &self.log_manager {
        log_manager.all()
            .last_n(20)
            .map(|entry| entry.format_display())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        "(No log data available)".to_string()
    }
}
```

---

## 7. Initialization Flow

### 7.1 App Startup Sequence

```rust
impl App {
    pub fn new() -> Self {
        // ... existing init ...

        // Initialize status watcher
        let status_watcher = match StatusWatcher::new_default() {
            Ok(watcher) => {
                info!("Status watcher initialized");
                Some(watcher)
            }
            Err(e) => {
                warn!("Failed to init status watcher: {}", e);
                None
            }
        };

        // Initial worker state from watcher
        let workers = status_watcher
            .as_ref()
            .map(|w| w.workers().clone())
            .unwrap_or_default();

        let pool_stats = Self::compute_pool_stats(&workers);

        Self {
            // ...
            status_watcher,
            workers,
            pool_stats,
            log_manager: Some(AggregateLogBuffer::new(1000)),
            cost_summary: CostSummary::default(),
        }
    }
}
```

### 7.2 Event Loop Integration

```rust
fn run_loop(&mut self, terminal: &mut Terminal<...>) -> AppResult<()> {
    while !self.should_quit {
        // 1. Poll for status events (non-blocking)
        if let Some(watcher) = &mut self.status_watcher {
            while let Some(event) = watcher.try_recv() {
                self.handle_status_event(event);
            }
        }

        // 2. Draw UI
        terminal.draw(|frame| self.draw(frame))?;

        // 3. Handle keyboard input
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                self.handle_key_event(key);
            }
        }
    }
    Ok(())
}

fn handle_status_event(&mut self, event: StatusEvent) {
    match event {
        StatusEvent::WorkerUpdated { worker_id, status } => {
            self.workers.insert(worker_id, status);
            self.pool_stats = Self::compute_pool_stats(&self.workers);
        }
        StatusEvent::WorkerRemoved { worker_id } => {
            self.workers.remove(&worker_id);
            self.pool_stats = Self::compute_pool_stats(&self.workers);
        }
        StatusEvent::InitialScanComplete { workers } => {
            self.workers = workers;
            self.pool_stats = Self::compute_pool_stats(&self.workers);
        }
        StatusEvent::Error { path, error } => {
            warn!("Status file error: {:?} - {}", path, error);
        }
    }
}
```

---

## 8. Workers Writing Status Files

### 8.1 Status Update Protocol

Workers MUST update their status file:

1. **On startup**: Create file with `status: starting`
2. **On ready**: Update to `status: idle`
3. **On task start**: Update `status: active`, `current_task: <bead_id>`
4. **On heartbeat**: Update `last_activity` timestamp (every 30s recommended)
5. **On task complete**: Update `status: idle`, `current_task: null`, increment `tasks_completed`
6. **On shutdown**: Delete status file or update `status: stopped`
7. **On error**: Update `status: failed` (leave file for debugging)

### 8.2 Worker Implementation Example

```bash
#!/bin/bash
# Worker status update helper

WORKER_ID="${1:-worker-$$}"
STATUS_DIR="${HOME}/.forge/status"
STATUS_FILE="${STATUS_DIR}/${WORKER_ID}.json"

mkdir -p "$STATUS_DIR"

write_status() {
    local status="$1"
    local task="${2:-null}"

    cat > "$STATUS_FILE" << EOF
{
  "worker_id": "${WORKER_ID}",
  "status": "${status}",
  "model": "${FORGE_MODEL:-unknown}",
  "workspace": "$(pwd)",
  "pid": $$,
  "started_at": "${STARTED_AT}",
  "last_activity": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "current_task": ${task},
  "tasks_completed": ${TASKS_COMPLETED:-0}
}
EOF
}

# On startup
STARTED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
write_status "starting"

# ... worker logic ...

# On ready
write_status "idle"

# On task start
write_status "active" '"fg-29n"'

# On task complete
TASKS_COMPLETED=$((TASKS_COMPLETED + 1))
write_status "idle"

# Cleanup on exit
trap 'rm -f "$STATUS_FILE"' EXIT
```

---

## 9. Graceful Degradation

### 9.1 When StatusWatcher Fails

If `StatusWatcher::new()` fails:
- Display a warning banner in the TUI
- Show placeholder text: "(Status monitoring unavailable)"
- Allow manual refresh via `r` key (re-try initialization)

### 9.2 When No Workers Exist

If `~/.forge/status/` is empty:
- Show helpful message: "No active workers. Use `spawn-workers.sh` to start."
- Display sample status format for debugging

### 9.3 When Status Files Are Corrupt

If JSON parsing fails:
- Mark worker as `status: error`
- Include error in `StatusEvent::Error`
- Show error indicator in worker list

---

## 10. Testing Strategy

### 10.1 Unit Tests

- `WorkerStatusFile::from_json()` with valid/invalid/minimal JSON
- `WorkerPoolStats` computation with various worker combinations
- `format_worker_table()` output validation

### 10.2 Integration Tests

- Create temp status directory, verify `StatusWatcher` picks up files
- Write status file, verify event received
- Delete status file, verify removal event

### 10.3 TUI Rendering Tests

- Verify `draw_overview()` shows real worker counts
- Verify `draw_workers()` shows actual worker table
- Test layout adaptation with 0, 1, 10, 100 workers

---

## 11. Migration Path

### Phase 1: Read Real Status (This Bead)
1. Integrate `StatusWatcher` into `App::new()`
2. Replace mock data in `draw_overview()` with `pool_stats`
3. Replace mock table in `draw_workers()` with real workers
4. Add graceful degradation for missing status dir

### Phase 2: Log Integration
1. Initialize `LogTailer` for each discovered worker
2. Replace mock logs in `draw_logs()` with real entries
3. Add log level filtering and source filtering

### Phase 3: Cost Tracking
1. Parse log entries for API call costs
2. Aggregate costs by worker/model/time period
3. Replace mock costs in `draw_costs()` with real data

### Phase 4: Task Queue Integration
1. Integrate with `br` CLI via JSONL watching
2. Replace mock task queue with real beads
3. Add task assignment controls

---

## 12. Open Questions

1. **Status file atomicity**: Should we use atomic write (write to temp, rename)?
   - Recommendation: Yes, prevent partial reads

2. **Stale status detection**: How long before a status file is considered stale?
   - Recommendation: 5 minutes without `last_activity` update

3. **Log file multiplexing**: Should logs from all workers go to one file or separate?
   - Recommendation: Separate files for easier tailing, aggregate in UI

4. **Hot reload configuration**: Should config changes require restart?
   - Recommendation: Watch `~/.forge/config.yaml` for live updates

---

## References

- ADR 0008: Real-Time Update Architecture
- ADR 0006: Technology Stack Selection (Rust/Ratatui)
- `forge-tui/src/status.rs` - StatusWatcher implementation
- `forge-tui/src/log.rs` - LogTailer implementation
