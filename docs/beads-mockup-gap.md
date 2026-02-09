# FORGE Mockup Gap Beads

## Phase 1: Data Layer (P0)

### Implement metrics aggregation system
**Type:** task
**Priority:** P0
**Labels:** metrics,backend,performance,data

Build performance metrics collection and aggregation:

**Metrics to Track:**
- Tasks completed (per hour, per day, per worker)
- Average task duration (from bead created → closed timestamp)
- Worker efficiency (tasks/hour, tokens/task)
- Success rate (completed / (completed + failed))
- Model comparison (cost per task, time per task, by model)

**Data Sources:**
- Beads JSONL (task lifecycle)
- Status files (worker active time)
- Logs (API calls, errors)
- Cost DB (token usage per task)

**Storage:**
- SQLite tables: hourly_stats, daily_stats, worker_stats, model_stats
- Background aggregation task (runs every 10 minutes)
- Incremental updates only

**Acceptance Criteria:**
- Accurate task counting from beads
- Duration calculations match reality
- Aggregations run in <1s

---

### Implement conversational chat backend
**Type:** task
**Priority:** P0
**Labels:** chat,ai,backend,critical,integration

Build AI-powered conversational interface:

**Architecture:**
- MCP tool integration (reuse FORGE's tool catalog)
- Claude Code as backend (restricted instance)
- Context injection (current dashboard state as JSON)
- Response parsing and UI rendering

**Tool Set (Read-Only):**
- get_worker_status(), get_task_queue(), get_subscription_usage()
- get_cost_analytics(), get_activity_log(), query_metrics()

**Action Tools (Require Confirmation):**
- spawn_worker(), kill_worker(), assign_task(), pause_workers()

**Safety:**
- Rate limit: max 10 commands/minute
- Cost tracking for agent API calls
- Confirmation prompts for destructive ops
- Audit logging

**Acceptance Criteria:**
- Answers questions using dashboard state
- Executes commands with proper confirmation
- Response time <2s for simple queries

---

## Phase 2: UI Implementation (P1)

### Implement 3-column responsive layout
**Type:** task
**Priority:** P1
**Labels:** ui,layout,ratatui,responsive
**Depends on:** fg-a8z

Build 3-column ultra-wide dashboard layout:

**Layout Strategy:**
- If width >= 199 cols: 3-column layout
- If width < 199 cols: fall back to tabbed view

**3-Column Layout (199×38):**
- Left (66 cols): Worker Pool + Subscriptions
- Middle (66 cols): Task Queue + Activity Log
- Right (65 cols): Cost Analytics + Quick Actions

**Ratatui Implementation:**
- Horizontal layout with 3 constraints
- Each column: Vertical split for panels
- Responsive: adjust on terminal resize
- Preserve scroll positions

**Acceptance Criteria:**
- All 6 panels visible simultaneously on 199×38
- No content clipping or overflow
- Smooth transitions on resize

---

### Build cost analytics panel UI
**Type:** task
**Priority:** P1
**Labels:** ui,costs,visualization,panel
**Depends on:** fg-a8z

Replace cost tracking stub with rich analytics:

**Data Display:**
- Per-model breakdown (Sonnet, GLM, Opus, DeepSeek, Haiku)
- Columns: Model | Requests | Tokens | Cost | Trend
- Sparkline trends (▁▂▃▅█ for last 7 days)
- Today / Projected / Month-End totals

**Cost Breakdown Chart:**
- Horizontal bar chart by priority (P0, P1, P2-P4)
- Show percentage of total spend
- Visual bars: ████████ (scale to panel width)

**Acceptance Criteria:**
- Displays accurate cost data from backend
- Sparklines show meaningful trends
- Charts scale properly to panel size
- Updates in real-time without flicker

---

### Build subscription status panel UI
**Type:** task
**Priority:** P1
**Labels:** ui,subscriptions,visualization,panel
**Depends on:** fg-1h8

Implement subscription usage tracking panel:

**Panel Content:**
- Table: Service | Usage | Limit | Resets | Action
- Services: Claude Pro, ChatGPT Plus, Cursor Pro, DeepSeek
- Usage bars: ████░░ (visual progress bars)
- Reset timers: '16d 9h' (countdown)
- Actions: 📊 On-Pace, 🚀 Accelerate, ⚠️ MaxOut, 💰 Active

**Usage Bar Colors:**
- Green: 0-70% used
- Yellow: 70-90% used
- Red: 90-100% used

**Acceptance Criteria:**
- Displays all configured subscriptions
- Usage bars accurate and color-coded
- Reset timers update every minute
- Recommendations actionable and correct

---

## Phase 3: Additional Features (P2)

### Build quick actions panel
**Type:** task
**Priority:** P2
**Labels:** ui,actions,hotkeys,panel

Implement quick actions panel with keyboard shortcuts:

**Actions:**
- Spawn workers: G (GLM), S (Sonnet), O (Opus), H (Haiku)
- Kill worker: K
- Refresh: R
- View details: W (workers), T (tasks), A (assign), L (logs)
- Configure: M (model settings), B (budget), C (config)

**Display:**
- Two-column grid with hotkey + description
- Always visible in right column (bottom 40%)
- Color-code by action type (spawn=green, kill=red, view=blue)

**Acceptance Criteria:**
- All hotkeys functional and responsive
- Actions execute via tool system
- Visual feedback on activation

---

### Implement metrics visualization panel
**Type:** task
**Priority:** P2
**Labels:** ui,metrics,visualization,panel

Build metrics panel for performance tracking:

**Display Elements:**
- Tasks completed today (large number display)
- Average task duration (MM:SS format)
- Tasks per hour histogram (last 24 hours)
- Model efficiency comparison (bar chart)

**Charts:**
- Bar charts for histogram data
- Comparison bars for model efficiency
- Summary statistics at top

**Updates:**
- Refresh every 10 seconds
- Smooth number transitions (no flashing)
- Historical trend indicators

**Acceptance Criteria:**
- Accurate metrics from aggregation system
- Charts scale to panel size
- Smooth real-time updates

---

## Phase 4: Polish (P3)

### Add sparkline chart support
**Type:** task
**Priority:** P3
**Labels:** ui,charts,visualization

Implement reusable sparkline widget:

**Features:**
- Unicode block chars: ▁▂▃▄▅▆▇█
- Auto-scaling to fit data range
- Configurable width (default: 10 chars)
- Color support (green for up, red for down)

**Usage:**
- Cost trends in cost analytics panel
- Task rate trends in metrics panel
- Worker efficiency trends

**Acceptance Criteria:**
- Smooth sparklines for any numeric data series
- Auto-scales correctly
- Reusable widget component

---

### Implement progress bar widgets
**Type:** task
**Priority:** P3
**Labels:** ui,widgets,visualization

Enhance progress bar support:

**Features:**
- Horizontal bars with fill percentage
- Color-coded by percentage (green/yellow/red)
- Label support (text before bar)
- Percentage display (text after bar)
- Unicode fill chars: ▓ (filled), ░ (empty)

**Usage:**
- Subscription usage bars
- Budget usage indicators
- Worker capacity indicators

**Acceptance Criteria:**
- Smooth, flicker-free rendering
- Color transitions at configurable thresholds
- Reusable in any panel

---

### Add color theme support
**Type:** task
**Priority:** P3
**Labels:** ui,theme,config

Implement configurable color themes:

**Themes:**
- Default (current colors)
- Dark (darker borders, muted colors)
- Light (light background, dark text)
- Cyberpunk (cyan/magenta/yellow)

**Configuration:**
- ~/.forge/theme.toml
- Theme selection in settings (C hotkey)
- Runtime theme switching without restart

**Acceptance Criteria:**
- Multiple themes available
- Smooth theme switching
- Persists across sessions

---

## Phase 5: Integration & Testing (P1)

### End-to-end integration testing
**Type:** task
**Priority:** P1
**Labels:** testing,integration,e2e

Build comprehensive integration tests:

**Test Scenarios:**
- Launch FORGE, spawn workers, monitor status
- Switch between all 7 views
- Enter chat mode, execute commands
- Verify cost tracking updates in real-time
- Check subscription status updates
- Test responsive layout at different terminal sizes

**Test Automation:**
- Headless terminal testing (via expect or similar)
- Mock worker status files
- Mock log files with API events
- Mock subscription API responses

**Coverage Goals:**
- All views render without errors
- All hotkeys functional
- All panels display correct data
- Layout responsive to resizes

**Acceptance Criteria:**
- 90%+ test coverage of UI code
- All critical paths tested
- No panics or crashes in tests

---

### Performance optimization
**Type:** task
**Priority:** P2
**Labels:** performance,optimization

Optimize FORGE for smooth operation:

**Targets:**
- CPU usage <5% while idle
- Memory usage <50MB total
- Frame rate 60 FPS during updates
- Status file update latency <50ms
- Database query time <50ms

**Optimizations:**
- Debounce file system events
- Batch database operations
- Minimize redraws (Ratatui diff optimization)
- Profile hot paths with cargo flamegraph
- Reduce allocations in render loop

**Acceptance Criteria:**
- Meets all performance targets
- Smooth animation during data updates
- No lag when switching views

---

### Documentation updates
**Type:** task
**Priority:** P2
**Labels:** docs,user-guide

Update documentation to reflect new features:

**Documents to Update:**
- USER_GUIDE.md: Add sections for all new panels
- HOTKEYS.md: Document all quick action keys
- TOOL_CATALOG.md: Add conversational backend tools
- README.md: Update feature list and screenshots

**New Screenshots:**
- 3-column layout (199×38)
- Cost analytics panel with sparklines
- Subscription status panel
- Chat interface examples

**Acceptance Criteria:**
- All features documented
- Screenshots current and accurate
- Examples for common workflows
