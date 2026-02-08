# ADR 0004: Tool-Based Conversational Interface

**Date**: 2026-02-07
**Status**: Accepted
**Deciders**: Jed Arden, Claude Sonnet 4.5

---

## Context

FORGE needs two distinct types of LLM integration:

1. **Chat Backend (CLI Worker)**: Headless LLM that receives prompts from FORGE and returns tool calls for FORGE to execute
   - User-driven: translates natural language commands to tool calls
   - Autonomous: analyzes telemetry and recommends actions

2. **Bead Workers**: Autonomous coding agents spawned in tmux sessions to work on beads in workspaces
   - Long-running processes
   - Work independently on tasks
   - FORGE monitors but doesn't control them directly

This ADR focuses on **#1: Chat Backend** - the decision-making LLM that helps control FORGE itself.

**Problems with hotkey-only approach**:
- Users must memorize dozens of shortcuts
- Not discoverable (requires help menu)
- Limited expressiveness (simple actions only)
- Difficult to combine multiple actions
- Poor accessibility

**Opportunity**: Use a headless LLM backend as FORGE's "brain" for both user commands and autonomous decisions.

---

## Decision

Use **tool-based conversational interface** where a headless LLM (Chat Backend) receives prompts from FORGE and returns structured tool calls for FORGE to execute.

**Two Modes of Operation**:

### 1. User-Driven Mode (Interactive)
User types natural language → Chat backend translates to tool calls → FORGE executes

```
User types: "Show me costs for the last week"
    ↓
FORGE → Chat Backend (stdin): {"message": "Show me costs...", "tools": [...]}
    ↓
Chat Backend → FORGE (stdout): {"tool_calls": [{"tool": "show_costs", "args": {"period": "last_week"}}]}
    ↓
FORGE executes: Switch to cost view, filter to 7 days
    ↓
User sees: Cost dashboard filtered to last week
```

### 2. Autonomous Mode (Telemetry-Driven)
FORGE detects event → Sends telemetry to backend → Backend recommends action → FORGE executes

```
FORGE detects: 3 claude-code-opus workers failing with rate limit errors
    ↓
FORGE → Chat Backend (stdin): {
  "message": "Detected: 3 claude-code-opus workers failing with rate_limit_error",
  "context": {
    "failing_workers": ["opus-alpha", "opus-beta", "opus-gamma"],
    "error_pattern": "rate_limit_error",
    "other_opus_workers": ["opus-delta", "opus-epsilon"]
  },
  "tools": [...]
}
    ↓
Chat Backend analyzes → Recommends action
    ↓
Chat Backend → FORGE (stdout): {
  "tool_calls": [
    {"tool": "kill_worker", "args": {"worker_id": "opus-delta"}},
    {"tool": "kill_worker", "args": {"worker_id": "opus-epsilon"}}
  ],
  "reasoning": "Proactively shutting down remaining opus workers to avoid rate limit"
}
    ↓
FORGE shows confirmation → User approves → Workers shut down
```

**Hotkeys remain available as optional shortcuts** for power users.

---

## Design

### Tool Categories

**1. View Control Tools**
```python
switch_view(view: str)
# Examples: "workers", "tasks", "costs", "metrics", "logs"
# User: "Show me the worker status"
# Tool: switch_view("workers")

split_view(left: str, right: str)
# User: "Show workers on left and tasks on right"
# Tool: split_view("workers", "tasks")

focus_panel(panel: str)
# User: "Focus on the activity log"
# Tool: focus_panel("activity_log")
```

**2. Worker Management Tools**
```python
spawn_worker(model: str, count: int, workspace: str = None)
# User: "Spawn 3 sonnet workers"
# Tool: spawn_worker("sonnet", 3)

kill_worker(worker_id: str)
# User: "Kill worker sonnet-alpha"
# Tool: kill_worker("sonnet-alpha")

list_workers(filter: str = None)
# User: "Show me all idle workers"
# Tool: list_workers(filter="idle")

restart_worker(worker_id: str)
# User: "Restart the failed worker"
# Tool: restart_worker("sonnet-beta")
```

**3. Task Management Tools**
```python
filter_tasks(priority: str = None, status: str = None)
# User: "Show only P0 tasks"
# Tool: filter_tasks(priority="P0")

assign_task(task_id: str, worker_id: str = None)
# User: "Assign bd-abc to the best available worker"
# Tool: assign_task("bd-abc", worker_id="auto")

create_task(title: str, priority: str, description: str = None)
# User: "Create a P1 task to fix the login bug"
# Tool: create_task("Fix login bug", "P1", "Users can't log in...")
```

**4. Cost & Analytics Tools**
```python
show_costs(period: str = "today", breakdown: str = None)
# User: "What did I spend this month?"
# Tool: show_costs(period="month", breakdown="by_model")

optimize_routing()
# User: "Optimize my cost routing"
# Tool: optimize_routing()

forecast_costs(days: int = 30)
# User: "What will I spend next month?"
# Tool: forecast_costs(days=30)
```

**5. Data Export Tools**
```python
export_logs(format: str = "json", period: str = "today")
# User: "Export today's logs as CSV"
# Tool: export_logs(format="csv", period="today")

export_metrics(metric_type: str, format: str = "json")
# User: "Export performance metrics"
# Tool: export_metrics("performance", "json")

screenshot(panel: str = "all")
# User: "Take a screenshot of the dashboard"
# Tool: screenshot("all")
```

**6. Configuration Tools**
```python
set_config(key: str, value: any)
# User: "Set the default model to sonnet"
# Tool: set_config("default_model", "sonnet")

get_config(key: str = None)
# User: "What's my current config?"
# Tool: get_config()

save_layout(name: str)
# User: "Save this layout as 'monitoring'"
# Tool: save_layout("monitoring")

load_layout(name: str)
# User: "Load my monitoring layout"
# Tool: load_layout("monitoring")
```

**7. Help & Discovery Tools**
```python
help(topic: str = None)
# User: "How do I spawn workers?"
# Tool: help("spawn_workers")

search_docs(query: str)
# User: "How does cost optimization work?"
# Tool: search_docs("cost optimization")

list_capabilities()
# User: "What can you do?"
# Tool: list_capabilities()
```

---

## Implementation Architecture

### Component Structure

```
┌──────────────────────────────────────────────────────┐
│                  FORGE TUI                           │
│  ┌────────────────────────────────────────────────┐  │
│  │  Two Input Sources:                            │  │
│  │  1. User types: "reconfigure max glm to 5"    │  │
│  │  2. Telemetry: "3 opus workers failing"       │  │
│  └───────────────────┬────────────────────────────┘  │
│                      │                                │
│                      ↓                                │
│  ┌────────────────────────────────────────────────┐  │
│  │  Chat Backend Interface (stdin/stdout)         │  │
│  │  - Prepare prompt with context + tools         │  │
│  │  - Send to headless CLI                        │  │
│  │  - Receive tool calls                          │  │
│  └───────────────────┬────────────────────────────┘  │
└────────────────────────│──────────────────────────────┘
                         │
                         ↓
┌──────────────────────────────────────────────────────┐
│  Chat Backend (Headless CLI - claude-code/aider/etc) │
│  - Receives: {"message": "...", "context": {...},    │
│              "tools": [{tool definitions}]}          │
│  - Returns: {"tool_calls": [{...}],                  │
│              "reasoning": "..."}                     │
└───────────────────┬──────────────────────────────────┘
                    │
                    ↓
┌──────────────────────────────────────────────────────┐
│              FORGE Tool Execution Engine              │
│  - Validate tool calls                               │
│  - Show confirmation if needed                       │
│  - Execute tools                                     │
│  - Return results to TUI                             │
└───────────────────┬──────────────────────────────────┘
                    │
                    ↓
┌──────────────────────────────────────────────────────┐
│              FORGE TUI Updates                        │
│  - Switch views (switch_view)                        │
│  - Spawn workers (spawn_worker)                      │
│  - Update config (set_config)                        │
│  - Kill workers (kill_worker)                        │
│  - Display results and status                        │
└──────────────────────────────────────────────────────┘
```

### Key Distinction: Bead Workers vs Chat Backend

```
┌─────────────────────────────────────────────────────┐
│  FORGE orchestrates TWO types of LLM usage:          │
├─────────────────────────────────────────────────────┤
│                                                      │
│  1. Chat Backend (Decision-Maker)                   │
│     - Single instance per FORGE                     │
│     - Receives prompts from FORGE                   │
│     - Returns tool calls to FORGE                   │
│     - FORGE executes the tools                      │
│     - Examples: claude-code, aider (in headless)    │
│                                                      │
│  2. Bead Workers (Coding Agents)                    │
│     - Multiple instances (2-10+)                    │
│     - Spawned in tmux sessions                      │
│     - Work autonomously on beads                    │
│     - FORGE monitors via logs/status files          │
│     - Examples: claude-code, aider (in workspace)   │
│                                                      │
└─────────────────────────────────────────────────────┘
```

## Autonomous CLI Worker Use Cases

### 1. Resource Optimization

**Scenario**: FORGE detects worker utilization patterns

```json
// FORGE → Chat Backend
{
  "message": "Analysis: 5 workers idle for >2h, task queue empty",
  "context": {
    "idle_workers": ["sonnet-alpha", "sonnet-beta", "haiku-gamma", "haiku-delta", "qwen-epsilon"],
    "idle_duration_hours": [2.5, 3.1, 4.0, 2.2, 5.5],
    "queue_size": 0,
    "queue_trend": "decreasing"
  },
  "tools": [...]
}

// Chat Backend → FORGE
{
  "tool_calls": [
    {"tool": "kill_worker", "args": {"worker_id": "qwen-epsilon"}},
    {"tool": "kill_worker", "args": {"worker_id": "haiku-delta"}},
    {"tool": "kill_worker", "args": {"worker_id": "haiku-gamma"}}
  ],
  "reasoning": "Queue empty and decreasing, recommend killing 3 longest-idle workers to save costs"
}
```

### 2. Cost Management

**Scenario**: FORGE detects spending approaching budget threshold

```json
// FORGE → Chat Backend
{
  "message": "Alert: Current spend $847/mo approaching budget limit $1000/mo",
  "context": {
    "current_monthly_spend": 847.32,
    "budget_limit": 1000,
    "days_remaining_in_month": 8,
    "projected_overage": 215.50,
    "active_workers": [
      {"id": "opus-alpha", "tier": "premium", "daily_cost": 15.20},
      {"id": "sonnet-beta", "tier": "standard", "daily_cost": 8.50},
      {"id": "haiku-gamma", "tier": "budget", "daily_cost": 2.10}
    ]
  },
  "tools": [...]
}

// Chat Backend → FORGE
{
  "tool_calls": [
    {"tool": "kill_worker", "args": {"worker_id": "opus-alpha"}},
    {"tool": "set_config", "args": {"key": "max_premium_workers", "value": 0}},
    {"tool": "show_notification", "args": {"message": "Budget protection: Disabled premium workers"}}
  ],
  "reasoning": "Projected to exceed budget by $215. Kill opus worker (most expensive) and prevent new premium spawns"
}
```

### 3. Task Routing Optimization

**Scenario**: FORGE analyzes bead queue and worker efficiency

```json
// FORGE → Chat Backend
{
  "message": "Analysis: 15 P0 beads queued, but only 2 premium workers active",
  "context": {
    "p0_beads": 15,
    "p1_beads": 8,
    "p2_beads": 22,
    "active_workers_by_tier": {
      "premium": 2,
      "standard": 5,
      "budget": 3
    },
    "avg_p0_completion_time_premium": "12min",
    "avg_p0_completion_time_standard": "35min",
    "subscription_status": {
      "claude_pro": {"active": true, "requests_remaining": "unlimited"},
      "openai_plus": {"active": false}
    }
  },
  "tools": [...]
}

// Chat Backend → FORGE
{
  "tool_calls": [
    {"tool": "spawn_worker", "args": {"model": "claude-sonnet-4.5", "count": 3}},
    {"tool": "filter_tasks", "args": {"priority": "P0", "status": "ready"}},
    {"tool": "show_notification", "args": {"message": "Spawned 3 sonnet workers for P0 queue (subscription cost: $0)"}}
  ],
  "reasoning": "P0 queue backlog with unlimited Claude Pro subscription. Spawn 3 more premium workers at zero marginal cost"
}
```

### 4. Error Pattern Detection

**Scenario**: FORGE detects repeated worker failures

```json
// FORGE → Chat Backend
{
  "message": "Pattern detected: 8 failures in last 10min, all glm-4.7 workers, error: connection_timeout",
  "context": {
    "error_pattern": {
      "error_type": "connection_timeout",
      "affected_model": "glm-4.7",
      "failure_count": 8,
      "time_window_minutes": 10,
      "affected_workers": ["glm-alpha", "glm-beta", "glm-gamma"]
    },
    "other_workers": {
      "claude-sonnet": {"status": "healthy", "count": 2},
      "claude-haiku": {"status": "healthy", "count": 3}
    },
    "glm_proxy_status": "unknown"
  },
  "tools": [...]
}

// Chat Backend → FORGE
{
  "tool_calls": [
    {"tool": "kill_worker", "args": {"worker_id": "glm-alpha"}},
    {"tool": "kill_worker", "args": {"worker_id": "glm-beta"}},
    {"tool": "kill_worker", "args": {"worker_id": "glm-gamma"}},
    {"tool": "set_config", "args": {"key": "disable_glm_spawning", "value": true, "duration_minutes": 30}},
    {"tool": "show_error", "args": {"message": "GLM proxy appears down. Disabled GLM workers for 30min. Check z.ai proxy status."}}
  ],
  "reasoning": "Systemic failure of all glm workers suggests proxy/API issue. Kill affected workers and temporarily disable GLM spawning to prevent waste"
}
```

### 5. Performance Optimization

**Scenario**: FORGE detects slow task completion rates

```json
// FORGE → Chat Backend
{
  "message": "Analysis: P0 beads taking 3x longer than expected with current worker mix",
  "context": {
    "p0_avg_completion_time": "45min",
    "p0_expected_completion_time": "15min",
    "active_workers": [
      {"model": "claude-haiku-4.5", "count": 5, "tier": "budget"},
      {"model": "claude-sonnet-4.5", "count": 1, "tier": "premium"}
    ],
    "recent_p0_beads": [
      {"id": "bd-abc", "completion_time": "52min", "worker": "haiku-gamma"},
      {"id": "bd-def", "completion_time": "38min", "worker": "haiku-delta"},
      {"id": "bd-ghi", "completion_time": "11min", "worker": "sonnet-alpha"}
    ]
  },
  "tools": [...]
}

// Chat Backend → FORGE
{
  "tool_calls": [
    {"tool": "kill_worker", "args": {"worker_id": "haiku-gamma"}},
    {"tool": "kill_worker", "args": {"worker_id": "haiku-delta"}},
    {"tool": "spawn_worker", "args": {"model": "claude-sonnet-4.5", "count": 2}},
    {"tool": "show_notification", "args": {"message": "Replaced 2 haiku workers with sonnet for better P0 throughput"}}
  ],
  "reasoning": "Haiku workers 4-5x slower on P0 tasks. Replace with sonnet to meet SLA. Cost increase justified by completion time"
}
```

### 6. Capacity Planning

**Scenario**: FORGE detects growing task queue

```json
// FORGE → Chat Backend
{
  "message": "Alert: Task queue growing, 25 ready beads but only 3 active workers",
  "context": {
    "queue_trend": {
      "1h_ago": 10,
      "30min_ago": 18,
      "now": 25,
      "growth_rate": "15 beads/hour"
    },
    "active_workers": 3,
    "max_workers_config": 10,
    "subscription_limits": {
      "claude_pro": "unlimited",
      "budget_remaining": "$150/mo"
    }
  },
  "tools": [...]
}

// Chat Backend → FORGE
{
  "tool_calls": [
    {"tool": "spawn_worker", "args": {"model": "claude-sonnet-4.5", "count": 4}},
    {"tool": "show_notification", "args": {"message": "Queue growing faster than completion rate. Spawned 4 workers to catch up."}}
  ],
  "reasoning": "Queue growing 15 beads/hour but workers completing ~8/hour. Spawn 4 more workers (subscription cost: $0) to prevent backlog"
}
```

### 7. Model Fallback

**Scenario**: FORGE detects primary model unavailable

```json
// FORGE → Chat Backend
{
  "message": "Error: Chat backend failed to start: 'claude-sonnet-4.5' unavailable (rate limit)",
  "context": {
    "attempted_model": "claude-sonnet-4.5",
    "error": "rate_limit_exceeded",
    "available_alternatives": [
      {"model": "claude-haiku-4.5", "status": "available"},
      {"model": "gpt-4o", "status": "available"},
      {"model": "qwen-2.5", "status": "available"}
    ],
    "user_preference_order": ["claude-sonnet-4.5", "claude-haiku-4.5", "gpt-4o"]
  },
  "tools": [...]
}

// Chat Backend → FORGE (this would be from a different backend instance)
{
  "tool_calls": [
    {"tool": "set_config", "args": {"key": "chat_backend_model", "value": "claude-haiku-4.5"}},
    {"tool": "restart_backend", "args": {}},
    {"tool": "show_warning", "args": {"message": "Sonnet unavailable (rate limit). Using Haiku as fallback."}}
  ],
  "reasoning": "Primary model rate-limited. Fall back to next preference (Haiku) to maintain chat functionality"
}
```

### 8. Dependency Resolution

**Scenario**: FORGE detects blocked beads with circular dependencies

```json
// FORGE → Chat Backend
{
  "message": "Warning: 5 P0 beads blocked, but blocking beads are P2 priority",
  "context": {
    "blocked_beads": [
      {"id": "bd-abc", "priority": "P0", "blocked_by": "bd-xyz"},
      {"id": "bd-def", "priority": "P0", "blocked_by": "bd-xyz"},
      {"id": "bd-ghi", "priority": "P0", "blocked_by": "bd-uvw"}
    ],
    "blocking_beads": [
      {"id": "bd-xyz", "priority": "P2", "status": "open", "blocks": 2},
      {"id": "bd-uvw", "priority": "P2", "status": "open", "blocks": 1}
    ]
  },
  "tools": [...]
}

// Chat Backend → FORGE (Note: Can't modify beads directly, can only suggest)
{
  "tool_calls": [
    {"tool": "show_warning", "args": {
      "message": "Priority inversion detected: P2 beads blocking P0 work",
      "details": "bd-xyz (P2) blocks 2 P0 beads. Consider running: br update bd-xyz --priority P0"
    }},
    {"tool": "highlight_beads", "args": {"bead_ids": ["bd-xyz", "bd-uvw"], "reason": "blocking_critical_work"}}
  ],
  "reasoning": "Can't modify beads directly (br CLI authority), but can alert user to priority inversion and suggest fix"
}
```

### 9. Health Monitoring

**Scenario**: FORGE detects worker performance degradation

```json
// FORGE → Chat Backend
{
  "message": "Degradation detected: sonnet-alpha response time increased 5x in last 30min",
  "context": {
    "worker_id": "sonnet-alpha",
    "performance_trend": {
      "avg_response_time_30min_ago": "2.5s",
      "avg_response_time_15min_ago": "7.8s",
      "avg_response_time_now": "12.3s",
      "trend": "degrading"
    },
    "worker_logs": [
      "2026-02-07T10:45:00 warning High memory usage: 85%",
      "2026-02-07T10:47:00 warning Context window approaching limit",
      "2026-02-07T10:50:00 error Timeout retrieving workspace files"
    ]
  },
  "tools": [...]
}

// Chat Backend → FORGE
{
  "tool_calls": [
    {"tool": "restart_worker", "args": {"worker_id": "sonnet-alpha"}},
    {"tool": "show_notification", "args": {"message": "Restarted sonnet-alpha due to performance degradation (5x slower)"}}
  ],
  "reasoning": "Worker showing memory issues and slow responses. Restart to clear state and restore performance"
}
```

### 10. Anomaly Detection

**Scenario**: FORGE detects unusual cost pattern

```json
// FORGE → Chat Backend
{
  "message": "Anomaly: Spend rate 10x normal for last 2 hours",
  "context": {
    "normal_hourly_spend": "$1.20",
    "last_2h_hourly_spend": "$12.50",
    "anomaly_multiplier": 10.4,
    "potential_causes": [
      {"worker": "opus-alpha", "activity": "Started 2h ago, unusually high token usage"},
      {"bead": "bd-xyz", "description": "Large codebase analysis, 500K+ tokens input"}
    ],
    "current_workers": [
      {"id": "opus-alpha", "model": "claude-opus-4", "cost_last_2h": "$18.50"}
    ]
  },
  "tools": [...]
}

// Chat Backend → FORGE
{
  "tool_calls": [
    {"tool": "show_warning", "args": {
      "message": "Cost anomaly: opus-alpha spending 10x normal rate",
      "details": "Working on bd-xyz (large codebase analysis). Cost: $18.50 in 2h vs $2.40 normal."
    }},
    {"tool": "ask_user", "args": {
      "question": "opus-alpha high cost detected. Continue or kill?",
      "options": ["Continue", "Kill worker", "Kill and reassign to cheaper model"]
    }}
  ],
  "reasoning": "Unusual spend pattern, but legitimate cause (large codebase). Alert user and request decision rather than auto-kill"
}
```

---

### Example Flow (User-Driven)

**User Input**: "Show me costs for last week and spawn 2 more workers if costs are low"

**LLM Processing**:
```json
{
  "tool_calls": [
    {
      "tool": "show_costs",
      "arguments": {
        "period": "last_week"
      }
    },
    {
      "tool": "conditional_spawn",
      "arguments": {
        "condition": "costs < budget_threshold",
        "action": {
          "tool": "spawn_worker",
          "arguments": {
            "model": "sonnet",
            "count": 2
          }
        }
      }
    }
  ]
}
```

**FORGE Execution**:
1. Execute `show_costs("last_week")` → Display cost panel
2. Evaluate cost condition
3. If condition true: Execute `spawn_worker("sonnet", 2)`
4. Display results to user with confirmation

**Visual Feedback**:
```
┌─ AGENT PROCESSING ──────────────────────────────┐
│ 🔧 Calling: show_costs(period="last_week")      │
│    → ✓ Cost view displayed                      │
│                                                  │
│ 🔧 Calling: spawn_worker(model="sonnet", n=2)   │
│    → ✓ sonnet-gamma spawned                     │
│    → ✓ sonnet-delta spawned                     │
│                                                  │
│ Press Esc to cancel remaining actions           │
└──────────────────────────────────────────────────┘
```

---

## Tool Security Model

### Restricted Tool Set

The headless LLM only has access to **safe, reversible tools**:

**✅ Allowed (Read-only & Safe Actions)**:
- View switching
- Data queries
- Filtering/sorting
- Export to files
- Help/documentation
- Configuration reading

**⚠️ Allowed with Confirmation (Potentially Disruptive)**:
- Spawning workers (user confirm count if >5)
- Killing workers (user confirm)
- Modifying configuration (user confirm changes)
- Cost optimization changes (preview + confirm)

**❌ Never Allowed (Dangerous)**:
- Direct file system access
- Network requests to external services
- Shell command execution
- Credential management
- System-level operations

### Tool Call Validation

```python
class ToolExecutor:
    def validate_tool_call(self, tool_call):
        """Validate tool call before execution"""

        # Check tool is in allowed list
        if tool_call.tool not in ALLOWED_TOOLS:
            raise SecurityError(f"Tool {tool_call.tool} not allowed")

        # Validate arguments
        schema = TOOL_SCHEMAS[tool_call.tool]
        validate_arguments(tool_call.arguments, schema)

        # Check rate limits (prevent spam)
        if not check_rate_limit(tool_call.tool):
            raise RateLimitError("Too many tool calls")

        # Check requires_confirmation
        if TOOL_METADATA[tool_call.tool].requires_confirmation:
            return ConfirmationRequired(tool_call)

        return ValidatedToolCall(tool_call)
```

---

## User Confirmation Flow

**For Potentially Disruptive Actions**:

```
User: "Kill all idle workers"
    ↓
LLM: kill_worker() × 5 calls
    ↓
FORGE: Shows confirmation dialog
    ↓
┌─ CONFIRM ACTION ────────────────────────────────┐
│ About to kill 5 workers:                        │
│   - sonnet-alpha (idle 2h)                      │
│   - sonnet-beta (idle 1.5h)                     │
│   - haiku-gamma (idle 3h)                       │
│   - opus-delta (idle 30m)                       │
│   - qwen-epsilon (idle 4h)                      │
│                                                  │
│ [Y] Confirm  [N] Cancel  [E] Exclude some       │
└──────────────────────────────────────────────────┘
```

---

## Rationale

### Why Tool-Based vs Hotkeys

**Discoverability**:
- Tool-based: User asks "what can you do?" and gets a list
- Hotkeys: User must find and read help menu

**Expressiveness**:
- Tool-based: "Show P0 tasks and spawn 2 workers if queue is long"
- Hotkeys: Multiple key presses in sequence

**Learnability**:
- Tool-based: Natural language, no memorization
- Hotkeys: Must memorize dozens of shortcuts

**Accessibility**:
- Tool-based: Works with screen readers, typing-based
- Hotkeys: Requires physical keyboard shortcuts

**Composability**:
- Tool-based: LLM can chain multiple actions intelligently
- Hotkeys: Limited to single actions

### Why Headless LLM

**No UI Bloat**: LLM runs in background, no API calls for simple view switches

**Fast Response**: Tool calls are local, no network latency

**Privacy**: Commands processed locally, no external API calls for control

**Offline**: Works without internet (if using local model)

---

## Consequences

### Positive

- **Dramatically better UX**: No memorization required
- **Self-documenting**: Users discover features by asking
- **Powerful automation**: Chain complex actions naturally
- **Accessible**: Works with assistive technologies
- **Extensible**: Easy to add new tools as features grow
- **Smart defaults**: LLM fills in missing parameters intelligently

### Negative

- **LLM dependency**: Requires LLM backend (cost/latency)
- **Ambiguity**: Natural language can be ambiguous
- **Learning curve**: Users must learn what's possible (but "help" tool mitigates this)
- **Potential errors**: LLM might misinterpret intent

### Neutral

- **Hybrid approach**: Hotkeys coexist as optional shortcuts (e.g., `Ctrl+W` for workers view)
- **Model selection**: Can use cheap/fast models (Haiku) for control
- **Fallback**: If LLM fails, graceful degradation to hotkeys or menu navigation
- **Learning curve**: Users naturally discover hotkeys through tool execution ("Press W to return to workers view")

---

## Alternatives Considered

### Option 1: Hotkeys Only
- **Pros**: Fast, no LLM needed, deterministic
- **Cons**: Poor discoverability, memorization burden
- **Verdict**: Rejected as primary interface, but kept as optional shortcuts

### Option 2: Menu-Driven UI
- **Pros**: Discoverable, no LLM needed
- **Cons**: Slow navigation, many keystrokes
- **Verdict**: Rejected - too cumbersome

### Option 3: Web Dashboard with Buttons
- **Pros**: Most discoverable, familiar
- **Cons**: Not terminal-native, requires HTTP server
- **Verdict**: Rejected - conflicts with TUI decision (ADR 0002)

### Option 4: Voice Control
- **Pros**: Hands-free, accessible
- **Cons**: Requires microphone, privacy concerns, noise issues
- **Verdict**: Interesting but out of scope

---

## Implementation Plan

### Phase 1: Core Tool Infrastructure
- [ ] Define tool schema format (OpenAPI-like)
- [ ] Implement tool executor with validation
- [ ] Add confirmation dialog system
- [ ] Basic tool set (view switching, help)

### Phase 2: Essential Tools
- [ ] Worker management tools
- [ ] Task management tools
- [ ] Cost/analytics tools
- [ ] Configuration tools

### Phase 3: LLM Integration
- [ ] Integrate headless LLM (Claude Haiku for speed)
- [ ] Tool call parsing and execution
- [ ] Error handling and fallbacks
- [ ] Rate limiting and security

### Phase 4: Advanced Features
- [ ] Tool call history and replay
- [ ] Custom tool definitions (user plugins)
- [ ] Multi-step workflows with conditionals
- [ ] Tool call analytics and optimization

---

## Protocol: FORGE ↔ Chat Backend

### Tool Definition Injection

**On Backend Startup**: FORGE provides tool definitions via stdin

```json
// FORGE → Chat Backend (initial message)
{
  "type": "init",
  "tools": [
    {
      "name": "spawn_worker",
      "description": "Spawn new AI workers",
      "parameters": {
        "type": "object",
        "properties": {
          "model": {
            "type": "string",
            "enum": ["claude-sonnet-4.5", "claude-haiku-4.5", "claude-opus-4", "gpt-4o", "qwen-2.5-72b"],
            "description": "Model to use for worker"
          },
          "count": {
            "type": "integer",
            "minimum": 1,
            "maximum": 10,
            "default": 1,
            "description": "Number of workers to spawn"
          }
        },
        "required": ["model"]
      }
    },
    {
      "name": "kill_worker",
      "description": "Terminate a worker",
      "parameters": {
        "type": "object",
        "properties": {
          "worker_id": {
            "type": "string",
            "description": "Worker ID to kill"
          }
        },
        "required": ["worker_id"]
      }
    },
    // ... 30+ more tools
  ]
}
```

### Chat Flow

**User-Driven Message**:
```json
// FORGE → Chat Backend
{
  "type": "message",
  "message": "reconfigure max glm workers to 5",
  "context": {
    "current_config": {
      "max_glm_workers": 3,
      "max_total_workers": 10
    },
    "active_workers": [
      {"id": "glm-alpha", "model": "glm-4.7"},
      {"id": "glm-beta", "model": "glm-4.7"}
    ]
  }
}

// Chat Backend → FORGE
{
  "type": "response",
  "tool_calls": [
    {
      "tool": "set_config",
      "arguments": {
        "key": "max_glm_workers",
        "value": 5
      }
    }
  ],
  "message": "Updated max_glm_workers from 3 to 5"
}
```

**Autonomous/Telemetry Message**:
```json
// FORGE → Chat Backend
{
  "type": "telemetry",
  "message": "3 claude-code-opus workers failing with rate_limit_error",
  "context": {
    "failing_workers": ["opus-alpha", "opus-beta", "opus-gamma"],
    "error_pattern": "rate_limit_error",
    "other_opus_workers": ["opus-delta", "opus-epsilon"],
    "rate_limit_details": {
      "requests_in_last_hour": 500,
      "limit": 500,
      "reset_in": "45min"
    }
  }
}

// Chat Backend → FORGE
{
  "type": "response",
  "tool_calls": [
    {"tool": "kill_worker", "arguments": {"worker_id": "opus-delta"}},
    {"tool": "kill_worker", "arguments": {"worker_id": "opus-epsilon"}},
    {"tool": "set_config", "arguments": {"key": "disable_opus_spawning", "value": true, "duration_minutes": 45}}
  ],
  "message": "Proactively shutting down remaining opus workers. Rate limit resets in 45min.",
  "reasoning": "Rate limit reached. Prevent additional failures by disabling opus workers until limit resets."
}
```

### Tool Execution Response

**FORGE → Chat Backend** (after execution):
```json
{
  "type": "tool_result",
  "tool_call_id": "call_123",
  "result": {
    "success": true,
    "worker_id": "opus-delta",
    "status": "killed"
  }
}
```

---

## Tool Definition Format

**Example Tool Schema** (YAML for documentation, JSON for protocol):
```yaml
name: spawn_worker
description: Spawn new AI workers
category: worker_management
requires_confirmation: true
confirmation_threshold:
  count: 5  # Confirm if count > 5

parameters:
  model:
    type: string
    required: true
    enum: [sonnet, opus, haiku, gpt4, qwen]
    description: Model to use for worker

  count:
    type: integer
    required: true
    min: 1
    max: 10
    default: 1
    description: Number of workers to spawn

  workspace:
    type: string
    required: false
    description: Workspace path (defaults to current)

returns:
  type: object
  properties:
    worker_ids:
      type: array
      items: string
    success: boolean

examples:
  - user: "Spawn 3 sonnet workers"
    tool_call:
      tool: spawn_worker
      arguments:
        model: sonnet
        count: 3

  - user: "I need more workers"
    tool_call:
      tool: spawn_worker
      arguments:
        model: sonnet  # LLM infers default
        count: 2        # LLM infers reasonable count
```

---

## Metrics

Success criteria:
- **Discovery rate**: ≥80% of users find features via chat vs help menu
- **Command success**: ≥90% of commands execute correctly
- **User satisfaction**: ≥4/5 on "ease of control" rating
- **Hotkey usage**: <20% of actions via hotkeys (most via chat)

Monitoring:
- Tool call frequency distribution
- Failed tool calls (intent misinterpretation)
- Confirmation rate (how often users confirm vs cancel)
- Time to complete common workflows

---

## Hotkey Integration

Hotkeys remain available as **optional shortcuts**, not replacements:

**Design Principles**:
1. **Chat is primary**: All features accessible via natural language
2. **Hotkeys are shortcuts**: Optional, for speed, not discovery
3. **Teach through use**: Show hotkey hints after tool execution
4. **User choice**: Power users can use hotkeys, new users can ignore them
5. **No hidden features**: Nothing is locked behind hotkeys

**Example Flow**:
```
User: "show workers"         [First time - uses chat]
FORGE: Executes switch_view("workers")
       Shows: "Worker view (Press W to return here)"

[Later]
User: W                      [Now knows the shortcut]
FORGE: Instant switch to workers view
```

**Hotkey Mapping**:
- Each tool can have an optional hotkey binding
- Hotkeys trigger the same tool calls as chat
- Configuration: `~/.forge/config.yaml`

See [HOTKEYS.md](../HOTKEYS.md) for complete reference.

---

## References

- [Tool Catalog](../TOOL_CATALOG.md) - Complete tool reference
- [Hotkeys Reference](../HOTKEYS.md) - Optional keyboard shortcuts
- [Conversational Interface Design](../notes/conversational-interface.md)
- [Dashboard Design](../notes/dashboard-design.md)
- ADR 0002: Use TUI for Control Panel Interface
- Claude Code tool system (inspiration)
- OpenAI function calling (similar pattern)

---

## Future Enhancements

1. **Tool Macros**: Save common tool sequences as named macros
2. **Conditional Execution**: "If X then Y else Z" logic
3. **Scheduled Tools**: "Spawn 2 workers every morning at 9am"
4. **Tool Plugins**: User-defined custom tools
5. **Natural Language Queries**: "How many workers are active?" returns data, not just view switch
6. **Voice Input**: Optional voice command support
7. **Tool Explanations**: LLM explains what each tool will do before execution

---

**FORGE** - Federated Orchestration & Resource Generation Engine
