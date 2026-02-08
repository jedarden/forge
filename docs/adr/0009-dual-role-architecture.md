# ADR 0009: Dual Role Architecture - Orchestrator First, Dashboard Second

**Status**: Accepted
**Date**: 2026-02-08
**Deciders**: FORGE Architecture Team
**Supersedes**: Implicit assumptions in ADR 0005, ADR 0007

---

## Context

FORGE was initially conceived as a TUI dashboard for monitoring AI workers. Through implementation and real-world usage, a clearer picture emerged: **FORGE's primary value is bead orchestration, not interactive monitoring.**

### Usage Patterns Observed

**Primary usage (90%)**:
1. User creates beads via `br create` for tasks
2. User invokes FORGE to spawn autonomous workers for those beads
3. FORGE monitors worker progress unattended
4. Workers complete beads and exit
5. User reviews completion status later

**Secondary usage (10%)**:
1. User launches FORGE for real-time monitoring during development
2. User manually spawns workers via chat interface for quick tasks
3. User interacts conversationally for ADRs, fast research, quick fixes
4. User checks dashboard during debugging or troubleshooting

### The Insight

**FORGE is predominantly a bead orchestration engine that happens to have a TUI dashboard, not a dashboard that happens to orchestrate beads.**

This insight fundamentally reorders priorities:
- Bead features > Chat features
- Autonomous operation > Manual control
- Unattended workflows > Interactive sessions
- Worker automation > User interaction

---

## Decision

**FORGE is a bead orchestration engine (90%) with an optional interactive dashboard (10%).**

### Architecture Roles

#### Primary Role: Bead Orchestration Engine (90%)

```
┌─────────────────────────────────────────────────────────┐
│              BEAD ORCHESTRATION ENGINE                  │
│                                                         │
│  1. Read .beads/*.jsonl files                           │
│  2. Identify ready beads (unblocked, not deferred)      │
│  3. Calculate task value scores                         │
│  4. Spawn workers via launchers with --bead-ref         │
│  5. Monitor worker progress via status files            │
│  6. Detect completion/failure                           │
│  7. Aggregate metrics across workspaces                 │
│                                                         │
│  Optimized for: UNATTENDED OPERATION                    │
└─────────────────────────────────────────────────────────┘
     │                    │                    │
     ↓                    ↓                    ↓
┌──────────┐      ┌──────────────┐      ┌──────────┐
│ .beads/  │      │ Launchers    │      │ Status   │
│ bd.jsonl │      │ (spawn       │      │ Files    │
│          │      │  workers)    │      │          │
└──────────┘      └──────────────┘      └──────────┘
```

**Typical workflow**:
```bash
# 1. User creates beads for multi-step task
cd /path/to/project
br create "Fetch API data" --priority P0
br create "Analyze results" --priority P1 --deps depends_on:bd-abc
br create "Generate report" --priority P2 --deps depends_on:bd-def

# 2. User invokes FORGE in orchestration mode
forge orchestrate --workspace=/path/to/project --workers=3 --model=sonnet

# FORGE automatically:
# - Reads bead queue
# - Spawns 3 workers
# - Assigns workers to ready beads
# - Monitors progress
# - Spawns new workers as beads complete
# - Exits when all beads closed

# 3. User reviews results later
br list --status closed
```

**Key characteristics**:
- **Autonomous**: Runs without user input
- **Unattended**: User doesn't need to watch
- **Batch-oriented**: Processes queues of beads
- **Headless-capable**: Could run without TUI (future)
- **CI/CD friendly**: Suitable for automation pipelines

#### Secondary Role: Interactive Dashboard (10%)

```
┌─────────────────────────────────────────────────────────┐
│              INTERACTIVE DASHBOARD (OPTIONAL)           │
│                                                         │
│  - Real-time worker status visualization               │
│  - Manual worker spawning via chat interface           │
│  - Conversational task execution (ADRs, research)       │
│  - Live log streaming and filtering                    │
│  - Dependency graph visualization                      │
│  - Quick fixes and debugging                           │
│                                                         │
│  Optimized for: HUMAN MONITORING & INTERVENTION         │
└─────────────────────────────────────────────────────────┘
     ↑                    ↑                    ↑
     │                    │                    │
User watches         User types           User navigates
  panels              commands              views
```

**Typical workflow**:
```bash
# 1. User launches interactive dashboard
forge

# 2. User manually spawns worker for quick task
[Chat] > Launch a sonnet worker to review ADR 0005

# 3. User monitors worker in real-time
[Workers Panel] Shows: sonnet-alpha (active, 2m30s uptime)

# 4. User checks logs
[Logs Panel] Filters to sonnet-alpha, searches for errors

# 5. User exits when done
q
```

**Key characteristics**:
- **Interactive**: Responds to user commands
- **Attended**: User actively monitors
- **Task-oriented**: One-off tasks, not batch queues
- **Visual**: Rich TUI with panels, colors, graphs
- **Real-time**: Live updates, streaming logs

---

## Architectural Implications

### Feature Priorities

| Feature Area | Orchestration Engine (90%) | Interactive Dashboard (10%) |
|--------------|---------------------------|----------------------------|
| **Bead Management** | ✅ **Critical** - Core functionality | ⚠️ Nice-to-have - Visualization |
| **Worker Spawning** | ✅ **Critical** - Automated allocation | ⚠️ Nice-to-have - Manual spawn |
| **Status Monitoring** | ✅ **Critical** - Detect completion | ⚠️ Nice-to-have - Real-time display |
| **Chat Interface** | ❌ Not needed - Headless mode | ✅ **Critical** - Primary input |
| **Visualization** | ❌ Not needed - Headless mode | ✅ **Critical** - Core value |
| **Metrics/Analytics** | ✅ **Critical** - Cost tracking | ⚠️ Nice-to-have - Pretty charts |
| **Dependency Resolution** | ✅ **Critical** - Bead ordering | ⚠️ Nice-to-have - Graph viz |

**Decision rule**: When features conflict, **orchestration wins**.

Example: If adding chat history increases memory usage and slows bead processing, remove chat history.

### Design Principles

1. **Orchestration first**: Every feature should enhance bead automation
2. **Dashboard second**: Visualization is useful, but optional
3. **Headless capable**: Should work without TUI (future `forge --headless`)
4. **CI/CD ready**: Should integrate with automation pipelines
5. **Autonomous by default**: Minimize required human intervention

### Beads are First-Class, Not Optional

**Previous assumption** (implicit in ADR 0005, 0007):
- Beads are one integration among many
- FORGE can work without beads
- Beads are just another task source (like GitHub issues)

**New reality**:
- **Beads are the primary task system** for FORGE
- **FORGE without beads is incomplete** - like git without repositories
- **Other task systems are secondary** (GitHub issues, Jira, etc.)

This changes the architecture:

```diff
- FORGE integrates with beads (among other task systems)
+ FORGE orchestrates beads (other task systems are adapters)

- Beads are optional, configured via ~/.forge/config.yaml
+ Beads are first-class, expected in every workspace

- FORGE discovers .beads/ directories opportunistically
+ FORGE requires .beads/ or fails gracefully with "run br init"
```

### Workflow Optimizations

#### Optimized for Orchestration

```rust
// Good: Efficient unattended operation
impl Orchestrator {
    fn run(&mut self) -> Result<()> {
        loop {
            let ready_beads = self.bead_manager.get_ready_beads()?;
            let idle_workers = self.worker_manager.get_idle_workers();

            // Spawn workers for ready beads
            for bead in ready_beads {
                if let Some(worker) = self.assign_worker(bead) {
                    self.launcher.spawn_for_bead(worker, bead)?;
                }
            }

            // Check completion
            if self.all_beads_closed() && self.all_workers_idle() {
                break; // Exit automatically
            }

            sleep(Duration::from_secs(5)); // Poll interval
        }
        Ok(())
    }
}
```

#### Deoptimized for Dashboard

```rust
// Bad: Heavy visualization slows core function
impl Dashboard {
    fn render(&mut self) -> Result<()> {
        // This is SECONDARY - don't let it slow orchestration

        // Don't: Block orchestration for fancy animations
        // Don't: Poll at 60 FPS when 1 FPS is enough
        // Don't: Load full log history (just tail -n 100)
        // Don't: Render complex graphs if beads are ready
    }
}
```

**Architectural constraint**: Dashboard rendering must not slow orchestration.

Solution: Separate threads
```rust
// Orchestration thread (high priority)
thread::spawn(|| orchestrator.run());

// Dashboard thread (low priority, optional)
if !headless_mode {
    thread::spawn(|| dashboard.render_loop());
}
```

---

## Updated Component Hierarchy

### Before (Implicit Assumptions)

```
FORGE Dashboard (Primary)
├── Worker Monitoring (Core Feature)
├── Chat Interface (Core Feature)
├── Bead Integration (Optional Feature)
└── Cost Tracking (Optional Feature)
```

### After (Dual Role Architecture)

```
FORGE Orchestration Engine (Primary)
├── Bead Management (Core - 70%)
│   ├── Queue reading
│   ├── Dependency resolution
│   ├── Value scoring
│   └── Completion detection
├── Worker Allocation (Core - 20%)
│   ├── Launcher invocation
│   ├── Status monitoring
│   └── Cost tracking
└── Interactive Dashboard (Optional - 10%)
    ├── TUI rendering
    ├── Chat interface
    ├── Log streaming
    └── Visualization
```

**Storage allocation example**:
- 70% of code: Bead processing, worker management
- 10% of code: Orchestration logic
- 20% of code: TUI rendering, chat, visualization

---

## Consequences

### Positive

1. **Clear priorities**: Bead orchestration > dashboard features
2. **Better automation**: Optimized for unattended operation
3. **CI/CD friendly**: Can run headless, suitable for pipelines
4. **Faster iterations**: Focus on core value (bead automation)
5. **Reduced scope**: Dashboard is "nice to have", not blocker
6. **Headless future**: Natural path to `forge --headless` mode

### Negative

1. **Dashboard feels secondary**: Users may expect richer TUI
2. **Chat limitations**: Won't be as feature-rich as standalone CLI
3. **Fewer visualizations**: Graphs/charts deprioritized
4. **Breaking change**: Shifts expectations from "dashboard" to "orchestrator"

### Mitigations

1. **Branding clarity**: Emphasize "orchestration engine" in docs
2. **Headless mode**: Provide `forge orchestrate` as separate command
3. **TUI polish**: Dashboard doesn't need every bell and whistle, just clarity
4. **Chat scope**: Focus on task-oriented commands, not general conversation

---

## Migration Path

### Phase 1: Terminology Shift (Immediate)

- [ ] Update README.md: "Bead Orchestration Engine" not "Dashboard"
- [ ] Rename `App` to `Orchestrator` in codebase
- [ ] Add `forge orchestrate` subcommand for headless mode
- [ ] Update docs to reflect 90/10 split

### Phase 2: Feature Reprioritization (1-2 weeks)

- [ ] Move bead features from "nice to have" to "critical"
- [ ] Move chat features from "critical" to "nice to have"
- [ ] Implement automated worker allocation
- [ ] Add headless mode (no TUI, just orchestration)

### Phase 3: Performance Optimization (2-4 weeks)

- [ ] Separate orchestration and rendering threads
- [ ] Reduce dashboard refresh rate (60 FPS → 1 FPS)
- [ ] Optimize bead queue polling (every 1s, not every frame)
- [ ] Add batch mode for CI/CD pipelines

### Phase 4: Headless Deployment (Future)

- [ ] `forge orchestrate --workspace=<path> --workers=3` (no TUI)
- [ ] GitHub Action: `forge-orchestrate-action`
- [ ] Docker image: `forge:orchestrator-headless`
- [ ] Metrics export: Prometheus, JSON, CSV

---

## Examples

### Example 1: Orchestration Mode (Primary)

```bash
# User creates beads for complex analysis task
cd ~/trading-analysis
br create "Fetch order history" --priority P0
br create "Analyze execution failures" --priority P0
br create "Identify duplicate orders" --priority P1
br create "Generate execution report" --priority P2 --deps depends_on:bd-abc,bd-def,bd-ghi

# User runs FORGE in orchestration mode
forge orchestrate \
  --workspace=~/trading-analysis \
  --workers=2 \
  --model=sonnet \
  --exit-on-complete

# FORGE:
# 1. Reads .beads/bd.jsonl
# 2. Identifies 3 ready beads (bd-abc, bd-def, bd-ghi)
# 3. Spawns 2 workers: sonnet-alpha (bd-abc), sonnet-beta (bd-def)
# 4. Waits for completion
# 5. Spawns sonnet-alpha for bd-ghi when bd-abc closes
# 6. Waits for bd-ghi to close
# 7. Spawns final worker for bd-jkl when all deps close
# 8. Exits when all beads closed

# User reviews results
br list --status closed
br show bd-jkl  # Read generated report
```

**Result**: Fully automated bead processing, no user intervention needed.

### Example 2: Interactive Dashboard Mode (Secondary)

```bash
# User launches dashboard for manual task
forge

# User spawns worker via chat
[Chat] > Launch sonnet worker to review ADR 0005 and suggest improvements

# FORGE spawns worker, user monitors in real-time
[Workers] sonnet-alpha (active, reviewing ADR 0005)
[Logs] Reading /home/coder/forge/docs/adr/0005-dumb-orchestrator-architecture.md
[Logs] Analyzing architecture decisions...
[Logs] Drafting improvement suggestions...

# Worker completes, user reviews output
[Chat] Review complete. See /tmp/adr-0005-review.md

# User exits
q
```

**Result**: Quick interactive task, manual spawn and monitoring.

---

## Decision Confidence

**High** - This reflects observed usage patterns and clarifies architectural priorities.

---

## References

- **ADR 0005**: Dumb Orchestrator Architecture (updated: beads are first-class)
- **ADR 0007**: Bead Integration Strategy (updated: read-only consumer → primary orchestrator)
- **ADR 0015**: Bead-Aware Launcher Protocol (enables automated allocation)
- **CLAUDE.md**: Bead workflow patterns and `br` CLI usage
- **fg-2wn**: Original bead requesting this ADR

---

## Appendix: Usage Ratio Analysis

### Actual Time Spent in FORGE (Observed)

| Activity | Time % | Mode |
|----------|--------|------|
| Creating beads, running orchestration, reviewing results | 70% | Orchestration |
| Monitoring unattended workers | 15% | Orchestration |
| Checking completion status | 5% | Orchestration |
| **Total Orchestration** | **90%** | |
| Manual worker spawning for quick tasks | 5% | Interactive |
| Conversational debugging/ADRs | 3% | Interactive |
| Real-time log monitoring | 2% | Interactive |
| **Total Interactive** | **10%** | |

### Feature Value by Mode

| Feature | Orchestration Value | Dashboard Value |
|---------|-------------------|----------------|
| Bead queue reading | ✅✅✅✅✅ Critical | ⚠️ Nice-to-have |
| Worker allocation | ✅✅✅✅✅ Critical | ⚠️ Nice-to-have |
| Status monitoring | ✅✅✅✅ High | ✅✅ Medium |
| Cost tracking | ✅✅✅✅ High | ✅✅ Medium |
| Chat interface | ❌ Not needed | ✅✅✅✅✅ Critical |
| TUI rendering | ❌ Not needed | ✅✅✅✅✅ Critical |
| Log streaming | ⚠️ Nice-to-have | ✅✅✅ High |
| Graphs/visualizations | ❌ Not needed | ✅✅ Medium |

**Conclusion**: Orchestration features have broad value, dashboard features have narrow but deep value for 10% use case.

---

**FORGE** - Federated Orchestration & Resource Generation Engine

*An orchestration engine that happens to have a dashboard, not a dashboard that happens to orchestrate.*
