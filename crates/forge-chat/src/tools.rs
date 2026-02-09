//! Chat tools for read-only queries and actions.

use ::async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::context::DashboardContext;
use crate::error::{ChatError, Result};

/// Trait for chat tools that can be invoked by the AI agent.
#[async_trait]
pub trait ChatTool: Send + Sync {
    /// Get the tool name.
    fn name(&self) -> &str;

    /// Get the tool description for the AI.
    fn description(&self) -> &str;

    /// Get the JSON schema for tool parameters.
    fn parameters_schema(&self) -> serde_json::Value;

    /// Whether this tool requires confirmation before execution.
    fn requires_confirmation(&self) -> bool {
        false
    }

    /// Execute the tool with the given parameters.
    async fn execute(
        &self,
        params: serde_json::Value,
        context: &DashboardContext,
    ) -> Result<ToolResult>;

    /// Get confirmation details for the action (if requires_confirmation is true).
    async fn get_confirmation(
        &self,
        params: &serde_json::Value,
        context: &DashboardContext,
    ) -> Option<ActionConfirmation> {
        let _ = (params, context);
        None
    }
}

/// Result from a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Whether the tool execution was successful.
    pub success: bool,

    /// The result data (JSON).
    pub data: serde_json::Value,

    /// Human-readable message.
    pub message: String,

    /// Any side effects that occurred.
    #[serde(default)]
    pub side_effects: Vec<SideEffect>,
}

impl ToolResult {
    /// Create a successful result.
    pub fn success(data: impl Serialize, message: impl Into<String>) -> Self {
        Self {
            success: true,
            data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
            message: message.into(),
            side_effects: vec![],
        }
    }

    /// Create a failed result.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: serde_json::Value::Null,
            message: message.into(),
            side_effects: vec![],
        }
    }

    /// Add a side effect.
    pub fn with_side_effect(mut self, effect: SideEffect) -> Self {
        self.side_effects.push(effect);
        self
    }
}

/// A side effect from a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideEffect {
    /// Type of side effect.
    pub effect_type: String,

    /// Description of what happened.
    pub description: String,

    /// Additional data.
    pub data: Option<serde_json::Value>,
}

/// Confirmation details for an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionConfirmation {
    /// Title of the confirmation dialog.
    pub title: String,

    /// Description of what will happen.
    pub description: String,

    /// Warning level (info, warning, danger).
    pub level: ConfirmationLevel,

    /// Estimated cost impact (if applicable).
    pub cost_impact: Option<f64>,

    /// Items that will be affected.
    pub affected_items: Vec<String>,

    /// Whether this action is reversible.
    pub reversible: bool,
}

/// Confirmation level for actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationLevel {
    /// Informational (no danger)
    Info,
    /// Warning (some risk)
    Warning,
    /// Danger (high risk, destructive)
    Danger,
}

/// A tool call from the AI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Tool name.
    pub name: String,

    /// Tool parameters (JSON).
    pub parameters: serde_json::Value,

    /// Tool call ID (from the API).
    pub id: Option<String>,
}

/// Registry of available tools.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn ChatTool>>,
}

impl ToolRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool.
    pub fn register(&mut self, tool: impl ChatTool + 'static) {
        let name = tool.name().to_string();
        self.tools.insert(name, Arc::new(tool));
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn ChatTool>> {
        self.tools.get(name).cloned()
    }

    /// Get all tool definitions for the API.
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.parameters_schema(),
            })
            .collect()
    }

    /// List all tool names.
    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// Execute a tool call.
    pub async fn execute(
        &self,
        call: &ToolCall,
        context: &DashboardContext,
    ) -> Result<ToolResult> {
        let tool = self
            .get(&call.name)
            .ok_or_else(|| ChatError::ToolNotFound(call.name.clone()))?;

        // Check if confirmation is required
        if tool.requires_confirmation() {
            if let Some(confirmation) = tool.get_confirmation(&call.parameters, context).await {
                return Err(ChatError::ConfirmationRequired(serde_json::to_string(
                    &confirmation,
                )?));
            }
        }

        tool.execute(call.parameters.clone(), context).await
    }

    /// Create a registry with all built-in tools.
    pub fn with_builtin_tools() -> Self {
        let mut registry = Self::new();

        // Read-only tools
        registry.register(GetWorkerStatusTool);
        registry.register(GetTaskQueueTool);
        registry.register(GetCostAnalyticsTool);
        registry.register(GetSubscriptionUsageTool);
        registry.register(GetActivityLogTool);

        // Action tools
        registry.register(SpawnWorkerTool);
        registry.register(KillWorkerTool);
        registry.register(AssignTaskTool);
        registry.register(PauseWorkersTool);
        registry.register(ResumeWorkersTool);

        registry
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::with_builtin_tools()
    }
}

/// Tool definition for the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name.
    pub name: String,

    /// Tool description.
    pub description: String,

    /// Input schema (JSON Schema).
    pub input_schema: serde_json::Value,
}

// ============ Built-in Read-Only Tools ============

/// Get worker status tool.
pub struct GetWorkerStatusTool;

#[async_trait]
impl ChatTool for GetWorkerStatusTool {
    fn name(&self) -> &str {
        "get_worker_status"
    }

    fn description(&self) -> &str {
        "Get the current status of all workers in the pool, including their health, activity, and assigned tasks."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "worker_id": {
                    "type": "string",
                    "description": "Optional: Filter by specific worker ID"
                },
                "status_filter": {
                    "type": "string",
                    "enum": ["all", "healthy", "idle", "unhealthy"],
                    "description": "Filter workers by status"
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        context: &DashboardContext,
    ) -> Result<ToolResult> {
        let worker_id = params.get("worker_id").and_then(|v| v.as_str());
        let status_filter = params
            .get("status_filter")
            .and_then(|v| v.as_str())
            .unwrap_or("all");

        let workers = if let Some(id) = worker_id {
            context
                .workers
                .iter()
                .filter(|w| w.session_name == id)
                .cloned()
                .collect::<Vec<_>>()
        } else {
            context.workers.clone()
        };

        let filtered: Vec<_> = workers
            .into_iter()
            .filter(|w| match status_filter {
                "healthy" => w.is_healthy,
                "idle" => w.is_idle,
                "unhealthy" => !w.is_healthy,
                _ => true,
            })
            .collect();

        let total = context.workers.len();
        let healthy = context.workers.iter().filter(|w| w.is_healthy).count();
        let idle = context.workers.iter().filter(|w| w.is_idle).count();

        Ok(ToolResult::success(
            serde_json::json!({
                "total": total,
                "healthy": healthy,
                "idle": idle,
                "unhealthy": total - healthy,
                "workers": filtered
            }),
            format!("Found {} workers ({} healthy, {} idle)", total, healthy, idle),
        ))
    }
}

/// Get task queue tool.
pub struct GetTaskQueueTool;

#[async_trait]
impl ChatTool for GetTaskQueueTool {
    fn name(&self) -> &str {
        "get_task_queue"
    }

    fn description(&self) -> &str {
        "Get the current task queue with ready beads, including priority and assignment information."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "workspace": {
                    "type": "string",
                    "description": "Optional: Filter by workspace path"
                },
                "priority": {
                    "type": "string",
                    "enum": ["P0", "P1", "P2", "P3", "P4"],
                    "description": "Optional: Filter by priority"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of tasks to return (default: 20)"
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        context: &DashboardContext,
    ) -> Result<ToolResult> {
        let workspace = params.get("workspace").and_then(|v| v.as_str());
        let priority = params.get("priority").and_then(|v| v.as_str());
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;

        let mut tasks = context.tasks.clone();

        if let Some(ws) = workspace {
            tasks.retain(|t| t.workspace.contains(ws));
        }

        if let Some(p) = priority {
            tasks.retain(|t| t.priority == p);
        }

        let total = tasks.len();
        tasks.truncate(limit);

        let in_progress = context.tasks.iter().filter(|t| t.in_progress).count();

        Ok(ToolResult::success(
            serde_json::json!({
                "total_ready": total,
                "in_progress": in_progress,
                "beads": tasks
            }),
            format!("{} ready tasks, {} in progress", total, in_progress),
        ))
    }
}

/// Get cost analytics tool.
pub struct GetCostAnalyticsTool;

#[async_trait]
impl ChatTool for GetCostAnalyticsTool {
    fn name(&self) -> &str {
        "get_cost_analytics"
    }

    fn description(&self) -> &str {
        "Get cost analytics including spending by model, time period, and projected costs."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "timeframe": {
                    "type": "string",
                    "enum": ["today", "yesterday", "week", "month", "projected"],
                    "description": "Time period for cost analysis (default: today)"
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        context: &DashboardContext,
    ) -> Result<ToolResult> {
        let timeframe = params
            .get("timeframe")
            .and_then(|v| v.as_str())
            .unwrap_or("today");

        // Return the cost analytics from context
        let costs = match timeframe {
            "today" => &context.costs_today,
            "projected" => &context.costs_projected,
            _ => &context.costs_today,
        };

        Ok(ToolResult::success(
            costs,
            format!("Cost analytics for {}", timeframe),
        ))
    }
}

/// Get subscription usage tool.
pub struct GetSubscriptionUsageTool;

#[async_trait]
impl ChatTool for GetSubscriptionUsageTool {
    fn name(&self) -> &str {
        "get_subscription_usage"
    }

    fn description(&self) -> &str {
        "Get subscription usage and quota status for all configured subscriptions."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        context: &DashboardContext,
    ) -> Result<ToolResult> {
        Ok(ToolResult::success(
            &context.subscriptions,
            format!("{} subscriptions tracked", context.subscriptions.len()),
        ))
    }
}

/// Get activity log tool.
pub struct GetActivityLogTool;

#[async_trait]
impl ChatTool for GetActivityLogTool {
    fn name(&self) -> &str {
        "get_activity_log"
    }

    fn description(&self) -> &str {
        "Get recent activity log entries including worker events, task completions, and errors."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "hours": {
                    "type": "integer",
                    "description": "Number of hours of history to retrieve (default: 1)"
                },
                "filter": {
                    "type": "string",
                    "enum": ["all", "spawns", "completions", "errors"],
                    "description": "Filter by event type"
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        context: &DashboardContext,
    ) -> Result<ToolResult> {
        let _hours = params.get("hours").and_then(|v| v.as_u64()).unwrap_or(1);
        let filter = params.get("filter").and_then(|v| v.as_str()).unwrap_or("all");

        let mut events = context.recent_events.clone();

        if filter != "all" {
            events.retain(|e| e.event_type == filter);
        }

        Ok(ToolResult::success(
            serde_json::json!({
                "events": events
            }),
            format!("{} events in activity log", events.len()),
        ))
    }
}

// ============ Built-in Action Tools ============

/// Spawn worker tool.
pub struct SpawnWorkerTool;

#[async_trait]
impl ChatTool for SpawnWorkerTool {
    fn name(&self) -> &str {
        "spawn_worker"
    }

    fn description(&self) -> &str {
        "Spawn one or more new workers with a specified model type."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "worker_type": {
                    "type": "string",
                    "enum": ["sonnet", "opus", "haiku", "glm"],
                    "description": "Type of worker to spawn"
                },
                "count": {
                    "type": "integer",
                    "description": "Number of workers to spawn (default: 1)"
                },
                "workspace": {
                    "type": "string",
                    "description": "Optional: Workspace to assign workers to"
                }
            },
            "required": ["worker_type"]
        })
    }

    fn requires_confirmation(&self) -> bool {
        true
    }

    async fn get_confirmation(
        &self,
        params: &serde_json::Value,
        _context: &DashboardContext,
    ) -> Option<ActionConfirmation> {
        let count = params.get("count").and_then(|v| v.as_u64()).unwrap_or(1);
        let worker_type = params
            .get("worker_type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        // Auto-confirm if spawning 2 or fewer workers
        if count <= 2 {
            return None;
        }

        Some(ActionConfirmation {
            title: format!("Spawn {} {} workers?", count, worker_type),
            description: format!(
                "This will spawn {} new {} workers. This may increase costs.",
                count, worker_type
            ),
            level: ConfirmationLevel::Warning,
            cost_impact: None,
            affected_items: vec![format!("{} new workers", count)],
            reversible: true,
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _context: &DashboardContext,
    ) -> Result<ToolResult> {
        let worker_type = params
            .get("worker_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ChatError::ToolExecutionFailed("worker_type is required".to_string()))?;

        let count = params.get("count").and_then(|v| v.as_u64()).unwrap_or(1);
        let workspace = params.get("workspace").and_then(|v| v.as_str());

        // In a real implementation, this would call the worker launcher
        // For now, we return a placeholder result
        let spawned_workers: Vec<String> = (0..count)
            .map(|i| format!("{}-worker-{}", worker_type, i + 1))
            .collect();

        let mut result = ToolResult::success(
            serde_json::json!({
                "spawned": spawned_workers,
                "count": count,
                "worker_type": worker_type,
                "workspace": workspace
            }),
            format!("Spawned {} {} worker(s)", count, worker_type),
        );

        for worker in &spawned_workers {
            result = result.with_side_effect(SideEffect {
                effect_type: "spawn".to_string(),
                description: format!("Spawned worker {}", worker),
                data: Some(serde_json::json!({"session": worker})),
            });
        }

        Ok(result)
    }
}

/// Kill worker tool.
pub struct KillWorkerTool;

#[async_trait]
impl ChatTool for KillWorkerTool {
    fn name(&self) -> &str {
        "kill_worker"
    }

    fn description(&self) -> &str {
        "Kill/terminate a specific worker by session name."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "session_name": {
                    "type": "string",
                    "description": "The session name of the worker to kill"
                }
            },
            "required": ["session_name"]
        })
    }

    fn requires_confirmation(&self) -> bool {
        true
    }

    async fn get_confirmation(
        &self,
        params: &serde_json::Value,
        context: &DashboardContext,
    ) -> Option<ActionConfirmation> {
        let session_name = params.get("session_name").and_then(|v| v.as_str())?;

        // Find the worker
        let worker = context
            .workers
            .iter()
            .find(|w| w.session_name == session_name)?;

        Some(ActionConfirmation {
            title: format!("Kill worker {}?", session_name),
            description: if worker.current_task.is_some() {
                format!(
                    "Worker {} is currently working on a task. Killing it will interrupt the task.",
                    session_name
                )
            } else {
                format!("Worker {} will be terminated.", session_name)
            },
            level: if worker.current_task.is_some() {
                ConfirmationLevel::Danger
            } else {
                ConfirmationLevel::Warning
            },
            cost_impact: None,
            affected_items: vec![session_name.to_string()],
            reversible: true,
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _context: &DashboardContext,
    ) -> Result<ToolResult> {
        let session_name = params
            .get("session_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ChatError::ToolExecutionFailed("session_name is required".to_string())
            })?;

        // In a real implementation, this would call tmux to kill the session
        Ok(ToolResult::success(
            serde_json::json!({
                "killed": session_name
            }),
            format!("Killed worker {}", session_name),
        )
        .with_side_effect(SideEffect {
            effect_type: "kill".to_string(),
            description: format!("Killed worker {}", session_name),
            data: Some(serde_json::json!({"session": session_name})),
        }))
    }
}

/// Assign task tool.
pub struct AssignTaskTool;

#[async_trait]
impl ChatTool for AssignTaskTool {
    fn name(&self) -> &str {
        "assign_task"
    }

    fn description(&self) -> &str {
        "Assign or reassign a task/bead to a specific model tier."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "bead_id": {
                    "type": "string",
                    "description": "The bead/task ID to assign"
                },
                "model": {
                    "type": "string",
                    "enum": ["sonnet", "opus", "haiku", "glm"],
                    "description": "Target model for the task"
                }
            },
            "required": ["bead_id", "model"]
        })
    }

    fn requires_confirmation(&self) -> bool {
        true
    }

    async fn get_confirmation(
        &self,
        params: &serde_json::Value,
        context: &DashboardContext,
    ) -> Option<ActionConfirmation> {
        let bead_id = params.get("bead_id").and_then(|v| v.as_str())?;
        let new_model = params.get("model").and_then(|v| v.as_str())?;

        // Find the task
        let task = context.tasks.iter().find(|t| t.id == bead_id)?;

        // Estimate cost difference
        let cost_impact = match (task.assigned_model.as_deref(), new_model) {
            (Some("glm"), "opus") => Some(22.50),
            (Some("sonnet"), "opus") => Some(18.00),
            (Some("opus"), "sonnet") => Some(-18.00),
            _ => None,
        };

        Some(ActionConfirmation {
            title: format!("Reassign {} to {}?", bead_id, new_model),
            description: format!(
                "Task {} will be reassigned from {} to {}.",
                bead_id,
                task.assigned_model.as_deref().unwrap_or("unassigned"),
                new_model
            ),
            level: if task.in_progress {
                ConfirmationLevel::Danger
            } else {
                ConfirmationLevel::Info
            },
            cost_impact,
            affected_items: vec![bead_id.to_string()],
            reversible: true,
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _context: &DashboardContext,
    ) -> Result<ToolResult> {
        let bead_id = params.get("bead_id").and_then(|v| v.as_str()).ok_or_else(|| {
            ChatError::ToolExecutionFailed("bead_id is required".to_string())
        })?;

        let model = params.get("model").and_then(|v| v.as_str()).ok_or_else(|| {
            ChatError::ToolExecutionFailed("model is required".to_string())
        })?;

        Ok(ToolResult::success(
            serde_json::json!({
                "bead_id": bead_id,
                "new_model": model,
                "status": "reassigned"
            }),
            format!("Reassigned {} to {}", bead_id, model),
        )
        .with_side_effect(SideEffect {
            effect_type: "assign".to_string(),
            description: format!("Assigned {} to {}", bead_id, model),
            data: Some(serde_json::json!({"bead_id": bead_id, "model": model})),
        }))
    }
}

/// Pause workers tool.
pub struct PauseWorkersTool;

#[async_trait]
impl ChatTool for PauseWorkersTool {
    fn name(&self) -> &str {
        "pause_workers"
    }

    fn description(&self) -> &str {
        "Pause all workers for a specified duration."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "duration_minutes": {
                    "type": "integer",
                    "description": "Duration to pause in minutes (default: 5)"
                }
            }
        })
    }

    fn requires_confirmation(&self) -> bool {
        true
    }

    async fn get_confirmation(
        &self,
        params: &serde_json::Value,
        context: &DashboardContext,
    ) -> Option<ActionConfirmation> {
        let duration = params
            .get("duration_minutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(5);

        // Only require confirmation for long pauses
        if duration <= 10 {
            return None;
        }

        let active_count = context.workers.iter().filter(|w| !w.is_idle).count();

        Some(ActionConfirmation {
            title: format!("Pause all workers for {} minutes?", duration),
            description: format!(
                "This will pause {} workers. {} are currently active and will be interrupted.",
                context.workers.len(),
                active_count
            ),
            level: ConfirmationLevel::Warning,
            cost_impact: None,
            affected_items: context
                .workers
                .iter()
                .map(|w| w.session_name.clone())
                .collect(),
            reversible: true,
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        context: &DashboardContext,
    ) -> Result<ToolResult> {
        let duration = params
            .get("duration_minutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(5);

        Ok(ToolResult::success(
            serde_json::json!({
                "paused": context.workers.len(),
                "duration_minutes": duration,
                "resume_at": chrono::Utc::now() + chrono::Duration::minutes(duration as i64)
            }),
            format!("Paused {} workers for {} minutes", context.workers.len(), duration),
        ))
    }
}

/// Resume workers tool.
pub struct ResumeWorkersTool;

#[async_trait]
impl ChatTool for ResumeWorkersTool {
    fn name(&self) -> &str {
        "resume_workers"
    }

    fn description(&self) -> &str {
        "Resume all paused workers."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        context: &DashboardContext,
    ) -> Result<ToolResult> {
        Ok(ToolResult::success(
            serde_json::json!({
                "resumed": context.workers.len()
            }),
            format!("Resumed {} workers", context.workers.len()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_context() -> DashboardContext {
        DashboardContext::default()
    }

    #[tokio::test]
    async fn test_tool_registry() {
        let registry = ToolRegistry::with_builtin_tools();

        assert!(registry.get("get_worker_status").is_some());
        assert!(registry.get("spawn_worker").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[tokio::test]
    async fn test_worker_status_tool() {
        let tool = GetWorkerStatusTool;
        let context = create_test_context();

        let result = tool
            .execute(serde_json::json!({}), &context)
            .await
            .unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_spawn_worker_confirmation() {
        let tool = SpawnWorkerTool;
        let context = create_test_context();

        // Spawning 1 worker should not require confirmation
        let params = serde_json::json!({"worker_type": "sonnet", "count": 1});
        let confirmation = tool.get_confirmation(&params, &context).await;
        assert!(confirmation.is_none());

        // Spawning 5 workers should require confirmation
        let params = serde_json::json!({"worker_type": "sonnet", "count": 5});
        let confirmation = tool.get_confirmation(&params, &context).await;
        assert!(confirmation.is_some());
    }
}
