# FORGE Marathon Coding Session Progress Tracker

## Session Overview

- **Start**: 2026-03-25
- **Current Phase**: Phase A - Fix failing tests [COMPLETE]
- **Exit Criterion**: `cargo test -p forge-tui` → 0 failures

## Progress

### Phase A: Fix failing tests [COMPLETED] ✓

**Status**: All 506 forge-tui tests now pass.

**Fixes applied**:
1. `status.rs`: Configured PollWatcher with:
   - Short poll interval matching debounce time (10-50ms)
   - `with_compare_contents(true)` to detect file modifications reliably
   - `NoCache::new()` for PollWatcher compatibility

2. `log_watcher.rs`: Same PollWatcher configuration plus:
   - Store debouncer in struct for proper cleanup (removed `std::mem::forget`)
   - Fixed resource leak that caused "too many open files" errors

**Root cause**: The default `notify::Config::default()` for PollWatcher uses a 30-second
poll interval and doesn't compare file contents. Tests were timing out before the watcher
detected file modifications.

### Phase B: Wire cost tracking to UI [NOT STARTED]

- Initialize `CostDatabase` in `App::new()`
- Wire real cost data into `draw_cost()` method
- Update cost records when workers report token usage
- Display: per-worker cost, total session cost, daily/weekly totals

### Phase C: Log parsing & metrics extraction [NOT STARTED]

- Parse worker log lines for token counts, error rates, task timing
- Store extracted metrics in time-series
- Feed metrics into Perf view and worker stats

### Phase D: Task filtering & search [NOT STARTED]

- Add search input field to Tasks view (press `/` to activate)
- Filter beads by title text, status, priority, label
- Sort options (by priority, created date, status)

### Phase E: Streaming chat tokens [NOT STARTED]

- Update `draw_chat()` to render partial responses as tokens arrive
- Show streaming indicator while waiting
- Handle stream cancellation (Escape key)

## Test Results

```
forge-tui: 506 passed, 0 failed
forge-core doctests: 3 failing (pre-existing, unrelated to current work)
```

## Next Session

Continue with Phase B: Wire cost tracking to UI.
