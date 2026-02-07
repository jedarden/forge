# ADR 0004: Tool-Based Conversational Interface

**Date**: 2026-02-07
**Status**: Accepted
**Deciders**: Jed Arden, Claude Sonnet 4.5

---

## Context

Users need to control FORGE (switch views, spawn workers, filter tasks, etc.) without memorizing complex keyboard shortcuts. Initial design used hotkeys (`h` for help, `q` for quit, `r` for refresh, etc.), but this has discoverability and usability issues.

**Problems with hotkey-only approach**:
- Users must memorize dozens of shortcuts
- Not discoverable (requires help menu)
- Limited expressiveness (simple actions only)
- Difficult to combine multiple actions
- Poor accessibility

**Opportunity**: FORGE already has a conversational interface (`:` key activation) with a headless LLM backend. We can leverage this for natural language control instead of requiring hotkeys.

---

## Decision

Use **tool-based conversational interface as primary control method** where natural language commands are translated to structured tool calls by a headless LLM, which FORGE then executes.

**Hotkeys remain available as optional shortcuts** for power users, but discovery and primary interaction happens through natural language chat.

**Architecture**:
```
User types: "Show me costs for the last week"
    ↓
Headless LLM (Claude/GPT with tool definitions)
    ↓
Tool call: show_costs(period="last_week")
    ↓
FORGE executes: Switch to cost view, filter to 7 days
    ↓
User sees: Cost dashboard filtered to last week
```

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
┌─────────────────────────────────────────────────┐
│            User types command                    │
│         "Show me P0 tasks and costs"            │
└────────────────────┬────────────────────────────┘
                     │
                     ↓
┌─────────────────────────────────────────────────┐
│         Conversational Interface Layer           │
│  - Parse input via headless LLM                 │
│  - LLM has access to tool definitions           │
│  - Returns structured tool call(s)              │
└────────────────────┬────────────────────────────┘
                     │
                     ↓
┌─────────────────────────────────────────────────┐
│            Tool Execution Engine                 │
│  - Validate tool calls                          │
│  - Execute in sandbox                           │
│  - Return results                               │
│  - Handle errors gracefully                     │
└────────────────────┬────────────────────────────┘
                     │
                     ↓
┌─────────────────────────────────────────────────┐
│              FORGE TUI Updates                   │
│  - Switch views                                 │
│  - Update panels                                │
│  - Display results                              │
│  - Show tool execution status                   │
└─────────────────────────────────────────────────┘
```

### Example Flow

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

## Tool Definition Format

**Example Tool Schema**:
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
