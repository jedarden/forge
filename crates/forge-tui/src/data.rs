//! Data management for the FORGE TUI.
//!
//! This module provides the data layer for the TUI, managing worker state,
//! task information, cost analytics, and formatting data for display. It integrates with
//! forge-core's StatusWatcher to provide real-time updates, forge-worker's
//! tmux discovery for additional session information, the BeadManager
//! for task queue data from monitored workspaces, forge-cost for cost analytics,
//! LogWatcher for real-time log parsing, HealthMonitor for worker health tracking,
//! and AlertManager for worker health alerts.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::activity_panel::{ActivityEntry, ActivityEventType, ActivityLogData};
use crate::alert::{AlertBadge, AlertManager, AlertNotifier, AlertSeverity, AlertType};
use crate::bead::BeadManager;
use crate::cost_panel::{BudgetConfig, CostPanelData};
use crate::log_watcher::{LogWatcher, LogWatcherConfig, LogWatcherEvent, RealtimeMetrics};
use crate::metrics_panel::MetricsPanelData;
use crate::perf_metrics::{get_memory_rss, PerfMetrics};
use crate::status::{StatusWatcher, StatusWatcherConfig, WorkerCounts, WorkerStatusFile};
use crate::subscription_panel::SubscriptionData;
use forge_core::types::WorkerStatus;
use forge_cost::{CostDatabase, CostQuery, SubscriptionTracker};
use forge_worker::discovery::DiscoveryResult;
use forge_worker::health::{
    HealthCheckType, HealthLevel, HealthMonitor, HealthMonitorConfig, WorkerHealthStatus,
};

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
    /// Health status per worker
    pub health_status: HashMap<String, WorkerHealthStatus>,
    /// Last health check timestamp
    pub health_last_update: Option<std::time::Instant>,
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

    /// Update health status from health monitor.
    pub fn update_health_status(&mut self, health: HashMap<String, WorkerHealthStatus>) {
        self.health_status = health;
        self.health_last_update = Some(std::time::Instant::now());
    }

    /// Get health status for a specific worker.
    pub fn get_health(&self, worker_id: &str) -> Option<&WorkerHealthStatus> {
        self.health_status.get(worker_id)
    }

    /// Count workers by health level.
    pub fn health_counts(&self) -> (usize, usize, usize) {
        let mut healthy = 0;
        let mut degraded = 0;
        let mut unhealthy = 0;

        for health in self.health_status.values() {
            match health.health_level() {
                HealthLevel::Healthy => healthy += 1,
                HealthLevel::Degraded => degraded += 1,
                HealthLevel::Unhealthy => unhealthy += 1,
            }
        }

        (healthy, degraded, unhealthy)
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
            self.tmux_sessions.as_ref().map_or(0, |s| s.workers.len())
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

            // Show health summary
            if !self.health_status.is_empty() {
                let (healthy, degraded, unhealthy) = self.health_counts();
                if unhealthy > 0 {
                    lines.push(format!(
                        "Health: {} ● | {} ◐ | {} ○",
                        healthy, degraded, unhealthy
                    ));
                } else if degraded > 0 {
                    lines.push(format!("Health: {} ● | {} ◐", healthy, degraded));
                } else {
                    lines.push(format!("Health: {} ● (all healthy)", healthy));
                }
            }

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
                    lines.push(format!(
                        "{:<10} {} active, {} idle",
                        display_name, active, idle
                    ));
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

        // If we have status file workers, show the detailed table with health
        if !self.is_empty() {
            // Show health summary if we have health data
            if !self.health_status.is_empty() {
                let (healthy, degraded, unhealthy) = self.health_counts();
                let total = healthy + degraded + unhealthy;
                lines.push(format!(
                    "Health: {} healthy | {} degraded | {} unhealthy | {} total",
                    healthy, degraded, unhealthy, total
                ));
                lines.push(String::new());
            }

            lines.push("┌───┬─────────────────┬──────────┬──────────┬─────────────┐".to_string());
            lines.push("│ H │ Worker ID       │ Model    │ Status   │ Task        │".to_string());
            lines.push("├───┼─────────────────┼──────────┼──────────┼─────────────┤".to_string());

            let mut workers: Vec<_> = self.workers.values().collect();
            workers.sort_by(|a, b| a.worker_id.cmp(&b.worker_id));

            for worker in workers.iter().take(10) {
                let health_indicator = self
                    .get_health(&worker.worker_id)
                    .map(|h| h.health_indicator())
                    .unwrap_or("?");
                let worker_id = truncate_string(&worker.worker_id, 15);
                let model = format_model_name_short(&worker.model);
                let status = format_status(&worker.status);
                let task = worker
                    .current_task
                    .as_deref()
                    .map(|t| truncate_string(t, 11))
                    .unwrap_or_else(|| "-".to_string());

                lines.push(format!(
                    "│ {} │ {:<15} │ {:<8} │ {:<8} │ {:<11} │",
                    health_indicator, worker_id, model, status, task
                ));
            }

            lines.push("└───┴─────────────────┴──────────┴──────────┴─────────────┘".to_string());

            if self.workers.len() > 10 {
                lines.push(format!(
                    "\n... and {} more workers",
                    self.workers.len() - 10
                ));
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
                let status = if worker.is_attached {
                    "attached"
                } else {
                    "detached"
                };
                let age = worker.age();

                lines.push(format!(
                    "│ {:<22} │ {:<8} │ {:<8} │ {:<8} │",
                    session, model, status, age
                ));
            }

            lines.push("└────────────────────────┴──────────┴──────────┴──────────┘".to_string());

            if tmux.workers.len() > 10 {
                lines.push(format!(
                    "\n... and {} more sessions",
                    tmux.workers.len() - 10
                ));
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
                    WorkerStatus::Paused => "⏸",
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
                    WorkerStatus::Paused => format!("{} paused", worker.worker_id),
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
                let status = if worker.is_attached {
                    "attached"
                } else {
                    "detached"
                };
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
        WorkerStatus::Paused => "Paused".to_string(),
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

/// Interval for cost data polling (10 seconds).
const COST_POLL_INTERVAL_SECS: u64 = 10;

/// Interval for metrics data polling (10 seconds).
const METRICS_POLL_INTERVAL_SECS: u64 = 10;

/// Interval for log watcher polling (500ms).
const LOG_WATCHER_POLL_INTERVAL_MS: u64 = 500;

/// Interval for subscription data polling (60 seconds).
const SUBSCRIPTION_POLL_INTERVAL_SECS: u64 = 60;

/// Interval for health monitoring (30 seconds).
const HEALTH_POLL_INTERVAL_SECS: u64 = 30;

/// Data manager that handles the StatusWatcher, BeadManager, CostDatabase, LogWatcher, HealthMonitor, AlertManager, and provides formatted data.
pub struct DataManager {
    /// StatusWatcher for real-time updates
    watcher: Option<StatusWatcher>,
    /// Cached worker data
    pub worker_data: WorkerData,
    /// Bead manager for task queue data
    pub bead_manager: BeadManager,
    /// Cost database for analytics
    cost_db: Option<CostDatabase>,
    /// Cached cost panel data
    pub cost_data: CostPanelData,
    /// Subscription tracker for subscription management
    subscription_tracker: SubscriptionTracker,
    /// Subscription tracking data
    pub subscription_data: SubscriptionData,
    /// Performance metrics data
    pub metrics_data: MetricsPanelData,
    /// Real-time metrics from log parsing
    pub realtime_metrics: RealtimeMetrics,
    /// Health monitor for worker health checks
    health_monitor: Option<HealthMonitor>,
    /// Alert manager for worker health alerts
    pub alert_manager: AlertManager,
    /// Alert notifier for terminal bell and visual notifications
    pub alert_notifier: AlertNotifier,
    /// Activity log data for real-time streaming display
    pub activity_data: ActivityLogData,
    /// FORGE internal performance metrics (event loop, render, memory)
    pub perf_metrics: PerfMetrics,
    /// Error message if watcher failed to initialize
    init_error: Option<String>,
    /// Last tmux discovery time
    last_tmux_poll: Option<std::time::Instant>,
    /// Last cost poll time
    last_cost_poll: Option<std::time::Instant>,
    /// Last metrics poll time
    last_metrics_poll: Option<std::time::Instant>,
    /// Last log watcher poll time
    last_log_poll: Option<std::time::Instant>,
    /// Last subscription poll time
    last_subscription_poll: Option<std::time::Instant>,
    /// Last health check time
    last_health_poll: Option<std::time::Instant>,
    /// Tokio runtime for async tmux discovery
    runtime: Option<tokio::runtime::Runtime>,
    /// Dirty flag - whether data changed since last check
    dirty: bool,
    /// Cached worker count for quick comparison
    cached_worker_count: usize,
    /// Cached bead count for quick comparison
    cached_bead_count: usize,
    /// Log watcher for real-time API usage parsing
    log_watcher: Option<LogWatcher>,
    /// Log watcher event receiver
    log_rx: Option<std::sync::mpsc::Receiver<LogWatcherEvent>>,
    /// Previous worker statuses for change detection
    prev_worker_statuses: HashMap<String, WorkerStatus>,
}

impl DataManager {
    /// Check if data changed since last poll.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Clear the dirty flag.
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Take and clear the dirty flag.
    pub fn take_dirty(&mut self) -> bool {
        if self.dirty {
            self.dirty = false;
            true
        } else {
            false
        }
    }

    /// Create a new DataManager, initializing the StatusWatcher, BeadManager, and CostDatabase.
    pub fn new() -> Self {
        use std::time::Instant;
        use tracing::info;

        let start = Instant::now();
        info!("⏱️ DataManager::new() started");

        let (watcher, init_error) = match StatusWatcher::new(StatusWatcherConfig::default()) {
            Ok(w) => (Some(w), None),
            Err(e) => (
                None,
                Some(format!("Failed to initialize status watcher: {}", e)),
            ),
        };
        info!("⏱️ StatusWatcher initialized in {:?}", start.elapsed());

        // Create a tokio runtime for async tmux discovery
        let runtime_start = Instant::now();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok();
        info!("⏱️ Tokio runtime created in {:?}", runtime_start.elapsed());

        // Initialize bead manager with default workspaces
        let bead_start = Instant::now();
        let mut bead_manager = BeadManager::new();
        bead_manager.add_default_workspaces();
        info!("⏱️ BeadManager initialized in {:?}", bead_start.elapsed());

        // Initialize cost database from default location
        let cost_start = Instant::now();
        let cost_db = Self::init_cost_database();
        let cost_data = CostPanelData::loading();
        info!("⏱️ Cost database initialized in {:?}", cost_start.elapsed());

        // Initialize metrics data
        let metrics_data = MetricsPanelData::loading();

        // Initialize subscription tracker and data
        let sub_start = Instant::now();
        let subscription_tracker = SubscriptionTracker::with_default_config();
        let subscription_data = Self::build_subscription_data(&subscription_tracker);
        info!(
            "⏱️ Subscription tracker initialized in {:?}",
            sub_start.elapsed()
        );

        // Initialize real-time metrics from log parsing
        let realtime_metrics = RealtimeMetrics::new();

        // Initialize log watcher for real-time API usage parsing
        let (log_watcher, log_rx) = match LogWatcher::new(LogWatcherConfig::default()) {
            Ok((watcher, rx)) => (Some(watcher), Some(rx)),
            Err(e) => {
                info!("Failed to initialize log watcher: {}", e);
                (None, None)
            }
        };

        // Initialize health monitor
        let health_start = Instant::now();
        let health_monitor = match HealthMonitor::new(HealthMonitorConfig::default()) {
            Ok(m) => {
                info!(
                    "⏱️ HealthMonitor initialized in {:?}",
                    health_start.elapsed()
                );
                Some(m)
            }
            Err(e) => {
                info!("Failed to initialize health monitor: {}", e);
                None
            }
        };

        // Initialize alert manager for worker health alerts
        let alert_manager = AlertManager::new(100);

        // Initialize alert notifier for terminal bell and visual notifications
        let alert_notifier = AlertNotifier::new();

        // Initialize activity log data
        let activity_data = ActivityLogData::with_default_capacity();

        // Initialize FORGE internal performance metrics
        let perf_metrics = PerfMetrics::new();

        let manager = Self {
            watcher,
            worker_data: WorkerData::new(),
            bead_manager,
            cost_db,
            cost_data,
            subscription_tracker,
            subscription_data,
            metrics_data,
            realtime_metrics,
            health_monitor,
            alert_manager,
            alert_notifier,
            activity_data,
            perf_metrics,
            init_error,
            last_tmux_poll: None,
            last_cost_poll: None,
            last_metrics_poll: None,
            last_log_poll: None,
            last_subscription_poll: None,
            last_health_poll: None,
            runtime,
            dirty: false,
            cached_worker_count: 0,
            cached_bead_count: 0,
            log_watcher,
            log_rx,
            prev_worker_statuses: HashMap::new(),
        };

        // Skip initial poll_updates during initialization - it blocks for too long
        // due to bead manager calling `br` commands which can take 20+ seconds each.
        // Let the first poll happen during the main loop instead.
        info!("⏱️ Skipping initial poll_updates (will poll in main loop)");

        info!("⏱️ DataManager::new() completed in {:?}", start.elapsed());
        manager
    }

    /// Initialize the cost database.
    fn init_cost_database() -> Option<CostDatabase> {
        // Try default location: ~/.forge/costs.db
        let home = std::env::var("HOME").ok()?;
        let db_path = PathBuf::from(home).join(".forge").join("costs.db");

        if db_path.exists() {
            CostDatabase::open(&db_path).ok()
        } else {
            // Try to create it
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent).ok()?;
            }
            CostDatabase::open(&db_path).ok()
        }
    }

    /// Create a DataManager with a custom status directory.
    pub fn with_status_dir(status_dir: std::path::PathBuf) -> Self {
        let config = StatusWatcherConfig::default().with_status_dir(status_dir);
        let (watcher, init_error) = match StatusWatcher::new(config) {
            Ok(w) => (Some(w), None),
            Err(e) => (
                None,
                Some(format!("Failed to initialize status watcher: {}", e)),
            ),
        };

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok();

        // Initialize bead manager with default workspaces
        let mut bead_manager = BeadManager::new();
        bead_manager.add_default_workspaces();

        // Initialize cost database
        let cost_db = Self::init_cost_database();
        let cost_data = CostPanelData::loading();

        // Initialize metrics data
        let metrics_data = MetricsPanelData::loading();

        // Initialize subscription tracker and data
        let subscription_tracker = SubscriptionTracker::with_default_config();
        let subscription_data = Self::build_subscription_data(&subscription_tracker);

        // Initialize real-time metrics from log parsing
        let realtime_metrics = RealtimeMetrics::new();

        // Initialize log watcher for real-time API usage parsing
        let (log_watcher, log_rx) = match LogWatcher::new(LogWatcherConfig::default()) {
            Ok((watcher, rx)) => (Some(watcher), Some(rx)),
            Err(_) => (None, None),
        };

        // Initialize health monitor
        let health_monitor = HealthMonitor::new(HealthMonitorConfig::default()).ok();

        // Initialize alert manager for worker health alerts
        let alert_manager = AlertManager::new(100);

        // Initialize alert notifier for terminal bell and visual notifications
        let alert_notifier = AlertNotifier::new();

        // Initialize activity log data
        let activity_data = ActivityLogData::with_default_capacity();

        // Initialize FORGE internal performance metrics
        let perf_metrics = PerfMetrics::new();

        // Skip initial poll_updates during initialization - it blocks for too long
        // due to bead manager calling `br` commands which can take seconds each.
        // Let the first poll happen during the main loop instead.
        Self {
            watcher,
            worker_data: WorkerData::new(),
            bead_manager,
            cost_db,
            cost_data,
            subscription_tracker,
            subscription_data,
            metrics_data,
            realtime_metrics,
            health_monitor,
            alert_manager,
            alert_notifier,
            activity_data,
            perf_metrics,
            init_error,
            last_tmux_poll: None,
            last_cost_poll: None,
            last_metrics_poll: None,
            last_log_poll: None,
            last_subscription_poll: None,
            last_health_poll: None,
            runtime,
            dirty: false,
            cached_worker_count: 0,
            cached_bead_count: 0,
            log_watcher,
            log_rx,
            prev_worker_statuses: HashMap::new(),
        }
    }

    /// Build SubscriptionData from the tracker.
    fn build_subscription_data(tracker: &SubscriptionTracker) -> SubscriptionData {
        use crate::subscription_panel::{
            ResetPeriod, SubscriptionAction, SubscriptionService, SubscriptionStatus,
        };
        use chrono::{Duration, Utc};
        use tracing::info;

        info!(
            "Building subscription data, tracker has {} subscriptions",
            tracker.len()
        );

        let mut data = SubscriptionData::new();

        for summary in tracker.get_summaries() {
            info!(
                "Processing subscription: {} (usage: {}/{:?})",
                summary.name, summary.quota_used, summary.quota_limit
            );
            // Map subscription name to service type
            let service = if summary.name.to_lowercase().contains("anthropic")
                || summary.name.to_lowercase().contains("claude")
            {
                SubscriptionService::ClaudePro
            } else if summary.name.to_lowercase().contains("openai")
                || summary.name.to_lowercase().contains("chatgpt")
            {
                SubscriptionService::ChatGPTPlus
            } else if summary.name.to_lowercase().contains("cursor") {
                SubscriptionService::CursorPro
            } else {
                SubscriptionService::DeepSeekAPI // Default fallback
            };

            let mut status = SubscriptionStatus::new(service);

            // Set usage if we have a limit
            if let Some(limit) = summary.quota_limit {
                status = status.with_usage(summary.quota_used as u64, limit as u64, "tokens");
            }

            // Set billing period
            let days_remaining = tracker.days_until_renewal(&summary.name).unwrap_or(30) as i64;
            let reset_period = if days_remaining <= 1 {
                ResetPeriod::Daily
            } else if days_remaining <= 7 {
                ResetPeriod::Weekly
            } else {
                ResetPeriod::Monthly
            };
            status = status.with_reset(Utc::now() + Duration::days(days_remaining), reset_period);

            // Set active status
            status = status.with_active(true);

            // Map quota status to action
            let action = match summary.status {
                forge_cost::QuotaStatus::OnPace => SubscriptionAction::OnPace,
                forge_cost::QuotaStatus::Accelerate => SubscriptionAction::Accelerate,
                forge_cost::QuotaStatus::MaxOut => SubscriptionAction::MaxOut,
                forge_cost::QuotaStatus::Depleted => SubscriptionAction::OverQuota,
            };

            // Check for alerts
            let alert = tracker.get_alert(&summary.name);
            if alert.is_alert() {
                // Mark as critical/high usage in the status
                if matches!(
                    alert,
                    forge_cost::SubscriptionAlert::Critical
                        | forge_cost::SubscriptionAlert::Depleted
                ) {
                    // Override action to show urgency
                    status.current_usage = status.limit.unwrap_or(100) - 1; // Show 99% used
                }
            }

            data.subscriptions.push(status);
        }

        data.last_updated = Some(Utc::now());
        data
    }

    /// Poll for updates from the StatusWatcher, BeadManager, CostDatabase, and tmux discovery.
    ///
    /// This should be called regularly (e.g., in the event loop).
    /// Returns true if any data changed.
    pub fn poll_updates(&mut self) -> bool {
        let mut changed = false;
        let previous_worker_count = self.cached_worker_count;
        let previous_bead_count = self.cached_bead_count;

        // Poll status watcher
        if let Some(ref mut watcher) = self.watcher {
            // Drain all available events and check if any were received
            let mut events_received = false;
            while watcher.try_recv().is_some() {
                events_received = true;
            }

            // Only update if events were received
            if events_received {
                // Clone current workers to detect changes (avoids borrow issues)
                let current_workers = watcher.workers().clone();

                // Detect changes before updating
                for (worker_id, worker) in &current_workers {
                    let prev_status = self.prev_worker_statuses.get(worker_id);

                    match prev_status {
                        None => {
                            // New worker discovered
                            self.activity_data.push(
                                ActivityEntry::new(
                                    ActivityEventType::WorkerSpawn,
                                    "Worker spawned",
                                )
                                .with_source(worker_id),
                            );
                        }
                        Some(prev) if *prev != worker.status => {
                            // Status transition
                            let message =
                                format!("Status changed: {:?} → {:?}", prev, worker.status);
                            let event_type = match worker.status {
                                WorkerStatus::Stopped => ActivityEventType::WorkerStop,
                                WorkerStatus::Failed | WorkerStatus::Error => {
                                    ActivityEventType::Error
                                }
                                _ => ActivityEventType::WorkerTransition,
                            };
                            self.activity_data.push(
                                ActivityEntry::new(event_type, message).with_source(worker_id),
                            );

                            // Add task info if available
                            if let Some(ref task) = worker.current_task {
                                self.activity_data.push(
                                    ActivityEntry::new(
                                        ActivityEventType::TaskPickup,
                                        format!("Working on {}", task),
                                    )
                                    .with_source(worker_id),
                                );
                            }
                        }
                        _ => {}
                    }
                }

                // Check for workers that were removed
                for worker_id in self.prev_worker_statuses.keys() {
                    if !current_workers.contains_key(worker_id) {
                        self.activity_data.push(
                            ActivityEntry::new(ActivityEventType::WorkerStop, "Worker stopped")
                                .with_source(worker_id),
                        );
                    }
                }

                // Update previous statuses
                self.prev_worker_statuses = current_workers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.status))
                    .collect();

                // Now update worker data
                self.worker_data.update_from_watcher(watcher);
                changed = true;
            }
        }

        // Poll bead manager for task queue updates
        let beads_changed = self.bead_manager.poll_updates();
        if beads_changed {
            // Add activity entries for bead changes
            self.detect_bead_changes();
            changed = true;
        }

        // Periodically poll tmux discovery (less frequently)
        let should_poll_tmux = self.last_tmux_poll.map_or(true, |t| {
            t.elapsed().as_secs() >= TMUX_DISCOVERY_INTERVAL_SECS
        });

        if should_poll_tmux {
            self.poll_tmux_discovery();
            self.last_tmux_poll = Some(std::time::Instant::now());
        }

        // Periodically poll cost data
        let should_poll_costs = self
            .last_cost_poll
            .map_or(true, |t| t.elapsed().as_secs() >= COST_POLL_INTERVAL_SECS);

        if should_poll_costs {
            self.poll_cost_data();
            self.last_cost_poll = Some(std::time::Instant::now());
        }

        // Periodically poll metrics data (every 10 seconds)
        let should_poll_metrics = self.last_metrics_poll.map_or(true, |t| {
            t.elapsed().as_secs() >= METRICS_POLL_INTERVAL_SECS
        });

        if should_poll_metrics {
            self.poll_metrics_data();
            self.last_metrics_poll = Some(std::time::Instant::now());
        }

        // Poll log watcher for real-time API usage (more frequently - every 500ms)
        let should_poll_logs = self.last_log_poll.map_or(true, |t| {
            t.elapsed().as_millis() >= LOG_WATCHER_POLL_INTERVAL_MS as u128
        });

        if should_poll_logs {
            self.poll_log_watcher();
            self.last_log_poll = Some(std::time::Instant::now());
        }

        // Periodically poll subscription data (every 60 seconds)
        let should_poll_subscriptions = self.last_subscription_poll.map_or(true, |t| {
            t.elapsed().as_secs() >= SUBSCRIPTION_POLL_INTERVAL_SECS
        });

        if should_poll_subscriptions {
            self.poll_subscription_data();
            self.last_subscription_poll = Some(std::time::Instant::now());
        }

        // Periodically poll health monitoring (every 30 seconds)
        let should_poll_health = self
            .last_health_poll
            .map_or(true, |t| t.elapsed().as_secs() >= HEALTH_POLL_INTERVAL_SECS);

        if should_poll_health {
            self.poll_health_monitor();
            self.last_health_poll = Some(std::time::Instant::now());
        }

        // Update cached counts and mark dirty if changed
        self.cached_worker_count = self.worker_data.total_worker_count();
        self.cached_bead_count = self.bead_manager.total_bead_count();

        if changed
            || previous_worker_count != self.cached_worker_count
            || previous_bead_count != self.cached_bead_count
        {
            self.dirty = true;
        }

        self.dirty
    }

    /// Detect bead/task changes and add activity entries.
    fn detect_bead_changes(&mut self) {
        // Add a generic activity entry when bead data changes
        // In a production system, we'd track specific bead state changes
        let total_beads = self.bead_manager.total_bead_count();
        if total_beads > 0 {
            // This is a simple notification - could be enhanced to track specific changes
            // The bead manager already handles state tracking internally
        }
    }

    /// Add an activity entry manually.
    pub fn add_activity(
        &mut self,
        event_type: ActivityEventType,
        source: Option<&str>,
        message: impl Into<String>,
    ) {
        let mut entry = ActivityEntry::new(event_type, message);
        if let Some(s) = source {
            entry = entry.with_source(s);
        }
        self.activity_data.push(entry);
        self.dirty = true;
    }

    // ========== FORGE Performance Metrics ==========

    /// Record a frame's performance metrics.
    ///
    /// Call this at the end of each frame with:
    /// - event_loop_us: Time spent in event loop (polling + handling events)
    /// - render_us: Time spent rendering the frame
    pub fn record_frame_perf(&mut self, event_loop_us: u64, render_us: u64) {
        self.perf_metrics.record_frame(event_loop_us, render_us);
    }

    /// Record a database query time.
    pub fn record_db_query(&mut self, query_us: u64) {
        self.perf_metrics.record_db_query(query_us);
    }

    /// Record a key event.
    pub fn record_event(&mut self) {
        self.perf_metrics.record_event();
    }

    /// Record a worker spawn.
    pub fn record_worker_spawn(&mut self) {
        self.perf_metrics.record_worker_spawn();
    }

    /// Record a worker exit.
    pub fn record_worker_exit(&mut self) {
        self.perf_metrics.record_worker_exit();
    }

    /// Update memory usage (call periodically).
    pub fn update_memory(&mut self) {
        let rss = get_memory_rss();
        self.perf_metrics.update_memory(rss);
    }

    /// Prune old performance alerts.
    pub fn prune_perf_alerts(&mut self) {
        self.perf_metrics.prune_alerts();
    }

    /// Get reference to perf metrics.
    pub fn perf_metrics(&self) -> &PerfMetrics {
        &self.perf_metrics
    }

    /// Get mutable reference to perf metrics.
    pub fn perf_metrics_mut(&mut self) -> &mut PerfMetrics {
        &mut self.perf_metrics
    }

    /// Get the activity log data.
    pub fn activity_log(&self) -> &ActivityLogData {
        &self.activity_data
    }

    /// Get mutable access to activity log data (for scroll control).
    pub fn activity_log_mut(&mut self) -> &mut ActivityLogData {
        &mut self.activity_data
    }

    /// Poll health monitor for worker health status.
    fn poll_health_monitor(&mut self) {
        let Some(ref mut monitor) = self.health_monitor else {
            return;
        };

        match monitor.check_all_health() {
            Ok(health_status) => {
                // Track previous health for comparison
                let prev_health = self.worker_data.health_status.clone();

                // Check if any worker became unhealthy and add activity entries and alerts
                for (worker_id, health) in &health_status {
                    let prev_was_healthy = prev_health
                        .get(worker_id)
                        .map(|h| h.is_healthy)
                        .unwrap_or(true);

                    // Log status transitions
                    if !health.is_healthy && prev_was_healthy {
                        // Worker just became unhealthy
                        let level = health.health_level();
                        let msg = match level {
                            forge_worker::health::HealthLevel::Degraded => {
                                format!(
                                    "Worker degraded: {} (score: {:.0}%)",
                                    health.primary_error.as_deref().unwrap_or("unknown"),
                                    health.health_score * 100.0
                                )
                            }
                            forge_worker::health::HealthLevel::Unhealthy => {
                                format!(
                                    "Worker unhealthy: {} (score: {:.0}%)",
                                    health.primary_error.as_deref().unwrap_or("unknown"),
                                    health.health_score * 100.0
                                )
                            }
                            forge_worker::health::HealthLevel::Healthy => {
                                "Worker healthy".to_string()
                            }
                        };

                        self.activity_data.push(
                            ActivityEntry::new(ActivityEventType::Warning, msg.clone())
                                .with_source(worker_id),
                        );

                        // Create one alert per worker based on the most critical failed check
                        // Priority: PID > Activity > Task > Memory > Response
                        let (alert_type, alert_msg) =
                            if health.failed_checks.contains(&HealthCheckType::PidExists) {
                                (AlertType::WorkerCrashed, health.primary_error.clone())
                            } else if health
                                .failed_checks
                                .contains(&HealthCheckType::ActivityFresh)
                            {
                                (AlertType::WorkerStale, health.primary_error.clone())
                            } else if health
                                .failed_checks
                                .contains(&HealthCheckType::TaskProgress)
                            {
                                (AlertType::TaskStuck, health.primary_error.clone())
                            } else if health.failed_checks.contains(&HealthCheckType::MemoryUsage) {
                                (AlertType::MemoryHigh, health.primary_error.clone())
                            } else if health
                                .failed_checks
                                .contains(&HealthCheckType::ResponseHealth)
                            {
                                (AlertType::WorkerUnresponsive, health.primary_error.clone())
                            } else {
                                // Fallback - shouldn't happen but just in case
                                (AlertType::WorkerCrashed, health.primary_error.clone())
                            };

                        self.alert_manager
                            .raise(alert_type, worker_id.clone(), alert_msg);

                        // Trigger notification (terminal bell) for the alert
                        let severity = alert_type.default_severity();
                        self.alert_notifier.notify(severity);

                        // Add guidance for recovery
                        if !health.guidance.is_empty() {
                            for guidance in &health.guidance {
                                self.activity_data.push(
                                    ActivityEntry::new(
                                        ActivityEventType::Info,
                                        format!("Recovery: {}", guidance),
                                    )
                                    .with_source(worker_id),
                                );
                            }
                        }
                    }

                    // Check for auto-restart trigger
                    if health.should_auto_restart {
                        let msg = format!(
                            "Auto-restart triggered after {} consecutive failures",
                            health.consecutive_failures
                        );
                        self.activity_data.push(
                            ActivityEntry::new(ActivityEventType::Warning, msg.clone())
                                .with_source(worker_id),
                        );
                        self.alert_manager.raise(
                            AlertType::AutoRestartTriggered,
                            worker_id.clone(),
                            Some(msg),
                        );
                        // Notify for auto-restart (warning severity)
                        self.alert_notifier.notify(AlertSeverity::Warning);
                    }

                    // Check for recovery exhaustion
                    if health.recovery_exhausted && !health.is_healthy {
                        let msg = format!(
                            "Recovery exhausted ({} attempts) - manual intervention required",
                            health.recovery_attempts
                        );
                        self.activity_data.push(
                            ActivityEntry::new(ActivityEventType::Error, msg.clone())
                                .with_source(worker_id),
                        );
                        self.alert_manager.raise(
                            AlertType::RecoveryExhausted,
                            worker_id.clone(),
                            Some(msg),
                        );
                        // Notify for recovery exhausted (critical severity)
                        self.alert_notifier.notify(AlertSeverity::Critical);
                    }

                    // Log when worker recovers - clear alerts
                    if health.is_healthy && !prev_was_healthy {
                        self.activity_data.push(
                            ActivityEntry::new(
                                ActivityEventType::Info,
                                "Worker recovered - all health checks passing".to_string(),
                            )
                            .with_source(worker_id),
                        );
                        // Resolve all alerts for this worker
                        self.alert_manager.resolve_all_for_worker(worker_id);
                    }
                }

                let unhealthy_count = health_status.values().filter(|h| !h.is_healthy).count();
                let degraded_count = health_status
                    .values()
                    .filter(|h| {
                        matches!(
                            h.health_level(),
                            forge_worker::health::HealthLevel::Degraded
                        )
                    })
                    .count();

                if unhealthy_count > 0 || degraded_count > 0 {
                    tracing::info!(
                        "Health check: {} unhealthy, {} degraded workers detected",
                        unhealthy_count,
                        degraded_count
                    );
                    self.dirty = true;
                }

                // Update worker data with health status
                self.worker_data.update_health_status(health_status);
            }
            Err(e) => {
                tracing::warn!("Health check failed: {}", e);
                self.activity_data.push(ActivityEntry::new(
                    ActivityEventType::Error,
                    format!("Health check failed: {}", e),
                ));
            }
        }
    }

    /// Poll subscription tracker for updates.
    fn poll_subscription_data(&mut self) {
        use chrono::{Duration, Utc};
        use tracing::info;

        let Some(ref db) = self.cost_db else {
            // No database, just rebuild from tracker
            self.subscription_data = Self::build_subscription_data(&self.subscription_tracker);
            return;
        };

        // 1. Check and auto-reset billing periods that have ended
        if let Ok(reset_count) = self.subscription_tracker.check_and_reset_billing(db) {
            if reset_count > 0 {
                info!("Reset {} subscription billing periods", reset_count);
                self.dirty = true;
            }
        }

        // 2. Track recent API calls against subscriptions
        // Get API calls from the last polling interval (5 minutes)
        let since = Utc::now() - Duration::seconds(300); // 5 minutes
        if let Ok(recent_calls) = db.get_api_calls_since(since) {
            for call in recent_calls {
                // Find which subscription this model belongs to
                if let Some(ref sub_name) = self.subscription_tracker
                    .find_subscription_for_model(&call.model)
                {
                    // Track total tokens used
                    let total_tokens = call.total_tokens();

                    // Record usage and update subscription quota
                    if let Err(e) = db.increment_subscription_usage(sub_name, total_tokens) {
                        tracing::warn!(
                            "Failed to increment subscription usage for {}: {}",
                            sub_name, e
                        );
                    } else {
                        // Update local tracker cache
                        self.subscription_tracker.increment_usage(sub_name, total_tokens);

                        // Also record detailed usage event
                        use forge_cost::SubscriptionUsageRecord;
                        let sub_id = db.get_subscription_id(sub_name).ok().flatten();
                        if let Some(sid) = sub_id {
                            let record = SubscriptionUsageRecord::new(sid, total_tokens)
                                .with_worker(&call.worker_id)
                                .with_api_call(call.id.unwrap_or(0));
                            let _ = db.record_subscription_usage(&record);
                        }
                    }
                }
            }
        }

        // 3. Reload subscriptions from database to get updated quota_used values
        if let Err(e) = self.subscription_tracker.load_from_database(db) {
            tracing::warn!("Failed to reload subscriptions from database: {}", e);
        }

        // 4. Rebuild subscription data for display
        self.subscription_data = Self::build_subscription_data(&self.subscription_tracker);

        // 5. Check for critical alerts
        if self.subscription_tracker.has_critical_alert() {
            self.dirty = true;
        }
    }

    /// Poll cost database for updates.
    fn poll_cost_data(&mut self) {
        let Some(ref db) = self.cost_db else {
            self.cost_data = CostPanelData::with_error("Cost database not initialized");
            return;
        };

        let query = CostQuery::new(db);

        // Get today's costs
        match query.get_today_costs() {
            Ok(today) => {
                self.cost_data.set_today(today);
            }
            Err(e) => {
                self.cost_data.error = Some(format!("Failed to get today's costs: {}", e));
                return;
            }
        }

        // Get this week's costs (last 7 days)
        match query.get_weekly_costs() {
            Ok(week) => {
                self.cost_data.set_week(week);
            }
            Err(_) => {
                // Weekly cost failure is non-critical
            }
        }

        // Get current month's costs
        match query.get_current_month_costs() {
            Ok(monthly) => {
                // Convert to model costs for display
                let model_costs: Vec<forge_cost::ModelCost> = monthly
                    .by_model
                    .iter()
                    .map(|b| forge_cost::ModelCost {
                        model: b.model.clone(),
                        total_cost_usd: b.total_cost_usd,
                        call_count: b.call_count,
                        avg_cost_per_call: if b.call_count > 0 {
                            b.total_cost_usd / b.call_count as f64
                        } else {
                            0.0
                        },
                        total_tokens: b.input_tokens
                            + b.output_tokens
                            + b.cache_creation_tokens
                            + b.cache_read_tokens,
                    })
                    .collect();

                self.cost_data
                    .set_monthly(monthly.total_cost_usd, model_costs);

                // Build daily trend from monthly data
                let trend: Vec<(chrono::NaiveDate, f64)> = monthly
                    .by_day
                    .iter()
                    .map(|d| (d.date, d.total_cost_usd))
                    .collect();
                self.cost_data.set_daily_trend(trend);
            }
            Err(e) => {
                self.cost_data.error = Some(format!("Failed to get monthly costs: {}", e));
            }
        }

        // Get projected costs
        match query.get_projected_costs(None) {
            Ok(projected) => {
                self.cost_data.set_projected(projected);
            }
            Err(_) => {
                // Projection failure is non-critical
            }
        }

        // Get optimization data using CostOptimizer
        let optimizer = forge_cost::CostOptimizer::new(db, forge_cost::OptimizerConfig::default());
        match optimizer.generate_report() {
            Ok(report) => {
                self.cost_data.set_recommendations(report.recommendations);
                self.cost_data.set_savings_achieved(report.savings_achieved);
                self.cost_data
                    .set_subscription_utilization(report.subscription_utilization);
            }
            Err(_) => {
                // Optimization failure is non-critical
            }
        }

        // Set default budget config (can be customized later)
        self.cost_data.set_budget(BudgetConfig::default());

        // Get worker cost breakdown (top 10 workers)
        match query.get_today_worker_costs(10) {
            Ok(mut workers) => {
                // Calculate session total and mark expensive workers
                let session_total: f64 = workers.iter().map(|w| w.total_cost_usd).sum();

                // Set expensive threshold to 10% of session total or $1, whichever is higher
                let threshold = (session_total * 0.1).max(1.0);
                for worker in &mut workers {
                    worker.is_expensive = worker.total_cost_usd >= threshold;
                }

                self.cost_data.set_worker_costs(workers);
                self.cost_data.set_session_total(session_total);
                self.cost_data.set_expensive_threshold(threshold);
            }
            Err(_) => {
                // Worker cost failure is non-critical
            }
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

    /// Poll metrics database for updates.
    fn poll_metrics_data(&mut self) {
        let Some(ref db) = self.cost_db else {
            self.metrics_data = MetricsPanelData::with_error("Cost database not initialized");
            return;
        };

        // Get today's daily stats
        let today = chrono::Utc::now().date_naive();
        match db.get_daily_stat(today) {
            Ok(Some(daily)) => {
                self.metrics_data.set_today(daily);
            }
            Ok(None) => {
                // No stats for today yet - create empty stats
                let empty_daily = forge_cost::DailyStat::new(today);
                self.metrics_data.set_today(empty_daily);
            }
            Err(e) => {
                self.metrics_data.error = Some(format!("Failed to get daily stats: {}", e));
                return;
            }
        }

        // Get hourly stats for the last 24 hours
        match db.get_recent_hourly_stats(24) {
            Ok(hourly) => {
                self.metrics_data.set_hourly_stats(hourly);
            }
            Err(_) => {
                // Hourly stats failure is non-critical
            }
        }

        // Get model performance for today
        match db.get_model_performance(today) {
            Ok(models) => {
                self.metrics_data.set_model_performance(models);
            }
            Err(_) => {
                // Model performance failure is non-critical
            }
        }

        // Get worker efficiency for today
        match db.get_worker_efficiency(today) {
            Ok(workers) => {
                self.metrics_data.set_worker_efficiency(workers);
            }
            Err(_) => {
                // Worker efficiency failure is non-critical
            }
        }

        // Get 7-day trend data for sparklines
        match db.get_7day_task_trend() {
            Ok(trend) => {
                self.metrics_data.set_task_trend_7day(trend);
            }
            Err(_) => {
                // Trend data failure is non-critical
            }
        }

        // Get 7-day cost trend
        match db.get_7day_cost_trend() {
            Ok(trend) => {
                self.metrics_data.set_cost_trend_7day(trend);
            }
            Err(_) => {}
        }

        // Get tasks per hour for histogram
        match db.get_tasks_per_hour() {
            Ok(data) => {
                self.metrics_data.set_tasks_per_hour(data);
            }
            Err(_) => {}
        }

        // Get 7-day model performance aggregation
        match db.get_model_performance_7day() {
            Ok(models) => {
                self.metrics_data.set_model_performance_7day(models);
            }
            Err(_) => {}
        }

        // Get 7-day worker efficiency aggregation
        match db.get_worker_efficiency_7day() {
            Ok(workers) => {
                self.metrics_data.set_worker_efficiency_7day(workers);
            }
            Err(_) => {}
        }

        // Get average cost by model
        match db.get_avg_cost_per_task_by_model() {
            Ok(data) => {
                self.metrics_data.set_avg_cost_by_model(data);
            }
            Err(_) => {}
        }
    }

    /// Poll log watcher for real-time API usage updates.
    fn poll_log_watcher(&mut self) {
        // Collect calls to persist to database
        let mut calls_to_persist: Vec<forge_cost::ApiCall> = Vec::new();

        // Process events from log watcher channel
        if let Some(ref rx) = self.log_rx {
            while let Ok(event) = rx.try_recv() {
                match event {
                    LogWatcherEvent::ApiCallParsed { call } => {
                        // Update real-time metrics
                        self.realtime_metrics.record_call(&call);

                        // Collect for database persistence
                        calls_to_persist.push(call.clone());

                        // Add activity entry for API call
                        let cost_str = if call.cost_usd >= 1.0 {
                            format!("${:.2}", call.cost_usd)
                        } else if call.cost_usd >= 0.01 {
                            format!("${:.3}", call.cost_usd)
                        } else {
                            format!("${:.4}", call.cost_usd)
                        };
                        let msg = format!(
                            "{} API call - {} in, {} out ({})",
                            call.model, call.input_tokens, call.output_tokens, cost_str
                        );
                        self.activity_data.push(
                            ActivityEntry::new(ActivityEventType::ApiCall, msg)
                                .with_source(&call.worker_id),
                        );

                        // The real-time metrics are now available for panels
                        // via self.realtime_metrics
                        self.dirty = true;
                    }
                    LogWatcherEvent::FileDiscovered { path, worker_id } => {
                        tracing::debug!("Log file discovered: {:?} for worker {}", path, worker_id);
                    }
                    LogWatcherEvent::FileRotated { path, worker_id } => {
                        tracing::debug!("Log file rotated: {:?} for worker {}", path, worker_id);
                    }
                    LogWatcherEvent::Error { message } => {
                        tracing::warn!("Log watcher error: {}", message);
                    }
                }
            }
        }

        // Also poll the log watcher directly for any new content
        if let Some(ref mut watcher) = self.log_watcher {
            let events = watcher.poll();
            for event in events {
                if let LogWatcherEvent::ApiCallParsed { call } = event {
                    self.realtime_metrics.record_call(&call);
                    calls_to_persist.push(call);
                    self.dirty = true;
                }
            }
        }

        // Persist collected API calls to the database for historical tracking
        if !calls_to_persist.is_empty() {
            if let Some(ref db) = self.cost_db {
                if let Err(e) = db.insert_api_calls(&calls_to_persist) {
                    tracing::warn!(
                        "Failed to persist {} API calls to database: {}",
                        calls_to_persist.len(),
                        e
                    );
                } else {
                    tracing::debug!("Persisted {} API calls to database", calls_to_persist.len());
                }
            }
        }

        // Propagate realtime metrics to cost_data and metrics_data for panel display
        if self.realtime_metrics.has_data() {
            self.cost_data.set_realtime(self.realtime_metrics.clone());
            self.metrics_data
                .set_realtime(self.realtime_metrics.clone());
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

    /// Get the alert badge summary for display.
    pub fn alert_badge(&self) -> AlertBadge {
        self.alert_manager.badge_summary()
    }

    /// Check if there are any active alerts.
    pub fn has_alerts(&self) -> bool {
        self.alert_manager.has_alerts()
    }

    /// Check if there are any unacknowledged alerts.
    pub fn has_unacknowledged_alerts(&self) -> bool {
        self.alert_manager.has_unacknowledged()
    }

    /// Acknowledge all alerts.
    pub fn acknowledge_all_alerts(&mut self) -> usize {
        let count = self.alert_manager.acknowledge_all();
        if count > 0 {
            self.dirty = true;
        }
        count
    }

    /// Configure the alert notifier from settings.
    pub fn configure_notifier(
        &mut self,
        bell_on_critical: bool,
        bell_on_warning: bool,
        bell_interval_secs: u64,
        visual_flash_enabled: bool,
    ) {
        self.alert_notifier.configure(
            bell_on_critical,
            bell_on_warning,
            bell_interval_secs,
            visual_flash_enabled,
        );
    }

    /// Check if the alert notifier has a pending bell to ring.
    /// Call this in the render loop and ring the bell if true.
    pub fn take_pending_bell(&mut self) -> bool {
        self.alert_notifier.take_pending_bell()
    }

    /// Check if visual flash is currently active.
    pub fn is_flashing(&self) -> bool {
        self.alert_notifier.is_flashing()
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

    // ============================================================
    // Initialization Performance Tests
    // ============================================================
    //
    // These tests verify that DataManager initialization is fast enough
    // to meet the < 2 second startup target. Each component should
    // initialize within 500ms.

    /// Test that DataManager initialization completes within 2 seconds.
    /// This is the primary performance validation for fg-2ir.
    #[test]
    fn test_datamanager_init_under_2_seconds() {
        use std::time::{Duration, Instant};

        let start = Instant::now();
        let _manager = DataManager::new();
        let elapsed = start.elapsed();

        // DataManager::new() should complete in under 2 seconds
        assert!(
            elapsed < Duration::from_secs(2),
            "DataManager::new() took {:?}, exceeds 2 second target",
            elapsed
        );

        // In practice, it should be much faster (< 10ms typically)
        // This is a sanity check, not a strict requirement
        if elapsed > Duration::from_millis(100) {
            eprintln!(
                "Warning: DataManager::new() took {:?}, consider optimization",
                elapsed
            );
        }
    }

    /// Test initialization with a temporary empty status directory.
    /// This simulates a fresh installation with no workers.
    #[test]
    fn test_datamanager_init_empty_status_dir() {
        use std::time::{Duration, Instant};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        std::fs::create_dir_all(&status_dir).unwrap();

        let start = Instant::now();
        let manager = DataManager::with_status_dir(status_dir);
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(2),
            "DataManager with empty status dir took {:?}",
            elapsed
        );

        // Verify manager works correctly (initial state has no workers from status files)
        // Note: worker_counts() returns 0 before first poll, which is correct for fast init
        assert_eq!(manager.worker_counts().total, 0);
    }

    /// Test initialization with 100 status files to verify O(n) scaling.
    /// This tests the scenario described in fg-2ir where users may have
    /// many workers running.
    #[test]
    fn test_datamanager_init_100_status_files() {
        use crate::status::{StatusWatcher, StatusWatcherConfig};
        use std::time::{Duration, Instant};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        std::fs::create_dir_all(&status_dir).unwrap();

        // Create 100 status files
        for i in 0..100 {
            let content = format!(
                r#"{{"worker_id": "worker-{}", "status": "active", "model": "test-model"}}"#,
                i
            );
            std::fs::write(status_dir.join(format!("worker-{}.json", i)), content).unwrap();
        }

        // Test StatusWatcher directly (which is responsible for loading status files)
        let config = StatusWatcherConfig::default().with_status_dir(&status_dir);

        let start = Instant::now();
        let watcher = StatusWatcher::new(config).unwrap();
        let elapsed = start.elapsed();

        // Even with 100 files, should complete well under 500ms
        assert!(
            elapsed < Duration::from_millis(500),
            "StatusWatcher with 100 status files took {:?}",
            elapsed
        );

        // Verify all workers were loaded during initial scan
        assert_eq!(
            watcher.workers().len(),
            100,
            "Should have loaded all 100 workers"
        );
    }

    /// Test that StatusWatcher initialization is fast (< 100ms target).
    #[test]
    fn test_statuswatcher_init_performance() {
        use crate::status::{StatusWatcher, StatusWatcherConfig};
        use std::time::{Duration, Instant};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        std::fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default().with_status_dir(&status_dir);

        let start = Instant::now();
        let _watcher = StatusWatcher::new(config).unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(100),
            "StatusWatcher init took {:?}, exceeds 100ms target",
            elapsed
        );
    }

    /// Test that BeadManager initialization is fast (< 50ms target).
    #[test]
    fn test_beadmanager_init_performance() {
        use crate::bead::BeadManager;
        use std::time::{Duration, Instant};

        let start = Instant::now();
        let mut manager = BeadManager::new();
        manager.add_default_workspaces();
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(50),
            "BeadManager init took {:?}, exceeds 50ms target",
            elapsed
        );
    }
}
