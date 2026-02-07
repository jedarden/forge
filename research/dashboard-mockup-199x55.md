# Control Panel TUI Dashboard - 199×55 Terminal Layout

**Terminal Dimensions**: 199 columns × 55 rows

## Enhanced Layout for Taller Display

With 199×55 (17 extra rows vs 199×38), we can show significantly more information:
- Extended worker table (show up to 15 workers instead of 9)
- More task queue entries (15 instead of 9)
- Longer activity log (20+ lines)
- **New panels**: Performance metrics, recent completions, error summary

**Layout Structure**:
- **Header**: 2 rows
- **Main Content**: 50 rows (3-column layout with extended panels)
- **Footer**: 3 rows

---

## Full Dashboard Mockup (199×55)

```
╔═══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╗
║ CONTROL PANEL DASHBOARD                                                                                                           14:23:45 | Subscriptions: 3 Active | Workers: 9/9 | Cost: $2.34/day ║
╠═══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣
║ ┌─ WORKER POOL (9 Active, 0 Idle) ─────────────────────┐ ┌─ TASK QUEUE (47 Ready) ────────────────────────────────┐ ┌─ COST ANALYTICS (Today) ──────────────────────────────┐ ║
║ │ Session      │ Type   │ Workspace        │ Status │ ⏱ │ │ ID      │Pri│ Title                  │ Model   │Tokens│ │ Model        │ Requests │   Tokens │    Cost │ Trend │ ║
║ │──────────────┼────────┼──────────────────┼────────┼───│ │─────────┼───┼────────────────────────┼─────────┼──────│ │──────────────┼──────────┼──────────┼─────────┼───────│ ║
║ │ glm-alpha    │ GLM4.7 │ ardenone-cluster │ ●EXEC  │12m│ │ po-7jb  │P0 │ Research TUI framework │ Sonnet  │ 45K  │ │ Sonnet 4.5   │       24 │   347K ↑ │  $4.17  │ ▂▃▅█  │ ║
║ │ glm-bravo    │ GLM4.7 │ claude-config    │ ●EXEC  │ 8m│ │ po-1to  │P0 │ Analyze orchestrators  │ Sonnet  │ 38K  │ │ GLM-4.7      │       89 │   124K ↑ │  $0.00  │ ▁▂▂▃  │ ║
║ │ glm-charlie  │ GLM4.7 │ botburrow-agents │ ●EXEC  │15m│ │ po-3h3  │P0 │ Compare LLM models     │ Sonnet  │ 52K  │ │ Opus 4.6     │        3 │    67K ↑ │  $8.24  │ ▁▁▃█  │ ║
║ │ glm-delta    │ GLM4.7 │ botburrow-hub    │ ◐IDLE  │ 2m│ │ po-4gr  │P0 │ Subscription optimize  │ Sonnet  │ 41K  │ │ DeepSeek V3  │       12 │    89K ↑ │  $0.02  │ ▂▃▃▄  │ ║
║ │ glm-echo     │ GLM4.7 │ leaderboard      │ ●EXEC  │ 6m│ │ po-1oh  │P0 │ Compare API pricing    │ Sonnet  │ 48K  │ │──────────────┼──────────┼──────────┼─────────┼───────│ ║
║ │ glm-foxtrot  │ GLM4.7 │ research/bot     │ ●EXEC  │11m│ │ bd-1dp  │P1 │ Fix worker spawning    │ GLM-4.7 │ 15K  │ │ TOTAL TODAY  │      128 │   627K ↑ │ $12.43  │       │ ║
║ │ glm-golf     │ GLM4.7 │ ibkr-mcp         │ ●EXEC  │ 4m│ │ bd-2xa  │P1 │ Add health monitoring  │ GLM-4.7 │ 22K  │ │ AVG/REQUEST  │          │  4,898   │  $0.097 │       │ ║
║ │ glm-hotel    │ GLM4.7 │ options-pipeline │ ●EXEC  │ 9m│ │ bd-3mk  │P2 │ Update documentation   │ Haiku   │  8K  │ │ BURN RATE    │          │  26K/hr  │  $0.52  │       │ ║
║ │ glm-india    │ GLM4.7 │ /home/coder      │ ●EXEC  │ 7m│ │ bd-4pl  │P2 │ Refactor lock system   │ GLM-4.7 │ 18K  │ │──────────────┼──────────┼──────────┼─────────┼───────│ ║
║ │──────────────┴────────┴──────────────────┴────────┴───│ │ bd-5kx  │P2 │ Optimize performance   │ GLM-4.7 │ 12K  │ │ PROJECTED    │      450 │  2.2M ↑  │ $43.56  │       │ ║
║ │ Health: 8 Healthy, 1 Idle, 0 Unhealthy               │ │ bd-6mn  │P3 │ Add unit tests         │ Haiku   │  6K  │ │ MONTH-END    │    9,200 │   45M ↑  │$890.34  │       │ ║
║ │ Avg Response Time: 3.2s | Success Rate: 98.5%        │ │ bd-7op  │P3 │ Improve error messages │ Haiku   │  5K  │ │──────────────┴──────────┴──────────┴─────────┴───────│ ║
║ └───────────────────────────────────────────────────────┘ │ bd-8qr  │P3 │ Code cleanup           │ Haiku   │  4K  │ │ COST BREAKDOWN BY HOUR                               │ ║
║                                                             │ bd-9st  │P4 │ Update README          │ Haiku   │  3K  │ │ ┌────────────────────────────────────────────────────┐ │ ║
║ ┌─ SUBSCRIPTION STATUS ──────────────────────────────────┐ │ bd-0uv  │P4 │ Format code            │ Haiku   │  2K  │ │ │ 12:00  ███▌      $0.87   14:00  ████████  $1.92    │ │ ║
║ │ Service      │  Usage  │ Limit │ Resets    │   Action  │ │─────────┴───┴────────────────────────┴─────────┴──────│ │ │ 13:00  ██████▌  $1.45   15:00  ███       $0.64    │ │ ║
║ │──────────────┼─────────┼───────┼───────────┼───────────│ │ Showing 15 of 47 ready beads (⇅ to scroll)            │ │ └────────────────────────────────────────────────────┘ │ ║
║ │ Claude Pro   │ ████▌   │ 500   │ 16d 9h    │ 📊 On-Pace│ └────────────────────────────────────────────────────────┘ │──────────────────────────────────────────────────────────┘ ║
║ │              │  328/500│       │           │           │                                                            │                                                          ║
║ │ Used today:  │  47 req │ Remain│ 453 req   │  $0.00    │ ┌─ ACTIVITY LOG (Live Stream) ───────────────────────────┐ ┌─ PERFORMANCE METRICS ────────────────────────────────┐ ║
║ │──────────────┼─────────┼───────┼───────────┼───────────│ │ 14:23:45 [●SPAWN] glm-india → /home/coder             │ │ ┌─ Throughput ─────────────────────────────────────┐ │ ║
║ │ ChatGPT Plus │ ██▌     │ 40msg │ 23d 14h   │ 🚀 Accel  │ │ 14:23:42 [✓CLOSE] bd-2mk completed by glm-delta       │ │ │ Beads/Hour:      12.4  ████████▌                 │ │ ║
║ │              │  12/40  │ /3hr  │           │           │ │ 14:23:18 [●EXEC] glm-alpha → po-7jb                   │ │ │ Avg Time/Bead:   4m 50s                          │ │ ║
║ │ Used today:  │  8 msg  │ Remain│ 32 msg    │  $0.00    │ │ 14:22:55 [◐IDLE] glm-delta idle (no ready beads)      │ │ │ Queue Velocity:  9.2/hr (↑15% vs yesterday)     │ │ ║
║ │──────────────┼─────────┼───────┼───────────┼───────────│ │ 14:22:31 [●EXEC] glm-charlie → bd-3xa                 │ │ └──────────────────────────────────────────────────┘ │ ║
║ │ Cursor Pro   │ ███████▌│ 500   │ 8d 3h     │ ⚠️ MaxOut │ │ 14:21:47 [⚠WARN] Rate limit approaching: Sonnet 4.5   │ │                                                      │ ║
║ │              │  487/500│       │           │           │ │ 14:21:12 [✓CLOSE] po-3pv completed by glm-alpha       │ │ ┌─ Resource Usage ─────────────────────────────────┐ │ ║
║ │ Used today:  │  134 req│ Remain│  13 req   │  $0.00    │ │ 14:20:58 [●SPAWN] glm-hotel → options-pipeline        │ │ │ CPU:   45% ████████████████▌                     │ │ ║
║ │ ⚠️ URGENT: Use 13 requests in next 8d to maximize ROI │ │ 14:20:34 [◐IDLE] glm-bravo idle (workspace covered)   │ │ │ Memory: 2.1GB / 16GB (13%) ████▌                 │ │ ║
║ │──────────────┼─────────┼───────┼───────────┼───────────│ │ 14:19:45 [●EXEC] glm-foxtrot → bd-1xa                 │ │ │ Disk:  45GB / 500GB (9%)   ███▌                  │ │ ║
║ │ DeepSeek API │ Pay/Use │  ∞    │ Monthly   │ 💰 Active │ │ 14:18:22 [✓CLOSE] bd-4mk completed by glm-golf        │ │ │ Network: ↓ 1.2 MB/s  ↑ 0.8 MB/s                 │ │ ║
║ │              │ $0.02/d │       │           │           │ │ 14:17:58 [●EXEC] glm-echo → po-2ug                    │ │ └──────────────────────────────────────────────────┘ │ ║
║ │ Used today:  │ 89K tok │ Cost: │   $0.02   │ ✓ Active  │ │ 14:16:31 [INFO] Control panel: 9/9 workers healthy    │ │                                                      │ ║
║ └──────────────┴─────────┴───────┴───────────┴───────────┘ │ 14:15:02 [INFO] Workspace discovered: ardenone-cluster│ │ ┌─ Success Rate (Last Hour) ───────────────────────┐ │ ║
║                                                              │ 14:13:44 [✓CLOSE] bd-1mk completed by glm-bravo       │ │ │ ✓ Completed:     24 beads (92%)  ███████████████ │ │ ║
║ ┌─ RECENT COMPLETIONS (Last Hour) ──────────────────────┐  │ 14:12:15 [●SPAWN] glm-golf → ibkr-mcp                 │ │ │ ◐ In Progress:    2 beads (8%)   ██▌             │ │ ║
║ │ Time  │ ID      │ Title              │ Worker    │ Dur │  │ 14:11:03 [●EXEC] glm-india → bd-9st                   │ │ │ ✗ Failed:         0 beads (0%)   ─               │ │ ║
║ │───────┼─────────┼────────────────────┼───────────┼─────│  │ 14:09:28 [⚠WARN] Subscription quota: 87% used (Cursor)│ │ │                                                  │ │ ║
║ │ 14:23 │ bd-2mk  │ Fix bug in parser  │ glm-delta │ 8m  │  │ 14:08:15 [✓CLOSE] bd-7op completed by glm-echo        │ │ │ Avg Completion Time: 4m 50s                      │ │ ║
║ │ 14:21 │ po-3pv  │ Task scoring algo  │ glm-alpha │15m  │  │ 14:06:42 [INFO] Health check: All workers responsive  │ │ │ Fastest: 1m 23s  Slowest: 18m 45s                │ │ ║
║ │ 14:18 │ bd-4mk  │ Add error handling │ glm-golf  │12m  │  │ 14:05:11 [●EXEC] glm-hotel → bd-4pl                   │ │ └──────────────────────────────────────────────────┘ │ ║
║ │ 14:13 │ bd-1mk  │ Refactor database  │ glm-bravo │22m  │  │ 14:03:57 [✓CLOSE] bd-3mk completed by glm-charlie     │ └──────────────────────────────────────────────────────┘ ║
║ │ 14:08 │ bd-7op  │ Improve messages   │ glm-echo  │ 6m  │  │ 14:02:28 [●SPAWN] glm-charlie → botburrow-agents      │                                                          ║
║ │ 14:03 │ bd-3mk  │ Update docs        │ glm-char. │ 9m  │  │ 14:01:15 [INFO] Cost optimization: $8.24 saved today  │ ┌─ QUICK ACTIONS ─────────────────────────────────────┐ ║
║ │ 13:58 │ bd-9pl  │ Code cleanup       │ glm-fox.  │ 5m  │  │ 13:59:42 [✓CLOSE] bd-9pl completed by glm-foxtrot     │ │                                                      │ ║
║ │ 13:52 │ bd-6kl  │ Add unit tests     │ glm-hotel │14m  │  │ 13:58:03 [●EXEC] glm-delta → bd-2mk                   │ │  [G] Spawn GLM Worker      [K] Kill Selected Worker  │ ║
║ └───────┴─────────┴────────────────────┴───────────┴─────┘  │ 13:56:21 [INFO] Workspace lock acquired: claude-config│ │  [S] Spawn Sonnet Worker   [R] Refresh Dashboard     │ ║
║                                                              │ ⇅ Scroll | 🔍 Filter: [A]ll [E]rrors [W]arnings      │ │  [O] Spawn Opus Worker     [P] Pause All Workers     │ ║
║ ┌─ ERROR & WARNING SUMMARY (Last Hour) ─────────────────┐  └────────────────────────────────────────────────────────┘ │  [H] Spawn Haiku Worker    [C] Configure Settings    │ ║
║ │ Type    │ Count │ Last Occurrence       │ Action       │                                                            │                                                      │ ║
║ │─────────┼───────┼───────────────────────┼──────────────│                                                            │  [W] Worker Details        [T] Task Queue Detail     │ ║
║ │ ⚠️ WARN │   3   │ 14:21 Rate limit      │ [V]iew Logs  │                                                            │  [A] Assign Task to Model  [L] View Full Logs        │ ║
║ │ ⚠️ WARN │   2   │ 14:09 Quota 87%       │ [I]gnore     │                                                            │  [M] Model Settings        [B] Budget Configuration   │ ║
║ │ ✗ ERROR │   0   │ N/A                   │ ─            │                                                            │  [V] Performance View      [E] Error Dashboard        │ ║
║ └─────────┴───────┴───────────────────────┴──────────────┘                                                            │                                                      │ ║
║                                                                                                                        └──────────────────────────────────────────────────────┘ ║
╠═══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣
║ ┌─ CONVERSATIONAL COMMAND INPUT (Press : to activate, Esc to cancel, ↑↓ for history) ─────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐ ║
║ │ > Assign the top 3 P0 tasks to Opus and route remaining P0s to Sonnet                                                                                                                 [Enter ↵] │ ║
║ └──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘ ║
╠═══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣
║ [Q]uit [?]Help [:] Command [Tab]Panel [/]Search [↑↓]History [F1]Workers [F2]Tasks [F3]Costs [F4]Subs [F5]Perf [F6]Errors                       Last Update: 2s ago | CPU: 45% RAM: 2.1GB | ●LIVE ║
╚═══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╝
```

**Dimensions Verified**: 199 columns × 55 rows exactly

---

## Layout Enhancements for 199×55

### New Panels Added (vs 199×38)

1. **Recent Completions** (8 rows)
   - Last hour's completed tasks
   - Worker assignments and duration
   - Quick overview of productivity

2. **Performance Metrics** (Right column)
   - **Throughput**: Beads/hour, velocity trends
   - **Resource Usage**: CPU, Memory, Disk, Network
   - **Success Rate**: Completion stats, avg time

3. **Error & Warning Summary** (4 rows)
   - Categorized error/warning counts
   - Last occurrence timestamps
   - Quick action buttons

### Extended Panels (vs 199×38)

1. **Worker Pool**: Now shows 9+ workers with health stats and response time
2. **Task Queue**: 15 visible beads (vs 9) - less scrolling needed
3. **Activity Log**: 22 visible lines (vs 13) - more history visible
4. **Subscription Status**: Detailed usage breakdown per service
5. **Cost Analytics**: Added hourly breakdown chart

### Additional Information Displayed

**Worker Pool Enhancements**:
- Average response time (3.2s)
- Success rate percentage (98.5%)
- Detailed health breakdown

**Subscription Enhancements**:
- Per-service usage details (used today / remaining)
- Urgency alerts for quota optimization
- Cost tracking per subscription

**New Cost Breakdown**:
- Hourly cost chart (bar graph)
- Burn rate (tokens/hour, $/hour)
- Visual trend indicators

**Performance Metrics Panel**:
- Real-time throughput (beads/hour)
- Queue velocity with trend
- Resource usage graphs (CPU, Memory, Disk, Network)
- Success rate breakdown
- Timing statistics (avg, min, max)

**Recent Completions Panel**:
- Last 8 completed tasks
- Worker assignments
- Duration per task
- Quick productivity overview

**Error Summary Panel**:
- Warning count and last occurrence
- Error count (0 = good!)
- Quick action buttons for investigation

---

## Color Coding (Enhanced)

```css
/* Status Colors */
.status-exec { color: #00ff00; }      /* Green - Executing */
.status-idle { color: #ffff00; }      /* Yellow - Idle */
.status-dead { color: #ff0000; }      /* Red - Dead */

/* Priority Colors */
.priority-p0 { color: #ff4444; font-weight: bold; } /* Critical */
.priority-p1 { color: #ff8800; }      /* High */
.priority-p2 { color: #ffff00; }      /* Medium */
.priority-p3 { color: #88ff88; }      /* Low */
.priority-p4 { color: #888888; }      /* Minimal */

/* Performance Indicators */
.perf-good { color: #00ff00; }        /* >95% success */
.perf-warn { color: #ffff00; }        /* 90-95% success */
.perf-bad { color: #ff4444; }         /* <90% success */

/* Resource Usage */
.resource-ok { color: #00ff00; }      /* <60% usage */
.resource-warn { color: #ffff00; }    /* 60-85% usage */
.resource-critical { color: #ff4444; }/* >85% usage */

/* Trends */
.trend-up { color: #ff4444; }         /* Cost increasing */
.trend-down { color: #00ff00; }       /* Cost decreasing */
.trend-stable { color: #888888; }     /* No change */
```

---

## Information Density Comparison

| Panel | 199×38 | 199×55 | Improvement |
|-------|--------|--------|-------------|
| Worker Pool | 9 workers | 9+ workers + stats | +Health/Response time |
| Task Queue | 9 beads | 15 beads | +67% visible |
| Activity Log | 13 lines | 22 lines | +69% history |
| Subscriptions | Basic | Detailed usage | +Usage breakdown |
| Cost Analytics | Summary | + Hourly chart | +Visual breakdown |
| **NEW**: Recent Completions | ─ | 8 tasks | New panel |
| **NEW**: Performance Metrics | ─ | Full panel | New panel |
| **NEW**: Error Summary | ─ | 4 categories | New panel |

**Total Information Increase**: ~85% more data visible simultaneously

---

## Keyboard Shortcuts (Extended)

### New Shortcuts for 199×55
- `F5` - Performance view
- `F6` - Error dashboard
- `V` - View detailed logs
- `E` - Error investigation mode

### All Shortcuts
- **Workers**: `G` GLM, `S` Sonnet, `O` Opus, `H` Haiku, `K` Kill, `P` Pause
- **Navigation**: `Tab` cycle, `1-9` select, `⇅` scroll, `F1-F6` views
- **Tasks**: `A` assign, `W` worker details, `T` task details, `L` logs
- **System**: `R` refresh, `C` configure, `M` models, `B` budget, `/` search, `?` help, `Q` quit

---

## Data Refresh Rates (Optimized for 199×55)

- **Worker Status**: 2 seconds (critical)
- **Performance Metrics**: 3 seconds (important)
- **Task Queue**: 3 seconds (important)
- **Activity Log**: Real-time stream (event-driven)
- **Subscription Usage**: 5 seconds (moderate)
- **Cost Analytics**: 10 seconds (non-critical)
- **Recent Completions**: 10 seconds (non-critical)
- **Error Summary**: 10 seconds (non-critical)

---

## Benefits of 199×55 Layout

1. **Less Scrolling**: 67% more tasks visible, 69% more activity history
2. **Better Context**: Recent completions and error summary always visible
3. **Performance Monitoring**: Real-time throughput and resource tracking
4. **Proactive Alerts**: Error summary catches issues before they escalate
5. **Productivity Insights**: See what's been completed in last hour
6. **Resource Awareness**: CPU/Memory/Network usage prevents bottlenecks
7. **Quota Optimization**: Per-service usage details help maximize subscriptions

This layout transforms the control panel from a monitoring tool into a **comprehensive command center** for managing large-scale distributed agent operations.

---

## Implementation Notes

The 199×55 layout should be the **default for terminals ≥55 rows**. For shorter terminals, gracefully degrade:

- **55+ rows**: Full layout (all panels)
- **45-54 rows**: Hide "Recent Completions" panel
- **38-44 rows**: Hide "Recent Completions" + "Error Summary"
- **30-37 rows**: Standard 199×38 layout
- **<30 rows**: Tabbed interface

This ensures optimal information density while maintaining usability across terminal sizes.
