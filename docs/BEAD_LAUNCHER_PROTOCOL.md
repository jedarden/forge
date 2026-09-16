# Bead-Aware Launcher Protocol

## Overview

The bead-aware launcher protocol extends the standard FORGE launcher protocol to enable workers to be allocated to specific beads/tasks from the `bead` CLI issue tracker (bead-rs). This allows forge to automatically distribute work to workers based on bead priority and availability.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         FORGE Control Panel                          │
│                   (Bead Queue Management)                            │
├─────────────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  Bead Queue Reader                                          │   │
│  │  - Reads the workspace bead store (.beads/ checkpoint)     │   │
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
│  1. Fetch bead data (bead show <bead-id> --json)                   │
│  2. Construct prompt with bead context                             │
│  3. Launch worker with injected task                               │
│  4. Apply bead status transitions via the bead CLI (§4)            │
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
- `--bead-ref` - Bead ID from the `bead` CLI (e.g., "fg-1qo", "bd-abc")
  - When present: launcher fetches bead data and constructs task prompt
  - When absent: launcher operates in standard mode (no task assigned)

## Launcher Responsibilities

### 1. Bead Data Fetching

When `--bead-ref` is provided, the launcher MUST:

```bash
# Fetch bead data using the bead CLI (from the bead's workspace)
bead show <bead-id> --json
```

The `--json` output is a one-element JSON array (NEEDLE v1 compatibility);
the bead object is its first element:
```json
[
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
]
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

**The `bead` CLI is the sole write authority over the store** (ADR 0007,
clarified by ADR 0020). No component — not FORGE, not the launcher, not the
worker — ever writes `.beads/` directly. Status transitions are applied by
*invoking the CLI*.

Two components may perform these invocations, never both for the same
assignment:

- **FORGE's scheduler** (`BeadScheduler`, `BeadStatusBackend::Cli`) shells
  out to `bead` itself: `update --status in_progress` on launch, `close` on
  completion, `release` when an assignment is freed without completion.
- **A standalone bead-aware launcher** (this protocol) applies the same
  transitions on FORGE's behalf when it operates without FORGE driving the
  lifecycle.

```bash
# Mark bead as in-progress when worker starts
bead update <bead-id> --status in_progress --assignee <worker-id>

# Mark bead as closed when worker completes
bead close <bead-id> --reason "Completed by <worker-id>" \
  --fencing-token <claim-epoch>

# Release an assignment without completion (back to open/unassigned)
bead release <bead-id> --fencing-token <claim-epoch>
```

For a claimed bead, `<claim-epoch>` comes from `claim_epoch` in the
`bead show <bead-id> --json` response. The fencing token prevents a stale
worker from closing or releasing a later owner's claim; omit it only when the
bead was never claimed.

Workers may also update their own beads via the CLI (worker autonomy,
ADR 0007); FORGE stays read-only over the store files at all times.

#### 4.1 Cross-Process Claims

FORGE's in-process bead → worker mapping is invisible to every other
process on the machine: a second FORGE instance, or an external worker
such as NEEDLE sharing the same bead queue, keeps its own mapping and can
select the very same bead. On the launch path the scheduler therefore makes
the **bead store itself** the shared lock
(`BeadClaimBackend`, `forge_worker::bead_claim`):

1. **Read** the bead's current assignment state:
   `bead show <bead-id> --json` → `assignee`, `status`, `revision`,
   `claim_epoch`.
   A bead the store already shows as held by someone else is refused
   outright.
2. **Claim** with a guarded, conditional write:
   `bead update <bead-id> --status in_progress --assignee <worker>
   --if-revision <revision>`. The `--if-revision` guard makes the
   read-then-write sequence atomic against competing processes: whoever
   commits first bumps the revision, and the loser's update fails with
   exit code 4 instead of silently clobbering the winner. The claim
   doubles as the §4 mark-in-progress transition, so assignment and status
   land in one guarded update.
3. **Verify on retry**: a re-launch of an already-claimed bead re-reads
   the store and only skips the write when the claim still holds for the
   same worker — a claim taken over by another process is reported,
   not assumed.

Lost races surface as `ForgeError::BeadClaimConflict` so the scheduler can
drop the bead and move on to the next candidate rather than launching two
workers onto one bead. When the store's claim state is unreadable (no
`bead` CLI, or a legacy flat store the CLI cannot serve), the scheduler
degrades to the in-process mapping lock rather than failing the launch.

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

The scheduler and pipeline described here are implemented by
`forge_worker::bead_scheduler::BeadScheduler` (crates/forge-worker), with
queue reading in `forge_worker::bead_queue::BeadQueueReader`.

### Bead Queue Reading

Forge reads bead queues from workspaces through `forge_core::bead_store`.
The canonical source is the bead-rs checkpoint committed under `.beads/`:

```text
.beads/
├── config.json
└── checkpoint/
    ├── current.json       # names the active objects/<root>.jsonl snapshot
    ├── forensic.jsonl     # complete fallback snapshot
    └── objects/*.jsonl    # immutable checkpoint generations
```

The reader follows `current.json`'s `active_root.path` and falls back to
`forensic.jsonl` if the active object is unavailable. It never opens the live
`.beads/beads.db`, because the checkpoint is the portable, committed read
surface. Legacy flat `.beads/issues.jsonl` is still parsed as a read-only
compatibility path, but it is not the canonical queue format and FORGE never
writes it.

```rust
// Parse bead data from the bead-rs checkpoint
let beads = forge_core::read_all_beads(workspace)?;
let index = forge_core::bead_store::build_index(&beads);
// Filter for ready beads (open, unassigned, no unfinished blockers)
let ready = beads
    .iter()
    .filter(|bead| forge_core::bead_store::is_ready(bead, &index));
// BeadQueueReader sorts ready beads by task score, then priority and ID.
```

### Bead Allocation

`BeadScheduler` allocates beads to workers:

```rust
// For each ready bead:
// 1. Check if bead is already assigned (BeadAlreadyAssigned is returned
//    if a second worker claims it — the mapping is the lock)
// 2. Fetch the bead context and build the task prompt
// 3. Claim the bead in the store with a guarded write
//    (bead update --if-revision — §4.1); a lost race is a
//    BeadClaimConflict and the bead is skipped
// 4. Call launcher with --bead-ref=<bead-id>
// 5. Track assignment in the bead -> worker mapping
// 6. The claim doubles as the in-progress transition (§4)
// 7. On completion: close the bead and record it
//    (scheduler.record_completion), or release it for reallocation
//    (scheduler.release) if the worker failed
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
$ bead show fg-1qo --json
[
  {
    "id": "fg-1qo",
    "title": "Design bead-aware launcher protocol",
    ...
  }
]
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
bead update fg-1qo --status in_progress --assignee forge-fg-1qo-sonnet
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
bead close fg-1qo --reason "Completed by forge-fg-1qo-sonnet"
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
if ! bead show "$BEAD_REF" --json >/dev/null 2>&1; then
  echo "Error: Bead $BEAD_REF not found" >&2
  exit 1
fi
```

### Bead Already Closed

```bash
# Check bead status before proceeding (show --json returns a one-element
# array; the bead object is element 0)
status=$(bead show "$BEAD_REF" --json | jq -r '.[0].status')
if [ "$status" = "closed" ]; then
  echo "Error: Bead $BEAD_REF is already closed" >&2
  exit 1
fi
```

### bead CLI Not Available

```bash
# Verify bead is available
if ! command -v bead >/dev/null 2>&1; then
  echo "Error: bead CLI not found" >&2
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
bead show fg-1qo

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
- [bead CLI (bead-rs)](https://git.ardenone.com/jedarden/bead-rs) - Issue tracker
- [ADR 0007: Bead Integration Strategy](./adr/0007-bead-integration-strategy.md) - Bead integration model
- [ADR 0020: Bead Write Authority](./adr/0020-bead-write-authority.md) - Who writes the store, and how
