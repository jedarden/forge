# FORGE Tool Catalog

Complete reference for all tools available in the conversational interface.

**Total Tools: 45** across 11 categories

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

## Notification

### `show_notification(message, level?)`
Display a notification message to the user.

**Parameters**:
- `message` (string): Notification message to display
- `level` (string, optional): Notification level - `info`, `warning`, `error`, `success` (default: `info`)

**Examples**:
```
"Show a notification"                           → show_notification("Task completed")
"Warn me about something"                       → show_notification("High costs detected", level="warning")
```

---

### `show_warning(message, details?)`
Display a warning message to the user.

**Parameters**:
- `message` (string): Warning message to display
- `details` (string, optional): Additional details about the warning

**Examples**:
```
"Show a warning about costs"                    → show_warning("Costs are high", "Consider optimizing routing")
```

---

### `ask_user(question, options?)`
Prompt the user for input with a question and options.

**Parameters**:
- `question` (string): Question to ask the user
- `options` (array, optional): List of options for the user to choose from

**Examples**:
```
"Ask if I should kill the worker"              → ask_user("Kill worker sonnet-alpha?", ["Yes", "No", "Cancel"])
```

---

### `highlight_beads(bead_ids, reason?)`
Highlight specific beads in the task queue.

**Parameters**:
- `bead_ids` (array): List of bead IDs to highlight
- `reason` (string, optional): Reason for highlighting

**Examples**:
```
"Highlight these beads"                        → highlight_beads(["bd-abc", "bd-def"], "Blocking critical work")
```

---

## System

### `get_status(component?)`
Get the current status of FORGE and all workers.

**Parameters**:
- `component` (string, optional): Specific component to check - `all`, `workers`, `backend`, `system` (default: `all`)

**Examples**:
```
"What's the status?"                           → get_status()
"Show worker status"                           → get_status(component="workers")
```

---

### `refresh(scope?)`
Refresh the current view or all data.

**Parameters**:
- `scope` (string, optional): What to refresh - `current`, `all`, `workers`, `tasks`, `costs` (default: `current`)

**Examples**:
```
"Refresh"                                      → refresh()
"Refresh all data"                             → refresh(scope="all")
```

---

### `ping_worker(worker_id)`
Check if a worker is responsive.

**Parameters**:
- `worker_id` (string): Worker ID to ping

**Examples**:
```
"Check if sonnet-alpha is responsive"          → ping_worker("sonnet-alpha")
```

---

### `get_worker_info(worker_id)`
Get detailed information about a specific worker.

**Parameters**:
- `worker_id` (string): Worker ID to get info for

**Examples**:
```
"Show info for sonnet-alpha"                   → get_worker_info("sonnet-alpha")
```

---

### `pause_worker(worker_id)`
Pause a worker (temporarily stop processing).

**Parameters**:
- `worker_id` (string): Worker ID to pause

**Requires confirmation**: Always

**Examples**:
```
"Pause sonnet-alpha"                           → pause_worker("sonnet-alpha")
```

---

### `resume_worker(worker_id)`
Resume a paused worker.

**Parameters**:
- `worker_id` (string): Worker ID to resume

**Examples**:
```
"Resume sonnet-alpha"                          → resume_worker("sonnet-alpha")
```

---

## Workspace

### `switch_workspace(path)`
Switch to a different workspace.

**Parameters**:
- `path` (string): Workspace path to switch to

**Requires confirmation**: Always

**Examples**:
```
"Switch to /home/coder/trading"                → switch_workspace("/home/coder/trading")
```

---

### `list_workspaces(filter?)`
List all available workspaces.

**Parameters**:
- `filter` (string, optional): Filter workspaces by status - `active`, `inactive`, `all`

**Examples**:
```
"Show all workspaces"                          → list_workspaces()
"Show active workspaces"                       → list_workspaces(filter="active")
```

---

### `create_workspace(path, template?)`
Create a new workspace.

**Parameters**:
- `path` (string): Workspace path to create
- `template` (string, optional): Template to use - `empty`, `python`, `javascript`, `rust`

**Examples**:
```
"Create a Python workspace"                    → create_workspace("/home/coder/myproject", template="python")
```

---

### `get_workspace_info()`
Get information about the current workspace.

**Examples**:
```
"Show workspace info"                          → get_workspace_info()
```

---

## Analytics

### `show_throughput(period?)`
Display task throughput metrics.

**Parameters**:
- `period` (string, optional): Time period for analysis (same as `show_costs`)

**Examples**:
```
"Show throughput today"                        → show_throughput(period="today")
```

---

### `show_latency(period?)`
Display task latency metrics.

**Parameters**:
- `period` (string, optional): Time period for analysis (same as `show_costs`)

**Examples**:
```
"Show latency this week"                       → show_latency(period="this_week")
```

---

### `show_success_rate(period?)`
Display task success rate metrics.

**Parameters**:
- `period` (string, optional): Time period for analysis (same as `show_costs`)

**Examples**:
```
"Show success rate"                            → show_success_rate()
```

---

### `show_worker_efficiency(by_model?)`
Display worker efficiency comparison.

**Parameters**:
- `by_model` (boolean, optional): Group by model type (default: `true`)

**Examples**:
```
"Show worker efficiency"                       → show_worker_efficiency()
```

---

### `show_task_distribution()`
Display task distribution across priorities.

**Examples**:
```
"Show task distribution"                       → show_task_distribution()
```

---

### `show_trends(metric, period?)`
Display trends for a specific metric over time.

**Parameters**:
- `metric` (string): Metric to show trends for - `costs`, `throughput`, `latency`, `success_rate`, `worker_count`
- `period` (string, optional): Time period for trends (default: `this_week`)

**Examples**:
```
"Show cost trends this week"                   → show_trends(metric="costs", period="this_week")
```

---

### `analyze_bottlenecks()`
Analyze potential bottlenecks in the workflow.

**Examples**:
```
"Find bottlenecks"                             → analyze_bottlenecks()
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
