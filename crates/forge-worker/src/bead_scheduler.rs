//! Bead scheduler with integrated launcher pipeline.
//!
//! This module implements the scheduler described in
//! `docs/BEAD_LAUNCHER_PROTOCOL.md`:
//!
//! - **Ready-bead selection**: reads workspace bead queues and sorts them by
//!   priority (P0 first, then by task score) via [`BeadQueueReader`].
//! - **Bead → worker mapping**: every allocation is recorded, so the
//!   scheduler always knows which worker owns which bead.
//! - **Duplicate-assignment prevention**: the mapping is a lock — a bead
//!   already assigned to one worker is refused to every other worker. This is
//!   the "bead-level locking to prevent duplicate work across workers"
//!   guarantee from the README.
//! - **Launch pipeline**: fetches the bead's context, injects it into the
//!   worker prompt, and spawns the worker through [`WorkerLauncher`] with
//!   `--bead-ref=<bead-id>` (the bead-aware launcher protocol extension).
//! - **Status updates**: marks the bead in-progress on launch, closes it on
//!   completion, and reopens it when an assignment is released, so beads are
//!   never silently lost.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use forge_worker::bead_scheduler::BeadScheduler;
//! use forge_worker::{LaunchConfig, WorkerLauncher};
//!
//! # async fn example() -> forge_core::Result<()> {
//! let mut scheduler = BeadScheduler::new(Arc::new(WorkerLauncher::new()));
//! // scheduler.add_workspace("/path/to/repo")?;
//!
//! // Full pipeline: pick the highest-priority ready bead, inject its context
//! // into the prompt, and launch a worker with --bead-ref.
//! let config = LaunchConfig::new(
//!     "/path/to/bead-worker-launcher.sh",
//!     "my-session",
//!     "/path/to/repo",
//!     "sonnet",
//! );
//! if let Some(handle) = scheduler.launch_next(config).await? {
//!     // Later, when the worker finishes:
//!     scheduler.record_completion(&handle.id).await?;
//! }
//! # Ok(())
//! # }
//! ```

use crate::bead_queue::{BeadQueueReader, QueuedBead};
use crate::launcher::WorkerLauncher;
use crate::scorer::TaskScorer;
use crate::types::{LaunchConfig, SpawnRequest, WorkerHandle};
use chrono::{DateTime, Utc};
use forge_core::types::BeadId;
use forge_core::{ForgeError, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tracing::{debug, info, warn};

/// Environment variable carrying the constructed task prompt.
///
/// Per the bead-aware launcher protocol: "For interactive tools: set
/// environment variable with prompt."
pub const FORGE_TASK_PROMPT_ENV: &str = "FORGE_TASK_PROMPT";

/// Environment variable carrying the assigned bead ID.
pub const FORGE_BEAD_ID_ENV: &str = "FORGE_BEAD_ID";

/// Human-readable label for a priority level (P0 → P4).
pub fn priority_label(priority: u8) -> &'static str {
    match priority {
        0 => "Critical",
        1 => "High",
        2 => "Normal",
        3 => "Low",
        _ => "Backlog",
    }
}

/// Construct the task prompt for a bead.
///
/// This mirrors the prompt template from `docs/BEAD_LAUNCHER_PROTOCOL.md`:
/// bead ID and title, priority, type, labels, description, and workspace.
pub fn build_bead_prompt(bead: &QueuedBead) -> String {
    let labels = bead.labels.join(", ");

    format!(
        "You are working on bead {}: {}\n\
         \n\
         Priority: P{} ({})\n\
         Type: {}\n\
         Labels: {}\n\
         \n\
         Description:\n\
         {}\n\
         \n\
         Workspace: {}\n\
         \n\
         Please work on this task. When complete, your changes should be committed to git.",
        bead.id,
        bead.title,
        bead.priority,
        priority_label(bead.priority),
        bead.issue_type,
        labels,
        bead.description,
        bead.workspace.display(),
    )
}

/// Backend used to apply bead status updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeadStatusBackend {
    /// Shell out to the bead CLI in the bead's workspace (the
    /// `br update` / `br close` calls from the launcher protocol).
    Cli {
        /// Bead CLI binary name (defaults to `br`, matching the rest of
        /// the codebase).
        binary: String,
    },
    /// Record status updates in memory without executing any CLI.
    ///
    /// Updates are still tracked (see [`BeadScheduler::status_updates`]), so
    /// tests and dry-runs can assert on the pipeline without a bead store.
    DryRun,
}

impl Default for BeadStatusBackend {
    fn default() -> Self {
        Self::Cli {
            binary: "br".to_string(),
        }
    }
}

/// A bead status update applied by the scheduler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeadStatusUpdate {
    /// Bead the update applies to
    pub bead_id: BeadId,
    /// Worker the update relates to
    pub worker_id: String,
    /// What was done
    pub action: BeadStatusAction,
    /// Reason / detail recorded with the update
    pub detail: String,
    /// Whether the update was executed against the bead CLI
    /// (always `false` for [`BeadStatusBackend::DryRun`])
    pub applied: bool,
    /// When the update was applied
    pub at: DateTime<Utc>,
}

/// Status transition applied to a bead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeadStatusAction {
    /// Worker launched on the bead: `update <id> --status in_progress --assignee <worker>`
    MarkInProgress,
    /// Worker finished the bead: `close <id> --reason "Completed by <worker>"`
    Close,
    /// Assignment released without completion: `update <id> --status open` and
    /// `update <id> --assignee ""` so the bead can be reallocated
    Reopen,
}

impl std::fmt::Display for BeadStatusAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MarkInProgress => write!(f, "in_progress"),
            Self::Close => write!(f, "closed"),
            Self::Reopen => write!(f, "open"),
        }
    }
}

/// A live bead → worker assignment (the bead-level lock).
#[derive(Debug, Clone)]
pub struct WorkerBeadAssignment {
    /// Assigned bead
    pub bead_id: BeadId,
    /// Bead title (for display)
    pub bead_title: String,
    /// Bead priority (0-4)
    pub bead_priority: u8,
    /// Worker holding the assignment
    pub worker_id: String,
    /// tmux session name the worker was configured with
    pub session_name: String,
    /// Workspace containing the bead
    pub workspace: PathBuf,
    /// When the assignment was made
    pub assigned_at: DateTime<Utc>,
}

/// Record of a completed bead assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionRecord {
    /// Bead that was completed
    pub bead_id: BeadId,
    /// Worker that completed it
    pub worker_id: String,
    /// When completion was recorded
    pub completed_at: DateTime<Utc>,
    /// Time the worker held the assignment
    pub duration: Duration,
}

/// Schedules ready beads onto workers and drives the launch pipeline.
///
/// The scheduler owns the bead → worker mapping. Every launch goes through
/// [`BeadScheduler::assign_bead`], which refuses to hand an already-assigned
/// bead to a second worker, so two workers can never receive the same bead.
#[derive(Debug)]
pub struct BeadScheduler {
    /// One reader per monitored workspace
    readers: Vec<BeadQueueReader>,
    /// Authoritative bead → worker mapping (the bead-level lock)
    assignments: HashMap<BeadId, WorkerBeadAssignment>,
    /// Completed assignments, in completion order
    completions: Vec<CompletionRecord>,
    /// Every status update applied by this scheduler, in order
    status_updates: Vec<BeadStatusUpdate>,
    /// How status updates reach the bead store
    status_backend: BeadStatusBackend,
    /// Worker launcher used by the launch pipeline
    launcher: Arc<WorkerLauncher>,
}

impl BeadScheduler {
    /// Create a new scheduler backed by the given worker launcher.
    pub fn new(launcher: Arc<WorkerLauncher>) -> Self {
        Self {
            readers: Vec::new(),
            assignments: HashMap::new(),
            completions: Vec::new(),
            status_updates: Vec::new(),
            status_backend: BeadStatusBackend::default(),
            launcher,
        }
    }

    /// Override how bead status updates are applied.
    pub fn with_status_backend(mut self, backend: BeadStatusBackend) -> Self {
        self.status_backend = backend;
        self
    }

    /// Add a workspace whose bead queue the scheduler should draw from.
    pub fn add_workspace(&mut self, workspace: impl Into<PathBuf>) -> Result<()> {
        let reader = BeadQueueReader::new(workspace.into())?;
        self.readers.push(reader);
        Ok(())
    }

    /// Number of monitored workspaces.
    pub fn workspace_count(&self) -> usize {
        self.readers.len()
    }

    // =========================================================================
    // Queue inspection
    // =========================================================================

    /// Ready beads across all workspaces, highest priority first, excluding
    /// beads already assigned by this scheduler.
    pub fn ready_beads(&mut self) -> Result<Vec<QueuedBead>> {
        let scorer = TaskScorer::new();
        let mut ready: Vec<QueuedBead> = Vec::new();

        for reader in &mut self.readers {
            for bead in reader.get_ready_beads()? {
                if !self.assignments.contains_key(&bead.id) {
                    ready.push(bead);
                }
            }
        }

        ready.sort_by(|a, b| {
            let score_a = a.calculate_score(&scorer).score;
            let score_b = b.calculate_score(&scorer).score;
            score_b
                .cmp(&score_a)
                .then_with(|| a.priority.cmp(&b.priority))
                .then_with(|| a.id.cmp(&b.id))
        });

        Ok(ready)
    }

    /// The next bead to allocate: highest-priority ready bead not already
    /// assigned. `None` when the queue is empty or fully assigned.
    pub fn next_ready_bead(&mut self) -> Result<Option<QueuedBead>> {
        Ok(self.ready_beads()?.into_iter().next())
    }

    // =========================================================================
    // Mapping inspection
    // =========================================================================

    /// Whether a bead is currently assigned to a worker.
    pub fn is_assigned(&self, bead_id: &BeadId) -> bool {
        self.assignments.contains_key(bead_id)
    }

    /// The worker currently holding a bead, if any.
    pub fn assignment_for_bead(&self, bead_id: &BeadId) -> Option<&WorkerBeadAssignment> {
        self.assignments.get(bead_id)
    }

    /// The bead currently assigned to a worker, if any.
    ///
    /// Matches on worker ID or tmux session name.
    pub fn assignment_for_worker(&self, worker_id: &str) -> Option<&WorkerBeadAssignment> {
        self.assignments
            .values()
            .find(|a| a.worker_id == worker_id || a.session_name == worker_id)
    }

    /// All live assignments (the bead → worker mapping).
    pub fn assignments(&self) -> impl Iterator<Item = &WorkerBeadAssignment> {
        self.assignments.values()
    }

    /// Completed assignments, in completion order.
    pub fn completions(&self) -> &[CompletionRecord] {
        &self.completions
    }

    /// Every status update this scheduler has applied, in order.
    pub fn status_updates(&self) -> &[BeadStatusUpdate] {
        &self.status_updates
    }

    // =========================================================================
    // Assignment (mapping only, no spawn)
    // =========================================================================

    /// Assign the next ready bead to `worker_id` and return the spawn request
    /// for it.
    ///
    /// Returns `Ok(None)` when no unassigned ready bead is available. The
    /// spawn request carries the `--bead-ref` assignment and the complexity
    /// prediction, but no prompt; use [`BeadScheduler::launch_next`] for the
    /// full pipeline.
    pub fn assign_next(
        &mut self,
        worker_id: impl Into<String>,
        config: LaunchConfig,
    ) -> Result<Option<SpawnRequest>> {
        match self.next_ready_bead()? {
            Some(bead) => {
                let bead_id = bead.id.clone();
                self.assign_bead(bead_id, worker_id, config).map(Some)
            }
            None => Ok(None),
        }
    }

    /// Assign a specific bead to `worker_id`.
    ///
    /// This is the bead-level lock: if the bead is already assigned to a
    /// different worker, [`ForgeError::BeadAlreadyAssigned`] is returned and
    /// the mapping is left untouched.
    pub fn assign_bead(
        &mut self,
        bead_id: impl Into<BeadId>,
        worker_id: impl Into<String>,
        config: LaunchConfig,
    ) -> Result<SpawnRequest> {
        let (request, _bead) = self.prepare_launch(&bead_id.into(), &worker_id.into(), config)?;
        Ok(request)
    }

    // =========================================================================
    // Launch pipeline
    // =========================================================================

    /// Launch a worker on the highest-priority ready bead.
    ///
    /// Pipeline: select bead → register the assignment → build the task
    /// prompt → spawn the worker with `--bead-ref=<bead-id>` → mark the bead
    /// in-progress. Returns `Ok(None)` when no bead is available.
    pub async fn launch_next(&mut self, config: LaunchConfig) -> Result<Option<WorkerHandle>> {
        match self.next_ready_bead()? {
            Some(bead) => {
                let bead_id = bead.id.clone();
                let worker_id = format!("forge-{}-{}", bead.id, config.model);
                self.launch_bead(bead_id, worker_id, config).await.map(Some)
            }
            None => Ok(None),
        }
    }

    /// Launch a worker on a specific bead.
    ///
    /// Same pipeline as [`BeadScheduler::launch_next`], but targeting a
    /// chosen bead. Fails with [`ForgeError::BeadAlreadyAssigned`] if another
    /// worker already holds it.
    pub async fn launch_bead(
        &mut self,
        bead_id: impl Into<BeadId>,
        worker_id: impl Into<String>,
        config: LaunchConfig,
    ) -> Result<WorkerHandle> {
        let bead_id = bead_id.into();
        let worker_id = worker_id.into();

        let (mut request, bead, reader_idx) =
            self.prepare_launch_full(&bead_id, &worker_id, config)?;

        // Inject the bead context into the worker prompt (protocol step 2).
        // Headless CLIs receive the task via stdin from the launcher script;
        // interactive tools can read it from the environment.
        let prompt = build_bead_prompt(&bead);
        request
            .config
            .env
            .push((FORGE_TASK_PROMPT_ENV.to_string(), prompt));
        request
            .config
            .env
            .push((FORGE_BEAD_ID_ENV.to_string(), bead.id.clone()));

        info!(
            "Launching worker {} on bead {} ({})",
            worker_id, bead_id, bead.title
        );

        // Spawn through the launcher (protocol step 3).
        match self.launcher.spawn(request).await {
            Ok(handle) => {
                // Protocol step 4: mark the bead in-progress for this worker.
                if let Err(e) = self
                    .apply_status_update(
                        Path::new(&bead.workspace),
                        &bead_id,
                        &worker_id,
                        BeadStatusAction::MarkInProgress,
                        format!("launched on session {}", handle.session_name),
                        &handle.session_name,
                    )
                    .await
                {
                    warn!(
                        bead_id = %bead_id,
                        error = %e,
                        "Worker launched but failed to mark bead in-progress"
                    );
                }
                Ok(handle)
            }
            Err(e) => {
                // Roll the mapping back so the bead stays allocatable — a
                // failed launch must not leave a stale lock behind.
                warn!(
                    bead_id = %bead_id,
                    worker_id = %worker_id,
                    error = %e,
                    "Launch failed; releasing bead assignment"
                );
                self.assignments.remove(&bead_id);
                if let Some(reader) = self.readers.get_mut(reader_idx) {
                    reader.unassign_bead(&bead_id);
                }
                Err(e)
            }
        }
    }

    // =========================================================================
    // Completion / release
    // =========================================================================

    /// Record that a worker finished its bead.
    ///
    /// Closes the bead (`close <id> --reason "Completed by <worker>"`), clears
    /// the bead → worker mapping, and returns the completion record. The
    /// mapping is only cleared after the status update succeeds, so a failed
    /// close can be retried.
    pub async fn record_completion(&mut self, worker_id: &str) -> Result<CompletionRecord> {
        let record = self
            .assignment_for_worker(worker_id)
            .cloned()
            .ok_or_else(|| ForgeError::WorkerNotFound {
                worker_id: worker_id.to_string(),
            })?;

        info!(
            "Recording completion of bead {} by worker {}",
            record.bead_id, record.worker_id
        );

        // Protocol step 4: close the bead on completion.
        self.apply_status_update(
            &record.workspace,
            &record.bead_id,
            &record.worker_id,
            BeadStatusAction::Close,
            format!("Completed by {}", record.worker_id),
            &record.worker_id,
        )
        .await?;

        self.remove_assignment(&record.bead_id);

        let completed_at = Utc::now();
        let duration = (completed_at - record.assigned_at)
            .to_std()
            .unwrap_or_default();

        let completion = CompletionRecord {
            bead_id: record.bead_id,
            worker_id: record.worker_id,
            completed_at,
            duration,
        };
        self.completions.push(completion.clone());

        Ok(completion)
    }

    /// Release a bead from its worker without recording completion.
    ///
    /// Used when a worker fails or is stopped: the bead is reopened so
    /// another worker can pick it up, and the mapping entry is cleared.
    pub async fn release(&mut self, bead_id: impl Into<BeadId>) -> Result<WorkerBeadAssignment> {
        let bead_id = bead_id.into();
        let record =
            self.assignments
                .get(&bead_id)
                .cloned()
                .ok_or_else(|| ForgeError::BeadNotFound {
                    bead_id: bead_id.clone(),
                })?;

        info!(
            "Releasing bead {} from worker {} for reallocation",
            bead_id, record.worker_id
        );

        self.apply_status_update(
            &record.workspace,
            &bead_id,
            &record.worker_id,
            BeadStatusAction::Reopen,
            format!("released by {}", record.worker_id),
            &record.worker_id,
        )
        .await?;

        self.remove_assignment(&bead_id);
        Ok(record)
    }

    // =========================================================================
    // Internals
    // =========================================================================

    /// Register an assignment and build the spawn request for it.
    ///
    /// Enforces the duplicate-assignment lock, fetches the bead context, and
    /// keeps the per-reader assignment maps in sync.
    fn prepare_launch(
        &mut self,
        bead_id: &BeadId,
        worker_id: &str,
        config: LaunchConfig,
    ) -> Result<(SpawnRequest, QueuedBead)> {
        let (request, bead, _idx) = self.prepare_launch_full(bead_id, worker_id, config)?;
        Ok((request, bead))
    }

    /// Like [`BeadScheduler::prepare_launch`], but also returns the index of
    /// the workspace reader that holds the bead (for rollback).
    fn prepare_launch_full(
        &mut self,
        bead_id: &BeadId,
        worker_id: &str,
        config: LaunchConfig,
    ) -> Result<(SpawnRequest, QueuedBead, usize)> {
        // Duplicate-assignment prevention: the scheduler mapping is the lock.
        if let Some(existing) = self.assignments.get(bead_id)
            && existing.worker_id != worker_id
        {
            return Err(ForgeError::BeadAlreadyAssigned {
                bead_id: bead_id.clone(),
                worker_id: existing.worker_id.clone(),
            });
        }

        let reader_idx =
            self.reader_index_for_bead(bead_id)?
                .ok_or_else(|| ForgeError::BeadNotFound {
                    bead_id: bead_id.clone(),
                })?;

        // Fetch the bead context (protocol step 1).
        let bead = self.readers[reader_idx].get_bead(bead_id)?.ok_or_else(|| {
            ForgeError::BeadNotFound {
                bead_id: bead_id.clone(),
            }
        })?;

        // Build the spawn request: bead ID on the launch config (which the
        // launcher passes to the script as --bead-ref) plus the complexity
        // prediction to persist before launch.
        let mut request = self.readers[reader_idx].create_spawn_request(&bead, config);
        request.worker_id = worker_id.to_string();

        // Lock the bead at the queue level too, so direct BeadQueueReader
        // users see the same assignment.
        self.readers[reader_idx].assign_bead(bead_id.clone(), worker_id.to_string())?;

        let record = WorkerBeadAssignment {
            bead_id: bead.id.clone(),
            bead_title: bead.title.clone(),
            bead_priority: bead.priority,
            worker_id: worker_id.to_string(),
            session_name: request.config.session_name.clone(),
            workspace: bead.workspace.clone(),
            assigned_at: Utc::now(),
        };
        self.assignments.insert(bead.id.clone(), record);

        Ok((request, bead, reader_idx))
    }

    /// Find the index of the reader that contains a bead.
    fn reader_index_for_bead(&mut self, bead_id: &BeadId) -> Result<Option<usize>> {
        for (idx, reader) in self.readers.iter_mut().enumerate() {
            if reader.has_beads() && reader.get_bead(bead_id)?.is_some() {
                return Ok(Some(idx));
            }
        }
        Ok(None)
    }

    /// Drop the scheduler- and reader-level mapping for a bead.
    fn remove_assignment(&mut self, bead_id: &BeadId) {
        self.assignments.remove(bead_id);
        for reader in &mut self.readers {
            if reader.is_assigned(bead_id) {
                reader.unassign_bead(bead_id);
            }
        }
    }

    /// Apply a status update through the configured backend and record it.
    ///
    /// `assignee` is the identity recorded with `--assignee` for
    /// [`BeadStatusAction::MarkInProgress`] (the tmux session name when one
    /// exists); it is unused by the other actions.
    async fn apply_status_update(
        &mut self,
        workspace: &Path,
        bead_id: &BeadId,
        worker_id: &str,
        action: BeadStatusAction,
        detail: String,
        assignee: &str,
    ) -> Result<BeadStatusUpdate> {
        let applied = match &self.status_backend {
            BeadStatusBackend::DryRun => {
                debug!(
                    bead_id = %bead_id,
                    action = %action,
                    "Dry-run status update (not applied)"
                );
                false
            }
            BeadStatusBackend::Cli { binary } => {
                self.run_bead_cli(workspace, binary, bead_id, action, &detail, assignee)
                    .await?;
                true
            }
        };

        let update = BeadStatusUpdate {
            bead_id: bead_id.clone(),
            worker_id: worker_id.to_string(),
            action,
            detail,
            applied,
            at: Utc::now(),
        };
        self.status_updates.push(update.clone());
        Ok(update)
    }

    /// Execute the bead CLI invocation for a status action.
    ///
    /// Command shapes mirror `docs/BEAD_LAUNCHER_PROTOCOL.md` §4 and the
    /// existing `br` usage in `forge-core`'s stuck detection.
    async fn run_bead_cli(
        &self,
        workspace: &Path,
        binary: &str,
        bead_id: &BeadId,
        action: BeadStatusAction,
        detail: &str,
        assignee: &str,
    ) -> Result<()> {
        let tool = format!("{} update/close", binary);

        match action {
            BeadStatusAction::MarkInProgress => {
                self.exec_cli(
                    workspace,
                    binary,
                    &[
                        "update",
                        bead_id,
                        "--status",
                        "in_progress",
                        "--assignee",
                        assignee,
                    ],
                    &tool,
                )
                .await
            }
            BeadStatusAction::Close => {
                self.exec_cli(
                    workspace,
                    binary,
                    &["close", bead_id, "--reason", detail],
                    &tool,
                )
                .await
            }
            BeadStatusAction::Reopen => {
                self.exec_cli(
                    workspace,
                    binary,
                    &["update", bead_id, "--status", "open"],
                    &tool,
                )
                .await?;
                // Best-effort assignee clear (matches stuck detection).
                if let Err(e) = self
                    .exec_cli(
                        workspace,
                        binary,
                        &["update", bead_id, "--assignee", ""],
                        &tool,
                    )
                    .await
                {
                    warn!(bead_id = %bead_id, error = %e, "Failed to clear assignee on reopen");
                }
                Ok(())
            }
        }
    }

    async fn exec_cli(
        &self,
        workspace: &Path,
        binary: &str,
        args: &[&str],
        tool: &str,
    ) -> Result<()> {
        let output = Command::new(binary)
            .args(args)
            .current_dir(workspace)
            .output()
            .await
            .map_err(|e| ForgeError::io(format!("running {}", tool).as_str(), workspace, e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ForgeError::ToolExecution {
                tool_name: tool.to_string(),
                message: stderr.to_string(),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::types::WorkerTier;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    /// Workspace with ready beads at several priorities plus a blocked one.
    fn create_multi_priority_workspace() -> TempDir {
        let dir = TempDir::new().unwrap();
        let beads_dir = dir.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();

        let mut file = fs::File::create(beads_dir.join("issues.jsonl")).unwrap();
        writeln!(file, r#"{{"id":"p-low","title":"Low priority task","description":"Do it later","status":"open","priority":3,"issue_type":"task","labels":[],"dependencies":[]}}"#).unwrap();
        writeln!(file, r#"{{"id":"p-high","title":"High priority bug","description":"Fix it now","status":"open","priority":0,"issue_type":"bug","labels":["urgent"],"dependencies":[]}}"#).unwrap();
        writeln!(file, r#"{{"id":"p-blocked","title":"Blocked task","description":"Waiting","status":"open","priority":0,"issue_type":"task","labels":[],"dependencies":["p-high"]}}"#).unwrap();

        dir
    }

    fn scheduler_for(dir: &TempDir) -> BeadScheduler {
        let mut scheduler = BeadScheduler::new(Arc::new(WorkerLauncher::new()))
            .with_status_backend(BeadStatusBackend::DryRun);
        scheduler.add_workspace(dir.path()).unwrap();
        scheduler
    }

    fn launch_config(dir: &TempDir) -> LaunchConfig {
        LaunchConfig::new(
            "/path/to/launcher.sh",
            "test-session",
            dir.path().to_path_buf(),
            "sonnet",
        )
    }

    #[test]
    fn test_priority_label() {
        assert_eq!(priority_label(0), "Critical");
        assert_eq!(priority_label(1), "High");
        assert_eq!(priority_label(2), "Normal");
        assert_eq!(priority_label(3), "Low");
        assert_eq!(priority_label(4), "Backlog");
        assert_eq!(priority_label(9), "Backlog");
    }

    #[test]
    fn test_build_bead_prompt_contains_context() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);
        let bead = scheduler
            .next_ready_bead()
            .unwrap()
            .expect("queue should not be empty");

        let prompt = build_bead_prompt(&bead);
        assert!(prompt.contains("You are working on bead p-high: High priority bug"));
        assert!(prompt.contains("Priority: P0 (Critical)"));
        assert!(prompt.contains("Type: bug"));
        assert!(prompt.contains("Labels: urgent"));
        assert!(prompt.contains("Fix it now"));
        assert!(prompt.contains(dir.path().to_str().unwrap()));
        assert!(prompt.contains("committed to git"));
    }

    #[test]
    fn test_assign_next_picks_highest_priority() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        let request = scheduler
            .assign_next("worker-a", launch_config(&dir))
            .unwrap()
            .expect("a bead should be available");

        // P0 bug beats the P3 task, and the blocked bead is never chosen.
        assert_eq!(request.config.bead_id.as_deref(), Some("p-high"));
        assert!(scheduler.is_assigned(&"p-high".to_string()));

        let assignment = scheduler
            .assignment_for_bead(&"p-high".to_string())
            .unwrap();
        assert_eq!(assignment.worker_id, "worker-a");
        assert_eq!(assignment.bead_title, "High priority bug");
        assert_eq!(assignment.bead_priority, 0);
        assert_eq!(assignment.workspace, dir.path());
    }

    #[test]
    fn test_assign_next_returns_none_when_queue_exhausted() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        // Two ready beads, both claimed.
        scheduler
            .assign_next("worker-a", launch_config(&dir))
            .unwrap();
        scheduler
            .assign_next("worker-b", launch_config(&dir))
            .unwrap();

        assert!(
            scheduler
                .assign_next("worker-c", launch_config(&dir))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn test_empty_queue_returns_none() {
        let dir = TempDir::new().unwrap();
        let mut scheduler = scheduler_for(&dir);

        assert!(scheduler.next_ready_bead().unwrap().is_none());
        assert!(
            scheduler
                .assign_next("worker-a", launch_config(&dir))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn test_duplicate_assignment_prevented() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        scheduler
            .assign_bead("p-high", "worker-a", launch_config(&dir))
            .unwrap();

        // A second worker must be refused: the mapping is the lock.
        let err = scheduler
            .assign_bead("p-high", "worker-b", launch_config(&dir))
            .unwrap_err();
        match err {
            ForgeError::BeadAlreadyAssigned {
                ref bead_id,
                ref worker_id,
            } => {
                assert_eq!(bead_id, "p-high");
                assert_eq!(worker_id, "worker-a");
            }
            other => panic!("expected BeadAlreadyAssigned, got: {}", other),
        }

        // And assign_next must skip the locked bead rather than double-book it.
        let request = scheduler
            .assign_next("worker-b", launch_config(&dir))
            .unwrap()
            .expect("the P3 bead should still be available");
        assert_eq!(request.config.bead_id.as_deref(), Some("p-low"));

        assert!(scheduler.is_assigned(&"p-high".to_string()));
        assert_eq!(
            scheduler
                .assignment_for_bead(&"p-high".to_string())
                .unwrap()
                .worker_id,
            "worker-a"
        );
    }

    #[test]
    fn test_assign_bead_idempotent_for_same_worker() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        scheduler
            .assign_bead("p-high", "worker-a", launch_config(&dir))
            .unwrap();
        scheduler
            .assign_bead("p-high", "worker-a", launch_config(&dir))
            .unwrap();

        assert_eq!(scheduler.assignments().count(), 1);
    }

    #[test]
    fn test_spawn_request_carries_bead_ref_and_worker_id() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        let request = scheduler
            .assign_next("my-worker", launch_config(&dir))
            .unwrap()
            .unwrap();

        // The caller's worker id is preserved so the mapping and the spawned
        // session agree.
        assert_eq!(request.worker_id, "my-worker");
        // The bead rides on the launch config; the launcher forwards it to
        // the launcher script as --bead-ref=<bead-id>.
        assert_eq!(request.config.bead_id.as_deref(), Some("p-high"));
        assert!(request.config.has_bead());
        // The complexity prediction is attached so it persists before launch.
        assert!(request.task_assignment.is_some());
        assert_eq!(request.task_assignment.as_ref().unwrap().bead_id, "p-high");
    }

    #[test]
    fn test_assign_unknown_bead_is_error() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        let result = scheduler.assign_bead("no-such-bead", "worker-a", launch_config(&dir));
        assert!(matches!(result, Err(ForgeError::BeadNotFound { .. })));
        assert!(scheduler.assignments().next().is_none());
    }

    #[test]
    fn test_assignment_for_worker_matches_session_name() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        let mut config = launch_config(&dir);
        config.session_name = "forge-special-session".to_string();
        scheduler.assign_bead("p-high", "worker-a", config).unwrap();

        assert!(scheduler.assignment_for_worker("worker-a").is_some());
        assert!(
            scheduler
                .assignment_for_worker("forge-special-session")
                .is_some()
        );
        assert!(scheduler.assignment_for_worker("worker-zzz").is_none());
    }

    #[tokio::test]
    async fn test_record_completion_clears_mapping_and_records() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        let request = scheduler
            .assign_next("worker-a", launch_config(&dir))
            .unwrap()
            .unwrap();

        let completion = scheduler.record_completion("worker-a").await.unwrap();

        assert_eq!(completion.bead_id, "p-high");
        assert_eq!(completion.worker_id, "worker-a");
        // The assignment was held only for the instant of the test.
        assert!(completion.duration <= Duration::from_secs(5));

        // Mapping cleared, completion recorded, status pipeline ran.
        assert!(!scheduler.is_assigned(&"p-high".to_string()));
        assert_eq!(scheduler.completions().len(), 1);

        // Only the close is recorded here: the bead went through assign_next
        // (no launch), so the scheduler never marked it in-progress itself.
        let updates = scheduler.status_updates();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].action, BeadStatusAction::Close);
        assert_eq!(updates[0].detail, "Completed by worker-a");
        // DryRun backend records but does not apply.
        assert!(!updates[0].applied);

        // The spawn request built earlier still names the bead (--bead-ref).
        assert_eq!(request.config.bead_id.as_deref(), Some("p-high"));
    }

    #[tokio::test]
    async fn test_record_completion_unknown_worker() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        let result = scheduler.record_completion("ghost-worker").await;
        assert!(matches!(result, Err(ForgeError::WorkerNotFound { .. })));
    }

    #[tokio::test]
    async fn test_record_completion_by_session_name() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        scheduler
            .assign_bead("p-high", "worker-a", launch_config(&dir))
            .unwrap();

        let completion = scheduler
            .record_completion("test-session")
            .await
            .expect("session name should resolve to the assignment");
        assert_eq!(completion.bead_id, "p-high");
    }

    #[tokio::test]
    async fn test_release_frees_bead_for_reassignment() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        scheduler
            .assign_bead("p-high", "worker-a", launch_config(&dir))
            .unwrap();

        let released = scheduler.release("p-high").await.unwrap();
        assert_eq!(released.worker_id, "worker-a");
        assert!(!scheduler.is_assigned(&"p-high".to_string()));

        // Reopen recorded in the status pipeline.
        let updates = scheduler.status_updates();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].action, BeadStatusAction::Reopen);
        assert_eq!(updates[0].action.to_string(), "open");

        // And the bead can be handed to a different worker.
        let request = scheduler
            .assign_next("worker-b", launch_config(&dir))
            .unwrap()
            .expect("released bead should be allocatable");
        assert_eq!(request.config.bead_id.as_deref(), Some("p-high"));
    }

    #[tokio::test]
    async fn test_release_unknown_bead() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        let result = scheduler.release("no-such-bead").await;
        assert!(matches!(result, Err(ForgeError::BeadNotFound { .. })));
    }

    #[tokio::test]
    async fn test_launch_failure_rolls_back_mapping() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        // The launcher script does not exist, so spawn fails after the
        // assignment step — the lock must not survive the failure.
        let config = LaunchConfig::new(
            "/path/to/missing-launcher.sh",
            "test-session",
            dir.path().to_path_buf(),
            "sonnet",
        );

        let worker_id = "forge-p-high-sonnet".to_string();
        let result = scheduler
            .launch_bead("p-high", worker_id.clone(), config)
            .await;
        assert!(result.is_err());
        assert!(!scheduler.is_assigned(&"p-high".to_string()));

        // The bead is allocatable again despite the failed launch.
        let request = scheduler
            .assign_next("worker-b", launch_config(&dir))
            .unwrap()
            .expect("bead should be allocatable after rollback");
        assert_eq!(request.config.bead_id.as_deref(), Some("p-high"));
        assert_eq!(worker_id, "forge-p-high-sonnet");
    }

    #[tokio::test]
    async fn test_launch_bead_unknown_bead() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        let result = scheduler
            .launch_bead("no-such-bead", "worker-a", launch_config(&dir))
            .await;
        assert!(matches!(result, Err(ForgeError::BeadNotFound { .. })));
    }

    #[test]
    fn test_default_status_backend_is_cli() {
        let scheduler = BeadScheduler::new(Arc::new(WorkerLauncher::new()));
        assert_eq!(
            scheduler.status_backend,
            BeadStatusBackend::Cli {
                binary: "br".to_string()
            }
        );
    }

    #[test]
    fn test_worker_tier_flows_into_spawn_request() {
        let dir = create_multi_priority_workspace();
        let mut scheduler = scheduler_for(&dir);

        let config = launch_config(&dir).with_tier(WorkerTier::Premium);
        let request = scheduler.assign_next("worker-a", config).unwrap().unwrap();
        assert_eq!(request.config.tier, WorkerTier::Premium);
    }
}
