# ADR 0007: Bead Integration Strategy

**Status**: Accepted
**Date**: 2026-02-07 (revised 2026-09-16)
**Deciders**: FORGE Architecture Team

## Context

FORGE needs to display and schedule bead work across one or more workspaces.
The original decision described a flat JSONL file and the deprecated `br`
command. Current workspaces use bead-rs instead:

```text
.beads/
├── beads.db                 # local live database; not a FORGE read surface
├── config.json
└── checkpoint/
    ├── current.json         # active checkpoint metadata
    ├── forensic.jsonl       # complete portable fallback
    └── objects/*.jsonl      # immutable checkpoint snapshots
```

The checkpoint is committed and available in a fresh clone even when the live
SQLite database is absent. FORGE must therefore read the checkpoint rather
than assume that a legacy `issues.jsonl` exists or require a CLI subprocess on
the TUI polling path.

## Decision

FORGE is a read consumer of bead state and uses the canonical `bead` CLI as
the sole mutation authority.

### Read path

All queue and dashboard readers use `forge_core::bead_store`:

1. Detect a bead-rs workspace from `.beads/config.json` or its checkpoint.
2. Read the snapshot named by `checkpoint/current.json`'s
   `active_root.path`.
3. Fall back to `checkpoint/forensic.jsonl` when the active object is missing
   or unreadable.
4. Normalize issue records into `StoreBead` values shared by the queue,
   TUI, chat context, workspace aggregation, and recovery code.

The reader ignores event records and never opens `.beads/beads.db`. A flat
`.beads/issues.jsonl` file remains a read-only compatibility fallback for
workspaces that have not migrated; FORGE does not create or update that file.

The ready frontier mirrors `bead list --ready`: a bead is ready only when it
is open, unassigned, not manually blocked, and has no unfinished `blocks`
dependency. In-progress beads are surfaced separately with their assignee.
The queue reader sorts ready beads by FORGE's task score, priority, and ID for
stable scheduling; the CLI remains authoritative for mutation-time validation.

### Write path

FORGE never edits checkpoint files, the live SQLite database, or legacy JSONL
directly. Lifecycle transitions are subprocess calls to `bead`:

```bash
bead update <id> --status in_progress --assignee <worker>
bead close <id> --reason "Completed by <worker>" --fencing-token <claim-epoch>
bead release <id> --fencing-token <claim-epoch>
```

`bead release` is the atomic transition from claimed work back to open and
unassigned. For a claimed bead, `<claim-epoch>` is read from `bead show
<id> --json`; it fences stale workers from mutating a later owner's claim.
The token may be omitted only for an unclaimed transition.

The scheduler and a standalone bead-aware launcher may each drive this
lifecycle, but a given assignment has one driver. Workers may run `bead`
themselves when their workflow requires it; they still do not edit the store
files directly.

### Cross-process claims

The scheduler's in-memory assignment map is not a cross-process lock. Before
launching, `BeadClaimBackend` reads:

```bash
bead show <id> --json  # includes assignee, status, revision, claim_epoch
```

It refuses a bead already assigned to another worker. For an unassigned bead,
it takes the claim with an optimistic revision guard:

```bash
bead update <id> --status in_progress --assignee <worker> \
  --if-revision <revision>
```

A conflict (exit code 4) is reported as `BeadClaimConflict`, and no worker is
spawned. A failed spawn releases only the claim still held by that worker.
If the CLI is unavailable or cannot serve a legacy store, FORGE retains its
in-process lock and logs that cross-process protection is unavailable.

## Consequences

### Positive

- Migrated bead-rs workspaces populate the Tasks panel and scheduler.
- Checkpoint reads are deterministic, cloneable, and independent of SQLite
  availability or CLI latency.
- All mutations use bead-rs validation and atomic transactions.
- Revision-guarded claims prevent duplicate launches across FORGE processes
  and other queue consumers.

### Negative

- Large checkpoint snapshots cost more to parse than a targeted database
  query; FORGE mitigates this with manager polling and cache intervals.
- Legacy workspaces remain readable but cannot receive new writes through
  FORGE until they are migrated to bead-rs.
- A missing `bead` executable disables CLI-backed claims and lifecycle
  updates; read-only queue display still works from the checkpoint.

## Alternatives considered

### Read the live SQLite database

Rejected. It duplicates bead-rs storage and locking behavior, makes fresh
clones unusable, and couples the TUI to an implementation detail of the CLI.

### Shell out to `bead list --ready` for every poll

Rejected as the default read path. It is slower and unavailable when only the
committed checkpoint is present. The CLI remains the authority for writes and
is used for cross-process claim verification.

### Keep the flat JSONL format as the primary source

Rejected. It is not the bead-rs store shape and silently produces an empty
queue after migration. It remains only as a compatibility reader.

## References

- [Bead-Aware Launcher Protocol](../BEAD_LAUNCHER_PROTOCOL.md)
- [ADR 0020: Bead Write Authority](0020-bead-write-authority.md)
- `forge_core::bead_store`
- `bead list --ready --json`
- `bead show --help`, `bead update --help`, `bead release --help`
