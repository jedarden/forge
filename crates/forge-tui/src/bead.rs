//! Bead data management for the FORGE TUI.
//!
//! This module provides functionality for querying beads from monitored workspaces
//! using the `br` CLI. It periodically polls for bead status and caches the results
//! for display in the task queue.
//!
//! ## Architecture
//!
//! The BeadManager:
//! 1. Maintains a list of monitored workspace paths
//! 2. Periodically queries the `br` CLI for bead status
//! 3. Caches results to minimize CLI invocations
//! 4. Provides formatted data for TUI display

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

// Re-export TaskScorer for use in TUI
pub use forge_worker::scorer::{ScoredBead, ScoreComponents, TaskScorer};

/// Default polling interval in seconds for bead updates.
const DEFAULT_POLL_INTERVAL_SECS: u64 = 30; // Increased from 5 to reduce blocking

/// Maximum age before considering cached data stale (in seconds).
const CACHE_STALE_SECS: u64 = 60; // Increased from 30

/// Timeout for br CLI commands in milliseconds.
/// Keep short to prevent blocking the UI.
const BR_COMMAND_TIMEOUT_MS: u64 = 2000;

/// Errors that can occur during bead operations.
#[derive(Error, Debug)]
pub enum BeadError {
    /// Failed to execute br CLI
    #[error("Failed to execute br CLI: {0}")]
    CliExecution(#[from] std::io::Error),

    /// Failed to parse br CLI output
    #[error("Failed to parse br output: {0}")]
    ParseError(#[from] serde_json::Error),

    /// br CLI returned non-zero exit code
    #[error("br CLI returned error: {0}")]
    CliError(String),

    /// Workspace has no .beads directory
    #[error("Workspace has no .beads directory: {0}")]
    NoBeadsDirectory(PathBuf),
}

/// Result type for bead operations.
pub type BeadResult<T> = Result<T, BeadError>;

/// A bead/issue as returned by the `br` CLI.
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

    /// Number of dependencies this bead is blocked by
    #[serde(default)]
    pub dependency_count: usize,

    /// Number of beads that depend on this one
    #[serde(default)]
    pub dependent_count: usize,

    /// Creation timestamp
    #[serde(default)]
    pub created_at: String,

    /// Last update timestamp
    #[serde(default)]
    pub updated_at: String,
}

impl Bead {
    /// Check if this bead is ready to work on (not blocked, not deferred).
    pub fn is_ready(&self) -> bool {
        self.dependency_count == 0 && !self.is_deferred()
    }

    /// Check if this bead is blocked by dependencies.
    pub fn is_blocked(&self) -> bool {
        self.dependency_count > 0
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
        match self.priority {
            0 => "🔴",
            1 => "🟠",
            2 => "🟡",
            3 => "🔵",
            _ => "⚪",
        }
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

        scorer.score_with_components(
            self.priority,
            self.dependent_count,
            age_hours,
            &self.labels,
        )
    }

    /// Get the score as a simple integer.
    pub fn score(&self) -> u32 {
        self.calculate_score().score
    }

    /// Format the score for display.
    pub fn score_display(&self) -> String {
        format!("{:3}", self.score())
    }
}

/// Statistics from `br stats` command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BeadStats {
    /// Summary statistics
    pub summary: BeadSummary,
}

/// Summary of bead counts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

    /// Statistics from br stats
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

    /// Last poll timestamp
    last_poll: Option<Instant>,

    /// Polling interval
    poll_interval: Duration,

    /// Whether br CLI is available
    br_available: Option<bool>,
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
            last_poll: None,
            poll_interval: Duration::from_secs(DEFAULT_POLL_INTERVAL_SECS),
            br_available: None,
        }
    }

    /// Add a workspace to monitor.
    pub fn add_workspace(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        if !self.workspaces.contains(&path) {
            self.workspaces.push(path.clone());
            self.cache.insert(path.clone(), WorkspaceBeads::new(path));
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

    /// Check if br CLI is available.
    pub fn is_br_available(&mut self) -> bool {
        if let Some(available) = self.br_available {
            return available;
        }

        let available = Command::new("br")
            .arg("--version")
            .output()
            .map_or(false, |o| o.status.success());

        self.br_available = Some(available);
        available
    }

    /// Poll for bead updates if the polling interval has elapsed.
    /// Returns true if any bead data changed.
    pub fn poll_updates(&mut self) -> bool {
        // Check if it's time to poll
        if let Some(last_poll) = self.last_poll {
            if last_poll.elapsed() < self.poll_interval {
                return false;
            }
        }

        // Don't poll if br is not available
        if !self.is_br_available() {
            return false;
        }

        self.last_poll = Some(Instant::now());

        let mut changed = false;

        // Poll each workspace
        for workspace in self.workspaces.clone() {
            if self.poll_workspace(&workspace) {
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

        let mut changed = false;

        // Query ready beads
        match Self::query_beads(workspace, "ready") {
            Ok(beads) => {
                if cache.ready != beads {
                    cache.ready = beads;
                    changed = true;
                }
                cache.last_error = None;
            }
            Err(e) => {
                debug!(workspace = ?workspace, error = %e, "Failed to query ready beads");
                cache.last_error = Some(e.to_string());
            }
        }

        // Query blocked beads
        match Self::query_beads(workspace, "blocked") {
            Ok(beads) => {
                if cache.blocked != beads {
                    cache.blocked = beads;
                    changed = true;
                }
            }
            Err(e) => {
                debug!(workspace = ?workspace, error = %e, "Failed to query blocked beads");
            }
        }

        // Query in-progress beads
        match Self::query_beads_filtered(workspace, Some("in_progress")) {
            Ok(beads) => {
                if cache.in_progress != beads {
                    cache.in_progress = beads;
                    changed = true;
                }
            }
            Err(e) => {
                debug!(workspace = ?workspace, error = %e, "Failed to query in-progress beads");
            }
        }

        // Query stats
        match Self::query_stats(workspace) {
            Ok(stats) => {
                cache.stats = stats;
            }
            Err(e) => {
                debug!(workspace = ?workspace, error = %e, "Failed to query stats");
            }
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

    /// Query beads using the `br ready` or `br blocked` command.
    /// Uses a timeout to avoid blocking the UI.
    fn query_beads(workspace: &PathBuf, subcommand: &str) -> BeadResult<Vec<Bead>> {
        use std::io::Read;
        use std::process::Stdio;

        let mut child = Command::new("br")
            .arg(subcommand)
            .arg("--format")
            .arg("json")
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Wait with timeout
        let start = Instant::now();
        let timeout = Duration::from_millis(BR_COMMAND_TIMEOUT_MS);

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Process finished
                    if !status.success() {
                        let mut stderr = String::new();
                        if let Some(mut err) = child.stderr {
                            let _ = err.read_to_string(&mut stderr);
                        }
                        if stderr.contains("no .beads") || stderr.contains("beads workspace") {
                            return Ok(Vec::new());
                        }
                        return Err(BeadError::CliError(stderr));
                    }

                    let mut stdout = String::new();
                    if let Some(mut out) = child.stdout {
                        let _ = out.read_to_string(&mut stdout);
                    }

                    if stdout.trim().is_empty() || stdout.trim() == "[]" {
                        return Ok(Vec::new());
                    }

                    let beads: Vec<Bead> = serde_json::from_str(&stdout)?;
                    return Ok(beads);
                }
                Ok(None) => {
                    // Still running
                    if start.elapsed() > timeout {
                        // Kill the process and return empty
                        let _ = child.kill();
                        debug!(workspace = ?workspace, subcommand, "br command timed out");
                        return Ok(Vec::new());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    return Err(BeadError::CliExecution(e));
                }
            }
        }
    }

    /// Query beads with a status filter using `br list`.
    /// Uses a timeout to avoid blocking the UI.
    fn query_beads_filtered(workspace: &PathBuf, status: Option<&str>) -> BeadResult<Vec<Bead>> {
        use std::io::Read;
        use std::process::Stdio;

        let mut cmd = Command::new("br");
        cmd.arg("list")
            .arg("--format")
            .arg("json")
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(status) = status {
            cmd.arg("--status").arg(status);
        }

        let mut child = cmd.spawn()?;

        // Wait with timeout
        let start = Instant::now();
        let timeout = Duration::from_millis(BR_COMMAND_TIMEOUT_MS);

        loop {
            match child.try_wait() {
                Ok(Some(exit_status)) => {
                    if !exit_status.success() {
                        let mut stderr = String::new();
                        if let Some(mut err) = child.stderr {
                            let _ = err.read_to_string(&mut stderr);
                        }
                        if stderr.contains("no .beads") || stderr.contains("beads workspace") {
                            return Ok(Vec::new());
                        }
                        return Err(BeadError::CliError(stderr));
                    }

                    let mut stdout = String::new();
                    if let Some(mut out) = child.stdout {
                        let _ = out.read_to_string(&mut stdout);
                    }

                    if stdout.trim().is_empty() || stdout.trim() == "[]" {
                        return Ok(Vec::new());
                    }

                    let beads: Vec<Bead> = serde_json::from_str(&stdout)?;
                    return Ok(beads);
                }
                Ok(None) => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        debug!(workspace = ?workspace, status, "br list command timed out");
                        return Ok(Vec::new());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    return Err(BeadError::CliExecution(e));
                }
            }
        }
    }

    /// Query statistics using `br stats`.
    /// Uses a timeout to avoid blocking the UI.
    fn query_stats(workspace: &PathBuf) -> BeadResult<BeadStats> {
        use std::io::Read;
        use std::process::Stdio;

        let mut child = Command::new("br")
            .arg("stats")
            .arg("--format")
            .arg("json")
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Wait with timeout
        let start = Instant::now();
        let timeout = Duration::from_millis(BR_COMMAND_TIMEOUT_MS);

        loop {
            match child.try_wait() {
                Ok(Some(exit_status)) => {
                    if !exit_status.success() {
                        let mut stderr = String::new();
                        if let Some(mut err) = child.stderr {
                            let _ = err.read_to_string(&mut stderr);
                        }
                        if stderr.contains("no .beads") || stderr.contains("beads workspace") {
                            return Ok(BeadStats::default());
                        }
                        return Err(BeadError::CliError(stderr));
                    }

                    let mut stdout = String::new();
                    if let Some(mut out) = child.stdout {
                        let _ = out.read_to_string(&mut stdout);
                    }

                    if stdout.trim().is_empty() {
                        return Ok(BeadStats::default());
                    }

                    let stats: BeadStats = serde_json::from_str(&stdout)?;
                    return Ok(stats);
                }
                Ok(None) => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        debug!(workspace = ?workspace, "br stats command timed out");
                        return Ok(BeadStats::default());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    return Err(BeadError::CliExecution(e));
                }
            }
        }
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
    pub fn get_filtered_aggregated_data_with_search(&self, priority_filter: Option<u8>, search_query: &str) -> AggregatedBeadData {
        let mut data = AggregatedBeadData::default();

        for (_, cache) in &self.cache {
            // Add ready beads (filtered)
            for bead in &cache.ready {
                if priority_filter.map_or(true, |p| bead.priority == p)
                    && bead.matches_search(search_query) {
                    data.ready.push((cache.name.clone(), bead.clone()));
                }
            }

            // Add blocked beads (filtered)
            for bead in &cache.blocked {
                if priority_filter.map_or(true, |p| bead.priority == p)
                    && bead.matches_search(search_query) {
                    data.blocked.push((cache.name.clone(), bead.clone()));
                }
            }

            // Add in-progress beads (filtered)
            for bead in &cache.in_progress {
                if priority_filter.map_or(true, |p| bead.priority == p)
                    && bead.matches_search(search_query) {
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
            score_b.cmp(&score_a).then_with(|| a.1.priority.cmp(&b.1.priority))
        });
        data.in_progress.sort_by(|a, b| {
            let score_a = a.1.calculate_score().score;
            let score_b = b.1.calculate_score().score;
            score_b.cmp(&score_a).then_with(|| a.1.priority.cmp(&b.1.priority))
        });
        data.blocked.sort_by(|a, b| {
            let score_a = a.1.calculate_score().score;
            let score_b = b.1.calculate_score().score;
            score_b.cmp(&score_a).then_with(|| a.1.priority.cmp(&b.1.priority))
        });

        data
    }

    /// Get the total count of actionable beads (ready + in_progress + blocked), filtered by priority.
    pub fn task_count_filtered(&self, priority_filter: Option<u8>) -> usize {
        let data = self.get_filtered_aggregated_data(priority_filter);
        data.ready.len() + data.in_progress.len() + data.blocked.len()
    }

    /// Get the total count of actionable beads (ready + in_progress + blocked), filtered by priority and search query.
    pub fn task_count_filtered_with_search(&self, priority_filter: Option<u8>, search_query: &str) -> usize {
        let data = self.get_filtered_aggregated_data_with_search(priority_filter, search_query);
        data.ready.len() + data.in_progress.len() + data.blocked.len()
    }

    /// Check if any data is loaded.
    pub fn is_loaded(&self) -> bool {
        self.cache.values().any(|c| c.last_update.is_some())
    }

    /// Check if br CLI is available and working.
    pub fn has_br(&self) -> bool {
        self.br_available.unwrap_or(false)
    }

    /// Get the number of monitored workspaces.
    pub fn workspace_count(&self) -> usize {
        self.workspaces.len()
    }

    /// Format task queue summary for the overview panel.
    pub fn format_task_queue_summary(&self) -> String {
        if !self.has_br() {
            return "br CLI not available.\n\n\
                    Install beads_rust to enable task queue:\n\
                    cargo install beads_rust"
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

    /// Format full task queue for the Tasks view with optional priority filter and search query.
    ///
    /// When `priority_filter` is Some(p), only beads with priority == p are shown.
    /// When `priority_filter` is None, all beads are shown.
    /// When `search_query` is non-empty, only beads matching the query are shown.
    pub fn format_task_queue_full_filtered_with_search(&self, priority_filter: Option<u8>, search_query: &str) -> String {
        if !self.has_br() {
            return "br CLI not available.\n\n\
                    Install beads_rust to enable task queue:\n\
                    cargo install beads_rust\n\n\
                    Documentation: https://github.com/Dicklesworthstone/beads_rust"
                .to_string();
        }

        if self.workspaces.is_empty() {
            return "No workspaces configured.\n\n\
                    To monitor workspaces, either:\n\
                    1. Set FORGE_WORKSPACES=/path/to/workspace1:/path/to/workspace2\n\
                    2. Run forge from a directory with a .beads/ folder\n\n\
                    Workspaces are initialized with: br init --prefix <prefix>"
                .to_string();
        }

        if !self.is_loaded() {
            return "Loading bead data...".to_string();
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

        // In-progress section
        if !data.in_progress.is_empty() {
            lines.push("● IN PROGRESS".to_string());
            lines.push("─────────────────────────────────────────────────────".to_string());
            for (_ws, bead) in &data.in_progress {
                let assignee = bead.assignee.as_deref().unwrap_or("-");
                let score = bead.score();
                lines.push(format!(
                    "{} {:8} {} | {:3} | {} [{}]",
                    bead.priority_indicator(),
                    bead.id,
                    bead.priority_str(),
                    score,
                    truncate_str(&bead.title, 25),
                    truncate_str(assignee, 10)
                ));
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
                lines.push(format!(
                    "{} {:8} {} | {:3} | {} ({} deps)",
                    bead.priority_indicator(),
                    bead.id,
                    bead.priority_str(),
                    score,
                    truncate_str(&bead.title, 20),
                    bead.dependency_count
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
                lines.push(format!("No tasks found matching \"{}\". Press Esc to clear search.", search_query));
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

#[cfg(test)]
mod tests {
    use super::*;

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
            issue_type: Some("task".to_string()),
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
            created_at: String::new(),
            updated_at: String::new(),
        }
    }
}
