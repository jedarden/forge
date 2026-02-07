# TUI Dashboard Research - Executive Summary

**Research Completed**: 2026-02-07
**Beads**: po-7jb (TUI framework research), po-3j1 (Dashboard design)
**Status**: ✓ Complete - Ready for implementation

---

## Bottom Line

**Use Textual** - It's the only viable framework for building the control panel dashboard.

**Why**: Native async/await support, comprehensive widgets, excellent docs, active development.

**Score**: 95/100 (next best: urwid at 60/100)

---

## Research Documents

### 1. tui-framework-research.md (30K)
Comprehensive analysis of 5 Python TUI frameworks:
- **Textual** - ⭐⭐⭐⭐⭐ Recommended (async-native, modern, feature-rich)
- **Rich** - ⭐⭐⭐ Output-only (no interactivity)
- **urwid** - ⭐⭐⭐ Mature but synchronous
- **py-cui** - ⭐⭐ Limited features, poor docs
- **asciimatics** - ⭐⭐⭐ No async support

### 2. dashboard-design.md (56K)
Complete UI/UX design with ASCII mockups for:
- Main Dashboard (worker pool, subscriptions, task queue, activity log)
- Worker Detail View (spawn/kill controls, table management)
- Subscription Management (usage tracking, recommendations)
- Task Queue View (ready beads, blocked tasks, assignments)
- Cost Analytics View (daily/monthly spend, insights, charts)

Includes:
- Color palette and styling guidelines
- Keyboard shortcuts (40+ commands)
- Interaction flows (spawn/kill workers, task assignment)
- Component specifications (dimensions, update frequencies)
- Implementation phases (6 phases, MVP to production)

### 3. tui-framework-comparison-matrix.md (10K)
Quick-reference comparison tables:
- Overall feature matrix (10 criteria across 5 frameworks)
- Detailed capability matrix (30+ features)
- Use case suitability (10 scenarios)
- Pool optimizer requirements (weighted scoring)
- Decision matrix (95.0 vs 45.0 vs 60.0 vs 28.5 vs 56.5)

### 4. implementation-guide.md (24K)
Production-ready code examples:
- Installation and quick start
- Basic dashboard scaffold (copy-paste ready)
- Core components (WorkerTable, SubscriptionPanel, ActivityLog, TaskQueue)
- TCSS styling examples
- Event handling patterns
- Multi-screen navigation
- Data integration (PoolManager, SubscriptionTracker interfaces)
- Testing (snapshot and unit tests)
- Performance optimization
- Deployment (systemd, tmux)
- Debugging tips

---

## Key Decision Points

### Why Textual Wins

| Criterion | Textual | Alternatives | Impact |
|-----------|---------|--------------|--------|
| **Async Support** | ✅ Native | ❌ None | **CRITICAL** - Pool manager is async |
| **Real-time Updates** | ✅ Reactive | ⚠️ Manual | High-frequency worker updates |
| **Widget Library** | ✅ Rich | ⚠️ Limited | DataTable, RichLog, Progress needed |
| **Documentation** | ✅ Excellent | ⚠️ Poor-Moderate | Development speed |
| **Maintenance** | ✅ Active | ⚠️ Stale | Long-term viability |

**The async requirement alone eliminates all competitors.**

---

## Dashboard Architecture

### Core Panels (Main View)

```
┌─ Header ─────────────────────────────────────────────┐
│ Control Panel Dashboard      [System: Healthy]     │
├──────────────────────────────────────────────────────┤
│ ┌─ Workers ────────┐ ┌─ Subscriptions ────────────┐ │
│ │ Total: 24        │ │ Claude Pro: 72/100 (72%)  │ │
│ │ Active: 18       │ │ ▓▓▓▓▓▓▓▓▓▓▓▓░░░░░          │ │
│ │ Idle: 6          │ │ Resets in: 12h 34m        │ │
│ │ Unhealthy: 0     │ │                            │ │
│ │                  │ │ GLM-4.7: 430/1000 (43%)   │ │
│ │ [Worker Table]   │ │ ▓▓▓▓▓▓▓▓░░░░░░░░░░        │ │
│ └──────────────────┘ └────────────────────────────┘ │
│                                                      │
│ ┌─ Task Queue (10 ready, 3 blocked) ───────────────┐│
│ │ /kalshi-improvement: 8 ready                     ││
│ │   [P0] bd-abc: Fetch order history               ││
│ │   [P0] bd-def: Analyze execution failures        ││
│ │ /control-panel: 2 ready                         ││
│ └──────────────────────────────────────────────────┘│
│                                                      │
│ ┌─ Activity Log ────────────────────────────────────┐│
│ │ 14:23:45 ✓ worker-glm-03 completed bd-xyz        ││
│ │ 14:23:12 ⟳ Spawned worker-sonnet-07              ││
│ │ 14:22:58 ✓ worker-opus-01 completed bd-mno       ││
│ └──────────────────────────────────────────────────┘│
│                                                      │
│ [Tab] Views [W] Workers [S] Subscriptions [Q] Quit  │
└──────────────────────────────────────────────────────┘
```

### Additional Views

- **Worker Detail** (W key) - Full worker table, spawn/kill controls, worker details
- **Subscription Management** (S key) - Usage history, projections, recommendations
- **Task Queue** (T key) - Ready/blocked/in-progress tasks, assignment controls
- **Cost Analytics** (C key) - Daily/monthly spend, charts, optimization insights

### Update Frequencies

- Worker status: **2 seconds**
- Subscription usage: **5 seconds**
- Task queue: **3 seconds**
- Activity log: **Real-time** (event-driven)
- Cost analytics: **10 seconds**
- System health: **1 second**

---

## Implementation Phases

### Phase 1: Core Dashboard (MVP) - 1 week
- Main dashboard layout
- Worker pool panel (read-only)
- Activity log (read-only)
- Basic keyboard navigation
- **Deliverable**: View worker status and logs

### Phase 2: Subscription Tracking - 1 week
- Subscription panel with progress bars
- Usage tracking integration
- Recommendations display
- **Deliverable**: Monitor subscription usage

### Phase 3: Task Queue - 1 week
- Task queue panel
- Bead integration (read-only)
- Priority visualization
- **Deliverable**: View task queue status

### Phase 4: Interactive Controls - 1 week
- Spawn/kill worker commands
- Task assignment
- Pool target adjustments
- **Deliverable**: Full worker management

### Phase 5: Cost Analytics - 1 week
- Cost tracking integration
- Analytics view
- Charts and insights
- **Deliverable**: Cost visibility and optimization

### Phase 6: Polish - 1 week
- Responsive design refinements
- Help system
- Configuration options
- Performance optimizations
- **Deliverable**: Production-ready dashboard

**Total Timeline**: 6 weeks from zero to production

---

## Quick Start

### Install Textual

```bash
pip install textual textual-dev
python -m textual --version
```

### Copy Scaffold

See `implementation-guide.md` for complete working code to get started immediately.

### Run with Hot Reload

```bash
# Terminal 1: Console for debugging
textual console

# Terminal 2: Run with auto-reload
textual run --dev dashboard.py
```

---

## Performance Targets

- **UI Responsiveness**: <100ms interaction lag
- **Memory Usage**: <200MB with 50+ workers
- **CPU Usage**: <5% idle, <15% active updates
- **Terminal Compatibility**: xterm-256color minimum

---

## Testing Strategy

- **Snapshot tests** - Visual regression for layouts
- **Unit tests** - Component behavior
- **Integration tests** - Pool manager data flow
- **Load tests** - 50+ workers, high-frequency updates

---

## Configuration

```yaml
# ~/.control-panel/dashboard.yaml
dashboard:
  refresh_rates:
    workers: 2s
    subscriptions: 5s
    tasks: 3s
    costs: 10s

  display:
    theme: dark
    colors: truecolor  # auto, 256, truecolor
    animations: true

  shortcuts:
    vim_mode: true  # Enable h/j/k/l navigation
```

---

## Keyboard Shortcuts (Most Used)

### Global
- `Tab` - Switch views
- `W` - Worker detail view
- `S` - Subscription view
- `T` - Task queue view
- `C` - Cost analytics view
- `H` - Home (main dashboard)
- `R` - Force refresh
- `Q` - Quit
- `?` - Help
- `Esc` - Back

### Worker View
- `↑↓` - Navigate
- `Enter` - Details
- `K` - Kill worker
- `G` - Spawn GLM worker
- `S` - Spawn Sonnet worker
- `O` - Spawn Opus worker

### Task Queue View
- `A` - Assign task
- `P` - Change priority
- `M` - Change model
- `C` - Close task

---

## Integration Points

### Pool Manager Interface

```python
class PoolManager:
    async def get_worker_status(self) -> list[WorkerStatus]:
        """Fetch worker statuses."""

    async def spawn_worker(self, model: str, workspace: str = None) -> str:
        """Spawn new worker."""

    async def kill_worker(self, worker_id: str) -> None:
        """Kill worker."""

    async def event_stream(self) -> AsyncIterator[dict]:
        """Stream activity events."""
```

### Subscription Tracker Interface

```python
class SubscriptionTracker:
    async def get_usage(self, subscription: str) -> SubscriptionUsage:
        """Get current usage for subscription."""
```

### Bead Integration

```python
# Query ready beads
beads = await get_ready_beads()

# Group by workspace
by_workspace = {}
for bead in beads:
    by_workspace.setdefault(bead.workspace, []).append(bead)
```

---

## Cost Estimate

### Development
- **Phase 1-3** (Read-only dashboard): 2-3 weeks
- **Phase 4-6** (Full interactive): 4-6 weeks total

### Dependencies
```bash
textual==0.50+
textual-dev==1.4+
rich==13.7+  # Bundled with Textual
```

### Performance
- Minimal overhead: <5% CPU idle
- Memory efficient: <200MB for 50+ workers

---

## Success Criteria

- ✅ Real-time worker monitoring (2s updates)
- ✅ Interactive worker management (spawn/kill)
- ✅ Subscription usage tracking with recommendations
- ✅ Task queue visibility and assignment
- ✅ Cost analytics with optimization insights
- ✅ Keyboard-driven efficiency
- ✅ Responsive to terminal size changes
- ✅ Production-stable with error handling

---

## Next Actions

1. ✅ Research complete (this document)
2. ⏭ Review design mockups in `dashboard-design.md`
3. ⏭ Copy scaffold from `implementation-guide.md`
4. ⏭ Implement Phase 1 (Core Dashboard)
5. ⏭ Integrate with pool manager data sources
6. ⏭ Test with real worker pool
7. ⏭ Iterate through remaining phases

---

## Files in Research

```
/home/coder/research/control-panel/
├── TUI-DASHBOARD-SUMMARY.md           # This file (executive summary)
├── tui-framework-research.md          # Comprehensive framework analysis (30K)
├── dashboard-design.md                # Complete UI/UX design (56K)
├── tui-framework-comparison-matrix.md # Quick-reference tables (10K)
└── implementation-guide.md            # Production code examples (24K)
```

**Total**: ~120KB of research documentation covering framework selection, UI design, comparison matrices, and implementation guidance.

---

## Key Takeaways

1. **Textual is the only choice** - Async support is non-negotiable
2. **Design is comprehensive** - 5 complete screen mockups with specifications
3. **Implementation is clear** - Working code examples for all components
4. **Timeline is realistic** - 6 weeks MVP to production
5. **Integration is straightforward** - Clean interfaces to pool manager

**Recommendation**: Proceed with confidence to Phase 1 implementation.

---

## Research Credits

- **Framework Research**: Claude Code (Sonnet 4.5)
- **Dashboard Design**: Claude Code (Sonnet 4.5)
- **Beads**: po-7jb, po-3j1
- **Date**: 2026-02-07
- **Status**: ✓ Complete, ready for implementation

---

## Resources

- **Textual Docs**: https://textual.textualize.io/
- **Widget Gallery**: https://textual.textualize.io/widget_gallery/
- **Examples**: https://github.com/Textualize/textual/tree/main/examples
- **Discord**: https://discord.gg/Enf6Z3qhVr

---

**This research provides everything needed to build a production-quality control panel dashboard. Begin with Phase 1 using the implementation guide.**
