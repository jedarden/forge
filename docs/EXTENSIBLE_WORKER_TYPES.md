# Extensible Worker Type System

**Date**: 2026-02-08
**Status**: Accepted
**Related**: ADR 0005 (Dumb Orchestrator Architecture)

---

## Overview

FORGE's "dumb orchestrator" design allows any worker type to integrate by writing status files and logs to well-known locations. The `metadata` field in the status file enables worker-specific data without requiring changes to FORGE core.

---

## Worker Status File Format

### Required Fields

All workers must include these fields in `~/.forge/status/<worker-id>.json`:

```json
{
  "worker_id": "unique-worker-identifier",
  "status": "active",           // active, idle, failed, stopped
  "model": "model-name",        // e.g., "claude-glm", "sonnet", "gpt4"
  "workspace": "/path/to/project",
  "pid": 12345,
  "started_at": "2026-02-08T10:30:00Z",
  "last_activity": "2026-02-08T10:35:00Z",
  "current_task": null,         // null or task identifier
  "tasks_completed": 0,
  "uptime_seconds": 300
}
```

### Optional Metadata Field

Workers may include a `metadata` object with type-specific data:

```json
{
  "worker_id": "claude-code-glm-47-alpha",
  "status": "active",
  "model": "claude-glm",
  "workspace": "/home/coder/forge",
  "pid": 12345,
  "started_at": "2026-02-08T10:30:00Z",
  "last_activity": "2026-02-08T10:35:00Z",
  "current_task": "fg-1qj",
  "tasks_completed": 5,
  "uptime_seconds": 300,
  "metadata": {
    "type": "bead_worker",
    "bead_id": "fg-1qj",
    "workspace_lock": "/home/coder/forge/.beads/locks/fg-1qj.lock",
    "executor": "claude-code-glm-47",
    "iteration": 3,
    "max_iterations": 100
  }
}
```

---

## Worker Type Examples

### Bead Worker (Reference Implementation)

**Purpose**: Processes beads from `br` CLI issue tracker

**Metadata Fields**:
- `type`: Always `"bead_worker"`
- `bead_id`: Current bead being processed
- `workspace_lock`: Path to lock file (if any)
- `executor`: Executor type (claude-glm, opencode, etc.)
- `iteration`: Current iteration number
- `max_iterations`: Maximum iterations before giving up

**Example Status File**:
```json
{
  "worker_id": "claude-code-glm-47-alpha",
  "status": "active",
  "model": "claude-glm",
  "workspace": "/home/coder/forge",
  "pid": 309581,
  "started_at": "2026-02-08T18:27:48+00:00",
  "last_activity": "2026-02-08T18:29:24+00:00",
  "current_task": "fg-1qj",
  "tasks_completed": 12,
  "uptime_seconds": 96,
  "metadata": {
    "type": "bead_worker",
    "bead_id": "fg-1qj",
    "workspace_lock": "/home/coder/forge/.beads/locks/fg-1qj.lock",
    "executor": "claude-code-glm-47",
    "iteration": 3,
    "max_iterations": 100
  }
}
```

**Implementation**: `/home/coder/claude-config/scripts/bead-worker.sh`

---

### GitHub Issue Worker

**Purpose**: Processes GitHub issues from a repository

**Metadata Fields**:
- `type`: Always `"github_worker"`
- `issue_id`: GitHub issue number
- `repo_owner`: Repository owner
- `repo_name`: Repository name
- `labels`: Array of issue labels
- `milestone`: Current milestone (if any)

**Example Status File**:
```json
{
  "worker_id": "github-worker-alpha",
  "status": "active",
  "model": "gpt4",
  "workspace": "/home/coder/myproject",
  "pid": 45678,
  "started_at": "2026-02-08T10:00:00Z",
  "last_activity": "2026-02-08T10:15:00Z",
  "current_task": "123",
  "tasks_completed": 8,
  "uptime_seconds": 900,
  "metadata": {
    "type": "github_worker",
    "issue_id": 123,
    "repo_owner": "myorg",
    "repo_name": "myproject",
    "labels": ["bug", "high-priority"],
    "milestone": "v1.0.0"
  }
}
```

---

### Jira Task Worker

**Purpose**: Processes Jira tickets from a project

**Metadata Fields**:
- `type`: Always `"jira_worker"`
- `ticket_id`: Jira ticket key (e.g., "PROJ-123")
- `project_key`: Jira project key
- "issue_type": Type of issue (Bug, Story, Task)
- "priority": Jira priority level

**Example Status File**:
```json
{
  "worker_id": "jira-worker-bravo",
  "status": "idle",
  "model": "claude-sonnet",
  "workspace": "/home/coder/myproject",
  "pid": 78901,
  "started_at": "2026-02-08T09:00:00Z",
  "last_activity": "2026-02-08T09:30:00Z",
  "current_task": null,
  "tasks_completed": 15,
  "uptime_seconds": 1800,
  "metadata": {
    "type": "jira_worker",
    "ticket_id": "PROJ-123",
    "project_key": "PROJ",
    "issue_type": "Bug",
    "priority": "High"
  }
}
```

---

### Generic Task Runner

**Purpose**: Executes tasks from a simple queue file

**Metadata Fields**:
- `type`: Always `"task_runner"`
- `queue_file`: Path to task queue file
- `queue_position`: Current position in queue
- `queue_length`: Total number of tasks in queue

**Example Status File**:
```json
{
  "worker_id": "task-runner-1",
  "status": "active",
  "model": "local-llama",
  "workspace": "/home/coder/myproject",
  "pid": 23456,
  "started_at": "2026-02-08T08:00:00Z",
  "last_activity": "2026-02-08T08:05:00Z",
  "current_task": "compile-project",
  "tasks_completed": 3,
  "uptime_seconds": 300,
  "metadata": {
    "type": "task_runner",
    "queue_file": "/home/coder/myproject/tasks.txt",
    "queue_position": 4,
    "queue_length": 10
  }
}
```

---

## FORGE Core Behavior

### Reading Status Files

FORGE reads status files from `~/.forge/status/*.json` and:

1. **Ignores unknown fields**: Any field not in the core schema is ignored
2. **Preserves metadata**: The `metadata` object is passed through to UI components
3. **No validation**: FORGE does not validate metadata structure

### UI Display

The TUI displays:
- **Required fields** in the main worker panel
- **Metadata** can be shown in a details panel or tooltip
- **Type-specific views** can be enabled based on `metadata.type`

### Log Parsing

Logs are parsed from `~/.forge/logs/<worker-id>.log` and support:
- JSON lines format
- Key-value format
- Structured events with `event` field

---

## Implementing a New Worker Type

### Step 1: Define Your Worker Type

Create a unique `type` identifier for your worker:
```bash
WORKER_TYPE="my_custom_worker"
```

### Step 2: Define Metadata Schema

Define what fields your worker needs in `metadata`:
```json
{
  "type": "my_custom_worker",
  "custom_field_1": "value1",
  "custom_field_2": "value2"
}
```

### Step 3: Write Status Files

In your worker script, write status files periodically:

```bash
#!/bin/bash
WORKER_ID="my-worker-1"
WORKSPACE="/path/to/project"
STATUS_FILE="$HOME/.forge/status/$WORKER_ID.json"

update_status() {
    local status="$1"
    local current_task="$2"

    cat > "$STATUS_FILE" << EOF
{
  "worker_id": "$WORKER_ID",
  "status": "$status",
  "model": "my-model",
  "workspace": "$WORKSPACE",
  "pid": $$,
  "started_at": "$STARTED_AT",
  "last_activity": "$(date -Iseconds)",
  "current_task": $(jq -n --arg t "$current_task" '$t'),
  "tasks_completed": $TASKS_COMPLETED,
  "uptime_seconds": $(($(date +%s) - STARTED_EPOCH)),
  "metadata": {
    "type": "my_custom_worker",
    "custom_field_1": "value1",
    "custom_field_2": "value2"
  }
}
EOF
}

# Update status on state transitions
update_status "starting" ""
# ... do work ...
update_status "active" "task-1"
# ... complete task ...
update_status "idle" ""
```

### Step 4: Write Log Files

Write structured logs to `~/.forge/logs/<worker-id>.log`:

```bash
LOG_FILE="$HOME/.forge/logs/$WORKER_ID.log"

log_event() {
    local event="$1"
    local level="${2:-info}"
    local timestamp=$(date -Iseconds)

    # JSON format
    echo "{\"timestamp\": \"$timestamp\", \"level\": \"$level\", \"worker_id\": \"$WORKER_ID\", \"event\": \"$event\"}" >> "$LOG_FILE"

    # Also update status file for last_activity
    update_status "$CURRENT_STATUS" "$CURRENT_TASK"
}

log_event "worker_started" "info"
log_event "task_started" "info"
log_event "task_completed" "success"
```

### Step 5: Test Integration

1. Start your worker
2. Verify status file exists at `~/.forge/status/<worker-id>.json`
3. Verify log file exists at `~/.forge/logs/<worker-id>.log`
4. Launch FORGE: `forge dashboard`
5. Verify your worker appears in the Workers panel

---

## Best Practices

### Status Updates

- **Update on state changes**: Update status when starting work, completing tasks, going idle
- **Include current_task**: Always set `current_task` when working on something
- **Accurate uptime**: Calculate `uptime_seconds` from `started_at`
- **Fresh timestamps**: Update `last_activity` on every status write

### Metadata Design

- **Include type field**: Always set `metadata.type` to identify your worker type
- **Worker-specific fields**: Add fields that make sense for your worker
- **Avoid nesting**: Keep metadata relatively flat for easier parsing
- **Document schema**: Document your metadata fields for other developers

### Log Format

- **Structured JSON**: Use JSON lines format for machine parsing
- **Standard fields**: Include `timestamp`, `level`, `worker_id`, `event`
- **Event-driven**: Log significant events (start, complete, error)
- **Context**: Include relevant context (task_id, error messages)

### Error Handling

- **Failed status**: Set `status` to `"failed"` on unrecoverable errors
- **Error details**: Include error information in metadata
- **Graceful degradation**: Write partial status if full status unavailable

---

## Summary

FORGE's extensible worker type system enables:

1. **Any worker type** to integrate without FORGE core changes
2. **Type-specific metadata** via the `metadata` field
3. **Standardized monitoring** via status files and logs
4. **UI extensibility** through type-aware display components

The bead-worker implementation serves as the reference for other worker types.

---

**See also**:
- ADR 0005: Dumb Orchestrator Architecture
- INTEGRATION_GUIDE.md: Complete integration documentation
- bead-worker.sh: Reference implementation
