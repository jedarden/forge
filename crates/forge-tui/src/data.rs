//! Data management for the FORGE TUI.
//!
//! This module provides the data layer for the TUI, managing worker state,
//! task information, and formatting data for display. It integrates with
//! forge-core's StatusWatcher to provide real-time updates, forge-worker's
//! tmux discovery for additional session information, and the BeadManager
//! for task queue data from monitored workspaces.

use std::collections::HashMap;

use crate::bead::BeadManager;
use crate::status::{StatusWatcher, StatusWatcherConfig, WorkerCounts, WorkerStatusFile};
use forge_core::types::WorkerStatus;
use forge_worker::discovery::DiscoveryResult;

/// Aggregated worker data for TUI display.
#[derive(Debug, Default)]
pub struct WorkerData {
    /// All known workers by ID (from status files)
    pub workers: HashMap<String, WorkerStatusFile>,
    /// Counts by status
    pub counts: WorkerCounts,
    /// Last update timestamp
    pub last_update: Option<std::time::Instant>,
    /// Discovered tmux sessions (supplement to status files)
    pub tmux_sessions: Option<DiscoveryResult>,
    /// Last tmux discovery timestamp
    pub tmux_last_update: Option<std::time::Instant>,
}

impl WorkerData {
    /// Create empty worker data (loading state).
    pub fn new() -> Self {
        Self::default()
    }

    /// Update from a StatusWatcher.
    pub fn update_from_watcher(&mut self, watcher: &StatusWatcher) {
        self.workers = watcher.workers().clone();
        self.counts = watcher.worker_counts();
        self.last_update = Some(std::time::Instant::now());
    }

    /// Update from tmux discovery result.
    pub fn update_from_tmux(&mut self, result: DiscoveryResult) {
        self.tmux_sessions = Some(result);
        self.tmux_last_update = Some(std::time::Instant::now());
    }

    /// Check if data has been loaded.
    pub fn is_loaded(&self) -> bool {
        self.last_update.is_some()
    }

    /// Check if there are no workers (from status files).
    pub fn is_empty(&self) -> bool {
        self.workers.is_empty()
    }

    /// Check if there are any workers from either source.
    pub fn has_any_workers(&self) -> bool {
        !self.workers.is_empty()
            || self
                .tmux_sessions
                .as_ref()
                .map_or(false, |s| !s.workers.is_empty())
    }

    /// Get total worker count from all sources.
    pub fn total_worker_count(&self) -> usize {
        // Prefer status file count, fall back to tmux
        if !self.workers.is_empty() {
            self.workers.len()
        } else {
            self.tmux_sessions
                .as_ref()
                .map_or(0, |s| s.workers.len())
        }
    }

    /// Get workers grouped by model.
    pub fn workers_by_model(&self) -> HashMap<String, Vec<&WorkerStatusFile>> {
        let mut by_model: HashMap<String, Vec<&WorkerStatusFile>> = HashMap::new();
        for worker in self.workers.values() {
            let model = if worker.model.is_empty() {
                "unknown".to_string()
            } else {
                worker.model.clone()
            };
            by_model.entry(model).or_default().push(worker);
        }
        by_model
    }

    /// Format worker pool summary for display.
    pub fn format_worker_pool_summary(&self) -> String {
        if !self.is_loaded() {
            return "Loading worker data...".to_string();
        }

        // Check both status files and tmux sessions
        if !self.has_any_workers() {
            return "No workers found.\n\n\
                    Start workers using:\n\
                    - ./scripts/spawn-workers.sh\n\
                    - Or create status files in ~/.forge/status/"
                .to_string();
        }

        let mut lines = Vec::new();

        // If we have status file data, use it (more detailed)
        if !self.is_empty() {
            let c = &self.counts;
            lines.push(format!(
                "Total: {} ({} active, {} idle)",
                c.total, c.active, c.idle
            ));
            if c.unhealthy() > 0 {
                lines.push(format!("Unhealthy: {}", c.unhealthy()));
            }
            lines.push(String::new());

            // Workers by model
            let by_model = self.workers_by_model();
            let mut models: Vec<_> = by_model.keys().cloned().collect();
            models.sort();

            for model in models {
                if let Some(workers) = by_model.get(&model) {
                    let active = workers
                        .iter()
                        .filter(|w| w.status == WorkerStatus::Active)
                        .count();
                    let idle = workers
                        .iter()
                        .filter(|w| w.status == WorkerStatus::Idle)
                        .count();

                    let display_name = format_model_name(&model);
                    lines.push(format!("{:<10} {} active, {} idle", display_name, active, idle));
                }
            }
        }

        // If we have tmux sessions, show supplemental info
        if let Some(ref tmux) = self.tmux_sessions {
            if !tmux.workers.is_empty() {
                if !self.is_empty() {
                    lines.push(String::new());
                    lines.push("Tmux Sessions:".to_string());
                } else {
                    // Tmux-only data (no status files)
                    lines.push(format!(
                        "Total: {} ({} attached, {} detached)",
                        tmux.workers.len(),
                        tmux.attached_count,
                        tmux.detached_count
                    ));
                    lines.push(String::new());
                }

                // Show by type
                let mut types: Vec<_> = tmux.by_type.iter().collect();
                types.sort_by_key(|(t, _)| t.short_name());

                for (worker_type, count) in types {
                    lines.push(format!("{:<10} {}", worker_type.short_name(), count));
                }
            }
        }

        lines.join("\n")
    }

    /// Format worker table for Workers view.
    pub fn format_worker_table(&self) -> String {
        if !self.is_loaded() {
            return "Loading worker data...".to_string();
        }

        // Check both sources
        if !self.has_any_workers() {
            return "No workers found.\n\n\
                    Workers will appear here when they register\n\
                    status files in ~/.forge/status/\n\n\
                    [G] Spawn GLM  [S] Spawn Sonnet  [O] Spawn Opus  [K] Kill"
                .to_string();
        }

        let mut lines = Vec::new();

        // If we have status file workers, show the detailed table
        if !self.is_empty() {
            lines.push("┌─────────────────┬──────────┬──────────┬─────────────┐".to_string());
            lines.push("│ Worker ID       │ Model    │ Status   │ Task        │".to_string());
            lines.push("├─────────────────┼──────────┼──────────┼─────────────┤".to_string());

            let mut workers: Vec<_> = self.workers.values().collect();
            workers.sort_by(|a, b| a.worker_id.cmp(&b.worker_id));

            for worker in workers.iter().take(10) {
                let worker_id = truncate_string(&worker.worker_id, 15);
                let model = format_model_name_short(&worker.model);
                let status = format_status(&worker.status);
                let task = worker
                    .current_task
                    .as_deref()
                    .map(|t| truncate_string(t, 11))
                    .unwrap_or_else(|| "-".to_string());

                lines.push(format!(
                    "│ {:<15} │ {:<8} │ {:<8} │ {:<11} │",
                    worker_id, model, status, task
                ));
            }

            lines.push("└─────────────────┴──────────┴──────────┴─────────────┘".to_string());

            if self.workers.len() > 10 {
                lines.push(format!("\n... and {} more workers", self.workers.len() - 10));
            }
        } else if let Some(ref tmux) = self.tmux_sessions {
            // Fall back to tmux session table
            lines.push("┌────────────────────────┬──────────┬──────────┬──────────┐".to_string());
            lines.push("│ Session Name           │ Model    │ Status   │ Age      │".to_string());
            lines.push("├────────────────────────┼──────────┼──────────┼──────────┤".to_string());

            let mut workers: Vec<_> = tmux.workers.iter().collect();
            workers.sort_by(|a, b| a.session_name.cmp(&b.session_name));

            for worker in workers.iter().take(10) {
                let session = truncate_string(&worker.session_name, 22);
                let model = worker.worker_type.short_name();
                let status = if worker.is_attached { "attached" } else { "detached" };
                let age = worker.age();

                lines.push(format!(
                    "│ {:<22} │ {:<8} │ {:<8} │ {:<8} │",
                    session, model, status, age
                ));
            }

            lines.push("└────────────────────────┴──────────┴──────────┴──────────┘".to_string());

            if tmux.workers.len() > 10 {
                lines.push(format!("\n... and {} more sessions", tmux.workers.len() - 10));
            }
        }

        lines.push("\n[G] Spawn GLM  [S] Spawn Sonnet  [O] Spawn Opus  [K] Kill".to_string());

        lines.join("\n")
    }

    /// Format activity log from worker data.
    pub fn format_activity_log(&self) -> String {
        if !self.is_loaded() {
            return "Loading activity data...".to_string();
        }

        if !self.has_any_workers() {
            return "No recent activity.\n\n\
                    Activity will appear here as workers\n\
                    complete tasks and update their status."
                .to_string();
        }

        let mut lines = Vec::new();

        // Use status file workers if available (more detailed)
        if !self.is_empty() {
            let mut workers: Vec<_> = self.workers.values().collect();
            workers.sort_by(|a, b| {
                b.last_activity
                    .cmp(&a.last_activity)
                    .then(b.started_at.cmp(&a.started_at))
            });

            let now = chrono::Local::now();

            for worker in workers.iter().take(10) {
                let time_str = if let Some(activity) = worker.last_activity {
                    activity.format("%H:%M:%S").to_string()
                } else if let Some(started) = worker.started_at {
                    started.format("%H:%M:%S").to_string()
                } else {
                    now.format("%H:%M:%S").to_string()
                };

                let icon = match worker.status {
                    WorkerStatus::Active => "⟳",
                    WorkerStatus::Idle => "💤",
                    WorkerStatus::Starting => "🔄",
                    WorkerStatus::Failed => "❌",
                    WorkerStatus::Stopped => "⏹",
                    WorkerStatus::Error => "⚠",
                };

                let message = match worker.status {
                    WorkerStatus::Active => {
                        if let Some(task) = &worker.current_task {
                            format!("{} working on {}", worker.worker_id, task)
                        } else {
                            format!("{} is active", worker.worker_id)
                        }
                    }
                    WorkerStatus::Idle => format!("{} idle", worker.worker_id),
                    WorkerStatus::Starting => format!("Spawned {}", worker.worker_id),
                    WorkerStatus::Failed => format!("{} failed", worker.worker_id),
                    WorkerStatus::Stopped => format!("{} stopped", worker.worker_id),
                    WorkerStatus::Error => format!("{} error", worker.worker_id),
                };

                lines.push(format!("{} {} {}", time_str, icon, message));
            }
        } else if let Some(ref tmux) = self.tmux_sessions {
            // Fall back to tmux session activity
            let mut workers: Vec<_> = tmux.workers.iter().collect();
            workers.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));

            for worker in workers.iter().take(10) {
                let time_str = worker.last_activity.format("%H:%M:%S").to_string();
                let icon = if worker.is_attached { "⟳" } else { "💤" };
                let status = if worker.is_attached { "attached" } else { "detached" };
                let message = format!("{} {} ({})", worker.session_name, status, worker.age());

                lines.push(format!("{} {} {}", time_str, icon, message));
            }
        }

        if lines.is_empty() {
            lines.push("No recent activity.".to_string());
        }

        lines.join("\n")
    }
}

/// Format a model identifier for display.
fn format_model_name(model: &str) -> String {
    // Common model name mappings
    let lower = model.to_lowercase();
    if lower.contains("glm-47") || lower.contains("glm47") || lower.contains("glm-4.7") {
        "GLM-4.7".to_string()
    } else if lower.contains("sonnet") {
        "Sonnet".to_string()
    } else if lower.contains("opus") {
        "Opus".to_string()
    } else if lower.contains("haiku") {
        "Haiku".to_string()
    } else if model.is_empty() {
        "Unknown".to_string()
    } else {
        model.to_string()
    }
}

/// Format model name for table (shorter version).
fn format_model_name_short(model: &str) -> String {
    let name = format_model_name(model);
    truncate_string(&name, 8)
}

/// Format worker status for display.
fn format_status(status: &WorkerStatus) -> String {
    match status {
        WorkerStatus::Active => "Active".to_string(),
        WorkerStatus::Idle => "Idle".to_string(),
        WorkerStatus::Starting => "Starting".to_string(),
        WorkerStatus::Failed => "Failed".to_string(),
        WorkerStatus::Stopped => "Stopped".to_string(),
        WorkerStatus::Error => "Error".to_string(),
    }
}

/// Truncate a string to a maximum length.
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len > 2 {
        format!("{}…", &s[..max_len - 1])
    } else {
        s[..max_len].to_string()
    }
}

/// Interval for tmux discovery polling (5 seconds).
const TMUX_DISCOVERY_INTERVAL_SECS: u64 = 5;

/// Data manager that handles the StatusWatcher, BeadManager, and provides formatted data.
pub struct DataManager {
    /// StatusWatcher for real-time updates
    watcher: Option<StatusWatcher>,
    /// Cached worker data
    pub worker_data: WorkerData,
    /// Bead manager for task queue data
    pub bead_manager: BeadManager,
    /// Error message if watcher failed to initialize
    init_error: Option<String>,
    /// Last tmux discovery time
    last_tmux_poll: Option<std::time::Instant>,
    /// Tokio runtime for async tmux discovery
    runtime: Option<tokio::runtime::Runtime>,
}

impl DataManager {
    /// Create a new DataManager, initializing the StatusWatcher and BeadManager.
    pub fn new() -> Self {
        let (watcher, init_error) = match StatusWatcher::new(StatusWatcherConfig::default()) {
            Ok(w) => (Some(w), None),
            Err(e) => (None, Some(format!("Failed to initialize status watcher: {}", e))),
        };

        // Create a tokio runtime for async tmux discovery
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok();

        // Initialize bead manager with default workspaces
        let mut bead_manager = BeadManager::new();
        bead_manager.add_default_workspaces();

        let mut manager = Self {
            watcher,
            worker_data: WorkerData::new(),
            bead_manager,
            init_error,
            last_tmux_poll: None,
            runtime,
        };

        // Initial data load
        manager.poll_updates();

        manager
    }

    /// Create a DataManager with a custom status directory.
    pub fn with_status_dir(status_dir: std::path::PathBuf) -> Self {
        let config = StatusWatcherConfig::default().with_status_dir(status_dir);
        let (watcher, init_error) = match StatusWatcher::new(config) {
            Ok(w) => (Some(w), None),
            Err(e) => (None, Some(format!("Failed to initialize status watcher: {}", e))),
        };

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok();

        // Initialize bead manager with default workspaces
        let mut bead_manager = BeadManager::new();
        bead_manager.add_default_workspaces();

        let mut manager = Self {
            watcher,
            worker_data: WorkerData::new(),
            bead_manager,
            init_error,
            last_tmux_poll: None,
            runtime,
        };

        manager.poll_updates();
        manager
    }

    /// Poll for updates from the StatusWatcher, BeadManager, and tmux discovery.
    ///
    /// This should be called regularly (e.g., in the event loop).
    pub fn poll_updates(&mut self) {
        // Poll status watcher
        if let Some(ref mut watcher) = self.watcher {
            // Drain all available events
            while watcher.try_recv().is_some() {}

            // Update worker data from watcher state
            self.worker_data.update_from_watcher(watcher);
        }

        // Poll bead manager for task queue updates
        self.bead_manager.poll_updates();

        // Periodically poll tmux discovery (less frequently)
        let should_poll_tmux = self
            .last_tmux_poll
            .map_or(true, |t| t.elapsed().as_secs() >= TMUX_DISCOVERY_INTERVAL_SECS);

        if should_poll_tmux {
            self.poll_tmux_discovery();
            self.last_tmux_poll = Some(std::time::Instant::now());
        }
    }

    /// Poll tmux for worker sessions.
    fn poll_tmux_discovery(&mut self) {
        if let Some(ref runtime) = self.runtime {
            // Run tmux discovery asynchronously but block briefly for result
            match runtime.block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_millis(500),
                    forge_worker::discover_workers(),
                )
                .await
            }) {
                Ok(Ok(result)) => {
                    self.worker_data.update_from_tmux(result);
                }
                Ok(Err(_)) | Err(_) => {
                    // Discovery failed or timed out - that's okay, status files are primary
                }
            }
        }
    }

    /// Check if there was an initialization error.
    pub fn init_error(&self) -> Option<&str> {
        self.init_error.as_deref()
    }

    /// Check if data is available.
    pub fn is_ready(&self) -> bool {
        self.watcher.is_some() && self.worker_data.is_loaded()
    }

    /// Get worker counts.
    pub fn worker_counts(&self) -> &WorkerCounts {
        &self.worker_data.counts
    }
}

impl Default for DataManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_model_name() {
        assert_eq!(format_model_name("claude-code-glm-47"), "GLM-4.7");
        assert_eq!(format_model_name("glm47"), "GLM-4.7");
        assert_eq!(format_model_name("sonnet-4.5"), "Sonnet");
        assert_eq!(format_model_name("claude-opus-4.6"), "Opus");
        assert_eq!(format_model_name("haiku"), "Haiku");
        assert_eq!(format_model_name(""), "Unknown");
        assert_eq!(format_model_name("custom-model"), "custom-model");
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("hello", 10), "hello");
        assert_eq!(truncate_string("hello world", 5), "hell…");
        assert_eq!(truncate_string("ab", 2), "ab");
        assert_eq!(truncate_string("abc", 2), "ab");
    }

    #[test]
    fn test_worker_data_empty() {
        let data = WorkerData::new();
        assert!(!data.is_loaded());
        assert!(data.is_empty());
        assert!(!data.has_any_workers());
    }

    #[test]
    fn test_worker_data_loading_message() {
        let data = WorkerData::new();
        let summary = data.format_worker_pool_summary();
        assert!(summary.contains("Loading"));
    }

    #[test]
    fn test_format_status() {
        assert_eq!(format_status(&WorkerStatus::Active), "Active");
        assert_eq!(format_status(&WorkerStatus::Idle), "Idle");
        assert_eq!(format_status(&WorkerStatus::Failed), "Failed");
    }

    #[test]
    fn test_worker_data_with_tmux() {
        let mut data = WorkerData::new();
        data.last_update = Some(std::time::Instant::now());

        // Initially empty
        assert!(!data.has_any_workers());

        // Add tmux sessions
        let discovery = DiscoveryResult {
            workers: vec![],
            by_type: std::collections::HashMap::new(),
            attached_count: 0,
            detached_count: 0,
        };
        data.update_from_tmux(discovery);

        // Still no workers
        assert!(!data.has_any_workers());
        assert_eq!(data.total_worker_count(), 0);
    }
}
