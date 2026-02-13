//! Context injection for dashboard state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::error::Result;

/// Dashboard context for tool execution.
///
/// This structure provides the current state of the dashboard to tools,
/// including worker status, task queue, costs, and subscriptions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DashboardContext {
    /// Current workers in the pool.
    pub workers: Vec<WorkerInfo>,

    /// Current task queue.
    pub tasks: Vec<TaskInfo>,

    /// Today's cost analytics.
    pub costs_today: CostAnalytics,

    /// Projected costs for the month.
    pub costs_projected: CostAnalytics,

    /// Subscription usage.
    pub subscriptions: Vec<SubscriptionInfo>,

    /// Recent events.
    pub recent_events: Vec<EventInfo>,

    /// Context timestamp.
    pub timestamp: DateTime<Utc>,
}

impl DashboardContext {
    /// Create a new empty context.
    pub fn new() -> Self {
        Self {
            timestamp: Utc::now(),
            ..Default::default()
        }
    }

    /// Format context as a summary for the AI prompt.
    pub fn to_summary(&self) -> String {
        let mut summary = String::new();

        // Workers summary
        let healthy = self.workers.iter().filter(|w| w.is_healthy).count();
        let idle = self.workers.iter().filter(|w| w.is_idle).count();
        summary.push_str(&format!(
            "Workers: {} total ({} healthy, {} idle)\n",
            self.workers.len(),
            healthy,
            idle
        ));

        // Tasks summary
        let in_progress = self.tasks.iter().filter(|t| t.in_progress).count();
        summary.push_str(&format!(
            "Tasks: {} ready, {} in progress\n",
            self.tasks.len(),
            in_progress
        ));

        // Costs summary
        summary.push_str(&format!(
            "Costs today: ${:.2}\n",
            self.costs_today.total_cost_usd
        ));

        // Subscriptions summary
        for sub in &self.subscriptions {
            if let Some(limit) = sub.quota_limit {
                summary.push_str(&format!(
                    "{}: {}/{} ({:.0}%)\n",
                    sub.name,
                    sub.quota_used,
                    limit,
                    (sub.quota_used as f64 / limit as f64) * 100.0
                ));
            }
        }

        summary
    }
}

/// Worker information for context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerInfo {
    /// Session name.
    pub session_name: String,

    /// Worker type (e.g., "sonnet", "opus", "glm").
    pub worker_type: String,

    /// Workspace path.
    pub workspace: String,

    /// Whether the worker is healthy.
    pub is_healthy: bool,

    /// Whether the worker is idle.
    pub is_idle: bool,

    /// Current task (if any).
    pub current_task: Option<String>,

    /// Uptime in seconds.
    pub uptime_secs: u64,

    /// Last activity timestamp.
    pub last_activity: Option<DateTime<Utc>>,

    /// Beads completed.
    pub beads_completed: u32,
}

/// Task information for context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskInfo {
    /// Task/bead ID.
    pub id: String,

    /// Task title.
    pub title: String,

    /// Priority (P0-P4).
    pub priority: String,

    /// Workspace path.
    pub workspace: String,

    /// Whether the task is in progress.
    pub in_progress: bool,

    /// Assigned model (if any).
    pub assigned_model: Option<String>,

    /// Estimated tokens.
    pub estimated_tokens: Option<u64>,
}

/// Cost analytics for context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostAnalytics {
    /// Time period.
    pub timeframe: String,

    /// Total cost in USD.
    pub total_cost_usd: f64,

    /// Cost by model.
    pub by_model: Vec<ModelCost>,

    /// Cost by priority.
    pub by_priority: Vec<PriorityCost>,
}

/// Cost breakdown by model.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelCost {
    /// Model name.
    pub model: String,

    /// Cost in USD.
    pub cost_usd: f64,

    /// Percentage of total.
    pub percentage: f64,
}

/// Cost breakdown by priority.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PriorityCost {
    /// Priority level.
    pub priority: String,

    /// Cost in USD.
    pub cost_usd: f64,
}

/// Subscription information for context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubscriptionInfo {
    /// Subscription name.
    pub name: String,

    /// Quota used.
    pub quota_used: i64,

    /// Quota limit.
    pub quota_limit: Option<i64>,

    /// Time until reset.
    pub reset_time: String,

    /// Status (on_pace, accelerate, max_out, depleted).
    pub status: String,
}

/// Event information for activity log.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventInfo {
    /// Event timestamp.
    pub timestamp: DateTime<Utc>,

    /// Event type.
    pub event_type: String,

    /// Event description.
    pub description: String,

    /// Related worker (if any).
    pub worker: Option<String>,

    /// Related bead (if any).
    pub bead_id: Option<String>,
}

/// Provider for dashboard context.
///
/// This trait allows different implementations for gathering context,
/// such as live data from the system or mock data for testing.
#[async_trait::async_trait]
pub trait ContextSource: Send + Sync {
    /// Gather the current dashboard context.
    async fn gather(&self) -> Result<DashboardContext>;
}

/// Context provider that caches context for a short duration.
pub struct ContextProvider {
    source: Arc<dyn ContextSource>,
    cache: RwLock<Option<CachedContext>>,
    cache_duration_secs: u64,
}

struct CachedContext {
    context: DashboardContext,
    cached_at: std::time::Instant,
}

impl ContextProvider {
    /// Create a new context provider.
    pub fn new(source: impl ContextSource + 'static) -> Self {
        Self {
            source: Arc::new(source),
            cache: RwLock::new(None),
            cache_duration_secs: 5, // 5 second cache by default
        }
    }

    /// Create with custom cache duration.
    pub fn with_cache_duration(mut self, secs: u64) -> Self {
        self.cache_duration_secs = secs;
        self
    }

    /// Get the current context (from cache if fresh).
    pub async fn get_context(&self) -> Result<DashboardContext> {
        // Check cache
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.as_ref()
                && cached.cached_at.elapsed().as_secs() < self.cache_duration_secs
            {
                return Ok(cached.context.clone());
            }
        }

        // Gather fresh context
        let context = self.source.gather().await?;

        // Update cache
        {
            let mut cache = self.cache.write().await;
            *cache = Some(CachedContext {
                context: context.clone(),
                cached_at: std::time::Instant::now(),
            });
        }

        Ok(context)
    }

    /// Force refresh the context.
    pub async fn refresh(&self) -> Result<DashboardContext> {
        let context = self.source.gather().await?;

        {
            let mut cache = self.cache.write().await;
            *cache = Some(CachedContext {
                context: context.clone(),
                cached_at: std::time::Instant::now(),
            });
        }

        Ok(context)
    }

    /// Invalidate the cache.
    pub async fn invalidate(&self) {
        let mut cache = self.cache.write().await;
        *cache = None;
    }
}

/// Mock context source for testing.
pub struct MockContextSource {
    context: DashboardContext,
}

impl MockContextSource {
    /// Create a new mock context source.
    pub fn new(context: DashboardContext) -> Self {
        Self { context }
    }

    /// Create with sample data.
    pub fn with_sample_data() -> Self {
        let context = DashboardContext {
            workers: vec![
                WorkerInfo {
                    session_name: "glm-alpha".to_string(),
                    worker_type: "glm".to_string(),
                    workspace: "/home/coder/ardenone-cluster".to_string(),
                    is_healthy: true,
                    is_idle: false,
                    current_task: Some("bd-123".to_string()),
                    uptime_secs: 3600,
                    last_activity: Some(Utc::now()),
                    beads_completed: 5,
                },
                WorkerInfo {
                    session_name: "glm-bravo".to_string(),
                    worker_type: "glm".to_string(),
                    workspace: "/home/coder/forge".to_string(),
                    is_healthy: true,
                    is_idle: true,
                    current_task: None,
                    uptime_secs: 1800,
                    last_activity: Some(Utc::now()),
                    beads_completed: 3,
                },
            ],
            tasks: vec![TaskInfo {
                id: "bd-456".to_string(),
                title: "Implement feature X".to_string(),
                priority: "P1".to_string(),
                workspace: "/home/coder/forge".to_string(),
                in_progress: false,
                assigned_model: Some("sonnet".to_string()),
                estimated_tokens: Some(50000),
            }],
            costs_today: CostAnalytics {
                timeframe: "today".to_string(),
                total_cost_usd: 12.43,
                by_model: vec![
                    ModelCost {
                        model: "opus".to_string(),
                        cost_usd: 8.24,
                        percentage: 66.0,
                    },
                    ModelCost {
                        model: "sonnet".to_string(),
                        cost_usd: 4.19,
                        percentage: 34.0,
                    },
                ],
                by_priority: vec![
                    PriorityCost {
                        priority: "P0".to_string(),
                        cost_usd: 8.41,
                    },
                    PriorityCost {
                        priority: "P1".to_string(),
                        cost_usd: 4.02,
                    },
                ],
            },
            costs_projected: CostAnalytics {
                timeframe: "month".to_string(),
                total_cost_usd: 373.00,
                by_model: vec![],
                by_priority: vec![],
            },
            subscriptions: vec![SubscriptionInfo {
                name: "Claude Pro".to_string(),
                quota_used: 328,
                quota_limit: Some(500),
                reset_time: "16d 9h".to_string(),
                status: "on_pace".to_string(),
            }],
            recent_events: vec![EventInfo {
                timestamp: Utc::now(),
                event_type: "completions".to_string(),
                description: "Completed bd-123".to_string(),
                worker: Some("glm-alpha".to_string()),
                bead_id: Some("bd-123".to_string()),
            }],
            timestamp: Utc::now(),
        };

        Self { context }
    }
}

#[async_trait::async_trait]
impl ContextSource for MockContextSource {
    async fn gather(&self) -> Result<DashboardContext> {
        Ok(self.context.clone())
    }
}

/// Real context source that reads live data from the dashboard.
///
/// This implementation reads:
/// - Worker status from ~/.forge/status/*.json
/// - Task queue from the workspace's .beads/ directory via `br` CLI
/// - Cost data from ~/.forge/costs.db
/// - Recent events from the chat audit log
pub struct RealContextSource {
    /// Path to the forge status directory (~/.forge/status/)
    status_dir: PathBuf,
    /// Path to the cost database (~/.forge/costs.db)
    cost_db_path: PathBuf,
    /// Path to the subscriptions config (~/.forge/subscriptions.yaml)
    subscriptions_path: PathBuf,
    /// Path to the chat audit log (~/.forge/chat-audit.jsonl)
    audit_log_path: PathBuf,
    /// Workspace path for beads (current working directory or FORGE_WORKSPACE)
    workspace: PathBuf,
}

impl RealContextSource {
    /// Create a new real context source with default paths.
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/coder".to_string());
        let forge_dir = PathBuf::from(&home).join(".forge");

        // Get workspace from environment or current directory
        let workspace = std::env::var("FORGE_WORKSPACE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from(&home))
            });

        Self {
            status_dir: forge_dir.join("status"),
            cost_db_path: forge_dir.join("costs.db"),
            subscriptions_path: forge_dir.join("subscriptions.yaml"),
            audit_log_path: forge_dir.join("chat-audit.jsonl"),
            workspace,
        }
    }

    /// Create with custom paths (for testing).
    pub fn with_paths(
        status_dir: PathBuf,
        cost_db_path: PathBuf,
        subscriptions_path: PathBuf,
        audit_log_path: PathBuf,
        workspace: PathBuf,
    ) -> Self {
        Self {
            status_dir,
            cost_db_path,
            subscriptions_path,
            audit_log_path,
            workspace,
        }
    }

    /// Read worker status files from the status directory.
    async fn read_workers(&self) -> Vec<WorkerInfo> {
        let mut workers = Vec::new();

        // Read status directory
        let status_dir = self.status_dir.clone();
        let entries = match tokio::fs::read_dir(&status_dir).await {
            Ok(entries) => entries,
            Err(e) => {
                debug!("Failed to read status directory {:?}: {}", status_dir, e);
                return workers;
            }
        };

        let mut entries = entries;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "json") {
                match self.read_worker_status(&path).await {
                    Ok(worker) => workers.push(worker),
                    Err(e) => {
                        debug!("Failed to read worker status from {:?}: {}", path, e);
                    }
                }
            }
        }

        workers
    }

    /// Read a single worker status file.
    async fn read_worker_status(&self, path: &PathBuf) -> std::io::Result<WorkerInfo> {
        let content = tokio::fs::read_to_string(path).await?;
        let status: WorkerStatusFile = serde_json::from_str(&content)?;

        // Parse timestamps
        let started_at = status.started_at.as_ref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        });

        let last_activity = status.last_activity.as_ref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        });

        // Calculate uptime
        let uptime_secs = if let Some(started) = started_at {
            (Utc::now() - started).num_seconds().max(0) as u64
        } else {
            0
        };

        // Determine worker type from model name
        let worker_type = if status.model.contains("sonnet") {
            "sonnet"
        } else if status.model.contains("opus") {
            "opus"
        } else if status.model.contains("haiku") {
            "haiku"
        } else if status.model.contains("glm") {
            "glm"
        } else {
            "unknown"
        }
        .to_string();

        // Check if healthy (active or idle status)
        let is_healthy = matches!(
            status.status.as_str(),
            "active" | "idle" | "starting"
        );

        Ok(WorkerInfo {
            session_name: status.worker_id,
            worker_type,
            workspace: status.workspace.unwrap_or_default(),
            is_healthy,
            is_idle: status.status == "idle",
            current_task: status.current_task,
            uptime_secs,
            last_activity,
            beads_completed: status.tasks_completed.unwrap_or(0) as u32,
        })
    }

    /// Read task queue from beads via br CLI.
    async fn read_tasks(&self) -> Vec<TaskInfo> {
        let mut tasks = Vec::new();

        // Use br CLI to get ready tasks
        let output = tokio::process::Command::new("br")
            .args(["ready", "--format", "json"])
            .current_dir(&self.workspace)
            .output()
            .await;

        if let Ok(output) = output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stdout.trim().is_empty() && stdout.trim() != "[]" {
                    if let Ok(beads) = serde_json::from_str::<Vec<BeadJson>>(&stdout) {
                        for bead in beads {
                            tasks.push(TaskInfo {
                                id: bead.id,
                                title: bead.title,
                                priority: format!("P{}", bead.priority),
                                workspace: self.workspace.to_string_lossy().to_string(),
                                in_progress: false,
                                assigned_model: bead.assignee,
                                estimated_tokens: None,
                            });
                        }
                    }
                }
            }
        }

        // Also get in-progress tasks
        let output = tokio::process::Command::new("br")
            .args(["list", "--status", "in_progress", "--format", "json"])
            .current_dir(&self.workspace)
            .output()
            .await;

        if let Ok(output) = output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stdout.trim().is_empty() && stdout.trim() != "[]" {
                    if let Ok(beads) = serde_json::from_str::<Vec<BeadJson>>(&stdout) {
                        for bead in beads {
                            tasks.push(TaskInfo {
                                id: bead.id,
                                title: bead.title,
                                priority: format!("P{}", bead.priority),
                                workspace: self.workspace.to_string_lossy().to_string(),
                                in_progress: true,
                                assigned_model: bead.assignee,
                                estimated_tokens: None,
                            });
                        }
                    }
                }
            }
        }

        tasks
    }

    /// Read cost data from the database.
    async fn read_costs(&self) -> (CostAnalytics, CostAnalytics) {
        // Use forge-cost to query costs
        let db = match forge_cost::CostDatabase::open(&self.cost_db_path) {
            Ok(db) => db,
            Err(e) => {
                debug!("Failed to open cost database: {}", e);
                return (CostAnalytics::default(), CostAnalytics::default());
            }
        };

        let query = forge_cost::CostQuery::new(&db);

        // Get today's costs
        let today = query.get_today_costs().unwrap_or_else(|_| {
            forge_cost::DailyCost {
                date: chrono::Utc::now().date_naive(),
                total_cost_usd: 0.0,
                call_count: 0,
                total_tokens: 0,
                by_model: vec![],
            }
        });
        let costs_today = CostAnalytics {
            timeframe: "today".to_string(),
            total_cost_usd: today.total_cost_usd,
            by_model: today
                .by_model
                .iter()
                .map(|b| ModelCost {
                    model: b.model.clone(),
                    cost_usd: b.total_cost_usd,
                    percentage: if today.total_cost_usd > 0.0 {
                        (b.total_cost_usd / today.total_cost_usd) * 100.0
                    } else {
                        0.0
                    },
                })
                .collect(),
            by_priority: vec![], // Not tracked in forge-cost
        };

        // Get projected costs
        let projected = query.get_projected_costs(None).unwrap_or_else(|_| {
            forge_cost::ProjectedCost {
                current_total: 0.0,
                daily_rate: 0.0,
                days_remaining: 0,
                projected_total: 0.0,
                confidence: 0.0,
            }
        });
        let costs_projected = CostAnalytics {
            timeframe: "month".to_string(),
            total_cost_usd: projected.projected_total,
            by_model: vec![],
            by_priority: vec![],
        };

        (costs_today, costs_projected)
    }

    /// Read subscription data from config.
    async fn read_subscriptions(&self) -> Vec<SubscriptionInfo> {
        let content = match tokio::fs::read_to_string(&self.subscriptions_path).await {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to read subscriptions config: {}", e);
                return vec![];
            }
        };

        let config: SubscriptionsConfig = match serde_yaml::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to parse subscriptions config: {}", e);
                return vec![];
            }
        };

        config
            .subscriptions
            .into_iter()
            .filter(|s| s.active)
            .map(|s| {
                let usage_percentage = if let Some(limit) = s.monthly_tokens {
                    if limit > 0 {
                        (s.current_usage as f64 / limit as f64) * 100.0
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };

                let status = if usage_percentage >= 100.0 {
                    "depleted"
                } else if usage_percentage >= 90.0 {
                    "max_out"
                } else if usage_percentage < 50.0 {
                    "accelerate"
                } else {
                    "on_pace"
                };

                let reset_time = if let Some(ref renewal) = s.renewal_date {
                    renewal.clone()
                } else {
                    "monthly".to_string()
                };

                SubscriptionInfo {
                    name: s.provider,
                    quota_used: s.current_usage,
                    quota_limit: s.monthly_tokens,
                    reset_time,
                    status: status.to_string(),
                }
            })
            .collect()
    }

    /// Read recent events from audit log.
    async fn read_recent_events(&self) -> Vec<EventInfo> {
        let content = match tokio::fs::read_to_string(&self.audit_log_path).await {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to read audit log: {}", e);
                return vec![];
            }
        };

        // Parse JSONL and get last 10 entries
        let mut events = Vec::new();
        for line in content.lines().rev().take(10) {
            if let Ok(entry) = serde_json::from_str::<AuditLogEntry>(line) {
                events.push(EventInfo {
                    timestamp: entry.timestamp,
                    event_type: if entry.success {
                        "completions".to_string()
                    } else {
                        "error".to_string()
                    },
                    description: entry.command,
                    worker: None,
                    bead_id: None,
                });
            }
        }

        events
    }
}

impl Default for RealContextSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ContextSource for RealContextSource {
    async fn gather(&self) -> Result<DashboardContext> {
        // Gather all data concurrently
        let (workers, tasks, (costs_today, costs_projected), subscriptions, recent_events) = tokio::join!(
            self.read_workers(),
            self.read_tasks(),
            self.read_costs(),
            self.read_subscriptions(),
            self.read_recent_events()
        );

        Ok(DashboardContext {
            workers,
            tasks,
            costs_today,
            costs_projected,
            subscriptions,
            recent_events,
            timestamp: Utc::now(),
        })
    }
}

/// Worker status file format (from ~/.forge/status/*.json).
#[derive(Debug, Clone, Deserialize)]
struct WorkerStatusFile {
    worker_id: String,
    status: String,
    model: String,
    workspace: Option<String>,
    started_at: Option<String>,
    last_activity: Option<String>,
    current_task: Option<String>,
    tasks_completed: Option<i64>,
}

/// Bead JSON format from br CLI.
#[derive(Debug, Clone, Deserialize)]
struct BeadJson {
    id: String,
    title: String,
    priority: u8,
    #[serde(default)]
    assignee: Option<String>,
}

/// Subscriptions config format.
#[derive(Debug, Clone, Deserialize)]
struct SubscriptionsConfig {
    subscriptions: Vec<SubscriptionEntry>,
}

/// Single subscription entry.
#[derive(Debug, Clone, Deserialize)]
struct SubscriptionEntry {
    provider: String,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    current_usage: i64,
    monthly_tokens: Option<i64>,
    renewal_date: Option<String>,
}

/// Audit log entry format.
#[derive(Debug, Clone, Deserialize)]
struct AuditLogEntry {
    timestamp: DateTime<Utc>,
    command: String,
    #[serde(default)]
    success: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_context_summary() {
        let source = MockContextSource::with_sample_data();
        let context = source.gather().await.unwrap();

        let summary = context.to_summary();
        assert!(summary.contains("Workers: 2"));
        assert!(summary.contains("healthy"));
        assert!(summary.contains("Costs today:"));
    }

    #[tokio::test]
    async fn test_context_provider_caching() {
        let source = MockContextSource::with_sample_data();
        let provider = ContextProvider::new(source).with_cache_duration(10);

        // First call should gather
        let ctx1 = provider.get_context().await.unwrap();

        // Second call should use cache (same reference time approximately)
        let ctx2 = provider.get_context().await.unwrap();

        assert_eq!(ctx1.timestamp, ctx2.timestamp);
    }

    #[tokio::test]
    async fn test_context_provider_invalidate() {
        let source = MockContextSource::with_sample_data();
        let provider = ContextProvider::new(source).with_cache_duration(10);

        // Get context to populate cache
        let _ = provider.get_context().await.unwrap();

        // Invalidate
        provider.invalidate().await;

        // Next call should gather fresh
        let _ = provider.get_context().await.unwrap();
    }
}
