# ADR 0020: Bead-rs Write Authority and CLI Normalization

**Status**: Accepted
**Date**: 2026-09-16
**Deciders**: FORGE Architecture Team

---

## Context

This ADR records the bead-rs-specific clarification behind the revised ADR
0007; the canonical executable name in all current FORGE commands is `bead`.

ADR 0007 fixed the bead integration model: FORGE is a read-only consumer of
the bead store, and the bead CLI is the sole write authority. Two things have
since drifted:

1. **The environment standardized on bead-rs.** The workspace store is now the
   bead-rs layout (`.beads/config.json` plus `.beads/checkpoint/` with
   `current.json`, `forensic.jsonl`, and `objects/*.jsonl`), and the `br`
   (beads_rust) and `bf` (bead-forge) binaries are deprecated environment-wide
   in favor of `bead`. Earlier documents — including ADR 0007 and the
   bead-aware launcher protocol — still described FORGE as reading a flat
   `.beads/*.jsonl` file and shelling out to `br`. Implemented against those
   documents, the task queue silently reads as empty after a store migration.

2. **An internal contradiction on who writes.** The launcher protocol said
   FORGE (or its launcher) "updates bead status on completion", which reads as
   a store write and conflicts with ADR 0007's "FORGE is read-only". Meanwhile
   the implementation (`BeadScheduler`'s `BeadStatusBackend::Cli`) already
   applies status transitions by shelling out to the CLI — which ADR 0007
   explicitly accepted ("BR CLI Subprocess Calls: accepted for writes").

---

## Decision

### 1. Reads: the checkpoint, not the CLI

FORGE reads bead state by parsing the workspace bead store directly through
`forge_core::bead_store`:

- **bead-rs** (current): `.beads/config.json` + `.beads/checkpoint/` — the
  snapshot named by `current.json`'s `active_root`, falling back to
  `forensic.jsonl`.
- **legacy bead-forge** (deprecated, still parsed): flat `.beads/issues.jsonl`.

The checkpoint is the committed, durable copy of the store, so reads need no
CLI subprocess, cannot block the UI on a slow invocation, and work on a fresh
clone where `beads.db` has not been restored. FORGE never opens the SQLite
live database (unchanged from ADR 0007).

### 2. Writes: CLI subprocess only — by whoever drives the lifecycle

**The `bead` CLI is the sole write authority.** "FORGE is read-only" means
FORGE never writes `.beads/` itself — no file writes, no SQLite access. It
does not mean FORGE cannot cause a transition: FORGE applies lifecycle
transitions by *invoking the CLI*, exactly as ADR 0007 accepted for writes.
This resolves the contradiction with the launcher protocol: both are the same
mechanism.

- `BeadScheduler` (`BeadStatusBackend::Cli`) invokes:
  - `bead update <id> --status in_progress --assignee <worker>` on launch —
    taken as a *guarded cross-process claim* (`BeadClaimBackend`): the
    scheduler reads `bead show <id> --json` first and passes
    `--if-revision <revision>`, so a second FORGE instance or an external
    worker sharing the queue cannot claim the same bead (the loser's update
    exits 4 and surfaces as `ForgeError::BeadClaimConflict`)
  - `bead close <id> --reason "..." --fencing-token <claim-epoch>` on
    completion, where the epoch comes from `bead show <id> --json`
  - `bead release <id> --fencing-token <claim-epoch>` when an assignment is
    freed without completion (the CLI's atomic claimed → open/unassigned
    transition; the older
    `update --status open` + `--assignee ""` shape was not a valid
    invocation and risked the assigned-but-open state the ready frontier
    silently skips)
- A standalone bead-aware launcher (the launcher protocol, §4) applies the
  same transitions when it drives the lifecycle instead of FORGE.
- Workers remain autonomous and may update their own beads via the CLI
  (ADR 0007, unchanged).
- `BeadStatusBackend::DryRun` records transitions in memory for tests and
  dry-runs without executing any CLI.

### 3. CLI normalization

All current-facing documentation and code name the `bead` binary (bead-rs).
References to `br` / `bf` in ADRs dated before this decision are historical
and read as `bead` today. The legacy flat JSONL format remains a *read*
compatibility target; it is not a write target.

---

## Consequences

### Positive

1. **No empty queue after migration** — the reader tracks the bead-rs
   checkpoint, so stores migrated from bead-forge keep flowing into the task
   queue.
2. **One write model** — the launcher protocol and ADR 0007 now describe the
   same mechanism (CLI subprocess), with the CLI as the only component that
   touches the store.
3. **Atomic release semantics** — `bead release` replaces an invalid
   two-command reopen shape that could strand beads as assigned-but-open.

### Negative

1. **Checkpoint read latency** — parsing `forensic.jsonl` on very large stores
   costs more than a targeted CLI query; mitigated by the scheduler/manager
   caching ready beads between polls.
2. **Two write drivers** — FORGE and standalone launchers can both invoke the
   CLI; a deployment should pick one per assignment (the launcher protocol
   assigns that responsibility explicitly).

---

## Alternatives Considered

#### FORGE writes the store directly
**Rejected**: violates the single-writer principle behind ADR 0007, brings
SQLite locking into FORGE, and duplicates CLI validation logic.

#### Route all FORGE-initiated writes through the worker
**Rejected**: a crashed or non-bead-aware worker could never release its
assignment; the scheduler needs a write path that does not depend on the
worker's cooperation.

#### Keep `br` compatibility shims in code
**Rejected**: `br`/`bf` are deprecated environment-wide; supporting a
configurable binary name (`BeadStatusBackend::Cli { binary }`) is enough for
the transition and keeps the default canonical.

---

## References

- [ADR 0007: Bead Integration Strategy](0007-bead-integration-strategy.md) —
  read-only consumer model this ADR clarifies
- [ADR 0015: Bead-Aware Launcher Protocol](0015-bead-aware-launcher-protocol.md)
- [Bead-Aware Launcher Protocol](../BEAD_LAUNCHER_PROTOCOL.md) — living
  protocol document, §4
- bead-rs CLI: `bead --help`, `bead show --help`, `bead update --help`
