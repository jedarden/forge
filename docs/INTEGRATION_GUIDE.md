# FORGE Integration Guide

**Complete guide for integrating external components with FORGE**

FORGE is a "dumb orchestrator" - it has no built-in AI or worker management. This guide documents all integration surfaces so you can configure FORGE to work with your tools.

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Headless CLI Backend](#headless-cli-backend-chat)
3. [Worker Launchers](#worker-launchers)
4. [Worker Configurations](#worker-configurations)
5. [Log Collection](#log-collection)
6. [File System Layout](#file-system-layout)
7. [Examples](#complete-examples)
8. [Troubleshooting](#troubleshooting)

---

## Quick Start

### Minimal Setup

```bash
# 1. Create FORGE directory structure
mkdir -p ~/.forge/{launchers,workers,logs,status,layouts}

# 2. Create basic configuration
cat > ~/.forge/config.yaml << 'EOF'
chat_backend:
  command: "claude-code"
  args: ["chat", "--headless", "--tools=/path/to/forge-tools.json"]

launchers:
  claude-code:
    executable: "~/.forge/launchers/claude-code-launcher"
    models: ["sonnet", "opus", "haiku"]

log_paths:
  - "~/.forge/logs/*.log"

status_path: "~/.forge/status/"
EOF

# 3. Create a simple launcher
cat > ~/.forge/launchers/claude-code-launcher << 'EOF'
#!/bin/bash
# Simple Claude Code launcher

MODEL="$1"
WORKSPACE="$2"
SESSION_NAME="$3"

# Spawn in tmux
tmux new-session -d -s "$SESSION_NAME" \
  "cd $WORKSPACE && claude-code --model=$MODEL"

# Output worker metadata
echo "{\"worker_id\": \"$SESSION_NAME\", \"pid\": $$, \"status\": \"spawned\"}"

# Create status file
cat > ~/.forge/status/$SESSION_NAME.json << JSON
{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "workspace": "$WORKSPACE",
  "started_at": "$(date -Iseconds)"
}
JSON
EOF

chmod +x ~/.forge/launchers/claude-code-launcher

# 4. Launch FORGE
forge dashboard
```

---

## Headless CLI Backend (Chat)

### Purpose

Provides conversational interface by translating natural language → tool calls.

### Requirements

Your CLI tool must:
1. Accept commands on stdin (or via CLI args)
2. Return tool calls as structured JSON on stdout
3. Support tool definitions (OpenAI function calling format)
4. Handle context/history (optional but recommended)

### Protocol

#### Input Format

FORGE sends JSON on stdin:

```json
{
  "message": "Show me all P0 tasks",
  "context": {
    "current_view": "workers",
    "visible_data": {
      "workers": [
        {"id": "sonnet-alpha", "status": "active", "model": "sonnet"},
        {"id": "opus-beta", "status": "idle", "model": "opus"}
      ],
      "tasks": [
        {"id": "bd-abc", "priority": "P0", "status": "open"},
        {"id": "bd-def", "priority": "P1", "status": "in_progress"}
      ]
    }
  },
  "tools": [
    {
      "name": "filter_tasks",
      "description": "Filter task queue by criteria",
      "parameters": {
        "type": "object",
        "properties": {
          "priority": {"type": "string", "enum": ["P0", "P1", "P2", "P3", "P4"]},
          "status": {"type": "string", "enum": ["open", "in_progress", "blocked", "completed"]}
        }
      }
    }
  ]
}
```

#### Output Format

Your CLI tool returns JSON on stdout:

```json
{
  "tool_calls": [
    {
      "id": "call_1",
      "tool": "filter_tasks",
      "arguments": {
        "priority": "P0"
      }
    }
  ],
  "message": "I'll filter the task queue to show only P0 priority tasks.",
  "requires_confirmation": false
}
```

### Configuration

`~/.forge/config.yaml`:

```yaml
chat_backend:
  # Command to execute
  command: "claude-code"

  # Arguments passed to command
  args:
    - "chat"
    - "--headless"
    - "--tools=${FORGE_TOOLS_FILE}"
    - "--model=${MODEL}"

  # Model to use (if backend supports multiple)
  model: "sonnet"

  # Environment variables
  env:
    ANTHROPIC_API_KEY: "${ANTHROPIC_API_KEY}"
    FORGE_TOOLS_FILE: "~/.forge/tools.json"

  # Timeout for responses (seconds)
  timeout: 30

  # Max retries on failure
  max_retries: 3
```

### Example Backends

#### Claude Code (Headless Mode)

```yaml
chat_backend:
  command: "claude-code"
  args: ["chat", "--headless", "--tools=~/.forge/tools.json"]
  model: "sonnet"
  env:
    ANTHROPIC_API_KEY: "${ANTHROPIC_API_KEY}"
```

#### OpenCode (API Mode)

```yaml
chat_backend:
  command: "opencode"
  args: ["--mode=api", "--tools=~/.forge/tools.json"]
  model: "gpt4"
  env:
    OPENAI_API_KEY: "${OPENAI_API_KEY}"
```

#### Custom Python Wrapper

```yaml
chat_backend:
  command: "python"
  args: ["~/.forge/backends/custom-llm-wrapper.py"]
  model: "local-model"
```

```python
# ~/.forge/backends/custom-llm-wrapper.py
import json
import sys
from anthropic import Anthropic

def main():
    # Read input from stdin
    input_data = json.load(sys.stdin)

    message = input_data["message"]
    tools = input_data["tools"]
    context = input_data.get("context", {})

    # Call LLM with tools
    client = Anthropic()
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

    # Return result
    result = {
        "tool_calls": tool_calls,
        "message": "Executing tools..."
    }

    print(json.dumps(result))

if __name__ == "__main__":
    main()
```

### Tool Definitions

FORGE provides tool definitions in `~/.forge/tools.json`:

```json
{
  "tools": [
    {
      "name": "switch_view",
      "description": "Switch to a different dashboard view",
      "parameters": {
        "type": "object",
        "properties": {
          "view": {
            "type": "string",
            "enum": ["workers", "tasks", "costs", "metrics", "logs", "overview"],
            "description": "The view to switch to"
          }
        },
        "required": ["view"]
      }
    },
    {
      "name": "spawn_worker",
      "description": "Spawn new AI coding workers",
      "parameters": {
        "type": "object",
        "properties": {
          "model": {
            "type": "string",
            "description": "Model type (sonnet, opus, haiku, gpt4, etc.)"
          },
          "count": {
            "type": "integer",
            "minimum": 1,
            "maximum": 10,
            "description": "Number of workers to spawn"
          },
          "workspace": {
            "type": "string",
            "description": "Workspace path (optional, defaults to current)"
          }
        },
        "required": ["model", "count"]
      }
    }
  ]
}
```

### Testing Your Backend

```bash
# Test backend manually
echo '{"message": "show workers", "tools": [...]}' | \
  your-backend-command --args

# Expected output:
# {"tool_calls": [{"tool": "switch_view", "arguments": {"view": "workers"}}]}

# Test with FORGE
forge test-backend --backend-config=~/.forge/config.yaml
```

---

## Worker Launchers

### Purpose

Spawn AI coding workers (Claude Code, OpenCode, Aider, custom tools) in various configurations.

### Requirements

Your launcher script must:
1. Be executable (`chmod +x`)
2. Accept standard arguments
3. Return worker metadata on stdout (JSON)
4. Create status file at `~/.forge/status/<worker-id>.json`
5. Log to `~/.forge/logs/<worker-id>.log`

### Launcher Protocol

#### Input (Command Line Arguments)

```bash
~/.forge/launchers/my-launcher \
  --model=sonnet \
  --workspace=/path/to/project \
  --session-name=sonnet-alpha \
  --config=/path/to/worker-config.yaml
```

#### Output (stdout)

```json
{
  "worker_id": "sonnet-alpha",
  "pid": 12345,
  "status": "spawned",
  "launcher": "my-launcher",
  "timestamp": "2026-02-07T10:30:00Z"
}
```

#### Status File

Create `~/.forge/status/<worker-id>.json`:

```json
{
  "worker_id": "sonnet-alpha",
  "status": "active",
  "model": "sonnet",
  "workspace": "/path/to/project",
  "pid": 12345,
  "started_at": "2026-02-07T10:30:00Z",
  "last_activity": "2026-02-07T10:35:00Z",
  "current_task": null,
  "tasks_completed": 0
}
```

#### Log File

Write logs to `~/.forge/logs/<worker-id>.log`:

```json
{"timestamp": "2026-02-07T10:30:00Z", "level": "info", "worker_id": "sonnet-alpha", "message": "Worker started"}
```

### Example Launcher (Claude Code + tmux)

```bash
#!/bin/bash
# ~/.forge/launchers/claude-code-launcher

set -e

# Parse arguments
MODEL=""
WORKSPACE=""
SESSION_NAME=""
CONFIG=""

while [[ $# -gt 0 ]]; do
  case $1 in
    --model=*)
      MODEL="${1#*=}"
      shift
      ;;
    --workspace=*)
      WORKSPACE="${1#*=}"
      shift
      ;;
    --session-name=*)
      SESSION_NAME="${1#*=}"
      shift
      ;;
    --config=*)
      CONFIG="${1#*=}"
      shift
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

# Validate required arguments
if [[ -z "$MODEL" ]] || [[ -z "$WORKSPACE" ]] || [[ -z "$SESSION_NAME" ]]; then
  echo "Missing required arguments" >&2
  exit 1
fi

# Create log directory
mkdir -p ~/.forge/logs ~/.forge/status

# Launch Claude Code in tmux
tmux new-session -d -s "$SESSION_NAME" \
  "cd $WORKSPACE && claude-code --model=$MODEL 2>&1 | tee ~/.forge/logs/$SESSION_NAME.log"

# Get PID (approximation - tmux session)
PID=$(tmux list-panes -t "$SESSION_NAME" -F '#{pane_pid}')

# Output worker metadata (JSON)
cat << EOF
{
  "worker_id": "$SESSION_NAME",
  "pid": $PID,
  "status": "spawned",
  "launcher": "claude-code-launcher",
  "timestamp": "$(date -Iseconds)"
}
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

# Write initial log entry
echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker started\"}" \
  >> ~/.forge/logs/$SESSION_NAME.log
```

### Example Launcher (OpenCode + subprocess)

```python
#!/usr/bin/env python3
# ~/.forge/launchers/opencode-launcher

import json
import subprocess
import sys
import os
from datetime import datetime
from pathlib import Path

def main():
    # Parse arguments
    args = dict(arg.split('=', 1) for arg in sys.argv[1:] if '=' in arg)

    model = args.get('--model')
    workspace = args.get('--workspace')
    session_name = args.get('--session-name')

    if not all([model, workspace, session_name]):
        print("Missing required arguments", file=sys.stderr)
        sys.exit(1)

    # Create directories
    Path("~/.forge/logs").expanduser().mkdir(parents=True, exist_ok=True)
    Path("~/.forge/status").expanduser().mkdir(parents=True, exist_ok=True)

    log_file = Path(f"~/.forge/logs/{session_name}.log").expanduser()
    status_file = Path(f"~/.forge/status/{session_name}.json").expanduser()

    # Launch OpenCode as subprocess
    process = subprocess.Popen(
        ["opencode", "--model", model, "--workspace", workspace],
        stdout=open(log_file, 'a'),
        stderr=subprocess.STDOUT,
        cwd=workspace
    )

    # Output worker metadata
    metadata = {
        "worker_id": session_name,
        "pid": process.pid,
        "status": "spawned",
        "launcher": "opencode-launcher",
        "timestamp": datetime.now().isoformat()
    }
    print(json.dumps(metadata))

    # Create status file
    status = {
        "worker_id": session_name,
        "status": "active",
        "model": model,
        "workspace": workspace,
        "pid": process.pid,
        "started_at": datetime.now().isoformat(),
        "last_activity": datetime.now().isoformat(),
        "current_task": None,
        "tasks_completed": 0
    }
    with open(status_file, 'w') as f:
        json.dump(status, f, indent=2)

if __name__ == "__main__":
    main()
```

### Configuration

`~/.forge/config.yaml`:

```yaml
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

  custom:
    executable: "~/.forge/launchers/custom-launcher"
    models: ["custom-model"]
```

### Testing Your Launcher

```bash
# Test launcher manually
~/.forge/launchers/my-launcher \
  --model=sonnet \
  --workspace=/tmp/test \
  --session-name=test-worker

# Expected output (JSON):
# {"worker_id": "test-worker", "pid": 12345, "status": "spawned"}

# Check status file created:
cat ~/.forge/status/test-worker.json

# Check log file created:
tail ~/.forge/logs/test-worker.log

# Test with FORGE:
forge test-launcher --launcher=my-launcher --model=sonnet
```

---

## Worker Configurations

### Purpose

Define reusable worker configurations that can be shared across projects and users.

### Configuration Format

`~/.forge/workers/<worker-type>.yaml`:

```yaml
# Worker metadata
name: "claude-code-sonnet"
description: "Claude Code with Sonnet 4.5 model"
version: "1.0.0"
author: "your-name"

# Launcher configuration
launcher: "claude-code"  # References launchers in config.yaml
model: "sonnet"

# Cost/tier information (for routing)
tier: "standard"  # premium, standard, budget, free
cost_per_million_tokens:
  input: 3.0
  output: 15.0

# Subscription information
subscription:
  enabled: true
  monthly_cost: 20
  quota_type: "unlimited"  # or "tokens", "requests"
  quota_limit: null  # null for unlimited

# Environment variables
environment:
  ANTHROPIC_API_KEY: "${ANTHROPIC_API_KEY}"
  CLAUDE_CONFIG_DIR: "~/.config/claude-code"
  CLAUDE_MODEL: "sonnet"

# Spawn arguments (passed to launcher)
spawn_args:
  - "--tmux"
  - "--session=${session_name}"
  - "--workspace=${workspace}"
  - "--model=${model}"

# File paths
log_path: "~/.forge/logs/${worker_id}.log"
status_path: "~/.forge/status/${worker_id}.json"

# Health check (optional)
health_check:
  enabled: true
  interval_seconds: 60
  timeout_seconds: 10
  command: "tmux has-session -t ${session_name}"

# Capabilities (for task routing)
capabilities:
  - "code_generation"
  - "code_review"
  - "debugging"
  - "refactoring"
max_context_tokens: 200000
supports_tools: true
supports_vision: false
```

### Shareable Worker Repos

Users can reference GitHub repos containing worker configs:

`~/.forge/config.yaml`:

```yaml
worker_repos:
  - url: "https://github.com/forge-community/worker-configs"
    branch: "main"
    path: "configs/"

  - url: "https://github.com/jedarden/custom-workers"
    branch: "main"
    path: "workers/"
```

FORGE will clone/pull these repos and load worker configs from them.

### Creating a Shareable Worker Config

```bash
# 1. Create a repo
mkdir worker-configs && cd worker-configs
git init

# 2. Create worker configs
mkdir configs
cat > configs/claude-sonnet.yaml << EOF
name: "claude-code-sonnet"
description: "Claude Code with Sonnet 4.5"
launcher: "claude-code"
model: "sonnet"
tier: "standard"
...
EOF

# 3. Add README
cat > README.md << EOF
# FORGE Worker Configurations

Community-maintained worker configs for FORGE.

## Available Workers
- claude-code-sonnet - Claude Code with Sonnet 4.5
- opencode-gpt4 - OpenCode with GPT-4
...

## Usage
Add to ~/.forge/config.yaml:
\`\`\`yaml
worker_repos:
  - url: "https://github.com/your-org/worker-configs"
\`\`\`
EOF

# 4. Push to GitHub
git add .
git commit -m "Initial worker configs"
git remote add origin https://github.com/your-org/worker-configs.git
git push -u origin main
```

### Testing Worker Configs

```bash
# Validate config syntax
forge validate-worker-config ~/.forge/workers/my-worker.yaml

# Test spawning with config
forge test-spawn --worker-config=~/.forge/workers/my-worker.yaml --workspace=/tmp/test

# Dry-run (show what would be executed)
forge test-spawn --worker-config=my-worker --dry-run
```

---

## Log Collection

### Purpose

Collect structured logs from workers for metrics, debugging, and display in the TUI.

### Log Format (JSON Lines - Recommended)

`~/.forge/logs/<worker-id>.log`:

```json
{"timestamp": "2026-02-07T10:30:00Z", "level": "info", "worker_id": "sonnet-alpha", "message": "Worker started"}
{"timestamp": "2026-02-07T10:30:05Z", "level": "info", "worker_id": "sonnet-alpha", "event": "task_started", "task_id": "bd-abc"}
{"timestamp": "2026-02-07T10:35:00Z", "level": "info", "worker_id": "sonnet-alpha", "event": "task_completed", "task_id": "bd-abc", "duration_seconds": 295, "tokens": {"input": 1000, "output": 500}}
{"timestamp": "2026-02-07T10:35:01Z", "level": "error", "worker_id": "sonnet-alpha", "event": "task_failed", "task_id": "bd-def", "error": "API rate limit exceeded"}
```

### Standard Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `timestamp` | ISO 8601 | Yes | When event occurred |
| `level` | string | Yes | Log level: `info`, `warning`, `error`, `debug` |
| `worker_id` | string | Yes | Worker identifier |
| `message` | string | No | Human-readable message |
| `event` | string | No | Event type (see below) |
| `task_id` | string | No | Associated task/bead ID |

### Event Types

| Event | Description | Additional Fields |
|-------|-------------|-------------------|
| `worker_started` | Worker initialized | `model`, `workspace` |
| `worker_stopped` | Worker terminated | `reason` |
| `task_started` | Task execution began | `task_id` |
| `task_completed` | Task finished successfully | `task_id`, `duration_seconds`, `tokens` |
| `task_failed` | Task failed | `task_id`, `error`, `error_code` |
| `api_call` | External API called | `api`, `endpoint`, `status_code`, `latency_ms` |
| `cost_incurred` | Cost tracked | `amount`, `currency`, `tokens` |

### Alternative Format (Key-Value)

```
2026-02-07T10:30:00Z level=info worker_id=sonnet-alpha message="Worker started"
2026-02-07T10:30:05Z level=info worker_id=sonnet-alpha event=task_started task_id=bd-abc
2026-02-07T10:35:00Z level=info worker_id=sonnet-alpha event=task_completed task_id=bd-abc duration=295 tokens_in=1000 tokens_out=500
```

### Configuration

`~/.forge/config.yaml`:

```yaml
log_collection:
  # Where to find logs
  paths:
    - "~/.forge/logs/*.log"
    - "/var/log/workers/*.log"

  # Log format parser
  format: "jsonl"  # jsonl, keyvalue, auto-detect

  # Polling interval
  poll_interval_seconds: 1

  # Retention
  max_age_days: 30
  max_size_mb: 1000
```

### Writing Logs from Your Worker

#### Python Example

```python
import json
import logging
from datetime import datetime

class ForgeLogHandler(logging.Handler):
    def __init__(self, worker_id, log_path):
        super().__init__()
        self.worker_id = worker_id
        self.log_file = open(log_path, 'a')

    def emit(self, record):
        log_entry = {
            "timestamp": datetime.utcnow().isoformat() + "Z",
            "level": record.levelname.lower(),
            "worker_id": self.worker_id,
            "message": record.getMessage()
        }

        # Add extra fields if present
        if hasattr(record, 'event'):
            log_entry['event'] = record.event
        if hasattr(record, 'task_id'):
            log_entry['task_id'] = record.task_id

        self.log_file.write(json.dumps(log_entry) + '\n')
        self.log_file.flush()

# Usage
logger = logging.getLogger()
logger.addHandler(ForgeLogHandler("sonnet-alpha", "~/.forge/logs/sonnet-alpha.log"))

logger.info("Worker started")
logger.info("Task started", extra={"event": "task_started", "task_id": "bd-abc"})
```

#### Bash Example

```bash
#!/bin/bash
WORKER_ID="sonnet-alpha"
LOG_FILE="~/.forge/logs/$WORKER_ID.log"

log_event() {
  local level="$1"
  local message="$2"
  local event="$3"
  local extra="$4"

  local timestamp=$(date -Iseconds)
  local log_entry="{\"timestamp\": \"$timestamp\", \"level\": \"$level\", \"worker_id\": \"$WORKER_ID\", \"message\": \"$message\""

  if [[ -n "$event" ]]; then
    log_entry="$log_entry, \"event\": \"$event\""
  fi

  if [[ -n "$extra" ]]; then
    log_entry="$log_entry, $extra"
  fi

  log_entry="$log_entry}"
  echo "$log_entry" >> "$LOG_FILE"
}

# Usage
log_event "info" "Worker started" "worker_started" "\"model\": \"sonnet\""
log_event "info" "Task started" "task_started" "\"task_id\": \"bd-abc\""
```

### Testing Log Collection

```bash
# Generate test logs
echo '{"timestamp": "2026-02-07T10:30:00Z", "level": "info", "worker_id": "test", "message": "Test"}' \
  >> ~/.forge/logs/test.log

# Test FORGE log parser
forge test-logs --log-file=~/.forge/logs/test.log

# Validate log format
forge validate-logs --log-file=~/.forge/logs/test.log
```

---

## File System Layout

### Standard Directory Structure

```
~/.forge/
├── config.yaml              # Main configuration
├── tools.json               # Tool definitions for chat backend
├── launchers/               # Worker launcher scripts
│   ├── claude-code-launcher
│   ├── opencode-launcher
│   ├── aider-launcher
│   └── custom-launcher
├── workers/                 # Worker configuration templates
│   ├── claude-code-sonnet.yaml
│   ├── claude-code-opus.yaml
│   ├── opencode-gpt4.yaml
│   ├── aider-default.yaml
│   └── custom-worker.yaml
├── logs/                    # Worker logs (FORGE reads)
│   ├── sonnet-alpha.log
│   ├── opus-beta.log
│   ├── haiku-gamma.log
│   └── archive/             # Rotated logs
├── status/                  # Worker status files (FORGE reads)
│   ├── sonnet-alpha.json
│   ├── opus-beta.json
│   └── haiku-gamma.json
├── layouts/                 # Saved dashboard layouts
│   ├── default.yaml
│   ├── monitoring.yaml
│   └── custom.yaml
├── repos/                   # Cloned worker config repos
│   └── forge-community-worker-configs/
└── cache/                   # Temporary cache
    └── tool-call-history.json
```

### Permissions

```bash
# Set correct permissions
chmod 755 ~/.forge
chmod 755 ~/.forge/launchers
chmod +x ~/.forge/launchers/*
chmod 644 ~/.forge/config.yaml
chmod 600 ~/.forge/config.yaml  # If contains secrets
chmod 755 ~/.forge/logs
chmod 644 ~/.forge/logs/*.log
chmod 755 ~/.forge/status
chmod 644 ~/.forge/status/*.json
```

### Configuration Locations

FORGE checks for configuration in this order:
1. `$FORGE_CONFIG` environment variable
2. `./forge.yaml` (current directory)
3. `~/.forge/config.yaml` (user home)
4. `/etc/forge/config.yaml` (system-wide)

### Environment Variables

```bash
# Override config location
export FORGE_CONFIG=~/.config/forge/config.yaml

# Override launcher directory
export FORGE_LAUNCHERS_DIR=~/custom-launchers

# Override worker configs directory
export FORGE_WORKERS_DIR=~/custom-workers

# Override log directory
export FORGE_LOGS_DIR=/var/log/forge

# Enable debug logging
export FORGE_DEBUG=1
```

---

## Complete Examples

### Example 1: Claude Code Integration

**Directory structure**:
```
~/.forge/
├── config.yaml
├── tools.json
├── launchers/
│   └── claude-code-launcher
└── workers/
    └── claude-code-sonnet.yaml
```

**config.yaml**:
```yaml
chat_backend:
  command: "claude-code"
  args: ["chat", "--headless", "--tools=~/.forge/tools.json"]
  model: "sonnet"
  env:
    ANTHROPIC_API_KEY: "${ANTHROPIC_API_KEY}"

launchers:
  claude-code:
    executable: "~/.forge/launchers/claude-code-launcher"
    models: ["sonnet", "opus", "haiku"]

log_collection:
  paths: ["~/.forge/logs/*.log"]
  format: "jsonl"
```

**claude-code-launcher**:
```bash
#!/bin/bash
MODEL="${1}"
WORKSPACE="${2}"
SESSION_NAME="${3}"

tmux new-session -d -s "$SESSION_NAME" \
  "cd $WORKSPACE && claude-code --model=$MODEL 2>&1 | tee ~/.forge/logs/$SESSION_NAME.log"

echo "{\"worker_id\": \"$SESSION_NAME\", \"pid\": $$, \"status\": \"spawned\"}"
```

**claude-code-sonnet.yaml**:
```yaml
name: "claude-code-sonnet"
launcher: "claude-code"
model: "sonnet"
tier: "standard"
cost_per_million_tokens:
  input: 3.0
  output: 15.0
subscription:
  enabled: true
  monthly_cost: 20
```

### Example 2: OpenCode + Docker Integration

**Launcher** (`~/.forge/launchers/opencode-docker-launcher`):
```bash
#!/bin/bash
MODEL="${1}"
WORKSPACE="${2}"
SESSION_NAME="${3}"

# Launch OpenCode in Docker container
CONTAINER_ID=$(docker run -d \
  --name "$SESSION_NAME" \
  -v "$WORKSPACE:/workspace" \
  -e OPENAI_API_KEY="$OPENAI_API_KEY" \
  opencode:latest \
  --model="$MODEL" \
  --workspace=/workspace)

# Stream logs to FORGE
docker logs -f "$SESSION_NAME" >> ~/.forge/logs/$SESSION_NAME.log &

echo "{\"worker_id\": \"$SESSION_NAME\", \"pid\": $CONTAINER_ID, \"status\": \"spawned\"}"
```

### Example 3: Custom Python Worker

**Launcher** (`~/.forge/launchers/custom-python-launcher`):
```python
#!/usr/bin/env python3
import json
import subprocess
import sys

model = sys.argv[1]
workspace = sys.argv[2]
session_name = sys.argv[3]

# Launch custom worker script
process = subprocess.Popen(
    ["python", "~/custom-worker.py", "--model", model],
    cwd=workspace,
    stdout=open(f"~/.forge/logs/{session_name}.log", 'a'),
    stderr=subprocess.STDOUT
)

print(json.dumps({
    "worker_id": session_name,
    "pid": process.pid,
    "status": "spawned"
}))
```

---

## Troubleshooting

### Chat Backend Not Responding

```bash
# Test backend manually
echo '{"message": "test", "tools": []}' | your-backend-command

# Check backend logs
forge debug-backend --verbose

# Verify tool definitions
forge validate-tools ~/.forge/tools.json
```

### Launcher Not Spawning Workers

```bash
# Test launcher manually
~/.forge/launchers/your-launcher --model=test --workspace=/tmp --session-name=test

# Check launcher permissions
ls -la ~/.forge/launchers/

# Enable launcher debug logging
FORGE_DEBUG=1 forge spawn --model=test
```

### Logs Not Appearing

```bash
# Verify log file exists
ls -la ~/.forge/logs/

# Test log parser
forge test-logs --log-file=~/.forge/logs/your-worker.log

# Check log format
head ~/.forge/logs/your-worker.log

# Validate JSON format (if using JSONL)
cat ~/.forge/logs/your-worker.log | jq .
```

### Worker Status Not Updating

```bash
# Check status file
cat ~/.forge/status/your-worker.json

# Verify status file permissions
ls -la ~/.forge/status/

# Test status file parsing
forge test-status --status-file=~/.forge/status/your-worker.json
```

---

## Next Steps

1. **Set up your first integration** - Start with chat backend
2. **Create a launcher** - Get workers spawning
3. **Configure logging** - See worker activity
4. **Share your configs** - Contribute to community

**Need help?** Open an issue at https://github.com/jedarden/forge/issues

---

**FORGE** - Federated Orchestration & Resource Generation Engine

A dumb orchestrator that gets smart by integrating with your tools.
