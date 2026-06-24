# FORGE

**F**ederated **O**rchestration & **R**esource **G**eneration **E**ngine

> Terminal dashboard for orchestrating AI coding agent workers across your workspace.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.88+-orange.svg)](https://www.rust-lang.org/)
[![Version](https://img.shields.io/badge/version-0.3.0-green.svg)](Cargo.toml)

---

> **Status note:** FORGE is feature-complete at v0.3.0 and in maintenance mode. Active
> development has moved to [NEEDLE](https://github.com/jedarden/NEEDLE), a headless
> deterministic worker that processes the same bead task queues without requiring a running
> dashboard. FORGE remains useful if you prefer a TUI control plane; NEEDLE is the better
> choice for unattended fleet operation.

---

## What is FORGE?

FORGE is a Rust terminal dashboard (ratatui) that manages multiple AI coding agent workers
simultaneously. It spawns workers in tmux sessions, tracks their health, monitors API costs,
routes tasks to the right model tier based on complexity, and provides a conversational chat
interface for natural-language control.

**Think of it as a control tower for a fleet of AI agents.**

---

## Key features

### Multi-agent orchestration
- Spawn and manage multiple AI coding agents simultaneously (Claude Code, OpenCode, Aider, etc.)
- Worker health monitoring with auto-recovery and configurable recovery policies
- Support for tmux, subprocess, and Docker-based workers
- Worker pooling with automatic failover

### Intelligent cost optimization
- Model routing based on task complexity (0–100 scoring system)
- Three routing tiers: Budget (Haiku), Standard (Sonnet), Premium (Opus)
- Prioritize use-or-lose subscriptions before falling back to pay-per-token APIs
- Real-time cost tracking by day, week, and month

### Responsive TUI dashboard

The dashboard adapts to your terminal size:

**Ultra-wide (199+ columns)** — all 6 panels simultaneously:
```
┌─ Workers ────────┐ ┌─ Tasks ──────────┐ ┌─ Costs ──────────┐
│ GLM-4.7  active  │ │ Ready: 0         │ │ Today: $25.43    │
│ Opus     idle    │ │ In Progress: 0   │ │ Week:  $178.92   │
│ Sonnet   active  │ │ Blocked: 0       │ │ Month: $762.90   │
├─ Activity Log ───┤ ├─ Quick Actions ──┤ ├─ Metrics ────────┤
│ Worker spawned   │ │ [s] Spawn Worker │ │ Tasks/hr: 12.5   │
│ Task completed   │ │ [k] Kill Worker  │ │ Success: 94.2%   │
└──────────────────┘ └──────────────────┘ └──────────────────┘
```

**Wide (120–198 columns)** — 4 panels; press `[c]`/`[m]` for Costs/Metrics.

**Narrow (<120 columns)** — single-view mode; use hotkeys to switch between panels.

### Conversational control
Press `:` to open the chat interface and control FORGE in natural language:
```
"Spawn 2 sonnet workers on ~/myproject"
"What did I spend today?"
"Show P0 tasks"
"Kill the idle worker"
```

### Bead task integration
- Reads task queues from `.beads/` directories (same format as NEEDLE)
- Dependency-aware scheduling
- Bead-level locking to prevent duplicate work across workers

---

## Architecture

FORGE is a 7-crate Rust workspace:

| Crate | Purpose |
|-------|---------|
| `forge-core` | Shared types, utilities, logging |
| `forge-config` | Configuration management, validation, hot-reload |
| `forge-cost` | Cost tracking database (SQLite) |
| `forge-worker` | Worker spawning, discovery, health monitoring |
| `forge-tui` | ratatui terminal UI |
| `forge-chat` | Chat backend providers (Claude, OpenCode, custom) |
| `forge-init` | Interactive setup wizard (`forge init`) |
| `forge-server` | Optional server mode for multi-user collaboration |

```
┌─────────────────────────────────────────────────────────────────────┐
│                         FORGE TUI (ratatui)                          │
│  Workers │ Tasks │ Costs │ Metrics │ Activity Log │ Chat            │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
        ┌──────────────────────┼──────────────────────┐
        ▼                      ▼                      ▼
┌───────────────┐    ┌──────────────────┐    ┌───────────────────┐
│  forge-chat   │    │  forge-worker    │    │  forge-cost       │
│  (LLM calls)  │    │  (tmux/process)  │    │  (SQLite)         │
└───────────────┘    └──────────────────┘    └───────────────────┘
                               │
                    ┌──────────────────┐
                    │  .beads/ store   │
                    │  ~/.forge/status/│
                    │  ~/.forge/logs/  │
                    └──────────────────┘
```

---

## Installation

### Prerequisites

- Rust 1.88+ (`rustup update stable`)
- tmux (for worker session management)
- An AI coding CLI (Claude Code, OpenCode, Aider, etc.)

### Build from source

```bash
git clone https://github.com/jedarden/forge.git
cd forge
cargo build --release
# Binary is at ./target/release/forge
```

### Download a release

Pre-built binaries are available on the [Releases page](https://github.com/jedarden/forge/releases).

---

## Quick start

```bash
# First-time setup (interactive wizard)
forge init

# Launch the dashboard
forge

# With debug logging (logs to ~/.forge/logs/forge.log)
forge --debug
```

### Hotkeys

| Key | Action |
|-----|--------|
| `:` | Activate chat (primary interface) |
| `o` | Overview |
| `w` | Workers view |
| `t` | Tasks view |
| `c` | Costs view |
| `m` | Metrics view |
| `l` | Activity log |
| `s` | Spawn worker |
| `k` | Kill worker |
| `r` | Routing view |
| `Ctrl+U` | Self-update binary |
| `?` | Help |
| `q` | Quit |

---

## Configuration

FORGE reads `~/.forge/config.yaml`. Run `forge init` to generate it interactively, or
`forge validate` to check an existing config.

Key configuration sections:

```yaml
# Chat backend (used for the : interface)
chat_backend:
  provider: claude  # claude | opencode | custom
  model: sonnet

# Worker launchers (scripts that spawn agent sessions)
launchers:
  claude-code:
    executable: "~/.forge/launchers/claude-code-launcher"
    models: [sonnet, opus, haiku]

# Cost thresholds
costs:
  daily_budget_usd: 50
  alert_threshold_pct: 80

# Dashboard refresh
dashboard:
  refresh_interval_ms: 1000
  theme: default  # default | dark | light | cyberpunk
```

---

## CLI commands

```bash
# Initialize / reconfigure
forge init
forge init --reconfigure

# Validate configuration
forge validate
forge validate --verbose

# Worker management
forge worker spawn --model sonnet --workspace ~/project
forge worker kill <worker-id>
forge worker list

# Self-update
forge --server              # Start collaboration server
forge --connect http://...  # Connect as a client
```

---

## Development

```bash
# Build
cargo build

# Tests
cargo test --workspace

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt

# Run with debug logging
cargo run -- --debug
```

> **Testing note:** The TUI uses alternate screen mode. Always test in a separate tmux session
> rather than inside Claude Code itself to avoid screen corruption:
> ```bash
> tmux new-session -d -s forge-test -x 140 -y 40
> tmux send-keys -t forge-test "./target/release/forge --debug" Enter
> tmux attach -t forge-test
> ```

---

## Project status

v0.3.0 is the final feature release. The project is in maintenance mode — bug fixes only. New
orchestration work happens in [NEEDLE](https://github.com/jedarden/NEEDLE), which processes the
same bead task queues headlessly with a deterministic state machine, making it better suited for
unattended and fleet operation.

If you need a TUI control plane over your agent workers, FORGE is still the right tool.

---

## License

MIT

---

## Why "FORGE"?

- **Federated** — coordinate across multiple providers and models
- **Orchestration** — intelligent task distribution and scheduling
- **Resource** — optimize subscription and API usage
- **Generation** — spawn workers dynamically based on demand
- **Engine** — reliable, continuous automation
