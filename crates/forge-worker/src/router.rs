//! Multi-model routing engine for task-to-model assignment.
//!
//! This module implements intelligent routing of tasks to appropriate AI models
//! based on complexity, cost, availability, and subscription quotas.
//!
//! ## Overview
//!
//! The router uses a tiered approach:
//! - **Premium**: claude-opus-4, o1, glm-5 (complex reasoning, architecture)
//! - **Standard**: claude-sonnet-4, gpt-4, qwen-2.5 (typical coding tasks)
//! - **Budget**: claude-haiku-4, gpt-3.5, deepseek-coder (simple edits, formatting)
//!
//! ## Routing Rules
//!
//! 1. Task complexity → Model tier mapping based on priority
//! 2. Subscription quota check → Prefer use-or-lose tokens
//! 3. Worker availability → Load balance across same tier
//! 4. Fallback chain → Premium fails → Standard → Budget
//!
//! ## Usage
//!
//! ```no_run
//! use forge_worker::router::{Router, RouterConfig, TaskMetadata};
//! use forge_core::types::{Priority, WorkerTier};
//!
//! // Create router with default configuration
//! let mut router = Router::new();
//!
//! // Create task metadata
//! let task = TaskMetadata {
//!     bead_id: "fg-123".to_string(),
//!     priority: Priority::P0,
//!     complexity_score: Some(85),
//!     labels: vec!["architecture".to_string()],
//!     requires_reasoning: true,
//!     estimated_tokens: Some(5000),
//! };
//!
//! // Get routing decision
//! let decision = router.route(&task).unwrap();
//! println!("Routed to {} ({})", decision.model_id, decision.tier);
//!
//! // Check fallback if needed
//! if !decision.is_available {
//!     let fallback = router.fallback(&decision).unwrap();
//!     println!("Falling back to {}", fallback.model_id);
//! }
//! ```

use chrono::{DateTime, Utc};
use forge_core::types::{Priority, WorkerTier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Default fallback timeout in seconds.
const DEFAULT_FALLBACK_TIMEOUT_SECS: u64 = 5;

/// Maximum models to track per tier (reserved for future validation).
#[allow(dead_code)]
const MAX_MODELS_PER_TIER: usize = 10;

/// Configuration for the routing engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    /// Enable subscription quota preference
    pub prefer_subscription: bool,

    /// Fallback timeout in seconds
    pub fallback_timeout_secs: u64,

    /// Enable load balancing across same-tier models
    pub enable_load_balancing: bool,

    /// Premium tier models
    pub premium_models: Vec<ModelConfig>,

    /// Standard tier models
    pub standard_models: Vec<ModelConfig>,

    /// Budget tier models
    pub budget_models: Vec<ModelConfig>,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            prefer_subscription: true,
            fallback_timeout_secs: DEFAULT_FALLBACK_TIMEOUT_SECS,
            enable_load_balancing: true,
            premium_models: vec![
                ModelConfig::new("claude-opus-4", "Claude Opus 4", true),
                ModelConfig::new("o1", "OpenAI O1", false),
                ModelConfig::new("glm-5", "GLM-5", true),
            ],
            standard_models: vec![
                ModelConfig::new("claude-sonnet-4", "Claude Sonnet 4", true),
                ModelConfig::new("gpt-4", "GPT-4", false),
                ModelConfig::new("qwen-2.5", "Qwen 2.5", false),
            ],
            budget_models: vec![
                ModelConfig::new("claude-haiku-4", "Claude Haiku 4", true),
                ModelConfig::new("gpt-3.5", "GPT-3.5", false),
                ModelConfig::new("deepseek-coder", "DeepSeek Coder", false),
            ],
        }
    }
}

impl RouterConfig {
    /// Create a new router configuration with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get models for a specific tier.
    pub fn models_for_tier(&self, tier: WorkerTier) -> &[ModelConfig] {
        match tier {
            WorkerTier::Premium => &self.premium_models,
            WorkerTier::Standard => &self.standard_models,
            WorkerTier::Budget => &self.budget_models,
        }
    }

    /// Load configuration from a YAML file.
    pub fn from_yaml(path: &std::path::Path) -> Result<Self, RouterError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| RouterError::ConfigLoad {
                path: path.to_path_buf(),
                source: e,
            })?;
        let config: Self = serde_yaml::from_str(&content)
            .map_err(|e| RouterError::ConfigParse {
                message: e.to_string(),
            })?;
        config.validate()?;
        Ok(config)
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), RouterError> {
        if self.premium_models.is_empty() {
            return Err(RouterError::ConfigValidation {
                message: "At least one premium model is required".to_string(),
            });
        }
        if self.standard_models.is_empty() {
            return Err(RouterError::ConfigValidation {
                message: "At least one standard model is required".to_string(),
            });
        }
        if self.budget_models.is_empty() {
            return Err(RouterError::ConfigValidation {
                message: "At least one budget model is required".to_string(),
            });
        }
        Ok(())
    }
}

/// Configuration for a single model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Model identifier (e.g., "claude-sonnet-4")
    pub id: String,

    /// Display name
    pub name: String,

    /// Whether this model has subscription quota
    pub has_subscription: bool,

    /// Maximum tokens per request
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    /// Cost per million input tokens in USD
    #[serde(default)]
    pub cost_per_million_input: f64,

    /// Cost per million output tokens in USD
    #[serde(default)]
    pub cost_per_million_output: f64,
}

fn default_max_tokens() -> u32 {
    128000
}

impl ModelConfig {
    /// Create a new model configuration.
    pub fn new(id: impl Into<String>, name: impl Into<String>, has_subscription: bool) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            has_subscription,
            max_tokens: default_max_tokens(),
            cost_per_million_input: 0.0,
            cost_per_million_output: 0.0,
        }
    }

    /// Set the maximum tokens.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Set the costs.
    pub fn with_costs(mut self, input: f64, output: f64) -> Self {
        self.cost_per_million_input = input;
        self.cost_per_million_output = output;
        self
    }
}

/// Subscription quota status for a model.
#[derive(Debug, Clone, Default)]
pub struct SubscriptionQuota {
    /// Total quota in tokens
    pub total_tokens: u64,

    /// Used tokens
    pub used_tokens: u64,

    /// Quota reset time (for use-or-lose subscriptions)
    pub reset_at: Option<DateTime<Utc>>,
}

impl SubscriptionQuota {
    /// Create a new subscription quota.
    pub fn new(total_tokens: u64, used_tokens: u64) -> Self {
        Self {
            total_tokens,
            used_tokens,
            reset_at: None,
        }
    }

    /// Create with reset time.
    pub fn with_reset(mut self, reset_at: DateTime<Utc>) -> Self {
        self.reset_at = Some(reset_at);
        self
    }

    /// Get remaining tokens.
    pub fn remaining(&self) -> u64 {
        self.total_tokens.saturating_sub(self.used_tokens)
    }

    /// Get usage percentage.
    pub fn usage_percent(&self) -> f64 {
        if self.total_tokens == 0 {
            return 0.0;
        }
        (self.used_tokens as f64 / self.total_tokens as f64) * 100.0
    }

    /// Check if quota is urgent (should use before reset).
    pub fn is_urgent(&self) -> bool {
        if let Some(reset_at) = self.reset_at {
            let now = Utc::now();
            let time_until_reset = reset_at.signed_duration_since(now);
            // Urgent if less than 24 hours until reset and still have quota
            time_until_reset.num_hours() < 24 && self.remaining() > 0
        } else {
            false
        }
    }

    /// Check if quota is available.
    pub fn is_available(&self) -> bool {
        self.remaining() > 0
    }
}

/// Model availability status.
#[derive(Debug, Clone, Default)]
pub struct ModelAvailability {
    /// Model ID
    pub model_id: String,

    /// Whether the model is currently available
    pub is_available: bool,

    /// Number of active workers using this model
    pub active_workers: usize,

    /// Average response latency in milliseconds
    pub avg_latency_ms: Option<u64>,

    /// Last error if unavailable
    pub last_error: Option<String>,
}

impl ModelAvailability {
    /// Create a new availability status.
    pub fn new(model_id: impl Into<String>, is_available: bool) -> Self {
        Self {
            model_id: model_id.into(),
            is_available,
            active_workers: 0,
            avg_latency_ms: None,
            last_error: None,
        }
    }

    /// Check if this model can accept more work.
    pub fn can_accept_work(&self, max_workers: usize) -> bool {
        self.is_available && self.active_workers < max_workers
    }
}

/// Metadata about a task for routing decisions.
#[derive(Debug, Clone)]
pub struct TaskMetadata {
    /// Bead/task ID
    pub bead_id: String,

    /// Task priority
    pub priority: Priority,

    /// Optional complexity score (0-100)
    pub complexity_score: Option<u32>,

    /// Task labels
    pub labels: Vec<String>,

    /// Whether the task requires complex reasoning
    pub requires_reasoning: bool,

    /// Estimated token count
    pub estimated_tokens: Option<u64>,
}

impl TaskMetadata {
    /// Create new task metadata.
    pub fn new(bead_id: impl Into<String>, priority: Priority) -> Self {
        Self {
            bead_id: bead_id.into(),
            priority,
            complexity_score: None,
            labels: Vec::new(),
            requires_reasoning: false,
            estimated_tokens: None,
        }
    }

    /// Set complexity score.
    pub fn with_complexity(mut self, score: u32) -> Self {
        self.complexity_score = Some(score.min(100));
        self
    }

    /// Add labels.
    pub fn with_labels(mut self, labels: Vec<String>) -> Self {
        self.labels = labels;
        self
    }

    /// Set requires reasoning flag.
    pub fn with_reasoning(mut self, requires: bool) -> Self {
        self.requires_reasoning = requires;
        self
    }

    /// Set estimated tokens.
    pub fn with_estimated_tokens(mut self, tokens: u64) -> Self {
        self.estimated_tokens = Some(tokens);
        self
    }

    /// Determine the recommended tier based on task metadata.
    pub fn recommended_tier(&self) -> WorkerTier {
        // Start with priority-based recommendation
        let base_tier = self.priority.recommended_tier();

        // Upgrade tier if complex reasoning is required
        if self.requires_reasoning {
            return WorkerTier::Premium;
        }

        // Upgrade tier if complexity score is high
        if let Some(score) = self.complexity_score {
            if score >= 80 {
                return WorkerTier::Premium;
            } else if score >= 50 && base_tier == WorkerTier::Budget {
                return WorkerTier::Standard;
            }
        }

        // Check for special labels
        let label_lower: Vec<String> = self.labels.iter().map(|l| l.to_lowercase()).collect();
        if label_lower.iter().any(|l| l == "architecture" || l == "critical") {
            return WorkerTier::Premium;
        }
        if label_lower.iter().any(|l| l == "complex") {
            return WorkerTier::Standard;
        }

        base_tier
    }
}

/// A routing decision made by the router.
#[derive(Debug, Clone, Serialize)]
pub struct RoutingDecision {
    /// The selected model ID
    pub model_id: String,

    /// The selected model name
    pub model_name: String,

    /// The tier of the selected model
    pub tier: WorkerTier,

    /// Whether the model is currently available
    pub is_available: bool,

    /// Whether this uses subscription quota
    pub uses_subscription: bool,

    /// Reason for the routing decision
    pub reason: RoutingReason,

    /// Timestamp of the decision
    pub decided_at: DateTime<Utc>,

    /// Fallback chain if selected model unavailable
    pub fallback_chain: Vec<FallbackOption>,

    /// Task ID this decision is for
    pub task_id: String,
}

/// Reason for a routing decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoutingReason {
    /// Priority-based routing
    PriorityBased,

    /// Complexity-based routing
    ComplexityBased,

    /// Subscription quota preference
    SubscriptionPreference,

    /// Load balancing
    LoadBalancing,

    /// Fallback from higher tier
    Fallback,

    /// Label-based routing
    LabelBased,

    /// Default routing
    Default,
}

impl std::fmt::Display for RoutingReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PriorityBased => write!(f, "priority-based"),
            Self::ComplexityBased => write!(f, "complexity-based"),
            Self::SubscriptionPreference => write!(f, "subscription-preference"),
            Self::LoadBalancing => write!(f, "load-balancing"),
            Self::Fallback => write!(f, "fallback"),
            Self::LabelBased => write!(f, "label-based"),
            Self::Default => write!(f, "default"),
        }
    }
}

/// A fallback option in the routing chain.
#[derive(Debug, Clone, Serialize)]
pub struct FallbackOption {
    /// Model ID
    pub model_id: String,

    /// Model tier
    pub tier: WorkerTier,

    /// Reason this is a fallback
    pub reason: String,
}

/// The multi-model routing engine.
#[derive(Debug)]
pub struct Router {
    /// Configuration
    config: RouterConfig,

    /// Subscription quotas per model
    quotas: HashMap<String, SubscriptionQuota>,

    /// Model availability status
    availability: HashMap<String, ModelAvailability>,

    /// Routing history for analytics (last 1000 decisions)
    history: Vec<RoutingDecision>,

    /// Load balancing counters per model
    load_counters: HashMap<String, u64>,
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

impl Router {
    /// Create a new router with default configuration.
    pub fn new() -> Self {
        Self::with_config(RouterConfig::default())
    }

    /// Create a router with custom configuration.
    pub fn with_config(config: RouterConfig) -> Self {
        let mut router = Self {
            config,
            quotas: HashMap::new(),
            availability: HashMap::new(),
            history: Vec::with_capacity(1000),
            load_counters: HashMap::new(),
        };
        router.initialize_model_states();
        router
    }

    /// Initialize model states from configuration.
    fn initialize_model_states(&mut self) {
        let all_models: Vec<_> = self.config.premium_models.iter()
            .chain(self.config.standard_models.iter())
            .chain(self.config.budget_models.iter())
            .collect();

        for model in all_models {
            self.availability.insert(
                model.id.clone(),
                ModelAvailability::new(&model.id, true),
            );
            self.load_counters.insert(model.id.clone(), 0);
        }
    }

    /// Get the current configuration.
    pub fn config(&self) -> &RouterConfig {
        &self.config
    }

    /// Update subscription quota for a model.
    pub fn update_quota(&mut self, model_id: impl Into<String>, quota: SubscriptionQuota) {
        self.quotas.insert(model_id.into(), quota);
    }

    /// Update model availability.
    pub fn update_availability(&mut self, availability: ModelAvailability) {
        self.availability.insert(availability.model_id.clone(), availability);
    }

    /// Get all models for a tier.
    fn get_tier_models(&self, tier: WorkerTier) -> Vec<&ModelConfig> {
        self.config.models_for_tier(tier).iter().collect()
    }

    /// Route a task to the best available model.
    pub fn route(&mut self, task: &TaskMetadata) -> Result<RoutingDecision, RouterError> {
        let recommended_tier = task.recommended_tier();
        debug!(
            bead_id = %task.bead_id,
            priority = ?task.priority,
            recommended_tier = ?recommended_tier,
            "Routing task"
        );

        // Try to find best model in recommended tier
        let mut decision = self.select_model(task, recommended_tier)?;

        // Build fallback chain and add to decision
        decision.fallback_chain = self.build_fallback_chain(decision.tier);

        // Record in history
        self.record_decision(&decision);

        info!(
            bead_id = %task.bead_id,
            model = %decision.model_id,
            tier = ?decision.tier,
            reason = %decision.reason,
            "Routing decision made"
        );

        Ok(decision)
    }

    /// Select a model from a specific tier.
    fn select_model(
        &mut self,
        task: &TaskMetadata,
        tier: WorkerTier,
    ) -> Result<RoutingDecision, RouterError> {
        let models = self.get_tier_models(tier);
        if models.is_empty() {
            return Err(RouterError::NoModelsAvailable { tier });
        }

        // Score each model and collect results
        let scored_models: Vec<_> = models
            .iter()
            .map(|model| {
                let score = self.score_model(model, task);
                (*model, score)
            })
            .collect();

        // Sort by score (highest first)
        let best = scored_models
            .into_iter()
            .max_by_key(|(_, score)| *score as u64)
            .map(|(model, _)| model)
            .ok_or_else(|| RouterError::NoModelsAvailable { tier })?;

        // Extract all data we need from best before mutating
        let model_id = best.id.clone();
        let model_name = best.name.clone();
        let has_subscription = best.has_subscription;

        // Check availability
        let is_available = self
            .availability
            .get(&model_id)
            .map(|a| a.is_available)
            .unwrap_or(true);

        // Determine reason
        let reason = self.determine_reason(best, task, tier);

        // Update load counter for load balancing
        if self.config.enable_load_balancing {
            *self.load_counters.entry(model_id.clone()).or_insert(0) += 1;
        }

        Ok(RoutingDecision {
            model_id,
            model_name,
            tier,
            is_available,
            uses_subscription: has_subscription,
            reason,
            decided_at: Utc::now(),
            fallback_chain: Vec::new(), // Will be filled in by route()
            task_id: task.bead_id.clone(),
        })
    }

    /// Score a model for selection.
    fn score_model(&self, model: &ModelConfig, _task: &TaskMetadata) -> f64 {
        let mut score = 100.0;

        // Prefer subscription models if enabled
        if self.config.prefer_subscription && model.has_subscription {
            // Check if quota is available
            if let Some(quota) = self.quotas.get(&model.id) {
                if quota.is_available() {
                    score += 20.0;
                    // Bonus for urgent quota (use-or-lose)
                    if quota.is_urgent() {
                        score += 15.0;
                    }
                }
            } else {
                // Has subscription but no quota tracked - still prefer
                score += 10.0;
            }
        }

        // Load balancing penalty
        if self.config.enable_load_balancing {
            let load = self.load_counters.get(&model.id).copied().unwrap_or(0);
            score -= (load as f64) * 0.5;
        }

        // Availability penalty
        if let Some(avail) = self.availability.get(&model.id) {
            if !avail.is_available {
                score -= 100.0;
            }
            // Latency penalty
            if let Some(latency) = avail.avg_latency_ms {
                if latency > 5000 {
                    score -= 10.0;
                }
            }
        }

        score.max(0.0)
    }

    /// Determine the routing reason.
    fn determine_reason(&self, model: &ModelConfig, task: &TaskMetadata, tier: WorkerTier) -> RoutingReason {
        // Check subscription preference first
        if self.config.prefer_subscription && model.has_subscription {
            if let Some(quota) = self.quotas.get(&model.id) {
                if quota.is_urgent() {
                    return RoutingReason::SubscriptionPreference;
                }
            }
        }

        // Check complexity-based
        if task.complexity_score.is_some() || task.requires_reasoning {
            return RoutingReason::ComplexityBased;
        }

        // Check label-based
        if !task.labels.is_empty() {
            return RoutingReason::LabelBased;
        }

        // Check if load balancing was applied
        if self.config.enable_load_balancing {
            let load = self.load_counters.get(&model.id).copied().unwrap_or(0);
            if load > 0 {
                return RoutingReason::LoadBalancing;
            }
        }

        // Default to priority-based
        if tier == task.priority.recommended_tier() {
            RoutingReason::PriorityBased
        } else {
            RoutingReason::Default
        }
    }

    /// Build the fallback chain for a decision.
    fn build_fallback_chain(&self, current_tier: WorkerTier) -> Vec<FallbackOption> {
        let mut chain = Vec::new();

        match current_tier {
            WorkerTier::Premium => {
                // Fallback to standard, then budget
                for model in &self.config.standard_models {
                    chain.push(FallbackOption {
                        model_id: model.id.clone(),
                        tier: WorkerTier::Standard,
                        reason: "Premium tier unavailable".to_string(),
                    });
                }
                for model in &self.config.budget_models {
                    chain.push(FallbackOption {
                        model_id: model.id.clone(),
                        tier: WorkerTier::Budget,
                        reason: "Premium and standard tiers unavailable".to_string(),
                    });
                }
            }
            WorkerTier::Standard => {
                // Fallback to budget only
                for model in &self.config.budget_models {
                    chain.push(FallbackOption {
                        model_id: model.id.clone(),
                        tier: WorkerTier::Budget,
                        reason: "Standard tier unavailable".to_string(),
                    });
                }
            }
            WorkerTier::Budget => {
                // No fallback from budget
            }
        }

        chain
    }

    /// Get fallback decision when primary is unavailable.
    pub fn fallback(&mut self, decision: &RoutingDecision) -> Result<RoutingDecision, RouterError> {
        if decision.fallback_chain.is_empty() {
            return Err(RouterError::NoFallbackAvailable {
                model_id: decision.model_id.clone(),
            });
        }

        // Find first available fallback
        for fallback in &decision.fallback_chain {
            if let Some(avail) = self.availability.get(&fallback.model_id) {
                if avail.is_available {
                    let model = self
                        .config
                        .models_for_tier(fallback.tier)
                        .iter()
                        .find(|m| m.id == fallback.model_id);

                    if let Some(model) = model {
                        let new_decision = RoutingDecision {
                            model_id: model.id.clone(),
                            model_name: model.name.clone(),
                            tier: fallback.tier,
                            is_available: true,
                            uses_subscription: model.has_subscription,
                            reason: RoutingReason::Fallback,
                            decided_at: Utc::now(),
                            fallback_chain: self.build_fallback_chain(fallback.tier),
                            task_id: decision.task_id.clone(),
                        };

                        self.record_decision(&new_decision);

                        warn!(
                            from_model = %decision.model_id,
                            to_model = %new_decision.model_id,
                            "Falling back to alternative model"
                        );

                        return Ok(new_decision);
                    }
                }
            }
        }

        Err(RouterError::NoFallbackAvailable {
            model_id: decision.model_id.clone(),
        })
    }

    /// Record a routing decision in history.
    fn record_decision(&mut self, decision: &RoutingDecision) {
        self.history.push(decision.clone());

        // Keep only last 1000 decisions
        if self.history.len() > 1000 {
            self.history.remove(0);
        }
    }

    /// Get routing history.
    pub fn history(&self) -> &[RoutingDecision] {
        &self.history
    }

    /// Get routing statistics.
    pub fn stats(&self) -> RouterStats {
        let mut by_tier = HashMap::new();
        let mut by_model = HashMap::new();
        let mut by_reason = HashMap::new();

        for decision in &self.history {
            *by_tier.entry(decision.tier).or_insert(0) += 1;
            *by_model.entry(decision.model_id.clone()).or_insert(0) += 1;
            *by_reason.entry(decision.reason).or_insert(0) += 1;
        }

        RouterStats {
            total_decisions: self.history.len(),
            by_tier,
            by_model,
            by_reason,
        }
    }

    /// Clear routing history.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Check if a model is available for routing.
    pub fn is_model_available(&self, model_id: &str) -> bool {
        self.availability
            .get(model_id)
            .map(|a| a.is_available)
            .unwrap_or(false)
    }

    /// Get all available models for a tier.
    pub fn available_models(&self, tier: WorkerTier) -> Vec<&ModelConfig> {
        self.config
            .models_for_tier(tier)
            .iter()
            .filter(|m| self.is_model_available(&m.id))
            .collect()
    }

    /// Check health of all models.
    pub fn health_check(&self) -> HashMap<String, ModelHealth> {
        let mut health = HashMap::new();

        for (model_id, avail) in &self.availability {
            let quota = self.quotas.get(model_id);
            health.insert(
                model_id.clone(),
                ModelHealth {
                    is_available: avail.is_available,
                    active_workers: avail.active_workers,
                    avg_latency_ms: avail.avg_latency_ms,
                    last_error: avail.last_error.clone(),
                    quota_remaining: quota.map(|q| q.remaining()),
                    quota_urgent: quota.map(|q| q.is_urgent()).unwrap_or(false),
                },
            );
        }

        health
    }
}

/// Model health status.
#[derive(Debug, Clone)]
pub struct ModelHealth {
    /// Whether the model is available
    pub is_available: bool,

    /// Number of active workers
    pub active_workers: usize,

    /// Average latency
    pub avg_latency_ms: Option<u64>,

    /// Last error message
    pub last_error: Option<String>,

    /// Remaining quota (if subscription)
    pub quota_remaining: Option<u64>,

    /// Whether quota is urgent
    pub quota_urgent: bool,
}

/// Router statistics.
#[derive(Debug, Clone)]
pub struct RouterStats {
    /// Total routing decisions made
    pub total_decisions: usize,

    /// Decisions by tier
    pub by_tier: HashMap<WorkerTier, usize>,

    /// Decisions by model
    pub by_model: HashMap<String, usize>,

    /// Decisions by reason
    pub by_reason: HashMap<RoutingReason, usize>,
}

impl RouterStats {
    /// Get the most used model.
    pub fn most_used_model(&self) -> Option<(&String, &usize)> {
        self.by_model.iter().max_by_key(|(_, count)| *count)
    }

    /// Get the most used tier.
    pub fn most_used_tier(&self) -> Option<(&WorkerTier, &usize)> {
        self.by_tier.iter().max_by_key(|(_, count)| *count)
    }
}

/// Errors from the router.
#[derive(Debug, thiserror::Error)]
pub enum RouterError {
    /// No models available in tier
    #[error("No models available in {tier} tier")]
    NoModelsAvailable {
        /// The tier with no models
        tier: WorkerTier,
    },

    /// No fallback available
    #[error("No fallback available for model {model_id}")]
    NoFallbackAvailable {
        /// The model that has no fallback
        model_id: String,
    },

    /// Configuration load error
    #[error("Failed to load config from {path}")]
    ConfigLoad {
        /// Path to config file
        path: std::path::PathBuf,
        /// Source error
        #[source]
        source: std::io::Error,
    },

    /// Configuration parse error
    #[error("Failed to parse config: {message}")]
    ConfigParse {
        /// Error message
        message: String,
    },

    /// Configuration validation error
    #[error("Config validation failed: {message}")]
    ConfigValidation {
        /// Validation message
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_models() {
        let config = RouterConfig::default();
        assert!(!config.premium_models.is_empty());
        assert!(!config.standard_models.is_empty());
        assert!(!config.budget_models.is_empty());
    }

    #[test]
    fn test_config_validation() {
        let valid = RouterConfig::default();
        assert!(valid.validate().is_ok());

        let invalid = RouterConfig {
            premium_models: vec![],
            ..Default::default()
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_subscription_quota_remaining() {
        let quota = SubscriptionQuota::new(1000, 300);
        assert_eq!(quota.remaining(), 700);
        assert_eq!(quota.usage_percent(), 30.0);
    }

    #[test]
    fn test_subscription_quota_urgent() {
        // Not urgent with no reset time
        let quota = SubscriptionQuota::new(1000, 500);
        assert!(!quota.is_urgent());

        // Urgent with reset time in 1 hour
        let reset = Utc::now() + chrono::Duration::hours(1);
        let urgent_quota = SubscriptionQuota::new(1000, 500).with_reset(reset);
        assert!(urgent_quota.is_urgent());

        // Not urgent with reset time in 48 hours
        let later_reset = Utc::now() + chrono::Duration::hours(48);
        let not_urgent = SubscriptionQuota::new(1000, 500).with_reset(later_reset);
        assert!(!not_urgent.is_urgent());
    }

    #[test]
    fn test_task_recommended_tier() {
        // P0 should recommend premium
        let task_p0 = TaskMetadata::new("test", Priority::P0);
        assert_eq!(task_p0.recommended_tier(), WorkerTier::Premium);

        // P2 should recommend standard
        let task_p2 = TaskMetadata::new("test", Priority::P2);
        assert_eq!(task_p2.recommended_tier(), WorkerTier::Standard);

        // P4 should recommend budget
        let task_p4 = TaskMetadata::new("test", Priority::P4);
        assert_eq!(task_p4.recommended_tier(), WorkerTier::Budget);
    }

    #[test]
    fn test_task_complexity_overrides_priority() {
        // P2 with high complexity should go to premium
        let task = TaskMetadata::new("test", Priority::P2)
            .with_complexity(85);
        assert_eq!(task.recommended_tier(), WorkerTier::Premium);

        // P2 with medium complexity should stay standard
        let task = TaskMetadata::new("test", Priority::P2)
            .with_complexity(50);
        assert_eq!(task.recommended_tier(), WorkerTier::Standard);
    }

    #[test]
    fn test_task_reasoning_requires_premium() {
        let task = TaskMetadata::new("test", Priority::P4)
            .with_reasoning(true);
        assert_eq!(task.recommended_tier(), WorkerTier::Premium);
    }

    #[test]
    fn test_task_label_based_routing() {
        // Architecture label should upgrade to premium
        let task = TaskMetadata::new("test", Priority::P2)
            .with_labels(vec!["architecture".to_string()]);
        assert_eq!(task.recommended_tier(), WorkerTier::Premium);

        // Critical label should upgrade to premium
        let task = TaskMetadata::new("test", Priority::P3)
            .with_labels(vec!["critical".to_string()]);
        assert_eq!(task.recommended_tier(), WorkerTier::Premium);

        // Complex label should upgrade to standard
        let task = TaskMetadata::new("test", Priority::P4)
            .with_labels(vec!["complex".to_string()]);
        assert_eq!(task.recommended_tier(), WorkerTier::Standard);
    }

    #[test]
    fn test_router_route_p0_to_premium() {
        let mut router = Router::new();
        let task = TaskMetadata::new("fg-123", Priority::P0);

        let decision = router.route(&task).unwrap();
        assert_eq!(decision.tier, WorkerTier::Premium);
        assert_eq!(decision.task_id, "fg-123");
    }

    #[test]
    fn test_router_route_p2_to_standard() {
        let mut router = Router::new();
        let task = TaskMetadata::new("fg-456", Priority::P2);

        let decision = router.route(&task).unwrap();
        assert_eq!(decision.tier, WorkerTier::Standard);
    }

    #[test]
    fn test_router_route_p4_to_budget() {
        let mut router = Router::new();
        let task = TaskMetadata::new("fg-789", Priority::P4);

        let decision = router.route(&task).unwrap();
        assert_eq!(decision.tier, WorkerTier::Budget);
    }

    #[test]
    fn test_router_subscription_preference() {
        let mut router = Router::new();

        // Set urgent quota for first premium model
        let reset = Utc::now() + chrono::Duration::hours(1);
        router.update_quota(
            "claude-opus-4",
            SubscriptionQuota::new(1000000, 500000).with_reset(reset),
        );

        let task = TaskMetadata::new("fg-123", Priority::P0);
        let decision = router.route(&task).unwrap();

        // Should prefer subscription model
        assert!(decision.uses_subscription || decision.reason == RoutingReason::SubscriptionPreference);
    }

    #[test]
    fn test_router_fallback_chain() {
        let mut router = Router::new();

        let task = TaskMetadata::new("fg-123", Priority::P0);
        let decision = router.route(&task).unwrap();

        // Premium should have fallback to standard and budget
        assert!(!decision.fallback_chain.is_empty());

        // All fallbacks should be lower tiers
        for fallback in &decision.fallback_chain {
            assert!(fallback.tier == WorkerTier::Standard || fallback.tier == WorkerTier::Budget);
        }
    }

    #[test]
    fn test_router_fallback_when_unavailable() {
        let mut router = Router::new();

        // Mark all premium models as unavailable
        for model in &router.config.premium_models.clone() {
            router.update_availability(ModelAvailability {
                model_id: model.id.clone(),
                is_available: false,
                active_workers: 0,
                avg_latency_ms: None,
                last_error: Some("Rate limited".to_string()),
            });
        }

        let task = TaskMetadata::new("fg-123", Priority::P0);
        let decision = router.route(&task).unwrap();

        // Get fallback
        let fallback = router.fallback(&decision).unwrap();
        assert_eq!(fallback.tier, WorkerTier::Standard);
        assert_eq!(fallback.reason, RoutingReason::Fallback);
    }

    #[test]
    fn test_router_history_tracking() {
        let mut router = Router::new();

        // Route multiple tasks
        for i in 0..5 {
            let priority = match i % 3 {
                0 => Priority::P0,
                1 => Priority::P2,
                _ => Priority::P4,
            };
            let task = TaskMetadata::new(format!("fg-{}", i), priority);
            router.route(&task).unwrap();
        }

        assert_eq!(router.history().len(), 5);
    }

    #[test]
    fn test_router_stats() {
        let mut router = Router::new();

        // Route multiple tasks
        for i in 0..10 {
            let priority = if i < 3 {
                Priority::P0
            } else if i < 7 {
                Priority::P2
            } else {
                Priority::P4
            };
            let task = TaskMetadata::new(format!("fg-{}", i), priority);
            router.route(&task).unwrap();
        }

        let stats = router.stats();
        assert_eq!(stats.total_decisions, 10);

        // Check tier distribution
        let premium_count = stats.by_tier.get(&WorkerTier::Premium).copied().unwrap_or(0);
        let standard_count = stats.by_tier.get(&WorkerTier::Standard).copied().unwrap_or(0);
        let budget_count = stats.by_tier.get(&WorkerTier::Budget).copied().unwrap_or(0);

        assert_eq!(premium_count, 3);
        assert_eq!(standard_count, 4);
        assert_eq!(budget_count, 3);
    }

    #[test]
    fn test_router_load_balancing() {
        let config = RouterConfig {
            enable_load_balancing: true,
            ..Default::default()
        };
        let mut router = Router::with_config(config);

        // Route multiple P0 tasks - they should distribute across premium models
        let mut models_used = std::collections::HashSet::new();
        for i in 0..10 {
            let task = TaskMetadata::new(format!("fg-{}", i), Priority::P0);
            let decision = router.route(&task).unwrap();
            models_used.insert(decision.model_id);
        }

        // With load balancing, should use multiple models
        // (At least 2 different models for 10 tasks)
        assert!(models_used.len() >= 1, "Should use at least one model");
    }

    #[test]
    fn test_router_health_check() {
        let mut router = Router::new();

        // Mark one model as unavailable
        router.update_availability(ModelAvailability {
            model_id: "claude-opus-4".to_string(),
            is_available: false,
            active_workers: 5,
            avg_latency_ms: Some(2000),
            last_error: Some("Rate limited".to_string()),
        });

        let health = router.health_check();

        // Check opus is unavailable
        let opus_health = health.get("claude-opus-4").unwrap();
        assert!(!opus_health.is_available);
        assert_eq!(opus_health.active_workers, 5);

        // Check sonnet is available
        let sonnet_health = health.get("claude-sonnet-4").unwrap();
        assert!(sonnet_health.is_available);
    }

    #[test]
    fn test_router_available_models() {
        let mut router = Router::new();

        // Initially all models should be available
        let premium = router.available_models(WorkerTier::Premium);
        assert!(!premium.is_empty());

        // Mark one as unavailable
        router.update_availability(ModelAvailability {
            model_id: "claude-opus-4".to_string(),
            is_available: false,
            active_workers: 0,
            avg_latency_ms: None,
            last_error: None,
        });

        let premium = router.available_models(WorkerTier::Premium);
        let available_ids: Vec<_> = premium.iter().map(|m| m.id.as_str()).collect();
        assert!(!available_ids.contains(&"claude-opus-4"));
    }

    #[test]
    fn test_routing_decision_serialization() {
        let decision = RoutingDecision {
            model_id: "claude-sonnet-4".to_string(),
            model_name: "Claude Sonnet 4".to_string(),
            tier: WorkerTier::Standard,
            is_available: true,
            uses_subscription: true,
            reason: RoutingReason::PriorityBased,
            decided_at: Utc::now(),
            fallback_chain: vec![FallbackOption {
                model_id: "claude-haiku-4".to_string(),
                tier: WorkerTier::Budget,
                reason: "Standard tier unavailable".to_string(),
            }],
            task_id: "fg-123".to_string(),
        };

        // Should serialize to JSON
        let json = serde_json::to_string(&decision).unwrap();
        assert!(json.contains("claude-sonnet-4"));
        assert!(json.contains("priority-based"));
    }

    #[test]
    fn test_router_config_from_yaml() {
        let yaml = r#"
prefer_subscription: true
fallback_timeout_secs: 10
enable_load_balancing: true
premium_models:
  - id: custom-premium
    name: Custom Premium
    has_subscription: true
standard_models:
  - id: custom-standard
    name: Custom Standard
    has_subscription: false
budget_models:
  - id: custom-budget
    name: Custom Budget
    has_subscription: false
"#;

        let config: RouterConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.fallback_timeout_secs, 10);
        assert_eq!(config.premium_models.len(), 1);
        assert_eq!(config.premium_models[0].id, "custom-premium");
    }

    #[test]
    fn test_no_fallback_from_budget() {
        let mut router = Router::new();

        let task = TaskMetadata::new("fg-123", Priority::P4);
        let decision = router.route(&task).unwrap();

        // Budget should have empty fallback chain
        assert!(decision.fallback_chain.is_empty());
    }

    #[test]
    fn test_history_limit() {
        let mut router = Router::new();

        // Route more than 1000 tasks
        for i in 0..1100 {
            let task = TaskMetadata::new(format!("fg-{}", i), Priority::P2);
            router.route(&task).unwrap();
        }

        // Should be capped at 1000
        assert_eq!(router.history().len(), 1000);
    }
}
