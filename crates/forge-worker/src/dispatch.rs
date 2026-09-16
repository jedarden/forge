//! Worker-pool bead dispatch control loop.
//!
//! This module wires [`BeadScheduler`] and [`BeadQueueReader`] into a
//! control loop that turns a workspace's bead queue into running workers
//! (forge-04a1e723). It is the production consumer of the bead-aware
//! launcher pipeline:
//!
//! ```text
//!            ┌─────────────────────────────────────────────────┐
//!            │               BeadDispatchLoop::tick            │
//!            └─────────────────────────────────────────────────┘
//!   completion │                    │ dispatch
//!              ▼                    ▼
//!  ┌────────────────────┐   ┌─────────────────────────────────────┐
//!  │ CompletionProbe:   │   │ 1. next_ready_bead  (select)        │
//!  │ which workers have │   │ 2. assign_next      (claim +       │
//!  │ finished their bead│   │                      --bead-ref     │
//!  └────────────────────┘   │ 3. launch_bead      (cross-process  │
//!              │            │                      --if-revision  │
//!              ▼            │                      claim, spawn,   │
//!  record_completion        │                      mark in-progress)
//!  → BeadStatusBackend      └─────────────────────────────────────┘
//!  (closes the bead)
//! ```
//!
//! ## Cadence and triggers
//!
//! The loop self-gates on its configured cadence
//! (`bead_dispatch.interval_secs`): the first [`BeadDispatchLoop::tick`]
//! runs immediately, later ticks only when the interval has elapsed.
//! A slot replenished by the worker pool short-circuits the wait — call
//! [`BeadDispatchLoop::notify_worker_available`] (or feed pool events into
//! [`BeadDispatchLoop::observe_pool_events`]) and the next tick dispatches
//! at once. Disabled loops (`bead_dispatch.enabled = false`, the default)
//! never tick and never dispatch.
//!
//! ## Duplicate-assignment refusal
//!
//! A dispatch candidate can be refused after selection: the bead was
//! claimed in the bead store by another process (a second FORGE instance
//! or an external worker such as NEEDLE), or our guarded `--if-revision`
//! claim lost the race to one. Refusals surface as
//! [`ForgeError::BeadAlreadyAssigned`] and
//! [`ForgeError::BeadClaimConflict`]; the loop logs them, drops the
//! candidate, and **continues with the next ready bead** in the same tick,
//! so a contested bead cannot head-of-line-block the queue. A refused bead
//! stays skipped for `bead_dispatch.refused_retry_secs` (the winner will
//! usually finish or release it), then becomes eligible again.
//!
//! ## Completion reporting
//!
//! Finished workers are recorded through the scheduler's existing
//! [`BeadStatusBackend`] — the bead is closed, the mapping cleared, and the
//! freed slot refilled on the same tick. The loop learns completion from a
//! [`CompletionProbe`]; [`LauncherCompletionProbe`] is the production
//! implementation (a dispatched worker is finished when its session is
//! gone). Callers that learn completion through other channels can report
//! directly via [`BeadDispatchLoop::record_completion`].
//!
//! ## Usage
//!
//! ```no_run
//! use forge_config::BeadDispatchConfig;
//! use forge_worker::dispatch::BeadDispatchLoop;
//! use std::path::PathBuf;
//!
//! # async fn example() -> forge_core::Result<()> {
//! let mut config = BeadDispatchConfig::default();
//! config.enabled = true;
//! config.workspaces = vec![PathBuf::from("/path/to/repo")];
//!
//! // Production constructor: returns None while disabled.
//! if let Some(mut dispatch) = BeadDispatchLoop::from_config(&config)? {
//!     loop {
//!         // Drive from your own timer; the loop self-gates on the cadence.
//!         let events = dispatch.tick(chrono::Utc::now()).await?;
//!         for event in events {
//!             tracing::info!(?event, "bead dispatch");
//!         }
//!         # break;
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use crate::bead_queue::QueuedBead;
use crate::bead_scheduler::{BeadScheduler, CompletionRecord};
use crate::launcher::WorkerLauncher;
use crate::pool::PoolEvent;
use crate::types::LaunchConfig;
use chrono::{DateTime, Utc};
use forge_config::BeadDispatchConfig;
use forge_core::types::{BeadId, WorkerStatus};
use forge_core::{ForgeError, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Seconds as a chrono duration, saturating at `i64::MAX`.
fn chrono_secs(secs: u64) -> chrono::TimeDelta {
    chrono::Duration::seconds(secs.min(i64::MAX as u64) as i64)
}

/// Learns which dispatched workers have finished their bead.
///
/// The loop polls this during every tick and records completion for the
/// reported workers through the scheduler's status backend. Implementations
/// only report workers that are *done*; crash handling stays with the
/// crash-recovery machinery, not the dispatch loop.
pub trait CompletionProbe {
    /// The subset of `workers` that has finished.
    fn finished_workers(&self, workers: &[String]) -> impl Future<Output = Vec<String>> + Send;
}

impl<T: CompletionProbe + ?Sized> CompletionProbe for Arc<T> {
    fn finished_workers(&self, workers: &[String]) -> impl Future<Output = Vec<String>> + Send {
        (**self).finished_workers(workers)
    }
}

/// Production [`CompletionProbe`] backed by the worker launcher.
///
/// A dispatched worker counts as finished when its tmux session (or Docker
/// container) is gone — the launcher's own status check, so the probe needs
/// no protocol of its own. Workers that cannot be probed (never spawned by
/// this launcher, probe error) are reported as still running: an
/// inconclusive probe must not close a bead.
#[derive(Debug)]
pub struct LauncherCompletionProbe {
    launcher: Arc<WorkerLauncher>,
}

impl LauncherCompletionProbe {
    /// Probe through the given launcher — the same instance the scheduler
    /// spawns through, so it knows every dispatched worker.
    pub fn new(launcher: Arc<WorkerLauncher>) -> Self {
        Self { launcher }
    }
}

impl CompletionProbe for LauncherCompletionProbe {
    async fn finished_workers(&self, workers: &[String]) -> Vec<String> {
        let mut finished = Vec::new();
        for worker_id in workers {
            match self.launcher.check_status(worker_id).await {
                Ok(WorkerStatus::Stopped) => finished.push(worker_id.clone()),
                Ok(status) => debug!(
                    worker_id = %worker_id,
                    status = ?status,
                    "Dispatched worker still active"
                ),
                Err(e) => debug!(
                    worker_id = %worker_id,
                    error = %e,
                    "Dispatched worker cannot be probed; treating as active"
                ),
            }
        }
        finished
    }
}

/// One thing the loop did during a tick, for display and tests.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DispatchEvent {
    /// A worker was launched on a bead with `--bead-ref=<bead-id>`.
    Dispatched {
        /// Bead the worker was launched on.
        bead_id: String,
        /// Worker dispatched to work the bead.
        worker_id: String,
        /// tmux session name the worker was launched into.
        session_name: String,
    },
    /// A worker finished its bead; completion was recorded (the bead is
    /// closed through the status backend).
    Completed {
        /// Bead that was completed.
        bead_id: String,
        /// Worker that completed it.
        worker_id: String,
    },
    /// A dispatch candidate was refused — the bead is already assigned in
    /// the bead store, or a competing process won the `--if-revision`
    /// claim race — and was dropped without crashing the loop.
    Refused {
        /// Bead that was dropped.
        bead_id: String,
        /// Why the candidate was refused.
        reason: String,
    },
    /// A worker launch failed after the claim was taken; the claim and
    /// mapping were rolled back and the tick stopped dispatching.
    SpawnFailed {
        /// Bead whose launch failed.
        bead_id: String,
        /// Launch error.
        error: String,
    },
    /// A finished worker's completion could not be recorded; it is retried
    /// on the next tick.
    CompletionFailed {
        /// Worker whose completion was not recorded.
        worker_id: String,
        /// Status-backend error.
        error: String,
    },
}

/// Cumulative counters over the loop's lifetime.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DispatchStats {
    /// Ticks actually run (gated-out ticks are not counted).
    pub ticks: u64,
    /// Workers launched onto beads.
    pub dispatched: u64,
    /// Completions recorded (including direct
    /// [`BeadDispatchLoop::record_completion`] calls).
    pub completed: u64,
    /// Candidates dropped as duplicate assignments.
    pub refused: u64,
    /// Launch failures after a claim was taken.
    pub spawn_failures: u64,
}

/// Normalized loop settings derived from [`BeadDispatchConfig`].
#[derive(Debug, Clone)]
struct DispatchSettings {
    enabled: bool,
    interval_secs: u64,
    max_in_flight: usize,
    refused_retry_secs: u64,
    launcher: PathBuf,
    model: String,
    worker_id_prefix: String,
}

impl DispatchSettings {
    fn from_config(config: &BeadDispatchConfig) -> Self {
        let prefix = config.worker_id_prefix.trim();
        Self {
            enabled: config.enabled,
            interval_secs: config.interval_secs.max(1),
            max_in_flight: config
                .max_in_flight
                .clamp(1, BeadDispatchConfig::MAX_IN_FLIGHT),
            refused_retry_secs: config.refused_retry_secs,
            launcher: config.resolved_launcher(),
            model: config.resolved_model(),
            worker_id_prefix: if prefix.is_empty() {
                "dispatch".to_string()
            } else {
                prefix.to_string()
            },
        }
    }
}

/// Result of one dispatch attempt (internal to a tick).
enum DispatchOutcome {
    /// Launched; keep dispatching while capacity remains.
    Dispatched(DispatchEvent),
    /// Dropped as a duplicate assignment; try the next candidate.
    Refused(DispatchEvent),
    /// Launch failed; stop the tick's dispatching (retry next cadence).
    SpawnFailed(DispatchEvent),
    /// No allocatable candidate remains.
    Exhausted,
}

/// Control loop that dispatches ready beads onto workers.
///
/// See the [module docs](self) for the dispatch, refusal, and completion
/// model. The loop owns a [`BeadScheduler`] (and through it the
/// bead-queue readers and cross-process claim backend); `P` is how it
/// learns about worker completion.
pub struct BeadDispatchLoop<P: CompletionProbe> {
    settings: DispatchSettings,
    scheduler: BeadScheduler,
    probe: P,
    /// Beads refused because another process holds them, with the instant
    /// of the refusal; skipped until `refused_retry` elapses.
    refused: HashMap<BeadId, DateTime<Utc>>,
    /// Last tick actually run; `None` until the first tick.
    last_tick: Option<DateTime<Utc>>,
    /// Set by a pool-replenish event; makes the next tick due at once.
    worker_available: bool,
    /// Sequence number for dispatched worker ids.
    seq: u64,
    stats: DispatchStats,
}

/// Production constructor: build the loop (scheduler, launcher-based
/// completion probe, launch template) from user config.
///
/// Returns `None` while `bead_dispatch.enabled` is false — the
/// documented opt-in gate. The scheduler keeps its default backends:
/// status updates and cross-process claims go through the `bead` CLI
/// in each bead's workspace.
impl BeadDispatchLoop<LauncherCompletionProbe> {
    pub fn from_config(config: &BeadDispatchConfig) -> Result<Option<Self>> {
        if !config.enabled {
            debug!("Bead dispatch loop disabled (bead_dispatch.enabled = false)");
            return Ok(None);
        }

        let launcher = Arc::new(WorkerLauncher::new());
        let scheduler = BeadScheduler::new(Arc::clone(&launcher));
        let probe = LauncherCompletionProbe::new(launcher);
        Ok(Some(BeadDispatchLoop::new(scheduler, probe, config)?))
    }
}

impl<P: CompletionProbe> BeadDispatchLoop<P> {
    /// Build a loop from an already-prepared scheduler, a completion
    /// probe, and config.
    ///
    /// The workspaces listed in `config` are registered on `scheduler`
    /// here; pass a scheduler without pre-registered workspaces so the
    /// queue is not read twice.
    pub fn new(scheduler: BeadScheduler, probe: P, config: &BeadDispatchConfig) -> Result<Self> {
        let mut scheduler = scheduler;
        for workspace in &config.workspaces {
            scheduler.add_workspace(workspace)?;
        }
        if config.enabled && config.workspaces.is_empty() {
            warn!(
                "Bead dispatch loop is enabled but no workspaces are configured; it will never find beads"
            );
        }

        Ok(Self {
            settings: DispatchSettings::from_config(config),
            scheduler,
            probe,
            refused: HashMap::new(),
            last_tick: None,
            worker_available: false,
            seq: 0,
            stats: DispatchStats::default(),
        })
    }

    /// Whether the loop dispatches at all.
    pub fn is_enabled(&self) -> bool {
        self.settings.enabled
    }

    /// Cadence the loop self-gates on.
    pub fn interval(&self) -> Duration {
        Duration::from_secs(self.settings.interval_secs)
    }

    /// Maximum concurrently in-flight bead workers.
    pub fn max_in_flight(&self) -> usize {
        self.settings.max_in_flight
    }

    /// Currently in-flight assignments.
    pub fn in_flight(&self) -> usize {
        self.scheduler.assignments().count()
    }

    /// Cumulative counters.
    pub fn stats(&self) -> &DispatchStats {
        &self.stats
    }

    /// The scheduler driving the pipeline (for display and tests).
    pub fn scheduler(&self) -> &BeadScheduler {
        &self.scheduler
    }

    /// The scheduler driving the pipeline, mutably (for tests and
    /// backends: `with_status_backend`, `with_claim_backend`).
    pub fn scheduler_mut(&mut self) -> &mut BeadScheduler {
        &mut self.scheduler
    }

    /// Report a pool-replenish event: a worker slot was replenished, so
    /// the next tick dispatches without waiting for the cadence.
    pub fn notify_worker_available(&mut self) {
        self.worker_available = true;
    }

    /// Feed worker-pool reconcile events into the loop: provisioning
    /// events (`Spawned`, `Ready`, `Replaced`, `Restarted`) count as
    /// replenished capacity and make the next tick due at once. Teardowns
    /// and alerts do not.
    pub fn observe_pool_events(&mut self, events: &[PoolEvent]) {
        for event in events {
            if matches!(
                event,
                PoolEvent::Spawned { .. }
                    | PoolEvent::Ready { .. }
                    | PoolEvent::Replaced { .. }
                    | PoolEvent::Restarted { .. }
            ) {
                self.notify_worker_available();
                return;
            }
        }
    }

    /// Whether a tick at `now` would run: the cadence elapsed (or never
    /// started), a pool-replenish event is pending, or the loop has not
    /// been driven yet. Disabled loops are never due.
    pub fn due(&self, now: DateTime<Utc>) -> bool {
        if !self.settings.enabled {
            return false;
        }
        self.worker_available
            || self
                .last_tick
                .is_none_or(|last| now - last >= chrono_secs(self.settings.interval_secs))
    }

    /// Run one control-loop pass: record completions for finished workers,
    /// then dispatch ready beads while capacity remains.
    ///
    /// The pass only runs when [`BeadDispatchLoop::due`] holds — the loop
    /// self-gates on its cadence and on pending pool-replenish events.
    /// Errors from the queue readers propagate; dispatch refusals
    /// ([`ForgeError::BeadAlreadyAssigned`],
    /// [`ForgeError::BeadClaimConflict`]) do not: they are logged, the
    /// candidate is dropped, and the next candidate is dispatched.
    pub async fn tick(&mut self, now: DateTime<Utc>) -> Result<Vec<DispatchEvent>> {
        let mut events = Vec::new();
        if !self.due(now) {
            return Ok(events);
        }

        self.last_tick = Some(now);
        self.worker_available = false;
        self.stats.ticks += 1;

        // 1. Completion pass: workers whose bead is done report through the
        //    status backend, freeing their slots for step 3.
        events.extend(self.reap_finished().await);

        // 2. Refused beads become candidates again after their retry window.
        self.expire_refusals(now);

        // 3. Dispatch while capacity and candidates remain.
        while self.in_flight() < self.settings.max_in_flight {
            match self.dispatch_next(now).await? {
                DispatchOutcome::Dispatched(event) | DispatchOutcome::Refused(event) => {
                    events.push(event);
                }
                DispatchOutcome::Exhausted => break,
                // A broken launcher must not burn through the whole queue
                // in one tick; the claim was rolled back, so the next
                // cadence tick retries the head of the queue.
                DispatchOutcome::SpawnFailed(event) => {
                    events.push(event);
                    break;
                }
            }
        }

        Ok(events)
    }

    /// Record that a worker finished its bead.
    ///
    /// The manual completion path for callers that learn completion outside
    /// the tick probe: closes the bead through the scheduler's status
    /// backend, clears the mapping, and frees the dispatch slot.
    pub async fn record_completion(&mut self, worker_id: &str) -> Result<CompletionRecord> {
        let record = self.scheduler.record_completion(worker_id).await?;
        self.stats.completed += 1;
        info!(
            bead_id = %record.bead_id,
            worker_id = %record.worker_id,
            "Dispatch loop recorded bead completion"
        );
        Ok(record)
    }

    /// Release a bead from its worker without recording completion (the
    /// worker failed or was stopped): the bead is reopened so the loop can
    /// reallocate it.
    pub async fn release(&mut self, bead_id: impl Into<BeadId>) -> Result<()> {
        self.scheduler.release(bead_id).await?;
        Ok(())
    }

    // =========================================================================
    // Internals
    // =========================================================================

    /// Record completion for every worker the probe reports as finished.
    ///
    /// A failed close is logged and retried on the next tick — the
    /// scheduler clears the mapping only after the status update succeeds.
    async fn reap_finished(&mut self) -> Vec<DispatchEvent> {
        let mut events = Vec::new();
        let live: Vec<String> = self
            .scheduler
            .assignments()
            .map(|a| a.worker_id.clone())
            .collect();
        if live.is_empty() {
            return events;
        }

        for worker_id in self.probe.finished_workers(&live).await {
            match self.scheduler.record_completion(&worker_id).await {
                Ok(record) => {
                    self.stats.completed += 1;
                    info!(
                        bead_id = %record.bead_id,
                        worker_id = %record.worker_id,
                        "Dispatch loop recorded bead completion"
                    );
                    events.push(DispatchEvent::Completed {
                        bead_id: record.bead_id,
                        worker_id,
                    });
                }
                Err(e) => {
                    warn!(
                        worker_id = %worker_id,
                        error = %e,
                        "Failed to record bead completion; retrying next tick"
                    );
                    events.push(DispatchEvent::CompletionFailed {
                        worker_id,
                        error: e.to_string(),
                    });
                }
            }
        }
        events
    }

    /// Expire refusal entries whose retry window has elapsed.
    fn expire_refusals(&mut self, now: DateTime<Utc>) {
        let ttl = chrono_secs(self.settings.refused_retry_secs);
        self.refused.retain(|bead_id, refused_at| {
            let expired = now - *refused_at >= ttl;
            if expired {
                debug!(bead_id = %bead_id, "Dispatch refusal expired; bead is a candidate again");
            }
            !expired
        });
    }

    /// Select the next dispatch candidate: the highest-priority ready bead
    /// that is neither already assigned nor inside its refusal window.
    fn next_candidate(&mut self) -> Result<Option<QueuedBead>> {
        if self.refused.is_empty() {
            // Common case: the queue head is the candidate. This is also
            // the selection half of the `assign_next` claim below.
            return self.scheduler.next_ready_bead();
        }
        // Some beads are pending refusal retry: pick the best bead outside
        // that set so a contested head cannot block the queue.
        let refused = &self.refused;
        Ok(self
            .scheduler
            .ready_beads()?
            .into_iter()
            .find(|bead| !refused.contains_key(&bead.id)))
    }

    /// Launch one bead: select → claim → spawn with `--bead-ref`.
    async fn dispatch_next(&mut self, now: DateTime<Utc>) -> Result<DispatchOutcome> {
        let Some(bead) = self.next_candidate()? else {
            debug!("Bead dispatch queue exhausted");
            return Ok(DispatchOutcome::Exhausted);
        };
        let bead_id = bead.id.clone();
        let worker_id = self.fresh_worker_id(&bead_id);
        let config = self.launch_config_for(&bead, &worker_id);

        // Claim the candidate for the available worker: registers the
        // assignment (the in-process bead lock) and builds the spawn
        // request carrying `--bead-ref=<bead-id>`. `assign_next` performs
        // the selection itself; once candidates have been refused this
        // session, its specific-bead form is used instead so the refused
        // head of the queue is not simply re-picked.
        let request = if self.refused.is_empty() {
            self.scheduler.assign_next(&worker_id, config)?
        } else {
            self.scheduler
                .assign_bead(&bead_id, &worker_id, config)
                .map(Some)?
        };
        let Some(request) = request else {
            return Ok(DispatchOutcome::Exhausted);
        };

        // Full launch pipeline: cross-process `--if-revision` claim →
        // spawn with `--bead-ref=<bead-id>` → mark in-progress.
        match self
            .scheduler
            .launch_bead(&bead_id, &worker_id, request.config)
            .await
        {
            Ok(handle) => {
                self.stats.dispatched += 1;
                info!(
                    bead_id = %bead_id,
                    worker_id = %handle.id,
                    session_name = %handle.session_name,
                    "Dispatched worker onto bead"
                );
                Ok(DispatchOutcome::Dispatched(DispatchEvent::Dispatched {
                    bead_id,
                    worker_id: handle.id,
                    session_name: handle.session_name,
                }))
            }
            Err(ForgeError::BeadAlreadyAssigned {
                bead_id,
                worker_id: holder,
            }) => {
                // Held by another worker — in the store, or (rarely) claimed
                // between our selection and our assignment by a competing
                // process. Drop and log; the loop moves on.
                Ok(self.refuse_candidate(bead_id, format!("already assigned to {holder}"), now))
            }
            Err(ForgeError::BeadClaimConflict { bead_id, detail }) => {
                // A competing process won the guarded `--if-revision` claim.
                Ok(self.refuse_candidate(bead_id, format!("claim race lost: {detail}"), now))
            }
            Err(e) => {
                self.stats.spawn_failures += 1;
                warn!(
                    bead_id = %bead_id,
                    worker_id = %worker_id,
                    error = %e,
                    "Bead dispatch launch failed; claim rolled back"
                );
                Ok(DispatchOutcome::SpawnFailed(DispatchEvent::SpawnFailed {
                    bead_id,
                    error: e.to_string(),
                }))
            }
        }
    }

    /// Record a refusal: log it, skip the bead until its retry window
    /// elapses, and keep the loop going.
    fn refuse_candidate(
        &mut self,
        bead_id: String,
        reason: String,
        now: DateTime<Utc>,
    ) -> DispatchOutcome {
        self.refused.insert(bead_id.clone(), now);
        self.stats.refused += 1;
        warn!(
            bead_id = %bead_id,
            reason = %reason,
            "Dropping dispatch candidate: bead already assigned elsewhere"
        );
        DispatchOutcome::Refused(DispatchEvent::Refused { bead_id, reason })
    }

    /// Launch configuration for a bead: the loop's launcher script and
    /// model, running in the bead's own workspace.
    fn launch_config_for(&self, bead: &QueuedBead, session_name: &str) -> LaunchConfig {
        LaunchConfig::new(
            &self.settings.launcher,
            session_name,
            &bead.workspace,
            &self.settings.model,
        )
    }

    /// Next pool-unique dispatched worker id (also used as the session
    /// name; the launcher applies its `forge-` prefix).
    fn fresh_worker_id(&mut self, bead_id: &str) -> String {
        let id = format!(
            "{}-{}-{}",
            self.settings.worker_id_prefix, self.seq, bead_id
        );
        self.seq += 1;
        id
    }
}

impl<P: CompletionProbe> std::fmt::Debug for BeadDispatchLoop<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BeadDispatchLoop")
            .field("settings", &self.settings)
            .field("scheduler", &self.scheduler)
            .field("refused", &self.refused)
            .field("last_tick", &self.last_tick)
            .field("worker_available", &self.worker_available)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bead_claim::{BeadClaimBackend, MemoryClaimStore, StoredClaim};
    use crate::bead_scheduler::{BeadStatusAction, BeadStatusBackend, SpawnFn};
    use crate::types::{SpawnRequest, WorkerHandle};
    use chrono::TimeZone;
    use std::fs;
    use std::io::Write;
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// Deterministic time base: 2026-09-16T00:00:00Z.
    fn base_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 16, 0, 0, 0).unwrap()
    }

    fn plus_secs(base: DateTime<Utc>, secs: i64) -> DateTime<Utc> {
        base + chrono::Duration::seconds(secs)
    }

    /// Queue fixture: two ready beads (P0 bug ahead of P3 task) plus a
    /// bead blocked by the P0 one that must never be dispatched.
    fn create_queue_workspace() -> TempDir {
        let dir = TempDir::new().unwrap();
        let beads_dir = dir.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();

        let mut file = fs::File::create(beads_dir.join("issues.jsonl")).unwrap();
        writeln!(file, r#"{{"id":"p-high","title":"High priority bug","description":"Fix it now","status":"open","priority":0,"issue_type":"bug","labels":["urgent"],"dependencies":[]}}"#).unwrap();
        writeln!(file, r#"{{"id":"p-low","title":"Low priority task","description":"Do it later","status":"open","priority":3,"issue_type":"task","labels":[],"dependencies":[]}}"#).unwrap();
        writeln!(file, r#"{{"id":"p-blocked","title":"Blocked task","description":"Waiting","status":"open","priority":0,"issue_type":"task","labels":[],"dependencies":["p-high"]}}"#).unwrap();

        dir
    }

    /// Records every spawn request the pipeline builds and returns a
    /// healthy handle, standing in for the tmux launcher runtime.
    #[derive(Default)]
    struct SpawnRecorder {
        requests: Mutex<Vec<SpawnRequest>>,
    }

    impl SpawnRecorder {
        fn recorded(&self) -> Vec<SpawnRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    fn recording_spawner(recorder: Arc<SpawnRecorder>) -> Arc<SpawnFn> {
        Arc::new(move |request| {
            recorder.requests.lock().unwrap().push(request.clone());
            let handle = WorkerHandle::new(
                request.worker_id.clone(),
                4242,
                format!("forge-{}", request.config.session_name),
                request.config.launcher_path.clone(),
                request.config.model.clone(),
                request.config.tier,
                request.config.workspace.clone(),
            );
            Box::pin(async move { Ok(handle) })
        })
    }

    /// Every launch fails, simulating a broken launcher runtime.
    fn failing_spawner() -> Arc<SpawnFn> {
        Arc::new(|request| {
            Box::pin(async move {
                Err(ForgeError::WorkerSpawn {
                    worker_id: request.worker_id.clone(),
                    message: "injected launcher failure".to_string(),
                })
            })
        })
    }

    /// Completion probe with a scripted set of finished workers.
    #[derive(Default)]
    struct CannedProbe {
        finished: Mutex<Vec<String>>,
    }

    impl CannedProbe {
        fn finish(&self, worker_id: &str) {
            self.finished.lock().unwrap().push(worker_id.to_string());
        }
    }

    impl CompletionProbe for CannedProbe {
        async fn finished_workers(&self, workers: &[String]) -> Vec<String> {
            let finished = self.finished.lock().unwrap().clone();
            workers
                .iter()
                .filter(|w| finished.contains(*w))
                .cloned()
                .collect()
        }
    }

    fn dispatch_config(max_in_flight: usize) -> BeadDispatchConfig {
        BeadDispatchConfig {
            enabled: true,
            interval_secs: 30,
            max_in_flight,
            workspaces: Vec::new(), // registered on the scheduler by BeadDispatchLoop::new
            launcher: None,
            model: None,
            refused_retry_secs: 300,
            worker_id_prefix: "dispatch".to_string(),
        }
    }

    fn test_loop(
        dir: &TempDir,
        store: MemoryClaimStore,
        spawner: Arc<SpawnFn>,
        probe: Arc<CannedProbe>,
        max_in_flight: usize,
    ) -> BeadDispatchLoop<Arc<CannedProbe>> {
        test_loop_with_prefix(dir, store, spawner, probe, max_in_flight, "dispatch")
    }

    /// [`test_loop`] with a distinct worker-id prefix. Two loops sharing a
    /// store must never generate the same worker id, or one would treat
    /// the other's claim as its own — the exact collision the wiring test
    /// exists to catch.
    fn test_loop_with_prefix(
        dir: &TempDir,
        store: MemoryClaimStore,
        spawner: Arc<SpawnFn>,
        probe: Arc<CannedProbe>,
        max_in_flight: usize,
        prefix: &str,
    ) -> BeadDispatchLoop<Arc<CannedProbe>> {
        let mut scheduler = BeadScheduler::with_spawner(spawner)
            .with_status_backend(BeadStatusBackend::DryRun)
            .with_claim_backend(BeadClaimBackend::Memory(store));
        scheduler.add_workspace(dir.path()).unwrap();

        let mut config = dispatch_config(max_in_flight);
        config.worker_id_prefix = prefix.to_string();
        BeadDispatchLoop::new(scheduler, probe, &config).unwrap()
    }

    // ------------------------------------------------------------
    // Opt-in gate and config normalization
    // ------------------------------------------------------------

    #[test]
    fn test_from_config_disabled_returns_none() {
        assert!(
            BeadDispatchLoop::from_config(&BeadDispatchConfig::default())
                .unwrap()
                .is_none(),
            "the loop is opt-in and must not build while disabled"
        );
    }

    #[test]
    fn test_from_config_enabled_builds_loop_with_workspaces() {
        let dir = create_queue_workspace();
        let config = BeadDispatchConfig {
            enabled: true,
            max_in_flight: 3,
            workspaces: vec![dir.path().to_path_buf()],
            ..BeadDispatchConfig::default()
        };

        let dispatch = BeadDispatchLoop::from_config(&config).unwrap().unwrap();
        assert!(dispatch.is_enabled());
        assert_eq!(dispatch.max_in_flight(), 3);
        assert_eq!(dispatch.scheduler().workspace_count(), 1);
        assert_eq!(dispatch.in_flight(), 0);
        // A never-ticked enabled loop runs its first tick immediately.
        assert!(dispatch.due(base_time()));
    }

    #[test]
    fn test_settings_normalize_degenerate_config() {
        let config = BeadDispatchConfig {
            enabled: true,
            interval_secs: 0,
            max_in_flight: BeadDispatchConfig::MAX_IN_FLIGHT + 999,
            ..BeadDispatchConfig::default()
        };

        let dispatch = BeadDispatchLoop::from_config(&config).unwrap().unwrap();
        assert_eq!(dispatch.interval(), Duration::from_secs(1));
        assert_eq!(dispatch.max_in_flight(), BeadDispatchConfig::MAX_IN_FLIGHT);
    }

    // ------------------------------------------------------------
    // Dispatch pipeline: select → claim → launch with --bead-ref
    // ------------------------------------------------------------

    #[tokio::test]
    async fn test_enabled_loop_dispatches_ready_bead_with_bead_ref() {
        let dir = create_queue_workspace();
        let recorder = Arc::new(SpawnRecorder::default());
        let store = MemoryClaimStore::new();
        let mut dispatch = test_loop(
            &dir,
            store.clone(),
            recording_spawner(Arc::clone(&recorder)),
            Arc::new(CannedProbe::default()),
            2,
        );

        let events = dispatch.tick(base_time()).await.unwrap();

        // Both ready beads dispatched, highest priority first; the blocked
        // bead is never a candidate.
        assert_eq!(
            events,
            vec![
                DispatchEvent::Dispatched {
                    bead_id: "p-high".to_string(),
                    worker_id: "dispatch-0-p-high".to_string(),
                    session_name: "forge-dispatch-0-p-high".to_string(),
                },
                DispatchEvent::Dispatched {
                    bead_id: "p-low".to_string(),
                    worker_id: "dispatch-1-p-low".to_string(),
                    session_name: "forge-dispatch-1-p-low".to_string(),
                },
            ]
        );

        // The spawn request carried the bead assignment, which the launcher
        // forwards to the launcher script as --bead-ref=<bead-id>, and ran
        // in the bead's own workspace.
        let requests = recorder.recorded();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].config.bead_id.as_deref(), Some("p-high"));
        assert!(requests[0].config.has_bead());
        assert_eq!(requests[0].config.session_name, "dispatch-0-p-high");
        assert_eq!(requests[0].config.workspace, dir.path());

        // The claim was taken in the bead store before the spawn.
        let claim = store.read(&"p-high".to_string());
        assert_eq!(claim.assignee.as_deref(), Some("dispatch-0-p-high"));
        assert_eq!(claim.status, "in_progress");

        assert_eq!(dispatch.stats().dispatched, 2);
        assert_eq!(dispatch.stats().ticks, 1);
        assert_eq!(dispatch.in_flight(), 2);
    }

    #[tokio::test]
    async fn test_disabled_loop_never_dispatches() {
        let recorder = Arc::new(SpawnRecorder::default());
        let mut config = dispatch_config(1);
        config.enabled = false;

        let scheduler = BeadScheduler::with_spawner(recording_spawner(Arc::clone(&recorder)));
        let mut dispatch =
            BeadDispatchLoop::new(scheduler, Arc::<CannedProbe>::default(), &config).unwrap();

        assert!(!dispatch.is_enabled());
        assert!(!dispatch.due(base_time()));

        let events = dispatch.tick(base_time()).await.unwrap();
        assert!(events.is_empty());
        assert!(recorder.recorded().is_empty());
        assert_eq!(dispatch.stats().ticks, 0);
    }

    // ------------------------------------------------------------
    // Cadence, capacity, and pool-replenish triggering
    // ------------------------------------------------------------

    #[tokio::test]
    async fn test_capacity_gate_and_completion_frees_slot() {
        let dir = create_queue_workspace();
        let probe = Arc::new(CannedProbe::default());
        let mut dispatch = test_loop(
            &dir,
            MemoryClaimStore::new(),
            recording_spawner(Arc::new(SpawnRecorder::default())),
            Arc::clone(&probe),
            1,
        );

        let t = base_time();
        let events = dispatch.tick(t).await.unwrap();
        assert_eq!(events.len(), 1, "max_in_flight = 1 caps dispatch");
        assert_eq!(
            events[0],
            DispatchEvent::Dispatched {
                bead_id: "p-high".to_string(),
                worker_id: "dispatch-0-p-high".to_string(),
                session_name: "forge-dispatch-0-p-high".to_string(),
            }
        );
        let dispatched_worker = "dispatch-0-p-high".to_string();

        // Within the cadence window: no tick runs, no new dispatch.
        let events = dispatch.tick(plus_secs(t, 10)).await.unwrap();
        assert!(events.is_empty());

        // The worker finishes; the slot frees and the queue advances on
        // the same tick.
        probe.finish(&dispatched_worker);
        let events = dispatch.tick(plus_secs(t, 31)).await.unwrap();
        assert_eq!(
            events,
            vec![
                DispatchEvent::Completed {
                    bead_id: "p-high".to_string(),
                    worker_id: dispatched_worker.clone(),
                },
                DispatchEvent::Dispatched {
                    bead_id: "p-low".to_string(),
                    worker_id: "dispatch-1-p-low".to_string(),
                    session_name: "forge-dispatch-1-p-low".to_string(),
                },
            ]
        );

        // Completion was reported through the scheduler's status backend:
        // the bead is closed "Completed by <worker>". The launch claimed
        // the bead in the store (which doubles as mark-in-progress), so
        // the close is the only recorded transition.
        let updates = dispatch.scheduler().status_updates();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].action, BeadStatusAction::Close);
        assert_eq!(updates[0].bead_id, "p-high");
        assert_eq!(updates[0].detail, "Completed by dispatch-0-p-high");

        assert_eq!(dispatch.stats().completed, 1);
        assert_eq!(dispatch.stats().dispatched, 2);
    }

    #[tokio::test]
    async fn test_worker_available_notification_makes_loop_due_within_window() {
        let dir = create_queue_workspace();
        let probe = Arc::new(CannedProbe::default());
        let mut dispatch = test_loop(
            &dir,
            MemoryClaimStore::new(),
            recording_spawner(Arc::new(SpawnRecorder::default())),
            Arc::clone(&probe),
            1,
        );

        let t = base_time();
        dispatch.tick(t).await.unwrap();
        assert!(!dispatch.due(plus_secs(t, 10)), "cadence window holds");

        // A pool-replenish event short-circuits the wait; the next tick
        // reaps the finished worker and dispatches the next bead at once.
        dispatch.notify_worker_available();
        assert!(dispatch.due(plus_secs(t, 10)));

        probe.finish("dispatch-0-p-high");
        let events = dispatch.tick(plus_secs(t, 10)).await.unwrap();
        assert_eq!(
            events,
            vec![
                DispatchEvent::Completed {
                    bead_id: "p-high".to_string(),
                    worker_id: "dispatch-0-p-high".to_string(),
                },
                DispatchEvent::Dispatched {
                    bead_id: "p-low".to_string(),
                    worker_id: "dispatch-1-p-low".to_string(),
                    session_name: "forge-dispatch-1-p-low".to_string(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn test_only_replenishing_pool_events_trigger_dispatch() {
        let dir = create_queue_workspace();
        let mut dispatch = test_loop(
            &dir,
            MemoryClaimStore::new(),
            recording_spawner(Arc::new(SpawnRecorder::default())),
            Arc::new(CannedProbe::default()),
            1,
        );

        dispatch.tick(base_time()).await.unwrap();
        let within = plus_secs(base_time(), 5);
        assert!(!dispatch.due(within));

        // Teardowns and alerts do not replenish capacity.
        dispatch.observe_pool_events(&[
            PoolEvent::TeardownIdle {
                tier: "standard".to_string(),
                worker_id: "pool-standard-1".to_string(),
            },
            PoolEvent::Alerted {
                tier: "standard".to_string(),
                worker_id: None,
                message: "pooled worker failed".to_string(),
            },
        ]);
        assert!(!dispatch.due(within));

        // A replacement slot does.
        dispatch.observe_pool_events(&[PoolEvent::Replaced {
            tier: "standard".to_string(),
            retired_id: "pool-standard-1".to_string(),
            new_id: "pool-standard-2".to_string(),
        }]);
        assert!(dispatch.due(within));
    }

    // ------------------------------------------------------------
    // Duplicate-assignment refusal (the wiring-level contract)
    // ------------------------------------------------------------

    /// Wiring-level duplicate-assignment refusal: two dispatch loops over
    /// one shared bead store, exactly as two FORGE instances (or FORGE and
    /// an external worker such as NEEDLE) would race. The loser must log
    /// the refusal, drop the candidate, and continue with the next ready
    /// bead in the same tick — not crash, error out, or stall the queue.
    #[tokio::test]
    async fn test_duplicate_refusal_drops_candidate_and_loop_continues() {
        let dir = create_queue_workspace();
        let store = MemoryClaimStore::new();

        // Loop A (the competing process) claims the head of the queue.
        let mut loop_a = test_loop_with_prefix(
            &dir,
            store.clone(),
            recording_spawner(Arc::new(SpawnRecorder::default())),
            Arc::new(CannedProbe::default()),
            1,
            "dispatch-a",
        );
        let events = loop_a.tick(base_time()).await.unwrap();
        let DispatchEvent::Dispatched {
            bead_id: ref winner_bead,
            worker_id: ref holder,
            ..
        } = events[0]
        else {
            panic!("expected loop A to dispatch, got {:?}", events);
        };
        assert_eq!(winner_bead, "p-high");

        // Loop B shares the queue and the store but not the scheduler —
        // its own mapping is free, so the conflict can only come from the
        // store claim path.
        let mut loop_b = test_loop_with_prefix(
            &dir,
            store.clone(),
            recording_spawner(Arc::new(SpawnRecorder::default())),
            Arc::new(CannedProbe::default()),
            2,
            "dispatch-b",
        );

        // B's tick succeeds: the contested head is dropped-and-logged and
        // the next candidate is dispatched to the available worker.
        let events = loop_b.tick(base_time()).await.unwrap();
        assert_eq!(
            events,
            vec![
                DispatchEvent::Refused {
                    bead_id: "p-high".to_string(),
                    reason: format!("already assigned to {holder}"),
                },
                DispatchEvent::Dispatched {
                    // The refused attempt consumed the first id.
                    bead_id: "p-low".to_string(),
                    worker_id: "dispatch-b-1-p-low".to_string(),
                    session_name: "forge-dispatch-b-1-p-low".to_string(),
                },
            ]
        );

        // B's mapping was rolled back for the refused bead; it holds only
        // p-low, and the winner's claim is untouched in the store.
        assert_eq!(loop_b.scheduler().assignments().count(), 1);
        let assignment = loop_b
            .scheduler()
            .assignment_for_bead(&"p-low".to_string())
            .expect("p-low should be assigned to loop B");
        assert_eq!(assignment.worker_id, "dispatch-b-1-p-low");
        assert!(
            loop_b
                .scheduler()
                .assignment_for_bead(&"p-high".to_string())
                .is_none(),
            "the refused bead must not stay locked in the loser's mapping"
        );

        let claim = store.read(&"p-high".to_string());
        assert_eq!(claim.assignee.as_deref(), Some(holder.as_str()));
        assert_eq!(claim.revision, 1, "a foreign claim must be left untouched");

        assert_eq!(loop_b.stats().refused, 1);
        assert_eq!(loop_b.stats().dispatched, 1);

        // Subsequent dispatch keeps flowing around the refused head.
        assert_eq!(loop_b.in_flight(), 1);
        assert!(loop_b.due(plus_secs(base_time(), 30)));
    }

    /// A competing process winning the guarded `--if-revision` claim race
    /// is the same dropped-and-logged contract: refuse, continue, and
    /// leave the winner's store state untouched.
    #[tokio::test]
    async fn test_lost_if_revision_race_is_refused_and_loop_continues() {
        let dir = create_queue_workspace();
        let store = MemoryClaimStore::new();
        // A competing process lands between our read and our guarded write.
        store.simulate_lost_race("p-high");
        let mut dispatch = test_loop(
            &dir,
            store.clone(),
            recording_spawner(Arc::new(SpawnRecorder::default())),
            Arc::new(CannedProbe::default()),
            2,
        );

        let events = dispatch.tick(base_time()).await.unwrap();
        assert_eq!(events.len(), 2);
        match &events[0] {
            DispatchEvent::Refused { bead_id, reason } => {
                assert_eq!(bead_id, "p-high");
                assert!(reason.contains("claim race lost"), "reason: {reason}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert_eq!(
            events[1],
            DispatchEvent::Dispatched {
                bead_id: "p-low".to_string(),
                worker_id: "dispatch-1-p-low".to_string(),
                session_name: "forge-dispatch-1-p-low".to_string(),
            }
        );

        // The loser never wrote the store.
        let claim = store.read(&"p-high".to_string());
        assert_eq!(claim.assignee, None);
        assert_eq!(claim.revision, 0);
        assert_eq!(dispatch.stats().refused, 1);
    }

    /// A refused bead stays skipped for its retry window — so the queue
    /// keeps flowing — and becomes a candidate again after it, so a bead
    /// the winner released is not lost to the loop forever.
    #[tokio::test]
    async fn test_refusal_expires_and_released_bead_redispatches() {
        let dir = create_queue_workspace();
        let store = MemoryClaimStore::new();
        // The winner holds p-high in the store.
        store.seed(
            "p-high",
            StoredClaim {
                assignee: Some("external-worker".to_string()),
                status: "in_progress".to_string(),
                revision: 3,
                claim_epoch: None,
            },
        );
        let backend = BeadClaimBackend::Memory(store.clone());
        let mut dispatch = test_loop(
            &dir,
            store.clone(),
            recording_spawner(Arc::new(SpawnRecorder::default())),
            Arc::new(CannedProbe::default()),
            2,
        );

        // First tick: p-high refused (held by the external worker), p-low
        // dispatched around it.
        let events = dispatch.tick(base_time()).await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0],
            DispatchEvent::Refused {
                bead_id: "p-high".to_string(),
                reason: "already assigned to external-worker".to_string(),
            }
        );

        // The winner releases the bead mid-window; the refusal still holds.
        // (+150 is past the 30s cadence but inside the 300s retry window.)
        assert!(
            backend
                .release_claim(dir.path(), &"p-high".to_string(), "external-worker")
                .await
                .unwrap()
        );
        let events = dispatch.tick(plus_secs(base_time(), 150)).await.unwrap();
        assert!(events.is_empty(), "cadence ran; refusal still holds");

        // After the retry window the bead is a candidate again — and now
        // that the store no longer holds it, it dispatches.
        let events = dispatch.tick(plus_secs(base_time(), 301)).await.unwrap();
        assert_eq!(
            events,
            vec![DispatchEvent::Dispatched {
                bead_id: "p-high".to_string(),
                worker_id: "dispatch-2-p-high".to_string(),
                session_name: "forge-dispatch-2-p-high".to_string(),
            }]
        );
    }

    // ------------------------------------------------------------
    // Launch failure, manual completion, and release
    // ------------------------------------------------------------

    #[tokio::test]
    async fn test_spawn_failure_stops_tick_but_loop_survives() {
        let dir = create_queue_workspace();
        let store = MemoryClaimStore::new();
        let mut dispatch = test_loop(
            &dir,
            store.clone(),
            failing_spawner(),
            Arc::new(CannedProbe::default()),
            2,
        );

        let events = dispatch.tick(base_time()).await.unwrap();
        assert_eq!(
            events,
            vec![DispatchEvent::SpawnFailed {
                bead_id: "p-high".to_string(),
                error: "Failed to spawn worker dispatch-0-p-high: injected launcher failure"
                    .to_string(),
            }]
        );

        // The claim and mapping were rolled back; nothing is in flight.
        assert_eq!(dispatch.in_flight(), 0);
        let claim = store.read(&"p-high".to_string());
        assert_eq!(claim.assignee, None, "claim released after failed launch");
        assert_eq!(claim.revision, 2, "acquire then release both bumped it");

        // The loop is alive: the next cadence tick retries the queue head.
        let events = dispatch.tick(plus_secs(base_time(), 30)).await.unwrap();
        assert_eq!(
            events,
            vec![DispatchEvent::SpawnFailed {
                bead_id: "p-high".to_string(),
                error: "Failed to spawn worker dispatch-1-p-high: injected launcher failure"
                    .to_string(),
            }]
        );
        assert_eq!(dispatch.stats().spawn_failures, 2);
    }

    #[tokio::test]
    async fn test_manual_completion_report_through_status_backend() {
        let dir = create_queue_workspace();
        let mut dispatch = test_loop(
            &dir,
            MemoryClaimStore::new(),
            recording_spawner(Arc::new(SpawnRecorder::default())),
            Arc::new(CannedProbe::default()),
            2,
        );

        dispatch.tick(base_time()).await.unwrap();

        let record = dispatch
            .record_completion("dispatch-0-p-high")
            .await
            .expect("completion should be recorded");
        assert_eq!(record.bead_id, "p-high");
        assert_eq!(record.worker_id, "dispatch-0-p-high");

        // Reported through the scheduler's existing status backend
        // (DryRun here: recorded, not executed).
        let updates = dispatch.scheduler().status_updates();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].action, BeadStatusAction::Close);
        assert_eq!(updates[0].bead_id, "p-high");
        assert!(!updates[0].applied);
        assert_eq!(dispatch.scheduler().completions().len(), 1);
        assert_eq!(dispatch.stats().completed, 1);
        assert_eq!(dispatch.in_flight(), 1);

        // Release path: the remaining assignment reopens for reallocation.
        dispatch.release("p-low").await.unwrap();
        assert!(!dispatch.scheduler().is_assigned(&"p-low".to_string()));
        assert_eq!(dispatch.in_flight(), 0);
    }
}
