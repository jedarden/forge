//! Bead data management for the FORGE TUI.
//!
//! This module provides functionality for querying beads from monitored workspaces
//! by reading the bead store directly. It periodically polls for bead status and
//! caches the results for display in the task queue.
//!
//! ## Architecture
//!
//! The BeadManager:
//! 1. Maintains a list of monitored workspace paths
//! 2. Periodically reads the bead store (bead-rs checkpoint, or the legacy flat
//!    file) via [`forge_core::bead_store`] — no CLI subprocess, so reads cannot
//!    block the UI and also work on a fresh clone where `beads.db` has not been
//!    restored yet
//! 3. Computes ready/blocked/in-progress buckets and statistics locally, using
//!    the same readiness semantics as `bead list --ready`
//! 4. Leaves all store mutations to the `bead` CLI (sole write authority —
//!    ADR 0007/0020); the only write path here is timing out stuck tasks
//!
//! Ready means: open, not deferred, no unfinished blocker, and not manually
//! blocked. Assigned-but-open beads remain visible in the queue with their
//! assignee, while only unassigned beads are offered to the scheduler.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use forge_worker::bead_scheduler::{AssignmentFeed, WorkerBeadAssignment};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

// Re-export TaskScorer for use in TUI
pub use forge_worker::scorer::{ScoreComponents, ScoredBead, TaskScorer};

// Re-export stuck detection for use in TUI
pub use forge_core::stuck_detection::{
    ActivityChecks, StuckDetectionConfig, StuckTask, StuckTaskDetector,
};

/// Default polling interval in seconds for bead updates.
const DEFAULT_POLL_INTERVAL_SECS: u64 = 30; // Increased from 5 to reduce blocking

/// Maximum age before considering cached data stale (in seconds).
const CACHE_STALE_SECS: u64 = 60; // Increased from 30

/// Errors that can occur during bead operations.
#[derive(Error, Debug)]
pub enum BeadError {
    /// Failed to read the bead store
    #[error("Failed to read bead store: {0}")]
    StoreRead(#[from] forge_core::ForgeError),

    /// Workspace has no .beads directory
    #[error("Workspace has no .beads directory: {0}")]
    NoBeadsDirectory(PathBuf),
}

/// Result type for bead operations.
pub type BeadResult<T> = Result<T, BeadError>;

/// A bead/issue as read from the bead store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bead {
    /// Unique bead identifier (e.g., "fg-1r1")
    pub id: String,

    /// Title of the bead
    pub title: String,

    /// Description of the bead
    #[serde(default)]
    pub description: String,

    /// Current status (open, in_progress, closed)
    pub status: String,

    /// Priority (0-4, where 0 is critical)
    #[serde(default)]
    pub priority: u8,

    /// Issue type (task, bug, feature, etc.)
    #[serde(default)]
    pub issue_type: String,

    /// Assignee (if any)
    #[serde(default)]
    pub assignee: Option<String>,

    /// Labels
    #[serde(default)]
    pub labels: Vec<String>,

    /// Number of unfinished blockers on this bead
    #[serde(default)]
    pub dependency_count: usize,

    /// Number of beads that depend on this one
    #[serde(default)]
    pub dependent_count: usize,

    /// Bead is manually marked as blocked
    #[serde(default)]
    pub manual_blocked: bool,

    /// Creation timestamp
    #[serde(default)]
    pub created_at: String,

    /// Last update timestamp
    #[serde(default)]
    pub updated_at: String,
}

impl Bead {
    /// Check if this bead is ready to work on (not blocked, not deferred, not closed).
    pub fn is_ready(&self) -> bool {
        !self.is_blocked() && !self.is_deferred() && !self.is_closed()
    }

    /// Check if this bead is blocked (by dependencies or manually).
    pub fn is_blocked(&self) -> bool {
        self.dependency_count > 0 || self.manual_blocked
    }

    /// Check if this bead is deferred.
    pub fn is_deferred(&self) -> bool {
        self.status == "deferred"
    }

    /// Check if this bead is in progress.
    pub fn is_in_progress(&self) -> bool {
        self.status == "in_progress"
    }

    /// Check if this bead is closed.
    pub fn is_closed(&self) -> bool {
        self.status == "closed"
    }

    /// Get the priority display string.
    pub fn priority_str(&self) -> String {
        format!("P{}", self.priority)
    }

    /// Get the status indicator for display.
    pub fn status_indicator(&self) -> &'static str {
        match self.status.as_str() {
            "open" => "○",
            "in_progress" => "●",
            "closed" => "✓",
            "blocked" => "⊘",
            "deferred" => "⏸",
            _ => "?",
        }
    }

    /// Get the priority indicator for display.
    pub fn priority_indicator(&self) -> &'static str {
        priority_indicator(self.priority)
    }

    /// Check if this bead matches a search query (case-insensitive substring match).
    /// Matches against id, title, description, labels, and issue_type.
    pub fn matches_search(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }

        let query_lower = query.to_lowercase();

        // Check ID
        if self.id.to_lowercase().contains(&query_lower) {
            return true;
        }

        // Check title
        if self.title.to_lowercase().contains(&query_lower) {
            return true;
        }

        // Check description
        if self.description.to_lowercase().contains(&query_lower) {
            return true;
        }

        // Check issue type
        if self.issue_type.to_lowercase().contains(&query_lower) {
            return true;
        }

        // Check labels
        for label in &self.labels {
            if label.to_lowercase().contains(&query_lower) {
                return true;
            }
        }

        // Check assignee
        if let Some(ref assignee) = self.assignee {
            if assignee.to_lowercase().contains(&query_lower) {
                return true;
            }
        }

        false
    }

    /// Calculate the task value score (0-100).
    ///
    /// Uses the TaskScorer to compute a score based on:
    /// - Priority (40% weight)
    /// - Blockers (30% weight)
    /// - Age (20% weight)
    /// - Labels (10% weight)
    pub fn calculate_score(&self) -> ScoredBead {
        let scorer = TaskScorer::new();

        // Parse age from created_at timestamp
        let age_hours = if !self.created_at.is_empty() {
            TaskScorer::parse_age_hours(&self.created_at)
        } else {
            None
        };

        scorer.score_with_components(self.priority, self.dependent_count, age_hours, &self.labels)
    }

    /// Get the score as a simple integer.
    pub fn score(&self) -> u32 {
        self.calculate_score().score
    }

    /// Get the score as a simple integer.
    pub fn score_display(&self) -> String {
        format!("{:3}", self.score())
    }
}

/// Statistics computed from a workspace's bead store contents.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BeadStats {
    /// Summary statistics
    pub summary: BeadSummary,
}

/// Summary of bead counts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BeadSummary {
    /// Total number of issues
    pub total_issues: usize,

    /// Number of open issues
    pub open_issues: usize,

    /// Number of in-progress issues
    pub in_progress_issues: usize,

    /// Number of closed issues
    pub closed_issues: usize,

    /// Number of blocked issues
    pub blocked_issues: usize,

    /// Number of deferred issues
    pub deferred_issues: usize,

    /// Number of ready issues (unblocked, not deferred)
    pub ready_issues: usize,
}

/// Cached bead data for a workspace.
#[derive(Debug, Default)]
pub struct WorkspaceBeads {
    /// Path to the workspace
    pub path: PathBuf,

    /// Workspace name (last component of path)
    pub name: String,

    /// List of ready beads
    pub ready: Vec<Bead>,

    /// List of blocked beads
    pub blocked: Vec<Bead>,

    /// List of in-progress beads
    pub in_progress: Vec<Bead>,

    /// Statistics computed from the store contents
    pub stats: BeadStats,

    /// Last successful update timestamp
    pub last_update: Option<Instant>,

    /// Last error (if any)
    pub last_error: Option<String>,
}

impl WorkspaceBeads {
    /// Create a new workspace beads cache for a path.
    pub fn new(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        Self {
            path,
            name,
            ..Default::default()
        }
    }

    /// Check if the cached data is stale.
    pub fn is_stale(&self) -> bool {
        self.last_update
            .map_or(true, |t| t.elapsed().as_secs() > CACHE_STALE_SECS)
    }

    /// Get total count of actionable beads (ready + in_progress).
    pub fn actionable_count(&self) -> usize {
        self.ready.len() + self.in_progress.len()
    }
}

/// Aggregated bead data across all monitored workspaces.
#[derive(Debug, Default)]
pub struct AggregatedBeadData {
    /// Ready beads across all workspaces (sorted by priority)
    pub ready: Vec<(String, Bead)>, // (workspace_name, bead)

    /// Blocked beads across all workspaces
    pub blocked: Vec<(String, Bead)>,

    /// In-progress beads across all workspaces
    pub in_progress: Vec<(String, Bead)>,

    /// Aggregate statistics
    pub total_ready: usize,
    pub total_blocked: usize,
    pub total_in_progress: usize,
    pub total_open: usize,
}

impl AggregatedBeadData {
    /// Format summary counts for display.
    pub fn format_summary(&self) -> String {
        format!(
            "Ready: {} | In Progress: {} | Blocked: {} | Total Open: {}",
            self.total_ready, self.total_in_progress, self.total_blocked, self.total_open
        )
    }
}

/// Manager for querying and caching bead data.
pub struct BeadManager {
    /// Monitored workspace paths
    workspaces: Vec<PathBuf>,

    /// Cached bead data per workspace
    cache: HashMap<PathBuf, WorkspaceBeads>,

    /// Live scheduler assignments (bead dispatch), when a feed is
    /// registered. `None` while dispatch is disabled or not wired — the
    /// panels then render exactly as before.
    assignment_feed: Option<AssignmentFeed>,

    /// Assignments cached from the last feed refresh, for change detection
    /// and rendering
    assignments: Vec<WorkerBeadAssignment>,

    /// Last poll timestamp
    last_poll: Option<Instant>,

    /// Polling interval
    poll_interval: Duration,

    /// Whether any monitored workspace has a detectable bead store
    store_available: Option<bool>,

    /// Stuck task detector
    stuck_detector: StuckTaskDetector,

    /// Cached stuck tasks
    stuck_tasks: Vec<StuckTask>,
}

impl Default for BeadManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BeadManager {
    /// Create a new bead manager with default settings.
    pub fn new() -> Self {
        Self {
            workspaces: Vec::new(),
            cache: HashMap::new(),
            assignment_feed: None,
            assignments: Vec::new(),
            last_poll: None,
            poll_interval: Duration::from_secs(DEFAULT_POLL_INTERVAL_SECS),
            store_available: None,
            stuck_detector: StuckTaskDetector::with_defaults(),
            stuck_tasks: Vec::new(),
        }
    }

    /// Add a workspace to monitor.
    pub fn add_workspace(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        if !self.workspaces.contains(&path) {
            self.workspaces.push(path.clone());
            self.cache
                .insert(path.clone(), WorkspaceBeads::new(path.clone()));
            self.stuck_detector.add_workspace(path);
            // A manager may be initialized before workspaces are discovered,
            // or a workspace may gain its checkpoint after the first poll.
            // Force format detection to run again in either case.
            self.store_available = None;
        }
    }

    /// Add multiple workspaces from environment or default paths.
    pub fn add_default_workspaces(&mut self) {
        // Check for FORGE_WORKSPACES environment variable
        if let Ok(workspaces) = std::env::var("FORGE_WORKSPACES") {
            for path in workspaces.split(':') {
                let path = PathBuf::from(path);
                if path.exists() {
                    self.add_workspace(path);
                }
            }
        }

        // Also check current directory
        if let Ok(cwd) = std::env::current_dir() {
            if cwd.join(".beads").exists() {
                self.add_workspace(cwd);
            }
        }

        // Check for common workspace locations
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);

            // Check ~/forge if it exists and has .beads
            let forge_dir = home.join("forge");
            if forge_dir.join(".beads").exists() {
                self.add_workspace(forge_dir);
            }
        }
    }

    /// Check whether any monitored workspace has a detectable bead store.
    pub fn is_bead_store_available(&mut self) -> bool {
        if let Some(available) = self.store_available {
            return available;
        }

        let available = self
            .workspaces
            .iter()
            .any(|ws| forge_core::bead_store::detect_format(ws).is_some());

        self.store_available = Some(available);
        available
    }

    /// Register the live scheduler assignment feed (bead dispatch).
    ///
    /// The feed is the shared handle from
    /// [`forge_worker::bead_scheduler::BeadScheduler::assignments_feed`];
    /// the manager refreshes it on every poll and renders each live
    /// bead → worker assignment alongside the queue state. A manager
    /// without a feed — opt-in dispatch disabled or not wired — keeps
    /// rendering the queue exactly as before.
    pub fn set_assignment_feed(&mut self, feed: AssignmentFeed) {
        self.assignment_feed = Some(feed);
        self.refresh_assignments();
    }

    /// Whether a live scheduler assignment feed is registered.
    pub fn has_assignment_feed(&self) -> bool {
        self.assignment_feed.is_some()
    }

    /// The assignments cached from the last feed refresh, in the
    /// scheduler's published order (priority, then bead ID).
    pub fn assignments(&self) -> &[WorkerBeadAssignment] {
        &self.assignments
    }

    /// Read the live feed into the cache. Returns `true` when the
    /// assignment set changed and the panel should redraw.
    fn refresh_assignments(&mut self) -> bool {
        let Some(feed) = &self.assignment_feed else {
            return false;
        };
        let current = feed.current();
        if current == self.assignments {
            return false;
        }
        self.assignments = current;
        true
    }

    /// Poll for bead updates if the polling interval has elapsed.
    /// Returns true if any bead data changed.
    pub fn poll_updates(&mut self) -> bool {
        // Refresh live scheduler assignments (bead dispatch) on every call:
        // the shared read is cheap, and dispatched assignments identify
        // running workers even when the bead store below is unavailable.
        let assignments_changed = self.refresh_assignments();

        // Check if it's time to poll
        if let Some(last_poll) = self.last_poll {
            if last_poll.elapsed() < self.poll_interval {
                return assignments_changed;
            }
        }

        // Don't poll if no workspace has a bead store
        if !self.is_bead_store_available() {
            return assignments_changed;
        }

        self.last_poll = Some(Instant::now());

        let mut changed = assignments_changed;

        // Poll each workspace
        for workspace in self.workspaces.clone() {
            if self.poll_workspace(&workspace) {
                changed = true;
            }
        }

        // Detect stuck tasks
        if let Ok(stuck) = self.stuck_detector.detect_stuck_tasks() {
            if stuck != self.stuck_tasks {
                self.stuck_tasks = stuck;
                changed = true;
            }
        }

        changed
    }

    /// Poll a single workspace for bead updates.
    /// Returns true if data changed.
    fn poll_workspace(&mut self, workspace: &PathBuf) -> bool {
        let cache = self
            .cache
            .entry(workspace.clone())
            .or_insert_with(|| WorkspaceBeads::new(workspace.clone()));

        let beads = match forge_core::bead_store::read_all_beads(workspace) {
            Ok(beads) => beads,
            Err(e) => {
                debug!(workspace = ?workspace, error = %e, "Failed to read bead store");
                cache.last_error = Some(e.to_string());
                return false;
            }
        };
        cache.last_error = None;

        let index = forge_core::bead_store::build_index(&beads);

        let mut ready = Vec::new();
        let mut blocked = Vec::new();
        let mut in_progress = Vec::new();
        let mut summary = BeadSummary::default();

        for store_bead in &beads {
            summary.total_issues += 1;
            match store_bead.status.as_str() {
                "closed" => {
                    summary.closed_issues += 1;
                    continue;
                }
                "deferred" => {
                    summary.deferred_issues += 1;
                    continue;
                }
                "in_progress" => summary.in_progress_issues += 1,
                _ => summary.open_issues += 1,
            }

            let bead = to_bead(store_bead, &beads, &index);
            if bead.status == "in_progress" {
                in_progress.push(bead);
            } else if bead.is_blocked() {
                summary.blocked_issues += 1;
                blocked.push(bead);
            } else {
                summary.ready_issues += 1;
                ready.push(bead);
            }
        }

        let mut changed = false;

        if cache.ready != ready {
            cache.ready = ready;
            changed = true;
        }
        if cache.blocked != blocked {
            cache.blocked = blocked;
            changed = true;
        }
        if cache.in_progress != in_progress {
            cache.in_progress = in_progress;
            changed = true;
        }
        if cache.stats.summary != summary {
            cache.stats = BeadStats { summary };
            changed = true;
        }

        cache.last_update = Some(Instant::now());
        changed
    }

    /// Get total bead count across all workspaces.
    pub fn total_bead_count(&self) -> usize {
        self.cache
            .values()
            .map(|w| w.ready.len() + w.blocked.len() + w.in_progress.len())
            .sum()
    }

    /// Get aggregated bead data across all workspaces.
    pub fn get_aggregated_data(&self) -> AggregatedBeadData {
        self.get_filtered_aggregated_data(None)
    }

    /// Get aggregated bead data, optionally filtered by priority.
    ///
    /// When `priority_filter` is Some(p), only beads with priority == p are included.
    /// When `priority_filter` is None, all beads are included.
    pub fn get_filtered_aggregated_data(&self, priority_filter: Option<u8>) -> AggregatedBeadData {
        self.get_filtered_aggregated_data_with_search(priority_filter, "")
    }

    /// Get aggregated bead data, optionally filtered by priority and search query.
    ///
    /// When `priority_filter` is Some(p), only beads with priority == p are included.
    /// When `priority_filter` is None, all beads are included.
    /// When `search_query` is non-empty, only beads matching the query are included.
    pub fn get_filtered_aggregated_data_with_search(
        &self,
        priority_filter: Option<u8>,
        search_query: &str,
    ) -> AggregatedBeadData {
        let mut data = AggregatedBeadData::default();

        for (_, cache) in &self.cache {
            // Add ready beads (filtered)
            for bead in &cache.ready {
                if priority_filter.map_or(true, |p| bead.priority == p)
                    && bead.matches_search(search_query)
                {
                    data.ready.push((cache.name.clone(), bead.clone()));
                }
            }

            // Add blocked beads (filtered)
            for bead in &cache.blocked {
                if priority_filter.map_or(true, |p| bead.priority == p)
                    && bead.matches_search(search_query)
                {
                    data.blocked.push((cache.name.clone(), bead.clone()));
                }
            }

            // Add in-progress beads (filtered)
            for bead in &cache.in_progress {
                if priority_filter.map_or(true, |p| bead.priority == p)
                    && bead.matches_search(search_query)
                {
                    data.in_progress.push((cache.name.clone(), bead.clone()));
                }
            }

            // Aggregate counts (always use totals regardless of filter for summary)
            data.total_ready += cache.stats.summary.ready_issues;
            data.total_blocked += cache.stats.summary.blocked_issues;
            data.total_in_progress += cache.stats.summary.in_progress_issues;
            data.total_open +=
                cache.stats.summary.open_issues + cache.stats.summary.in_progress_issues;
        }

        // Sort by score (highest first), then priority for stable ordering
        data.ready.sort_by(|a, b| {
            let score_a = a.1.calculate_score().score;
            let score_b = b.1.calculate_score().score;
            score_b
                .cmp(&score_a)
                .then_with(|| a.1.priority.cmp(&b.1.priority))
        });
        data.in_progress.sort_by(|a, b| {
            let score_a = a.1.calculate_score().score;
            let score_b = b.1.calculate_score().score;
            score_b
                .cmp(&score_a)
                .then_with(|| a.1.priority.cmp(&b.1.priority))
        });
        data.blocked.sort_by(|a, b| {
            let score_a = a.1.calculate_score().score;
            let score_b = b.1.calculate_score().score;
            score_b
                .cmp(&score_a)
                .then_with(|| a.1.priority.cmp(&b.1.priority))
        });

        data
    }

    /// Get the total count of actionable beads (ready + in_progress + blocked), filtered by priority.
    pub fn task_count_filtered(&self, priority_filter: Option<u8>) -> usize {
        let data = self.get_filtered_aggregated_data(priority_filter);
        data.ready.len() + data.in_progress.len() + data.blocked.len()
    }

    /// Get the total count of actionable beads (ready + in_progress + blocked), filtered by priority and search query.
    pub fn task_count_filtered_with_search(
        &self,
        priority_filter: Option<u8>,
        search_query: &str,
    ) -> usize {
        let data = self.get_filtered_aggregated_data_with_search(priority_filter, search_query);
        data.ready.len() + data.in_progress.len() + data.blocked.len()
    }

    /// Check if any data is loaded.
    pub fn is_loaded(&self) -> bool {
        self.cache.values().any(|c| c.last_update.is_some())
    }

    /// Check whether a bead store was found in any monitored workspace.
    pub fn has_bead_store(&self) -> bool {
        self.store_available.unwrap_or(false)
    }

    /// Get the number of monitored workspaces.
    pub fn workspace_count(&self) -> usize {
        self.workspaces.len()
    }

    /// Format task queue summary for the overview panel.
    pub fn format_task_queue_summary(&self) -> String {
        if !self.has_bead_store() {
            return "No bead store found.\n\n\
                    Install bead-rs to enable the task queue:\n\
                    https://git.ardenone.com/jedarden/bead-rs"
                .to_string();
        }

        if self.workspaces.is_empty() {
            return "No workspaces configured.\n\n\
                    Set FORGE_WORKSPACES or run forge from\n\
                    a directory with a .beads/ folder."
                .to_string();
        }

        if !self.is_loaded() {
            return "Loading bead data...".to_string();
        }

        let data = self.get_aggregated_data();

        let mut lines = Vec::new();

        // Summary line
        lines.push(data.format_summary());
        lines.push(String::new());

        // Live scheduler assignments (bead dispatch): bead → worker pairs.
        // Shown only when present, so an idle or disabled dispatcher keeps
        // the compact summary unchanged.
        if !self.assignments.is_empty() {
            lines.push(format!("Assigned: {}", self.assignments.len()));
            for assignment in self.assignments.iter().take(3) {
                lines.push(format!(
                    "  ◆ {} → {}",
                    truncate_str(&assignment.bead_id, 14),
                    truncate_str(&assignment.worker_id, 20)
                ));
            }
            if self.assignments.len() > 3 {
                lines.push(format!("  ... and {} more", self.assignments.len() - 3));
            }
            lines.push(String::new());
        }

        // Show top ready beads
        if !data.ready.is_empty() {
            lines.push("Ready:".to_string());
            for (ws, bead) in data.ready.iter().take(3) {
                lines.push(format!(
                    "  {} {} {} [{}]",
                    bead.priority_indicator(),
                    bead.id,
                    truncate_str(&bead.title, 25),
                    ws
                ));
            }
            if data.ready.len() > 3 {
                lines.push(format!("  ... and {} more", data.ready.len() - 3));
            }
            lines.push(String::new());
        }

        // Show in-progress beads
        if !data.in_progress.is_empty() {
            lines.push("In Progress:".to_string());
            for (_ws, bead) in data.in_progress.iter().take(3) {
                let assignee = bead.assignee.as_deref().unwrap_or("-");
                lines.push(format!(
                    "  ● {} {} [{}]",
                    bead.id,
                    truncate_str(&bead.title, 20),
                    truncate_str(assignee, 10)
                ));
            }
            if data.in_progress.len() > 3 {
                lines.push(format!("  ... and {} more", data.in_progress.len() - 3));
            }
        }

        if lines.is_empty() || (data.ready.is_empty() && data.in_progress.is_empty()) {
            lines.push("No pending tasks.".to_string());
        }

        lines.join("\n")
    }

    /// Format full task queue for the Tasks view.
    pub fn format_task_queue_full(&self) -> String {
        self.format_task_queue_full_filtered(None)
    }

    /// Format full task queue for the Tasks view with optional priority filter.
    ///
    /// When `priority_filter` is Some(p), only beads with priority == p are shown.
    /// When `priority_filter` is None, all beads are shown.
    pub fn format_task_queue_full_filtered(&self, priority_filter: Option<u8>) -> String {
        self.format_task_queue_full_filtered_with_search(priority_filter, "")
    }

    /// Get stuck tasks detected by the stuck task detector.
    pub fn get_stuck_tasks(&self) -> &[StuckTask] {
        &self.stuck_tasks
    }

    /// Timeout a stuck task, making it available for reassignment.
    pub fn timeout_stuck_task(&self, task: &StuckTask) -> Result<(), String> {
        self.stuck_detector
            .timeout_task(&task.workspace, &task.bead_id)
            .map_err(|e| format!("Failed to timeout task: {}", e))
    }

    /// Check if a bead is marked as stuck.
    pub fn is_bead_stuck(&self, bead_id: &str) -> bool {
        self.stuck_tasks.iter().any(|t| t.bead_id == bead_id)
    }

    /// Get activity information for a bead.
    pub fn get_bead_activity(&self, bead_id: &str) -> Option<&ActivityChecks> {
        self.stuck_detector.get_cached_activity(bead_id)
    }

    /// Format full task queue for the Tasks view with optional priority filter and search query.
    ///
    /// When `priority_filter` is Some(p), only beads with priority == p are shown.
    /// When `priority_filter` is None, all beads are shown.
    /// When `search_query` is non-empty, only beads matching the query are shown.
    pub fn format_task_queue_full_filtered_with_search(
        &self,
        priority_filter: Option<u8>,
        search_query: &str,
    ) -> String {
        if let Some(message) = self.queue_unavailable_message() {
            let mut lines = vec![message];
            // Live scheduler assignments render even while the queue cannot:
            // they identify running workers independently of the store read.
            if self.assignment_feed.is_some() {
                lines.push(String::new());
                self.push_assignment_section(&mut lines, None, "");
            }
            return lines.join("\n");
        }

        let data = self.get_filtered_aggregated_data_with_search(priority_filter, search_query);
        let mut lines = Vec::new();

        // Filter indicator in header (priority and search)
        let mut filter_parts = Vec::new();
        if let Some(p) = priority_filter {
            filter_parts.push(format!("P{}", p));
        }
        if !search_query.is_empty() {
            filter_parts.push(format!("Search: \"{}\"", search_query));
        }
        let filter_text = if filter_parts.is_empty() {
            String::new()
        } else {
            format!(" [Filtered: {}]", filter_parts.join(", "))
        };

        // Summary header
        lines.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());
        lines.push(format!("{}{}", data.format_summary(), filter_text));
        lines.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());
        lines.push(String::new());

        // Live scheduler assignments (bead dispatch): the bead → worker
        // mapping, refreshed from the scheduler's shared feed on every poll.
        if self.assignment_feed.is_some() {
            self.push_assignment_section(&mut lines, priority_filter, search_query);
            lines.push(String::new());
        }

        // In-progress section
        if !data.in_progress.is_empty() {
            lines.push("● IN PROGRESS".to_string());
            lines.push("─────────────────────────────────────────────────────".to_string());
            for (_ws, bead) in &data.in_progress {
                let assignee = bead.assignee.as_deref().unwrap_or("-");
                let score = bead.score();

                // Check if task is stuck
                let stuck_indicator = if self.is_bead_stuck(&bead.id) {
                    " ⚠️ STUCK"
                } else {
                    ""
                };

                lines.push(format!(
                    "{} {:8} {} | {:3} | {} [{}]{}",
                    bead.priority_indicator(),
                    bead.id,
                    bead.priority_str(),
                    score,
                    truncate_str(&bead.title, 25),
                    truncate_str(assignee, 10),
                    stuck_indicator
                ));
            }
            lines.push(String::new());
        }

        // Stuck tasks section
        if !self.stuck_tasks.is_empty() {
            lines.push("⚠️  STUCK TASKS (Timeout Available)".to_string());
            lines.push("─────────────────────────────────────────────────────".to_string());
            for task in &self.stuck_tasks {
                let duration_mins = task.in_progress_duration.as_secs() / 60;
                lines.push(format!(
                    "⚠️  {:8} | {}m | {}",
                    task.bead_id,
                    duration_mins,
                    truncate_str(&task.title, 30)
                ));
                lines.push(format!("    Reason: {}", truncate_str(&task.reason, 50)));
                if let Some(activity) = self.get_bead_activity(&task.bead_id) {
                    lines.push(format!("    Activity: {}", activity.summary()));
                }
            }
            lines.push(String::new());
        }

        // Ready section
        if !data.ready.is_empty() {
            lines.push("○ READY".to_string());
            lines.push("─────────────────────────────────────────────────────".to_string());
            for (ws, bead) in data.ready.iter().take(10) {
                let score = bead.score();
                lines.push(format!(
                    "{} {:8} {} | {:3} | {} [{}]",
                    bead.priority_indicator(),
                    bead.id,
                    bead.priority_str(),
                    score,
                    truncate_str(&bead.title, 25),
                    ws
                ));
            }
            if data.ready.len() > 10 {
                lines.push(format!("... {} more ready tasks", data.ready.len() - 10));
            }
            lines.push(String::new());
        }

        // Blocked section
        if !data.blocked.is_empty() {
            lines.push("⊘ BLOCKED".to_string());
            lines.push("─────────────────────────────────────────────────────".to_string());
            for (_ws, bead) in data.blocked.iter().take(5) {
                let score = bead.score();
                let reason = if bead.manual_blocked {
                    "manual".to_string()
                } else {
                    format!("{} deps", bead.dependency_count)
                };
                lines.push(format!(
                    "{} {:8} {} | {:3} | {} ({})",
                    bead.priority_indicator(),
                    bead.id,
                    bead.priority_str(),
                    score,
                    truncate_str(&bead.title, 20),
                    reason
                ));
            }
            if data.blocked.len() > 5 {
                lines.push(format!("... {} more blocked tasks", data.blocked.len() - 5));
            }
            lines.push(String::new());
        }

        // Show message if filter is active but no results
        if data.in_progress.is_empty() && data.ready.is_empty() && data.blocked.is_empty() {
            if !search_query.is_empty() {
                lines.push(format!(
                    "No tasks found matching \"{}\". Press Esc to clear search.",
                    search_query
                ));
                lines.push(String::new());
            } else if let Some(p) = priority_filter {
                lines.push(format!("No P{p} tasks found. Press {p} to clear filter."));
                lines.push(String::new());
            }
        }

        // Hotkeys
        lines.push("─────────────────────────────────────────────────────".to_string());
        lines.push(
            "[/] Search  [0-4] Filter by priority  [X] Clear filter  [Enter] View  [Esc] Clear search".to_string(),
        );

        lines.join("\n")
    }

    /// Why the full queue cannot render its body, as a replacement message:
    /// no readable bead store, no workspaces configured, or the first load
    /// still pending. `None` when the queue body should render.
    fn queue_unavailable_message(&self) -> Option<String> {
        if !self.has_bead_store() {
            return Some(
                "No bead store found.\n\n\
                 Install bead-rs to enable the task queue:\n\
                 https://git.ardenone.com/jedarden/bead-rs\n\n\
                 Documentation: docs/BEAD_LAUNCHER_PROTOCOL.md"
                    .to_string(),
            );
        }

        if self.workspaces.is_empty() {
            return Some(
                "No workspaces configured.\n\n\
                 To monitor workspaces, either:\n\
                 1. Set FORGE_WORKSPACES=/path/to/workspace1:/path/to/workspace2\n\
                 2. Run forge from a directory with a .beads/ folder\n\n\
                 Workspaces are initialized with: bead init"
                    .to_string(),
            );
        }

        if !self.is_loaded() {
            return Some("Loading bead data...".to_string());
        }

        None
    }

    /// Append the scheduler-assignment section: one row per live
    /// bead → worker mapping, in the scheduler's published order.
    ///
    /// Honors the same priority filter and search query as the queue
    /// sections around it; renders a clean empty marker when nothing is
    /// assigned (or nothing matches the active filter).
    fn push_assignment_section(
        &self,
        lines: &mut Vec<String>,
        priority_filter: Option<u8>,
        search_query: &str,
    ) {
        lines.push("◆ ASSIGNED (scheduler dispatch)".to_string());
        lines.push("─────────────────────────────────────────────────────".to_string());

        if self.assignments.is_empty() {
            lines.push("  No active assignments".to_string());
            return;
        }

        let matching: Vec<&WorkerBeadAssignment> = self
            .assignments
            .iter()
            .filter(|a| priority_filter.map_or(true, |p| a.bead_priority == p))
            .filter(|a| assignment_matches_search(a, search_query))
            .collect();

        if matching.is_empty() {
            lines.push("  No assignments match the current filter".to_string());
            return;
        }

        for assignment in matching {
            let priority = format!("P{}", assignment.bead_priority);
            lines.push(format!(
                "{} {:8} {} | {} → {}",
                priority_indicator(assignment.bead_priority),
                assignment.bead_id,
                priority,
                truncate_str(&assignment.bead_title, 20),
                truncate_str(&assignment.worker_id, 25)
            ));
        }
    }
}

/// Case-insensitive substring match for a scheduler assignment against
/// the task search query (bead ID, title, worker, session).
fn assignment_matches_search(assignment: &WorkerBeadAssignment, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    let query_lower = query.to_lowercase();
    [
        assignment.bead_id.as_str(),
        assignment.bead_title.as_str(),
        assignment.worker_id.as_str(),
        assignment.session_name.as_str(),
    ]
    .iter()
    .any(|field| field.to_lowercase().contains(&query_lower))
}

/// Priority indicator for display, shared by queue rows and scheduler
/// assignment rows.
fn priority_indicator(priority: u8) -> &'static str {
    match priority {
        0 => "🔴",
        1 => "🟠",
        2 => "🟡",
        3 => "🔵",
        _ => "⚪",
    }
}

/// Convert a store bead into the display-oriented [`Bead`], computing the
/// dependency and dependent counts from the graph. `dependency_count` counts
/// *unfinished* blockers (a closed blocker no longer blocks), matching the
/// ready/blocked partition used here.
fn to_bead(
    bead: &forge_core::StoreBead,
    beads: &[forge_core::StoreBead],
    index: &HashMap<&str, &forge_core::StoreBead>,
) -> Bead {
    let dependency_count = bead
        .blocking_dependency_ids()
        .into_iter()
        .filter(|blocker| index.get(blocker).is_none_or(|dep| dep.status != "closed"))
        .count();

    Bead {
        id: bead.id.clone(),
        title: bead.title.clone(),
        description: bead.description.clone(),
        status: bead.status.clone(),
        priority: bead.priority,
        issue_type: bead.issue_type.clone(),
        assignee: bead.assignee.clone(),
        labels: bead.labels.clone(),
        dependency_count,
        dependent_count: forge_core::bead_store::count_dependents(beads, &bead.id),
        manual_blocked: bead.manual_blocked,
        created_at: bead.created_at.clone().unwrap_or_default(),
        updated_at: bead.updated_at.clone().unwrap_or_default(),
    }
}

/// Truncate a string to a maximum length with ellipsis.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len > 3 {
        format!("{}...", &s[..max_len - 3])
    } else {
        s[..max_len].to_string()
    }
}

impl Default for Bead {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            description: String::new(),
            status: "open".to_string(),
            priority: 2,
            issue_type: "task".to_string(),
            assignee: None,
            labels: Vec::new(),
            dependency_count: 0,
            dependent_count: 0,
            manual_blocked: false,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
    use std::sync::Arc;

    use forge_worker::bead_scheduler::{BeadScheduler, BeadStatusBackend};
    use forge_worker::{LaunchConfig, WorkerLauncher};

    #[test]
    fn test_bead_status_checks() {
        let ready_bead = Bead {
            id: "fg-1".to_string(),
            title: "Test task".to_string(),
            status: "open".to_string(),
            priority: 2,
            dependency_count: 0,
            ..Default::default()
        };

        assert!(ready_bead.is_ready());
        assert!(!ready_bead.is_blocked());
        assert!(!ready_bead.is_in_progress());
    }

    #[test]
    fn test_bead_blocked() {
        let blocked_bead = Bead {
            id: "fg-2".to_string(),
            title: "Blocked task".to_string(),
            status: "open".to_string(),
            priority: 1,
            dependency_count: 2,
            ..Default::default()
        };

        assert!(!blocked_bead.is_ready());
        assert!(blocked_bead.is_blocked());
    }

    #[test]
    fn test_bead_manually_blocked() {
        let bead = Bead {
            id: "fg-mb".to_string(),
            title: "Manually blocked".to_string(),
            status: "open".to_string(),
            manual_blocked: true,
            ..Default::default()
        };

        assert!(bead.is_blocked());
        assert!(!bead.is_ready());
    }

    #[test]
    fn test_priority_display() {
        let bead = Bead {
            id: "fg-3".to_string(),
            title: "Critical".to_string(),
            status: "open".to_string(),
            priority: 0,
            ..Default::default()
        };

        assert_eq!(bead.priority_str(), "P0");
        assert_eq!(bead.priority_indicator(), "🔴");
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world", 8), "hello...");
        assert_eq!(truncate_str("ab", 2), "ab");
    }

    #[test]
    fn test_workspace_beads_stale() {
        let mut ws = WorkspaceBeads::new(PathBuf::from("/test"));
        assert!(ws.is_stale()); // No last_update

        ws.last_update = Some(Instant::now());
        assert!(!ws.is_stale()); // Just updated
    }

    #[test]
    fn test_bead_manager_new() {
        let manager = BeadManager::new();
        assert_eq!(manager.workspace_count(), 0);
        assert!(!manager.is_loaded());
    }

    #[test]
    fn test_bead_matches_search() {
        let bead = Bead {
            id: "fg-1m0v".to_string(),
            title: "Implement task filtering and search".to_string(),
            description: "Add fuzzy search for tasks".to_string(),
            labels: vec!["feature".to_string(), "ui".to_string()],
            issue_type: "task".to_string(),
            ..Default::default()
        };

        // Empty query matches all
        assert!(bead.matches_search(""));

        // ID match
        assert!(bead.matches_search("fg-1m0v"));
        assert!(bead.matches_search("1m0v"));
        assert!(bead.matches_search("FG-1M0V")); // Case insensitive

        // Title match
        assert!(bead.matches_search("filter"));
        assert!(bead.matches_search("SEARCH"));
        assert!(bead.matches_search("task"));

        // Description match
        assert!(bead.matches_search("fuzzy"));

        // Label match
        assert!(bead.matches_search("feature"));
        assert!(bead.matches_search("ui"));

        // Issue type match
        assert!(bead.matches_search("task"));

        // No match
        assert!(!bead.matches_search("nonexistent"));
        assert!(!bead.matches_search("xyz123"));
    }

    #[test]
    fn test_aggregated_data_format() {
        let data = AggregatedBeadData {
            total_ready: 5,
            total_blocked: 2,
            total_in_progress: 3,
            total_open: 8,
            ..Default::default()
        };

        let summary = data.format_summary();
        assert!(summary.contains("Ready: 5"));
        assert!(summary.contains("Blocked: 2"));
        assert!(summary.contains("In Progress: 3"));
    }

    /// Create a temporary bead-rs workspace fixture (config + checkpoint).
    fn create_bead_rs_workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let beads = dir.path().join(".beads");
        let checkpoint = beads.join("checkpoint");
        let objects = checkpoint.join("objects");
        fs::create_dir_all(&objects).unwrap();

        fs::write(
            beads.join("config.json"),
            r#"{"bead_cli": {"backend": "bead-rs"}}"#,
        )
        .unwrap();

        let snapshot = concat!(
            r#"{"issue":{"id":"tui-ready","title":"Ready task","base_status":"open","priority":1,"issue_type":"task","labels":[],"assignee":null,"dependencies":[]},"record_type":"issue"}"#,
            "\n",
            r#"{"issue":{"id":"tui-blocked","title":"Blocked task","base_status":"open","priority":2,"issue_type":"task","labels":[],"assignee":null,"dependencies":[{"blocker":"tui-ready","kind":"blocks"}]},"record_type":"issue"}"#,
            "\n",
            r#"{"issue":{"id":"tui-wip","title":"In progress task","base_status":"in_progress","priority":0,"issue_type":"task","labels":[],"assignee":"alpha","dependencies":[]},"record_type":"issue"}"#,
            "\n",
            r#"{"issue":{"id":"tui-closed","title":"Closed task","base_status":"closed","priority":3,"issue_type":"task","labels":[],"assignee":null,"dependencies":[]},"record_type":"issue"}"#,
            "\n",
            r#"{"record_type":"event","data":{"op":"close","bead":"tui-closed"}}"#,
            "\n",
        );
        // The snapshot filename is arbitrary from the reader's perspective —
        // it only follows active_root.path.
        let hash = "gen-3851a9f2c0d4e7b6a9c1d3e5f708162a";
        fs::write(objects.join(format!("{hash}.jsonl")), snapshot).unwrap();

        fs::write(
            checkpoint.join("current.json"),
            format!(r#"{{"active_root": {{"path": "objects/{hash}.jsonl"}}}}"#),
        )
        .unwrap();
        fs::write(checkpoint.join("forensic.jsonl"), snapshot).unwrap();

        dir
    }

    #[test]
    fn test_poll_workspace_reads_bead_rs_store() {
        let dir = create_bead_rs_workspace();
        let workspace = dir.path().to_path_buf();

        let mut manager = BeadManager::new();
        manager.add_workspace(workspace.clone());
        assert!(manager.is_bead_store_available());

        assert!(manager.poll_updates());
        assert!(manager.is_loaded());

        let cache = manager.cache.get(&workspace).unwrap();
        assert_eq!(cache.ready.len(), 1);
        assert_eq!(cache.ready[0].id, "tui-ready");
        assert_eq!(cache.in_progress.len(), 1);
        assert_eq!(cache.in_progress[0].id, "tui-wip");
        assert_eq!(cache.in_progress[0].assignee.as_deref(), Some("alpha"));
        assert_eq!(cache.blocked.len(), 1);
        assert_eq!(cache.blocked[0].id, "tui-blocked");
        assert_eq!(cache.blocked[0].dependency_count, 1);
        assert_eq!(cache.blocked[0].dependent_count, 0);
        assert_eq!(cache.ready[0].dependent_count, 1);

        let s = &cache.stats.summary;
        assert_eq!(s.total_issues, 4);
        assert_eq!(s.open_issues, 2);
        assert_eq!(s.in_progress_issues, 1);
        assert_eq!(s.closed_issues, 1);
        assert_eq!(s.ready_issues, 1);
        assert_eq!(s.blocked_issues, 1);

        // Aggregation surfaces the same buckets
        let data = manager.get_aggregated_data();
        assert_eq!(data.ready.len(), 1);
        assert_eq!(data.in_progress.len(), 1);
        assert_eq!(data.blocked.len(), 1);
        assert_eq!(data.total_open, 3);
    }

    // ============================================================
    // Scheduler assignments (bead dispatch) in the queue panels
    // ============================================================

    /// A dry-run scheduler over the bead-rs fixture workspace, driving the
    /// shared assignment feed exactly as the dispatch loop would.
    fn fixture_scheduler(dir: &tempfile::TempDir) -> BeadScheduler {
        let mut scheduler = BeadScheduler::new(Arc::new(WorkerLauncher::new()))
            .with_status_backend(BeadStatusBackend::DryRun);
        scheduler.add_workspace(dir.path()).unwrap();
        scheduler
    }

    fn fixture_launch_config(dir: &tempfile::TempDir) -> LaunchConfig {
        LaunchConfig::new(
            "/path/to/launcher.sh",
            "test-session",
            dir.path().to_path_buf(),
            "sonnet",
        )
    }

    /// The full queue view renders each live scheduler assignment with
    /// both identities: the bead and the worker holding it.
    #[test]
    fn test_full_queue_renders_scheduler_assignments() {
        let dir = create_bead_rs_workspace();
        let mut scheduler = fixture_scheduler(&dir);

        let mut manager = BeadManager::new();
        manager.add_workspace(dir.path().to_path_buf());
        manager.poll_updates();
        manager.set_assignment_feed(scheduler.assignments_feed());

        scheduler
            .assign_bead(
                "tui-ready",
                "dispatch-0-tui-ready",
                fixture_launch_config(&dir),
            )
            .unwrap();
        assert!(
            manager.poll_updates(),
            "an assignment change must mark the panel dirty"
        );

        let output = manager.format_task_queue_full();
        assert!(output.contains("ASSIGNED"), "section header missing");
        assert!(output.contains("tui-ready"), "bead identity missing");
        assert!(
            output.contains("dispatch-0-tui-ready"),
            "worker identity missing"
        );
        // The queue body renders alongside the assignments.
        assert!(output.contains("READY"));
    }

    /// A registered feed with no live assignments renders its empty state
    /// cleanly, without disturbing the queue sections around it.
    #[test]
    fn test_full_queue_empty_assignments_render_cleanly() {
        let dir = create_bead_rs_workspace();

        let mut manager = BeadManager::new();
        manager.add_workspace(dir.path().to_path_buf());
        manager.poll_updates();
        manager.set_assignment_feed(fixture_scheduler(&dir).assignments_feed());
        manager.poll_updates();

        let output = manager.format_task_queue_full();
        assert!(output.contains("ASSIGNED"));
        assert!(output.contains("No active assignments"));
        // The queue sections are intact around the empty section.
        assert!(output.contains("Ready: 1"));
        assert!(output.contains("READY"));
    }

    /// Without a registered feed (opt-in dispatch disabled or not wired),
    /// the panels render exactly as before: no assignment section at all.
    #[test]
    fn test_no_assignment_feed_preserves_queue_output() {
        let dir = create_bead_rs_workspace();

        let mut disabled = BeadManager::new();
        disabled.add_workspace(dir.path().to_path_buf());
        disabled.poll_updates();
        assert!(!disabled.has_assignment_feed());

        let output = disabled.format_task_queue_full();
        assert!(!output.contains("ASSIGNED"));
        assert!(!output.contains("No active assignments"));
        assert!(!disabled.format_task_queue_summary().contains("Assigned:"));
        // And polling never reports assignment-driven changes.
        assert!(!disabled.poll_updates());
    }

    /// The compact overview summary lists assignments when present and
    /// stays unchanged when the assignment set is empty.
    #[test]
    fn test_summary_renders_assignments_when_present() {
        let dir = create_bead_rs_workspace();
        let mut scheduler = fixture_scheduler(&dir);

        let mut manager = BeadManager::new();
        manager.add_workspace(dir.path().to_path_buf());
        manager.poll_updates();
        manager.set_assignment_feed(scheduler.assignments_feed());

        // Empty assignment set: the compact summary stays unchanged.
        manager.poll_updates();
        assert!(!manager.format_task_queue_summary().contains("Assigned:"));

        scheduler
            .assign_bead(
                "tui-ready",
                "dispatch-0-tui-ready",
                fixture_launch_config(&dir),
            )
            .unwrap();
        manager.poll_updates();

        let summary = manager.format_task_queue_summary();
        assert!(summary.contains("Assigned: 1"));
        assert!(summary.contains("tui-ready → dispatch-0-tui-ready"));
    }

    /// The panel tracks the live feed: an unchanged assignment set polls
    /// clean, a scheduler change surfaces on the next poll.
    #[test]
    fn test_poll_reports_assignment_changes_only_on_change() {
        let dir = create_bead_rs_workspace();
        let mut scheduler = fixture_scheduler(&dir);

        let mut manager = BeadManager::new();
        manager.add_workspace(dir.path().to_path_buf());
        manager.set_assignment_feed(scheduler.assignments_feed());
        manager.poll_updates(); // load the queue and the (empty) assignments

        // No change on the feed: nothing new to report.
        assert!(!manager.poll_updates());

        scheduler
            .assign_bead(
                "tui-ready",
                "dispatch-0-tui-ready",
                fixture_launch_config(&dir),
            )
            .unwrap();
        assert!(manager.poll_updates());
        assert_eq!(manager.assignments().len(), 1);
        assert_eq!(manager.assignments()[0].bead_id, "tui-ready");
        assert_eq!(manager.assignments()[0].worker_id, "dispatch-0-tui-ready");
    }

    /// Assignments honor the same search query as the queue sections.
    #[test]
    fn test_assignment_search_filters_rows() {
        let dir = create_bead_rs_workspace();
        let mut scheduler = fixture_scheduler(&dir);

        let mut manager = BeadManager::new();
        manager.add_workspace(dir.path().to_path_buf());
        manager.poll_updates();
        manager.set_assignment_feed(scheduler.assignments_feed());

        scheduler
            .assign_bead(
                "tui-ready",
                "dispatch-0-tui-ready",
                fixture_launch_config(&dir),
            )
            .unwrap();
        manager.poll_updates();

        // Match on the worker identity...
        let output = manager.format_task_queue_full_filtered_with_search(None, "dispatch-0");
        assert!(output.contains("tui-ready"));
        // ...on the bead identity...
        let output = manager.format_task_queue_full_filtered_with_search(None, "tui-ready");
        assert!(output.contains("dispatch-0-tui-ready"));
        // ...and a non-matching query falls back to the filtered marker.
        let output = manager.format_task_queue_full_filtered_with_search(None, "no-such-worker");
        assert!(output.contains("No assignments match the current filter"));
    }
}
