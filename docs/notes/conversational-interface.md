# Conversational Command Interface

**Feature**: Natural language CLI input for control panel interaction

---

## Overview

The **Conversational Command Interface** transforms the control panel from a traditional keyboard-shortcut-driven TUI into an **intelligent assistant** that understands natural language queries and commands.

Instead of memorizing shortcuts, users can:
- Ask questions: "Why is glm-delta idle?"
- Issue commands: "Spawn 3 Sonnet workers"
- Request analysis: "Compare cost per task by model this week"
- Get recommendations: "Which subscription should I max out?"

---

## Architecture

### Backend: Restricted Coding Agent

Under the hood, the command interface uses a **restricted Claude Code (or OpenCode) instance** with limited tool access:

```
┌─────────────────────────────────────────────────────────────┐
│ User Input (Natural Language)                               │
│ > "Why is my cost so high today?"                           │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│ Command Processor                                           │
│ - Inject dashboard context (workers, tasks, costs, etc.)    │
│ - Rate limiting (10 commands/min)                           │
│ - Safety checks (confirm destructive ops)                   │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│ Restricted Coding Agent (Claude Code/OpenCode)              │
│ Model: claude-sonnet-4.5 (fast, cost-effective)             │
│ Max Tokens: 1000 (concise responses)                        │
│                                                              │
│ Available Tools (RESTRICTED):                               │
│ ✓ read_control_panel_state()                                │
│ ✓ execute_action(action, params)                            │
│ ✓ query_database(sql)  [read-only]                          │
│ ✓ calculate_metrics(metric, timeframe)                      │
│                                                              │
│ Forbidden Tools:                                             │
│ ✗ file_system_access                                        │
│ ✗ arbitrary_code_execution                                  │
│ ✗ external_api_calls                                        │
│ ✗ database_mutations (read-only queries only)               │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│ Response Formatter                                          │
│ - Markdown → TUI formatting                                 │
│ - Tables, progress bars, colors                             │
│ - Truncate to fit terminal                                  │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│ TUI Display (Response Box)                                  │
│ ┌───────────────────────────────────────────────────────┐   │
│ │ Your cost today ($12.43) is 45% higher than avg.     │   │
│ │ Main driver: 3 Opus tasks ($8.24, 66% of spend)      │   │
│ │ Recommendation: Review task value scoring            │   │
│ │                                            [Esc] Close│   │
│ └───────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

---

## Tool Definitions

### Read-Only Tools

#### `get_worker_status()`
Returns current worker pool state:
```json
{
  "total": 9,
  "healthy": 8,
  "idle": 1,
  "unhealthy": 0,
  "sessions": [
    {
      "name": "glm-alpha",
      "type": "GLM-4.7",
      "workspace": "/home/coder/ardenone-cluster",
      "status": "executing",
      "uptime": "12m",
      "last_activity": "30s ago",
      "beads_completed": 3
    }
  ]
}
```

#### `get_task_queue(workspace=None, priority=None)`
Returns ready beads:
```json
{
  "total_ready": 47,
  "in_progress": 9,
  "beads": [
    {
      "id": "po-7jb",
      "priority": "P0",
      "title": "Research TUI frameworks",
      "workspace": "/home/coder/research/control-panel",
      "estimated_tokens": 45000,
      "assigned_model": "Sonnet 4.5"
    }
  ]
}
```

#### `get_subscription_usage()`
Returns quota tracking:
```json
{
  "claude_pro": {
    "used": 328,
    "limit": 500,
    "percentage": 66,
    "resets_in": "16d 9h",
    "cost_today": 0.00,
    "recommendation": "on-pace"
  }
}
```

#### `get_cost_analytics(timeframe="today")`
Returns spending data:
```json
{
  "timeframe": "today",
  "total_cost": 12.43,
  "by_model": {
    "sonnet-4.5": 4.17,
    "opus-4.6": 8.24,
    "glm-4.7": 0.00
  },
  "by_priority": {
    "P0": 8.41,
    "P1": 3.12,
    "P2": 0.90
  }
}
```

#### `get_activity_log(hours=1, filter=None)`
Returns recent events:
```json
{
  "events": [
    {
      "timestamp": "2026-02-07T14:23:45Z",
      "type": "spawn",
      "session": "glm-india",
      "workspace": "/home/coder"
    },
    {
      "timestamp": "2026-02-07T14:23:18Z",
      "type": "close",
      "bead_id": "bd-2mk",
      "worker": "glm-delta",
      "duration": "8m"
    }
  ]
}
```

#### `query_history(sql)`
Read-only database queries:
```sql
SELECT model, AVG(cost_per_task), COUNT(*)
FROM tasks
WHERE completed_at > NOW() - INTERVAL '7 days'
GROUP BY model;
```

**Safety**: SQL is validated and restricted to SELECT only, no mutations.

---

### Action Tools (Require Confirmation)

#### `spawn_worker(type, count=1, workspace=None)`
Spawn new workers:
```python
spawn_worker(type="sonnet", count=3)
# Returns: ["sonnet-juliet", "sonnet-kilo", "sonnet-lima"]
```

**Safety**: Auto-confirms if count ≤ 2, prompts for count > 2

#### `kill_worker(session_name, confirm=True)`
Kill a worker:
```python
kill_worker("glm-delta", confirm=True)
# Prompts: "Kill glm-delta? [Y/n]"
```

**Safety**: Always requires confirmation for manual kills

#### `assign_task(bead_id, model)`
Reassign task to different model:
```python
assign_task("po-7jb", model="opus-4.6")
# Returns: {"status": "reassigned", "old_model": "sonnet-4.5", "new_model": "opus-4.6"}
```

**Safety**: Warns if reassigning mid-execution

#### `pause_workers(duration_minutes=None)`
Pause all workers:
```python
pause_workers(duration_minutes=5)
# Returns: {"paused": 9, "resume_at": "2026-02-07T14:28:45Z"}
```

**Safety**: Requires confirmation if duration > 10 minutes

#### `resume_workers()`
Resume paused workers:
```python
resume_workers()
# Returns: {"resumed": 9}
```

---

### Analysis Tools

#### `calculate_cost_per_task(model, timeframe="7d")`
Cost efficiency metrics:
```python
calculate_cost_per_task(model="sonnet-4.5", timeframe="7d")
# Returns: {"cost_per_task": 2.34, "task_count": 67}
```

#### `compare_models(metric, timeframe="7d")`
Model comparison:
```python
compare_models(metric="cost_per_task", timeframe="7d")
# Returns table of all models with metric values
```

#### `predict_month_end_cost()`
Cost projection:
```python
predict_month_end_cost()
# Returns: {
#   "current": 234.56,
#   "projected": 890.34,
#   "days_remaining": 23,
#   "daily_burn_rate": 28.50
# }
```

#### `recommend_subscription_optimization()`
Optimization suggestions:
```python
recommend_subscription_optimization()
# Returns: {
#   "cursor_pro": {
#     "action": "max_out",
#     "urgency": "high",
#     "remaining": 13,
#     "resets_in": "8d 3h",
#     "potential_savings": 2.60
#   }
# }
```

---

## System Prompt

The agent is initialized with a specialized system prompt:

```
You are the conversational interface for a distributed worker control panel.

Context:
- The user manages a pool of coding agents (workers) across multiple workspaces
- Workers process "beads" (tasks) using different LLM models (Sonnet, Opus, GLM, etc.)
- The system optimizes costs across subscriptions and pay-per-token APIs
- You have access to real-time dashboard state and historical data

Your role:
- Answer questions about worker status, costs, tasks, and subscriptions
- Execute commands safely (confirm destructive operations)
- Provide analysis and recommendations
- Be concise (max 5 sentences unless asked for details)
- Use tables, progress bars, and formatting when helpful
- Explain your reasoning when making recommendations

Available tools: [tool definitions here]

Response format:
- Use markdown for formatting
- Tables for comparisons
- ✓/✗/◐ for status indicators
- Progress bars: ████▌ 66%
- Keep responses under 10 lines (TUI space is limited)

Safety rules:
- Always confirm before killing workers or pausing for >10min
- Explain cost implications for model changes
- Warn about context loss when reassigning in-progress tasks
- Rate limit: User can run max 10 commands/minute
```

---

## Example Interactions

### Questions

**Worker Status**:
```
> Why is glm-delta idle?
→ glm-delta is idle because its workspace (botburrow-hub) has 0 ready beads.
  Last activity: 2m ago (completed bd-2mk in 8m)
  Worker is healthy, waiting for new tasks.

> Which workers are slowest?
→ Slowest workers (avg response time, last hour):
  1. glm-charlie: 8.2s - Processing complex architecture tasks
  2. glm-alpha: 6.5s - Large codebase context (ardenone-cluster)
  3. glm-foxtrot: 4.1s - Normal performance

  Overall avg: 3.2s
```

**Cost Analysis**:
```
> Why is my cost so high today?
→ Cost today: $12.43 (↑45% vs avg $8.50/day)
  Main driver: 3 Opus tasks = $8.24 (66% of spend)

  Opus tasks:
  • po-3h3 (P0) - LLM comparison - $2.75
  • bd-1xa (P0) - Architecture refactor - $3.24
  • po-1to (P0) - Orchestrator analysis - $2.25

  All were P0 (critical), justified for Opus.

> Show me cost per task by model
→ Cost efficiency (last 7 days):

  Model       │ Cost/Task │ Tasks │ Total
  ────────────┼───────────┼───────┼───────
  GLM-4.7     │  $0.00    │  124  │ $0.00
  Haiku       │  $0.08    │   89  │ $7.12
  DeepSeek V3 │  $0.12    │   45  │ $5.40
  Sonnet      │  $2.34    │   67  │ $156.78
  Opus        │  $8.75    │   12  │ $105.00
```

**Subscription Optimization**:
```
> Which subscription should I max out first?
→ Recommendation: Cursor Pro

  Current: 487/500 (97%) | Resets: 8d 3h
  Action: Use 13 remaining requests in next 8 days

  Strategy:
  1. Assign P1 tasks to Cursor Pro workers
  2. Estimated savings: $2.60 vs API pricing
  3. After maxed out, switch to Claude Pro (66% used)

> Am I on track with Claude Pro usage?
→ Claude Pro: 328/500 (66%) | Resets: 16d 9h

  ✓ On-Pace (should be at ~65% by now)
  Current rate: 21 requests/day
  Remaining: 172 requests over 16 days = 11/day

  No action needed.
```

**Task Queue**:
```
> Show me blocked tasks
→ Found 3 blocked tasks:

  • bd-5kx (P2) - blocked by bd-1dp (worker spawning fix)
  • bd-6mn (P3) - blocked by bd-5kx (performance optimization)
  • bd-9pl (P4) - blocked by bd-3mk (documentation update)

  Unblock by completing: bd-1dp, bd-5kx, bd-3mk

> What's the highest priority task not being worked on?
→ Highest priority ready task:
  po-4gr (P0) - "Design subscription optimization"

  Currently unassigned (no worker)
  Recommended model: Sonnet 4.5
  Estimated cost: $4.50

  Assign with: "assign po-4gr to sonnet"
```

---

### Commands

**Worker Management**:
```
> Spawn 3 Sonnet workers
→ Spawning 3 Sonnet workers...
  ✓ sonnet-juliet (workspace: auto-discovered /home/coder/ardenone-cluster)
  ✓ sonnet-kilo (workspace: auto-discovered /home/coder/botburrow-agents)
  ✓ sonnet-lima (workspace: auto-discovered /home/coder/research/control-panel)

  Pool status: 12/9 workers (3 over target)

> Kill all idle workers
→ Found 1 idle worker: glm-delta
  Kill glm-delta? [Y/n] y

  ✓ glm-delta killed
  Pool status: 8/9 workers

> Pause all workers for 10 minutes
→ Pausing 9 workers for 10 minutes...
  ✓ All workers paused until 14:33:45

  Resume automatically at 14:33:45
  Or manually: "resume all workers"
```

**Task Assignment**:
```
> Assign po-7jb to Opus
→ Reassigning po-7jb from Sonnet to Opus...

  Task: "Research TUI frameworks" (P0)
  Old model: Sonnet 4.5 ($4.50)
  New model: Opus 4.6 ($22.50)

  Cost increase: +$18.00 for premium model
  Continue? [y/N] y

  ✓ po-7jb reassigned to Opus

> Route all P0 tasks to Opus
→ Found 5 P0 tasks, 2 already on Opus.
  Reassigning 3 tasks to Opus:

  • po-1to: Sonnet → Opus (+$18.00)
  • po-3h3: Sonnet → Opus (+$18.00)
  • po-4gr: GLM → Opus (+$22.50)

  Total cost increase: +$58.50
  Continue? [y/N]
```

**Bulk Operations**:
```
> Assign top 3 P0 tasks to Opus, rest to Sonnet
→ Processing bulk assignment...

  Top 3 P0 tasks → Opus:
  ✓ po-7jb → Opus (was: Sonnet, +$18.00)
  ✓ po-1to → Opus (was: GLM, +$22.50)
  ✓ po-3h3 → Opus (was: GLM, +$22.50)

  Remaining 2 P0 tasks → Sonnet:
  ✓ po-4gr → Sonnet (was: GLM, +$4.50)
  ✓ po-1oh → Sonnet (was: Haiku, +$3.90)

  Total cost impact: +$71.40
```

---

## Response Formatting

### Tables

Input: `compare models cost efficiency`

Output:
```
Model       │ Cost/Task │ Tasks │ Success Rate │ Avg Time
────────────┼───────────┼───────┼──────────────┼─────────
GLM-4.7     │  $0.00    │  124  │  97.5%       │  4m 12s
Haiku       │  $0.08    │   89  │  99.1%       │  2m 08s
DeepSeek V3 │  $0.12    │   45  │  95.2%       │  3m 45s
Sonnet      │  $2.34    │   67  │  98.9%       │  4m 32s
Opus        │  $8.75    │   12  │ 100.0%       │  5m 18s
```

### Progress Bars

Input: `show subscription usage`

Output:
```
Claude Pro:   ████████▌        66%  (328/500, resets 16d 9h)
ChatGPT Plus: ██▌              30%  (12/40, resets 23d 14h)
Cursor Pro:   ███████████████▌ 97%  (487/500, resets 8d 3h)  ⚠️  MAX OUT
DeepSeek:     Pay-per-use      N/A  ($0.02 today)
```

### Status Indicators

```
✓ Completed successfully
✗ Failed
◐ In progress / Idle
⚠️  Warning
⏳ Processing
●  Active/Healthy
```

### Truncation

Responses exceeding 10 lines show truncation:

```
> List all workers
→ Worker pool (9 total):
  1. glm-alpha (GLM-4.7) - ardenone-cluster - ●EXEC 12m
  2. glm-bravo (GLM-4.7) - claude-config - ●EXEC 8m
  3. glm-charlie (GLM-4.7) - botburrow-agents - ●EXEC 15m
  4. glm-delta (GLM-4.7) - botburrow-hub - ◐IDLE 2m
  5. glm-echo (GLM-4.7) - leaderboard - ●EXEC 6m
  ... 4 more (show all with "list all workers --full")
```

---

## Safety & Limits

### Confirmation Prompts

Destructive operations require confirmation:

```
> Kill all workers
→ ⚠️  DESTRUCTIVE OPERATION
  This will kill 9 active workers, interrupting 9 in-progress beads.

  Type "yes" to confirm, "cancel" to abort: _
```

### Rate Limiting

```
> [11th command in 1 minute]
→ ⚠️  RATE LIMIT EXCEEDED
  Max 10 commands/minute
  Next command available in: 42s
```

### Cost Limits

```
> Assign all tasks to Opus
→ ⚠️  COST ALERT
  This operation would cost $456.00 (47 tasks × $9.70 avg)
  Daily budget limit: $50.00

  Recommend: Route only P0 tasks to Opus (5 tasks, $48.75)
  Continue anyway? [y/N]
```

### Audit Logging

All commands are logged:

```json
{
  "timestamp": "2026-02-07T14:25:30Z",
  "user": "human",
  "command": "spawn 3 sonnet workers",
  "agent_response": "Spawned sonnet-juliet, sonnet-kilo, sonnet-lima",
  "actions": [
    {"type": "spawn", "session": "sonnet-juliet", "workspace": "/home/coder/ardenone-cluster"},
    {"type": "spawn", "session": "sonnet-kilo", "workspace": "/home/coder/botburrow-agents"},
    {"type": "spawn", "session": "sonnet-lima", "workspace": "/home/coder/research/control-panel"}
  ],
  "cost": 0.015,
  "duration_ms": 1250
}
```

Log location: `~/.control-panel/command-audit.jsonl`

---

## Configuration

```yaml
# control-panel-config.yaml
conversational_interface:
  enabled: true

  # Agent backend
  agent:
    type: claude-code  # or opencode, aider
    model: claude-sonnet-4.5
    max_tokens: 1000
    temperature: 0.2  # Deterministic responses

  # UI settings
  activation_key: ":"
  deactivation_key: "Esc"
  history_size: 100  # Command history
  show_thinking: false  # Hide agent reasoning steps

  # Safety & limits
  rate_limit:
    max_per_minute: 10
    max_per_hour: 100

  cost_limit:
    max_per_hour: 1.00  # $1/hr for agent calls
    max_per_day: 10.00

  confirmations:
    required_for:
      - kill_worker
      - kill_all_workers
      - pause_workers  # if duration > 10min
      - high_cost_operations  # if cost > $10

  # Response formatting
  formatting:
    use_tables: true
    use_progress_bars: true
    use_colors: true
    max_response_lines: 10
    truncate_long_lists: true

  # Audit logging
  audit:
    enabled: true
    log_file: ~/.control-panel/command-audit.jsonl
    log_level: all  # all | commands_only | errors_only
```

---

## Implementation

### Python Class Structure

```python
from dataclasses import dataclass
from typing import Optional, List, Dict, Any
import subprocess
import json

@dataclass
class CommandResult:
    success: bool
    response: str
    actions: List[Dict[str, Any]]
    cost: float
    duration_ms: int

class ConversationalInterface:
    def __init__(self, config: Dict):
        self.config = config
        self.agent = self._initialize_agent()
        self.rate_limiter = RateLimiter(
            max_per_minute=config['rate_limit']['max_per_minute']
        )
        self.cost_tracker = CostTracker(
            max_per_hour=config['cost_limit']['max_per_hour']
        )
        self.audit_log = AuditLog(config['audit']['log_file'])

    def _initialize_agent(self):
        """Initialize restricted Claude Code instance"""
        return ClaudeCodeAgent(
            model=self.config['agent']['model'],
            tools=[
                ReadControlPanelStateTool(),
                ExecuteActionTool(),
                QueryDatabaseTool(),
                CalculateMetricsTool()
            ],
            system_prompt=SYSTEM_PROMPT,
            max_tokens=self.config['agent']['max_tokens']
        )

    async def process_command(self, user_input: str) -> CommandResult:
        """Process user command and return result"""
        # Rate limiting
        if not self.rate_limiter.allow():
            return CommandResult(
                success=False,
                response="⚠️ Rate limit exceeded. Try again in 30s.",
                actions=[],
                cost=0,
                duration_ms=0
            )

        # Get current dashboard context
        context = self._get_dashboard_context()

        # Invoke agent
        start_time = time.time()
        agent_response = await self.agent.run(
            prompt=f"User: {user_input}\n\nDashboard state:\n{json.dumps(context, indent=2)}"
        )
        duration_ms = int((time.time() - start_time) * 1000)

        # Parse response and extract actions
        result = self._parse_agent_response(agent_response)

        # Track cost
        self.cost_tracker.add(result.cost)

        # Audit log
        self.audit_log.write({
            'timestamp': datetime.now().isoformat(),
            'user_input': user_input,
            'response': result.response,
            'actions': result.actions,
            'cost': result.cost,
            'duration_ms': duration_ms
        })

        return result

    def _get_dashboard_context(self) -> Dict:
        """Gather current dashboard state for agent context"""
        return {
            'workers': get_worker_status(),
            'tasks': get_task_queue(),
            'subscriptions': get_subscription_usage(),
            'costs': get_cost_analytics('today'),
            'activity': get_activity_log(hours=1)
        }
```

### Textual Widget

```python
from textual.widgets import Input
from textual.containers import Container

class CommandInput(Container):
    """Conversational command input widget"""

    def __init__(self):
        super().__init__()
        self.input = Input(
            placeholder="Type : to activate command mode, then ask a question or issue a command...",
            id="command-input"
        )
        self.interface = ConversationalInterface(config)
        self.visible = False

    def on_key(self, event: Key) -> None:
        """Handle : key to activate command mode"""
        if event.key == "colon" and not self.visible:
            self.visible = True
            self.input.focus()
        elif event.key == "escape" and self.visible:
            self.visible = False
            self.input.value = ""

    async def on_input_submitted(self, event: Input.Submitted) -> None:
        """Process command when user hits Enter"""
        command = event.value
        self.input.value = ""

        # Show processing indicator
        self.show_processing()

        # Process command
        result = await self.interface.process_command(command)

        # Display response
        self.show_response(result.response)
```

---

## Future Enhancements

### 1. Multi-Turn Conversations
Maintain context across commands:

```
> What's my cost today?
→ $12.43

> Why is it so high?
→ [Agent remembers previous question about cost]
  Main driver: 3 Opus tasks ($8.24, 66% of spend)
```

### 2. Proactive Suggestions
Agent suggests actions based on state:

```
[Dashboard shows glm-delta idle for 5m]

💡 Suggestion: glm-delta has been idle for 5 minutes.
   Consider killing it to free resources, or spawning more workers?

   Reply "yes" to kill, or "spawn 2" to add workers.
```

### 3. Voice Input
Speak commands instead of typing:

```
🎤 [Voice mode activated]

"Show me worker health"
→ [Transcribed and processed]
```

### 4. Command Aliases
Save frequently-used commands:

```
> alias spawn-team = spawn 5 sonnet workers in ardenone-cluster

> spawn-team
→ Spawning 5 Sonnet workers in ardenone-cluster...
```

### 5. Scripting Mode
Chain multiple commands:

```
> script morning-routine
  spawn 9 glm workers
  assign all P0 to sonnet
  show cost projection

→ Script "morning-routine" saved.
  Run with: "run morning-routine"
```

### 6. Learning & Adaptation
Agent learns user preferences:

```
[After user repeatedly assigns P0 to Opus]

💡 I noticed you always assign P0 tasks to Opus.
   Should I auto-assign P0 → Opus going forward?
   [Y/n]
```

---

## Benefits

1. **Lower Learning Curve**: No need to memorize shortcuts
2. **Natural Interaction**: Ask questions like talking to a teammate
3. **Complex Operations**: Multi-step commands in natural language
4. **Context-Aware**: Agent sees full dashboard state
5. **Explainability**: Agent explains decisions and recommendations
6. **Efficiency**: Faster than clicking through menus
7. **Accessibility**: Voice input for hands-free operation
8. **Intelligent Assistance**: Proactive suggestions and learning

This transforms the control panel from a **monitoring tool** into an **intelligent co-pilot** for managing distributed agent workloads.

---

## Chat History & Context Management

### Multi-Turn Conversation Support

The command interface maintains conversation history to handle follow-up questions and contextual references.

#### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ Conversation History (Last 10 exchanges)                    │
│                                                              │
│ [1] User: What's my cost today?                             │
│     Agent: $12.43 (3 Opus tasks: $8.24, 66% of spend)       │
│                                                              │
│ [2] User: Why is it so high?                                │
│     Agent: [Refers to [1] - knows "it" = cost today]        │
│           Main driver: 3 Opus P0 tasks                       │
│                                                              │
│ [3] User: Show me those tasks                               │
│     Agent: [Refers to [2] - knows "those" = Opus tasks]     │
│           1. po-3h3 - $2.75 | 2. bd-1xa - $3.24 | ...       │
│                                                              │
│ [4] User: Reassign the second one to Sonnet                 │
│     Agent: [Refers to [3] - knows "second one" = bd-1xa]    │
│           Reassigning bd-1xa: Opus → Sonnet                  │
└─────────────────────────────────────────────────────────────┘
```

#### Context Window Management

```python
class ConversationHistory:
    def __init__(self, max_turns=10):
        self.history = []
        self.max_turns = max_turns
        self.dashboard_snapshots = {}  # State at each turn

    def add_exchange(self, user_input: str, agent_response: str, 
                     dashboard_state: Dict):
        """Add a new exchange to history"""
        self.history.append({
            'turn': len(self.history) + 1,
            'user': user_input,
            'agent': agent_response,
            'timestamp': datetime.now().isoformat(),
            'dashboard_state': dashboard_state
        })

        # Trim to max_turns
        if len(self.history) > self.max_turns:
            self.history.pop(0)

    def get_context(self) -> str:
        """Format history for agent context injection"""
        context_lines = []
        for exchange in self.history:
            context_lines.append(f"[Turn {exchange['turn']}]")
            context_lines.append(f"User: {exchange['user']}")
            context_lines.append(f"Assistant: {exchange['agent']}")
            context_lines.append("")
        
        return "\n".join(context_lines)

    def clear(self):
        """Clear history (user command: 'clear history')"""
        self.history = []
```

#### Contextual References Supported

**Pronouns**:
```
> What's my cost today?
→ $12.43

> Why is it so high?
→ [Agent knows "it" refers to cost from previous question]
  Main driver: 3 Opus tasks ($8.24, 66% of spend)
```

**Anaphora (references)**:
```
> Show me idle workers
→ Found 2: glm-delta, glm-echo

> Kill them both
→ [Agent knows "them" = glm-delta and glm-echo]
  Kill 2 workers? [Y/n]
```

**Follow-up questions**:
```
> Compare models by cost
→ [Table showing Opus $8.75, Sonnet $2.34, GLM $0.00, etc.]

> Which one is most cost-effective?
→ [Agent knows "which one" = asking about models from table]
  Most cost-effective: GLM-4.7 ($0.00/task, free tier)
  Best value for paid: DeepSeek V3 ($0.12/task)
```

**Implicit context**:
```
> Spawn 3 Sonnet workers
→ Spawned sonnet-juliet, sonnet-kilo, sonnet-lima

> Actually make it 5 total
→ [Agent knows "it" = Sonnet workers, "5 total" = 2 more needed]
  Spawning 2 additional Sonnet workers...
  sonnet-mike, sonnet-november spawned
  Total Sonnet workers: 5
```

#### History Visibility in UI

User can view conversation history:

```
> show history
→ Conversation history (last 10 exchanges):

  [1] 14:20:15 - What's my cost today?
      → $12.43 (3 Opus tasks)

  [2] 14:20:42 - Why is it so high?
      → Main driver: 3 Opus P0 tasks

  [3] 14:21:08 - Show me those tasks
      → po-3h3 ($2.75), bd-1xa ($3.24), po-1to ($2.25)

  [4] 14:21:35 - Reassign bd-1xa to Sonnet
      → ✓ Reassigned, saved $2.10

  Clear history: "clear history"
```

#### History Persistence

```yaml
# control-panel-config.yaml
conversational_interface:
  history:
    max_turns: 10  # Keep last 10 exchanges
    persist: true  # Save to disk
    persist_file: ~/.control-panel/conversation-history.jsonl
    auto_clear_on_restart: false  # Resume from previous session
```

Persisted format (JSONL):
```jsonl
{"turn":1,"timestamp":"2026-02-07T14:20:15Z","user":"What's my cost today?","agent":"$12.43","dashboard_state":{...}}
{"turn":2,"timestamp":"2026-02-07T14:20:42Z","user":"Why is it so high?","agent":"Main driver: 3 Opus tasks","dashboard_state":{...}}
```

---

## Tool Call Transparency & User Reactions

### Real-Time Tool Call Visibility

When the agent invokes tools, the user sees **exactly what's happening** and can **react/interrupt** if needed.

#### Tool Call Display

```
┌─ COMMAND INPUT ─────────────────────────────────────────────────────┐
│ > Spawn 3 Sonnet workers                             ⏳ Processing...│
└──────────────────────────────────────────────────────────────────────┘

┌─ AGENT THINKING ────────────────────────────────────────────────────┐
│ 🔧 Calling tool: get_worker_status()                                │
│    → {"total": 9, "healthy": 8, "idle": 1}                          │
│                                                                      │
│ 🔧 Calling tool: spawn_worker(type="sonnet", count=3)               │
│    → Spawning workers...                                            │
│    → ✓ sonnet-juliet spawned (workspace: ardenone-cluster)          │
│    → ✓ sonnet-kilo spawned (workspace: botburrow-agents)            │
│    → ✓ sonnet-lima spawned (workspace: control-panel)               │
│                                                                      │
│ 🔧 Calling tool: get_worker_status()                                │
│    → {"total": 12, "healthy": 11, "idle": 1}                        │
│                                                                      │
│                                        [Esc] Cancel | [Enter] Continue│
└──────────────────────────────────────────────────────────────────────┘

┌─ RESPONSE ──────────────────────────────────────────────────────────┐
│ Spawned 3 Sonnet workers:                                           │
│ • sonnet-juliet (ardenone-cluster)                                  │
│ • sonnet-kilo (botburrow-agents)                                    │
│ • sonnet-lima (control-panel)                                       │
│ Pool status: 12/9 workers (3 over target)                           │
│                                                [Esc] Close           │
└──────────────────────────────────────────────────────────────────────┘
```

#### Tool Call Stream

```python
class ToolCallStream:
    """Real-time tool call display"""

    def __init__(self, ui_panel):
        self.ui_panel = ui_panel
        self.calls = []
        self.user_cancelled = False

    async def on_tool_call(self, tool_name: str, params: Dict) -> bool:
        """Called when agent invokes a tool"""
        # Display tool call
        self.ui_panel.add_line(f"🔧 Calling tool: {tool_name}({self._format_params(params)})")

        # Wait for user reaction
        if await self._is_destructive(tool_name):
            # Give user 2 seconds to cancel destructive operations
            self.ui_panel.add_line(f"   [Press Esc within 2s to cancel]")
            cancelled = await self._wait_for_cancel(timeout_ms=2000)
            if cancelled:
                self.user_cancelled = True
                return False  # Abort tool call

        return True  # Proceed with tool call

    async def on_tool_result(self, result: Any):
        """Called when tool returns result"""
        # Display result (truncated if long)
        formatted = self._format_result(result)
        self.ui_panel.add_line(f"   → {formatted}")

    def _is_destructive(self, tool_name: str) -> bool:
        """Check if tool is potentially destructive"""
        return tool_name in [
            'kill_worker',
            'kill_all_workers',
            'pause_workers',
            'assign_task'  # If reassigning mid-execution
        ]
```

#### User Reactions

**Cancel during execution**:
```
🔧 Calling tool: kill_all_workers()
   [Press Esc within 2s to cancel]

[User presses Esc]

→ ⚠️  CANCELLED BY USER
  Tool call aborted, no workers killed
```

**Interrupt tool sequence**:
```
🔧 Calling tool: spawn_worker(type="opus", count=10)
   → Spawning 10 Opus workers...
   → ✓ opus-alpha spawned ($22.50/task)
   → ✓ opus-bravo spawned ($22.50/task)
   → ✓ opus-charlie spawned ($22.50/task)

[User presses Esc]

→ ⚠️  INTERRUPTED (3/10 workers spawned)
  3 Opus workers created, 7 remaining cancelled
  Cost impact: ~$67.50/day for 3 workers
```

**Confirm before continuing**:
```
🔧 Calling tool: reassign_all_tasks(from_model="sonnet", to_model="opus")
   → This will reassign 24 tasks from Sonnet to Opus
   → Cost increase: +$432.00

⚠️  HIGH COST OPERATION DETECTED
   Proceed? [y/N] _

[User types 'n']

→ ✗ OPERATION CANCELLED
  No tasks reassigned
```

#### Tool Call Logging

All tool calls are logged for audit/debugging:

```json
{
  "timestamp": "2026-02-07T14:25:30Z",
  "command": "Spawn 3 Sonnet workers",
  "tool_calls": [
    {
      "tool": "get_worker_status",
      "params": {},
      "result": {"total": 9, "healthy": 8},
      "duration_ms": 15
    },
    {
      "tool": "spawn_worker",
      "params": {"type": "sonnet", "count": 3},
      "result": ["sonnet-juliet", "sonnet-kilo", "sonnet-lima"],
      "duration_ms": 3250
    },
    {
      "tool": "get_worker_status",
      "params": {},
      "result": {"total": 12, "healthy": 11},
      "duration_ms": 12
    }
  ],
  "user_cancelled": false,
  "total_duration_ms": 3277
}
```

Log location: `~/.control-panel/tool-call-audit.jsonl`

---

## Advanced Conversation Features

### 1. Streaming Responses

Show agent thinking in real-time:

```
┌─ AGENT RESPONSE ────────────────────────────────────────────────────┐
│ Let me analyze your cost breakdown...                               │
│                                                                      │
│ Looking at today's spending ($12.43):                               │
│ • Opus tasks account for 66% ($8.24)                               │
│ • All 3 Opus tasks were P0 priority ✓                              │
│ • Task value scoring seems appropriate                              │
│                                                                      │
│ Checking historical averages...                                     │
│ • Your 7-day avg is $8.50/day                                      │
│ • Today is 45% higher than usual                                    │
│                                                                      │
│ Recommendation: This is expected variance for P0-heavy days.        │
│ No action needed, but monitor for sustained high spend.             │
│                                                                      │
│                                                [Esc] Stop streaming │
└──────────────────────────────────────────────────────────────────────┘
```

### 2. Disambiguation

Agent asks for clarification when input is ambiguous:

```
> Kill the slow worker

→ ❓ CLARIFICATION NEEDED
  Found 3 workers with below-average speed:
  1. glm-charlie (8.2s avg response time)
  2. glm-alpha (6.5s avg)
  3. glm-foxtrot (4.1s avg)

  Which one? [1/2/3 or "all"] _
```

### 3. Suggestions

Agent proactively suggests next actions:

```
> Show me subscription status

→ Subscription usage:
  • Claude Pro: 66% (on-pace) ✓
  • Cursor Pro: 97% (resets in 8d) ⚠️

  💡 SUGGESTION:
     Cursor Pro is nearly maxed out (487/500).
     Use remaining 13 requests in next 8 days for max ROI.

     Try: "assign next 13 P1 tasks to cursor workers"
```

### 4. Macro Commands

Pre-defined command templates:

```
> run morning-routine

→ Executing macro "morning-routine":
  1. 🔧 spawn_worker(type="glm", count=9)
     → 9 GLM workers spawned ✓
  
  2. 🔧 assign_tasks(priority="P0", model="sonnet")
     → 5 P0 tasks assigned to Sonnet ✓
  
  3. 🔧 get_cost_analytics(timeframe="projected")
     → Month-end projection: $890.34 ✓

  Macro completed in 4.2s
```

### 5. Error Recovery

When tool calls fail, agent suggests fixes:

```
> Spawn 15 Sonnet workers

→ 🔧 Calling tool: spawn_worker(type="sonnet", count=15)
   → ✗ ERROR: Model worker limit reached (9/9 GLM workers)
   → Cannot spawn Sonnet workers, limit exceeded

  💡 SUGGESTED FIX:
     Kill some GLM workers first to free slots:
     • "kill all idle workers" (frees 1 slot)
     • "kill glm-delta glm-echo" (frees 2 slots)
     • "increase model limit to 15" (requires config change)

     Which approach? _
```

---

## Configuration

```yaml
# control-panel-config.yaml
conversational_interface:
  # ... existing config ...

  # Chat history
  history:
    enabled: true
    max_turns: 10
    persist: true
    persist_file: ~/.control-panel/conversation-history.jsonl
    auto_clear_on_restart: false
    show_dashboard_snapshots: false  # Include state in history view

  # Tool call transparency
  tool_visibility:
    enabled: true
    show_params: true
    show_results: true
    truncate_long_results: true
    max_result_length: 200

    # User reaction timeouts
    allow_cancel: true
    cancel_window_ms: 2000  # 2s to cancel destructive ops

    # Confirmation thresholds
    confirm_destructive: true
    confirm_high_cost: true  # If cost > $10
    confirm_bulk_operations: true  # If affecting >5 workers/tasks

  # Advanced features
  features:
    streaming_responses: true
    disambiguation: true
    suggestions: true
    macro_commands: true
    error_recovery_hints: true

  # Audit logging
  audit:
    log_tool_calls: true
    tool_call_log_file: ~/.control-panel/tool-call-audit.jsonl
```

---

## Summary

The conversational interface now includes:

1. **Chat History**: 10-turn memory for contextual follow-ups
2. **Tool Call Transparency**: Real-time visibility into agent actions
3. **User Reactions**: Cancel/interrupt during execution
4. **Streaming**: See agent thinking as it processes
5. **Disambiguation**: Clarification when input is ambiguous
6. **Suggestions**: Proactive next-action recommendations
7. **Error Recovery**: Helpful fixes when operations fail

These features transform the control panel into a **truly intelligent assistant** that the user can converse with naturally, while maintaining full transparency and control over all operations.

---

## Scrolling & Large Message Handling

### Response Panel Scrolling

When agent responses exceed the available screen space, the interface provides multiple scrolling mechanisms:

#### 1. Automatic Scrollable Panels

```
┌─ RESPONSE ──────────────────────────────────────────────────────────┐
│ Cost efficiency (last 7 days):                                      │
│                                                                      │
│ Model       │ Cost/Task │ Tasks │ Total    │ Success Rate          │
│─────────────┼───────────┼───────┼──────────┼───────────────────────│
│ GLM-4.7     │  $0.00    │  124  │    $0.00 │ 97.5%                 │
│ Haiku 4.5   │  $0.08    │   89  │   $7.12  │ 99.1%                 │
│ DeepSeek V3 │  $0.12    │   45  │   $5.40  │ 95.2%                 │
│ Sonnet 4.5  │  $2.34    │   67  │ $156.78  │ 98.9%                 │
│ Opus 4.6    │  $8.75    │   12  │ $105.00  │ 100.0%                │
│ GPT-4 Turbo │  $4.12    │   34  │ $140.08  │ 98.2%                 │
│ GPT-3.5     │  $0.15    │   56  │   $8.40  │ 94.1%                 │
│ Qwen 2.5    │  $0.18    │   23  │   $4.14  │ 96.5%                 │
│ Mistral     │  $0.95    │   18  │  $17.10  │ 97.8%                 │
│ Codestral   │  $1.25    │   15  │  $18.75  │ 99.3%                 │
│                                                                      │
│ ⇅ Scroll (10 of 15 models shown)            [↓] More [Esc] Close   │
└──────────────────────────────────────────────────────────────────────┘
```

**Keyboard Controls**:
- `↑/↓` or `j/k` - Scroll line by line
- `PgUp/PgDn` - Scroll page by page
- `Home/End` - Jump to top/bottom
- `g`/`G` - Vim-style: top/bottom
- Mouse scroll wheel - Natural scrolling

#### 2. Pagination for Very Long Responses

For responses exceeding 50 lines, automatic pagination:

```
┌─ RESPONSE (Page 1 of 3) ────────────────────────────────────────────┐
│ Worker activity analysis for last 24 hours:                         │
│                                                                      │
│ === glm-alpha (Workspace: ardenone-cluster) ===                     │
│ • Total beads processed: 23                                         │
│ • Success rate: 95.7% (22 completed, 1 failed)                      │
│ • Avg time per bead: 4m 32s                                         │
│ • Total compute time: 104m 16s                                      │
│ • Idle time: 12m 45s (10.9%)                                        │
│                                                                      │
│ Top 3 slowest tasks:                                                │
│ 1. bd-4xa - Architecture refactor (18m 45s)                         │
│ 2. bd-2mk - Database migration (12m 30s)                            │
│ 3. bd-7op - Complex algorithm (9m 15s)                              │
│                                                                      │
│ === glm-bravo (Workspace: claude-config) ===                        │
│ • Total beads processed: 18                                         │
│ • Success rate: 100.0% (18 completed)                               │
│ • Avg time per bead: 3m 12s                                         │
│                                                                      │
│ [Space] Next Page | [b] Previous | [q] Quit | [/] Search           │
└──────────────────────────────────────────────────────────────────────┘
```

**Navigation**:
- `Space` or `→` - Next page
- `b` or `←` - Previous page
- `1-9` - Jump to page N
- `/` - Search within response
- `n/N` - Next/previous search result

#### 3. Smart Truncation with Expansion

Long responses truncated with expand option:

```
┌─ RESPONSE ──────────────────────────────────────────────────────────┐
│ Found 47 ready beads across 8 workspaces:                           │
│                                                                      │
│ Workspace: ardenone-cluster (17 beads)                              │
│ • po-7jb (P0) - Research TUI frameworks                             │
│ • po-1to (P0) - Analyze orchestrators                               │
│ • bd-3h3 (P1) - Fix worker spawning                                 │
│ ... 14 more                                                          │
│                                                                      │
│ Workspace: claude-config (13 beads)                                 │
│ • po-3pv (P0) - Task value scoring                                  │
│ • bd-1dp (P1) - Health monitoring                                   │
│ ... 11 more                                                          │
│                                                                      │
│ Workspace: botburrow-agents (10 beads)                              │
│ ... (collapsed)                                                      │
│                                                                      │
│ [Enter] Expand All | [e] Expand Selected | [c] Collapse | [Esc] Close│
└──────────────────────────────────────────────────────────────────────┘
```

After pressing `Enter`:

```
┌─ RESPONSE (Expanded) ───────────────────────────────────────────────┐
│ Found 47 ready beads across 8 workspaces:                           │
│                                                                      │
│ Workspace: ardenone-cluster (17 beads)                              │
│ • po-7jb (P0) - Research TUI frameworks | Sonnet | 45K tokens       │
│ • po-1to (P0) - Analyze orchestrators | Sonnet | 38K tokens         │
│ • bd-3h3 (P1) - Fix worker spawning | GLM-4.7 | 15K tokens          │
│ • bd-2xa (P1) - Add health monitoring | GLM-4.7 | 22K tokens        │
│ • bd-5kx (P2) - Optimize performance | GLM-4.7 | 12K tokens         │
│ ... (scrollable list continues)                                     │
│                                                                      │
│ ⇅ Scroll (Showing 5 of 47) | [c] Collapse | [Esc] Close            │
└──────────────────────────────────────────────────────────────────────┘
```

#### 4. Incremental Loading for Real-Time Data

For streaming responses or live data:

```
┌─ AGENT RESPONSE (Loading...) ───────────────────────────────────────┐
│ Analyzing worker performance patterns...                            │
│                                                                      │
│ 🔍 Scanning last 7 days of activity logs...                         │
│ ✓ Processed 1,234 task completions                                  │
│ ✓ Analyzed 89 workers (current + historical)                        │
│ ✓ Calculated efficiency metrics                                     │
│                                                                      │
│ Key findings:                                                        │
│ 1. Workers in ardenone-cluster are 23% slower than average          │
│    Likely cause: Large codebase context (4.2M tokens)               │
│                                                                      │
│ 2. P0 tasks taking 2.3x longer than P2 tasks                        │
│    Expected - higher complexity justifies time                      │
│                                                                      │
│ 3. Opus workers have 100% success rate but cost 72x more            │
│    than GLM workers (98% success rate)                              │
│                                                                      │
│ ⏳ Generating recommendations... (streaming)                         │
│                                                                      │
│ [Esc] Stop | Auto-scroll: ON [a] to toggle                          │
└──────────────────────────────────────────────────────────────────────┘
```

**Auto-scroll behavior**:
- Enabled by default for streaming responses
- Automatically scrolls to bottom as new content arrives
- Toggle with `a` key
- Disabled if user manually scrolls up

#### 5. Multi-Column Scrolling for Wide Tables

For tables wider than screen:

```
┌─ RESPONSE ──────────────────────────────────────────────────────────┐
│ Worker comparison (all metrics):                                    │
│                                                                      │
│ Session  │Type  │Workspace│Status│Time│Beads│Succ%│AvgT ◀─────────│
│──────────┼──────┼─────────┼──────┼────┼─────┼─────┼─────┼─────────│
│ glm-alpha│GLM4.7│ardenone │●EXEC │12m │  23 │95.7%│4m32s│ ... ⇨    │
│ glm-bravo│GLM4.7│claude-c │●EXEC │ 8m │  18 │100% │3m12s│ ... ⇨    │
│ glm-charli│GLM4.7│botburro│●EXEC │15m │  15 │93.3%│5m08s│ ... ⇨    │
│                                                                      │
│ ⇅ Scroll vertically | ⇆ Scroll horizontally (←/→)                   │
│ [f] Fit to width | [w] Wrap columns | [Esc] Close                   │
└──────────────────────────────────────────────────────────────────────┘
```

Press `→` to scroll right:

```
┌─ RESPONSE ──────────────────────────────────────────────────────────┐
│ Worker comparison (all metrics):                                    │
│                                                                      │
│ ──────── Continued ───────│ Cost│Tokens │Memory│CPU%│Errors│  ◀───│
│ ─────────────────────────┼─────┼───────┼──────┼────┼──────┼───────│
│ ... ⇦  ardenone-cluster  │$4.17│ 347K  │2.1GB │ 45%│   0  │       │
│ ... ⇦  claude-config     │$2.34│ 234K  │1.8GB │ 38%│   0  │       │
│ ... ⇦  botburrow-agents  │$3.12│ 289K  │2.4GB │ 52%│   1  │       │
│                                                                      │
│ ⇅ Scroll vertically | ⇆ Scroll horizontally (←/→)                   │
│ [Home] Jump to start | [End] Jump to end | [Esc] Close              │
└──────────────────────────────────────────────────────────────────────┘
```

---

### Conversation History Scrolling

View and scroll through past exchanges:

```
┌─ CONVERSATION HISTORY ──────────────────────────────────────────────┐
│ Last 10 exchanges (showing 3 of 10):                                │
│                                                                      │
│ [8] 14:15:23                                                         │
│ User: What's my cost today?                                         │
│ Assistant: $12.43                                                   │
│     • Opus tasks: $8.24 (66%)                                       │
│     • Sonnet tasks: $4.17 (33%)                                     │
│     • GLM tasks: $0.00 (0%)                                         │
│                                                                      │
│ [9] 14:16:45                                                         │
│ User: Why is it so high?                                            │
│ Assistant: Main driver: 3 Opus P0 tasks                             │
│     45% higher than 7-day average ($8.50/day)                       │
│                                                                      │
│ [10] 14:17:12 (current)                                             │
│ User: Show me those Opus tasks                                      │
│ Assistant: [current response being displayed]                       │
│                                                                      │
│ ⇅ Scroll | [#] Jump to exchange | [c] Clear history | [Esc] Close  │
└──────────────────────────────────────────────────────────────────────┘
```

**Navigation**:
- `↑/↓` - Scroll through history
- `1-9` - Jump to exchange N
- `[` / `]` - Previous/next exchange
- `/` - Search history
- `c` - Clear history (with confirmation)

---

### Tool Call Panel Scrolling

When many tool calls are made:

```
┌─ AGENT THINKING ────────────────────────────────────────────────────┐
│ 🔧 get_worker_status()                                              │
│    → {"total": 9, "healthy": 8, "idle": 1}                          │
│                                                                      │
│ 🔧 get_task_queue(priority="P0")                                    │
│    → Found 5 P0 tasks across 3 workspaces                           │
│                                                                      │
│ 🔧 assign_task(bead_id="po-7jb", model="opus")                      │
│    → ✓ Assigned to Opus 4.6                                         │
│                                                                      │
│ 🔧 assign_task(bead_id="po-1to", model="opus")                      │
│    → ✓ Assigned to Opus 4.6                                         │
│                                                                      │
│ 🔧 assign_task(bead_id="po-3h3", model="sonnet")                    │
│    → ✓ Assigned to Sonnet 4.5                                       │
│                                                                      │
│ 🔧 assign_task(bead_id="po-4gr", model="sonnet")                    │
│    → ✓ Assigned to Sonnet 4.5                                       │
│                                                                      │
│ 🔧 get_cost_analytics(timeframe="projected")                        │
│    → Projected cost increase: +$71.40                               │
│                                                                      │
│ ⇅ Scroll (showing 8 of 12 tool calls) | Auto-scroll: ON [a]        │
│ [Esc] Cancel operation | [Enter] Continue                           │
└──────────────────────────────────────────────────────────────────────┘
```

**Auto-scroll for tool calls**:
- Enabled by default to show latest tool execution
- User can scroll up to review earlier calls
- Auto-scroll resumes when user returns to bottom
- Toggle with `a` key

---

### Smart Response Sizing

The response panel dynamically sizes based on content and available screen space:

#### Small Response (fits in 10 lines)
```
┌─ RESPONSE ──────────────────────────────────────────────────────────┐
│ Pool status: 9/9 workers                                            │
│ • Healthy: 8 (89%)                                                  │
│ • Idle: 1 (11%)                                                     │
│ • Unhealthy: 0                                                      │
│                                                                      │
│                                                        [Esc] Close   │
└──────────────────────────────────────────────────────────────────────┘
```

#### Medium Response (fits in 20 lines)
Expands to use more vertical space:
```
┌─ RESPONSE ──────────────────────────────────────────────────────────┐
│ [20 lines of content]                                               │
│                                                        [Esc] Close   │
└──────────────────────────────────────────────────────────────────────┘
```

#### Large Response (exceeds available space)
Scrollable panel with indicators:
```
┌─ RESPONSE ──────────────────────────────────────────────────────────┐
│ [Scrollable content]                                                │
│                                                                      │
│ ... (45 more lines) ...                                             │
│                                                                      │
│ ⇅ Scroll | Line 5 of 50 | [Esc] Close                              │
└──────────────────────────────────────────────────────────────────────┘
```

---

### Configuration

```yaml
# control-panel-config.yaml
conversational_interface:
  scrolling:
    # Response panel
    max_response_height: 30  # Max lines before scroll
    auto_scroll_streaming: true
    show_scroll_indicators: true
    scroll_speed: 3  # Lines per scroll action

    # Pagination
    auto_paginate_threshold: 50  # Lines before auto-paginate
    page_size: 30
    show_page_numbers: true

    # Truncation
    truncate_long_lists: true
    max_list_items: 10  # Items before "... N more"
    collapse_by_default: false

    # Tool call panel
    max_tool_panel_height: 25
    auto_scroll_tool_calls: true
    collapse_old_tool_calls: false  # Keep full history visible

    # History panel
    max_history_height: 25
    show_timestamps: true
    group_by_session: false

  # Keyboard shortcuts
  keybindings:
    scroll_up: ["k", "↑"]
    scroll_down: ["j", "↓"]
    page_up: ["PgUp", "Ctrl+b"]
    page_down: ["PgDn", "Ctrl+f", "Space"]
    top: ["g", "Home"]
    bottom: ["G", "End"]
    toggle_auto_scroll: ["a"]
    expand_all: ["Enter"]
    collapse_all: ["c"]
    search: ["/"]
    next_result: ["n"]
    prev_result: ["N"]
```

---

### Visual Scroll Indicators

#### Scroll Position Indicator
```
┌─ RESPONSE ──────────────────────────────────────────────────────────┐
│ [Content here]                                                  ▲   │
│                                                                 █   │
│                                                                 ┃   │
│                                                                 ┃   │
│                                                                 ┃   │
│                                                                 ┃   │
│                                                                 ┃   │
│                                                                 ┃   │
│                                                                 ▼   │
│                                                                      │
│ Line 15-45 of 120 | 37% | [PgDn] Next page                          │
└──────────────────────────────────────────────────────────────────────┘
```

#### Content Overflow Indicators
```
Top overflow (more content above):
┌─ RESPONSE ──────────────────────────────────────────────────────────┐
│ ▲▲▲ More content above ▲▲▲                                          │
│ [Visible content]                                                   │
└──────────────────────────────────────────────────────────────────────┘

Bottom overflow (more content below):
┌─ RESPONSE ──────────────────────────────────────────────────────────┐
│ [Visible content]                                                   │
│ ▼▼▼ More content below ▼▼▼                                          │
└──────────────────────────────────────────────────────────────────────┘

Both:
┌─ RESPONSE ──────────────────────────────────────────────────────────┐
│ ▲▲▲ More content above ▲▲▲                                          │
│ [Visible content]                                                   │
│ ▼▼▼ More content below ▼▼▼                                          │
└──────────────────────────────────────────────────────────────────────┘
```

---

### Mouse Support

For terminals with mouse support:

- **Scroll wheel**: Natural scrolling (up/down)
- **Click scrollbar**: Jump to position
- **Drag scrollbar**: Smooth scrolling
- **Click "More content" indicators**: Jump to that section
- **Click pagination buttons**: Navigate pages

---

## Summary

Scrolling for large chat messages supports:

1. **Automatic scrollable panels** - Standard up/down navigation
2. **Pagination** - For very long responses (50+ lines)
3. **Smart truncation** - Collapse/expand long lists
4. **Incremental loading** - Streaming responses with auto-scroll
5. **Multi-column scrolling** - Horizontal navigation for wide tables
6. **Visual indicators** - Scrollbars, position, overflow arrows
7. **Keyboard shortcuts** - Vim-style and standard navigation
8. **Mouse support** - Scroll wheel and click navigation
9. **Dynamic sizing** - Panel adapts to content size
10. **Context preservation** - Scroll position maintained across views

All scrolling behaviors are configurable to match user preferences.
