# Bead-Aware Launcher Protocol

## Overview

The Bead-Aware Launcher Protocol extends FORGE's launcher system to support automatic worker allocation to specific beads. This enables FORGE to spawn workers that are pre-configured to work on specific tasks from the bead queue.

## Protocol Specification

### Standard Arguments (Required)

All launchers MUST accept these standard arguments:

| Argument | Description | Example |
|----------|-------------|---------|
| `--model=<model>` | Model identifier (sonnet, opus, haiku) | `--model=sonnet` |
| `--workspace=<path>` | Path to the workspace directory | `--workspace=/home/coder/forge` |
| `--session-name=<name>` | Unique session name for the worker | `--session-name=forge-fg-1qo` |

### Optional Arguments

| Argument | Description | Example |
|----------|-------------|---------|
| `--config=<path>` | Path to worker configuration file | `--config=/path/to/config.yaml` |
| `--bead-ref=<bead-id>` | Bead ID to work on (bead-aware mode) | `--bead-ref=fg-1qo` |

### Output Format

Launchers MUST output JSON to stdout with the following fields:

#### Standard Output Fields

```json
{
  "worker_id": "forge-fg-1qo-sonnet",
  "pid": 12345,
  "status": "spawned",
  "model": "sonnet",
  "session": "forge-fg-1qo-sonnet",
  "timestamp": "2026-02-08T23:34:00Z"
}
```

#### Bead-Aware Output Fields (when --bead-ref provided)

```json
{
  "worker_id": "forge-fg-1qo-sonnet",
  "pid": 12345,
  "status": "spawned",
  "model": "sonnet",
  "session": "forge-fg-1qo-sonnet",
  "timestamp": "2026-02-08T23:34:00Z",
  "bead_id": "fg-1qo",
  "bead_title": "Design bead-aware launcher protocol"
}
```

### Error Output

On failure, launchers MUST output:

```json
{
  "pid": 0,
  "session": "",
  "status": "failed",
  "error": "Error message describing the failure"
}
```

## Bead-Aware Behavior

When `--bead-ref` is provided, the launcher MUST:

1. **Fetch bead data** using `br show <bead-id> --format json`
2. **Parse bead JSON** to extract:
   - `id`: Bead identifier
   - `title`: Bead title
   - `description`: Task description
   - `priority`: Priority level (0-4)
   - `issue_type`: Type of task
   - `labels`: Associated labels
3. **Update bead status** to `in_progress` on launch
4. **Construct prompt** with bead context
5. **Launch worker** with bead-specific prompt
6. **Update bead status** on completion:
   - Exit code 0 → Close bead (completed)
   - Exit code 1+ → Leave bead in_progress (needs attention)

### Bead Prompt Format

Launchers SHOULD construct a prompt like:

```markdown
# Task: <bead-id>: <bead-title>

## Description
<bead-description>

## Context
- Priority: P0-P4
- Type: task/bug/feature/etc
- Workspace: <workspace-path>
- Labels: <labels>

## Instructions
You are working on bead <bead-id>. Follow the task description above.

When you have completed the task:
1. Ensure all requirements are met
2. Commit your changes with clear commit messages
3. Run any applicable tests
4. Exit with code 0 to mark the bead as complete

If you encounter a blocker:
1. Create a new bead for the blocker
2. Add a dependency from current bead to blocker
3. Exit with code 1 to indicate incomplete status

Current bead ID: <bead-id>
```

## Status File Format

Launchers MUST create a status file at `~/.forge/status/<worker-id>.json`:

### Generic Worker Status

```json
{
  "worker_id": "forge-sonnet-alpha",
  "status": "active",
  "model": "sonnet",
  "workspace": "/home/coder/forge",
  "pid": 12345,
  "started_at": "2026-02-08T23:34:00Z",
  "last_activity": "2026-02-08T23:34:00Z",
  "current_task": {
    "type": "generic",
    "description": "No specific bead assigned"
  },
  "tasks_completed": 0,
  "metadata": {
    "type": "generic_worker"
  }
}
```

### Bead Worker Status

```json
{
  "worker_id": "forge-fg-1qo-sonnet",
  "status": "active",
  "model": "sonnet",
  "workspace": "/home/coder/forge",
  "pid": 12345,
  "started_at": "2026-02-08T23:34:00Z",
  "last_activity": "2026-02-08T23:34:00Z",
  "current_task": {
    "bead_id": "fg-1qo",
    "bead_title": "Design bead-aware launcher protocol",
    "bead_priority": "1"
  },
  "tasks_completed": 0,
  "metadata": {
    "type": "bead_worker",
    "bead_id": "fg-1qo"
  }
}
```

## Reference Implementation

A reference implementation is provided at:

```
scripts/launchers/bead-worker-launcher.sh
```

This script demonstrates:
- Argument parsing for standard and bead-aware modes
- Bead data fetching using `br show`
- Prompt construction with bead context
- tmux session spawning
- Status file creation
- Proper JSON output

## Examples

### Example 1: Launch Worker for Specific Bead

```bash
scripts/launchers/bead-worker-launcher.sh \
  --model=sonnet \
  --workspace=/home/coder/forge \
  --session-name=forge-fg-1qo-sonnet \
  --bead-ref=fg-1qo
```

**Output:**
```json
{
  "worker_id": "forge-fg-1qo-sonnet",
  "pid": 12345,
  "status": "spawned",
  "model": "sonnet",
  "session": "forge-fg-1qo-sonnet",
  "timestamp": "2026-02-08T23:34:00Z",
  "bead_id": "fg-1qo",
  "bead_title": "Design bead-aware launcher protocol"
}
```

### Example 2: Launch Generic Worker

```bash
scripts/launchers/bead-worker-launcher.sh \
  --model=sonnet \
  --workspace=/home/coder/forge \
  --session-name=forge-sonnet-alpha
```

**Output:**
```json
{
  "worker_id": "forge-sonnet-alpha",
  "pid": 12346,
  "status": "spawned",
  "model": "sonnet",
  "session": "forge-sonnet-alpha",
  "timestamp": "2026-02-08T23:35:00Z"
}
```

## Extensibility

The `--bead-ref` pattern is extensible to other task systems:

- `--github-issue=<issue-number>` for GitHub issues
- `--jira-ticket=<ticket-id>` for Jira tickets
- `--linear-task=<task-id>` for Linear tasks

Each task type would have its own launcher implementation that:

1. Fetches task data from its API
2. Constructs an appropriate prompt
3. Updates task status on completion

## Environment Variables

The following environment variables are set by FORGE when launching:

| Variable | Description |
|----------|-------------|
| `FORGE_WORKER_ID` | Unique worker identifier |
| `FORGE_SESSION` | tmux session name |
| `FORGE_MODEL` | Model identifier |
| `FORGE_WORKSPACE` | Workspace path |

## See Also

- [ADR 0015: Bead-Aware Launcher Protocol](docs/adr/0015-bead-aware-launcher-protocol.md)
- [ADR 0005: Dumb Orchestrator](docs/adr/0005-dumb-orchestrator.md)
- [EXTENSIBLE_WORKER_TYPES.md](docs/EXTENSIBLE_WORKER_TYPES.md)
