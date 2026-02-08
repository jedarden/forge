# ADR 0015: Bead-Aware Launcher Protocol

## Status

Accepted

## Context

FORGE is a "dumb orchestrator" that spawns AI workers using launcher scripts. The original launcher protocol (ADR 0005) defined standard arguments for launching workers: `--model`, `--workspace`, `--session-name`, and `--config`.

As FORGE evolves to support bead-based task queues, we need a way to allocate workers to specific beads while maintaining clean separation between forge (orchestrator) and workers (task executors).

## Problem

1. **No bead context**: Workers don't know which bead they're working on
2. **Manual assignment**: Users must manually assign workers to beads via chat
3. **No automation**: Can't automatically spawn workers for ready beads
4. **Tight coupling risk**: Adding bead logic to workers violates "dumb orchestrator" principle

## Decision

### Bead-Aware Launcher Protocol

Extend the launcher protocol with an optional `--bead-ref` parameter that allows forge to pass bead context to launchers.

#### Standard Arguments (unchanged)

```bash
launcher.sh \
  --model=<model> \
  --workspace=<path> \
  --session-name=<name> \
  [--config=<path>]
```

#### New Bead Argument (optional)

```bash
launcher.sh \
  --model=<model> \
  --workspace=<path> \
  --session-name=<name> \
  [--bead-ref=<bead-id>]  # NEW: Optional bead reference
```

### Launcher Responsibilities

When `--bead-ref` is provided, the launcher MUST:

1. **Fetch bead data** using `br show <bead-id>`
2. **Parse bead JSON** to extract title, description, priority, type
3. **Construct prompt** with bead context
4. **Launch worker** with injected task/prompt
5. **Update bead status** to `in_progress` on launch
6. **Update bead status** to `closed` on completion (exit code 0)

When `--bead-ref` is NOT provided, the launcher behaves normally (generic worker).

### Bead Prompt Format

The launcher constructs a prompt that includes:

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
Work on this task. When complete, exit with code 0.
If blocked or needing human input, exit with code 1.
```

### Reference Implementation

See `scripts/launchers/bead-worker-launcher.sh` for a complete reference implementation.

### Forge Integration

Forge will:

1. **Read bead queues** from `.beads/*.jsonl` files
2. **Identify ready beads** (unblocked, not deferred, not in_progress)
3. **Call launchers** with `--bead-ref=<bead-id>` parameter
4. **Monitor workers** via status files (existing mechanism)
5. **Track bead assignments** in worker metadata

### Worker Bead-Agnosticism

Workers remain bead-agnostic. They receive:

- A prompt constructed by the launcher
- Standard environment variables
- No direct br CLI dependency

The launcher handles all bead interactions.

### Extensibility

The `--bead-ref` pattern is extensible to other task systems:

- `--github-issue=<issue-number>` for GitHub issues
- `--jira-ticket=<ticket-id>` for Jira tickets
- `--linear-task=<task-id>` for Linear tasks

Each task type would have its own launcher implementation that:

1. Fetches task data from its API
2. Constructs an appropriate prompt
3. Updates task status on completion

## Consequences

### Positive

1. **Clean separation**: Forge orchestrates, launchers bridge, workers execute
2. **Flexible launchers**: Different launchers for beads, GitHub, Jira, etc.
3. **Worker agnosticism**: Workers don't need to know about bead system
4. **Automatic assignment**: Forge can auto-assign workers to ready beads
5. **Status tracking**: Bead status automatically updated by launcher

### Negative

1. **Launcher complexity**: Launchers must now fetch and parse bead data
2. **br dependency**: Bead launchers require br CLI in workspace
3. **Status race conditions**: Multiple workers could claim same bead (need locking)

### Mitigations

1. **Launcher library**: Share bead-fetching logic across launchers
2. **Graceful degradation**: Fall back to generic worker if br unavailable
3. **Bead locking**: Use bead-level locking (separate concern, see bead locking ADR)

## Examples

### Example 1: Launch worker for specific bead

```bash
# Forge identifies ready bead fg-1qo
# Forge calls launcher with bead ref
scripts/launchers/bead-worker-launcher.sh \
  --model=sonnet \
  --workspace=/home/coder/forge \
  --session-name=forge-fg-1qo-sonnet \
  --bead-ref=fg-1qo

# Launcher fetches bead data
br show fg-1qo --format json

# Launcher constructs prompt and spawns worker
tmux new-session -d -s "forge-fg-1qo-sonnet" \
  "claude-code --model=sonnet '<bead-prompt>'"
```

### Example 2: Launch generic worker (no bead)

```bash
# Manual spawn via chat interface
scripts/launchers/claude-code-launcher.sh \
  --model=sonnet \
  --workspace=/home/coder/forge \
  --session-name=forge-sonnet-alpha
  # No --bead-ref parameter

# Launcher spawns generic worker
tmux new-session -d -s "forge-sonnet-alpha" \
  "claude-code --model=sonnet"
```

## Implementation

### Phase 1: Protocol Extension

- [x] Define bead-aware launcher protocol
- [ ] Update ADR 0005 with bead parameter
- [ ] Create bead-worker-launcher reference implementation
- [ ] Add tests for bead-aware launchers

### Phase 2: Forge Integration

- [ ] Update WorkerLauncher to pass bead refs
- [ ] Add bead queue reading to BeadManager
- [ ] Implement auto-assignment logic
- [ ] Update status file format to include bead_id

### Phase 3: Status Tracking

- [ ] Implement bead status updates on spawn
- [ ] Implement bead status updates on completion
- [ ] Add bead completion detection
- [ ] Handle bead locking for concurrent access

## References

- ADR 0005: Launcher Protocol
- ADR 0007: Bead Integration
- beads_rust: https://github.com/Dicklesworthstone/beads_rust
- bead-worker.sh: Reference bead worker implementation

## Appendix: Launcher Output Format

Bead-aware launchers MUST include bead info in output JSON:

```json
{
  "worker_id": "forge-fg-1qo-sonnet",
  "pid": 12345,
  "status": "spawned",
  "model": "sonnet",
  "bead_id": "fg-1qo",         // NEW: Bead reference
  "bead_title": "Design bead-aware launcher protocol",
  "session": "forge-fg-1qo-sonnet",
  "timestamp": "2026-02-08T23:34:00Z"
}
```

Status files MUST include bead_id:

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
    "bead_id": "fg-1qo",      // NEW: Current bead
    "bead_title": "Design bead-aware launcher protocol"
  },
  "tasks_completed": 0,
  "metadata": {
    "type": "bead_worker",    // NEW: Worker type
    "bead_id": "fg-1qo"       // NEW: Bead assignment
  }
}
```

## Changelog

- 2026-02-08: Initial ADR creation
