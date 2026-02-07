# Control Panel TUI Dashboard - 199×38 Terminal Layout

**Terminal Dimensions**: 199 columns × 38 rows

## Design Strategy

With a 199×38 terminal, we can use a **three-column ultra-wide layout** that displays all critical information in a single view without requiring screen switching.

**Layout Structure**:
- **Header**: 2 rows (title + status bar)
- **Main Content**: 33 rows (3-column layout)
- **Footer**: 3 rows (shortcuts + command bar)

**Column Distribution** (199 cols total):
- Left Column: 66 cols - Worker Pool + Subscriptions
- Middle Column: 66 cols - Task Queue + Activity Log
- Right Column: 65 cols - Cost Analytics + Actions
- Borders: 2 cols between columns

---

## Full Dashboard Mockup (199×38)

```
╔═══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╗
║ CONTROL PANEL DASHBOARD                                                                                                          14:23:45 | Subscriptions: 3 Active | Workers: 9/9 | Cost: $2.34/day ║
╠═══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣
║ ┌─ WORKER POOL (9 Active) ──────────────────────────────┐ ┌─ TASK QUEUE (47 Ready) ────────────────────────────────┐ ┌─ COST ANALYTICS (Today) ──────────────────────────────┐ ║
║ │ Session      │ Type   │ Workspace        │ Status │ ⏱ │ │ ID      │Pri│ Title                  │ Model   │Tokens│ │ Model        │ Requests │   Tokens │    Cost │ Trend │ ║
║ │──────────────┼────────┼──────────────────┼────────┼───│ │─────────┼───┼────────────────────────┼─────────┼──────│ │──────────────┼──────────┼──────────┼─────────┼───────│ ║
║ │ glm-alpha    │ GLM4.7 │ ardenone-cluster │ ●EXEC  │12m│ │ po-7jb  │P0 │ Research TUI framework │ Sonnet  │ 45K  │ │ Sonnet 4.5   │       24 │   347K ↑ │  $4.17  │ ▂▃▅█  │ ║
║ │ glm-bravo    │ GLM4.7 │ claude-config    │ ●EXEC  │ 8m│ │ po-1to  │P0 │ Analyze orchestrators  │ Sonnet  │ 38K  │ │ GLM-4.7      │       89 │   124K ↑ │  $0.00  │ ▁▂▂▃  │ ║
║ │ glm-charlie  │ GLM4.7 │ botburrow-agents │ ●EXEC  │15m│ │ po-3h3  │P0 │ Compare LLM models     │ Sonnet  │ 52K  │ │ Opus 4.6     │        3 │    67K ↑ │  $8.24  │ ▁▁▃█  │ ║
║ │ glm-delta    │ GLM4.7 │ botburrow-hub    │ ◐IDLE  │ 2m│ │ po-4gr  │P0 │ Subscription optimize  │ Sonnet  │ 41K  │ │ DeepSeek V3  │       12 │    89K ↑ │  $0.02  │ ▂▃▃▄  │ ║
║ │ glm-echo     │ GLM4.7 │ leaderboard      │ ●EXEC  │ 6m│ │ po-1oh  │P0 │ Compare API pricing    │ Sonnet  │ 48K  │ │──────────────┼──────────┼──────────┼─────────┼───────│ ║
║ │ glm-foxtrot  │ GLM4.7 │ research/bot     │ ●EXEC  │11m│ │ bd-1dp  │P1 │ Fix worker spawning    │ GLM-4.7 │ 15K  │ │ TOTAL TODAY  │      128 │   627K ↑ │ $12.43  │       │ ║
║ │ glm-golf     │ GLM4.7 │ ibkr-mcp         │ ●EXEC  │ 4m│ │ bd-2xa  │P1 │ Add health monitoring  │ GLM-4.7 │ 22K  │ │ AVG/REQUEST  │          │  4,898   │  $0.097 │       │ ║
║ │ glm-hotel    │ GLM4.7 │ options-pipeline │ ●EXEC  │ 9m│ │ bd-3mk  │P2 │ Update documentation   │ Haiku   │  8K  │ │──────────────┼──────────┼──────────┼─────────┼───────│ ║
║ │ glm-india    │ GLM4.7 │ /home/coder      │ ●EXEC  │ 7m│ │ bd-4pl  │P2 │ Refactor lock system   │ GLM-4.7 │ 18K  │ │ PROJECTED    │      450 │  2.2M ↑  │ $43.56  │       │ ║
║ │──────────────┴────────┴──────────────────┴────────┴───│ │─────────┴───┴────────────────────────┴─────────┴──────│ │ MONTH-END    │    9,200 │   45M ↑  │$890.34  │       │ ║
║ │ Health: 9 Healthy, 0 Unhealthy, 0 Stale              │ │ Showing 9 of 47 ready beads (⇅ to scroll)             │ │──────────────┴──────────┴──────────┴─────────┴───────│ ║
║ └───────────────────────────────────────────────────────┘ └────────────────────────────────────────────────────────┘ │ COST BREAKDOWN (Today)                               │ ║
║                                                                                                                        │ ┌────────────────────────────────────────────────────┐ │ ║
║ ┌─ SUBSCRIPTION STATUS ──────────────────────────────────┐ ┌─ ACTIVITY LOG (Live) ──────────────────────────────────┐ │ │ Premium Tasks (P0):    $8.41 (68%)  ████████████▌  │ │ ║
║ │ Service      │  Usage  │ Limit │ Resets    │   Action  │ │ 14:23:42 [●SPAWN] glm-india → /home/coder             │ │ │ Standard Tasks (P1):   $3.12 (25%)  █████          │ │ ║
║ │──────────────┼─────────┼───────┼───────────┼───────────│ │ 14:23:18 [✓CLOSE] bd-2mk completed by glm-delta       │ │ │ Budget Tasks (P2-P4):  $0.90 (7%)   █▌             │ │ ║
║ │ Claude Pro   │ ████▌   │ 500   │ 16d 9h    │ 📊 On-Pace│ │ 14:22:55 [◐IDLE] glm-delta idle (no ready beads)      │ │ └────────────────────────────────────────────────────┘ │ ║
║ │              │  328/500│       │           │           │ │ 14:22:31 [●EXEC] glm-charlie → bd-3xa                 │ └────────────────────────────────────────────────────────┘ ║
║ │──────────────┼─────────┼───────┼───────────┼───────────│ │ 14:21:47 [⚠WARN] Rate limit approaching: Sonnet 4.5   │                                                          ║
║ │ ChatGPT Plus │ ██▌     │ 40msg │ 23d 14h   │ 🚀 Accel  │ │ 14:21:12 [✓CLOSE] po-3pv completed by glm-alpha       │ ┌─ QUICK ACTIONS ─────────────────────────────────────┐ ║
║ │              │  12/40  │ /3hr  │           │           │ │ 14:20:58 [●SPAWN] glm-hotel → options-pipeline        │ │                                                      │ ║
║ │──────────────┼─────────┼───────┼───────────┼───────────│ │ 14:20:34 [◐IDLE] glm-bravo idle (workspace covered)   │ │  [G] Spawn GLM Worker      [K] Kill Selected Worker  │ ║
║ │ Cursor Pro   │ ███████▌│ 500   │ 8d 3h     │ ⚠️ MaxOut │ │ 14:19:45 [●EXEC] glm-foxtrot → bd-1xa                 │ │  [S] Spawn Sonnet Worker   [R] Refresh Dashboard     │ ║
║ │              │  487/500│       │           │           │ │ 14:18:22 [✓CLOSE] bd-4mk completed by glm-golf        │ │  [O] Spawn Opus Worker     [P] Pause All Workers     │ ║
║ │──────────────┼─────────┼───────┼───────────┼───────────│ │ 14:17:58 [●EXEC] glm-echo → po-2ug                    │ │  [H] Spawn Haiku Worker    [C] Configure Settings    │ ║
║ │ DeepSeek API │ Pay/Use │  ∞    │ Monthly   │ 💰 Active │ │ 14:16:31 [◐IDLE] Pool optimizer: 9/9 workers healthy  │ │                                                      │ ║
║ │              │ $0.02/d │       │           │           │ │ 14:15:02 [INFO] Workspace discovered: ardenone-cluster│ │  [W] Worker Details        [T] Task Queue Detail     │ ║
║ └──────────────┴─────────┴───────┴───────────┴───────────┘ │ ⇅ Scroll | 🔍 Filter by: [A]ll [E]rrors [W]arnings    │ │  [A] Assign Task to Model  [L] View Full Logs        │ ║
║                                                              └────────────────────────────────────────────────────────┘ │  [M] Model Settings        [B] Budget Configuration   │ ║
║                                                                                                                        │                                                      │ ║
║                                                                                                                        └──────────────────────────────────────────────────────┘ ║
╠═══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣
║ ┌─ COMMAND INPUT (Press : to activate, Esc to cancel) ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐ ║
║ │ > Why is glm-delta idle? Are there no beads in botburrow-hub?                                                                                                                            [Enter ↵] │ ║
║ └──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘ ║
╠═══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣
║ [Q]uit [?]Help [:] Command [Tab]Panel [/]Search [F1]Workers [F2]Tasks [F3]Costs [F4]Subscriptions [F5]Settings                                      Last Update: 2s ago | CPU: 45% RAM: 2.1GB | ●LIVE ║
╚═══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╝
```

**Dimensions Verified**: 199 columns × 38 rows exactly

---

## Layout Breakdown

### Header (Row 1-2)
- **Row 1**: Title + Real-time stats (subscriptions, workers, cost)
- **Row 2**: Separator line

### Main Content (Row 3-35) - Three Columns

#### Left Column (Cols 1-66): Worker & Subscription Management
- **Worker Pool Table** (Rows 3-15):
  - 9 worker rows + header + separator
  - Columns: Session, Type, Workspace, Status, Time Active
  - Status indicators: ●EXEC (green), ◐IDLE (yellow), ○DEAD (red)
  - Health summary at bottom

- **Subscription Status Table** (Rows 17-35):
  - 4 subscription services + headers
  - Usage bars, limits, reset timers, recommended actions
  - Visual indicators: 📊 On-Pace, 🚀 Accelerate, ⚠️ Max Out, 💰 Active

#### Middle Column (Cols 69-134): Tasks & Activity
- **Task Queue Table** (Rows 3-15):
  - Top 9 ready beads from across all workspaces
  - Columns: ID, Priority, Title, Assigned Model, Est. Tokens
  - Scrollable list (⇅ indicators)

- **Activity Log** (Rows 17-35):
  - Live streaming event log
  - Color-coded events: ●SPAWN, ✓CLOSE, ◐IDLE, ⚠WARN, INFO
  - Scroll controls and filters at bottom

#### Right Column (Cols 137-199): Analytics & Actions
- **Cost Analytics** (Rows 3-15):
  - Per-model breakdown with sparkline trends
  - Today, projected, month-end estimates
  - Average cost per request

- **Cost Breakdown Chart** (Rows 17-22):
  - Horizontal bar chart by task priority
  - Percentage breakdown of spending

- **Quick Actions Panel** (Rows 24-35):
  - 14 keyboard shortcuts for common operations
  - Organized by function (spawn, manage, view)

### Footer (Rows 36-38)
- **Row 36**: Separator
- **Row 37**: Primary shortcuts + function keys + system stats
- **Row 38**: Bottom border

---

## Color Scheme (Textual CSS)

```css
/* Status Colors */
.status-exec { color: #00ff00; }      /* Green - Executing */
.status-idle { color: #ffff00; }      /* Yellow - Idle */
.status-dead { color: #ff0000; }      /* Red - Dead */

/* Priority Colors */
.priority-p0 { color: #ff4444; }      /* Critical */
.priority-p1 { color: #ff8800; }      /* High */
.priority-p2 { color: #ffff00; }      /* Medium */
.priority-p3 { color: #88ff88; }      /* Low */

/* Panel Borders */
.panel-border { border: solid #00aaff; }

/* Cost Trend */
.trend-up { color: #ff4444; }         /* Increasing */
.trend-down { color: #00ff00; }       /* Decreasing */
```

---

## Responsive Behavior

### Auto-Adjustment for Smaller Terminals
If terminal < 199 cols, switch to tabbed view:
- **Tab 1**: Workers + Subscriptions
- **Tab 2**: Tasks + Activity Log
- **Tab 3**: Cost Analytics + Actions

### Vertical Scrolling
- Worker table: If > 9 workers, scrollable
- Task queue: If > 9 ready beads, scrollable
- Activity log: Auto-scroll with manual override

---

## Keyboard Shortcuts

### Worker Management
- `G` - Spawn new GLM-4.7 worker
- `S` - Spawn new Sonnet worker
- `O` - Spawn new Opus worker
- `H` - Spawn new Haiku worker
- `K` - Kill selected worker
- `P` - Pause all workers
- `R` - Refresh dashboard

### Navigation
- `Tab` - Cycle between panels
- `1-9` - Select worker by number
- `⇅` - Scroll active panel
- `F1-F5` - Quick view switching

### Task Management
- `A` - Assign task to specific model
- `W` - View worker details
- `T` - View task queue detail
- `L` - View full logs

### System
- `C` - Configure settings
- `M` - Model settings
- `B` - Budget configuration
- `/` - Search
- `?` - Help
- `Q` - Quit

---

## Data Refresh Rates

- **Worker Status**: 2 seconds
- **Subscription Usage**: 5 seconds
- **Task Queue**: 3 seconds
- **Activity Log**: Real-time (event stream)
- **Cost Analytics**: 10 seconds

---

## Implementation Notes

### Panel Components (Textual Widgets)

```python
class PoolOptimizerDashboard(App):
    CSS_PATH = "dashboard.tcss"

    def compose(self) -> ComposeResult:
        yield Header()
        with Horizontal():
            # Left column
            with Vertical(classes="left-column"):
                yield WorkerPoolTable(id="workers")
                yield SubscriptionStatusTable(id="subscriptions")

            # Middle column
            with Vertical(classes="middle-column"):
                yield TaskQueueTable(id="tasks")
                yield ActivityLog(id="activity")

            # Right column
            with Vertical(classes="right-column"):
                yield CostAnalyticsPanel(id="costs")
                yield QuickActionsPanel(id="actions")

        yield Footer()
```

### Grid Layout (CSS)

```css
Screen {
    layout: grid;
    grid-size: 3 1;
    grid-columns: 66 66 65;
}

.left-column { column-span: 1; }
.middle-column { column-span: 1; }
.right-column { column-span: 1; }
```

---

## Benefits of 199×38 Layout

1. **Single View**: All critical information visible without switching screens
2. **Side-by-Side Comparison**: Workers vs Tasks vs Costs simultaneously
3. **Efficient Width Usage**: Ultra-wide terminals fully utilized
4. **Real-time Monitoring**: Multiple live data streams in parallel
5. **Quick Actions**: Always visible, never need to switch modes
6. **No Scrolling**: Vertical space (38 rows) perfect for 9 workers + logs

This layout maximizes the 199×38 terminal for optimal monitoring and control of the control panel system.

---

## Conversational CLI Input

### Command Bar (Activated with `:` key)

The command bar at the bottom provides a **conversational interface** to the control panel. Instead of memorizing keyboard shortcuts, users can ask questions or give commands in natural language.

#### Example Interactions

**Questions**:
```
> Why is glm-delta idle?
→ glm-delta is idle because botburrow-hub workspace has 0 ready beads. 
  Last activity was 2m ago. Worker is healthy and waiting for tasks.

> What's our total spend this month?
→ Month-to-date spend: $234.56 across 4 models.
  Sonnet: $145.23 (62%) | GLM-4.7: $0.00 (0%) | Opus: $67.82 (29%) | DeepSeek: $21.51 (9%)
  Projected end-of-month: $890.34

> Which subscription should I max out first?
→ Recommendation: Cursor Pro
  Current usage: 487/500 (97%), resets in 8d 3h
  Strategy: Use remaining 13 requests in next 8 days for maximum ROI
  Potential savings: $2.60 vs API pricing

> Show me the slowest workers
→ Slowest by avg response time (last hour):
  1. glm-charlie: 8.2s (processing complex architecture tasks)
  2. glm-alpha: 6.5s (large codebase context)
  3. glm-foxtrot: 4.1s (normal)
```

**Commands**:
```
> Spawn 3 Sonnet workers
→ Spawning 3 Sonnet workers...
  ✓ sonnet-juliet spawned (workspace: auto-discover)
  ✓ sonnet-kilo spawned (workspace: auto-discover)
  ✓ sonnet-lima spawned (workspace: auto-discover)
  Pool status: 12/9 workers (over target)

> Kill all idle workers
→ Found 1 idle worker: glm-delta
  Kill glm-delta? [Y/n] y
  ✓ glm-delta killed
  Pool status: 8/9 workers

> Assign po-7jb to Opus
→ Reassigning bead po-7jb from Sonnet to Opus...
  ✓ po-7jb reassigned to Opus 4.6
  Estimated cost: $4.50 (premium model for P0 task)

> Pause all workers for 5 minutes
→ Pausing 9 workers for 5 minutes...
  ✓ All workers paused until 14:28:45
  Resume with: "resume all workers"
```

**Analysis Queries**:
```
> Compare cost per task by model this week
→ Cost efficiency (last 7 days):
  GLM-4.7:     $0.00/task (free tier, 124 tasks)
  Haiku 4.5:   $0.08/task (fast, 89 tasks)
  DeepSeek V3: $0.12/task (budget, 45 tasks)
  Sonnet 4.5:  $2.34/task (premium, 67 tasks)
  Opus 4.6:    $8.75/task (ultra-premium, 12 tasks)
  
> Show me failed tasks in last hour
→ No failed tasks in last hour
  Success rate: 100% (24/24 completed)
  Last failure: bd-3mk at 13:08 (timeout, retried successfully)
```

---

### How It Works

#### 1. Natural Language Processing
The command bar uses a **restricted Claude Code instance** as the backend:

```python
class CommandProcessor:
    def __init__(self):
        self.agent = ClaudeCode(
            model="claude-sonnet-4.5",  # Fast, cost-effective
            tools=[
                "read_control_panel_state",  # Read-only dashboard data
                "execute_action",            # Spawn/kill/assign workers
                "query_database",            # Historical data
                "calculate_metrics"          # Aggregations
            ],
            max_tokens=1000,  # Keep responses concise
            system_prompt=CONTROL_PANEL_PROMPT
        )
    
    async def process(self, user_input: str) -> str:
        """Process user input and return conversational response"""
        context = self.get_dashboard_context()
        response = await self.agent.run(
            prompt=f"User question: {user_input}\n\nCurrent state:\n{context}"
        )
        return response
```

#### 2. Restricted Tool Set

The agent only has access to **control panel-specific tools**:

**Read-Only Tools**:
- `get_worker_status()` - Current worker pool state
- `get_task_queue()` - Ready beads across workspaces
- `get_subscription_usage()` - Quota tracking
- `get_cost_analytics()` - Spending data
- `get_activity_log(hours=1)` - Recent events
- `query_history(sql)` - Historical queries (read-only)

**Action Tools** (require confirmation for destructive ops):
- `spawn_worker(type, count, workspace=None)`
- `kill_worker(session_name, confirm=True)`
- `assign_task(bead_id, model)`
- `pause_workers(duration_minutes)`
- `resume_workers()`

**Analysis Tools**:
- `calculate_cost_per_task(model, timeframe)`
- `compare_models(metric, timeframe)`
- `predict_month_end_cost()`
- `recommend_subscription_optimization()`

#### 3. Context Injection

Each command gets the current dashboard state as context:

```json
{
  "workers": {
    "total": 9,
    "healthy": 8,
    "idle": 1,
    "sessions": [...]
  },
  "tasks": {
    "ready": 47,
    "in_progress": 9,
    "queue": [...]
  },
  "subscriptions": {
    "claude_pro": {"used": 328, "limit": 500, "resets_in": "16d 9h"},
    "cursor_pro": {"used": 487, "limit": 500, "resets_in": "8d 3h"}
  },
  "costs": {
    "today": 12.43,
    "month": 234.56,
    "projected": 890.34
  }
}
```

#### 4. Response Formatting

Responses are automatically formatted for TUI display:
- **Tables** for comparisons
- **Progress bars** for usage/costs
- **Checkmarks** for confirmations
- **Inline actions** when appropriate

#### 5. Safety Features

**Confirmation for destructive operations**:
```
> Kill all workers
→ ⚠️  DESTRUCTIVE OPERATION
  This will kill 9 active workers processing beads.
  Type "yes" to confirm, or "cancel" to abort: _
```

**Rate limiting**:
- Max 10 commands/minute (prevent API abuse)
- Cost tracking for agent invocations

**Audit logging**:
- All commands logged with timestamp, user, action, outcome

---

### Command Bar States

#### Inactive (Default)
```
[Q]uit [?]Help [:] Command [Tab]Panel
```

#### Active (`:` pressed)
```
┌─ COMMAND INPUT ─────────────────────────────────────────┐
│ > _                                                      │
└──────────────────────────────────────────────────────────┘
```

#### Processing
```
┌─ COMMAND INPUT ─────────────────────────────────────────┐
│ > Show me worker health                  ⏳ Processing...│
└──────────────────────────────────────────────────────────┘
```

#### Response Display
```
┌─ RESPONSE ──────────────────────────────────────────────┐
│ Worker health (9 workers):                              │
│ ✓ Healthy: 8 (89%)  ◐ Idle: 1 (11%)  ✗ Unhealthy: 0    │
│ Avg response time: 3.2s | Success rate: 98.5%           │
│                                              [Esc] Close │
└──────────────────────────────────────────────────────────┘
```

---

### Example Conversation Flow

```
User: Why is my cost so high today?
Assistant: Your cost today ($12.43) is 45% higher than average ($8.50/day).
           Main driver: 3 Opus tasks ($8.24, 66% of today's spend)
           These were P0 priority tasks assigned to premium model.
           Recommendation: Review task value scoring to ensure Opus
           is only used for truly critical work.

User: Show me those Opus tasks
Assistant: Opus tasks today:
           1. po-3h3 (14:15) - Compare LLM models - $2.75 (completed)
           2. bd-1xa (13:42) - Architecture refactor - $3.24 (in progress)
           3. po-1to (12:08) - Orchestrator analysis - $2.25 (completed)
           
           All were P0 priority, justified for Opus assignment.

User: Switch bd-1xa to Sonnet
Assistant: Reassigning bd-1xa from Opus to Sonnet...
           ⚠️  Warning: Task is 35% complete on Opus.
           Switching models mid-task may cause context loss.
           Continue? [y/N] y
           ✓ bd-1xa reassigned to Sonnet 4.5
           Estimated savings: $2.10 (remaining work)

User: Thanks
Assistant: You're welcome! Cost reduced by $2.10.
           Updated projection: $890.34 → $888.24 month-end.
```

---

### Benefits

1. **Discoverability**: Users don't need to memorize shortcuts
2. **Complex Queries**: Natural language handles multi-step analysis
3. **Context-Aware**: Agent sees full dashboard state
4. **Learning**: New users learn by asking questions
5. **Efficiency**: Faster than navigating menus for power users
6. **Transparency**: Agent explains decisions and recommendations

---

### Configuration

```yaml
# control-panel-config.yaml
command_interface:
  enabled: true
  model: claude-sonnet-4.5  # Fast, balanced
  max_tokens: 1000
  activation_key: ":"
  
  # Tool restrictions
  allow_spawn: true
  allow_kill: true
  require_confirmation:
    - kill_worker
    - pause_workers
    - kill_all_workers
  
  # Rate limits
  max_commands_per_minute: 10
  max_cost_per_hour: 1.00  # $1/hr for agent calls
  
  # Response formatting
  use_tables: true
  use_progress_bars: true
  use_colors: true
  max_response_lines: 10  # Truncate long responses
```

---

### Future Enhancements

1. **Multi-turn Conversations**: Remember context across commands
2. **Suggestions**: Auto-suggest based on current state ("glm-delta is idle, spawn more workers?")
3. **Scripting**: Save common command sequences ("alias spawn-sonnet-team = spawn 5 sonnet workers")
4. **Voice Input**: Speak commands instead of typing
5. **Predictive Text**: Autocomplete common queries
6. **Help Examples**: Show example commands for current view
7. **Learning Mode**: Agent learns from user patterns, suggests optimizations

This conversational interface transforms the control panel from a **monitoring tool** into an **intelligent assistant** for managing distributed agent workloads.
