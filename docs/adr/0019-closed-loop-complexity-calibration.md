# ADR 0019: Closed-Loop Complexity Calibration from Cost/Outcome Data

**Date**: 2026-07-20
**Status**: Proposed
**Deciders**: Artifact improvement review (fleet-wide pass, 2026-07-20)

## Context

FORGE's stated core value proposition (ADR 0003, "Cost Optimization Strategy") is
subscription-first, complexity-aware routing: cheap tasks go to budget models, hard
tasks go to premium models, and the user saves money without having to think about it.

The routing decision is made by `crates/forge-worker/src/complexity.rs`: a static,
keyword-weighted heuristic (title 30%, labels 25%, file count 20%, blocks 15%, task
type 10%) that produces a 0-100 score and maps it to a fixed tier via hardcoded
thresholds (Budget ≤30, Standard ≤60, Premium >60). The weights and thresholds are
compile-time constants; the only way to change them today is to hand-edit
`ComplexityConfig` in `~/.forge/config.yaml`.

Separately, `crates/forge-cost` already records, per bead, exactly what actually
happened: `api_calls.bead_id` / `task_events.bead_id` capture the model used, tokens
consumed, and cost. `crates/forge-cost/src/optimizer.rs::calculate_model_efficiency()`
already turns this into a cost-per-success figure per model and even emits a
human-readable recommendation ("Route simple tasks to claude-haiku-4") that surfaces
in the Metrics/Optimization panel (`crates/forge-tui/src/metrics_panel.rs`).

These two systems never talk to each other. The scorer that assigns a tier is
completely blind to whether its own past assignments were cost-effective; the
optimizer that knows the outcomes only prints a suggestion that a human has to notice
and then manually act on by hand-editing YAML weights. The design intent for closing
this loop existed from the beginning — `docs/notes/algorithms/adaptive-learning-system.md`
specs a full "adaptive learning system" with prediction/actual tracking — but that note
predates the Rust rewrite (it's written as Python dataclasses) and was never carried
into the shipped crates. `grep` across `crates/` confirms zero references to
"adaptive" outside that doc and zero coupling between `forge-worker`'s scorer and
`forge-cost`'s outcome tables.

Net effect: a FORGE install's routing quality never improves with use. Day 200 of
usage gets exactly the same complexity heuristic as day 1, even though the cost
database has accumulated hundreds of (predicted tier → actual cost/success) pairs
that are never read back.

## Decision

Close the loop with a periodic (not per-request/online) calibration pass:

1. **Persist the prediction.** At worker-spawn time, record the `ComplexityScore`
   (score + tier) that drove the model assignment, keyed by `bead_id`. This doesn't
   exist today — `forge-cost` only records what model was *actually* used, never what
   the scorer *predicted*. Add a `forge-cost` schema migration (v4) — e.g. a
   `task_assignments(bead_id, predicted_score, predicted_tier, assigned_model,
   created_at)` table — and call it from every `forge-worker` spawn path.

2. **Calibrate on a cadence, not per-task.** Reuse the existing daily/hourly rollup
   machinery (`daily_stats`/`hourly_stats`) to periodically join `task_assignments`
   against actual `api_calls`/`task_events` outcomes, and adjust `ComplexityConfig`'s
   **tier thresholds** (not the keyword weights — keep the score itself legible) so
   the empirical cost-per-success curve for each tier stays within target bounds.
   Require a minimum sample size per tier (e.g. N≥20) before acting, to avoid a
   single outlier task skewing thresholds.

3. **Write back, log, and stay overridable.** Persist recalibrated thresholds to
   `~/.forge/config.yaml` under a distinct `calibrated_thresholds` section (not
   silently overwriting user-set values), emit an activity-log entry ("Routing:
   Budget cutoff 30→24, ~$12/mo saved last week") through the existing real-time
   event pipe (ADR 0008), and gate the whole thing behind `auto_calibrate: true|false`
   (default true) so a user can pin thresholds manually — preserving ADR 0005's "dumb
   orchestrator, user stays in control" principle.

4. **Ownership stays in `forge-worker`.** `forge-worker` already owns
   `ComplexityConfig`; the calibration job lives there and reads from `forge-cost`'s
   public query API, rather than having `forge-cost` reach into config — keeps the
   crate dependency graph one-directional per the existing architecture table in
   `README.md`.

## Alternatives Considered

1. **Full online/per-task learning** — adjust weights after every completed task.
   Rejected: single-user local FORGE installs rarely accumulate statistically
   significant per-tier sample sizes fast enough for this to be stable, and it
   violates ADR 0005's "dumb orchestrator" philosophy of a legible, deterministic
   decision path a user can reason about.

2. **Status quo: human-in-the-loop suggestion only** — leave the optimizer's text
   recommendation as-is. Rejected: this is what ships today, and the evidence (a
   `calculate_model_efficiency()` function whose output is display-only, with zero
   code path back to `ComplexityConfig`) shows the suggestion is inert. Nobody
   hand-retunes routing YAML in practice; this is exactly the kind of numeric
   threshold-tuning grind an agent-orchestration tool should automate for its own
   operator.

3. **Replace the keyword heuristic with an LLM-judged complexity call** — rejected:
   adds latency, cost, and an external dependency to the hot path of *every* task
   assignment, defeating the entire point of a free/instant "budget tier" check, and
   removes the auditability that makes ADR 0004's tool-based, deterministic design
   attractive.

4. **Put the calibration job in `forge-cost::optimizer` and have it own writing
   `config.yaml` directly** — considered viable, rejected in favor of keeping
   `forge-cost` strictly read/aggregate-side and letting `forge-worker` (which already
   owns the config type) pull from it — avoids a new dependency cycle and matches the
   existing crate boundaries.

## Consequences

**Positive**
- FORGE's routing gets measurably better the longer it runs, closing the gap between
  documented design intent (`adaptive-learning-system.md`) and shipped behavior.
- Makes the optimizer's existing, already-computed recommendations load-bearing
  instead of decorative — no new cost-analysis logic needed, just a feedback wire.
- Directly serves the project's own stated value proposition (ADR 0003) rather than
  adding a tangential feature.

**Negative**
- Requires a new `forge-cost` migration (v4) and a new call site at every
  `forge-worker` spawn path to persist predictions that aren't captured today.
- New failure mode: a bad calibration run could push thresholds the wrong way if the
  sample is skewed; mitigated by the minimum-sample-size gate and always-visible,
  always-overridable config output.
- Adds a small amount of new maintenance surface (one background job, one config
  section) to a project in maintenance mode — justified here specifically because
  it's bounded, reuses existing rollup infrastructure, and is the single highest-
  leverage change identified in this review.

## References

- `crates/forge-worker/src/complexity.rs` — static complexity scorer
- `crates/forge-worker/src/scorer.rs` — priority/routing tier application
- `crates/forge-cost/src/db.rs` — `api_calls`, `task_events`, `model_performance` schema
- `crates/forge-cost/src/optimizer.rs::calculate_model_efficiency()` — existing,
  currently-inert efficiency computation
- `docs/notes/algorithms/adaptive-learning-system.md` — original (pre-Rust,
  unimplemented) design for this feedback loop
- `docs/adr/0003-cost-optimization-strategy.md` — the value proposition this closes
  the loop on
- `docs/adr/0005-dumb-orchestrator-architecture.md` — constraint this decision
  respects (thresholds not weights, always overridable)
- `docs/adr/0008-real-time-update-architecture.md` — event pipe reused for the
  calibration log entry
