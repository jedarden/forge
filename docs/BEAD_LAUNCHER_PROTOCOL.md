# Bead-Aware Launcher Protocol

## Overview

The bead-aware launcher protocol extends the standard FORGE launcher protocol to enable workers to be allocated to specific beads/tasks from the `br` CLI issue tracker. This allows forge to automatically distribute work to workers based on bead priority and availability.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         FORGE Control Panel                          │
│                   (Bead Queue Management)                            │
├─────────────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  Bead Queue Reader                                          │   │
│  │  - Reads .beads/*.jsonl from workspaces                     │   │
│  │  - Identifies ready beads (unblocked, not deferred)        │   │
│  │  - Sorts by priority (P0 → P4)                              │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                        │
│                              ▼                                        │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  Bead Scheduler                                            │   │
│  │  - Assigns beads to available workers                      │   │
│  │  - Tracks bead → worker mapping                            │   │
│  │  - Prevents duplicate bead assignment                      │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                        │
│                              ▼                                        │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  Launcher Protocol Extension                               │   │
│  │  --bead-ref=<bead-id> parameter                            │   │
│  └─────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     Bead-Aware Launcher                              │
│  1. Fetch bead data (br show <bead-id>)                            │
│  2. Construct prompt with bead context                             │
│  3. Launch worker with injected task                               │
│  4. Update bead status on completion                               │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         Worker (Bead-Agnostic)                      │
│  - Receives task via prompt or stdin                               │
│  - Works on task independently                                     │
│  - No knowledge of bead system                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Protocol Specification

### Standard Launcher Arguments

All launchers MUST support these standard arguments:

```bash
launcher \
  --model=<model> \
  --workspace=<path> \
  --session-name=<name> \
  [--config=<path>]
```

### Bead-Aware Extension

Bead-aware launchers MUST additionally support:

```bash
launcher \
  --model=<model> \
  --workspace=<path> \
  --session-name=<name> \
  --bead-ref=<bead-id> \
  [--config=<path>]
```

**New Parameter:**
- `--bead-ref` - Bead ID from br CLI (e.g., "fg-1qo", "bd-abc")
  - When present: launcher fetches bead data and constructs task prompt
  - When absent: launcher operates in standard mode (no task assigned)

## Launcher Responsibilities

### 1. Bead Data Fetching

When `--bead-ref` is provided, the launcher MUST:

```bash
# Fetch bead data using br CLI
br show <bead-id> --format json
```

Expected JSON structure:
```json
{
  "id": "fg-1qo",
  "title": "Design bead-aware launcher protocol",
  "description": "Full task description...",
  "status": "open",
  "priority": 0,
  "issue_type": "feature",
  "labels": ["launcher", "protocol"],
  "dependencies": [],
  "workspace": "/home/coder/forge"
}
```

### 2. Task Prompt Construction

Construct a prompt that includes:
- Bead ID and title
- Full description
- Priority level
- Any relevant labels
- Workspace context

Example prompt template:
```
You are working on bead {bead_id}: {title}

Priority: P{priority} ({priority_label})
Type: {issue_type}
Labels: {labels}

Description:
{description}

Workspace: {workspace}

Please work on this task. When complete, your changes should be committed to git.
```

### 3. Worker Launch

Launch the worker with the constructed prompt:
- For headless CLIs: pipe prompt to stdin
- For tmux sessions: write to a temp file and pass as argument
- For interactive tools: set environment variable with prompt

### 4. Bead Status Updates

Launcher MUST update bead status at appropriate times:

```bash
# Mark bead as in-progress when worker starts
br update <bead-id> --status in_progress --assignee <worker-id>

# Mark bead as closed when worker completes
br close <bead-id> --reason "Completed by <worker-id>"
```

### 5. Status File Enhancement

The status file MUST include bead reference:

```json
{
  "worker_id": "sonnet-alpha",
  "status": "active",
  "model": "sonnet",
  "workspace": "/home/coder/forge",
  "pid": 12345,
  "started_at": "2026-02-08T23:00:00Z",
  "last_activity": "2026-02-08T23:00:00Z",
  "current_task": {
    "bead_id": "fg-1qo",
    "bead_title": "Design bead-aware launcher protocol",
    "priority": 0
  },
  "tasks_completed": 0
}
```

## Reference Implementation

See `test/example-launchers/bead-worker-launcher.sh` for a complete reference implementation.

## FORGE Integration

### Bead Queue Reading

Forge reads bead queues from workspace directories:

```rust
// Read .beads/*.jsonl files
// Parse bead data
// Filter for ready beads (status=open, dependency_count=0)
// Sort by priority (P0 first)
```

### Bead Allocation

Forge allocates beads to workers:

```rust
// For each ready bead:
// 1. Check if bead is already assigned
// 2. Find available worker (matching tier)
// 3. Call launcher with --bead-ref=<bead-id>
// 4. Track assignment in memory
// 5. Update bead status to in_progress
```

### Status Monitoring

Forge monitors worker status files:
- Track which bead each worker is working on
- Detect worker completion
- Update bead status accordingly
- Reassign beads if workers fail

## Example Workflow

### 1. Forge Identifies Ready Bead

```
Ready bead found: fg-1qo "Design bead-aware launcher protocol" [P0]
```

### 2. Forge Calls Launcher

```bash
bead-worker-launcher \
  --model=sonnet \
  --workspace=/home/coder/forge \
  --session-name=forge-fg-1qo-sonnet \
  --bead-ref=fg-1qo
```

### 3. Launcher Fetches Bead Data

```bash
$ br show fg-1qo --format json
{
  "id": "fg-1qo",
  "title": "Design bead-aware launcher protocol",
  ...
}
```

### 4. Launcher Constructs Prompt

```
You are working on bead fg-1qo: Design bead-aware launcher protocol

Priority: P0 (Critical)
Type: feature
Labels: launcher, protocol

Description:
Design launcher protocol extension that allows forge to allocate workers...
```

### 5. Launcher Spawns Worker

```bash
tmux new-session -d -s "forge-fg-1qo-sonnet" \
  "cd /home/coder/forge && claude-code --model=sonnet << 'EOF'
You are working on bead fg-1qo: Design bead-aware launcher protocol
...
EOF"
```

### 6. Launcher Updates Bead Status

```bash
br update fg-1qo --status in_progress --assignee forge-fg-1qo-sonnet
```

### 7. Launcher Outputs Metadata

```json
{
  "worker_id": "forge-fg-1qo-sonnet",
  "pid": 12345,
  "status": "spawned",
  "bead_ref": "fg-1qo",
  "timestamp": "2026-02-08T23:00:00Z"
}
```

### 8. Forge Monitors Progress

```bash
# Watch status file for completion
tail -f ~/.forge/status/forge-fg-1qo-sonnet.json

# When worker completes:
br close fg-1qo --reason "Completed by forge-fg-1qo-sonnet"
```

## Compatibility

### Backward Compatibility

Launchers that don't support `--bead-ref` remain functional:
- Forge operates in standard mode (no bead assignment)
- Workers run without specific tasks
- Existing workflows continue to work

### Forward Compatibility

Launchers can opt-in to bead-aware mode:
- Add `--bead-ref` parameter support
- Implement bead fetching and prompt construction
- Maintain standard mode when `--bead-ref` is absent

## Error Handling

### Bead Not Found

```bash
# Launcher should handle gracefully
if ! br show "$BEAD_REF" --format json >/dev/null 2>&1; then
  echo "Error: Bead $BEAD_REF not found" >&2
  exit 1
fi
```

### Bead Already Closed

```bash
# Check bead status before proceeding
status=$(br show "$BEAD_REF" --format json | jq -r '.status')
if [ "$status" = "closed" ]; then
  echo "Error: Bead $BEAD_REF is already closed" >&2
  exit 1
fi
```

### br CLI Not Available

```bash
# Verify br is available
if ! command -v br >/dev/null 2>&1; then
  echo "Error: br CLI not found" >&2
  exit 1
fi
```

## Testing

### Test Bead-Aware Launcher

```bash
# Test with a real bead
./test/example-launchers/bead-worker-launcher.sh \
  --model=sonnet \
  --workspace=/home/coder/forge \
  --session-name=test-bead-launch \
  --bead-ref=fg-1qo

# Verify bead status updated
br show fg-1qo

# Verify status file contains bead_ref
cat ~/.forge/status/test-bead-launch.json | jq '.current_task'
```

### Test Standard Mode (No Bead)

```bash
# Test without bead ref (should work normally)
./test/example-launchers/bead-worker-launcher.sh \
  --model=sonnet \
  --workspace=/home/coder/forge \
  --session-name=test-standard-launch
```

## Future Enhancements

1. **Multi-Bead Assignment**: Workers could handle multiple related beads
2. **Bead Dependencies**: Launcher could fetch and display dependency chain
3. **Progress Reporting**: Workers could report incremental progress back to bead
4. **Automatic Reassignment**: Detect stuck workers and reassign beads
5. **Bead Time Tracking**: Track time spent per bead for analytics

## Related Documentation

- [FORGE Launcher Protocol](../test/example-launchers/README.md) - Standard launcher protocol
- [br CLI Documentation](https://github.com/Dicklesworthstone/beads_rust) - Issue tracker
- [Bead Architecture](./ADR_0002_BEAD_INTEGRATION.md) - Bead system architecture
