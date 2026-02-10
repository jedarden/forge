# FORGE

**F**ederated **O**rchestration & **R**esource **G**eneration **E**ngine

> Intelligent control panel for orchestrating AI coding agents across your workspace

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Python Version](https://img.shields.io/badge/python-3.10%2B-blue.svg)](https://www.python.org/downloads/)
[![Code Style](https://img.shields.io/badge/code%20style-ruff-21C1FF.svg)](https://github.com/astral-sh/ruff)

---

## What is FORGE?

FORGE is a terminal-based control panel that intelligently manages multiple AI coding agents (Claude Code, OpenCode, Aider, Cursor) across your workspace. It automatically distributes tasks to the right models based on complexity and cost, optimizes your AI subscriptions, and provides real-time monitoring through a beautiful TUI.

**Think of it as Kubernetes for AI workers.**

---

## Table of Contents

- [Key Features](#key-features)
- [Architecture](#architecture)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [CLI Setup Guide](#cli-setup-guide)
- [Configuration](#configuration)
- [Documentation](#documentation)
- [Troubleshooting](#troubleshooting)
- [Contributing](#contributing)
- [License](#license)

---

## Key Features

### 🤖 Multi-Agent Orchestration
- Spawn and manage multiple AI coding agents simultaneously
- Distribute work across different models and providers (Claude, GPT, Qwen, GLM, etc.)
- Real-time health monitoring with auto-recovery
- Support for tmux, subprocess, and Docker-based workers
- Worker pooling with automatic failover

### 💰 Cost Optimization
- Smart model routing based on task complexity (0-100 scoring system)
- Maximize use-or-lose subscriptions before falling back to pay-per-token APIs
- Save 87-94% on AI costs with intelligent routing
- Real-time cost tracking and forecasting
- Subscription vs API cost analysis
- Budget alerts with visual progress bars

### 📊 Beautiful TUI Dashboard

**Responsive Multi-Panel Layout** - Automatically adapts to your terminal size:

#### 🖥️ Ultra-Wide Mode (199+ columns × 38+ rows)
**All 6 panels visible simultaneously** in a 3-column layout:
```
┌─ Workers ────────┐ ┌─ Tasks ──────────┐ ┌─ Costs ──────────┐
│ GLM-4.7  active  │ │ Ready: 0         │ │ Today: $25.43    │
│ Opus     idle    │ │ In Progress: 0   │ │ Week:  $178.92   │
│ Sonnet   active  │ │ Blocked: 0       │ │ Month: $762.90   │
├─ Subscriptions ──┤ ├─ Activity Log ───┤ ├─ Quick Actions ─┤
│ Claude Pro 328/  │ │ Worker spawned   │ │ [s] Spawn Worker │
│ ChatGPT+ 12/40   │ │ Task completed   │ │ [k] Kill Worker  │
└──────────────────┘ └──────────────────┘ └──────────────────┘
```
**Best for**: Large monitors, ultra-wide displays, comprehensive overview

#### 💻 Wide Mode (120-198 columns × 30+ rows)
**4 panels visible** in a 2-column layout:
```
┌─ Worker Pool ────────────┐ ┌─ Utilization ─────────────┐
│ Total: 17 (4 active)     │ │ Worker Utilization: 23%   │
│ Unhealthy: 12            │ │ ██████                    │
│                          │ │ 4/17 workers active       │
├─ Task Queue ─────────────┤ │                           │
│ Ready: 0                 │ │ Status Breakdown:         │
│ No pending tasks         │ │ ⚡ Active:  4             │
│                          │ │ 💤 Idle:    1             │
├─ Activity Log ───────────┤ │ ⛔ Stopped: 12            │
│ Worker stopped...        │ │ ⚠️ 12 unhealthy workers   │
│ Task completed...        │ │                           │
└──────────────────────────┘ └───────────────────────────┘
```
**Access Costs/Metrics via hotkeys**: Press `[c]` for Costs, `[m]` for Metrics
**Best for**: Standard terminals, laptop screens

#### 📱 Narrow Mode (<120 columns × 20+ rows)
**Single-view mode** - Switch between views using hotkeys:
```
┌─ FORGE Dashboard - Overview ─────────────────────────────┐
│ Worker Pool              │ Utilization                   │
│ Total: 17 (4 active)     │ Worker Utilization: 23%       │
│ Unhealthy: 12            │ 4/17 workers active (23%)     │
├──────────────────────────┴───────────────────────────────┤
│ Task Queue: Ready: 0 | In Progress: 0 | Blocked: 0      │
│ No pending tasks.                                        │
├──────────────────────────────────────────────────────────┤
│ Activity Log                                             │
│ 21:55:48 claude-code-glm-47-bravo stopped                │
│ 21:50:39 claude-code-glm-47-alpha stopped                │
└──────────────────────────────────────────────────────────┘
[o]Overview [w]Workers [t]Tasks [c]Costs [m]Metrics [:]Chat
```
**Navigate with hotkeys**: `o` Overview, `w` Workers, `t` Tasks, `c` Costs, `m` Metrics, `l` Logs, `:` Chat
**Best for**: SSH sessions, tmux panes, small terminals

#### Features Across All Modes
- **6 available views**: Workers, Tasks, Costs, Metrics, Activity Log, Chat
- Real-time worker status and health metrics
- Task queue visualization with bead integration
- Live activity logs and performance metrics
- **Conversational interface as primary control** - just ask in natural language
- Optional hotkeys for power users (W for workers, T for tasks, etc.)
- **4 configurable themes**: Default, Dark, Light, Cyberpunk

### 🔄 Self-Updating & Hot-Reload
- Hot-reload capability for live configuration updates
- State preservation across updates
- Runtime configuration changes via CLI

### 🎯 Task Intelligence
- Automatic task value scoring (P0-P4 priority levels)
- Model capability matching
- Dependency-aware scheduling with Beads integration
- Bead-level locking for multi-worker coordination
- Lock management to prevent duplicate work

### 💬 Conversational Chat Interface
- Natural language commands to control FORGE
- AI-powered tool execution
- Command history and context awareness
- Rate limiting (10 commands/min)
- Audit logging for all commands

### 🔌 Extensible Integration
- **Headless CLI backend**: Integrate with any AI tool that supports structured I/O
- **Custom launchers**: Create worker launchers for any AI coding tool
- **Worker configurations**: Share reusable worker configs via Git repos
- **45+ built-in tools** for dashboard control and management
- **Chat tools**: worker_status, task_queue, cost_analytics, subscription_usage, spawn_worker, kill_worker, assign_task, and more

---

## Architecture

FORGE operates as a **federated orchestration system** with a "dumb orchestrator" design:

```
┌─────────────────────────────────────────────────────────────────────┐
│                         FORGE Control Panel                          │
│                   (Textual TUI Dashboard)                            │
├─────────────────────────────────────────────────────────────────────┤
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐           │
│  │ Workers  │  │  Tasks   │  │  Costs   │  │ Metrics  │           │
│  │   Pool   │  │  Queue   │  │ Tracking │  │ & Stats  │           │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘           │
│  ┌──────────────────┐  ┌──────────────────────────────────────┐   │
│  │   Activity Log   │  │   Chat Interface (Conversational)    │   │
│  │   (Real-time)    │  │   "Spawn 3 sonnet workers"           │   │
│  └──────────────────┘  └──────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
┌───────────────┐    ┌───────────────┐    ┌───────────────┐
│   Chat        │    │   Worker      │    │   Log &       │
│   Backend     │    │   Launchers   │    │   Status      │
│               │    │               │    │   Watchers    │
│ • claude-code │    │ • tmux        │    │ • JSONL       │
│ • opencode    │    │ • subprocess  │    │   parsing     │
│ • custom      │    │ • docker      │    │ • inotify     │
└───────────────┘    └───────────────┘    └───────────────┘
        │                     │                     │
        └─────────────────────┼─────────────────────┘
                              ▼
                    ┌─────────────────┐
                    │   Data Layer    │
                    ├─────────────────┤
                    │ • Beads (JSONL) │
                    │ • Status Files  │
                    │ • Cost DB (SQL) │
                    │ • Config (YAML) │
                    └─────────────────┘
```

### Component Overview

| Component | Purpose |
|-----------|---------|
| **Dashboard (TUI)** | Responsive multi-panel UI with 6 available views |
| **Chat Backend** | Translates natural language to tool calls via AI |
| **Worker Launchers** | Spawns AI coding agents in tmux/subprocess/docker |
| **Status Watcher** | Monitors worker status files for real-time updates |
| **Log Watcher** | Parses worker logs for metrics and activity |
| **Beads Integration** | Task tracking with dependency management |
| **Cost Tracker** | Monitors API costs and subscription usage |

### Design Philosophy

FORGE is a **"dumb orchestrator"** - it has no built-in AI or worker management. Instead, it integrates with your existing tools:

- **No built-in AI**: Uses your Claude Code, OpenCode, or other tools via headless CLI backend
- **No worker management**: Delegates to your launcher scripts (tmux, Docker, etc.)
- **Configuration-driven**: All behavior controlled via YAML config
- **Extensible**: Create custom launchers, backends, and worker configs

---

## Installation

### Prerequisites

FORGE requires:
- **Python 3.10 or higher**
- **Linux, macOS, or WSL2** (Windows support coming soon)
- **tmux** (for worker session management, optional but recommended)
- **Git** (for workspace and repository features)

### Installing FORGE

#### Option 1: Install from PyPI (when available)

```bash
pip install llmforge
```

#### Option 2: Install from source (development mode)

```bash
# Clone the repository
git clone https://github.com/jedarden/forge.git
cd forge

# Create virtual environment (recommended)
python -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# Install in editable mode
pip install -e .

# Verify installation
forge --version
```

#### Option 3: Install with development dependencies

```bash
pip install -e ".[dev]"
```

This installs additional tools:
- `pytest` - Testing framework
- `pytest-asyncio` - Async test support
- `pytest-textual` - TUI testing
- `coverage` - Code coverage
- `mypy` - Type checking
- `ruff` - Linting and formatting

### Post-Installation Setup

```bash
# Initialize FORGE configuration
forge init

# This creates the directory structure:
# ~/.forge/
# ├── config.yaml       # Main configuration
# ├── launchers/        # Worker launcher scripts
# ├── workers/          # Worker configuration templates
# ├── logs/             # Worker logs
# ├── status/           # Worker status files
# └── layouts/          # Saved dashboard layouts
```

### Upgrading FORGE

```bash
# From PyPI
pip install --upgrade llmforge

# From source
cd forge
git pull
pip install -e .
```

---

## Quick Start

### First Launch

```bash
# Start the FORGE dashboard
forge dashboard
```

The dashboard will adapt to your terminal size (see [Beautiful TUI Dashboard](#-beautiful-tui-dashboard) for layout details).

**Ultra-Wide Mode** (199+ columns × 38+ rows) - All 6 panels visible:

```
┌─ FORGE Control Panel ──────────────────────────────────────────────────┐
│                                                                         │
│  ┌─ Workers ──────┐  ┌─ Tasks ─────────┐  ┌─ Costs ──────────┐       │
│  │ sonnet-alpha   │  │ fg-abc  [P0]   │  │ Today: $12.34   │       │
│  │ opus-beta      │  │ fg-def  [P1]   │  │ Week:  $87.65   │       │
│  │ haiku-gamma    │  │ fg-ghi  [P2]   │  │                  │       │
│  └────────────────┘  └─────────────────┘  └──────────────────┘       │
│                                                                         │
│  ┌─ Metrics ───────┐  ┌─ Activity Log ────┐  ┌─ Chat ─────────────┐   │
│  │ Tasks/hr: 12.5  │  │ Worker spawned...  │  │ :                  │   │
│  │ Avg time: 8.3m │  │ Task completed...  │  │ Type : to chat     │   │
│  │ Success: 94.2%  │  │ Optimizing costs...│  │                    │   │
│  └─────────────────┘  └────────────────────┘  └────────────────────┘   │
│                                                                         │
│  [Press : for chat | ? for help | q to quit]                            │
└─────────────────────────────────────────────────────────────────────────┘
```

**Narrow/Wide Mode** (<199 columns) - Use hotkeys to switch between views (see [Beautiful TUI Dashboard](#-beautiful-tui-dashboard) for details).

### Your First Commands

**Using the conversational interface (press `:`):**

```bash
# Press : to activate chat, then type:

"Show me all workers"
"Spawn 2 sonnet workers"
"What did I spend today?"
"Show P0 tasks"
"Optimize my costs"
```

**Using hotkeys (optional):**

| Key | Action |
|-----|--------|
| `:` | Activate chat (primary interface) |
| `W` | Workers view |
| `T` | Tasks view |
| `C` | Costs view |
| `M` | Metrics view |
| `L` | Activity log view |
| `O` | Overview dashboard |
| `S` | Spawn worker |
| `K` | Kill worker |
| `?` | Help |
| `q` | Quit |

### Traditional CLI Commands

```bash
# Spawn workers
forge spawn --model=sonnet --count=3

# Check status
forge status

# Optimize costs
forge optimize

# Validate configuration
forge validate-config

# Get/set config values
forge get dashboard.refresh_interval_ms
forge set dashboard.refresh_interval_ms 500
```

---

## CLI Setup Guide

FORGE integrates with AI coding tools through "headless" backends - CLI tools that accept structured input and return structured output.

### Claude Code Setup

```bash
# 1. Install Claude Code
npm install -g @anthropic-ai/claude-code

# 2. Set up environment
export ANTHROPIC_API_KEY="sk-ant-..."

# 3. Test headless mode
echo '{"message": "hello", "tools": []}' | \
  claude-code chat --headless --tools=/dev/null

# 4. Create launcher script
cat > ~/.forge/launchers/claude-code-launcher << 'EOF'
#!/bin/bash
MODEL="$1"
WORKSPACE="$2"
SESSION_NAME="$3"

# Create directories
mkdir -p ~/.forge/logs ~/.forge/status

# Launch Claude Code in tmux
tmux new-session -d -s "$SESSION_NAME" \
  "cd $WORKSPACE && claude-code --model=$MODEL 2>&1 | tee ~/.forge/logs/$SESSION_NAME.log"

# Get PID
PID=$(tmux list-panes -t "$SESSION_NAME" -F '#{pane_pid}')

# Output metadata (JSON)
cat << JSON
{"worker_id": "$SESSION_NAME", "pid": $PID, "status": "spawned"}
JSON

# Create status file
cat > ~/.forge/status/$SESSION_NAME.json << EOF
{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "workspace": "$WORKSPACE",
  "pid": $PID,
  "started_at": "$(date -Iseconds)"
}
EOF
EOF

chmod +x ~/.forge/launchers/claude-code-launcher

# 5. Configure FORGE
cat >> ~/.forge/config.yaml << 'EOF'
chat_backend:
  command: "claude-code"
  args:
    - "chat"
    - "--headless"
    - "--tools=${FORGE_TOOLS_FILE}"
  model: "sonnet"
  env:
    ANTHROPIC_API_KEY: "${ANTHROPIC_API_KEY}"
    FORGE_TOOLS_FILE: "~/.forge/tools.json"

launchers:
  claude-code:
    executable: "~/.forge/launchers/claude-code-launcher"
    models: ["sonnet", "opus", "haiku"]
EOF

# 6. Generate tool definitions
forge generate-tools > ~/.forge/tools.json
```

### Aider Setup

```bash
# 1. Install Aider
pip install aider-chat

# 2. Set up environment
export OPENAI_API_KEY="sk-..."  # or ANTHROPIC_API_KEY for Claude

# 3. Create launcher script
cat > ~/.forge/launchers/aider-launcher << 'EOF'
#!/bin/bash
MODEL="$1"
WORKSPACE="$2"
SESSION_NAME="$3"

# Create directories
mkdir -p ~/.forge/logs ~/.forge/status

# Map model names
case "$MODEL" in
  "sonnet") MODEL_ARG="claude-sonnet-4-5" ;;
  "opus") MODEL_ARG="claude-opus-4-5" ;;
  "gpt4") MODEL_ARG="gpt-4" ;;
  *) MODEL_ARG="$MODEL" ;;
esac

# Launch Aider in tmux
tmux new-session -d -s "$SESSION_NAME" \
  "cd $WORKSPACE && aider --model=$MODEL_ARG 2>&1 | tee ~/.forge/logs/$SESSION_NAME.log"

# Get PID
PID=$(tmux list-panes -t "$SESSION_NAME" -F '#{pane_pid}')

# Output metadata (JSON)
cat << JSON
{"worker_id": "$SESSION_NAME", "pid": $PID, "status": "spawned"}
JSON

# Create status file
cat > ~/.forge/status/$SESSION_NAME.json << EOF
{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "workspace": "$WORKSPACE",
  "pid": $PID,
  "started_at": "$(date -Iseconds)"
}
EOF
EOF

chmod +x ~/.forge/launchers/aider-launcher

# 4. Configure FORGE
cat >> ~/.forge/config.yaml << 'EOF'
launchers:
  aider:
    executable: "~/.forge/launchers/aider-launcher"
    models: ["sonnet", "opus", "gpt4"]
EOF
```

### Continue.dev Setup

```bash
# 1. Install Continue
# Continue is typically installed as an extension, but can be used via CLI
# See: https://continue.dev/reference/headless

# 2. Create launcher script
cat > ~/.forge/launchers/continue-launcher << 'EOF'
#!/bin/bash
MODEL="$1"
WORKSPACE="$2"
SESSION_NAME="$3"

# Create directories
mkdir -p ~/.forge/logs ~/.forge/status

# Continue uses config file for model selection
# Launch Continue headless mode
tmux new-session -d -s "$SESSION_NAME" \
  "cd $WORKSPACE && continue --headless --model=$MODEL 2>&1 | tee ~/.forge/logs/$SESSION_NAME.log"

# Get PID
PID=$(tmux list-panes -t "$SESSION_NAME" -F '#{pane_pid}')

# Output metadata (JSON)
cat << JSON
{"worker_id": "$SESSION_NAME", "pid": $PID, "status": "spawned"}
JSON

# Create status file
cat > ~/.forge/status/$SESSION_NAME.json << EOF
{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "workspace": "$WORKSPACE",
  "pid": $PID,
  "started_at": "$(date -Iseconds)"
}
EOF
EOF

chmod +x ~/.forge/launchers/continue-launcher

# 4. Configure FORGE
cat >> ~/.forge/config.yaml << 'EOF'
launchers:
  continue:
    executable: "~/.forge/launchers/continue-launcher"
    models: ["gpt4", "sonnet", "claude"]
EOF
```

### Custom Backend Wrapper

If your AI tool doesn't support headless mode, create a wrapper:

```python
#!/usr/bin/env python3
# ~/.forge/backends/custom-wrapper.py

import json
import sys
from anthropic import Anthropic

def main():
    # Read FORGE input
    input_data = json.load(sys.stdin)
    message = input_data["message"]
    tools = input_data.get("tools", [])

    # Call your AI tool
    client = Anthropic(api_key=os.environ.get("ANTHROPIC_API_KEY"))
    response = client.messages.create(
        model="claude-sonnet-4-5",
        messages=[{"role": "user", "content": message}],
        tools=tools
    )

    # Extract tool calls
    tool_calls = []
    for block in response.content:
        if block.type == "tool_use":
            tool_calls.append({
                "id": block.id,
                "tool": block.name,
                "arguments": block.input
            })

    # Return FORGE output
    result = {
        "tool_calls": tool_calls,
        "message": response.content[0].text if response.content else "",
        "requires_confirmation": False
    }

    print(json.dumps(result))

if __name__ == "__main__":
    main()
```

### Testing Your Backend

```bash
# Test manually
echo '{"message": "show workers", "tools": []}' | \
  your-backend-command --args

# Expected output:
# {"tool_calls": [{"tool": "switch_view", "arguments": {"view": "workers"}}]}

# Test with FORGE (when available)
forge test-backend --verbose
```

---

## Configuration

### Main Configuration File

Location: `~/.forge/config.yaml`

```yaml
# Chat backend configuration
chat_backend:
  command: "claude-code"
  args:
    - "chat"
    - "--headless"
    - "--tools=${FORGE_TOOLS_FILE}"
  model: "sonnet"
  env:
    ANTHROPIC_API_KEY: "${ANTHROPIC_API_KEY}"
    FORGE_TOOLS_FILE: "~/.forge/tools.json"
  timeout: 30
  max_retries: 3

# Worker launcher configurations
launchers:
  claude-code:
    executable: "~/.forge/launchers/claude-code-launcher"
    models: ["sonnet", "opus", "haiku"]
    default_args:
      - "--tmux"

# Worker configuration repositories
worker_repos:
  - url: "https://github.com/forge-community/worker-configs"
    branch: "main"
    path: "configs/"

# Log collection settings
log_collection:
  paths:
    - "~/.forge/logs/*.log"
  format: "jsonl"  # or "keyvalue" or "auto-detect"
  poll_interval_seconds: 1
  max_age_days: 30
  max_size_mb: 1000

# Status file location
status_path: "~/.forge/status/"

# Cost tracking
cost_tracking:
  enabled: true
  database_path: "~/.forge/costs.db"
  forecast_days: 30

# Dashboard settings
dashboard:
  refresh_interval_ms: 1000
  max_fps: 60
  default_layout: "overview"

# Hotkey customization
hotkeys:
  workers_view: "W"
  tasks_view: "T"
  costs_view: "C"
  metrics_view: "M"
  logs_view: "L"
  overview: "O"
  spawn_worker: "S"
  kill_worker: "K"
  chat_input: ":"

# Model routing (cost optimization)
routing:
  # Tier assignments for task priorities
  priority_tiers:
    P0: "premium"    # Use opus for critical tasks
    P1: "premium"    # Use opus for high priority
    P2: "standard"   # Use sonnet for normal tasks
    P3: "budget"     # Use haiku for low priority
    P4: "budget"     # Use haiku for backlog

  # Subscription optimization
  subscription_first: true
  fallback_to_api: true

  # Task scoring weights (0-100 scale)
  scoring_weights:
    priority: 0.4      # 40 points for P0, 30 for P1, etc.
    blockers: 0.3      # 10 points per blocked task, max 30
    age: 0.2          # Older tasks get more points, max 20
    labels: 0.1       # Critical/urgent labels give 10 points
```

### Environment Variables

```bash
# Override config location
export FORGE_CONFIG=~/.config/forge/config.yaml

# Override directories
export FORGE_LAUNCHERS_DIR=~/custom-launchers
export FORGE_WORKERS_DIR=~/custom-workers
export FORGE_LOGS_DIR=/var/log/forge

# API keys (used by workers)
export ANTHROPIC_API_KEY="sk-ant-..."
export OPENAI_API_KEY="sk-..."

# Enable debug logging
export FORGE_DEBUG=1
export FORGE_LOG_LEVEL=debug

# Specify workspace
export FORGE_WORKSPACE=/path/to/project
```

---

## Documentation

- **[User Guide](docs/USER_GUIDE.md)** - Complete installation, configuration, and usage guide
- **[Integration Guide](docs/INTEGRATION_GUIDE.md)** - Integrate external tools with FORGE
- **[Tool Catalog](docs/TOOL_CATALOG.md)** - Reference for all 45+ tools
- **[Hotkeys Reference](docs/HOTKEYS.md)** - Keyboard shortcuts
- **[ADRs](docs/adr/)** - Architecture Decision Records

Comprehensive research documentation available in [`docs/notes/`](./docs/notes/):

- **Naming Analysis**: Why "FORGE" and alternatives considered
- **TUI Design**: Dashboard layouts, responsive strategies
- **Cost Optimization**: Subscription vs API analysis, 87-94% savings strategies
- **Model Routing**: Task scoring algorithms, capability matching
- **Conversational Interface**: Chat history, tool transparency, scrolling
- **System Architecture**: Complete system architecture diagrams
- **Algorithm Design**: Task assignment, adaptive learning, deadlock detection

---

## Troubleshooting

### FORGE Won't Start

```bash
# Check Python version
python --version  # Must be 3.10+

# Check FORGE installation
forge --version

# Check configuration
forge validate

# Debug mode
FORGE_DEBUG=1 forge

# Common issues:
# - Python version too old → Upgrade Python
# - Dependencies missing → pip install -e ".[dev]"
# - Config syntax error → forge validate
# - Port in use → Check for running forge processes
```

### Workers Won't Spawn

```bash
# Check launcher script
ls -la ~/.forge/launchers/
chmod +x ~/.forge/launchers/*

# Test launcher manually
~/.forge/launchers/claude-code-launcher \
  --model=sonnet \
  --workspace=/tmp/test \
  --session-name=test

# Expected output: JSON with worker_id, pid, status

# Check for required tools
which tmux
which claude-code  # or other AI tool

# Common issues:
# - Launcher not executable → chmod +x
# - Missing dependencies → Install AI tool
# - Invalid arguments → Check launcher syntax
# - Workspace doesn't exist → Create directory
```

### Chat Backend Not Responding

```bash
# Test backend manually
echo '{"message": "test", "tools": []}' | \
  claude-code chat --headless --tools=/dev/null

# Check API keys
echo $ANTHROPIC_API_KEY  # Should be set

# Test tool definitions
# (when available) forge validate-tools ~/.forge/tools.json

# Check backend logs
FORGE_DEBUG=1 forge 2>&1 | grep backend

# Common issues:
# - API key not set → Export ANTHROPIC_API_KEY
# - Backend not installed → Install AI tool
# - Tools file invalid → Generate tools.json
# - Timeout too short → Increase timeout in config
```

### Workers Not Showing Up

```bash
# Check status files
ls -la ~/.forge/status/
cat ~/.forge/status/sonnet-alpha.json

# Check log files
ls -la ~/.forge/logs/
tail ~/.forge/logs/sonnet-alpha.log

# Check tmux sessions
tmux list-sessions

# Verify worker process
ps aux | grep sonnet-alpha

# Common issues:
# - Status file not created → Check launcher script
# - Status file invalid → Validate JSON format
# - Worker crashed → Check log file for errors
# - Tmux session died → Restart worker
```

### Dashboard Not Updating

```bash
# Check inotify availability
python -c "from watchdog.observers import Observer; print('OK')"

# Verify refresh interval
forge get dashboard.refresh_interval_ms

# Force refresh
# Press Ctrl+L in dashboard

# Common issues:
# - inotify not available → Falls back to polling
# - Refresh too slow → Decrease interval
# - High CPU usage → Increase interval
# - UI frozen → Press Ctrl+L to redraw
```

### High Memory/CPU Usage

```bash
# Check log file sizes
du -sh ~/.forge/logs/

# Rotate logs if needed
# (when available) forge rotate-logs --max-age=7

# Check database size
du -sh ~/.forge/costs.db

# Reduce history retention
forge set log_collection.max_age_days 7
forge set log_collection.max_size_mb 500

# Common issues:
# - Large log files → Rotate logs
# - Large database → Vacuum or recreate
# - Too many workers → Kill idle workers
# - High refresh rate → Increase interval
```

### Getting Help

```bash
# Built-in help
: help  # Type in chat interface
?  # Press ? key

# Documentation
forge docs  # Opens https://forge.readthedocs.io (when available)

# Report issues
# https://github.com/jedarden/forge/issues

# Community
# Discord: https://discord.gg/forge (when available)
```

---

## Project Status

🚧 **Active Development** - Research phase complete, implementation in progress

See [`docs/notes/`](./docs/notes/) for comprehensive design documentation and architecture decisions.

---

## Development Roadmap

### Phase 1: MVP (Current)
- [x] Basic TUI dashboard implementation
- [x] Worker spawning and management
- [x] Task queue integration (Beads)
- [x] Real-time status monitoring
- [ ] Log parsing and metrics extraction
- [ ] Cost tracking implementation

### Phase 2: Intelligence
- [ ] Task value scoring algorithm
- [ ] Multi-model routing engine
- [ ] Cost optimization logic
- [ ] Subscription tracking

### Phase 3: Advanced Features
- [ ] Conversational CLI interface (fully implemented)
- [ ] Hot-reload and self-updating
- [ ] Advanced health monitoring
- [ ] Performance analytics

### Phase 4: Enterprise
- [ ] Multi-workspace coordination
- [ ] Team collaboration features
- [ ] Audit logs and compliance
- [ ] Advanced RBAC

---

## Contributing

Contributions welcome! This project is in active development.

1. Check out the [research documentation](./docs/notes/) to understand the architecture
2. Open issues for bugs, features, or questions
3. Submit PRs with clear descriptions and tests

### Development Setup

```bash
# Clone the repository
git clone https://github.com/jedarden/forge.git
cd forge

# Create virtual environment
python -m venv venv
source venv/bin/activate

# Install with dev dependencies
pip install -e ".[dev]"

# Run tests
pytest

# Run linter
ruff check src/
ruff format src/

# Run type checker
mypy src/
```

---

## License

MIT License - see [LICENSE](LICENSE) for details

---

## Why "FORGE"?

The name reflects the core mission:

- **Federated**: Coordinate across multiple AI providers and models
- **Orchestration**: Intelligent task distribution and scheduling
- **Resource**: Optimize subscription and API usage
- **Generation**: Spawn workers dynamically based on demand
- **Engine**: Powerful, efficient, reliable automation

Like a blacksmith's forge that transforms raw materials into refined tools, FORGE transforms AI resources into optimized, coordinated intelligence.

---

## Acknowledgments

Built with insights from the AI coding tools community, optimized for real-world use cases where managing multiple AI subscriptions and workers becomes complex.

**FORGE** - *Where AI agents are forged, orchestrated, and optimized.*
