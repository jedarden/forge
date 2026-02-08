# FORGE Developer Guide

Complete guide for contributing to FORGE - architecture, adding tools, creating launchers, and testing.

---

## Table of Contents

- [Architecture Overview](#architecture-overview)
- [Project Structure](#project-structure)
- [Development Setup](#development-setup)
- [Adding New Tools](#adding-new-tools)
- [Creating Custom Launchers](#creating-custom-launchers)
- [Testing Strategy](#testing-strategy)
- [Code Style Guidelines](#code-style-guidelines)
- [ADR Summaries](#adr-summaries)
- [Contributing](#contributing)

---

## Architecture Overview

FORGE is a **"dumb orchestrator"** - it doesn't contain AI logic or worker management itself. Instead, it integrates with external components through well-defined interfaces.

### Core Philosophy

> **FORGE displays information, triggers launchers, parses logs, and delegates chat to external LLMs.**

### System Architecture

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

### Key Design Principles

1. **Dumb Orchestrator** - FORGE has no built-in AI or worker management
2. **Integration-First** - All external components integrate via protocols
3. **Visibility Over Automation** - Errors are shown, not hidden
4. **Graceful Degradation** - Components fail independently, not the whole app
5. **Configuration-Driven** - All behavior controlled via YAML config

### Integration Surfaces

| Surface | Purpose | Protocol | Config Location |
|---------|---------|----------|-----------------|
| **Chat Backend** | Conversational interface | stdin/stdout JSON | `config.yaml` |
| **Worker Launchers** | Spawn AI coding agents | CLI + JSON output | `~/.forge/launchers/` |
| **Worker Configs** | Reusable worker templates | YAML schema | `~/.forge/workers/` |
| **Log Collection** | Worker activity monitoring | JSON/key-value | `~/.forge/logs/` |
| **Status Files** | Worker state tracking | JSON | `~/.forge/status/` |

---

## Project Structure

```
forge/
├── src/forge/              # Main package
│   ├── __init__.py        # Package init
│   ├── app.py             # TUI application (Textual)
│   ├── cli.py             # CLI entry point (Click)
│   ├── config.py          # Configuration loading
│   ├── tools.py           # Tool definitions (45+ tools)
│   ├── tool_definitions.py    # Tool catalog & executor
│   ├── tool_execution.py  # Tool execution engine
│   ├── launcher.py        # Worker launcher integration
│   ├── chat_backend.py    # Chat backend integration
│   ├── log_parser.py      # Log parsing (JSON/key-value)
│   ├── log_watcher.py     # File watcher for logs
│   ├── status_watcher.py  # File watcher for status
│   ├── beads.py           # Beads integration
│   ├── cost_tracker.py    # Cost tracking & forecasting
│   ├── metrics.py         # Performance metrics
│   ├── health_monitor.py  # Worker health monitoring
│   ├── error_display.py   # Error display patterns
│   └── confirmation_dialog.py  # User confirmation UI
│
├── test/                  # Test harnesses
│   ├── README.md          # Testing documentation
│   ├── launcher-test-harness.py     # Launcher protocol tests
│   ├── backend-test-harness.py      # Backend protocol tests
│   ├── worker-config-validator.py    # Config validation
│   ├── log-format-validator.py       # Log format validation
│   ├── status-file-validator.py      # Status file validation
│   └── example-launchers/
│       ├── README.md
│       └── passing-launcher.sh  # Reference implementation
│
├── tests/                 # pytest unit tests
│   └── __init__.py
│
├── docs/                  # Documentation
│   ├── adr/              # Architecture Decision Records
│   │   ├── README.md
│   │   ├── 0001-use-forge-as-project-name.md
│   │   ├── 0002-use-tui-for-control-panel-interface.md
│   │   ├── 0003-cost-optimization-strategy.md
│   │   ├── 0004-tool-based-conversational-interface.md
│   │   ├── 0005-dumb-orchestrator-architecture.md
│   │   ├── 0006-technology-stack-selection.md
│   │   ├── 0007-bead-integration-strategy.md
│   │   ├── 0008-real-time-update-architecture.md
│   │   ├── 0010-security-and-credential-management.md
│   │   └── 0014-error-handling-strategy.md
│   ├── notes/            # Research & design notes
│   ├── USER_GUIDE.md     # End-user documentation
│   ├── INTEGRATION_GUIDE.md  # Integration guide
│   ├── TOOL_CATALOG.md   # Complete tool reference
│   ├── HOTKEYS.md        # Keyboard shortcuts
│   └── DEVELOPER_GUIDE.md  # This file
│
├── example-workers/       # Example worker configurations
│   └── README.md
│
├── pyproject.toml        # Project config (hatchling)
├── README.md             # Project README
└── LICENSE               # MIT License
```

### Core Components

#### `app.py` - Main TUI Application
- Built with [Textual](https://textual.textual.io/)
- 6-panel dashboard layout
- Real-time updates via file watchers
- Chat interface integration
- Error display and dialogs

#### `tools.py` & `tool_definitions.py` - Tool System
- 45+ tool definitions across 11 categories
- OpenAI function-calling compatible format
- Tool executor with validation and rate limiting
- Tool export for LLM consumption

#### `launcher.py` - Worker Launcher Integration
- Spawns launcher subprocesses
- Validates protocol compliance
- Handles launcher errors gracefully
- Protocol validation and testing

#### `chat_backend.py` - Chat Backend Integration
- Manages headless CLI process
- Sends/receives JSON messages
- Handles backend failures
- Degrades to hotkey-only mode on error

#### `log_parser.py` - Log Parsing
- Detects JSON vs key-value format
- Parses worker activity logs
- Extracts metrics and events
- Skips malformed entries gracefully

#### `config.py` - Configuration Management
- Loads YAML config from multiple sources
- Merges user + workspace overrides
- Hot-reload support for runtime changes
- Validation and error handling

---

## Development Setup

### Prerequisites

- Python 3.10 or higher
- Git
- tmux (for worker testing)
- Virtual environment (recommended)

### Setup Steps

```bash
# Clone the repository
git clone https://github.com/jedarden/forge.git
cd forge

# Create virtual environment
python -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# Install in editable mode with dev dependencies
pip install -e ".[dev]"

# Run tests to verify setup
pytest

# Run type checking
mypy src/

# Run linting
ruff check src/
ruff format --check src/
```

### Development Workflow

```bash
# Make changes to code

# Run linting and formatting
ruff check src/
ruff format src/

# Run type checker
mypy src/

# Run tests
pytest

# Make changes pass
pytest

# Format and lint
ruff format src/
ruff check src/
```

### Running FORGE in Development

```bash
# Initialize config if needed
forge init

# Validate configuration
forge validate

# Launch dashboard
forge dashboard

# Launch with custom config
forge dashboard --config ./test-config.yaml --workspace ./test-workspace
```

---

## Adding New Tools

Tools are the primary way users interact with FORGE through the conversational interface. Tools are defined in `src/forge/tool_definitions.py` and executed via `src/forge/tools.py`.

### Tool Categories

FORGE has 11 tool categories:
1. **View Control** - Switch views, layouts, panels
2. **Worker Management** - Spawn, kill, list workers
3. **Task Management** - Filter, create, assign tasks
4. **Cost Analytics** - Show costs, optimize routing, forecast
5. **Data Export** - Export logs, metrics, screenshots
6. **Configuration** - Get/set config, save/load layouts
7. **Help & Discovery** - Help, search docs, list capabilities
8. **Notification** - Show notifications, warnings, prompts
9. **System** - Status, refresh, ping workers
10. **Workspace** - Switch, list, create workspaces
11. **Analytics** - Throughput, latency, trends, bottlenecks

### Step 1: Define the Tool

Add your tool to `src/forge/tool_definitions.py`:

```python
from forge.tool_definitions import (
    ToolCategory,
    ToolDefinition,
    ToolParameter,
)

# Add to appropriate category list
ANALYTICS_TOOLS = [
    # ... existing tools ...

    ToolDefinition(
        name="show_throughput_trends",
        description="Display throughput trends over time with optional filtering.",
        category=ToolCategory.ANALYTICS,
        parameters=[
            ToolParameter(
                name="period",
                type="string",
                description="Time period for trend analysis",
                required=False,
                enum=["today", "this_week", "this_month", "last_week", "last_month"],
                default="this_week"
            ),
            ToolParameter(
                name="by_model",
                type="boolean",
                description="Group trends by model type",
                required=False,
                default=True
            ),
        ],
        rate_limit=5  # Max 5 calls per minute
    ),
]
```

### Step 2: Implement Tool Execution

Register a callback in `src/forge/app.py`:

```python
from forge.tool_definitions import ToolExecutor, ToolCallResult

class ForgeApp(App):
    def __init__(self):
        super().__init__()

        # Initialize tool executor
        self.tool_executor = ToolExecutor(register_all=True)

        # Register your tool callback
        self.tool_executor.register_tool(
            ToolDefinition(...),  # Your tool definition
            callback=self._show_throughput_trends
        )

    def _show_throughput_trends(
        self,
        period: str = "this_week",
        by_model: bool = True
    ) -> ToolCallResult:
        """Execute show_throughput_trends tool"""

        # Implement your logic here
        # Access data from self.metrics, self.costs, etc.

        try:
            # Calculate trends
            trends = self.metrics.calculate_throughput_trends(
                period=period,
                by_model=by_model
            )

            # Update UI to show trends
            self.switch_to_trends_view(trends)

            return ToolCallResult(
                success=True,
                tool_name="show_throughput_trends",
                message=f"Showing throughput trends for {period}",
                data={"trends": trends}
            )

        except Exception as e:
            return ToolCallResult(
                success=False,
                tool_name="show_throughput_trends",
                message=f"Failed to show trends: {e}",
                error=str(e)
            )
```

### Step 3: Update Tool Catalog

Run the tool catalog generator:

```bash
# Export tools to tools.json
python -c "
from forge.tool_definitions import generate_tools_file
generate_tools_file('~/.forge/tools.json', format='openai')
"
```

Or use the CLI (when implemented):

```bash
forge generate-tools
```

### Step 4: Test Your Tool

```bash
# Test tool definition loading
python -c "
from forge.tool_definitions import ToolExecutor
executor = ToolExecutor(register_all=True)
tool = executor.get_tool('show_throughput_trends')
print(tool.to_openai_format())
"

# Test tool execution
python -c "
from forge.tool_definitions import ToolExecutor
executor = ToolExecutor()
result = executor.execute('show_throughput_trends', {'period': 'today'})
print(result.to_dict())
"
```

### Tool Best Practices

1. **Descriptive Names** - Use clear, action-oriented names
2. **Rich Descriptions** - Help LLM understand when to use the tool
3. **Smart Defaults** - Provide sensible defaults for optional parameters
4. **Validation** - Use enum constraints for known values
5. **Rate Limiting** - Set rate limits for expensive operations
6. **Error Messages** - Return actionable error messages
7. **Confirmation** - Require confirmation for destructive actions

### Example: Complete Tool Implementation

```python
# 1. Define the tool
ToolDefinition(
    name="kill_idle_workers",
    description="Terminate all workers that have been idle for longer than a specified duration.",
    category=ToolCategory.WORKER_MANAGEMENT,
    parameters=[
        ToolParameter(
            name="idle_minutes",
            type="integer",
            description="Minimum idle time in minutes",
            required=False,
            minimum=1,
            maximum=1440,  # 24 hours
            default=120  # 2 hours
        ),
    ],
    requires_confirmation=True,
    confirmation_message="Kill {count} workers idle for >{idle_minutes} minutes?",
    rate_limit=2
)

# 2. Implement the callback
def _kill_idle_workers(
    self,
    idle_minutes: int = 120
) -> ToolCallResult:
    """Kill workers idle longer than specified"""

    # Find idle workers
    idle_workers = [
        w for w in self.workers.values()
        if w.idle_time_minutes >= idle_minutes
    ]

    if not idle_workers:
        return ToolCallResult(
            success=True,
            tool_name="kill_idle_workers",
            message=f"No workers idle for >{idle_minutes} minutes",
            data={"killed": 0}
        )

    # Kill each worker
    killed = []
    for worker in idle_workers:
        try:
            self.launcher.kill_worker(worker.worker_id)
            killed.append(worker.worker_id)
        except Exception as e:
            self.log.warning(f"Failed to kill {worker.worker_id}: {e}")

    return ToolCallResult(
        success=True,
        tool_name="kill_idle_workers",
        message=f"Killed {len(killed)} workers idle for >{idle_minutes} minutes",
        data={
            "killed": len(killed),
            "worker_ids": killed
        }
    )

# 3. Register in __init__
self.tool_executor.register_tool(
    ToolDefinition(...),
    callback=self._kill_idle_workers
)
```

---

## Creating Custom Launchers

Launchers are scripts that spawn AI coding workers in tmux sessions, subprocesses, or Docker containers.

### Launcher Protocol

Launchers must:

1. **Accept standard arguments**: `--model`, `--workspace`, `--session-name`
2. **Return JSON on stdout**: With `worker_id`, `pid`, `status` fields
3. **Create status file**: At `~/.forge/status/<session-name>.json`
4. **Create log file**: At `~/.forge/logs/<session-name>.log`
5. **Exit with code 0**: On success

### Example Launcher (Bash + tmux)

```bash
#!/bin/bash
# ~/.forge/launchers/claude-code-launcher

# Parse arguments
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
```

### Example Launcher (Python + subprocess)

```python
#!/usr/bin/env python3
# ~/.forge/launchers/opencode-launcher

import json
import os
import subprocess
import sys
from datetime import datetime
from pathlib import Path

def main():
    # Parse arguments
    model = sys.argv[1]
    workspace = sys.argv[2]
    session_name = sys.argv[3]

    # Create directories
    forge_dir = Path.home() / ".forge"
    logs_dir = forge_dir / "logs"
    status_dir = forge_dir / "status"
    logs_dir.mkdir(parents=True, exist_ok=True)
    status_dir.mkdir(parents=True, exist_ok=True)

    # Log file
    log_file = logs_dir / f"{session_name}.log"

    # Launch OpenCode as subprocess
    process = subprocess.Popen(
        ["opencode", "--model", model, "--workspace", workspace],
        stdout=log_file.open("w"),
        stderr=subprocess.STDOUT,
        cwd=workspace
    )

    # Create status file
    status_file = status_dir / f"{session_name}.json"
    status_file.write_text(json.dumps({
        "worker_id": session_name,
        "status": "active",
        "model": model,
        "workspace": workspace,
        "pid": process.pid,
        "started_at": datetime.now().isoformat()
    }, indent=2))

    # Output metadata
    print(json.dumps({
        "worker_id": session_name,
        "pid": process.pid,
        "status": "spawned"
    }))

    sys.exit(0)

if __name__ == "__main__":
    main()
```

### Example Launcher (Docker)

```bash
#!/bin/bash
# ~/.forge/launchers/docker-launcher

MODEL="$1"
WORKSPACE="$2"
SESSION_NAME="$3"

# Create directories
mkdir -p ~/.forge/logs ~/.forge/status

# Run container in detached mode
CONTAINER_ID=$(docker run -d \
  --name "$SESSION_NAME" \
  -v "$WORKSPACE:/workspace" \
  -e MODEL="$MODEL" \
  my-ai-worker-image:latest)

# Get PID (container's main process PID)
PID=$(docker inspect --format='{{.State.Pid}}' "$CONTAINER_ID")

# Output metadata
echo "{\"worker_id\": \"$SESSION_NAME\", \"pid\": $PID, \"status\": \"spawned\"}"

# Create status file
cat > ~/.forge/status/$SESSION_NAME.json << EOF
{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "workspace": "$WORKSPACE",
  "container_id": "$CONTAINER_ID",
  "pid": $PID,
  "started_at": "$(date -Iseconds)"
}
EOF
```

### Testing Your Launcher

Use the launcher test harness:

```bash
# Test your launcher
./test/launcher-test-harness.py ~/.forge/launchers/my-launcher

# Test with the reference passing launcher
./test/launcher-test-harness.py ./test/example-launchers/passing-launcher.sh
```

Expected output:
```
============================================================
Testing launcher: ~/.forge/launchers/my-launcher
============================================================

Test 1: Argument Parsing... ✅ PASS
Test 2: Output Format... ✅ PASS
Test 3: Status File Creation... ✅ PASS
Test 4: Log File Creation... ✅ PASS
Test 5: Process Spawning... ✅ PASS

============================================================
Results: 5 passed, 0 failed
============================================================
```

### Register Your Launcher

Add to `~/.forge/config.yaml`:

```yaml
launchers:
  my-launcher:
    executable: "~/.forge/launchers/my-launcher"
    default_args:
      - "--detached"
    models: ["sonnet", "opus", "haiku"]
```

### Launcher Best Practices

1. **Error Handling** - Check for missing arguments, invalid paths
2. **Cleanup** - Kill processes/containers on failure
3. **Logging** - Write to log file even on startup errors
4. **Status Updates** - Update status file with current state
5. **Idempotency** - Handle re-runs gracefully
6. **Security** - Don't log credentials or secrets

---

## Testing Strategy

FORGE uses a multi-layered testing approach:

### 1. Unit Tests (pytest)

Test individual components in isolation:

```bash
# Run all unit tests
pytest

# Run specific test file
pytest tests/test_tools.py

# Run with coverage
pytest --cov=src/forge --cov-report=html

# Run with verbose output
pytest -v
```

Example test:

```python
# tests/test_tools.py
import pytest
from forge.tool_definitions import (
    ToolExecutor,
    ToolDefinition,
    ToolCategory,
    ToolParameter,
)

def test_tool_registration():
    """Test tool registration and retrieval"""
    executor = ToolExecutor()

    tool = ToolDefinition(
        name="test_tool",
        description="Test tool",
        category=ToolCategory.SYSTEM,
        parameters=[]
    )

    executor.register_tool(tool)

    retrieved = executor.get_tool("test_tool")
    assert retrieved is not None
    assert retrieved.name == "test_tool"

def test_tool_validation():
    """Test tool parameter validation"""
    executor = ToolExecutor()

    # Register tool with required parameter
    tool = ToolDefinition(
        name="spawn_worker",
        description="Spawn worker",
        category=ToolCategory.WORKER_MANAGEMENT,
        parameters=[
            ToolParameter(
                name="model",
                type="string",
                description="Model type",
                required=True,
                enum=["sonnet", "opus", "haiku"]
            )
        ]
    )
    executor.register_tool(tool, callback=lambda **_: None)

    # Valid call
    result = executor.execute("spawn_worker", {"model": "sonnet"})
    assert result.success

    # Missing required parameter
    result = executor.execute("spawn_worker", {})
    assert not result.success
    assert "Missing required parameter" in result.error

    # Invalid enum value
    result = executor.execute("spawn_worker", {"model": "invalid"})
    assert not result.success
    assert "Invalid value" in result.error
```

### 2. Integration Tests (Test Harnesses)

Test integration surfaces with external components:

```bash
# Test launcher protocol
./test/launcher-test-harness.py ~/.forge/launchers/claude-code-launcher

# Test backend protocol
./test/backend-test-harness.py claude-code chat --headless

# Validate worker config
./test/worker-config-validator.py ~/.forge/workers/claude-code-sonnet.yaml

# Validate log format
./test/log-format-validator.py ~/.forge/logs/sonnet-alpha.log

# Validate status file
./test/status-file-validator.py ~/.forge/status/sonnet-alpha.json
```

### 3. TUI Tests (pytest-textual)

Test the Textual UI:

```bash
# Run TUI tests
pytest --textual

# Run specific TUI test
pytest tests/test_app.py --textual
```

Example TUI test:

```python
# tests/test_app.py
from textual.app import App
from forge.app import ForgeApp

async def test_worker_panel():
    """Test worker panel display"""
    app = ForgeApp()

    async with app.run_test() as pilot:
        # Start the app
        await pilot.pause()

        # Check workers panel exists
        workers_panel = app.query_one("#workers-panel")
        assert workers_panel is not None

        # Switch to workers view
        await pilot.press("w")
        await pilot.pause()

        # Verify workers are displayed
        assert len(workers_panel.children) > 0
```

### 4. Error Injection Tests

Test error handling per ADR 0014:

```python
# tests/test_error_handling.py
import pytest
from forge.launcher import WorkerLauncher, LauncherConfig

def test_launcher_not_found():
    """Test graceful handling when launcher doesn't exist"""
    launcher = WorkerLauncher()
    config = LauncherConfig(
        launcher_path="/nonexistent/launcher",
        model="sonnet",
        workspace="/tmp",
        session_name="test"
    )

    result = launcher.spawn(config)

    assert not result.success
    assert result.error_type == "not_found"
    assert len(result.guidance) > 0

def test_backend_timeout():
    """Test backend timeout handling"""
    # Mock backend that times out
    ...

def test_malformed_log_parsing():
    """Test log parser skips bad entries"""
    from forge.log_parser import LogParser

    parser = LogParser()

    # Valid entry
    entry = parser.parse_entry('{"timestamp": "2026-02-07T10:00:00", "level": "info"}')
    assert entry is not None

    # Malformed entry
    entry = parser.parse_entry('invalid json {')
    assert entry is None
    assert parser.parse_errors == 1

    # Parser still works
    entry = parser.parse_entry('{"timestamp": "2026-02-07T10:00:01", "level": "info"}')
    assert entry is not None
```

### Test Coverage Goals

- **Unit tests**: 80%+ coverage of core logic
- **Integration tests**: All integration surfaces covered
- **Error paths**: All error states tested
- **TUI tests**: Major user flows covered

### CI Integration

Add to `.github/workflows/test.yml`:

```yaml
name: Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3

      - name: Set up Python
        uses: actions/setup-python@v4
        with:
          python-version: '3.11'

      - name: Install dependencies
        run: |
          pip install -e ".[dev]"

      - name: Run unit tests
        run: pytest --cov=src/forge

      - name: Run linting
        run: |
          ruff check src/
          ruff format --check src/

      - name: Run type checking
        run: mypy src/
```

---

## Code Style Guidelines

### Python Style (Ruff)

FORGE uses [Ruff](https://github.com/astral-sh/ruff) for linting and formatting:

```bash
# Check code
ruff check src/

# Format code
ruff format src/

# Check format without changes
ruff format --check src/
```

**Key Style Rules**:
- Line length: 100 characters
- Use f-strings for string formatting
- Use type hints for all functions
- Use `str | None` instead of `Optional[str]` (Python 3.10+)
- Use dataclasses for data containers
- Use enums for fixed sets of values

### Type Checking (MyPy)

FORGE uses strict type checking:

```bash
mypy src/
```

**Type Guidelines**:
- All functions must have type hints
- Return types must be explicit
- Use generic types (`list[str]`, `dict[str, int]`)
- Enable strict mode in `pyproject.toml`

### Documentation Style

```python
def spawn_worker(
    model: str,
    count: int,
    workspace: Path | str,
    session_name: str
) -> LauncherResult:
    """
    Spawn a worker using the configured launcher.

    Args:
        model: Model identifier (e.g., "sonnet", "opus")
        count: Number of workers to spawn
        workspace: Path to the workspace directory
        session_name: Unique session name for the worker

    Returns:
        LauncherResult with success status and error details if failed

    Raises:
        ValueError: If model is not supported
        LauncherError: If launcher fails to spawn worker

    Example:
        >>> result = spawn_worker("sonnet", 2, "/workspace", "sonnet-alpha")
        >>> if result.success:
        ...     print(f"Spawned: {result.worker_id}")
    """
```

### Commit Message Style

Follow conventional commits:

```
feat(tools): add show_throughput_trends tool

Add new tool for displaying throughput trends over time.
Supports filtering by period and model type.

Closes #123
```

```
fix(launcher): handle timeout gracefully

Launcher now returns proper error result on timeout
instead of raising exception. Adds guidance for users.

Fixes #456
```

---

## ADR Summaries

FORGE's architecture is documented in Architecture Decision Records (ADRs) in `docs/adr/`. Here are summaries of the key ADRs:

### ADR 0001: Use FORGE as Project Name

**Decision**: Name the project "FORGE" (Federated Orchestration & Resource Generation Engine)

**Rationale**:
- Reflects core mission: federated orchestration
- Memorable and pronounceable
- Evokes "forging" agents from resources

### ADR 0002: Use TUI for Control Panel Interface

**Decision**: Use terminal-based TUI (Textual) instead of web UI

**Rationale**:
- Native terminal experience for developers
- Lower resource usage
- No HTTP server complexity
- Works over SSH

### ADR 0003: Cost Optimization Strategy

**Decision**: Maximize subscription usage before falling back to pay-per-token

**Rationale**:
- Subscriptions are "use or lose"
- Save 87-94% on costs
- Intelligent routing based on task complexity

### ADR 0004: Tool-Based Conversational Interface

**Decision**: Use headless LLM backend with tool calling for chat interface

**Rationale**:
- Natural language interface
- No hotkey memorization
- LLM can chain actions intelligently
- Hotkeys remain as shortcuts

### ADR 0005: Dumb Orchestrator Architecture

**Decision**: FORGE has no built-in AI or worker management

**Rationale**:
- Decoupled from specific implementations
- Easy to extend with new worker types
- Community can share worker configs
- Simpler to maintain

### ADR 0006: Technology Stack Selection

**Decision**: Python + Textual + Click + PyYAML

**Rationale**:
- Python: Widely used in ML/AI community
- Textual: Modern, powerful TUI framework
- Click: De-facto standard for CLIs
- PyYAML: Standard YAML parsing

### ADR 0007: Bead Integration Strategy

**Decision**: Integrate with Beads (SQLite + JSONL task tracker)

**Rationale**:
- Agent-friendly task tracking
- Dependency management
- Git-friendly format
- Existing ecosystem

### ADR 0008: Real-Time Update Architecture

**Decision**: Use inotify with polling fallback

**Rationale**:
- Instant updates on supported systems
- Graceful fallback to polling
- Watch multiple file types

### ADR 0010: Security and Credential Management

**Decision**: No built-in credential management, use environment variables

**Rationale**:
- Don't reinvent the wheel
- User controls their secrets
- Works with existing tools (env vars, vaults)

### ADR 0014: Error Handling Strategy

**Decision**: Visibility over automation - no silent failures

**Rationale**:
- Developers need to see what's broken
- No automatic retry hides issues
- Clear error messages with actionable guidance
- Graceful degradation

---

## Contributing

### How to Contribute

1. **Fork** the repository
2. **Branch** for your work (`git checkout -b feature/my-feature`)
3. **Develop** following the guidelines above
4. **Test** your changes thoroughly
5. **Commit** with conventional commit messages
6. **Push** to your fork
7. **Open** a pull request

### Pull Request Guidelines

- **Description**: Clearly describe what you're changing and why
- **Tests**: Include tests for new functionality
- **Docs**: Update documentation as needed
- **ADR**: Consider an ADR for architectural changes
- **Breaking Changes**: Clearly highlight any breaking changes

### Code Review Criteria

- Does it follow the style guidelines?
- Are tests included and passing?
- Is documentation updated?
- Does it align with ADRs?
- Are error messages clear and actionable?

### Getting Help

- **Documentation**: Check `docs/` folder
- **ADRs**: Review architectural decisions
- **Issues**: Search or create GitHub issues
- **Discussions**: Use GitHub Discussions for questions

---

## Next Steps

1. **Read the codebase**: Start with `app.py`, `tools.py`, `launcher.py`
2. **Run FORGE locally**: `forge dashboard`
3. **Create a launcher**: Follow the launcher template
4. **Add a tool**: Implement a simple tool first
5. **Write tests**: Ensure your changes are tested
6. **Open a PR**: Share your contributions

---

**FORGE** - Federated Orchestration & Resource Generation Engine

Where AI agents are forged, orchestrated, and optimized.
