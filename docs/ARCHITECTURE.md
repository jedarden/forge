# FORGE Architecture

FORGE (Forge Orchestration Runtime for Generative Executors) is a terminal-based dashboard for managing AI worker agents. This document describes the system architecture, module structure, data flow, and key design decisions.

## Table of Contents

1. [System Overview](#system-overview)
2. [Module Structure](#module-structure)
3. [Crate Dependency Graph](#crate-dependency-graph)
4. [Data Flow](#data-flow)
5. [TUI Rendering Pipeline](#tui-rendering-pipeline)
6. [Chat Backend Design](#chat-backend-design)
7. [Worker Management](#worker-management)
8. [Cost Tracking](#cost-tracking)
9. [Beads Integration](#beads-integration)
10. [Key Design Decisions](#key-design-decisions)

---

## System Overview

FORGE provides a unified control panel for AI coding agents. It monitors workers running in tmux sessions, tracks API costs, manages task queues via the beads issue tracking system, and offers a conversational chat interface for dashboard operations.

```
                    ┌─────────────────────────────────────────────────────────┐
                    │                      FORGE TUI                          │
                    │  ┌───────────┬───────────┬───────────┬───────────────┐  │
                    │  │ Overview  │  Workers  │   Tasks   │    Costs      │  │
                    │  │  Panel    │   Panel   │   Panel   │    Panel      │  │
                    │  └───────────┴───────────┴───────────┴───────────────┘  │
                    │  ┌─────────────────────────────────────────────────────┐  │
                    │  │                  Chat Interface                     │  │
                    │  └─────────────────────────────────────────────────────┘  │
                    └─────────────────────────────────────────────────────────┘
                                              │
              ┌───────────────────────────────┼───────────────────────────────┐
              │                               │                               │
              ▼                               ▼                               ▼
    ┌─────────────────┐           ┌─────────────────┐           ┌─────────────────┐
    │  forge-worker   │           │   forge-chat    │           │   forge-cost    │
    │  (tmux mgmt)    │           │  (AI backend)   │           │ (cost tracking) │
    └────────┬────────┘           └────────┬────────┘           └────────┬────────┘
             │                             │                             │
             ▼                             ▼                             ▼
    ┌─────────────────┐           ┌─────────────────┐           ┌─────────────────┐
    │  tmux sessions  │           │  Claude API /   │           │    SQLite DB    │
    │  (workers)      │           │  Claude CLI     │           │  (~/.forge/)    │
    └─────────────────┘           └─────────────────┘           └─────────────────┘
```

### Core Capabilities

- **Worker Monitoring**: Real-time status updates from worker agents via JSON status files
- **tmux Discovery**: Automatic discovery of worker sessions running in tmux
- **Cost Analytics**: Token usage tracking with model-specific pricing and projections
- **Task Queue**: Integration with beads issue tracking for agent task management
- **Chat Interface**: Natural language commands via Claude API or CLI
- **Multi-View Dashboard**: Hotkey-driven navigation between specialized views

---

## Module Structure

FORGE is organized as a Cargo workspace with seven crates:

```
forge/
├── src/
│   └── main.rs              # Entry point, CLI parsing, onboarding
├── crates/
│   ├── forge-core/          # Shared types, errors, logging, file watching
│   ├── forge-config/        # Configuration file handling (planned)
│   ├── forge-tui/           # Terminal UI with Ratatui
│   ├── forge-chat/          # AI chat backend (Claude API/CLI)
│   ├── forge-worker/        # Worker process management
│   ├── forge-cost/          # Cost tracking and analytics
│   └── forge-init/          # Onboarding and tool detection
└── docs/
    └── ARCHITECTURE.md      # This file
```

### Crate Responsibilities

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `forge-core` | Shared infrastructure | `ForgeError`, `StatusWatcher`, `LogGuard` |
| `forge-tui` | Terminal user interface | `App`, `DataManager`, `View`, `Theme` |
| `forge-chat` | AI conversational backend | `ChatBackend`, `ChatProvider`, `ToolRegistry` |
| `forge-worker` | Worker lifecycle management | `WorkerLauncher`, `DiscoveredWorker` |
| `forge-cost` | Cost tracking and queries | `CostDatabase`, `LogParser`, `CostQuery` |
| `forge-init` | First-run setup | `CliToolDetection`, `ConfigGenerator` |

---

## Crate Dependency Graph

```
                    ┌──────────────┐
                    │    forge     │  (binary crate)
                    │   main.rs    │
                    └──────┬───────┘
                           │
         ┌─────────────────┼─────────────────┐
         │                 │                 │
         ▼                 ▼                 ▼
   ┌───────────┐    ┌───────────┐    ┌───────────┐
   │forge-core │    │ forge-tui │    │forge-init │
   │           │◄───│           │    │           │
   └───────────┘    └─────┬─────┘    └───────────┘
         ▲                │
         │     ┌──────────┼──────────┐
         │     │          │          │
         │     ▼          ▼          ▼
         │ ┌───────────┐ ┌───────────┐ ┌───────────┐
         │ │forge-chat │ │forge-cost │ │forge-worker│
         └─┤           │ │           │ │            │
           └───────────┘ └───────────┘ └────────────┘
```

**Dependency Rules:**
- `forge-core` is the foundation with no internal dependencies
- `forge-tui` depends on all other crates (integration layer)
- Worker, chat, and cost crates are independent of each other
- All crates share workspace dependencies for consistency

---

## Data Flow

### Status Update Flow

```
┌─────────────────┐     write JSON      ┌─────────────────┐
│  Worker Agent   │ ───────────────────►│ ~/.forge/status/│
│ (Claude Code)   │                     │  worker-1.json  │
└─────────────────┘                     └────────┬────────┘
                                                 │
                                          inotify watch
                                                 │
                                                 ▼
                                        ┌─────────────────┐
                                        │  StatusWatcher  │
                                        │ (forge-core)    │
                                        └────────┬────────┘
                                                 │
                                          mpsc channel
                                                 │
                                                 ▼
                                        ┌─────────────────┐
                                        │   DataManager   │
                                        │  (forge-tui)    │
                                        └────────┬────────┘
                                                 │
                                           render()
                                                 │
                                                 ▼
                                        ┌─────────────────┐
                                        │    Terminal     │
                                        │    Display      │
                                        └─────────────────┘
```

### Event Loop Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                        Main Event Loop                          │
│                                                                 │
│  1. Poll crossterm events (keyboard, resize) - 50ms timeout    │
│  2. Poll StatusWatcher for file change events                  │
│  3. Poll BeadManager for task queue updates                    │
│  4. Poll async chat responses (non-blocking)                   │
│  5. Check if redraw needed (dirty flag)                        │
│  6. Render frame if dirty                                      │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## TUI Rendering Pipeline

The TUI uses Ratatui with a multi-view architecture. Each view has dedicated rendering logic.

### View Architecture

```rust
pub enum View {
    Overview,   // Dashboard summary of all components
    Workers,    // Worker pool management
    Tasks,      // Bead queue visualization
    Costs,      // Cost analytics and budgets
    Metrics,    // Performance statistics
    Logs,       // Activity log viewer
    Chat,       // Conversational interface
}
```

### Layout Modes

The TUI adapts to terminal dimensions:

| Mode | Width | Layout |
|------|-------|--------|
| UltraWide | 199+ cols | 3-column, 6 panels |
| Wide | 120-198 cols | 2-column, 4 panels |
| Narrow | <120 cols | Single-view mode |

### Rendering Pipeline

```
┌─────────────────────────────────────────────────────────────────┐
│                      App::render_frame()                        │
│                                                                 │
│  1. Get terminal size (crossterm::terminal::size)               │
│  2. Determine LayoutMode from width                             │
│  3. Create root layout (header, body, footer)                   │
│  4. Match current View:                                         │
│     - Overview: render_overview_view()                          │
│     - Workers: render_workers_view()                            │
│     - Tasks: render_tasks_view()                                │
│     - etc.                                                      │
│  5. Render status bar with hotkey hints                         │
│  6. If chat mode: render chat overlay                           │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Widget Library

Custom widgets in `forge-tui/src/widget.rs`:

- `SparklineWidget`: Mini bar charts for metrics
- `ProgressBar`: Configurable progress indicators
- `HotkeyHints`: Styled hotkey display
- `QuickActionsPanel`: Action buttons panel
- `StatusIndicator`: Worker status icons

### Theme System

Themes are defined in `forge-tui/src/theme.rs`:

```rust
pub enum ThemeName {
    Dark,        // Default dark theme
    Light,       // Light background
    Cyberpunk,   // Neon colors
    Forest,      // Green tones
    Ocean,       // Blue palette
}
```

Themes provide consistent `ThemeColors` for:
- Primary/secondary colors
- Status colors (success, warning, error)
- Border and background colors
- Text and accent colors

---

## Chat Backend Design

The chat system provides AI-powered dashboard control via natural language.

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        ChatBackend                              │
│  ┌───────────────┐ ┌───────────────┐ ┌───────────────────────┐  │
│  │  RateLimiter  │ │  AuditLogger  │ │    ContextProvider    │  │
│  │ (10 cmd/min)  │ │   (JSONL)     │ │   (dashboard state)   │  │
│  └───────────────┘ └───────────────┘ └───────────────────────┘  │
│                                                                 │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                     ToolRegistry                          │  │
│  │  worker_status | task_queue | cost_analytics | spawn_...  │  │
│  └───────────────────────────────────────────────────────────┘  │
│                              │                                  │
│                              ▼                                  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                   ChatProvider trait                      │  │
│  │  ┌─────────────┐ ┌──────────────┐ ┌────────────────────┐  │  │
│  │  │ ClaudeApi   │ │  ClaudeCli   │ │   MockProvider     │  │  │
│  │  │ (HTTP API)  │ │ (subprocess) │ │    (testing)       │  │  │
│  │  └─────────────┘ └──────────────┘ └────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### Provider Trait

```rust
#[async_trait]
pub trait ChatProvider: Send + Sync {
    async fn process(
        &self,
        prompt: &str,
        context: &DashboardContext,
        tools: &[ProviderTool],
    ) -> Result<ProviderResponse>;

    fn name(&self) -> &str;
    fn model(&self) -> &str;
    fn supports_streaming(&self) -> bool;
}
```

### Tool Categories

**Read-Only Tools** (no confirmation required):
- `worker_status` - Get current worker pool state
- `task_queue` - Get ready beads/tasks
- `cost_analytics` - Get spending data
- `subscription_usage` - Get quota tracking

**Action Tools** (require confirmation):
- `spawn_worker` - Spawn new workers
- `kill_worker` - Kill a worker
- `assign_task` - Reassign task to different model

### Async-to-Sync Bridge

The chat backend runs asynchronously while the TUI is synchronous. This is bridged via:

1. **Tokio Runtime**: Single-threaded runtime created during `App::new()`
2. **Message Channels**: `mpsc` channels for request/response flow
3. **Non-Blocking Polling**: UI polls for responses each frame

```rust
// Submit request (non-blocking)
chat_tx.send(request)?;
chat_pending = true;

// Each frame, check for response
if let Ok(response) = response_rx.try_recv() {
    chat_pending = false;
    chat_history.push(response);
}
```

---

## Worker Management

Workers are AI coding agents running in tmux sessions.

### Worker Lifecycle

```
┌─────────────────────────────────────────────────────────────────┐
│                     WorkerLauncher                              │
│                                                                 │
│  spawn() ─────────────────────────────────────────────────►     │
│    │                                                            │
│    ├─ 1. Validate launcher script exists                        │
│    │                                                            │
│    ├─ 2. Kill existing session if present                       │
│    │                                                            │
│    ├─ 3. Execute launcher with environment:                     │
│    │      FORGE_WORKER_ID, FORGE_SESSION, FORGE_MODEL           │
│    │                                                            │
│    ├─ 4. Parse JSON output from launcher                        │
│    │      { "pid": 12345, "session": "forge-worker-1" }         │
│    │                                                            │
│    ├─ 5. Verify tmux session was created                        │
│    │                                                            │
│    └─ 6. Return WorkerHandle                                    │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Session Discovery

The discovery module finds existing worker sessions:

```rust
pub async fn discover_workers() -> Result<DiscoveryResult> {
    // Query tmux: list-sessions -F '#{session_name}:#{session_created}:...'
    // Parse known patterns: claude-code-glm-47-*, claude-code-opus-*, etc.
    // Return structured worker information
}
```

**Session Naming Convention:**
- `claude-code-glm-47-alpha` → GLM-4.7 model
- `claude-code-sonnet-bravo` → Sonnet 4.5
- `claude-code-opus-charlie` → Opus 4.5/4.6
- `opencode-glm-47-delta` → OpenCode with GLM

### Worker Types

```rust
pub enum WorkerType {
    Glm47,    // GLM-4.7 via z.ai proxy
    Sonnet,   // Claude Sonnet 4.5
    Opus,     // Claude Opus 4.5/4.6
    Haiku,    // Claude Haiku 4.5
    Unknown,  // Unrecognized pattern
}
```

### Bead-Aware Workers

Workers can be launched with bead assignments:

```bash
# Launcher receives --bead-ref flag
./launcher.sh --model=sonnet --workspace=/project --bead-ref=fg-123
```

The launcher output includes bead information:
```json
{
    "pid": 12345,
    "session": "forge-worker-1",
    "model": "sonnet",
    "bead_id": "fg-123",
    "bead_title": "Implement feature X"
}
```

---

## Cost Tracking

FORGE tracks API costs from worker log files and provides analytics.

### Database Schema

```sql
-- Core API call tracking
CREATE TABLE api_calls (
    id INTEGER PRIMARY KEY,
    timestamp TEXT NOT NULL,
    worker_id TEXT NOT NULL,
    session_id TEXT,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_creation_tokens INTEGER DEFAULT 0,
    cache_read_tokens INTEGER DEFAULT 0,
    cost_usd REAL NOT NULL,
    bead_id TEXT,
    event_type TEXT DEFAULT 'result'
);

-- Aggregation tables for efficient queries
CREATE TABLE daily_costs (...);
CREATE TABLE model_costs (...);
CREATE TABLE hourly_stats (...);
CREATE TABLE daily_stats (...);
CREATE TABLE worker_efficiency (...);
CREATE TABLE model_performance (...);

-- Subscription tracking
CREATE TABLE subscriptions (...);
CREATE TABLE subscription_usage (...);
```

### Log Parsing

The `LogParser` extracts usage from worker JSON logs:

```rust
// Supported formats:
// - Anthropic: input_tokens, output_tokens, cache_*_tokens
// - OpenAI: prompt_tokens, completion_tokens
// - GLM (z.ai): Anthropic-compatible with modelUsage

let parser = LogParser::new();
let calls = parser.parse_directory("~/.forge/logs")?;
db.insert_api_calls(&calls)?;
```

### Pricing Configuration

```rust
// Model pricing (USD per million tokens)
pricing.insert("claude-opus", ModelPricing::new(15.0, 75.0));
pricing.insert("claude-sonnet", ModelPricing::new(3.0, 15.0));
pricing.insert("claude-haiku", ModelPricing::new(0.80, 4.0));
pricing.insert("glm-4.7", ModelPricing::new(1.0, 2.0));
```

### Query API

```rust
let query = CostQuery::new(&db);

// Today's costs
let today = query.get_today_costs()?;

// Monthly breakdown by model
let monthly = query.get_current_month_costs()?;

// Projections
let projected = query.get_projected_costs(None)?;
```

---

## Beads Integration

FORGE integrates with the beads issue tracking system for task management.

### BeadManager Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        BeadManager                              │
│                                                                 │
│  ┌─────────────────┐                                            │
│  │   Workspaces    │  FORGE_WORKSPACES or auto-detected        │
│  │  [/path/to/ws1] │                                            │
│  │  [/path/to/ws2] │                                            │
│  └────────┬────────┘                                            │
│           │                                                     │
│           │  poll_updates() every 30s                           │
│           │                                                     │
│           ▼                                                     │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │              br CLI queries (with timeout)                 │ │
│  │  br ready --format json                                    │ │
│  │  br blocked --format json                                  │ │
│  │  br list --status in_progress --format json                │ │
│  │  br stats --format json                                    │ │
│  └────────────────────────────────────────────────────────────┘ │
│                                                                 │
│  ┌─────────────────┐                                            │
│  │ WorkspaceBeads  │  Cached per workspace                     │
│  │   .ready[]      │                                            │
│  │   .blocked[]    │                                            │
│  │   .in_progress[]│                                            │
│  │   .stats        │                                            │
│  └─────────────────┘                                            │
└─────────────────────────────────────────────────────────────────┘
```

### Bead Data Model

```rust
pub struct Bead {
    pub id: String,           // e.g., "fg-1r1"
    pub title: String,
    pub description: String,
    pub status: String,       // open, in_progress, closed, blocked, deferred
    pub priority: u8,         // 0-4 (0 = critical)
    pub issue_type: String,   // task, bug, feature
    pub assignee: Option<String>,
    pub labels: Vec<String>,
    pub dependency_count: usize,
    pub dependent_count: usize,
}
```

### Integration Points

1. **Task Queue Panel**: Shows ready/blocked/in-progress beads
2. **Worker Assignment**: Launch workers with `--bead-ref`
3. **Cost Attribution**: Track costs per bead ID
4. **Chat Commands**: Query and manage beads via chat

---

## Key Design Decisions

### 1. Synchronous TUI with Async Backend

**Decision**: Keep the TUI event loop synchronous, use message passing for async operations.

**Rationale**:
- Ratatui is synchronous; forcing async would complicate rendering
- Non-blocking polling allows responsive UI during API calls
- Single-threaded tokio runtime avoids threading complexity

### 2. File-Based Worker Status

**Decision**: Workers write JSON status files to `~/.forge/status/`.

**Rationale**:
- Language-agnostic (workers can be Python, Go, etc.)
- Survives worker crashes (last status preserved)
- Simple debugging (cat the file)
- Works with any IPC boundary

### 3. Pluggable Chat Providers

**Decision**: Abstract chat providers behind a trait.

**Rationale**:
- Support both API and CLI modes
- Easy mock provider for testing
- Future: add OpenAI, local models

### 4. SQLite for Cost Tracking

**Decision**: Use SQLite with aggregation tables.

**Rationale**:
- No external dependencies
- Pre-computed aggregations for fast queries
- Schema migrations built-in
- Portable (single file)

### 5. Beads as External CLI

**Decision**: Shell out to `br` CLI instead of embedding library.

**Rationale**:
- Beads has its own release cycle
- Reduces coupling
- Users can use br independently
- Timeout protection prevents UI blocking

### 6. Responsive Layout System

**Decision**: Three layout modes based on terminal width.

**Rationale**:
- Works on laptops and ultrawide monitors
- Graceful degradation to single-view on small terminals
- Same codebase, adaptive rendering

### 7. Hotkey-Driven Navigation

**Decision**: Single-key hotkeys for all major actions.

**Rationale**:
- Fast navigation for power users
- Discoverable (displayed in status bar)
- Vim-inspired muscle memory (j/k, :command)

---

## Performance Considerations

### Startup Time Target: < 2 seconds

Achieved through:
- Lazy loading of bead data (polled after first render)
- Async tmux discovery (500ms timeout)
- Minimal blocking in App::new()

### Memory Efficiency

- Ring buffer for activity log (last N events)
- LRU-style caching for cost queries
- Status files parsed on-demand

### UI Responsiveness

- 50ms poll timeout for input events
- Non-blocking chat API calls
- 2s timeout for br CLI commands
- Dirty flag to avoid unnecessary redraws

---

## Future Architecture Considerations

1. **Streaming Chat**: Add SSE/WebSocket support for streaming responses
2. **Multi-Cluster**: Support remote worker management via kubectl proxy
3. **Plugin System**: User-defined views and widgets
4. **Metrics Export**: Prometheus/OpenTelemetry integration
5. **Session Recording**: Replay worker sessions for debugging

---

## Related Documentation

- [Chat Backend Architecture](./CHAT_BACKEND.md) - Detailed chat system design
- [Worker Protocol](./WORKER_PROTOCOL.md) - Launcher script specification
- [Beads Integration](./BEADS_INTEGRATION.md) - Task queue details

---

*Last updated: 2026-02-11*
