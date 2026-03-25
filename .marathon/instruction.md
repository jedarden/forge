# FORGE — Marathon Coding Instruction

## Project Overview

FORGE is a terminal-based AI agent orchestration dashboard written in Rust. It lets users
spawn, manage, and monitor multiple AI coding agents (Claude, OpenCode, Aider, etc.) from
a single TUI interface, with cost tracking, model routing, beads task integration, and a
conversational chat interface.

- **Language**: Rust (edition 2024, requires 1.88+)
- **TUI Framework**: Ratatui 0.29 + Crossterm 0.28
- **Async**: Tokio 1.43
- **DB**: SQLite via rusqlite (bundled)
- **Crates**: forge-core, forge-config, forge-cost, forge-worker, forge-tui, forge-chat, forge-init

## Working Directory

`/home/coding/FORGE`

## Current State (as of 2026-03-24)

- **Builds**: Yes — `cargo build` completes with only warnings
- **Tests**: 457 passing, **49 failing** — all failures in `forge-tui` crate:
  - `status::tests` — worker status file watching tests
  - `log_watcher::tests` — log parsing tests
  - `integration_tests::tests` — e2e worker lifecycle tests
- **Compilation blockers**: None (View::Perf was already fixed)
- **Key gaps**: Cost tracking not wired to UI, log metrics not extracted, task filtering absent, streaming chat tokens not displayed

## Iteration Protocol

Each iteration:

1. **Check current state**: run `cargo test -p forge-tui 2>&1 | grep -E "FAILED|passed|failed"` to see test status. Check `PROGRESS.md` if present.
2. **Identify next work**: follow the priority order below. Don't skip ahead — fix the test suite before adding features.
3. **Implement one coherent unit**: a fix, a module, a feature. Stay focused.
4. **Write or fix tests**: all new code must have tests. Existing failing tests must be diagnosed and fixed.
5. **Build clean**: `cargo build` must succeed with no new errors.
6. **Commit and push**: mandatory before ending each iteration. Every iteration ends with a commit and `git push origin main`.
7. **Update PROGRESS.md**: mark what was done, what's next.

**Critical**: After every 3-4 build/test cycles, run `cargo clean` to prevent disk bloat from build artifacts (they were 3.8 GiB). The build target directory grows fast.

## Priority Order

### Phase A — Fix Failing Tests (do this first)

All 49 failing tests are in `forge-tui`. Diagnose root causes before fixing:

```bash
cargo test -p forge-tui -- status::tests 2>&1 | grep -A 20 "FAILED\|thread.*panicked"
cargo test -p forge-tui -- log_watcher::tests 2>&1 | grep -A 20 "FAILED\|thread.*panicked"
cargo test -p forge-tui -- integration_tests::tests 2>&1 | grep -A 20 "FAILED\|thread.*panicked"
```

Likely causes based on test names: file watcher timing races, temp directory cleanup issues,
async timing assumptions. Fix at the root — don't just add `sleep()` calls.

**Exit criterion**: `cargo test --workspace` → 0 failures.

### Phase B — Wire Cost Tracking to UI

The `forge-cost` crate has a complete `CostDatabase` and optimizer. The Cost view in the TUI
shows placeholder data. Work needed:

1. Initialize `CostDatabase` in `App::new()` (see `crates/forge-tui/src/app.rs`)
2. Wire real cost data into `draw_cost()` method
3. Update cost records when workers report token usage
4. Display: per-worker cost, total session cost, daily/weekly totals

Key files: `crates/forge-cost/src/lib.rs`, `crates/forge-cost/src/db.rs`,
`crates/forge-tui/src/app.rs` (search for `draw_cost`)

**Exit criterion**: Cost view shows real data, not placeholder text.

### Phase C — Log Parsing & Metrics Extraction

Worker log watching infrastructure exists but metrics are not extracted. Work needed:

1. Parse worker log lines for token counts, error rates, task timing
2. Store extracted metrics in time-series (SQLite or in-memory ring buffer)
3. Feed metrics into the Perf view (`draw_perf()`) and worker stats

Key file: `crates/forge-tui/src/log_watcher.rs`

**Exit criterion**: Token usage and error rates visible in the dashboard.

### Phase D — Task Filtering & Search

The Tasks view loads beads but has no filtering. Work needed:

1. Add search input field to Tasks view (press `/` to activate)
2. Filter beads by title text, status, priority, label
3. Sort options (by priority, created date, status)

Key file: `crates/forge-tui/src/app.rs` (search for `draw_tasks`)

**Exit criterion**: User can type `/` in Tasks view and filter the task list.

### Phase E — Streaming Chat Tokens

Chat backend supports streaming but the UI waits for the full response. Work needed:

1. Update `draw_chat()` to render partial responses as tokens arrive
2. Show a streaming indicator (cursor or spinner) while waiting
3. Handle stream cancellation (Escape key)

Key file: `crates/forge-chat/src/`, `crates/forge-tui/src/app.rs` (search for `draw_chat`)

**Exit criterion**: Chat responses appear token-by-token, not all at once.

### Phase F — P1 Bug Fixes

After Phases A–E:
- **fg-1gjn**: Panel focus visual indicator broken — which panel is active is not obvious
- **fg-jqw3**: Chat rendering visual artifacts / text overflow in narrow terminals
- **fg-16bd**: No confirmation dialog before destructive actions (kill worker, etc.)

### Phase G — Phase 2 Intelligence (Model Routing)

The high-value differentiator. The scoring algorithm exists in `forge-cost` but isn't integrated:

1. Score incoming tasks 0-100 for complexity
2. Route low-complexity tasks to Haiku/Sonnet, high-complexity to Opus
3. Track routing decisions and cost savings
4. Display routing stats in Cost view

Key file: `crates/forge-cost/src/optimizer.rs`

**Exit criterion**: Workers are automatically assigned based on task complexity score.

### Phase H — Subscription Tracking

Tracks per-model subscription quotas and usage:

1. Backend: quota DB, usage counters, billing cycle reset
2. UI: Subscriptions view shows real quota data

### Phase I — Advanced Health Monitoring

1. Alert thresholds (worker stuck > N minutes, error rate > X%)
2. Auto-recovery strategies (restart crashed worker, reassign tasks)
3. Anomaly detection (sudden spike in errors or cost)

### Phase J — CHANGELOG & Release

1. Fill in CHANGELOG.md v0.1.1 through v0.1.9 from git log
2. Bump version to v0.2.0
3. Create GitHub release with compiled binary

## Building

```bash
# Debug build (fast, use for development)
cargo build

# Release build (optimized)
cargo build --release

# Run tests (specific crate)
cargo test -p forge-tui

# Run all tests
cargo test --workspace

# Lint
cargo clippy -- -D warnings

# Format
cargo fmt

# IMPORTANT: Clean periodically to prevent 3.8GB+ disk bloat
cargo clean
```

## Testing the TUI

**Never run the TUI inside Claude Code's terminal** — it uses alternate screen mode and will
corrupt the terminal state.

Always test in a separate tmux session:

```bash
# Build first
cargo build --release

# Create test session
tmux new-session -d -s forge-test -x 140 -y 40

# Run forge in the session
tmux send-keys -t forge-test "./target/release/forge --debug" Enter

# Attach to interact
tmux attach -t forge-test

# Cleanup when done
tmux kill-session -t forge-test
```

Test at multiple dimensions:
- Narrow: `tmux new-session -d -s forge-narrow -x 80 -y 24`
- Wide: `tmux new-session -d -s forge-wide -x 140 -y 40`
- UltraWide: `tmux new-session -d -s forge-ultrawide -x 200 -y 50`

## Git Workflow

```bash
# All work goes to main (no feature branches needed for marathon sessions)
git add -p          # stage selectively
git commit -m "fix/feat/chore: description"
git push origin main
```

Commit message conventions from the repo:
- `fix(scope): message` — bug fixes
- `feat(scope): message` — new features
- `chore(scope): message` — maintenance
- `docs(scope): message` — documentation only

## Key File Map

| File | Purpose |
|------|---------|
| `crates/forge-tui/src/app.rs` | Main TUI app (~5000+ lines): all draw_*() methods, event handling |
| `crates/forge-tui/src/view.rs` | View enum, hotkeys, titles |
| `crates/forge-tui/src/status.rs` | Worker status file watching |
| `crates/forge-tui/src/log_watcher.rs` | Worker log watching |
| `crates/forge-cost/src/db.rs` | Cost database |
| `crates/forge-cost/src/optimizer.rs` | Model routing / task scoring |
| `crates/forge-chat/src/` | Chat backend providers |
| `crates/forge-core/src/` | Shared types, recovery utilities |
| `GAPS_ANALYSIS.md` | Detailed gap analysis (generated 2026-02-13) |
| `docs/adr/` | Architecture Decision Records (16 ADRs) |

## Important Constraints

- Rust 1.88+ (edition 2024 — some syntax won't compile on older toolchains)
- The `self-update` feature flag is referenced in `src/main.rs` but not declared in `Cargo.toml` — causes warnings, not errors. Fix it or leave it.
- `WorkerPerfTracker` in `forge-core/src/worker_perf.rs` has unused fields — they're stubs for Phase G/H work.
- Do not force-push. Do not amend published commits.
