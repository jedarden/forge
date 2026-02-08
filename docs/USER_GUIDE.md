# FORGE User Guide

**Complete guide to installing, configuring, and using FORGE**

**FORGE** (Federated Orchestration & Resource Generation Engine) is a terminal-based control panel for intelligently managing multiple AI coding agents across your workspace. Think of it as "Kubernetes for AI workers" - it orchestrates agents, optimizes costs, and provides real-time monitoring through a beautiful TUI.

---

## Table of Contents

1. [Installation](#installation)
2. [Quick Start](#quick-start)
3. [Configuration](#configuration)
4. [Setting Up Headless CLIs](#setting-up-headless-clis)
5. [Basic Usage](#basic-usage)
6. [Tool Catalog](#tool-catalog)
7. [Hotkeys Reference](#hotkeys-reference)
8. [Multi-Workspace Usage](#multi-workspace-usage)
9. [Troubleshooting](#troubleshooting)
10. [Advanced Topics](#advanced-topics)

---

## Installation

### Prerequisites

FORGE requires:
- **Python 3.10 or higher**
- **Linux, macOS, or WSL2** (Windows support coming soon)
- **tmux** (for worker session management)
- **Git** (for workspace and repository features)

### Installing FORGE

```bash
# Option 1: Install from PyPI (when available)
pip install llmforge

# Option 2: Install from source
git clone https://github.com/jedarden/forge.git
cd forge
pip install -e .

# Option 3: Install with development dependencies
pip install -e ".[dev]"

# Verify installation
forge --version
```

### Post-Installation Setup

```bash
# Create FORGE directory structure
forge init

# This creates:
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

You'll see the 6-panel dashboard:

```
┌─ FORGE Control Panel ──────────────────────────────────────────────────┐
│                                                                         │
│  ┌─ Workers ──────┐  ┌─ Tasks ─────────┐  ┌─ Costs ──────────┐       │
│  │ sonnet-alpha   │  │ bd-abc  [P0]   │  │ Today: $12.34   │       │
│  │ opus-beta      │  │ bd-def  [P1]   │  │ Week:  $87.65   │       │
│  │ haiku-gamma    │  │ bd-ghi  [P2]   │  │                  │       │
│  └────────────────┘  └─────────────────┘  └──────────────────┘       │
│                                                                         │
│  ┌─ Metrics ───────┐  ┌─ Activity Log ────┐  ┌─ Chat ─────────────┐   │
│  │ Tasks/hr: 12.5  │  │ Worker spawned...  │  │ :                  │   │
│  │ Avg time: 8.3m │  │ Task completed...  │  │ Type : to chat     │   │
│  │ Success: 94.2%  │  │ Optimizing costs...│  │                    │   │
│  └─────────────────┘  └────────────────────┘  └────────────────────┘   │
│                                                                         │
│  [Press ? for help | q to quit]                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### Your First Commands

```bash
# Press : to activate chat, then type:

# Show worker status
"Show me all workers"

# Spawn a new worker
"Spawn 2 sonnet workers"

# View costs
"What did I spend today?"

# Get help
"Help"
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

  opencode:
    executable: "~/.forge/launchers/opencode-launcher"
    models: ["gpt4", "gpt35"]
    default_args:
      - "--background"

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

## Setting Up Headless CLIs

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

# 4. Configure FORGE
cat >> ~/.forge/config.yaml << 'EOF'
chat_backend:
  command: "claude-code"
  args:
    - "chat"
    - "--headless"
    - "--tools=${FORGE_TOOLS_FILE}"
  model: "sonnet"
EOF

# 5. Create tool definitions
forge generate-tools > ~/.forge/tools.json
```

### OpenCode Setup

```bash
# 1. Install OpenCode
pip install opencode-cli

# 2. Set up environment
export OPENAI_API_KEY="sk-..."

# 3. Test API mode
echo '{"message": "test"}' | opencode --mode=api --tools=/dev/null

# 4. Configure FORGE
cat >> ~/.forge/config.yaml << 'EOF'
chat_backend:
  command: "opencode"
  args:
    - "--mode=api"
    - "--tools=${FORGE_TOOLS_FILE}"
  model: "gpt4"
EOF
```

### Custom Backend Wrapper

If your AI tool doesn't support headless mode, create a wrapper:

```python
#!/usr/bin/env python3
# ~/.forge/backends/custom-wrapper.py

import json
import sys
from your_ai_library import Client

def main():
    # Read FORGE input
    input_data = json.load(sys.stdin)
    message = input_data["message"]
    tools = input_data.get("tools", [])

    # Call your AI tool
    client = Client(api_key="your-api-key")
    response = client.chat(
        message=message,
        tools=tools,
        model="your-model"
    )

    # Extract tool calls
    tool_calls = []
    for tool_use in response.tool_calls:
        tool_calls.append({
            "id": tool_use.id,
            "tool": tool_use.name,
            "arguments": tool_use.arguments
        })

    # Return FORGE output
    result = {
        "tool_calls": tool_calls,
        "message": response.message,
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

# Test with FORGE
forge test-backend --verbose
```

---

## Basic Usage

### Starting FORGE

```bash
# Basic start
forge

# Start with specific workspace
forge --workspace /path/to/project

# Start with custom config
forge --config /path/to/config.yaml

# Start with debug logging
FORGE_DEBUG=1 forge
```

### Navigation

#### Chat Interface (Primary)

Press `:` to activate the chat input:

```
┌─ Chat ──────────────────────────────────┐
│ : Show me all P0 tasks                   │
│                                          │
│ [Enter to send | Esc to cancel]         │
└──────────────────────────────────────────┘
```

Type natural language commands:
- "Show me all workers"
- "Spawn 3 sonnet workers"
- "What did I spend this week?"
- "Kill idle workers"
- "Optimize my costs"

#### Hotkey Navigation (Optional)

For power users, hotkeys provide quick access:

| Key | Action |
|-----|--------|
| `:` | Activate chat |
| `W` | Workers view |
| `T` | Tasks view |
| `C` | Costs view |
| `M` | Metrics view |
| `L` | Logs view |
| `O` | Overview dashboard |
| `S` | Spawn worker |
| `K` | Kill worker |
| `?` | Help |
| `q` | Quit |

### Managing Workers

#### Spawning Workers

```bash
# Via chat
: Spawn 3 sonnet workers
: Start 2 opus workers in the trading workspace
: Launch a haiku worker

# Via hotkey
S  # Prompts for model and count

# Via CLI (separate terminal)
forge spawn --model=sonnet --count=3
forge spawn --model=opus --workspace=/path/to/project
```

#### Monitoring Workers

The Workers panel shows:
- **Worker ID**: Name (e.g., `sonnet-alpha`)
- **Status**: Active, idle, failed, or error
- **Model**: Which AI model it's using
- **Current Task**: What it's working on
- **Tasks Completed**: How many tasks finished
- **Uptime**: How long it's been running

#### Killing Workers

```bash
# Via chat
: Kill worker sonnet-alpha
: Stop all idle workers
: Terminate the failed workers

# Via hotkey
K  # Prompts for worker selection

# Via CLI
forge kill --worker=sonnet-alpha
forge kill --all --filter=idle
```

#### Restarting Workers

```bash
# Via chat
: Restart worker sonnet-beta
: Restart all failed workers

# Via hotkey (when worker is selected)
R
```

### Managing Tasks

FORGE integrates with **Beads** - a task/bead tracking system stored in `.beads/*.jsonl` files.

#### Viewing Tasks

```bash
# Via chat
: Show me all tasks
: Show P0 tasks only
: Show blocked tasks

# Via hotkey
T  # Switch to tasks view
```

#### Filtering Tasks

```bash
# By priority
: Show P0 tasks
: Show P1 and P2 tasks

# By status
: Show in-progress tasks
: Show blocked tasks

# By labels
: Show tasks with "urgent" label
```

#### Creating Tasks

```bash
# Via chat
: Create a P0 task: Fix the login bug
: Add a P1 task for database optimization
: Create task: "Investigate API timeout" priority P2

# Via CLI
forge task create "Fix login bug" --priority P0
forge task create "Optimize queries" --priority P1 --description "..."
```

#### Assigning Tasks

```bash
# Via chat
: Assign bd-abc to sonnet-alpha
: Assign the top task to any worker
: Distribute P0 tasks to available workers
```

### Cost Management

#### Viewing Costs

```bash
# Via chat
: What did I spend today?
: Show costs this week
: Display cost breakdown by model
: Show costs by workspace

# Via hotkey
C  # Switch to costs view
```

The Costs panel shows:
- **Current period**: Today/week/month costs
- **By model**: Per-model cost breakdown
- **By worker**: Which workers spent what
- **Forecast**: Projected costs based on usage

#### Optimizing Costs

```bash
# Via chat
: Optimize my costs
: How can I save money?
: Show cost optimization recommendations

FORGE will analyze:
# - Subscription usage vs API fallback
# - Model-to-task efficiency
# - Worker utilization
# - Task routing opportunities
```

#### Cost Forecasting

```bash
# Via chat
: Forecast costs for next month
: What will I spend in 30 days?
: Project costs if I add 3 workers
```

### Exporting Data

```bash
# Export logs
: Export today's logs as CSV
: Save logs to /tmp/forge-logs.json

# Export metrics
: Export performance metrics
: Save cost data as CSV

# Take screenshot
: Take a screenshot
: Screenshot the costs panel
```

---

## Tool Catalog

FORGE provides **45 tools** organized into 11 categories. The chat interface translates your natural language into tool calls automatically.

### View Control (3 tools)

| Tool | Description | Example |
|------|-------------|---------|
| `switch_view` | Switch dashboard view | "Show me workers" |
| `split_view` | Create split-screen layout | "Split: workers and tasks" |
| `focus_panel` | Focus on specific panel | "Expand the activity log" |

### Worker Management (5 tools)

| Tool | Description | Example |
|------|-------------|---------|
| `spawn_worker` | Spawn AI workers | "Spawn 3 sonnet workers" |
| `kill_worker` | Terminate workers | "Kill worker sonnet-alpha" |
| `list_workers` | List workers with filters | "Show idle workers" |
| `restart_worker` | Restart worker | "Restart the failed worker" |
| `ping_worker` | Check worker responsiveness | "Ping sonnet-alpha" |

### Task Management (3 tools)

| Tool | Description | Example |
|------|-------------|---------|
| `filter_tasks` | Filter task queue | "Show P0 tasks" |
| `create_task` | Create new task | "Create P0 task: Fix bug" |
| `assign_task` | Assign task to worker | "Assign bd-abc to sonnet-alpha" |

### Cost & Analytics (4 tools)

| Tool | Description | Example |
|------|-------------|---------|
| `show_costs` | Display cost analysis | "What did I spend?" |
| `optimize_routing` | Run cost optimization | "Optimize my costs" |
| `forecast_costs` | Forecast future costs | "Forecast next month" |
| `show_metrics` | Display performance metrics | "Show throughput" |

### Data Export (3 tools)

| Tool | Description | Example |
|------|-------------|---------|
| `export_logs` | Export activity logs | "Export logs as CSV" |
| `export_metrics` | Export metrics data | "Save cost metrics" |
| `screenshot` | Take screenshot | "Screenshot dashboard" |

### Configuration (4 tools)

| Tool | Description | Example |
|------|-------------|---------|
| `set_config` | Update configuration | "Set default model to opus" |
| `get_config` | View configuration | "What's my config?" |
| `save_layout` | Save current layout | "Save this layout" |
| `load_layout` | Load saved layout | "Load monitoring layout" |

### Help & Discovery (3 tools)

| Tool | Description | Example |
|------|-------------|---------|
| `help` | Get help on topic | "Help with spawning" |
| `search_docs` | Search documentation | "How does routing work?" |
| `list_capabilities` | List all tools | "What can you do?" |

### Notification (4 tools)

| Tool | Description | Example |
|------|-------------|---------|
| `show_notification` | Display notification | "Show task complete" |
| `show_warning` | Display warning | "Warn about high costs" |
| `ask_user` | Prompt for input | "Ask to confirm" |
| `highlight_beads` | Highlight tasks | "Highlight P0 tasks" |

### System (6 tools)

| Tool | Description | Example |
|------|-------------|---------|
| `get_status` | Get system status | "What's the status?" |
| `refresh` | Refresh data | "Refresh all views" |
| `pause_worker` | Pause worker | "Pause sonnet-alpha" |
| `resume_worker` | Resume worker | "Resume sonnet-alpha" |
| `get_worker_info` | Get worker details | "Show info for sonnet-alpha" |

### Workspace (4 tools)

| Tool | Description | Example |
|------|-------------|---------|
| `switch_workspace` | Switch workspace | "Switch to /path/to/project" |
| `list_workspaces` | List workspaces | "Show all workspaces" |
| `create_workspace` | Create workspace | "Create new workspace" |
| `get_workspace_info` | Get workspace info | "Show workspace info" |

### Analytics (7 tools)

| Tool | Description | Example |
|------|-------------|---------|
| `show_throughput` | Task throughput metrics | "Show throughput today" |
| `show_latency` | Task latency metrics | "Show latency this week" |
| `show_success_rate` | Success rate metrics | "Show success rate" |
| `show_worker_efficiency` | Worker comparison | "Show worker efficiency" |
| `show_task_distribution` | Task distribution | "Show task distribution" |
| `show_trends` | Metric trends over time | "Show cost trends" |
| `analyze_bottlenecks` | Find bottlenecks | "Find bottlenecks" |

### Tool Chaining

FORGE can chain multiple tools:

```bash
# Complex request
: Show P0 tasks and spawn 2 workers if count > 5, then show costs

# This executes:
1. filter_tasks(priority="P0")
2. [conditional] spawn_worker("sonnet", 2)  # Only if task count > 5
3. switch_view("costs")
```

---

## Hotkeys Reference

### Philosophy

Hotkeys are **optional shortcuts**. Everything can be done via chat (`:`). Use hotkeys for actions you repeat frequently.

### Global Hotkeys

| Key | Action | Notes |
|-----|--------|-------|
| `:` | Activate chat | Primary interface |
| `?` or `h` | Show help | Context-aware help |
| `q` | Quit FORGE | Confirms if workers active |
| `Esc` | Cancel operation | Returns to previous state |
| `Ctrl+C` | Force quit | Immediate exit |
| `Ctrl+L` | Clear/refresh | Redraw screen |

### View Navigation

| Key | View | Chat Equivalent |
|-----|------|-----------------|
| `W` | Workers | "show workers" |
| `T` | Tasks | "show tasks" |
| `C` | Costs | "show costs" |
| `M` | Metrics | "show metrics" |
| `L` | Logs | "show logs" |
| `O` | Overview | "show overview" |
| `Tab` | Next view | "next view" |
| `Shift+Tab` | Previous view | "previous view" |

### Worker Management

| Key | Action | Chat Equivalent |
|-----|--------|-----------------|
| `S` | Spawn worker | "spawn worker" |
| `K` | Kill worker | "kill worker" |
| `R` | Restart worker | "restart worker" |
| `Ctrl+S` | Quick spawn | "spawn a worker" |

### Task Management

| Key | Action | Chat Equivalent |
|-----|--------|-----------------|
| `N` | New task | "create task" |
| `F` | Filter tasks | "filter tasks" |
| `A` | Assign task | "assign task" |
| `/` | Search tasks | "search [query]" |
| `0-4` | Priority filter | "show P0 tasks" |

### Panel Navigation

| Key | Action |
|-----|--------|
| `↑/↓` | Scroll up/down |
| `PgUp/PgDn` | Page up/down |
| `Home/End` | Jump to top/bottom |
| `Enter` | Select/expand |
| `Space` | Toggle selection |

### Advanced

| Key | Action | Chat Equivalent |
|-----|--------|-----------------|
| `X` | Export view | "export [data]" |
| `P` | Screenshot | "screenshot" |
| `[` | Save layout | "save layout" |
| `]` | Load layout | "load layout" |
| `Ctrl+O` | Optimize routing | "optimize costs" |
| `Ctrl+F` | Forecast costs | "forecast costs" |

### Customization

Customize hotkeys in `~/.forge/config.yaml`:

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

### Learning Path

**Week 1**: Use chat exclusively
- "show workers" → "show tasks" → "spawn worker"

**Week 2**: Notice hotkey hints
- "Worker view (Press W to return here)"

**Week 3**: Start using hotkeys
- W → T → C (view navigation)

**Week 4**: Hybrid workflow
- W [hotkey] : [chat for complex] T [hotkey]

---

## Multi-Workspace Usage

FORGE can manage workers across multiple project workspaces.

### Workspace Discovery

FORGE automatically discovers workspaces:

1. **Current directory**: `.beads/` directory marks a workspace
2. **Git repositories**: Any git repo with `.beads/`
3. **Manual registration**: Add to `~/.forge/config.yaml`

```yaml
# Manual workspace registration
workspaces:
  - path: ~/projects/trading-bot
    name: trading-bot
    priority: high

  - path: ~/projects/website
    name: website
    priority: normal
```

### Switching Workspaces

```bash
# Via chat
: Switch to trading-bot workspace
: Change workspace to ~/projects/website

# Via CLI
forge workspace --switch ~/projects/trading-bot

# List workspaces
: Show all workspaces
forge workspace --list
```

### Per-Workspace Workers

Workers can be workspace-scoped:

```bash
# Spawn worker in specific workspace
: Spawn 2 sonnet workers in trading-bot

# Workers inherit workspace context
# - Read workspace .beads/ for tasks
# - Log to workspace-specific files
# - Respect workspace configuration
```

### Cross-Workspace Operations

```bash
# View costs across all workspaces
: Show costs by workspace

# Move workers between workspaces
: Move sonnet-alpha to trading-bot

# Aggregate metrics
: Show task throughput across all workspaces
```

### Workspace Configuration

Per-workspace overrides in `~/.forge/config.yaml`:

```yaml
workspace_overrides:
  ~/projects/trading-bot:
    default_model: "opus"
    max_workers: 5
    routing:
      priority_tiers:
        P0: "premium"
        P1: "premium"
        P2: "standard"

  ~/projects/website:
    default_model: "sonnet"
    max_workers: 3
```

---

## Troubleshooting

### FORGE Won't Start

```bash
# Check Python version
python --version  # Must be 3.10+

# Check FORGE installation
forge --version

# Check configuration
forge validate-config

# Debug mode
FORGE_DEBUG=1 forge

# Common issues:
# - Python version too old → Upgrade Python
# - Dependencies missing → pip install -e ".[dev]"
# - Config syntax error → forge validate-config
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
  --session-name-test

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
forge validate-tools ~/.forge/tools.json

# Check backend logs
FORGE_DEBUG=1 forge 2>&1 | grep backend

# Common issues:
# - API key not set → Export ANTHROPIC_API_KEY
# - Backend not installed → Install AI tool
# - Tools file invalid → forge generate-tools
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

### Costs Not Tracking

```bash
# Check cost tracking enabled
forge get-config cost_tracking

# Verify database
ls -la ~/.forge/costs.db
sqlite3 ~/.forge/costs.db "SELECT * FROM costs LIMIT 10;"

# Check log format
head ~/.forge/logs/*.log | jq .

# Verify logs have cost events
grep "cost_incurred" ~/.forge/logs/*.log

# Common issues:
# - Cost tracking disabled → Enable in config
# - Database corrupted → Delete and recreate
# - Log format invalid → Use JSONL format
# - Missing cost events → Check worker logging
```

### Dashboard Not Updating

```bash
# Check inotify availability
python -c "from watchdog.observers import Observer; print('OK')"

# Verify refresh interval
forge get-config dashboard.refresh_interval_ms

# Force refresh
: refresh
# or press Ctrl+L

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
forge rotate-logs --max-age=7

# Check database size
du -sh ~/.forge/costs.db

# Reduce history retention
forge set-config log_collection.max_age_days 7
forge set-config log_collection.max_size_mb 500

# Common issues:
# - Large log files → Rotate logs
# - Large database → Vacuum or recreate
# - Too many workers → Kill idle workers
# - High refresh rate → Increase interval
```

### Getting Help

```bash
# Built-in help
: help
?  # Press ? key

# Documentation
forge docs
# Opens https://forge.readthedocs.io

# Report issues
forge bug-report
# Opens GitHub issue template

# Community
# Discord: https://discord.gg/forge
# GitHub: https://github.com/jedarden/forge/issues
```

---

## Advanced Topics

### Custom Worker Configurations

Create reusable worker configurations:

```yaml
# ~/.forge/workers/my-custom-worker.yaml
name: "my-custom-worker"
description: "Custom worker for specialized tasks"
version: "1.0.0"

# Launcher to use
launcher: "claude-code"
model: "sonnet"

# Cost tier
tier: "standard"
cost_per_million_tokens:
  input: 3.0
  output: 15.0

# Subscription
subscription:
  enabled: true
  monthly_cost: 20
  quota_type: "unlimited"

# Environment
environment:
  ANTHROPIC_API_KEY: "${ANTHROPIC_API_KEY}"
  CUSTOM_VAR: "value"

# Capabilities
capabilities:
  - "code_generation"
  - "code_review"
max_context_tokens: 200000
supports_tools: true
```

### Custom Launchers

Create custom launchers for any AI tool:

```bash
#!/bin/bash
# ~/.forge/launchers/custom-launcher

MODEL="$1"
WORKSPACE="$2"
SESSION_NAME="$3"

# Validate
if [[ -z "$MODEL" ]] || [[ -z "$WORKSPACE" ]] || [[ -z "$SESSION_NAME" ]]; then
  echo "Missing arguments" >&2
  exit 1
fi

# Create directories
mkdir -p ~/.forge/logs ~/.forge/status

# Launch worker (tmux, docker, subprocess, etc.)
tmux new-session -d -s "$SESSION_NAME" \
  "cd $WORKSPACE && your-ai-tool --model=$MODEL"

# Get PID
PID=$(tmux list-panes -t "$SESSION_NAME" -F '#{pane_pid}')

# Output metadata (JSON ONLY)
cat << EOF
{"worker_id": "$SESSION_NAME", "pid": $PID, "status": "spawned"}
EOF

# Create status file
cat > ~/.forge/status/$SESSION_NAME.json << EOF
{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "workspace": "$WORKSPACE",
  "pid": $PID,
  "started_at": "$(date -Iseconds)",
  "last_activity": "$(date -Iseconds)",
  "current_task": null,
  "tasks_completed": 0
}
EOF

exit 0
```

### Cost Optimization Strategies

FORGE provides several cost optimization strategies:

#### 1. Subscription-First Routing

Maximize use-or-lose subscriptions before falling back to pay-per-token:

```yaml
routing:
  subscription_first: true
  subscription_models:
    - "sonnet"  # Use until monthly quota exhausted
  fallback_models:
    - "haiku"   # Then fall back to budget model
```

#### 2. Task-to-Model Matching

Match task complexity to appropriate model:

```yaml
routing:
  priority_tiers:
    P0: "premium"   # Complex tasks → Opus
    P1: "premium"
    P2: "standard"  # Normal tasks → Sonnet
    P3: "budget"    # Simple tasks → Haiku
    P4: "budget"
```

#### 3. Worker Consolidation

Reduce idle workers:

```bash
# Kill idle workers automatically
: Kill idle workers

# Or set idle timeout
forge set-config worker.idle_timeout_minutes 30
```

#### 4. Batch Processing

Group similar tasks for efficiency:

```bash
# FORGE automatically batches tasks by:
# - Model required
# - Workspace
# - Priority level

# Monitor batching efficiency
: Show task distribution
: Analyze bottlenecks
```

### Performance Tuning

#### Refresh Rate Optimization

```yaml
dashboard:
  refresh_interval_ms: 1000  # Balance responsiveness vs CPU
  max_fps: 60
  auto_scroll: true
```

#### Log Polling Optimization

```yaml
log_collection:
  poll_interval_seconds: 1  # Check logs every second
  batch_size: 100           # Process 100 entries at once
  max_buffer_size: 1000     # Ring buffer size
```

#### Database Optimization

```bash
# Vacuum cost database
sqlite3 ~/.forge/costs.db "VACUUM;"

# Reindex
sqlite3 ~/.forge/costs.db "REINDEX;"

# Analyze query performance
sqlite3 ~/.forge/costs.db "EXPLAIN QUERY PLAN SELECT * FROM costs;"
```

### Security Best Practices

#### API Key Management

```bash
# Use environment variables (never hardcode)
export ANTHROPIC_API_KEY="sk-ant-..."

# Add to ~/.bashrc or ~/.zshrc
echo 'export ANTHROPIC_API_KEY="sk-ant-..."' >> ~/.bashrc

# FORGE inherits environment when spawning workers
# Workers get API keys via environment, not config files
```

#### Credential Isolation

```bash
# Per-user FORGE instances
# Each user has own ~/.forge/ directory
# Unix file permissions prevent access

# Workspace-specific credentials
# ~/project/.env files (gitignored)
# Loaded by workers via launcher
```

#### Audit Logging

```yaml
# Enable audit logging
audit:
  enabled: true
  log_path: "~/.forge/audit.log"
  log_events:
    - "worker_spawned"
    - "worker_killed"
    - "task_assigned"
    - "api_call"
```

---

## Next Steps

1. **Explore the documentation**:
   - [Integration Guide](INTEGRATION_GUIDE.md) - Integrate your tools
   - [Tool Catalog](TOOL_CATALOG.md) - Complete tool reference
   - [Hotkeys](HOTKEYS.md) - Keyboard shortcuts
   - [ADRs](adr/) - Architecture decision records

2. **Join the community**:
   - GitHub: https://github.com/jedarden/forge
   - Discord: https://discord.gg/forge
   - Docs: https://forge.readthedocs.io

3. **Contribute**:
   - Report bugs
   - Suggest features
   - Submit PRs
   - Share worker configs

---

**FORGE** - Federated Orchestration & Resource Generation Engine

*Where AI agents are forged, orchestrated, and optimized.*
