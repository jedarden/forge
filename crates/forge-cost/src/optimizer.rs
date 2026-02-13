//! Cost optimization logic for maximizing subscription value and minimizing API costs.
//!
//! This module provides intelligent cost optimization strategies:
//! - **Subscription Exhaustion**: Use subscription quota before billing period ends
//! - **Model Downgrade**: Route simple tasks to cheaper models
//! - **Batch Processing**: Combine related tasks to reduce API overhead
//! - **Cache Utilization**: Leverage prompt caching when available
//! - **Off-Peak Routing**: Queue non-urgent tasks for cheaper time windows
//!
//! ## Usage
//!
//! ```no_run
//! use forge_cost::{CostDatabase, CostQuery, CostOptimizer, OptimizerConfig, TaskPriority};
//!
//! fn main() -> anyhow::Result<()> {
//!     let db = CostDatabase::open("~/.forge/costs.db")?;
//!     let query = CostQuery::new(&db);
//!     let mut optimizer = CostOptimizer::new(&db, OptimizerConfig::default());
//!
//!     // Get optimization recommendations
//!     let report = optimizer.generate_report()?;
//!     println!("Potential savings: ${:.2}", report.total_potential_savings);
//!
//!     // Get best model for a task
//!     let recommendation = optimizer.recommend_model(1000, TaskPriority::Low)?;
//!     println!("Use {} to save ${:.4}", recommendation.model_id, recommendation.savings_vs_naive);
//!
//!     Ok(())
//! }
//! ```

use crate::db::CostDatabase;
use crate::error::Result;
use crate::models::{QuotaStatus, Subscription};
use crate::query::{CostQuery, SubscriptionOptimizationReport};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Task priority levels for cost-aware routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    /// Critical tasks requiring best models
    Critical,
    /// High priority but can use standard models
    High,
    /// Normal priority, flexible routing
    Normal,
    /// Low priority, maximize cost savings
    Low,
    /// Background tasks, use cheapest available
    Background,
}

impl Default for TaskPriority {
    fn default() -> Self {
        Self::Normal
    }
}

impl TaskPriority {
    /// Get the cost sensitivity for this priority level.
    /// Higher values mean more sensitive to cost.
    pub fn cost_sensitivity(&self) -> f64 {
        match self {
            TaskPriority::Critical => 0.1, // Cost doesn't matter much
            TaskPriority::High => 0.3,
            TaskPriority::Normal => 0.5,
            TaskPriority::Low => 0.8, // Cost matters a lot
            TaskPriority::Background => 1.0, // Only use cheapest
        }
    }

    /// Check if this priority allows subscription-only models.
    pub fn allows_subscription_only(&self) -> bool {
        matches!(self, TaskPriority::Low | TaskPriority::Background)
    }
}

/// Configuration for the cost optimizer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerConfig {
    /// Target subscription utilization percentage before renewal (default: 90%)
    pub target_subscription_utilization: f64,

    /// Minimum savings threshold to recommend optimization (default: $0.01)
    pub min_savings_threshold: f64,

    /// Days before renewal to start accelerating subscription usage (default: 7)
    pub acceleration_days: i64,

    /// Enable aggressive subscription exhaustion near renewal
    pub enable_aggressive_exhaustion: bool,

    /// Prefer models with cache support
    pub prefer_cache_enabled: bool,

    /// Historical lookback days for cost analysis (default: 30)
    pub lookback_days: i64,

    /// Cost weight in routing decisions (0.0-1.0, default: 0.5)
    pub cost_weight: f64,

    /// Quality weight in routing decisions (0.0-1.0, default: 0.5)
    pub quality_weight: f64,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            target_subscription_utilization: 90.0,
            min_savings_threshold: 0.01,
            acceleration_days: 7,
            enable_aggressive_exhaustion: true,
            prefer_cache_enabled: true,
            lookback_days: 30,
            cost_weight: 0.5,
            quality_weight: 0.5,
        }
    }
}

impl OptimizerConfig {
    /// Create a new optimizer configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set target subscription utilization.
    pub fn with_target_utilization(mut self, target: f64) -> Self {
        self.target_subscription_utilization = target.clamp(50.0, 100.0);
        self
    }

    /// Set minimum savings threshold.
    pub fn with_min_savings(mut self, threshold: f64) -> Self {
        self.min_savings_threshold = threshold.max(0.0);
        self
    }

    /// Set the cost-quality balance weights.
    pub fn with_weights(mut self, cost: f64, quality: f64) -> Self {
        let total = cost + quality;
        if total > 0.0 {
            self.cost_weight = cost / total;
            self.quality_weight = quality / total;
        }
        self
    }
}

/// Model recommendation from the optimizer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRecommendation {
    /// Recommended model ID
    pub model_id: String,

    /// Estimated cost for the task
    pub estimated_cost: f64,

    /// Cost if using the naive approach (highest quality model)
    pub naive_cost: f64,

    /// Savings vs naive approach
    pub savings_vs_naive: f64,

    /// Quality score (0-100)
    pub quality_score: f64,

    /// Whether this uses subscription quota
    pub uses_subscription: bool,

    /// Reason for this recommendation
    pub reason: RecommendationReason,

    /// Estimated tokens for calculation
    pub estimated_tokens: u64,
}

/// Reason for a model recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationReason {
    /// Best balance of cost and quality
    OptimalBalance,
    /// Uses urgent subscription quota (use-or-lose)
    UrgentSubscription,
    /// Subscription quota available
    SubscriptionAvailable,
    /// Lowest cost for task
    LowestCost,
    /// Highest quality for task
    HighestQuality,
    /// Model performance based
    PerformanceBased,
    /// Only available option
    OnlyAvailable,
    /// Fallback from preferred option
    Fallback,
}

impl std::fmt::Display for RecommendationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OptimalBalance => write!(f, "optimal balance"),
            Self::UrgentSubscription => write!(f, "urgent subscription"),
            Self::SubscriptionAvailable => write!(f, "subscription available"),
            Self::LowestCost => write!(f, "lowest cost"),
            Self::HighestQuality => write!(f, "highest quality"),
            Self::PerformanceBased => write!(f, "performance based"),
            Self::OnlyAvailable => write!(f, "only available"),
            Self::Fallback => write!(f, "fallback"),
        }
    }
}

/// Optimization recommendation for the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationRecommendation {
    /// Recommendation type
    pub recommendation_type: RecommendationType,

    /// Priority (higher = more important)
    pub priority: u32,

    /// Title for display
    pub title: String,

    /// Detailed description
    pub description: String,

    /// Estimated savings if followed
    pub estimated_savings: f64,

    /// Action to take
    pub action: String,
}

/// Type of optimization recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationType {
    /// Subscription quota needs acceleration
    AccelerateSubscription,
    /// Subscription quota should be maximized before reset
    MaxOutSubscription,
    /// Switch to cheaper model for task type
    ModelDowngrade,
    /// Enable caching for repeated prompts
    EnableCaching,
    /// Batch similar tasks
    BatchTasks,
    /// Schedule tasks for off-peak
    OffPeakScheduling,
    /// Subscription depleted, consider upgrade
    SubscriptionDepleted,
}

/// Comprehensive optimization report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationReport {
    /// When this report was generated
    pub generated_at: DateTime<Utc>,

    /// Total potential monthly savings
    pub total_potential_savings: f64,

    /// Current monthly spend
    pub current_monthly_spend: f64,

    /// Projected monthly spend with optimizations
    pub optimized_monthly_spend: f64,

    /// Subscription utilization percentage
    pub subscription_utilization: f64,

    /// Recommendations
    pub recommendations: Vec<OptimizationRecommendation>,

    /// Per-model cost efficiency
    pub model_efficiency: Vec<ModelEfficiency>,

    /// Subscription optimization details
    pub subscription_details: SubscriptionOptimizationReport,

    /// Savings achieved from previous optimizations
    pub savings_achieved: f64,
}

/// Model cost efficiency metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEfficiency {
    /// Model ID
    pub model_id: String,

    /// Total cost with this model
    pub total_cost: f64,

    /// Cost per successful task
    pub cost_per_success: f64,

    /// Success rate (0-1)
    pub success_rate: f64,

    /// Efficiency score (lower is better, cost per success adjusted for success rate)
    pub efficiency_score: f64,

    /// Number of tasks
    pub task_count: u64,

    /// Recommendation for this model
    pub recommendation: String,
}

/// The cost optimizer engine.
pub struct CostOptimizer<'a> {
    /// Database reference
    db: &'a CostDatabase,

    /// Query interface
    query: CostQuery<'a>,

    /// Configuration
    config: OptimizerConfig,

    /// Cached model pricing (per million tokens)
    pricing: HashMap<String, ModelPricing>,

    /// Cached subscription data
    subscriptions: Vec<Subscription>,
}

/// Model pricing information.
#[derive(Debug, Clone)]
struct ModelPricing {
    /// Cost per million input tokens
    input_per_million: f64,
    /// Cost per million output tokens
    output_per_million: f64,
    /// Has subscription option
    has_subscription: bool,
    /// Quality tier (1-3, 3 = premium)
    quality_tier: u8,
}

impl ModelPricing {
    /// Estimate cost for given tokens.
    fn estimate_cost(&self, input_tokens: u64, output_tokens: u64) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input_per_million;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output_per_million;
        input_cost + output_cost
    }

    /// Get quality score (0-100).
    fn quality_score(&self) -> f64 {
        match self.quality_tier {
            3 => 95.0, // Premium
            2 => 75.0, // Standard
            1 => 50.0, // Budget
            _ => 50.0,
        }
    }
}

impl<'a> CostOptimizer<'a> {
    /// Create a new cost optimizer.
    pub fn new(db: &'a CostDatabase, config: OptimizerConfig) -> Self {
        let query = CostQuery::new(db);
        let subscriptions = db.get_active_subscriptions().unwrap_or_default();

        let mut optimizer = Self {
            db,
            query,
            config,
            pricing: HashMap::new(),
            subscriptions,
        };

        optimizer.initialize_pricing();
        optimizer
    }

    /// Initialize model pricing data.
    fn initialize_pricing(&mut self) {
        // Premium models
        self.pricing.insert(
            "claude-opus-4".to_string(),
            ModelPricing {
                input_per_million: 15.0,
                output_per_million: 75.0,
                has_subscription: true,
                quality_tier: 3,
            },
        );
        self.pricing.insert(
            "claude-opus-4-5-20251101".to_string(),
            ModelPricing {
                input_per_million: 15.0,
                output_per_million: 75.0,
                has_subscription: true,
                quality_tier: 3,
            },
        );
        self.pricing.insert(
            "o1".to_string(),
            ModelPricing {
                input_per_million: 15.0,
                output_per_million: 60.0,
                has_subscription: false,
                quality_tier: 3,
            },
        );
        self.pricing.insert(
            "glm-5".to_string(),
            ModelPricing {
                input_per_million: 0.0, // Free via z.ai proxy
                output_per_million: 0.0,
                has_subscription: true,
                quality_tier: 3,
            },
        );
        self.pricing.insert(
            "glm-4.7".to_string(),
            ModelPricing {
                input_per_million: 0.0, // Free via z.ai proxy
                output_per_million: 0.0,
                has_subscription: true,
                quality_tier: 3,
            },
        );

        // Standard models
        self.pricing.insert(
            "claude-sonnet-4".to_string(),
            ModelPricing {
                input_per_million: 3.0,
                output_per_million: 15.0,
                has_subscription: true,
                quality_tier: 2,
            },
        );
        self.pricing.insert(
            "claude-sonnet-4-5-20250929".to_string(),
            ModelPricing {
                input_per_million: 3.0,
                output_per_million: 15.0,
                has_subscription: true,
                quality_tier: 2,
            },
        );
        self.pricing.insert(
            "gpt-4".to_string(),
            ModelPricing {
                input_per_million: 30.0,
                output_per_million: 60.0,
                has_subscription: false,
                quality_tier: 2,
            },
        );
        self.pricing.insert(
            "qwen-2.5".to_string(),
            ModelPricing {
                input_per_million: 0.5,
                output_per_million: 2.0,
                has_subscription: false,
                quality_tier: 2,
            },
        );

        // Budget models
        self.pricing.insert(
            "claude-haiku-4".to_string(),
            ModelPricing {
                input_per_million: 0.25,
                output_per_million: 1.25,
                has_subscription: true,
                quality_tier: 1,
            },
        );
        self.pricing.insert(
            "claude-haiku-4-5-20251001".to_string(),
            ModelPricing {
                input_per_million: 0.25,
                output_per_million: 1.25,
                has_subscription: true,
                quality_tier: 1,
            },
        );
        self.pricing.insert(
            "gpt-3.5".to_string(),
            ModelPricing {
                input_per_million: 0.5,
                output_per_million: 1.5,
                has_subscription: false,
                quality_tier: 1,
            },
        );
        self.pricing.insert(
            "deepseek-coder".to_string(),
            ModelPricing {
                input_per_million: 0.14,
                output_per_million: 0.28,
                has_subscription: false,
                quality_tier: 1,
            },
        );
    }

    /// Refresh subscription data from database.
    pub fn refresh_subscriptions(&mut self) -> Result<()> {
        self.subscriptions = self.db.get_active_subscriptions()?;
        Ok(())
    }

    /// Get pricing for a model (internal use).
    fn get_model_pricing(&self, model_id: &str) -> Option<&ModelPricing> {
        // Try exact match first
        if let Some(pricing) = self.pricing.get(model_id) {
            return Some(pricing);
        }

        // Try partial match (e.g., "claude-opus" matches "claude-opus-4")
        let model_lower = model_id.to_lowercase();
        for (key, pricing) in &self.pricing {
            if model_lower.contains(&key.to_lowercase()) || key.to_lowercase().contains(&model_lower)
            {
                return Some(pricing);
            }
        }

        None
    }

    /// Estimate cost for a task on a specific model.
    pub fn estimate_task_cost(
        &self,
        model_id: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> f64 {
        if let Some(pricing) = self.get_model_pricing(model_id) {
            // If model has subscription and quota available, cost is 0
            if pricing.has_subscription && self.has_subscription_quota(model_id) {
                return 0.0;
            }
            pricing.estimate_cost(input_tokens, output_tokens)
        } else {
            // Default estimate: $3/MTok input, $15/MTok output
            let input_cost = (input_tokens as f64 / 1_000_000.0) * 3.0;
            let output_cost = (output_tokens as f64 / 1_000_000.0) * 15.0;
            input_cost + output_cost
        }
    }

    /// Check if a model has available subscription quota.
    pub fn has_subscription_quota(&self, model_id: &str) -> bool {
        for sub in &self.subscriptions {
            if let Some(ref sub_model) = sub.model {
                if sub_model.contains(model_id) || model_id.contains(sub_model) {
                    if let Some(remaining) = sub.remaining_quota() {
                        return remaining > 0;
                    }
                    // Unlimited subscription
                    return true;
                }
            }
        }
        false
    }

    /// Get subscription status for a model.
    pub fn get_subscription_status(&self, model_id: &str) -> Option<QuotaStatus> {
        for sub in &self.subscriptions {
            if let Some(ref sub_model) = sub.model {
                if sub_model.contains(model_id) || model_id.contains(sub_model) {
                    return Some(sub.quota_status());
                }
            }
        }
        None
    }

    /// Get the best model recommendation for a task.
    pub fn recommend_model(
        &self,
        estimated_tokens: u64,
        priority: TaskPriority,
    ) -> Result<ModelRecommendation> {
        // Assume 70% input, 30% output ratio
        let input_tokens = (estimated_tokens as f64 * 0.7) as u64;
        let output_tokens = (estimated_tokens as f64 * 0.3) as u64;

        // Get naive cost (using premium model)
        let naive_cost = self.estimate_task_cost("claude-opus-4", input_tokens, output_tokens);

        // Find best option considering cost and quality
        let mut best: Option<(String, f64, f64, f64, bool, RecommendationReason)> = None;
        let cost_sensitivity = priority.cost_sensitivity();
        let quality_weight = self.config.quality_weight * (1.0 - cost_sensitivity);
        let cost_weight = self.config.cost_weight * cost_sensitivity;

        // First check for urgent subscriptions (use-or-lose)
        for sub in &self.subscriptions {
            if sub.quota_status() == QuotaStatus::MaxOut || sub.quota_status() == QuotaStatus::Accelerate {
                if let Some(ref model_id) = sub.model {
                    if let Some(pricing) = self.get_model_pricing(model_id) {
                        if let Some(remaining) = sub.remaining_quota() {
                            if remaining > 0 {
                                let cost = 0.0; // Free with subscription
                                let quality = pricing.quality_score();
                                let score = quality * quality_weight + (100.0 * cost_weight); // Max cost score

                                let reason = if sub.quota_status() == QuotaStatus::MaxOut {
                                    RecommendationReason::UrgentSubscription
                                } else {
                                    RecommendationReason::SubscriptionAvailable
                                };

                                if best.as_ref().map_or(true, |(_, _, _, s, _, _)| score > *s) {
                                    best = Some((model_id.clone(), cost, quality, score, true, reason));
                                }
                            }
                        }
                    }
                }
            }
        }

        // Then check all models for best balance
        for (model_id, pricing) in &self.pricing {
            // Skip if priority requires subscription and this doesn't have one
            if priority.allows_subscription_only() && !pricing.has_subscription {
                continue;
            }

            let cost = if pricing.has_subscription && self.has_subscription_quota(model_id) {
                0.0
            } else {
                pricing.estimate_cost(input_tokens, output_tokens)
            };

            // Skip if cost is too high for this priority
            let max_cost = naive_cost * (2.0 - cost_sensitivity);
            if cost > max_cost {
                continue;
            }

            let quality = pricing.quality_score();

            // Normalize cost score (0 = expensive, 100 = free)
            let cost_score = if naive_cost > 0.0 {
                ((naive_cost - cost) / naive_cost * 100.0).clamp(0.0, 100.0)
            } else {
                100.0
            };

            let score = quality * quality_weight + cost_score * cost_weight;

            if best.as_ref().map_or(true, |(_, _, _, s, _, _)| score > *s) {
                let reason = if cost == 0.0 && pricing.has_subscription {
                    RecommendationReason::SubscriptionAvailable
                } else if cost_score > 80.0 {
                    RecommendationReason::LowestCost
                } else if quality > 90.0 {
                    RecommendationReason::HighestQuality
                } else {
                    RecommendationReason::OptimalBalance
                };

                best = Some((
                    model_id.clone(),
                    cost,
                    quality,
                    score,
                    pricing.has_subscription && self.has_subscription_quota(model_id),
                    reason,
                ));
            }
        }

        // Return best option or fallback
        if let Some((model_id, cost, quality, _, uses_subscription, reason)) = best {
            Ok(ModelRecommendation {
                model_id,
                estimated_cost: cost,
                naive_cost,
                savings_vs_naive: naive_cost - cost,
                quality_score: quality,
                uses_subscription,
                reason,
                estimated_tokens,
            })
        } else {
            // Fallback to standard model
            Ok(ModelRecommendation {
                model_id: "claude-sonnet-4".to_string(),
                estimated_cost: naive_cost * 0.2,
                naive_cost,
                savings_vs_naive: naive_cost * 0.8,
                quality_score: 75.0,
                uses_subscription: false,
                reason: RecommendationReason::Fallback,
                estimated_tokens,
            })
        }
    }

    /// Generate a comprehensive optimization report.
    pub fn generate_report(&self) -> Result<OptimizationReport> {
        let generated_at = Utc::now();

        // Get subscription optimization report
        let subscription_details = self.query.get_subscription_optimization_report()?;

        // Calculate subscription utilization
        let subscription_utilization = subscription_details.overall_utilization;

        // Generate recommendations
        let mut recommendations = Vec::new();

        // Check subscriptions needing attention
        for summary in &subscription_details.subscriptions {
            match summary.status {
                QuotaStatus::Accelerate => {
                    recommendations.push(OptimizationRecommendation {
                        recommendation_type: RecommendationType::AccelerateSubscription,
                        priority: 80,
                        title: format!("Accelerate {} usage", summary.name),
                        description: format!(
                            "{} is under-utilized at {:.1}%. Consider routing more tasks to this subscription.",
                            summary.name, summary.usage_percentage
                        ),
                        estimated_savings: summary.monthly_cost * (1.0 - summary.usage_percentage / 100.0) * 0.5,
                        action: format!("Route low-priority tasks to {}", summary.model.as_deref().unwrap_or("this model")),
                    });
                }
                QuotaStatus::MaxOut => {
                    recommendations.push(OptimizationRecommendation {
                        recommendation_type: RecommendationType::MaxOutSubscription,
                        priority: 90,
                        title: format!("Max out {} before reset", summary.name),
                        description: format!(
                            "{} has {:.0}/{} quota remaining. Reset in {}.",
                            summary.name,
                            summary.quota_limit.unwrap_or(0) as f64 - summary.quota_used as f64,
                            summary.quota_limit.unwrap_or(0),
                            summary.reset_time
                        ),
                        estimated_savings: summary.monthly_cost,
                        action: "Route all non-critical tasks to this model immediately".to_string(),
                    });
                }
                QuotaStatus::Depleted => {
                    recommendations.push(OptimizationRecommendation {
                        recommendation_type: RecommendationType::SubscriptionDepleted,
                        priority: 70,
                        title: format!("{} quota exhausted", summary.name),
                        description: format!(
                            "{} quota is depleted. Consider upgrading or using API fallback.",
                            summary.name
                        ),
                        estimated_savings: 0.0,
                        action: "Review subscription tier or use pay-per-use fallback".to_string(),
                    });
                }
                QuotaStatus::OnPace => {}
            }
        }

        // Get model efficiency metrics
        let model_efficiency = self.calculate_model_efficiency()?;

        // Add model downgrade recommendations
        for eff in &model_efficiency {
            if eff.efficiency_score > 0.05 && eff.task_count > 10 {
                recommendations.push(OptimizationRecommendation {
                    recommendation_type: RecommendationType::ModelDowngrade,
                    priority: 50,
                    title: format!("Consider cheaper alternative for {}", eff.model_id),
                    description: format!(
                        "{} has an efficiency score of {:.4} (cost per success: ${:.4}). Consider routing simple tasks to budget models.",
                        eff.model_id, eff.efficiency_score, eff.cost_per_success
                    ),
                    estimated_savings: eff.total_cost * 0.3,
                    action: "Route simple tasks to claude-haiku-4 or deepseek-coder".to_string(),
                });
            }
        }

        // Add caching recommendation if not using cache heavily
        let today = Utc::now().date_naive();
        if let Ok(models) = self.db.get_model_performance(today) {
            for perf in models {
                if perf.cache_hit_rate < 0.1 && perf.total_calls > 10 {
                    recommendations.push(OptimizationRecommendation {
                        recommendation_type: RecommendationType::EnableCaching,
                        priority: 40,
                        title: format!("Enable caching for {}", perf.model),
                        description: format!(
                            "{} has only {:.1}% cache hit rate. Enable prompt caching for repeated patterns.",
                            perf.model, perf.cache_hit_rate * 100.0
                        ),
                        estimated_savings: perf.total_cost_usd * 0.2, // ~20% savings from caching
                        action: "Review prompts for reusable context and enable caching".to_string(),
                    });
                }
            }
        }

        // Sort recommendations by priority
        recommendations.sort_by(|a, b| b.priority.cmp(&a.priority));

        // Calculate totals
        let current_monthly_spend = self.query.get_current_month_costs()?.total_cost_usd;
        let total_potential_savings: f64 = recommendations.iter().map(|r| r.estimated_savings).sum();
        let optimized_monthly_spend = (current_monthly_spend - total_potential_savings).max(0.0);

        // Calculate savings achieved from previous optimizations
        let savings_achieved = self.calculate_savings_achieved()?;

        Ok(OptimizationReport {
            generated_at,
            total_potential_savings,
            current_monthly_spend,
            optimized_monthly_spend,
            subscription_utilization,
            recommendations,
            model_efficiency,
            subscription_details,
            savings_achieved,
        })
    }

    /// Calculate model efficiency metrics.
    fn calculate_model_efficiency(&self) -> Result<Vec<ModelEfficiency>> {
        let today = Utc::now().date_naive();
        let models = self.db.get_model_performance(today)?;

        let efficiency: Vec<ModelEfficiency> = models
            .into_iter()
            .map(|perf| {
                let efficiency_score = if perf.success_rate > 0.0 {
                    perf.avg_cost_per_task / perf.success_rate
                } else {
                    f64::MAX
                };

                let recommendation = if efficiency_score < 0.01 {
                    "Excellent efficiency".to_string()
                } else if efficiency_score < 0.05 {
                    "Good efficiency".to_string()
                } else if efficiency_score < 0.10 {
                    "Consider for complex tasks only".to_string()
                } else {
                    "Consider cheaper alternatives".to_string()
                };

                ModelEfficiency {
                    model_id: perf.model.clone(),
                    total_cost: perf.total_cost_usd,
                    cost_per_success: if perf.success_rate > 0.0 {
                        perf.avg_cost_per_task / perf.success_rate
                    } else {
                        perf.avg_cost_per_task
                    },
                    success_rate: perf.success_rate,
                    efficiency_score,
                    task_count: perf.tasks_completed as u64 + perf.tasks_failed as u64,
                    recommendation,
                }
            })
            .collect();

        Ok(efficiency)
    }

    /// Calculate savings achieved from optimization.
    fn calculate_savings_achieved(&self) -> Result<f64> {
        // Calculate savings from subscription usage vs API pricing
        let subscription_savings = self.query.get_total_subscription_savings()?;

        // Add estimated savings from using budget models instead of premium
        // This is a rough estimate based on typical cost differences
        let today = Utc::now().date_naive();
        let budget_savings = if let Ok(models) = self.db.get_model_performance(today) {
            models
                .iter()
                .filter(|m| {
                    m.model.contains("haiku")
                        || m.model.contains("deepseek")
                        || m.model.contains("gpt-3.5")
                })
                .map(|m| {
                    // Estimate savings vs using premium model
                    let premium_cost_per_task = 0.15; // ~$0.15/task for premium
                    let tasks = m.tasks_completed.max(1);
                    (premium_cost_per_task - m.avg_cost_per_task).max(0.0) * tasks as f64
                })
                .sum()
        } else {
            0.0
        };

        Ok(subscription_savings + budget_savings)
    }

    /// Get optimization recommendations for the dashboard.
    pub fn get_recommendations(&self) -> Result<Vec<OptimizationRecommendation>> {
        let report = self.generate_report()?;
        Ok(report.recommendations)
    }

    /// Calculate the cost reduction percentage vs naive routing.
    pub fn calculate_cost_reduction(&self) -> Result<f64> {
        let report = self.generate_report()?;

        if report.current_monthly_spend > 0.0 {
            let reduction =
                report.total_potential_savings / report.current_monthly_spend * 100.0;
            Ok(reduction.min(100.0))
        } else {
            Ok(0.0)
        }
    }

    /// Check if subscription utilization meets target.
    pub fn is_subscription_utilization_on_target(&self) -> bool {
        if let Ok(report) = self.query.get_subscription_optimization_report() {
            report.overall_utilization >= self.config.target_subscription_utilization
        } else {
            false
        }
    }

    /// Get days until nearest subscription renewal.
    pub fn days_until_renewal(&self) -> Option<i64> {
        self.subscriptions
            .iter()
            .filter_map(|s| {
                let duration = s.time_until_reset();
                if duration.num_seconds() > 0 {
                    Some(duration.num_days())
                } else {
                    None
                }
            })
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Subscription, SubscriptionType};
    use chrono::Duration as ChronoDuration;

    fn create_test_db() -> CostDatabase {
        CostDatabase::open_in_memory().unwrap()
    }

    #[test]
    fn test_optimizer_config_default() {
        let config = OptimizerConfig::default();
        assert_eq!(config.target_subscription_utilization, 90.0);
        assert_eq!(config.min_savings_threshold, 0.01);
        assert!(config.enable_aggressive_exhaustion);
    }

    #[test]
    fn test_optimizer_config_weights() {
        let config = OptimizerConfig::new().with_weights(0.8, 0.2);
        assert!((config.cost_weight - 0.8).abs() < 0.001);
        assert!((config.quality_weight - 0.2).abs() < 0.001);
    }

    #[test]
    fn test_task_priority_cost_sensitivity() {
        assert!(TaskPriority::Critical.cost_sensitivity() < TaskPriority::Normal.cost_sensitivity());
        assert!(TaskPriority::Normal.cost_sensitivity() < TaskPriority::Low.cost_sensitivity());
        assert!(TaskPriority::Low.cost_sensitivity() < TaskPriority::Background.cost_sensitivity());
    }

    #[test]
    fn test_estimate_task_cost() {
        let db = create_test_db();
        let optimizer = CostOptimizer::new(&db, OptimizerConfig::default());

        // Premium model without subscription
        let cost = optimizer.estimate_task_cost("claude-opus-4", 10000, 5000);
        assert!(cost > 0.0);

        // Free model (GLM)
        let cost = optimizer.estimate_task_cost("glm-5", 10000, 5000);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_recommend_model_low_priority() {
        let db = create_test_db();
        let optimizer = CostOptimizer::new(&db, OptimizerConfig::default());

        let rec = optimizer.recommend_model(10000, TaskPriority::Low).unwrap();

        // Should recommend a cost-effective option
        assert!(rec.savings_vs_naive >= 0.0);
        assert!(!rec.model_id.is_empty());
    }

    #[test]
    fn test_recommend_model_critical_priority() {
        let db = create_test_db();
        let optimizer = CostOptimizer::new(&db, OptimizerConfig::default());

        let rec = optimizer.recommend_model(10000, TaskPriority::Critical).unwrap();

        // Critical tasks should still optimize but prefer quality
        assert!(rec.quality_score >= 50.0);
    }

    #[test]
    fn test_has_subscription_quota() {
        let db = create_test_db();

        // Add a subscription
        let start = Utc::now() - ChronoDuration::days(15);
        let end = Utc::now() + ChronoDuration::days(15);
        let mut sub = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
            .with_quota(500)
            .with_model("claude-sonnet-4");
        sub.quota_used = 250;

        db.upsert_subscription(&sub).unwrap();

        let optimizer = CostOptimizer::new(&db, OptimizerConfig::default());

        // Should have quota for sonnet
        assert!(optimizer.has_subscription_quota("claude-sonnet-4"));
    }

    #[test]
    fn test_get_subscription_status() {
        let db = create_test_db();

        // Add a subscription that needs acceleration
        let start = Utc::now() - ChronoDuration::days(24);
        let end = Utc::now() + ChronoDuration::days(6);
        let mut sub = Subscription::new("Claude Pro", SubscriptionType::FixedQuota, 20.0, start, end)
            .with_quota(500)
            .with_model("claude-sonnet-4");
        sub.quota_used = 150; // 30% used, 80% of time elapsed

        db.upsert_subscription(&sub).unwrap();

        let optimizer = CostOptimizer::new(&db, OptimizerConfig::default());

        // Should be in accelerate status
        let status = optimizer.get_subscription_status("claude-sonnet-4");
        assert!(matches!(status, Some(QuotaStatus::Accelerate)));
    }

    #[test]
    fn test_generate_report() {
        let db = create_test_db();
        let optimizer = CostOptimizer::new(&db, OptimizerConfig::default());

        let report = optimizer.generate_report().unwrap();

        assert!(report.generated_at.timestamp() != 0);
        // With no subscriptions, utilization should be 0
        assert_eq!(report.subscription_utilization, 0.0);
    }

    #[test]
    fn test_model_pricing_estimate() {
        let pricing = ModelPricing {
            input_per_million: 3.0,
            output_per_million: 15.0,
            has_subscription: true,
            quality_tier: 2,
        };

        // 1M input, 500K output = $3 + $7.5 = $10.5
        let cost = pricing.estimate_cost(1_000_000, 500_000);
        assert!((cost - 10.5).abs() < 0.001);
    }

    #[test]
    fn test_model_pricing_quality_score() {
        let premium = ModelPricing {
            input_per_million: 15.0,
            output_per_million: 75.0,
            has_subscription: true,
            quality_tier: 3,
        };
        assert_eq!(premium.quality_score(), 95.0);

        let budget = ModelPricing {
            input_per_million: 0.25,
            output_per_million: 1.25,
            has_subscription: true,
            quality_tier: 1,
        };
        assert_eq!(budget.quality_score(), 50.0);
    }

    #[test]
    fn test_recommendation_reason_display() {
        assert_eq!(
            RecommendationReason::UrgentSubscription.to_string(),
            "urgent subscription"
        );
        assert_eq!(
            RecommendationReason::OptimalBalance.to_string(),
            "optimal balance"
        );
    }
}
