# FORGE — Plan

This file is new as of 2026-07-20. FORGE predates this workspace's
`docs/plan/plan.md` convention and already has an established, working
architecture-decision process in `docs/adr/` (18 accepted ADRs covering the TUI
choice, cost-optimization strategy, bead integration, real-time updates, security,
error handling, launcher protocol, onboarding, testing, and crash recovery — see
`docs/adr/README.md` for the index). Rather than fabricate a retroactive full plan
here, this file exists to hold new, forward-looking ADRs going forward, cross-linked
with the existing `docs/adr/` sequence so the project keeps a single numbering
scheme.

For the existing architecture and feature set, see:
- `README.md` — what FORGE is, install/usage, crate map
- `docs/ARCHITECTURE.md`, `docs/diagrams/` — system architecture
- `docs/adr/` — the full accepted-decision history
- `CHANGELOG.md` — release history (currently v0.3.0, feature-complete /
  maintenance mode)

---

## ADR-19: 2026-07-20 — Closed-Loop Complexity Calibration from Cost/Outcome Data

**Status**: Proposed
**Full text**: [`docs/adr/0019-closed-loop-complexity-calibration.md`](../adr/0019-closed-loop-complexity-calibration.md)
(filed there too, to keep this project's single ADR sequence intact — this heading
is a pointer per this workspace's plan.md convention.)

### Context

FORGE's stated core value proposition (ADR 0003) is subscription-first,
complexity-aware model routing. The router (`crates/forge-worker/src/complexity.rs`)
is a static, keyword-weighted heuristic with hardcoded tier thresholds. Separately,
`crates/forge-cost` already records per-bead actuals (model used, tokens, cost) and
`optimizer.rs::calculate_model_efficiency()` already computes cost-per-success per
model — but only as a *displayed* recommendation. The two systems have never been
wired together: the router doesn't know whether its past assignments were
cost-effective, and the optimizer's insight goes nowhere but a metrics panel. A
design for exactly this feedback loop exists in
`docs/notes/algorithms/adaptive-learning-system.md` but predates the Rust rewrite
and was never implemented.

### Decision

Add a periodic (not per-request) calibration pass: persist the predicted
complexity score/tier at spawn time (new `forge-cost` migration v4,
`task_assignments` table), join it against actual outcomes on a daily cadence
(reusing existing rollup infrastructure), and adjust `ComplexityConfig`'s tier
*thresholds* (not the keyword weights) when a minimum sample size is met. Write
recalibrated thresholds to `~/.forge/config.yaml` under `calibrated_thresholds`,
log the change to the existing activity-log event pipe (ADR 0008), and gate the
whole thing behind `auto_calibrate: true|false` (default true) so it stays
overridable, preserving ADR 0005's "dumb orchestrator" principle. Ownership stays
in `forge-worker` (which already owns `ComplexityConfig`), reading from
`forge-cost`'s query API, to keep the crate dependency graph one-directional.

### Alternatives Considered

1. Full online/per-task weight learning — rejected, too noisy at typical
   single-user sample sizes and violates the "dumb orchestrator" legibility goal.
2. Status quo (human reads the optimizer's suggestion and hand-edits YAML) —
   rejected, this is what ships today and the evidence shows nobody acts on it.
3. Replace the heuristic with an LLM-judged complexity call per task — rejected,
   adds latency/cost/dependency to the hot path of every single assignment,
   defeating the point of a free "budget tier" check.
4. Put calibration inside `forge-cost::optimizer` and have it write config
   directly — rejected in favor of keeping `forge-cost` read/aggregate-only and
   `forge-worker` as the sole owner/writer of `ComplexityConfig`.

### Consequences

- Positive: routing quality improves with usage instead of staying static forever;
  makes the optimizer's existing computation load-bearing instead of decorative;
  directly serves the project's own stated value proposition.
- Negative: requires a new `forge-cost` migration and new call sites at every
  `forge-worker` spawn path to persist predictions not captured today; introduces
  a new failure mode (bad calibration from a skewed sample), mitigated by a
  minimum-sample-size gate and always-visible, always-overridable output; adds a
  small amount of new maintenance surface to a project in maintenance mode,
  justified as the single highest-leverage change identified in this review.

See the full ADR (`docs/adr/0019-closed-loop-complexity-calibration.md`) for
complete references and code-location detail.

---

## Other improvement ideas from this review (2026-07-20)

Not architecturally significant enough to warrant their own ADR; filed as
`artifact-improvement`-labeled beads instead (see `bf list --label
artifact-improvement` in this repo's `.beads/` workspace):

- Persist predicted complexity score at spawn time — Phase 1 groundwork for
  ADR-19.
- Implement the periodic recalibration job itself — Phase 2, depends on the above.
- Self-update (`crates/forge-core/src/self_update.rs`) only checks ELF magic bytes
  before swapping the running binary — no checksum/signature verification of the
  downloaded release asset.
- `README.md`'s example `costs:` config block (`daily_budget_usd`,
  `alert_threshold_pct`) uses field names that don't exist in
  `CostTrackingConfig` (`crates/forge-config/src/lib.rs`) — copy-pasting it
  silently no-ops budget tracking.
- `forge-server`'s auth (`crates/forge-server/src/auth.rs`) compares plaintext
  passwords directly with no hashing, and `docs/TEAM_COLLABORATION.md`'s example
  config shows binding to `0.0.0.0` over unencrypted `ws://` with no caveat.
- Chat interface (`forge-chat`) round-trips every query through the LLM backend,
  even high-frequency exact-match queries like "what did I spend today" — a local
  fast-path for a small set of common phrasings would cut latency and API cost
  for the most common interactions without changing the tool-based architecture
  for anything else.
