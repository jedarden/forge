# Responsive Layout Strategy for Control Panel TUI

The TUI dashboard must adapt gracefully to different terminal sizes while maintaining usability and information density.

## Terminal Size Breakpoints

### Ultra-Wide Layout (≥180 cols × ≥35 rows)
**Example**: 199×38, 200×40, 240×50

**Layout**: 3-column side-by-side
- Left: Workers + Subscriptions (33%)
- Middle: Tasks + Activity Log (33%)
- Right: Cost Analytics + Actions (33%)

**Features**:
- All information visible simultaneously
- No screen switching required
- Maximum information density

---

### Wide Layout (140-179 cols × ≥30 rows)
**Example**: 160×40, 170×35

**Layout**: 2-column layout with rotating third panel
- Left: Workers + Subscriptions (50%)
- Right: Tasks + Activity **OR** Cost Analytics (50%)

**Features**:
- Toggle between Activity Log and Cost Analytics with hotkey
- Quick Actions moved to footer as shortcuts
- Still efficient, minimal scrolling

---

### Standard Layout (100-139 cols × ≥25 rows)
**Example**: 120×30, 130×35

**Layout**: Single column with tabs
- **Tab 1**: Worker Pool + Subscriptions
- **Tab 2**: Task Queue + Activity Log
- **Tab 3**: Cost Analytics

**Features**:
- Tab navigation with F1-F5 keys
- Active tab shown in header
- Compact mode with abbreviated columns

---

### Narrow Layout (80-99 cols × ≥24 rows)
**Example**: 80×24 (classic terminal), 90×30

**Layout**: Single column, stacked panels
- Collapsible panels (expand/collapse with Enter)
- Horizontal scrolling for wide tables
- Abbreviated text and icons

**Features**:
- Priority information shown first
- Details hidden behind expand actions
- Mobile-like accordion interface

---

### Minimal Layout (<80 cols OR <24 rows)
**Example**: 70×20, 60×30

**Layout**: Command-line interface fallback
- Menu-driven navigation
- One panel at a time, fullscreen
- Text-based selection menus

**Features**:
- "Pool status" command shows summary
- Drill-down interface
- Graceful degradation to CLI mode

---

## Responsive Component Behavior

### Worker Pool Table

| Terminal Width | Columns Shown |
|----------------|---------------|
| ≥160 cols | Session, Type, Workspace (full path), Status, Time, Health |
| 100-159 cols | Session, Type, Workspace (truncated), Status, Time |
| 80-99 cols | Session (short), Type (abbrev), Status, Time |
| <80 cols | Session, Status (icons only) |

### Task Queue Table

| Terminal Width | Columns Shown |
|----------------|---------------|
| ≥160 cols | ID, Priority, Title (full), Model, Est. Tokens, Assignee |
| 100-159 cols | ID, Priority, Title (truncated 30 chars), Model |
| 80-99 cols | ID, Pri, Title (truncated 20 chars) |
| <80 cols | ID, Title (truncated 15 chars) |

### Subscription Status

| Terminal Width | Display Mode |
|----------------|--------------|
| ≥160 cols | Full table with progress bars (horizontal) |
| 100-159 cols | Compact table with percentage only |
| 80-99 cols | List view with vertical bars |
| <80 cols | Summary only ("3/4 on-pace") |

### Activity Log

| Terminal Width | Display Mode |
|----------------|--------------|
| ≥160 cols | Full timestamps, session names, full messages |
| 100-159 cols | Short timestamps (HH:MM), abbreviated messages |
| 80-99 cols | Time + icon + short message |
| <80 cols | Icon + message only (no timestamp) |

---

## Layout Switching Logic

### Automatic Detection

```python
from textual.app import App
from textual.reactive import reactive

class PoolOptimizerDashboard(App):
    terminal_width = reactive(0)
    terminal_height = reactive(0)
    layout_mode = reactive("ultra-wide")

    def on_mount(self):
        self.update_layout_mode()

    def on_resize(self, event):
        self.terminal_width = event.size.width
        self.terminal_height = event.size.height
        self.update_layout_mode()

    def update_layout_mode(self):
        w, h = self.terminal_width, self.terminal_height

        if w >= 180 and h >= 35:
            self.layout_mode = "ultra-wide"
        elif w >= 140 and h >= 30:
            self.layout_mode = "wide"
        elif w >= 100 and h >= 25:
            self.layout_mode = "standard"
        elif w >= 80 and h >= 24:
            self.layout_mode = "narrow"
        else:
            self.layout_mode = "minimal"

        self.refresh_layout()

    def refresh_layout(self):
        # Rebuild UI based on layout_mode
        self.query_one("#main").remove_children()
        if self.layout_mode == "ultra-wide":
            self.mount_ultra_wide_layout()
        elif self.layout_mode == "wide":
            self.mount_wide_layout()
        # ... etc
```

---

## Responsive TCSS Styling

```css
/* Ultra-Wide Layout (≥180 cols) */
@media (min-width: 180) {
    Screen {
        layout: grid;
        grid-size: 3 1;
        grid-columns: 1fr 1fr 1fr;
    }

    .left-column { display: block; }
    .middle-column { display: block; }
    .right-column { display: block; }

    WorkerPoolTable { height: 14; }
    SubscriptionTable { height: auto; }
}

/* Wide Layout (140-179 cols) */
@media (min-width: 140) and (max-width: 179) {
    Screen {
        layout: grid;
        grid-size: 2 1;
        grid-columns: 1fr 1fr;
    }

    .left-column { display: block; }
    .middle-column { display: block; }
    .right-column { display: none; } /* Hidden, toggle with hotkey */

    WorkerPoolTable { height: 12; }
}

/* Standard Layout (100-139 cols) */
@media (min-width: 100) and (max-width: 139) {
    Screen {
        layout: vertical;
    }

    TabbedContent { height: 100%; }

    .left-column { display: none; }
    .middle-column { display: none; }
    .right-column { display: none; }

    WorkerPoolTable { height: 10; }
    .table-column-workspace { display: none; } /* Hide workspace path */
}

/* Narrow Layout (80-99 cols) */
@media (min-width: 80) and (max-width: 99) {
    Screen {
        layout: vertical;
    }

    Collapsible { border: solid #444; }

    WorkerPoolTable {
        height: 8;
    }

    .table-column-type { display: none; }
    .table-column-workspace { display: none; }
}

/* Minimal Layout (<80 cols) */
@media (max-width: 79) {
    Screen {
        layout: vertical;
    }

    .all-panels { display: none; }
    .cli-mode { display: block; }

    ListView { height: 100%; }
}
```

---

## Example Mockups by Size

### 160×40 (Wide Layout)

```
╔══════════════════════════════════════════════════════════════════════╦══════════════════════════════════════════════════════════════════════╗
║ CONTROL PANEL - Wide Layout                          Workers: 9/9  ║  Cost: $12.43/day | Subscriptions: 3 Active         14:23:45 Sat 2/7 ║
╠══════════════════════════════════════════════════════════════════════╬══════════════════════════════════════════════════════════════════════╣
║ ┌─ WORKER POOL ─────────────────────────────────────┐              ║ ┌─ TASK QUEUE (47 Ready) ──────────────────────────┐                ║
║ │ Session    │ Type   │ Workspace    │ Status │ ⏱  │              ║ │ ID     │Pri│ Title              │ Model  │Tokens│                ║
║ │────────────┼────────┼──────────────┼────────┼────│              ║ │────────┼───┼────────────────────┼────────┼──────│                ║
║ │ glm-alpha  │ GLM4.7 │ ardenone-... │ ●EXEC  │12m │              ║ │ po-7jb │P0 │ Research TUI fra...│ Sonnet │ 45K  │                ║
║ │ glm-bravo  │ GLM4.7 │ claude-cfg   │ ●EXEC  │ 8m │              ║ │ po-1to │P0 │ Analyze orchestr...│ Sonnet │ 38K  │                ║
║ │ glm-charlie│ GLM4.7 │ botburrow-ag │ ●EXEC  │15m │              ║ │ po-3h3 │P0 │ Compare LLM mode...│ Sonnet │ 52K  │                ║
║ │ glm-delta  │ GLM4.7 │ botburrow-hub│ ◐IDLE  │ 2m │              ║ │ bd-1dp │P1 │ Fix worker spawn...│ GLM4.7 │ 15K  │                ║
║ │ glm-echo   │ GLM4.7 │ leaderboard  │ ●EXEC  │ 6m │              ║ │ bd-2xa │P1 │ Add health monit...│ GLM4.7 │ 22K  │                ║
║ │ glm-foxtrot│ GLM4.7 │ research/bot │ ●EXEC  │11m │              ║ │────────┴───┴────────────────────┴────────┴──────│                ║
║ │ glm-golf   │ GLM4.7 │ ibkr-mcp     │ ●EXEC  │ 4m │              ║ │ Showing 5 of 47 (⇅ scroll) [F2] Details         │                ║
║ │ glm-hotel  │ GLM4.7 │ options-pipe │ ●EXEC  │ 9m │              ║ └───────────────────────────────────────────────────┘                ║
║ │ glm-india  │ GLM4.7 │ /home/coder  │ ●EXEC  │ 7m │              ║                                                                      ║
║ └────────────────────────────────────────────────────┘              ║ ┌─ ACTIVITY LOG (Press [C] for Costs) ─────────────┐                ║
║                                                                      ║ │ 14:23:42 [●] glm-india → /home/coder             │                ║
║ ┌─ SUBSCRIPTIONS ───────────────────────────────────┐              ║ │ 14:23:18 [✓] bd-2mk completed (glm-delta)        │                ║
║ │ Service     │  Usage    │ Resets  │   Action      │              ║ │ 14:22:55 [◐] glm-delta idle (no beads)           │                ║
║ │─────────────┼───────────┼─────────┼───────────────│              ║ │ 14:22:31 [●] glm-charlie → bd-3xa                │                ║
║ │ Claude Pro  │ ████▌ 66% │ 16d 9h  │ 📊 On-Pace    │              ║ │ 14:21:47 [⚠] Rate limit: Sonnet 4.5              │                ║
║ │ ChatGPT+    │ ██▌   30% │ 23d 14h │ 🚀 Accelerate │              ║ │ 14:21:12 [✓] po-3pv completed (glm-alpha)        │                ║
║ │ Cursor Pro  │ ███████▌  │ 8d 3h   │ ⚠️ Max Out    │              ║ │ 14:20:58 [●] glm-hotel → options-pipeline        │                ║
║ │ DeepSeek    │ Pay/Use   │ Monthly │ 💰 Active     │              ║ │ 14:20:34 [◐] glm-bravo idle                      │                ║
║ └─────────────────────────────────────────────────────┘              ║ │ ⇅ Scroll | Filter: [A]ll [E]rrors [W]arnings    │                ║
║                                                                      ║ └───────────────────────────────────────────────────┘                ║
╠══════════════════════════════════════════════════════════════════════╩══════════════════════════════════════════════════════════════════════╣
║ [Q]uit [?]Help [C]osts [R]efresh [G]LM [S]onnet [O]pus [K]ill [F1]Workers [F2]Tasks [F3]Costs                      Update: 2s | CPU: 45% ║
╚══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╝
```

**160 cols × 40 rows**

---

### 120×30 (Standard Layout with Tabs)

```
╔══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╗
║ CONTROL PANEL                                                              Workers: 9/9 | Cost: $12.43 | 14:23:45 Sat 2/7 ║
╠══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣
║ [F1 Workers] [F2 Tasks] [F3 Costs] [F4 Subscriptions] [F5 Settings]                                                     ║
╠══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣
║                                                                                                                          ║
║ ┌─ WORKER POOL (9 Active) ────────────────────────────────────────────────────────────────────────────────────────────┐ ║
║ │ Session      │ Type   │ Workspace            │ Status │ Time │ Health │                                             │ ║
║ │──────────────┼────────┼──────────────────────┼────────┼──────┼────────│                                             │ ║
║ │ glm-alpha    │ GLM4.7 │ ardenone-cluster     │ ●EXEC  │  12m │ ✓ OK   │                                             │ ║
║ │ glm-bravo    │ GLM4.7 │ claude-config        │ ●EXEC  │   8m │ ✓ OK   │                                             │ ║
║ │ glm-charlie  │ GLM4.7 │ botburrow-agents     │ ●EXEC  │  15m │ ✓ OK   │                                             │ ║
║ │ glm-delta    │ GLM4.7 │ botburrow-hub        │ ◐IDLE  │   2m │ ⚠ IDLE │                                             │ ║
║ │ glm-echo     │ GLM4.7 │ leaderboard          │ ●EXEC  │   6m │ ✓ OK   │                                             │ ║
║ │ glm-foxtrot  │ GLM4.7 │ research/botburrow   │ ●EXEC  │  11m │ ✓ OK   │                                             │ ║
║ │ glm-golf     │ GLM4.7 │ ibkr-mcp             │ ●EXEC  │   4m │ ✓ OK   │                                             │ ║
║ │ glm-hotel    │ GLM4.7 │ options-pipeline     │ ●EXEC  │   9m │ ✓ OK   │                                             │ ║
║ │ glm-india    │ GLM4.7 │ /home/coder          │ ●EXEC  │   7m │ ✓ OK   │                                             │ ║
║ └──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘ ║
║                                                                                                                          ║
║ ┌─ SUBSCRIPTION STATUS ────────────────────────────────────────────────────────────────────────────────────────────────┐ ║
║ │ Service          │  Usage Progress  │ Limit    │ Resets      │ Recommendation │                                     │ ║
║ │──────────────────┼──────────────────┼──────────┼─────────────┼────────────────│                                     │ ║
║ │ Claude Pro       │ ████████▌ 66%    │ 500 req  │ 16d 9h      │ 📊 On-Pace     │                                     │ ║
║ │ ChatGPT Plus     │ ████▌ 30%        │ 40/3hr   │ 23d 14h     │ 🚀 Accelerate  │                                     │ ║
║ │ Cursor Pro       │ ███████████▌ 97% │ 500 req  │ 8d 3h       │ ⚠️ Max Out Now │                                     │ ║
║ │ DeepSeek API     │ Pay-per-use      │ No limit │ Monthly bill│ 💰 Cost $0.02  │                                     │ ║
║ └──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘ ║
║                                                                                                                          ║
║ Actions: [G]LM Worker [S]onnet [O]pus [H]aiku [K]ill [R]efresh [P]ause [C]onfigure                                     ║
║                                                                                                                          ║
╠══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣
║ [Q]uit [?]Help [Tab]Next Panel [/]Search [1-9]Select Worker                           Last Update: 2s ago | CPU: 45%   ║
╚══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╝
```

**120 cols × 30 rows**

---

### 80×24 (Narrow Layout - Classic Terminal)

```
╔══════════════════════════════════════════════════════════════════════════════╗
║ CONTROL PANEL                                       Workers: 9/9 | Cost $12 ║
╠══════════════════════════════════════════════════════════════════════════════╣
║ ▼ WORKER POOL (Click to collapse)                                           ║
║ ┌────────────────────────────────────────────────────────────────────────┐  ║
║ │ Session      │ Status │ Time  │                                         │  ║
║ │──────────────┼────────┼───────│                                         │  ║
║ │ glm-alpha    │ ●EXEC  │   12m │                                         │  ║
║ │ glm-bravo    │ ●EXEC  │    8m │                                         │  ║
║ │ glm-charlie  │ ●EXEC  │   15m │                                         │  ║
║ │ glm-delta    │ ◐IDLE  │    2m │                                         │  ║
║ │ glm-echo     │ ●EXEC  │    6m │  (Scroll for more)                      │  ║
║ └────────────────────────────────────────────────────────────────────────┘  ║
║                                                                              ║
║ ▶ SUBSCRIPTIONS (Click to expand)                                           ║
║ ▶ TASK QUEUE (47 ready)                                                     ║
║ ▶ ACTIVITY LOG                                                              ║
║ ▶ COST ANALYTICS                                                            ║
║                                                                              ║
║ Actions: [G]LM [S]onnet [O]pus [K]ill [R]efresh [?]Help                     ║
╠══════════════════════════════════════════════════════════════════════════════╣
║ [Q]uit [Tab]Next [Enter]Expand                          Update: 2s | CPU 45%║
╚══════════════════════════════════════════════════════════════════════════════╝
```

**80 cols × 24 rows**

---

## User Preference Override

Allow users to force a specific layout mode via config:

```yaml
# ~/.control-panel/config.yaml
ui:
  layout_mode: auto  # auto | ultra-wide | wide | standard | narrow | minimal
  min_width: 120     # Minimum terminal width required
  min_height: 30     # Minimum terminal height required

  # Column visibility preferences
  workers:
    show_workspace_path: auto  # auto | always | never
    show_executor_type: auto
    show_health_details: auto

  tasks:
    show_estimated_tokens: auto
    show_assigned_model: auto
    max_title_length: auto  # auto | 20 | 30 | 50

  # Refresh rates (seconds)
  refresh:
    workers: 2
    subscriptions: 5
    tasks: 3
    activity_log: 1
    costs: 10
```

---

## Testing Matrix

Test the TUI at these common terminal sizes:

| Size | Name | Layout Mode | Notes |
|------|------|-------------|-------|
| 199×38 | Ultra-wide | ultra-wide | 3-column, all visible |
| 160×40 | Wide screen | wide | 2-column, toggle third |
| 120×30 | Standard | standard | Tabbed interface |
| 100×25 | Compact | standard | Tabbed, compact mode |
| 80×24 | Classic | narrow | Accordion panels |
| 70×20 | Small | minimal | CLI fallback |

---

## Implementation Priority

1. **Phase 1**: Implement ultra-wide (199×38) layout first
2. **Phase 2**: Add standard (120×30) tabbed layout
3. **Phase 3**: Add responsive breakpoint detection
4. **Phase 4**: Implement wide (160×40) layout
5. **Phase 5**: Add narrow (80×24) accordion layout
6. **Phase 6**: Polish and user preferences

This ensures the most common use cases work first, with graceful degradation for edge cases.

---

## Ultra-Tall Layout (199×55+) - NEW

### Enhanced Layout for Tall Terminals

For terminals with 55+ rows, we can display significantly more information:

**New Panels**:
- Recent Completions (last hour's completed tasks)
- Performance Metrics (throughput, resource usage, success rate)
- Error & Warning Summary

**Extended Panels**:
- Task Queue: 15 visible beads (vs 9 in 199×38)
- Activity Log: 22 visible lines (vs 13)
- Subscriptions: Detailed per-service usage breakdown
- Cost Analytics: Hourly breakdown chart

**Information Density**: ~85% more data visible vs 199×38

### Responsive Breakpoint for Tall Terminals

```css
/* Ultra-Tall Layout (≥55 rows) */
@media (min-height: 55) {
    .recent-completions-panel { display: block; }
    .performance-metrics-panel { display: block; }
    .error-summary-panel { display: block; }
    
    TaskQueueTable { max-items: 15; }
    ActivityLog { max-items: 22; }
    SubscriptionTable { show-details: true; }
}

/* Tall Layout (45-54 rows) */
@media (min-height: 45) and (max-height: 54) {
    .performance-metrics-panel { display: block; }
    .error-summary-panel { display: block; }
    .recent-completions-panel { display: none; }
    
    TaskQueueTable { max-items: 12; }
    ActivityLog { max-items: 18; }
}

/* Standard Height (38-44 rows) */
@media (min-height: 38) and (max-height: 44) {
    .performance-metrics-panel { display: none; }
    .error-summary-panel { display: none; }
    .recent-completions-panel { display: none; }
    
    TaskQueueTable { max-items: 9; }
    ActivityLog { max-items: 13; }
}
```

### Updated Testing Matrix

| Size | Name | Layout | New Panels |
|------|------|--------|------------|
| 199×55 | Ultra-tall | 3-col + extended | All 3 new panels |
| 199×45 | Tall | 3-col + partial | Performance + Errors |
| 199×38 | Ultra-wide | 3-col standard | None (baseline) |
| 160×40 | Wide | 2-col | None |
| 120×30 | Standard | Tabbed | None |
| 80×24 | Classic | Accordion | None |

See `dashboard-mockup-199x55.md` for full ultra-tall layout.
