# FORGE Hotkey Reference

**Primary Interface**: Conversational chat (press `:`)
**Optional Shortcuts**: Hotkeys for common actions

---

## Philosophy

Hotkeys are **optional shortcuts** for power users. You don't need to memorize them - everything can be done via chat:

```
Chat:    "Show me worker status"     → switch_view("workers")
Hotkey:  W                           → switch_view("workers")
```

**When hotkeys are useful**:
- You already know the shortcut
- Maximum speed is needed (1 keystroke vs typing)
- You're navigating rapidly between views

**When chat is better**:
- You don't know/remember the hotkey
- Complex actions ("Show P0 tasks and spawn 2 workers")
- Discovering features ("What can I do?")
- First time doing something

---

## Discovering Hotkeys

The system teaches you hotkeys naturally:

```
User: "Show me the costs"
FORGE: → Executes show_costs()
       → Displays: "Cost view (Press C to return here quickly)"
```

After using chat a few times, you'll naturally learn shortcuts for actions you repeat often.

---

## Global Hotkeys

| Key | Action | Chat Equivalent |
|-----|--------|-----------------|
| `:` | Activate chat input | N/A (primary interface) |
| `?` or `h` | Show help overlay | "help" |
| `q` | Quit FORGE | "quit" or "exit" |
| `Esc` | Cancel current operation | N/A |
| `Ctrl+C` | Force quit | N/A |
| `Ctrl+L` | Clear screen/refresh | "refresh" |

---

## View Navigation

| Key | View | Chat Equivalent |
|-----|------|-----------------|
| `W` | Workers view | "show workers" |
| `T` | Tasks view | "show tasks" |
| `C` | Costs view | "show costs" |
| `M` | Metrics view | "show metrics" |
| `L` | Logs view | "show logs" |
| `O` | Overview (dashboard) | "show overview" |
| `Tab` | Cycle through views | "next view" |
| `Shift+Tab` | Reverse cycle | "previous view" |

---

## Worker Management

| Key | Action | Chat Equivalent |
|-----|--------|-----------------|
| `S` | Spawn worker (prompts for model) | "spawn worker" |
| `K` | Kill worker (prompts for selection) | "kill worker" |
| `R` | Restart selected worker | "restart worker" |
| `Ctrl+S` | Quick spawn (default model) | "spawn a worker" |

---

## Task Management

| Key | Action | Chat Equivalent |
|-----|--------|-----------------|
| `N` | Create new task | "create task" |
| `F` | Filter tasks (prompts for criteria) | "filter tasks" |
| `A` | Assign task to worker | "assign task" |
| `/` | Search tasks | "search for [query]" |
| `0-4` | Filter by priority (P0-P4) | "show P0 tasks" |

---

## Panel Focus

| Key | Action | Chat Equivalent |
|-----|--------|-----------------|
| `↑/↓` | Scroll up/down in active panel | N/A |
| `PgUp/PgDn` | Page up/down | N/A |
| `Home/End` | Jump to top/bottom | N/A |
| `Enter` | Select/expand item | N/A |
| `Space` | Toggle item selection | N/A |

---

## Advanced

| Key | Action | Chat Equivalent |
|-----|--------|-----------------|
| `X` | Export current view | "export [data]" |
| `P` | Take screenshot | "screenshot" |
| `[` | Save current layout | "save layout" |
| `]` | Load saved layout | "load layout" |
| `Ctrl+O` | Optimize routing | "optimize costs" |
| `Ctrl+F` | Forecast costs | "forecast costs" |

---

## Customization

Users can customize hotkeys in `~/.forge/config.yaml`:

```yaml
hotkeys:
  workers_view: "W"
  tasks_view: "T"
  spawn_worker: "S"
  custom_actions:
    - key: "Ctrl+D"
      tool: "spawn_worker"
      args:
        model: "sonnet"
        count: 1
```

---

## Hotkey Hints

When you hover over UI elements, hints appear:

```
┌─ Workers (W) ──────────────────────┐
│ sonnet-alpha  [Active]  (R)restart │
│ sonnet-beta   [Idle]    (K)kill    │
│                                     │
│ (S)pawn  (K)ill  (R)estart         │
└─────────────────────────────────────┘
```

---

## Chat + Hotkey Hybrid Examples

**Scenario 1: First time user**
```
User: "What can I show?"  → Discovers views
User: "Show me workers"   → Sees: "Worker view (Press W to return)"
[Later] User: W           → Instantly switches to workers
```

**Scenario 2: Power user workflow**
```
User: W                   → Workers view (hotkey)
User: ":"                 → Activate chat
User: "Spawn 2 sonnet"    → Natural language for complex action
User: T                   → Tasks view (hotkey)
```

**Scenario 3: Complex action**
```
User: ":"
User: "Show P0 tasks, spawn 2 workers if count > 5, then show costs"
      → Multi-step action (impossible with hotkeys alone)
```

---

## Accessibility Note

All hotkey actions are also available via:
1. **Chat interface** (`:` key)
2. **Menu navigation** (if enabled)
3. **Screen reader commands** (ARIA labels)

Hotkeys are purely optional shortcuts - no functionality is locked behind them.

---

## Learning Path

**Week 1**: Use chat exclusively, discover features
```
"show workers" → "show tasks" → "spawn worker"
```

**Week 2**: Notice hotkey hints in responses
```
"Worker view (Press W to return here)"
```

**Week 3**: Start using hotkeys for common actions
```
W → T → C  (view navigation)
```

**Week 4**: Hybrid workflow (hotkeys + chat)
```
W  [hotkey for workers]
:  [chat for complex action]
"Spawn 3 workers if costs < $50"
T  [hotkey back to tasks]
```

**Result**: Optimal speed + discoverability

---

## Future: Smart Hotkey Suggestions

FORGE will eventually suggest personalized hotkeys:

```
┌─ SUGGESTION ────────────────────────┐
│ You've used "show costs" 15 times   │
│ this week. Press C for quick access │
│                                      │
│ [Learn More] [Dismiss]              │
└──────────────────────────────────────┘
```

---

## Hotkey Cheat Sheet (Printable)

```
FORGE Hotkeys - Quick Reference

Global:        View Navigation:    Worker Mgmt:
:  Chat        W  Workers          S  Spawn
?  Help        T  Tasks            K  Kill
q  Quit        C  Costs            R  Restart
Esc Cancel     M  Metrics
               L  Logs             Task Mgmt:
               O  Overview         N  New task
                                   F  Filter
                                   A  Assign
                                   0-4 Priority

Remember: Everything can be done via chat (:)
Hotkeys are optional shortcuts for speed!
```

---

**FORGE** - Federated Orchestration & Resource Generation Engine
