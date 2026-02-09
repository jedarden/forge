//! Context injection for dashboard state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

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
            if let Some(cached) = cache.as_ref() {
                if cached.cached_at.elapsed().as_secs() < self.cache_duration_secs {
                    return Ok(cached.context.clone());
                }
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
            tasks: vec![
                TaskInfo {
                    id: "bd-456".to_string(),
                    title: "Implement feature X".to_string(),
                    priority: "P1".to_string(),
                    workspace: "/home/coder/forge".to_string(),
                    in_progress: false,
                    assigned_model: Some("sonnet".to_string()),
                    estimated_tokens: Some(50000),
                },
            ],
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
            recent_events: vec![
                EventInfo {
                    timestamp: Utc::now(),
                    event_type: "completions".to_string(),
                    description: "Completed bd-123".to_string(),
                    worker: Some("glm-alpha".to_string()),
                    bead_id: Some("bd-123".to_string()),
                },
            ],
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
