# FORGE Tool Catalog

Complete reference for all tools available in the conversational interface.

---

## How to Use Tools

Press `:` to activate command input, then type natural language commands:

```
User: "Show me all P0 tasks"
FORGE: Executes filter_tasks(priority="P0")

User: "Spawn 3 sonnet workers in the trading workspace"
FORGE: Executes spawn_worker("sonnet", 3, "/path/to/trading")

User: "What did I spend this week?"
FORGE: Executes show_costs(period="this_week")
```

You don't need to know the exact tool names - the LLM translates your intent into tool calls.

---

## Tool Categories

- [View Control](#view-control) - Switch views, layouts, panels
- [Worker Management](#worker-management) - Spawn, kill, restart workers
- [Task Management](#task-management) - Create, filter, assign tasks
- [Cost & Analytics](#cost--analytics) - Costs, metrics, forecasting
- [Data Export](#data-export) - Export logs, metrics, screenshots
- [Configuration](#configuration) - Settings, layouts, preferences
- [Help & Discovery](#help--discovery) - Documentation, search, help
- [Notification](#notification) - Display notifications and prompts
- [System](#system) - System status, health checks, worker control
- [Workspace](#workspace) - Workspace management and switching
- [Analytics](#analytics) - Performance analytics and trends

---

## View Control

### `switch_view(view)`
Switch to a different dashboard view.

**Parameters**:
- `view` (string): View name - `workers`, `tasks`, `costs`, `metrics`, `logs`, `overview`

**Examples**:
```
"Show me the worker status"  → switch_view("workers")
"Go to cost view"            → switch_view("costs")
"Show me the dashboard"      → switch_view("overview")
```

---

### `split_view(left, right)`
Create a split-screen layout.

**Parameters**:
- `left` (string): Left panel view
- `right` (string): Right panel view

**Examples**:
```
"Show workers on left and tasks on right"  → split_view("workers", "tasks")
"Split screen with costs and metrics"      → split_view("costs", "metrics")
```

---

### `focus_panel(panel)`
Focus on a specific panel within current view.

**Parameters**:
- `panel` (string): Panel name - `activity_log`, `task_queue`, `worker_status`, etc.

**Examples**:
```
"Focus on the activity log"    → focus_panel("activity_log")
"Expand the cost breakdown"    → focus_panel("cost_breakdown")
```

---

## Worker Management

### `spawn_worker(model, count, workspace?)`
Spawn new AI coding workers.

**Parameters**:
- `model` (string): Model type - `sonnet`, `opus`, `haiku`, `gpt4`, `qwen`, etc.
- `count` (integer): Number of workers (1-10)
- `workspace` (string, optional): Workspace path

**Requires confirmation if**: `count > 5`

**Examples**:
```
"Spawn 3 sonnet workers"                        → spawn_worker("sonnet", 3)
"Start 2 opus workers in the trading project"   → spawn_worker("opus", 2, "/path/to/trading")
"I need more workers"                           → spawn_worker("sonnet", 2)  # LLM infers defaults
```

---

### `kill_worker(worker_id)`
Terminate a specific worker.

**Parameters**:
- `worker_id` (string): Worker identifier or "all" for all workers

**Requires confirmation**: Always

**Examples**:
```
"Kill worker sonnet-alpha"      → kill_worker("sonnet-alpha")
"Stop all idle workers"         → kill_worker("all", filter="idle")  # With implicit filter
"Terminate the failed worker"   → kill_worker("auto")  # LLM identifies failed worker
```

---

### `list_workers(filter?)`
List workers with optional filtering.

**Parameters**:
- `filter` (string, optional): Filter by status - `idle`, `active`, `failed`, `all`

**Examples**:
```
"Show me all workers"           → list_workers()
"Show idle workers"             → list_workers(filter="idle")
"Which workers are failing?"    → list_workers(filter="failed")
```

---

### `restart_worker(worker_id)`
Restart a worker (kills and respawns).

**Parameters**:
- `worker_id` (string): Worker identifier

**Requires confirmation**: If worker is active

**Examples**:
```
"Restart worker sonnet-beta"    → restart_worker("sonnet-beta")
"Restart the hung worker"       → restart_worker("auto")  # LLM identifies hung worker
```

---

## Task Management

### `filter_tasks(priority?, status?, labels?)`
Filter the task queue display.

**Parameters**:
- `priority` (string, optional): `P0`, `P1`, `P2`, `P3`, `P4`
- `status` (string, optional): `open`, `in_progress`, `blocked`, `completed`
- `labels` (array, optional): Array of label strings

**Examples**:
```
"Show only P0 tasks"                    → filter_tasks(priority="P0")
"Show me blocked tasks"                 → filter_tasks(status="blocked")
"Show P1 tasks that are in progress"   → filter_tasks(priority="P1", status="in_progress")
```

---

### `create_task(title, priority, description?)`
Create a new task (bead).

**Parameters**:
- `title` (string): Task title
- `priority` (string): `P0` to `P4`
- `description` (string, optional): Detailed description

**Examples**:
```
"Create a P1 task to fix the login bug"              → create_task("Fix login bug", "P1")
"Add a P0 task: investigate trading halt failures"   → create_task("Investigate halt failures", "P0", "...")
```

---

### `assign_task(task_id, worker_id?)`
Assign a task to a worker.

**Parameters**:
- `task_id` (string): Task/bead ID (e.g., `bd-abc`)
- `worker_id` (string, optional): Worker ID, or "auto" for automatic assignment

**Examples**:
```
"Assign bd-abc to sonnet-alpha"           → assign_task("bd-abc", "sonnet-alpha")
"Assign the top task to the best worker"  → assign_task("auto", "auto")  # LLM picks both
```

---

## Cost & Analytics

### `show_costs(period?, breakdown?)`
Display cost analysis.

**Parameters**:
- `period` (string, optional): `today`, `yesterday`, `this_week`, `last_week`, `this_month`, `last_month`
- `breakdown` (string, optional): `by_model`, `by_worker`, `by_task`, `by_workspace`

**Examples**:
```
"What did I spend today?"                    → show_costs(period="today")
"Show me last month's costs by model"       → show_costs(period="last_month", breakdown="by_model")
"How much am I spending?"                    → show_costs(period="today")
```

---

### `optimize_routing()`
Run cost optimization analysis and update routing rules.

**Requires confirmation**: Always (shows preview of changes)

**Examples**:
```
"Optimize my costs"           → optimize_routing()
"How can I save money?"       → optimize_routing()  # Shows recommendations
```

---

### `forecast_costs(days?)`
Forecast future costs based on current usage.

**Parameters**:
- `days` (integer, optional): Days to forecast (default: 30)

**Examples**:
```
"What will I spend next month?"     → forecast_costs(days=30)
"Project my costs for 2 weeks"      → forecast_costs(days=14)
```

---

### `show_metrics(metric_type?, period?)`
Display performance metrics.

**Parameters**:
- `metric_type` (string, optional): `throughput`, `latency`, `success_rate`, `all`
- `period` (string, optional): Time period (same as `show_costs`)

**Examples**:
```
"Show me performance metrics"          → show_metrics(metric_type="all")
"What's my task throughput today?"    → show_metrics(metric_type="throughput", period="today")
```

---

## Data Export

### `export_logs(format?, period?)`
Export activity logs.

**Parameters**:
- `format` (string, optional): `json`, `csv`, `txt` (default: `json`)
- `period` (string, optional): Time period (same as `show_costs`)

**Examples**:
```
"Export today's logs as CSV"        → export_logs(format="csv", period="today")
"Save logs"                         → export_logs()  # Defaults to JSON, today
```

---

### `export_metrics(metric_type?, format?)`
Export metrics data.

**Parameters**:
- `metric_type` (string, optional): `performance`, `costs`, `workers`, `all`
- `format` (string, optional): `json`, `csv` (default: `json`)

**Examples**:
```
"Export performance metrics as CSV"  → export_metrics("performance", "csv")
"Save cost data"                     → export_metrics("costs")
```

---

### `screenshot(panel?)`
Take a screenshot of the dashboard.

**Parameters**:
- `panel` (string, optional): Specific panel name, or "all" for full dashboard

**Examples**:
```
"Take a screenshot"                   → screenshot("all")
"Screenshot the cost panel"           → screenshot("costs")
```

---

## Configuration

### `set_config(key, value)`
Update configuration setting.

**Requires confirmation**: For critical settings

**Examples**:
```
"Set default model to sonnet"             → set_config("default_model", "sonnet")
"Change max workers to 10"                → set_config("max_workers", 10)
"Enable debug mode"                       → set_config("debug_mode", true)
```

---

### `get_config(key?)`
View configuration settings.

**Parameters**:
- `key` (string, optional): Specific config key, or omit for all settings

**Examples**:
```
"What's my current config?"        → get_config()
"What's the default model?"        → get_config("default_model")
```

---

### `save_layout(name)`
Save current dashboard layout.

**Parameters**:
- `name` (string): Layout name

**Examples**:
```
"Save this layout as 'monitoring'"    → save_layout("monitoring")
"Remember this view"                  → save_layout("default")
```

---

### `load_layout(name)`
Load a saved dashboard layout.

**Parameters**:
- `name` (string): Layout name

**Examples**:
```
"Load my monitoring layout"     → load_layout("monitoring")
"Switch to default view"        → load_layout("default")
```

---

## Help & Discovery

### `help(topic?)`
Get help on a specific topic or general usage.

**Parameters**:
- `topic` (string, optional): Topic name - `spawning`, `costs`, `tasks`, `tools`, etc.

**Examples**:
```
"How do I spawn workers?"         → help("spawning")
"Help with cost optimization"     → help("costs")
"What can you do?"                → help()
```

---

### `search_docs(query)`
Search documentation for a query.

**Parameters**:
- `query` (string): Search query

**Examples**:
```
"How does cost optimization work?"     → search_docs("cost optimization")
"Find info about task scoring"         → search_docs("task scoring")
```

---

### `list_capabilities()`
List all available tools and features.

**Examples**:
```
"What can you do?"           → list_capabilities()
"Show me all commands"       → list_capabilities()
```

---

## Advanced Patterns

### Chaining Actions

The LLM can chain multiple actions intelligently:

```
"Show me P0 tasks and spawn 2 workers if there are more than 5"
→ filter_tasks(priority="P0")
→ [conditional] spawn_worker("sonnet", 2)  # Only if task count > 5
```

### Conditional Execution

```
"Kill idle workers if costs are high"
→ show_costs(period="today")
→ [if costs > threshold] kill_worker("all", filter="idle")
```

### Smart Defaults

The LLM fills in missing parameters intelligently:

```
"Spawn some workers"
→ spawn_worker("sonnet", 2)  # Infers default model and reasonable count
```

### Error Recovery

```
"Fix the broken workers"
→ list_workers(filter="failed")
→ restart_worker([identified failed workers])
```

---

## Tool Execution Feedback

When tools execute, you'll see real-time feedback:

```
┌─ AGENT PROCESSING ──────────────────────────────┐
│ 🔧 Calling: filter_tasks(priority="P0")         │
│    → ✓ Showing 3 P0 tasks                       │
│                                                  │
│ 🔧 Calling: spawn_worker(model="sonnet", n=2)   │
│    → ⏳ Spawning sonnet-gamma...                 │
│    → ✓ sonnet-gamma spawned                     │
│    → ⏳ Spawning sonnet-delta...                 │
│    → ✓ sonnet-delta spawned                     │
│                                                  │
│ Press Esc within 2s to cancel remaining actions │
└──────────────────────────────────────────────────┘
```

---

## Security Notes

- **Safe by default**: All tools are designed to be reversible
- **Confirmation required**: Potentially disruptive actions require user confirmation
- **Rate limited**: Tool calls are rate-limited to prevent abuse
- **Validated**: All parameters are validated before execution
- **Logged**: All tool executions are logged for audit

---

## Custom Tools (Future)

Users will be able to define custom tools via plugins:

```yaml
# ~/.forge/tools/deploy.yaml
name: deploy_to_staging
description: Deploy current workspace to staging environment
category: custom
parameters:
  workspace: string
command: |
  cd {workspace} && ./deploy.sh staging
```

---

**FORGE** - Federated Orchestration & Resource Generation Engine
