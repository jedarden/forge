//! Worker pool with warm spares, automatic failover, and configurable recovery.
//!
//! This module implements the pool abstraction promised by the README:
//! it maintains a configurable number of ready ("warm spare") workers per
//! model tier, reacts when a member is detected dead or unhealthy, and tears
//! down idle spares. Pool size and the recovery policy live in
//! `~/.forge/config.yaml` under `worker_pool` (see
//! [`forge_config::WorkerPoolConfig`]).
//!
//! ## How failover works
//!
//! The pool keeps N workers warm per tier, so a consumer that needs a worker
//! of that tier never waits for a cold spawn: [`WorkerPool::take_ready`]
//! hands out an already-running spare and the next reconcile refills the
//! slot. When a member is detected dead or unhealthy — either by the pool's
//! own liveness probe or by an external report from the health monitor
//! ([`WorkerPool::report_unhealthy`]) — the configured recovery policy
//! decides what happens:
//!
//! - **`restart`**: the worker is respawned in place (same worker id), at
//!   most `max_retries` times per pooled worker, with exponential backoff
//!   between attempts. A worker that exhausts its budget is retired (held in
//!   the terminal failed state and alerted on) rather than respawned forever
//!   — the same "user must manually intervene" philosophy as crash
//!   recovery's rate limiter.
//! - **`replace`**: the dead worker is torn down and a *fresh* worker (new
//!   id, fresh retry budget) takes the slot. Each replacement is a new
//!   pooled worker; only consecutive spawn failures are bounded by
//!   `max_retries`.
//! - **`alert`**: no automatic action. The member is marked failed and an
//!   alert event is emitted; the tier runs short until the user intervenes.
//!
//! Automation is opt-in: `enabled` defaults to `false` and the policy to
//! `alert`, consistent with ADR 0014 (visibility first).
//!
//! ## Capacity accounting
//!
//! Every tracked member holds a slot, including recovering and failed ones.
//! A tier that is short because capacity failed stays short and alerts
//! instead of churning replacements forever; refilling only happens for
//! slots handed out via [`WorkerPool::take_ready`] or added by raising a
//! tier's size.
//!
//! ## Idle teardown
//!
//! When a tier holds more ready workers than its configured size (for
//! example after a config hot-reload shrinks the tier via
//! [`WorkerPool::set_tier_size`]), ready workers idle longer than
//! `idle_timeout_secs` — oldest first — are torn down until the tier is back
//! at size. Setting `idle_timeout_secs` to 0 disables teardown.
//!
//! ## Usage
//!
//! ```no_run
//! use forge_worker::pool::{LauncherPoolSpawner, WorkerPool};
//! use forge_config::WorkerPoolConfig;
//!
//! # async fn example() -> forge_core::Result<()> {
//! let mut config = WorkerPoolConfig::default();
//! config.enabled = true;
//! config.recovery_policy = "replace".to_string();
//!
//! let mut pool = WorkerPool::new(config, LauncherPoolSpawner::new());
//!
//! // Tick periodically (cadence: config.reconcile_interval_secs).
//! let events = pool.reconcile(chrono::Utc::now()).await?;
//! for event in events {
//!     tracing::info!(?event, "worker pool");
//! }
//!
//! // Hand a warm spare to a consumer; the slot is refilled on the next tick.
//! if let Some(handle) = pool.take_ready("standard") {
//!     println!("acquired pooled worker {}", handle.id);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Reconcile is deterministic: it takes the current time as a parameter, so
//! backoff and idle-timeout behavior can be exercised without sleeping.

use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::{debug, info, warn};

use forge_config::{ResolvedTierConfig, WorkerPoolConfig};
use forge_core::Result;
use forge_core::types::{WorkerStatus, WorkerTier};

use crate::launcher::WorkerLauncher;
use crate::types::{LaunchConfig, SpawnRequest, WorkerHandle};

/// Seconds as a chrono duration, saturating at `i64::MAX`.
fn chrono_secs(secs: u64) -> chrono::TimeDelta {
    chrono::Duration::seconds(secs.min(i64::MAX as u64) as i64)
}

/// Recovery policy applied when a pooled worker is detected dead or unhealthy.
///
/// Parsed from `worker_pool.recovery_policy`; unrecognized values parse as
/// [`PoolRecoveryPolicy::Alert`] (the config validator rejects them anyway,
/// and visibility-first is the safe fallback).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolRecoveryPolicy {
    /// Respawn the worker in place (same id), bounded by `max_retries`.
    Restart,
    /// Tear down the dead worker and give the slot a fresh worker.
    Replace,
    /// Take no automatic action; raise an alert only.
    #[default]
    Alert,
}

impl PoolRecoveryPolicy {
    /// Parse a policy from its config string
    /// (`"restart"`, `"replace"`, or `"alert"`).
    pub fn parse(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "restart" => Self::Restart,
            "replace" => Self::Replace,
            _ => Self::Alert,
        }
    }
}

/// Lifecycle state of a pooled worker.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolWorkerState {
    /// Spawned but not yet proven alive by a probe.
    #[default]
    Starting,
    /// Proven alive; available for [`WorkerPool::take_ready`].
    Ready,
    /// Dead or unhealthy; waiting for backoff to elapse before the next
    /// recovery spawn. Holds its slot so the tier is not over-filled.
    Recovering,
    /// Terminal: recovery budget exhausted or alert-only failure. Holds its
    /// slot (so the pool does not churn replacements forever) until the user
    /// intervenes or the pool is shut down.
    Failed,
}

impl PoolWorkerState {
    /// Live states own a probeable session.
    fn is_live(self) -> bool {
        matches!(self, Self::Starting | Self::Ready)
    }
}

/// Outcome of probing a pooled worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// The worker's process/session is alive.
    Healthy,
    /// The worker is gone, with a human-readable reason.
    Dead(String),
}

/// One worker tracked by the pool.
#[derive(Debug, Clone)]
pub struct PoolWorker {
    /// Pool-unique worker id (`pool-<tier>-<n>`).
    pub id: String,
    /// Tier name this worker belongs to.
    pub tier: String,
    /// Lifecycle state.
    pub state: PoolWorkerState,
    /// Handle to the live worker, if it has a running session. Always `None`
    /// in [`PoolWorkerState::Recovering`] (the corpse is stopped before
    /// backoff) and [`PoolWorkerState::Failed`] after exhaustion.
    pub handle: Option<WorkerHandle>,
    /// Spawn attempts made for this slot since its last successful
    /// provisioning: respawns after death (restart policy) and retries after
    /// failed spawns. `max_retries` bounds this counter; a fresh replacement
    /// slot starts over.
    pub recovery_attempts: u32,
    /// Id of the dead worker this slot is replacing (replace policy only);
    /// cleared once the replacement spawn succeeds.
    pub replacing: Option<String>,
    /// When the worker became ready (idle-teardown clock).
    pub ready_since: Option<DateTime<Utc>>,
    /// When the pool last probed the worker.
    pub last_probe: Option<DateTime<Utc>>,
    /// Earliest time the next recovery spawn may run.
    pub next_action_at: Option<DateTime<Utc>>,
    /// Most recent failure reason, for display.
    pub last_error: Option<String>,
}

/// Audit event emitted by a reconcile pass.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PoolEvent {
    /// A slot was provisioned with a new worker.
    Spawned { tier: String, worker_id: String },
    /// A starting worker passed its liveness probe.
    Ready { tier: String, worker_id: String },
    /// A dead worker was respawned in place (restart policy).
    Restarted {
        tier: String,
        worker_id: String,
        attempt: u32,
    },
    /// A dead worker's slot received a fresh worker (replace policy).
    Replaced {
        tier: String,
        retired_id: String,
        new_id: String,
    },
    /// A worker was permanently removed from active recovery.
    Retired {
        tier: String,
        worker_id: String,
        reason: String,
    },
    /// An idle spare was torn down to bring the tier back to size.
    TeardownIdle { tier: String, worker_id: String },
    /// An alert was raised (alert policy, or recovery exhausted).
    Alerted {
        tier: String,
        worker_id: Option<String>,
        message: String,
    },
    /// A spawn attempt failed and will be retried with backoff.
    SpawnFailed {
        tier: String,
        worker_id: String,
        attempt: u32,
        error: String,
    },
}

/// Spawns, probes, and stops workers on behalf of a pool.
///
/// The pool is generic over this trait so recovery behavior can be exercised
/// deterministically in tests; [`LauncherPoolSpawner`] is the production
/// implementation backed by tmux launcher scripts.
pub trait PoolSpawner: Send + Sync {
    /// Spawn a new worker for a tier. The worker must exist (or fail) before
    /// returning; the result seeds the member's initial state.
    fn spawn_member(
        &self,
        tier: &str,
        worker_id: &str,
        settings: &ResolvedTierConfig,
    ) -> impl std::future::Future<Output = Result<WorkerHandle>> + Send;

    /// Probe whether a spawned worker is still alive.
    fn probe(
        &self,
        handle: &WorkerHandle,
    ) -> impl std::future::Future<Output = ProbeOutcome> + Send;

    /// Stop a worker. Implementations should tear down the session; the pool
    /// treats a stop error as a warning, not a failure.
    fn stop(&self, handle: &WorkerHandle) -> impl std::future::Future<Output = Result<()>> + Send;
}

impl<T: PoolSpawner + ?Sized> PoolSpawner for std::sync::Arc<T> {
    fn spawn_member(
        &self,
        tier: &str,
        worker_id: &str,
        settings: &ResolvedTierConfig,
    ) -> impl std::future::Future<Output = Result<WorkerHandle>> + Send {
        (**self).spawn_member(tier, worker_id, settings)
    }

    fn probe(
        &self,
        handle: &WorkerHandle,
    ) -> impl std::future::Future<Output = ProbeOutcome> + Send {
        (**self).probe(handle)
    }

    fn stop(&self, handle: &WorkerHandle) -> impl std::future::Future<Output = Result<()>> + Send {
        (**self).stop(handle)
    }
}

/// Production [`PoolSpawner`] backed by tmux launcher scripts.
///
/// Workers are spawned through [`WorkerLauncher`] using the tier's resolved
/// launcher script, model, and workspace; probing reuses the launcher's
/// status check (tmux session + session PID).
#[derive(Debug)]
pub struct LauncherPoolSpawner {
    launcher: WorkerLauncher,
}

impl Default for LauncherPoolSpawner {
    fn default() -> Self {
        Self::new()
    }
}

impl LauncherPoolSpawner {
    /// Create a spawner using the default `forge-` session prefix.
    pub fn new() -> Self {
        Self {
            launcher: WorkerLauncher::new(),
        }
    }

    /// Create a spawner with a custom tmux session prefix.
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            launcher: WorkerLauncher::with_prefix(prefix),
        }
    }
}

/// Map a tier name to its routing tier.
fn tier_from_name(name: &str) -> WorkerTier {
    match name.to_lowercase().as_str() {
        "premium" => WorkerTier::Premium,
        "budget" => WorkerTier::Budget,
        _ => WorkerTier::Standard,
    }
}

impl PoolSpawner for LauncherPoolSpawner {
    async fn spawn_member(
        &self,
        tier: &str,
        worker_id: &str,
        settings: &ResolvedTierConfig,
    ) -> Result<WorkerHandle> {
        let config = LaunchConfig::new(
            &settings.launcher,
            worker_id,
            &settings.workspace,
            &settings.model,
        )
        .with_tier(tier_from_name(tier));
        self.launcher
            .spawn(SpawnRequest::new(worker_id, config))
            .await
    }

    async fn probe(&self, handle: &WorkerHandle) -> ProbeOutcome {
        match self.launcher.check_status(&handle.id).await {
            Ok(WorkerStatus::Active) => ProbeOutcome::Healthy,
            Ok(WorkerStatus::Stopped) => ProbeOutcome::Dead("tmux session is gone".to_string()),
            Ok(other) => ProbeOutcome::Dead(format!("worker status: {other:?}")),
            Err(e) => ProbeOutcome::Dead(e.to_string()),
        }
    }

    async fn stop(&self, handle: &WorkerHandle) -> Result<()> {
        self.launcher.stop(&handle.id).await
    }
}

/// Delay before recovery attempt `n` (1-based): `base * 2^(n-1)` seconds,
/// capped at `max`. Attempt 1 waits `base` seconds, attempt 2 `2 * base`,
/// and so on.
pub fn backoff_delay_secs(base: u64, max: u64, attempt: u32) -> u64 {
    let exp = attempt.saturating_sub(1).min(63);
    base.saturating_mul(1u64 << exp).min(max)
}

/// Normalized pool settings derived from [`WorkerPoolConfig`].
#[derive(Debug, Clone)]
struct PoolSettings {
    enabled: bool,
    policy: PoolRecoveryPolicy,
    max_retries: u32,
    backoff_base_secs: u64,
    backoff_max_secs: u64,
    idle_timeout_secs: u64,
}

impl PoolSettings {
    fn from_config(config: &WorkerPoolConfig) -> Self {
        let backoff_base_secs = config.backoff_base_secs.max(1);
        Self {
            enabled: config.enabled,
            policy: PoolRecoveryPolicy::parse(&config.recovery_policy),
            max_retries: config.max_retries,
            backoff_base_secs,
            backoff_max_secs: config.backoff_max_secs.max(backoff_base_secs),
            idle_timeout_secs: config.idle_timeout_secs,
        }
    }

    /// Delay before recovery attempt `n`.
    fn backoff_delay_secs(&self, attempt: u32) -> u64 {
        backoff_delay_secs(self.backoff_base_secs, self.backoff_max_secs, attempt)
    }
}

/// Per-tier pool state.
#[derive(Debug, Clone)]
struct TierState {
    settings: ResolvedTierConfig,
    members: Vec<PoolWorker>,
    next_seq: u64,
}

impl TierState {
    fn new(settings: ResolvedTierConfig) -> Self {
        Self {
            settings,
            members: Vec::new(),
            next_seq: 1,
        }
    }

    /// Next pool-unique worker id for this tier.
    fn fresh_id(&mut self) -> String {
        let id = format!("pool-{}-{}", self.settings.name, self.next_seq);
        self.next_seq += 1;
        id
    }

    fn index_of(&self, worker_id: &str) -> Option<usize> {
        self.members.iter().position(|m| m.id == worker_id)
    }

    fn count_in_state(&self, state: PoolWorkerState) -> usize {
        self.members.iter().filter(|m| m.state == state).count()
    }
}

/// Per-tier capacity summary, for display and tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PoolTierSummary {
    /// Tier name.
    pub tier: String,
    /// Desired ready workers.
    pub size: usize,
    /// Members spawned but not yet proven alive.
    pub starting: usize,
    /// Members proven alive and available.
    pub ready: usize,
    /// Members awaiting a recovery spawn.
    pub recovering: usize,
    /// Members in the terminal failed state.
    pub failed: usize,
}

/// Worker pool maintaining warm spares per model tier with automatic failover.
///
/// See the [module docs](self) for the failover and recovery model.
pub struct WorkerPool<S: PoolSpawner> {
    settings: PoolSettings,
    reconcile_interval_secs: u64,
    spawner: S,
    tiers: Vec<TierState>,
}

impl<S: PoolSpawner> WorkerPool<S> {
    /// Create a pool from config with a spawner implementation.
    ///
    /// The config is normalized defensively: zero-size tiers are not pooled,
    /// tier sizes are clamped to [`WorkerPoolConfig::MAX_TIER_SIZE`], the
    /// backoff base is floored at 1 second, and the backoff cap is raised to
    /// at least the base. Callers reading user config should still run it
    /// through [`forge_config::ForgeConfig::sanitized`].
    pub fn new(config: WorkerPoolConfig, spawner: S) -> Self {
        let reconcile_interval_secs = config.reconcile_interval_secs;
        let mut tiers: Vec<TierState> = config
            .tiers
            .keys()
            .filter_map(|name| config.resolve_tier(name))
            .map(TierState::new)
            .collect();
        // Sorted for deterministic iteration (config tiers are a HashMap).
        tiers.sort_by(|a, b| a.settings.name.cmp(&b.settings.name));

        Self {
            settings: PoolSettings::from_config(&config),
            reconcile_interval_secs,
            spawner,
            tiers,
        }
    }

    /// Reconcile interval from config, for callers driving the pool.
    pub fn reconcile_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.reconcile_interval_secs.max(1))
    }

    /// Whether the pool maintains capacity. Disabled pools keep their config
    /// (so they can be flipped on) but reconcile is a no-op.
    pub fn is_enabled(&self) -> bool {
        self.settings.enabled
    }

    /// The active recovery policy.
    pub fn policy(&self) -> PoolRecoveryPolicy {
        self.settings.policy
    }

    /// Desired size of a tier (0 for tiers that are not pooled).
    pub fn tier_size(&self, tier: &str) -> usize {
        self.tiers
            .iter()
            .find(|t| t.settings.name == tier)
            .map(|t| t.settings.size)
            .unwrap_or(0)
    }

    /// Update a tier's desired size at runtime (config hot-reload). Shrinking
    /// does not kill workers immediately: excess ready spares are torn down
    /// once they exceed the idle timeout. Unknown tiers are ignored.
    pub fn set_tier_size(&mut self, tier: &str, size: usize) {
        if let Some(state) = self.tiers.iter_mut().find(|t| t.settings.name == tier) {
            state.settings.size = size.min(WorkerPoolConfig::MAX_TIER_SIZE);
        }
    }

    /// Members of a tier (empty for unknown tiers).
    pub fn members(&self, tier: &str) -> Vec<PoolWorker> {
        self.tiers
            .iter()
            .find(|t| t.settings.name == tier)
            .map(|t| t.members.clone())
            .unwrap_or_default()
    }

    /// Capacity summary per configured tier, sorted by tier name.
    pub fn summaries(&self) -> Vec<PoolTierSummary> {
        self.tiers
            .iter()
            .map(|t| PoolTierSummary {
                tier: t.settings.name.clone(),
                size: t.settings.size,
                starting: t.count_in_state(PoolWorkerState::Starting),
                ready: t.count_in_state(PoolWorkerState::Ready),
                recovering: t.count_in_state(PoolWorkerState::Recovering),
                failed: t.count_in_state(PoolWorkerState::Failed),
            })
            .collect()
    }

    /// Ready count for a tier.
    pub fn ready_count(&self, tier: &str) -> usize {
        self.tiers
            .iter()
            .find(|t| t.settings.name == tier)
            .map(|t| t.count_in_state(PoolWorkerState::Ready))
            .unwrap_or(0)
    }

    /// Take the oldest-ready spare from a tier, handing it to a consumer.
    ///
    /// This is the failover entry point: spares are already warm, so callers
    /// never wait on a cold spawn while the pool has capacity. The slot is
    /// refilled on the next reconcile.
    pub fn take_ready(&mut self, tier: &str) -> Option<WorkerHandle> {
        let state = self.tiers.iter_mut().find(|t| t.settings.name == tier)?;
        let idx = state
            .members
            .iter()
            .enumerate()
            .filter(|(_, m)| m.state == PoolWorkerState::Ready)
            .min_by_key(|(_, m)| m.ready_since)
            .map(|(i, _)| i)?;
        let member = state.members.remove(idx);
        debug!(tier, worker_id = %member.id, "Handed pooled worker to consumer");
        member.handle
    }

    /// Feed an external health-monitor verdict into the pool.
    ///
    /// Equivalent to the member failing its own probe: the recovery policy
    /// decides what happens. Members already recovering or failed are
    /// ignored (their fate is already decided). Returns the alert event when
    /// the alert policy fired, so callers can surface it immediately.
    pub async fn report_unhealthy(
        &mut self,
        tier: &str,
        worker_id: &str,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Option<PoolEvent> {
        if !self.settings.enabled {
            return None;
        }
        let tier_idx = self.tiers.iter().position(|t| t.settings.name == tier)?;
        let idx = {
            let state = &self.tiers[tier_idx];
            match state.index_of(worker_id) {
                Some(idx) if state.members[idx].state.is_live() => idx,
                _ => return None,
            }
        };
        let events = self
            .handle_failure(tier_idx, idx, reason.to_string(), now)
            .await;
        events
            .into_iter()
            .find(|e| matches!(e, PoolEvent::Alerted { .. }))
    }

    /// Run one reconcile pass: probe members, apply the recovery policy to
    /// failures, run due recovery spawns, tear down idle excess, and refill
    /// to the configured size.
    ///
    /// `now` is injected so backoff and idle-timeout behavior are
    /// deterministic; production callers pass `Utc::now()` once per tick
    /// (suggested cadence: [`WorkerPool::reconcile_interval`]).
    pub async fn reconcile(&mut self, now: DateTime<Utc>) -> Result<Vec<PoolEvent>> {
        let mut events = Vec::new();
        if !self.settings.enabled {
            return Ok(events);
        }

        for tier_idx in 0..self.tiers.len() {
            // 1. Probe live members, collecting deaths. Split borrows: the
            //    tier is borrowed mutably while the shared spawner is only
            //    read.
            let deaths = {
                let tier = &mut self.tiers[tier_idx];
                let mut deaths = Vec::new();
                for idx in 0..tier.members.len() {
                    if !tier.members[idx].state.is_live() {
                        continue;
                    }
                    let Some(handle) = tier.members[idx].handle.clone() else {
                        continue;
                    };
                    let outcome = self.spawner.probe(&handle).await;
                    tier.members[idx].last_probe = Some(now);
                    match outcome {
                        ProbeOutcome::Healthy => {
                            if tier.members[idx].state == PoolWorkerState::Starting {
                                let member = &mut tier.members[idx];
                                member.state = PoolWorkerState::Ready;
                                member.ready_since = Some(now);
                                info!(tier = %tier.settings.name, worker_id = %member.id,
                                      "Pooled worker is ready");
                                events.push(PoolEvent::Ready {
                                    tier: tier.settings.name.clone(),
                                    worker_id: member.id.clone(),
                                });
                            }
                        }
                        ProbeOutcome::Dead(reason) => deaths.push((idx, reason)),
                    }
                }
                deaths
            };

            // 2. Route deaths through the recovery policy. handle_failure
            //    only mutates members in place, so indices stay valid.
            for (idx, reason) in deaths {
                events.extend(self.handle_failure(tier_idx, idx, reason, now).await);
            }

            // 3. Run recovery spawns whose backoff has elapsed.
            for idx in 0..self.tiers[tier_idx].members.len() {
                let due = {
                    let member = &self.tiers[tier_idx].members[idx];
                    member.state == PoolWorkerState::Recovering
                        && member.next_action_at.is_some_and(|at| now >= at)
                };
                if due {
                    events.extend(self.run_recovery(tier_idx, idx, now).await);
                }
            }

            // 4. Tear down idle excess, then refill handed-out slots.
            let tier = &mut self.tiers[tier_idx];
            events.extend(teardown_idle(tier, &self.spawner, &self.settings, now).await);
            events.extend(fill_to_size(tier, &self.spawner, &self.settings, now).await);
        }

        Ok(events)
    }

    /// Stop every member, clear the pool, and disable it.
    ///
    /// A disabled pool's reconcile is a no-op, so a caller that keeps
    /// ticking after shutdown does not have members resurrected.
    pub async fn shutdown(&mut self) -> Result<()> {
        for tier in &mut self.tiers {
            for member in &mut tier.members {
                if let Some(handle) = member.handle.take()
                    && let Err(e) = self.spawner.stop(&handle).await
                {
                    warn!(tier = %tier.settings.name, worker_id = %member.id, error = %e,
                          "Failed to stop pooled worker during shutdown");
                }
            }
            tier.members.clear();
        }
        self.settings.enabled = false;
        info!("Worker pool shut down");
        Ok(())
    }

    /// Apply the recovery policy to a live member (Starting or Ready) that
    /// was detected dead or unhealthy.
    async fn handle_failure(
        &mut self,
        tier_idx: usize,
        idx: usize,
        reason: String,
        now: DateTime<Utc>,
    ) -> Vec<PoolEvent> {
        let policy = self.settings.policy;
        let mut events = Vec::new();

        if policy == PoolRecoveryPolicy::Alert {
            let tier = &mut self.tiers[tier_idx];
            let member = &mut tier.members[idx];
            member.state = PoolWorkerState::Failed;
            member.next_action_at = None;
            member.last_error = Some(reason.clone());
            warn!(tier = %tier.settings.name, worker_id = %member.id, reason = %reason,
                  "Pooled worker dead; alert-only policy, no automatic recovery");
            events.push(PoolEvent::Alerted {
                tier: tier.settings.name.clone(),
                worker_id: Some(member.id.clone()),
                message: format!("pooled worker failed: {reason}"),
            });
            return events;
        }

        // restart/replace: stop the corpse before scheduling recovery.
        let tier = &mut self.tiers[tier_idx];
        let member = &mut tier.members[idx];
        if let Some(handle) = member.handle.take()
            && let Err(e) = self.spawner.stop(&handle).await
        {
            warn!(tier = %tier.settings.name, worker_id = %member.id, error = %e,
                  "Failed to stop dead pooled worker");
        }
        member.ready_since = None;
        member.last_error = Some(reason.clone());

        match policy {
            PoolRecoveryPolicy::Restart => {
                let attempt = member.recovery_attempts + 1;
                if attempt > self.settings.max_retries {
                    member.state = PoolWorkerState::Failed;
                    member.next_action_at = None;
                    let (id, attempts) = (member.id.clone(), member.recovery_attempts);
                    info!(tier = %tier.settings.name, worker_id = %id, attempts,
                          "Pooled worker recovery exhausted; retiring");
                    events.push(PoolEvent::Retired {
                        tier: tier.settings.name.clone(),
                        worker_id: id.clone(),
                        reason: format!(
                            "recovery attempts exhausted after {attempts} respawn(s): {reason}"
                        ),
                    });
                    events.push(PoolEvent::Alerted {
                        tier: tier.settings.name.clone(),
                        worker_id: Some(id),
                        message: format!(
                            "pooled worker retired after {attempts} recovery attempt(s): {reason}"
                        ),
                    });
                } else {
                    member.state = PoolWorkerState::Recovering;
                    let delay = self.settings.backoff_delay_secs(attempt);
                    member.next_action_at = Some(now + chrono_secs(delay));
                    debug!(tier = %tier.settings.name, worker_id = %member.id, attempt,
                           delay_secs = delay, "Pooled worker scheduled for in-place restart");
                }
            }
            PoolRecoveryPolicy::Replace => {
                // The slot gets a fresh worker id with a fresh retry budget;
                // the corpse's id is remembered so the replacement can be
                // attributed when the spawn succeeds.
                let corpse_id = member.id.clone();
                let fresh_id = tier.fresh_id();
                let member = &mut tier.members[idx];
                member.replacing = Some(corpse_id.clone());
                member.recovery_attempts = 0;
                member.last_error = Some(reason.clone());
                member.id = fresh_id.clone();
                member.state = PoolWorkerState::Recovering;
                member.next_action_at = Some(now); // spawn immediately
                info!(tier = %tier.settings.name, retired = %corpse_id,
                      replacement = %fresh_id, reason = %reason,
                      "Pooled worker dead; slot queued for replacement");
                events.push(PoolEvent::Retired {
                    tier: tier.settings.name.clone(),
                    worker_id: corpse_id,
                    reason: format!("dead: {reason}; slot queued for replacement"),
                });
            }
            PoolRecoveryPolicy::Alert => unreachable!("handled above"),
        }

        events
    }

    /// Run the scheduled recovery spawn for a due `Recovering` member.
    async fn run_recovery(
        &mut self,
        tier_idx: usize,
        idx: usize,
        now: DateTime<Utc>,
    ) -> Vec<PoolEvent> {
        let mut events = Vec::new();
        let tier_name = self.tiers[tier_idx].settings.name.clone();

        let attempt = {
            let member = &mut self.tiers[tier_idx].members[idx];
            member.recovery_attempts += 1;
            member.recovery_attempts
        };
        let (worker_id, replacing) = {
            let member = &self.tiers[tier_idx].members[idx];
            (member.id.clone(), member.replacing.clone())
        };
        let settings = self.tiers[tier_idx].settings.clone();

        match self
            .spawner
            .spawn_member(&tier_name, &worker_id, &settings)
            .await
        {
            Ok(handle) => {
                let member = &mut self.tiers[tier_idx].members[idx];
                member.handle = Some(handle);
                member.state = PoolWorkerState::Starting;
                member.next_action_at = None;
                match replacing {
                    Some(retired_id) => {
                        member.replacing = None;
                        info!(tier = %tier_name, worker_id = %worker_id, retired = %retired_id,
                              "Pooled worker replaced");
                        events.push(PoolEvent::Replaced {
                            tier: tier_name,
                            retired_id,
                            new_id: worker_id,
                        });
                    }
                    None => {
                        info!(tier = %tier_name, worker_id = %worker_id, attempt,
                              "Pooled worker respawned in place");
                        events.push(PoolEvent::Restarted {
                            tier: tier_name,
                            worker_id,
                            attempt,
                        });
                    }
                }
            }
            Err(error) => {
                let exhausted =
                    self.settings.max_retries == 0 || attempt >= self.settings.max_retries;
                {
                    let member = &mut self.tiers[tier_idx].members[idx];
                    member.last_error = Some(error.to_string());
                    if exhausted {
                        member.state = PoolWorkerState::Failed;
                        member.next_action_at = None;
                    } else {
                        member.next_action_at =
                            Some(now + chrono_secs(self.settings.backoff_delay_secs(attempt + 1)));
                    }
                }
                warn!(tier = %tier_name, worker_id = %worker_id, attempt,
                      exhausted, error = %error,
                      "Pooled worker recovery spawn failed");
                events.push(PoolEvent::SpawnFailed {
                    tier: tier_name.clone(),
                    worker_id: worker_id.clone(),
                    attempt,
                    error: error.to_string(),
                });
                if exhausted {
                    events.push(PoolEvent::Retired {
                        tier: tier_name.clone(),
                        worker_id: worker_id.clone(),
                        reason: format!("spawn failed {attempt} time(s): {error}"),
                    });
                    events.push(PoolEvent::Alerted {
                        tier: tier_name,
                        worker_id: Some(worker_id),
                        message: format!(
                            "pooled worker recovery exhausted: spawn failed {attempt} time(s): {error}"
                        ),
                    });
                }
            }
        }

        events
    }
}

/// Tear down ready spares beyond the tier's size, oldest-idle first.
async fn teardown_idle<S: PoolSpawner>(
    tier: &mut TierState,
    spawner: &S,
    settings: &PoolSettings,
    now: DateTime<Utc>,
) -> Vec<PoolEvent> {
    let mut events = Vec::new();
    if settings.idle_timeout_secs == 0 {
        return events; // teardown disabled
    }

    let ready_count = tier.count_in_state(PoolWorkerState::Ready);
    let mut excess = ready_count.saturating_sub(tier.settings.size);
    if excess == 0 {
        return events;
    }

    // Oldest-ready first; only spares idle longer than the timeout qualify.
    let mut candidates: Vec<usize> = (0..tier.members.len())
        .filter(|&i| {
            tier.members[i].state == PoolWorkerState::Ready
                && tier.members[i]
                    .ready_since
                    .is_some_and(|since| now - since > chrono_secs(settings.idle_timeout_secs))
        })
        .collect();
    candidates.sort_by_key(|&i| tier.members[i].ready_since);

    let mut removed = Vec::new();
    for idx in candidates {
        if excess == 0 {
            break;
        }
        let member = &mut tier.members[idx];
        if let Some(handle) = member.handle.take()
            && let Err(e) = spawner.stop(&handle).await
        {
            warn!(tier = %tier.settings.name, worker_id = %member.id, error = %e,
                  "Failed to stop idle pooled worker");
        }
        info!(tier = %tier.settings.name, worker_id = %member.id,
              "Tearing down idle pooled spare");
        events.push(PoolEvent::TeardownIdle {
            tier: tier.settings.name.clone(),
            worker_id: member.id.clone(),
        });
        removed.push(idx);
        excess -= 1;
    }

    for idx in removed.into_iter().rev() {
        tier.members.remove(idx);
    }
    events
}

/// Provision new workers until the tier reaches its configured size.
///
/// Every existing member (including recovering and failed ones) holds a
/// slot, so a tier short because of failed capacity stays short — that is
/// the alert-only / exhausted-retirement contract, and it prevents endless
/// replacement churn. Failed provisioning attempts become `Recovering`
/// slots so retries happen with backoff, not in a tight loop.
async fn fill_to_size<S: PoolSpawner>(
    tier: &mut TierState,
    spawner: &S,
    settings: &PoolSettings,
    now: DateTime<Utc>,
) -> Vec<PoolEvent> {
    let mut events = Vec::new();

    while tier.members.len() < tier.settings.size {
        let worker_id = tier.fresh_id();
        match spawner
            .spawn_member(&tier.settings.name, &worker_id, &tier.settings)
            .await
        {
            Ok(handle) => {
                info!(tier = %tier.settings.name, worker_id = %worker_id,
                      "Provisioned pooled worker");
                events.push(PoolEvent::Spawned {
                    tier: tier.settings.name.clone(),
                    worker_id: worker_id.clone(),
                });
                tier.members.push(PoolWorker {
                    id: worker_id,
                    tier: tier.settings.name.clone(),
                    state: PoolWorkerState::Starting,
                    handle: Some(handle),
                    recovery_attempts: 0,
                    replacing: None,
                    ready_since: None,
                    last_probe: None,
                    next_action_at: None,
                    last_error: None,
                });
            }
            Err(error) => {
                warn!(tier = %tier.settings.name, worker_id = %worker_id, error = %error,
                      "Failed to provision pooled worker; will retry with backoff");
                events.push(PoolEvent::SpawnFailed {
                    tier: tier.settings.name.clone(),
                    worker_id: worker_id.clone(),
                    attempt: 1,
                    error: error.to_string(),
                });
                tier.members.push(PoolWorker {
                    id: worker_id,
                    tier: tier.settings.name.clone(),
                    state: PoolWorkerState::Recovering,
                    handle: None,
                    recovery_attempts: 1,
                    replacing: None,
                    ready_since: None,
                    last_probe: None,
                    next_action_at: Some(now + chrono_secs(settings.backoff_delay_secs(2))),
                    last_error: Some(error.to_string()),
                });
            }
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use forge_core::ForgeError;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// Deterministic time base: 2026-09-16T00:00:00Z.
    fn base_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 16, 0, 0, 0).unwrap()
    }

    fn tier_config(name: &str, size: usize) -> (String, forge_config::PoolTierConfig) {
        (
            name.to_string(),
            forge_config::PoolTierConfig {
                size,
                ..Default::default()
            },
        )
    }

    fn pool_config(tiers: Vec<(String, forge_config::PoolTierConfig)>) -> WorkerPoolConfig {
        WorkerPoolConfig {
            enabled: true,
            tiers: tiers.into_iter().collect(),
            recovery_policy: "alert".to_string(),
            max_retries: 3,
            backoff_base_secs: 1,
            backoff_max_secs: 8,
            idle_timeout_secs: 60,
            reconcile_interval_secs: 30,
        }
    }

    /// In-memory spawner with scriptable spawn failures and probe outcomes.
    #[derive(Default)]
    struct SpawnerState {
        /// Remaining forced spawn failures per worker id (0 or absent = ok).
        spawn_failures: HashMap<String, u32>,
        /// Probe outcome per worker id (absent = Healthy).
        probes: HashMap<String, ProbeOutcome>,
        spawned: Vec<String>,
        stopped: Vec<String>,
    }

    #[derive(Default)]
    struct TestSpawner {
        state: Mutex<SpawnerState>,
    }

    impl TestSpawner {
        fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        /// Force the next `count` spawns of `worker_id` to fail.
        fn fail_next_spawns(&self, worker_id: &str, count: u32) {
            self.state
                .lock()
                .unwrap()
                .spawn_failures
                .insert(worker_id.to_string(), count);
        }

        fn set_probe(&self, worker_id: &str, outcome: ProbeOutcome) {
            self.state
                .lock()
                .unwrap()
                .probes
                .insert(worker_id.to_string(), outcome);
        }

        fn spawned(&self) -> Vec<String> {
            self.state.lock().unwrap().spawned.clone()
        }

        fn stopped(&self) -> Vec<String> {
            self.state.lock().unwrap().stopped.clone()
        }
    }

    fn test_handle(worker_id: &str) -> WorkerHandle {
        WorkerHandle::new(
            worker_id,
            4242,
            format!("forge-{worker_id}"),
            "/tmp/launcher.sh",
            "sonnet",
            WorkerTier::Standard,
            "/workspace",
        )
    }

    impl PoolSpawner for TestSpawner {
        async fn spawn_member(
            &self,
            _tier: &str,
            worker_id: &str,
            _settings: &ResolvedTierConfig,
        ) -> Result<WorkerHandle> {
            let mut state = self.state.lock().unwrap();
            let failures = state.spawn_failures.get(worker_id).copied().unwrap_or(0);
            if failures > 0 {
                state
                    .spawn_failures
                    .insert(worker_id.to_string(), failures - 1);
                return Err(ForgeError::WorkerSpawn {
                    worker_id: worker_id.to_string(),
                    message: "injected failure".to_string(),
                });
            }
            state.spawned.push(worker_id.to_string());
            Ok(test_handle(worker_id))
        }

        async fn probe(&self, handle: &WorkerHandle) -> ProbeOutcome {
            self.state
                .lock()
                .unwrap()
                .probes
                .get(&handle.id)
                .cloned()
                .unwrap_or(ProbeOutcome::Healthy)
        }

        async fn stop(&self, handle: &WorkerHandle) -> Result<()> {
            self.state.lock().unwrap().stopped.push(handle.id.clone());
            Ok(())
        }
    }

    fn standard_restart_pool(
        spawner: &Arc<TestSpawner>,
        size: usize,
    ) -> WorkerPool<Arc<TestSpawner>> {
        let mut config = pool_config(vec![tier_config("standard", size)]);
        config.recovery_policy = "restart".to_string();
        WorkerPool::new(config, Arc::clone(spawner))
    }

    // ------------------------------------------------------------
    // Policy parsing and backoff math
    // ------------------------------------------------------------

    #[test]
    fn test_policy_parse() {
        assert_eq!(
            PoolRecoveryPolicy::parse("restart"),
            PoolRecoveryPolicy::Restart
        );
        assert_eq!(
            PoolRecoveryPolicy::parse("REPLACE"),
            PoolRecoveryPolicy::Replace
        );
        assert_eq!(
            PoolRecoveryPolicy::parse("alert"),
            PoolRecoveryPolicy::Alert
        );
        // Unknown and empty values fall back to alert-only.
        assert_eq!(
            PoolRecoveryPolicy::parse("explode"),
            PoolRecoveryPolicy::Alert
        );
        assert_eq!(PoolRecoveryPolicy::parse(""), PoolRecoveryPolicy::Alert);
        assert_eq!(PoolRecoveryPolicy::default(), PoolRecoveryPolicy::Alert);
    }

    #[test]
    fn test_backoff_delay_growth_and_cap() {
        assert_eq!(backoff_delay_secs(5, 300, 1), 5);
        assert_eq!(backoff_delay_secs(5, 300, 2), 10);
        assert_eq!(backoff_delay_secs(5, 300, 3), 20);
        assert_eq!(backoff_delay_secs(5, 300, 4), 40);
        // Capped at max.
        assert_eq!(backoff_delay_secs(5, 300, 8), 300);
        assert_eq!(backoff_delay_secs(5, 300, 63), 300);
        // Degenerate inputs are safe.
        assert_eq!(backoff_delay_secs(5, 300, 0), 5);
        assert_eq!(backoff_delay_secs(u64::MAX, u64::MAX, 70), u64::MAX);
    }

    #[test]
    fn test_new_normalizes_config() {
        let mut config = pool_config(vec![
            tier_config("standard", WorkerPoolConfig::MAX_TIER_SIZE + 5),
            tier_config("budget", 0),
        ]);
        config.backoff_base_secs = 0;
        config.backoff_max_secs = 0;
        config.recovery_policy = "yolo".to_string();

        let spawner = TestSpawner::new();
        let pool = WorkerPool::new(config, Arc::clone(&spawner));

        // Zero-size tiers are dropped; oversized tiers are clamped.
        assert_eq!(pool.tier_size("budget"), 0);
        assert_eq!(pool.tier_size("standard"), WorkerPoolConfig::MAX_TIER_SIZE);
        // Bad policy string is normalized to alert-only.
        assert_eq!(pool.policy(), PoolRecoveryPolicy::Alert);
        assert_eq!(
            pool.reconcile_interval(),
            std::time::Duration::from_secs(30)
        );
    }

    // ------------------------------------------------------------
    // Fill and readiness
    // ------------------------------------------------------------

    #[tokio::test]
    async fn test_fill_spawns_to_size_and_probes_ready() {
        let spawner = TestSpawner::new();
        let mut pool = standard_restart_pool(&spawner, 2);
        let now = base_time();

        let events = pool.reconcile(now).await.unwrap();
        assert_eq!(
            events,
            vec![
                PoolEvent::Spawned {
                    tier: "standard".into(),
                    worker_id: "pool-standard-1".into(),
                },
                PoolEvent::Spawned {
                    tier: "standard".into(),
                    worker_id: "pool-standard-2".into(),
                },
            ]
        );
        assert_eq!(
            spawner.spawned(),
            vec!["pool-standard-1", "pool-standard-2"]
        );
        assert_eq!(pool.ready_count("standard"), 0);

        // Next tick probes them healthy → Ready.
        let events = pool
            .reconcile(now + chrono::Duration::seconds(1))
            .await
            .unwrap();
        assert_eq!(
            events,
            vec![
                PoolEvent::Ready {
                    tier: "standard".into(),
                    worker_id: "pool-standard-1".into(),
                },
                PoolEvent::Ready {
                    tier: "standard".into(),
                    worker_id: "pool-standard-2".into(),
                },
            ]
        );
        assert_eq!(pool.ready_count("standard"), 2);

        // Settled pool: no more events.
        let events = pool
            .reconcile(now + chrono::Duration::seconds(2))
            .await
            .unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn test_disabled_pool_is_a_noop() {
        let spawner = TestSpawner::new();
        let mut config = pool_config(vec![tier_config("standard", 2)]);
        config.enabled = false;
        let mut pool = WorkerPool::new(config, Arc::clone(&spawner));

        let events = pool.reconcile(base_time()).await.unwrap();
        assert!(events.is_empty());
        assert!(spawner.spawned().is_empty());
        assert!(!pool.is_enabled());
    }

    #[tokio::test]
    async fn test_take_ready_hands_out_oldest_spare_and_refills() {
        let spawner = TestSpawner::new();
        let mut pool = standard_restart_pool(&spawner, 2);
        let now = base_time();

        pool.reconcile(now).await.unwrap();
        pool.reconcile(now + chrono::Duration::seconds(1))
            .await
            .unwrap();

        // Both ready; the first handed out is the first that became ready.
        let first = pool.take_ready("standard").expect("spare available");
        assert_eq!(first.id, "pool-standard-1");
        assert_eq!(pool.ready_count("standard"), 1);

        // The slot is refilled with a fresh worker id on the next tick.
        pool.reconcile(now + chrono::Duration::seconds(2))
            .await
            .unwrap();
        assert!(spawner.spawned().contains(&"pool-standard-3".to_string()));
        assert_eq!(pool.members("standard").len(), 2);
        assert_eq!(pool.ready_count("standard"), 1);

        // Once the remaining spare is taken the tier is dry until refill.
        assert_eq!(
            pool.take_ready("standard").expect("second spare").id,
            "pool-standard-2"
        );
        assert!(pool.take_ready("standard").is_none());
        assert!(pool.take_ready("unknown-tier").is_none());
    }

    // ------------------------------------------------------------
    // Recovery policies
    // ------------------------------------------------------------

    #[tokio::test]
    async fn test_alert_policy_marks_failed_without_recovery() {
        let spawner = TestSpawner::new();
        let mut config = pool_config(vec![tier_config("standard", 1)]);
        config.recovery_policy = "alert".to_string();
        let mut pool = WorkerPool::new(config, Arc::clone(&spawner));
        let now = base_time();

        pool.reconcile(now).await.unwrap();
        pool.reconcile(now + chrono::Duration::seconds(1))
            .await
            .unwrap();

        spawner.set_probe("pool-standard-1", ProbeOutcome::Dead("session gone".into()));
        let events = pool
            .reconcile(now + chrono::Duration::seconds(2))
            .await
            .unwrap();

        assert!(events.contains(&PoolEvent::Alerted {
            tier: "standard".into(),
            worker_id: Some("pool-standard-1".into()),
            message: "pooled worker failed: session gone".into(),
        }));
        // No respawn: the member is failed and holds its slot.
        assert_eq!(spawner.spawned().len(), 1);
        let summary = &pool.summaries()[0];
        assert_eq!(
            (summary.ready, summary.recovering, summary.failed),
            (0, 0, 1)
        );
        // Subsequent ticks neither respawn nor re-alert.
        let events = pool
            .reconcile(now + chrono::Duration::seconds(120))
            .await
            .unwrap();
        assert!(events.is_empty());
        assert_eq!(spawner.spawned().len(), 1);
    }

    #[tokio::test]
    async fn test_restart_policy_respawns_in_place_after_backoff() {
        let spawner = TestSpawner::new();
        let mut pool = standard_restart_pool(&spawner, 1);
        let now = base_time();

        pool.reconcile(now).await.unwrap();
        pool.reconcile(now + chrono::Duration::seconds(1))
            .await
            .unwrap();

        spawner.set_probe("pool-standard-1", ProbeOutcome::Dead("crashed".into()));
        let events = pool
            .reconcile(now + chrono::Duration::seconds(2))
            .await
            .unwrap();

        // The corpse was stopped and a restart scheduled after base backoff.
        assert!(spawner.stopped().contains(&"pool-standard-1".to_string()));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, PoolEvent::Restarted { .. }))
        );
        let member = &pool.members("standard")[0];
        assert_eq!(member.state, PoolWorkerState::Recovering);
        assert_eq!(member.recovery_attempts, 0);
        assert_eq!(member.last_error.as_deref(), Some("crashed"));

        // Too early: backoff has not elapsed, nothing happens.
        pool.reconcile(now + chrono::Duration::seconds(2))
            .await
            .unwrap();
        assert_eq!(spawner.spawned().len(), 1);

        // After the backoff delay the same worker id respawns.
        let events = pool
            .reconcile(now + chrono::Duration::seconds(4))
            .await
            .unwrap();
        assert_eq!(
            events,
            vec![PoolEvent::Restarted {
                tier: "standard".into(),
                worker_id: "pool-standard-1".into(),
                attempt: 1,
            }]
        );
        assert_eq!(
            spawner.spawned(),
            vec!["pool-standard-1", "pool-standard-1"]
        );
        assert_eq!(pool.members("standard")[0].state, PoolWorkerState::Starting);
    }

    #[tokio::test]
    async fn test_restart_policy_exhaustion_retires_and_alerts() {
        let spawner = TestSpawner::new();
        let mut config = pool_config(vec![tier_config("standard", 1)]);
        config.recovery_policy = "restart".to_string();
        config.max_retries = 2;
        let mut pool = WorkerPool::new(config, Arc::clone(&spawner));
        let now = base_time();

        pool.reconcile(now).await.unwrap();
        pool.reconcile(now + chrono::Duration::seconds(1))
            .await
            .unwrap();

        // Each death→recovery cycle takes two ticks: the death is detected
        // on one tick (respawn scheduled after backoff), and the respawn
        // runs on the next. With max_retries = 2 the third death exhausts
        // the budget.
        let mut t = now + chrono::Duration::seconds(2);
        for cycle in 0..3 {
            spawner.set_probe("pool-standard-1", ProbeOutcome::Dead("crash loop".into()));
            pool.reconcile(t).await.unwrap();
            let member = &pool.members("standard")[0];
            let (expected_state, expected_attempts) = if cycle < 2 {
                (PoolWorkerState::Recovering, cycle)
            } else {
                (PoolWorkerState::Failed, 2)
            };
            assert_eq!(member.state, expected_state, "cycle {cycle}");
            assert_eq!(member.recovery_attempts, expected_attempts, "cycle {cycle}");

            t += chrono::Duration::seconds(8); // clear every backoff window
            pool.reconcile(t).await.unwrap();
            if cycle < 2 {
                assert_eq!(pool.members("standard")[0].state, PoolWorkerState::Starting);
            }
        }

        let member = &pool.members("standard")[0];
        assert_eq!(member.state, PoolWorkerState::Failed);
        assert_eq!(member.recovery_attempts, 2);

        // The corpse was stopped at each detection.
        assert!(
            spawner
                .stopped()
                .iter()
                .filter(|id| *id == "pool-standard-1")
                .count()
                >= 3
        );

        // No further respawns after exhaustion, and no re-alerting.
        let spawned_before = spawner.spawned().len();
        let events = pool
            .reconcile(now + chrono::Duration::seconds(600))
            .await
            .unwrap();
        assert!(events.is_empty());
        assert_eq!(spawner.spawned().len(), spawned_before);
    }

    #[tokio::test]
    async fn test_replace_policy_swaps_in_a_fresh_worker() {
        let spawner = TestSpawner::new();
        let mut config = pool_config(vec![tier_config("standard", 1)]);
        config.recovery_policy = "replace".to_string();
        let mut pool = WorkerPool::new(config, Arc::clone(&spawner));
        let now = base_time();

        pool.reconcile(now).await.unwrap();
        pool.reconcile(now + chrono::Duration::seconds(1))
            .await
            .unwrap();

        spawner.set_probe("pool-standard-1", ProbeOutcome::Dead("OOM killed".into()));
        let events = pool
            .reconcile(now + chrono::Duration::seconds(2))
            .await
            .unwrap();

        // The corpse is retired and its slot queued for replacement.
        assert!(events.contains(&PoolEvent::Retired {
            tier: "standard".into(),
            worker_id: "pool-standard-1".into(),
            reason: "dead: OOM killed; slot queued for replacement".into(),
        }));
        assert!(spawner.stopped().contains(&"pool-standard-1".to_string()));

        // Same tick: the replacement spawns with a fresh id.
        assert!(events.contains(&PoolEvent::Replaced {
            tier: "standard".into(),
            retired_id: "pool-standard-1".into(),
            new_id: "pool-standard-2".into(),
        }));
        assert_eq!(
            spawner.spawned(),
            vec!["pool-standard-1", "pool-standard-2"]
        );
        let member = &pool.members("standard")[0];
        assert_eq!(member.id, "pool-standard-2");
        assert_eq!(member.state, PoolWorkerState::Starting);
        // The replacement's own counter starts fresh (the corpse's budget did
        // not carry over); its provisioning spawn is attempt 1.
        assert_eq!(member.recovery_attempts, 1);
        assert_eq!(pool.ready_count("standard"), 0);
    }

    #[tokio::test]
    async fn test_replace_policy_spawn_failures_escalate_to_alert() {
        let spawner = TestSpawner::new();
        let mut config = pool_config(vec![tier_config("standard", 1)]);
        config.recovery_policy = "replace".to_string();
        config.max_retries = 2;
        let mut pool = WorkerPool::new(config, Arc::clone(&spawner));
        let now = base_time();

        pool.reconcile(now).await.unwrap();
        pool.reconcile(now + chrono::Duration::seconds(1))
            .await
            .unwrap();

        // Every replacement spawn fails; the retry backoff doubles, and the
        // slot fails terminally after max_retries instead of churning.
        let replacement_id = "pool-standard-2";
        spawner.fail_next_spawns(replacement_id, 2);

        spawner.set_probe("pool-standard-1", ProbeOutcome::Dead("gone".into()));
        let events = pool
            .reconcile(now + chrono::Duration::seconds(2))
            .await
            .unwrap();
        // First replacement attempt fails; retry scheduled with backoff.
        assert!(events.contains(&PoolEvent::SpawnFailed {
            tier: "standard".into(),
            worker_id: replacement_id.into(),
            attempt: 1,
            error: "Failed to spawn worker pool-standard-2: injected failure".into(),
        }));
        assert_eq!(
            pool.members("standard")[0].state,
            PoolWorkerState::Recovering
        );

        // Second attempt also fails → exhausted: the slot fails terminally.
        let events = pool
            .reconcile(now + chrono::Duration::seconds(8))
            .await
            .unwrap();
        assert_eq!(pool.members("standard")[0].state, PoolWorkerState::Failed);

        assert!(events.contains(&PoolEvent::Retired {
            tier: "standard".into(),
            worker_id: replacement_id.into(),
            reason: "spawn failed 2 time(s): Failed to spawn worker pool-standard-2: injected failure"
                .into(),
        }));
        assert!(events.contains(&PoolEvent::Alerted {
            tier: "standard".into(),
            worker_id: Some(replacement_id.into()),
            message: "pooled worker recovery exhausted: spawn failed 2 time(s): Failed to spawn worker pool-standard-2: injected failure"
                .into(),
        }));

        // The failed slot still holds capacity: no fresh-spawn churn.
        assert_eq!(pool.summaries()[0].failed, 1);
        let spawned_before = spawner.spawned().len();
        pool.reconcile(now + chrono::Duration::seconds(600))
            .await
            .unwrap();
        assert_eq!(spawner.spawned().len(), spawned_before);
    }

    #[tokio::test]
    async fn test_provision_failure_retries_with_backoff_then_succeeds() {
        let spawner = TestSpawner::new();
        let mut pool = standard_restart_pool(&spawner, 1);
        spawner.fail_next_spawns("pool-standard-1", 1);
        let now = base_time();

        let events = pool.reconcile(now).await.unwrap();
        assert!(events.contains(&PoolEvent::SpawnFailed {
            tier: "standard".into(),
            worker_id: "pool-standard-1".into(),
            attempt: 1,
            error: "Failed to spawn worker pool-standard-1: injected failure".into(),
        }));
        assert_eq!(
            pool.members("standard")[0].state,
            PoolWorkerState::Recovering
        );

        // Too early: backoff holds.
        pool.reconcile(now + chrono::Duration::seconds(1))
            .await
            .unwrap();
        assert!(spawner.spawned().is_empty());

        // Retry succeeds after backoff.
        pool.reconcile(now + chrono::Duration::seconds(60))
            .await
            .unwrap();
        assert_eq!(spawner.spawned(), vec!["pool-standard-1"]);
        assert_eq!(pool.members("standard")[0].state, PoolWorkerState::Starting);
    }

    // ------------------------------------------------------------
    // External health reports and idle teardown
    // ------------------------------------------------------------

    #[tokio::test]
    async fn test_report_unhealthy_routes_by_policy() {
        let spawner = TestSpawner::new();
        let mut config = pool_config(vec![tier_config("standard", 1)]);
        config.recovery_policy = "restart".to_string();
        let mut pool = WorkerPool::new(config, Arc::clone(&spawner));
        let now = base_time();

        pool.reconcile(now).await.unwrap();
        pool.reconcile(now + chrono::Duration::seconds(1))
            .await
            .unwrap();

        let alert = pool
            .report_unhealthy("standard", "pool-standard-1", "stale activity 30m", now)
            .await;
        // Restart policy: no immediate alert, corpse stopped, backoff set.
        assert!(alert.is_none());
        assert!(spawner.stopped().contains(&"pool-standard-1".to_string()));
        assert_eq!(
            pool.members("standard")[0].state,
            PoolWorkerState::Recovering
        );

        // Unknown worker ids and unknown tiers are ignored.
        assert!(
            pool.report_unhealthy("standard", "no-such-worker", "x", now)
                .await
                .is_none()
        );
        assert!(
            pool.report_unhealthy("unknown-tier", "pool-standard-1", "x", now)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_report_unhealthy_alert_policy_surfaces_event() {
        let spawner = TestSpawner::new();
        let mut config = pool_config(vec![tier_config("standard", 1)]);
        config.recovery_policy = "alert".to_string();
        let mut pool = WorkerPool::new(config, Arc::clone(&spawner));
        let now = base_time();

        pool.reconcile(now).await.unwrap();
        let alert = pool
            .report_unhealthy("standard", "pool-standard-1", "unresponsive", now)
            .await
            .expect("alert policy returns the alert");
        assert_eq!(
            alert,
            PoolEvent::Alerted {
                tier: "standard".into(),
                worker_id: Some("pool-standard-1".into()),
                message: "pooled worker failed: unresponsive".into(),
            }
        );
        // Alert-only: the session is left alone.
        assert!(spawner.stopped().is_empty());
    }

    #[tokio::test]
    async fn test_idle_teardown_shrinks_oldest_first_after_timeout() {
        let spawner = TestSpawner::new();
        let mut pool = standard_restart_pool(&spawner, 3);
        let now = base_time();

        pool.reconcile(now).await.unwrap();
        pool.reconcile(now + chrono::Duration::seconds(1))
            .await
            .unwrap();
        assert_eq!(pool.ready_count("standard"), 3);

        // Hot-reload the tier down to 1.
        pool.set_tier_size("standard", 1);
        assert_eq!(pool.tier_size("standard"), 1);

        // Too soon: the idle timeout has not elapsed, nothing is torn down.
        pool.reconcile(now + chrono::Duration::seconds(30))
            .await
            .unwrap();
        assert_eq!(pool.members("standard").len(), 3);

        // After the timeout the two oldest spares are torn down.
        let events = pool
            .reconcile(now + chrono::Duration::seconds(120))
            .await
            .unwrap();
        let torn_down: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                PoolEvent::TeardownIdle { worker_id, .. } => Some(worker_id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(torn_down, vec!["pool-standard-1", "pool-standard-2"]);
        assert_eq!(pool.ready_count("standard"), 1);
        assert!(spawner.stopped().contains(&"pool-standard-1".to_string()));
        assert!(spawner.stopped().contains(&"pool-standard-2".to_string()));

        // Disabling teardown (idle_timeout_secs = 0) keeps the excess.
        let spawner2 = TestSpawner::new();
        let mut config = pool_config(vec![tier_config("standard", 3)]);
        config.idle_timeout_secs = 0;
        let mut pool2 = WorkerPool::new(config, Arc::clone(&spawner2));
        pool2.reconcile(now).await.unwrap();
        pool2
            .reconcile(now + chrono::Duration::seconds(1))
            .await
            .unwrap();
        pool2.set_tier_size("standard", 1);
        pool2
            .reconcile(now + chrono::Duration::seconds(600))
            .await
            .unwrap();
        assert_eq!(pool2.members("standard").len(), 3);
    }

    // ------------------------------------------------------------
    // Lifecycle
    // ------------------------------------------------------------

    #[tokio::test]
    async fn test_shutdown_stops_all_members() {
        let spawner = TestSpawner::new();
        let mut pool = standard_restart_pool(&spawner, 2);
        let now = base_time();

        pool.reconcile(now).await.unwrap();
        pool.reconcile(now + chrono::Duration::seconds(1))
            .await
            .unwrap();

        pool.shutdown().await.unwrap();
        assert!(pool.members("standard").is_empty());
        assert_eq!(spawner.stopped().len(), 2);

        // Shutdown disables the pool: a caller that keeps ticking does not
        // have members resurrected.
        assert!(!pool.is_enabled());
        let events = pool
            .reconcile(now + chrono::Duration::seconds(2))
            .await
            .unwrap();
        assert!(events.is_empty());
        assert!(pool.members("standard").is_empty());
    }

    #[test]
    fn test_summaries_cover_all_configured_tiers_sorted() {
        let spawner = TestSpawner::new();
        let mut config = pool_config(vec![tier_config("standard", 2), tier_config("premium", 1)]);
        config.recovery_policy = "restart".to_string();
        let pool = WorkerPool::new(config, Arc::clone(&spawner));

        let summaries = pool.summaries();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].tier, "premium");
        assert_eq!(summaries[1].tier, "standard");
        assert_eq!(summaries[1].size, 2);
    }
}
