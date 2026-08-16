//! Task complexity scoring for intelligent model routing.
//!
//! This module implements complexity analysis (0-100 scale) to help route
//! tasks to appropriate AI models. Unlike priority scoring which determines
//! execution order, complexity scoring determines *which model* should handle
//! a task.
//!
//! ## Complexity Factors
//!
//! - **Title/Description Analysis**: Keywords indicating complexity
//!   - "refactor", "architecture", "redesign" → high complexity
//!   - "fix", "update", "tweak" → low complexity
//! - **Label Analysis**: Labels like "complex", "architecture"
//! - **File Count**: More files = more complex
//! - **Blocking Dependencies**: Tasks blocking others are often more complex
//! - **Task Type**: Bugs can be simple or complex, features often complex
//!
//! ## Routing Thresholds
//!
//! - **Score 0-30**: Simple tasks → Budget tier (Haiku, DeepSeek)
//! - **Score 31-60**: Moderate tasks → Standard tier (Sonnet, GPT-4)
//! - **Score 61-100**: Complex tasks → Premium tier (Opus, O1)
//!
//! ## Usage
//!
//! ```no_run
//! use forge_worker::complexity::{ComplexityScorer, TaskContext};
//!
//! let scorer = ComplexityScorer::new();
//!
//! let context = TaskContext::new("Refactor authentication system for multi-tenant support")
//!     .with_description("Redesign the auth flow...")
//!     .with_labels(vec!["architecture".to_string(), "complex".to_string()])
//!     .with_file_count(12)
//!     .with_blocks(3)
//!     .as_feature();
//!
//! let score = scorer.score(&context);
//! println!("Complexity: {} → {:?}", score.score, score.tier());
//! ```

use chrono::Utc;
use forge_config::{CalibratedThresholds, ForgeConfig};
use forge_cost::{CostDatabase, CostOptimizer, OptimizerConfig, TierEfficiency};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// Default weight for title analysis.
const TITLE_WEIGHT: f64 = 0.30;

/// Default weight for label analysis.
const LABEL_WEIGHT: f64 = 0.25;

/// Default weight for file count.
const FILE_COUNT_WEIGHT: f64 = 0.20;

/// Default weight for blocking dependencies.
const BLOCKS_WEIGHT: f64 = 0.15;

/// Default weight for task type.
const TYPE_WEIGHT: f64 = 0.10;

/// Default inclusive upper bound for the budget tier.
pub const DEFAULT_BUDGET_THRESHOLD: u32 = 30;

/// Default inclusive upper bound for the standard tier.
pub const DEFAULT_STANDARD_THRESHOLD: u32 = 60;

/// Minimum number of completed assignments required for each tier before a
/// calibration pass can change any threshold.
pub const DEFAULT_CALIBRATION_MIN_SAMPLES: u64 = 20;

/// Default cadence for the calibration pass (one daily/hourly-rollup window).
pub const DEFAULT_CALIBRATION_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// Configuration for the complexity scorer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityConfig {
    /// Weight for title/description analysis
    pub title_weight: f64,

    /// Weight for label analysis
    pub label_weight: f64,

    /// Weight for file count
    pub file_count_weight: f64,

    /// Weight for blocking dependencies
    pub blocks_weight: f64,

    /// Weight for task type
    pub type_weight: f64,

    /// Maximum files to consider (files beyond this don't add complexity)
    pub max_file_count: usize,

    /// Maximum blocking dependencies to consider
    pub max_blocks: usize,

    /// Inclusive upper bound for the budget tier.
    #[serde(default = "default_budget_threshold")]
    pub budget_threshold: u32,

    /// Inclusive upper bound for the standard tier.
    #[serde(default = "default_standard_threshold")]
    pub standard_threshold: u32,
}

impl Default for ComplexityConfig {
    fn default() -> Self {
        Self {
            title_weight: TITLE_WEIGHT,
            label_weight: LABEL_WEIGHT,
            file_count_weight: FILE_COUNT_WEIGHT,
            blocks_weight: BLOCKS_WEIGHT,
            type_weight: TYPE_WEIGHT,
            max_file_count: 20,
            max_blocks: 5,
            budget_threshold: DEFAULT_BUDGET_THRESHOLD,
            standard_threshold: DEFAULT_STANDARD_THRESHOLD,
        }
    }
}

impl ComplexityConfig {
    /// Create a new config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build scorer settings from the persisted FORGE config.
    pub fn from_forge_config(config: &ForgeConfig) -> Self {
        let mut complexity = Self::default();
        if let Some(thresholds) = config.calibrated_thresholds {
            complexity.apply_calibrated_thresholds(thresholds);
        }
        complexity
    }

    /// Apply a calibrated threshold overlay while leaving all keyword weights
    /// and feature caps untouched.
    pub fn apply_calibrated_thresholds(&mut self, thresholds: CalibratedThresholds) {
        self.budget_threshold = thresholds.budget.clamp(1, 98);
        self.standard_threshold = thresholds
            .standard
            .clamp(self.budget_threshold.saturating_add(1), 99);
    }

    /// Return the current thresholds as a config-file overlay.
    pub fn calibrated_thresholds(&self) -> CalibratedThresholds {
        CalibratedThresholds::new(self.budget_threshold, self.standard_threshold)
    }

    /// Return the tier for a score using these thresholds.
    pub fn tier_for_score(&self, score: u32) -> ComplexityTier {
        match score {
            score if score <= self.budget_threshold => ComplexityTier::Budget,
            score if score <= self.standard_threshold => ComplexityTier::Standard,
            _ => ComplexityTier::Premium,
        }
    }

    /// Validate that weights sum to approximately 1.0.
    pub fn validate(&self) -> Result<(), String> {
        let sum = self.title_weight
            + self.label_weight
            + self.file_count_weight
            + self.blocks_weight
            + self.type_weight;
        let tolerance = 0.05;

        if (sum - 1.0).abs() > tolerance {
            return Err(format!(
                "Complexity weights should sum to ~1.0, got {:.3}",
                sum
            ));
        }

        if self.budget_threshold == 0
            || self.budget_threshold >= self.standard_threshold
            || self.standard_threshold >= 100
        {
            return Err(format!(
                "Complexity thresholds must satisfy 0 < budget < standard < 100, got {} and {}",
                self.budget_threshold, self.standard_threshold
            ));
        }

        Ok(())
    }
}

fn default_budget_threshold() -> u32 {
    DEFAULT_BUDGET_THRESHOLD
}

fn default_standard_threshold() -> u32 {
    DEFAULT_STANDARD_THRESHOLD
}

/// Context about a task for complexity analysis.
#[derive(Debug, Clone, Default)]
pub struct TaskContext {
    /// Task title/summary
    pub title: String,

    /// Optional detailed description
    pub description: Option<String>,

    /// Labels attached to the task
    pub labels: Vec<String>,

    /// Number of files involved (if known)
    pub file_count: Option<usize>,

    /// Number of other tasks this task blocks
    pub blocks_count: usize,

    /// Whether this is a bug fix
    pub is_bug: bool,

    /// Whether this is a new feature
    pub is_feature: bool,

    /// Whether this requires complex reasoning
    pub requires_reasoning: bool,
}

impl TaskContext {
    /// Create a new task context with just a title.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Default::default()
        }
    }

    /// Add description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Add labels.
    pub fn with_labels(mut self, labels: Vec<String>) -> Self {
        self.labels = labels;
        self
    }

    /// Set file count.
    pub fn with_file_count(mut self, count: usize) -> Self {
        self.file_count = Some(count);
        self
    }

    /// Set blocks count.
    pub fn with_blocks(mut self, count: usize) -> Self {
        self.blocks_count = count;
        self
    }

    /// Mark as bug.
    pub fn as_bug(mut self) -> Self {
        self.is_bug = true;
        self.is_feature = false;
        self
    }

    /// Mark as feature.
    pub fn as_feature(mut self) -> Self {
        self.is_feature = true;
        self.is_bug = false;
        self
    }

    /// Set requires reasoning.
    pub fn with_reasoning(mut self, requires: bool) -> Self {
        self.requires_reasoning = requires;
        self
    }
}

/// Result of complexity scoring with breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityScore {
    /// Total complexity score (0-100)
    pub score: u32,

    /// Title analysis contribution (0-100)
    pub title_score: u32,

    /// Label analysis contribution (0-100)
    pub label_score: u32,

    /// File count contribution (0-100)
    pub file_count_score: u32,

    /// Blocking dependencies contribution (0-100)
    pub blocks_score: u32,

    /// Task type contribution (0-100)
    pub type_score: u32,

    /// Detected complexity indicators
    pub indicators: Vec<String>,

    /// Threshold overlay used to classify this score.
    #[serde(default = "default_budget_threshold")]
    budget_threshold: u32,

    /// Standard-tier threshold overlay used to classify this score.
    #[serde(default = "default_standard_threshold")]
    standard_threshold: u32,
}

impl ComplexityScore {
    /// Get the recommended model tier for this complexity.
    pub fn tier(&self) -> ComplexityTier {
        match self.score {
            score if score <= self.budget_threshold => ComplexityTier::Budget,
            score if score <= self.standard_threshold => ComplexityTier::Standard,
            _ => ComplexityTier::Premium,
        }
    }

    /// Get the recommended tier using a configurable threshold overlay.
    pub fn tier_with_config(&self, config: &ComplexityConfig) -> ComplexityTier {
        config.tier_for_score(self.score)
    }

    /// Check if this is a simple task.
    pub fn is_simple(&self) -> bool {
        self.score <= 30
    }

    /// Check if this is a complex task.
    pub fn is_complex(&self) -> bool {
        self.score >= 61
    }

    /// Build the assignment record for this prediction and selected model.
    pub fn task_assignment(
        &self,
        bead_id: &str,
        assigned_model: &str,
    ) -> forge_cost::TaskAssignment {
        self.task_assignment_with_config(bead_id, assigned_model, &ComplexityConfig::default())
    }

    /// Build an assignment record using the supplied threshold overlay.
    pub fn task_assignment_with_config(
        &self,
        bead_id: &str,
        assigned_model: &str,
        config: &ComplexityConfig,
    ) -> forge_cost::TaskAssignment {
        forge_cost::TaskAssignment::new(
            bead_id,
            self.score,
            self.tier_with_config(config).to_string(),
            assigned_model,
        )
    }

    /// Persist this prediction alongside the model selected for the task.
    ///
    /// The scorer remains side-effect free during normal scoring. Callers invoke
    /// this only after the routing tier/model decision has been made and just
    /// before spawning the worker.
    pub fn record_assignment(
        &self,
        db: &forge_cost::CostDatabase,
        bead_id: &str,
        assigned_model: &str,
    ) -> forge_cost::Result<i64> {
        let assignment = self.task_assignment(bead_id, assigned_model);
        db.insert_task_assignment(&assignment)
    }

    /// Persist this prediction using the supplied threshold overlay.
    pub fn record_assignment_with_config(
        &self,
        db: &forge_cost::CostDatabase,
        bead_id: &str,
        assigned_model: &str,
        config: &ComplexityConfig,
    ) -> forge_cost::Result<i64> {
        let assignment = self.task_assignment_with_config(bead_id, assigned_model, config);
        db.insert_task_assignment(&assignment)
    }
}

/// Model tier recommendation based on complexity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplexityTier {
    /// Simple tasks - use budget models
    Budget,
    /// Moderate tasks - use standard models
    Standard,
    /// Complex tasks - use premium models
    Premium,
}

impl std::fmt::Display for ComplexityTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Budget => write!(f, "budget"),
            Self::Standard => write!(f, "standard"),
            Self::Premium => write!(f, "premium"),
        }
    }
}

/// Keywords indicating high complexity.
const HIGH_COMPLEXITY_KEYWORDS: &[&str] = &[
    "refactor",
    "architecture",
    "redesign",
    "migrate",
    "rewrite",
    "integration",
    "multi-tenant",
    "scalab",
    "distribute",
    "concurrent",
    "async",
    "parallel",
    "security",
    "auth",
    "encrypt",
    "performance",
    "optimize",
    "algorithm",
    "machine learning",
    "ml",
    "ai",
    "neural",
    "complex",
    "complicated",
    "deep",
    "fundamental",
    "core",
    "critical",
    "infrastructure",
    "deployment",
    "pipeline",
    "orchestrat",
];

/// Keywords indicating low complexity.
const LOW_COMPLEXITY_KEYWORDS: &[&str] = &[
    "fix",
    "typo",
    "rename",
    "update",
    "tweak",
    "minor",
    "simple",
    "small",
    "trivial",
    "cosmetic",
    "format",
    "style",
    "docs",
    "comment",
    "readme",
    "changelog",
    "cleanup",
    "remove unused",
    "deprecate",
    "log",
    "print",
    "debug",
];

/// Labels indicating complexity.
const HIGH_COMPLEXITY_LABELS: &[&str] = &[
    "architecture",
    "complex",
    "critical",
    "security",
    "infra",
    "integration",
    "refactor",
    "breaking",
];

const LOW_COMPLEXITY_LABELS: &[&str] = &[
    "good first issue",
    "help wanted",
    "documentation",
    "docs",
    "trivial",
    "easy",
    "beginner",
];

/// The complexity scorer engine.
#[derive(Debug, Clone)]
pub struct ComplexityScorer {
    config: ComplexityConfig,
}

impl Default for ComplexityScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl ComplexityScorer {
    /// Create a new scorer with default configuration.
    pub fn new() -> Self {
        Self {
            config: ComplexityConfig::default(),
        }
    }

    /// Create a scorer with custom configuration.
    pub fn with_config(config: ComplexityConfig) -> Self {
        Self { config }
    }

    /// Create a scorer using the calibrated threshold overlay from a loaded
    /// FORGE config. Missing overlays retain the static defaults.
    pub fn from_forge_config(config: &ForgeConfig) -> Self {
        Self::with_config(ComplexityConfig::from_forge_config(config))
    }

    /// Get the current configuration.
    pub fn config(&self) -> &ComplexityConfig {
        &self.config
    }

    /// Calculate complexity score for a task.
    pub fn score(&self, context: &TaskContext) -> ComplexityScore {
        let mut indicators = Vec::new();

        // Analyze title/description
        let title_score = self.analyze_title(context, &mut indicators);

        // Analyze labels
        let label_score = self.analyze_labels(context, &mut indicators);

        // Analyze file count
        let file_count_score = self.analyze_file_count(context, &mut indicators);

        // Analyze blocking dependencies
        let blocks_score = self.analyze_blocks(context, &mut indicators);

        // Analyze task type
        let type_score = self.analyze_type(context, &mut indicators);

        // Calculate weighted total
        let total = (title_score as f64 * self.config.title_weight)
            + (label_score as f64 * self.config.label_weight)
            + (file_count_score as f64 * self.config.file_count_weight)
            + (blocks_score as f64 * self.config.blocks_weight)
            + (type_score as f64 * self.config.type_weight);

        let score = total.round().clamp(0.0, 100.0) as u32;

        ComplexityScore {
            score,
            title_score,
            label_score,
            file_count_score,
            blocks_score,
            type_score,
            indicators,
            budget_threshold: self.config.budget_threshold,
            standard_threshold: self.config.standard_threshold,
        }
    }

    /// Quick complexity score without full context.
    pub fn quick_score(&self, title: &str, labels: &[String]) -> u32 {
        let context = TaskContext::new(title).with_labels(labels.to_vec());
        self.score(&context).score
    }

    /// Analyze title and description for complexity indicators.
    fn analyze_title(&self, context: &TaskContext, indicators: &mut Vec<String>) -> u32 {
        let text = format!(
            "{} {}",
            context.title,
            context.description.as_deref().unwrap_or("")
        )
        .to_lowercase();

        let mut score: f64 = 50.0; // Start at neutral

        // Check for high complexity keywords
        for keyword in HIGH_COMPLEXITY_KEYWORDS {
            if text.contains(keyword) {
                score += 8.0;
                indicators.push(format!("keyword:{}", keyword));
            }
        }

        // Check for low complexity keywords
        for keyword in LOW_COMPLEXITY_KEYWORDS {
            if text.contains(keyword) {
                score -= 8.0;
                indicators.push(format!("simple:{}", keyword));
            }
        }

        // Check for reasoning requirement
        if context.requires_reasoning {
            score += 15.0;
            indicators.push("requires_reasoning".to_string());
        }

        // Length heuristic: very short titles are often simple
        if context.title.len() < 20 {
            score -= 5.0;
        } else if context.title.len() > 80 {
            score += 5.0;
        }

        score.round().clamp(0.0, 100.0) as u32
    }

    /// Analyze labels for complexity indicators.
    fn analyze_labels(&self, context: &TaskContext, indicators: &mut Vec<String>) -> u32 {
        let mut score: f64 = 50.0; // Start at neutral

        for label in &context.labels {
            let label_lower = label.to_lowercase();

            // Check high complexity labels
            for high_label in HIGH_COMPLEXITY_LABELS {
                if label_lower.contains(high_label) {
                    score += 12.0;
                    indicators.push(format!("label:{}", label));
                    break;
                }
            }

            // Check low complexity labels
            for low_label in LOW_COMPLEXITY_LABELS {
                if label_lower.contains(low_label) {
                    score -= 10.0;
                    indicators.push(format!("simple_label:{}", label));
                    break;
                }
            }
        }

        score.round().clamp(0.0, 100.0) as u32
    }

    /// Analyze file count for complexity.
    fn analyze_file_count(&self, context: &TaskContext, indicators: &mut Vec<String>) -> u32 {
        match context.file_count {
            Some(0) => 30,
            Some(1) => 35,
            Some(2..=3) => 45,
            Some(4..=5) => 55,
            Some(6..=10) => 65,
            Some(count) => {
                let capped = count.min(self.config.max_file_count);
                // Scale from 65 to 100 based on files 10-20+
                let extra = ((capped - 10) as f64 / 10.0 * 35.0).min(35.0);
                if count > 5 {
                    indicators.push(format!("files:{}", count));
                }
                (65.0 + extra).min(100.0) as u32
            }
            None => 50, // Unknown, assume moderate
        }
    }

    /// Analyze blocking dependencies.
    fn analyze_blocks(&self, context: &TaskContext, indicators: &mut Vec<String>) -> u32 {
        match context.blocks_count {
            0 => 40,
            1 => 50,
            2 => 60,
            3..=5 => {
                indicators.push(format!("blocks:{}", context.blocks_count));
                70
            }
            _ => {
                indicators.push(format!("blocks:{}", context.blocks_count));
                85
            }
        }
    }

    /// Analyze task type.
    fn analyze_type(&self, context: &TaskContext, indicators: &mut Vec<String>) -> u32 {
        // Base score depends on task type
        let base = if context.is_feature {
            indicators.push("feature".to_string());
            60 // Features tend to be more complex
        } else if context.is_bug {
            // Bugs vary widely - start neutral
            50
        } else {
            50 // Unknown type
        };

        // Reasoning requirement overrides
        if context.requires_reasoning {
            return 80;
        }

        base
    }
}

/// Errors returned by the periodic complexity calibration job.
#[derive(Debug, Error)]
pub enum CalibrationError {
    /// Cost database query failed.
    #[error("cost query failed: {0}")]
    Cost(#[from] forge_cost::CostError),

    /// Configuration could not be loaded or written.
    #[error("configuration error: {0}")]
    Config(String),
}

/// Result type used by complexity calibration APIs.
pub type CalibrationResult<T> = std::result::Result<T, CalibrationError>;

/// A real-time activity event emitted after thresholds are recalibrated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComplexityCalibrationEvent {
    /// Event timestamp.
    pub timestamp: chrono::DateTime<Utc>,

    /// Human-readable activity-log message.
    pub message: String,

    /// Threshold changes included in this pass.
    pub changes: Vec<ThresholdChange>,

    /// Estimated savings for the observed calibration sample.
    pub estimated_savings_usd: f64,
}

/// One changed tier cutoff.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThresholdChange {
    /// Name of the cutoff (budget or standard).
    pub tier: String,

    /// Previous inclusive upper bound.
    pub old: u32,

    /// New inclusive upper bound.
    pub new: u32,
}

/// Outcome of one calibration pass.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationReport {
    /// Whether a new overlay was written.
    pub changed: bool,

    /// Thresholds used before this pass.
    pub old_thresholds: CalibratedThresholds,

    /// Thresholds proposed by this pass.
    pub new_thresholds: CalibratedThresholds,

    /// Empirical tier statistics used by the pass.
    pub tier_efficiency: Vec<TierEfficiency>,

    /// Estimated savings represented by the proposed changes.
    pub estimated_savings_usd: f64,

    /// Why the pass did not change configuration, if applicable.
    pub skipped_reason: Option<String>,

    /// Human-readable summary suitable for logs and activity panels.
    pub message: String,
}

/// Periodic closed-loop complexity calibration job.
///
/// The job is deliberately owned by `forge-worker`: it reads the public
/// efficiency API from `forge-cost`, updates the worker-owned threshold
/// overlay through `forge-config`, and never asks the cost crate to write
/// configuration. Call [`ComplexityCalibrationJob::start`] to run it on the
/// same daily cadence used by the cost rollups, or [`run_once`] for a manual
/// or test-triggered pass.
pub struct ComplexityCalibrationJob {
    db: Arc<CostDatabase>,
    complexity_config: ComplexityConfig,
    config_path: PathBuf,
    min_samples: u64,
    interval: Duration,
    activity_tx: broadcast::Sender<ComplexityCalibrationEvent>,
}

impl std::fmt::Debug for ComplexityCalibrationJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComplexityCalibrationJob")
            .field("config_path", &self.config_path)
            .field("min_samples", &self.min_samples)
            .field("interval", &self.interval)
            .finish_non_exhaustive()
    }
}

impl ComplexityCalibrationJob {
    /// Create a calibration job with default cadence and sample floor.
    pub fn new(db: Arc<CostDatabase>, complexity_config: ComplexityConfig) -> Self {
        let config_path =
            forge_config::config_path().unwrap_or_else(|| PathBuf::from(".forge/config.yaml"));
        let (activity_tx, _) = broadcast::channel(32);
        Self {
            db,
            complexity_config,
            config_path,
            min_samples: DEFAULT_CALIBRATION_MIN_SAMPLES,
            interval: Duration::from_secs(DEFAULT_CALIBRATION_INTERVAL_SECS),
            activity_tx,
        }
    }

    /// Use a specific config path, primarily for isolated tests.
    pub fn with_config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_path = path.into();
        self
    }

    /// Set the minimum terminal samples required in every tier.
    pub fn with_min_samples(mut self, min_samples: u64) -> Self {
        self.min_samples = min_samples.max(1);
        self
    }

    /// Set the scheduler interval.
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Use an externally created activity channel.
    pub fn with_activity_sender(
        mut self,
        activity_tx: broadcast::Sender<ComplexityCalibrationEvent>,
    ) -> Self {
        self.activity_tx = activity_tx;
        self
    }

    /// Subscribe to calibration activity events.
    pub fn subscribe(&self) -> broadcast::Receiver<ComplexityCalibrationEvent> {
        self.activity_tx.subscribe()
    }

    /// Start the background calibration loop.
    pub fn start(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            // The rollup scheduler also runs immediately on startup. Do the
            // same here, then wait for the next full calibration window.
            self.run_and_report();
            let mut interval = tokio::time::interval(self.interval);
            interval.tick().await;
            loop {
                interval.tick().await;
                self.run_and_report();
            }
        })
    }

    /// Run one calibration pass synchronously.
    pub fn run_once(&self) -> CalibrationResult<CalibrationReport> {
        let mut config = self.load_config()?;
        let old_thresholds = config
            .calibrated_thresholds
            .map(|thresholds| CalibratedThresholds::new(thresholds.budget, thresholds.standard))
            .unwrap_or_else(|| self.complexity_config.calibrated_thresholds());

        if !config.auto_calibrate {
            return Ok(CalibrationReport {
                changed: false,
                old_thresholds,
                new_thresholds: old_thresholds,
                tier_efficiency: Vec::new(),
                estimated_savings_usd: 0.0,
                skipped_reason: Some("auto_calibrate is disabled".to_string()),
                message: "Routing calibration skipped: auto_calibrate is disabled".to_string(),
            });
        }

        let optimizer = CostOptimizer::new(&self.db, OptimizerConfig::default());
        let efficiency = optimizer.calculate_tier_efficiency()?;
        let by_tier: HashMap<String, TierEfficiency> = efficiency
            .iter()
            .cloned()
            .map(|tier| (tier.predicted_tier.to_ascii_lowercase(), tier))
            .collect();

        let missing_or_small = ["budget", "standard", "premium"].iter().find(|tier| {
            by_tier
                .get(**tier)
                .map(|stats| stats.sample_count < self.min_samples)
                .unwrap_or(true)
        });
        if let Some(tier) = missing_or_small {
            let message = format!(
                "Routing calibration skipped: {} tier has fewer than {} completed samples",
                tier, self.min_samples
            );
            debug!(tier = %tier, min_samples = self.min_samples, "Complexity calibration sample gate not met");
            return Ok(CalibrationReport {
                changed: false,
                old_thresholds,
                new_thresholds: old_thresholds,
                tier_efficiency: efficiency,
                estimated_savings_usd: 0.0,
                skipped_reason: Some(message.clone()),
                message,
            });
        }

        let budget = by_tier.get("budget").expect("sample gate checked budget");
        let standard = by_tier
            .get("standard")
            .expect("sample gate checked standard");
        let premium = by_tier.get("premium").expect("sample gate checked premium");

        let (budget_threshold, budget_savings) =
            move_cutoff(old_thresholds.budget, budget, standard);
        let (standard_threshold, standard_savings) =
            move_cutoff(old_thresholds.standard, standard, premium);
        let new_thresholds = CalibratedThresholds::new(
            budget_threshold.min(standard_threshold.saturating_sub(1)),
            standard_threshold.max(budget_threshold.saturating_add(1)),
        );
        let estimated_savings_usd = budget_savings + standard_savings;

        if new_thresholds == old_thresholds {
            return Ok(CalibrationReport {
                changed: false,
                old_thresholds,
                new_thresholds,
                tier_efficiency: efficiency,
                estimated_savings_usd,
                skipped_reason: Some(
                    "empirical tier costs did not justify a threshold change".to_string(),
                ),
                message: "Routing calibration made no threshold change".to_string(),
            });
        }

        config.calibrated_thresholds = Some(new_thresholds);
        config
            .save_to(&self.config_path)
            .map_err(|error| CalibrationError::Config(error.to_string()))?;

        let changes = threshold_changes(old_thresholds, new_thresholds);
        let message = format_calibration_message(&changes, estimated_savings_usd);
        let event = ComplexityCalibrationEvent {
            timestamp: Utc::now(),
            message: message.clone(),
            changes,
            estimated_savings_usd,
        };
        let _ = self.activity_tx.send(event);
        info!(
            target: "forge::activity",
            event = "complexity_calibration",
            estimated_savings_usd,
            message = %message,
            "Complexity thresholds recalibrated"
        );

        Ok(CalibrationReport {
            changed: true,
            old_thresholds,
            new_thresholds,
            tier_efficiency: efficiency,
            estimated_savings_usd,
            skipped_reason: None,
            message,
        })
    }

    /// Alias for callers that prefer calibration terminology.
    pub fn calibrate(&self) -> CalibrationResult<CalibrationReport> {
        self.run_once()
    }

    fn load_config(&self) -> CalibrationResult<ForgeConfig> {
        if self.config_path.exists() {
            ForgeConfig::load_from_with_error(&self.config_path)
                .map_err(|error| CalibrationError::Config(error.to_string()))
        } else {
            Ok(ForgeConfig::default())
        }
    }

    fn run_and_report(&self) {
        match self.run_once() {
            Ok(report) if report.changed => {
                info!(message = %report.message, "Periodic complexity calibration completed")
            }
            Ok(_) => {}
            Err(error) => warn!(error = %error, "Periodic complexity calibration failed"),
        }
    }
}

const CALIBRATION_STEP: u32 = 5;

/// Move a cutoff toward the tier with the lower empirical cost per success.
fn move_cutoff(
    cutoff: u32,
    lower_tier: &TierEfficiency,
    higher_tier: &TierEfficiency,
) -> (u32, f64) {
    if !lower_tier.cost_per_success.is_finite()
        || !higher_tier.cost_per_success.is_finite()
        || (lower_tier.cost_per_success - higher_tier.cost_per_success).abs() < f64::EPSILON
    {
        return (cutoff, 0.0);
    }

    let cheaper_cost = lower_tier
        .cost_per_success
        .min(higher_tier.cost_per_success);
    let expensive_cost = lower_tier
        .cost_per_success
        .max(higher_tier.cost_per_success);
    let moved_tasks = lower_tier.sample_count.min(higher_tier.sample_count) as f64
        * (CALIBRATION_STEP as f64 / 100.0);
    let estimated_savings = (expensive_cost - cheaper_cost) * moved_tasks;
    let threshold = if lower_tier.cost_per_success < higher_tier.cost_per_success {
        cutoff.saturating_add(CALIBRATION_STEP)
    } else {
        cutoff.saturating_sub(CALIBRATION_STEP)
    };
    (threshold.clamp(1, 98), estimated_savings)
}

fn threshold_changes(
    old_thresholds: CalibratedThresholds,
    new_thresholds: CalibratedThresholds,
) -> Vec<ThresholdChange> {
    let mut changes = Vec::new();
    if old_thresholds.budget != new_thresholds.budget {
        changes.push(ThresholdChange {
            tier: "Budget".to_string(),
            old: old_thresholds.budget,
            new: new_thresholds.budget,
        });
    }
    if old_thresholds.standard != new_thresholds.standard {
        changes.push(ThresholdChange {
            tier: "Standard".to_string(),
            old: old_thresholds.standard,
            new: new_thresholds.standard,
        });
    }
    changes
}

fn format_calibration_message(changes: &[ThresholdChange], estimated_savings_usd: f64) -> String {
    let changes = changes
        .iter()
        .map(|change| format!("{} cutoff {} -> {}", change.tier, change.old, change.new))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Routing: {}; estimated savings ${:.2} per calibration window",
        changes, estimated_savings_usd
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation() {
        let config = ComplexityConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_custom_thresholds_classify_scores() {
        let config = ComplexityConfig {
            budget_threshold: 20,
            standard_threshold: 40,
            ..ComplexityConfig::default()
        };
        let scorer = ComplexityScorer::with_config(config);

        let score = scorer.score(&TaskContext::new("Task"));
        assert_eq!(score.tier(), score.tier_with_config(scorer.config()));
        assert_eq!(score.tier(), ComplexityTier::Premium);
    }

    #[test]
    fn test_simple_task() {
        let scorer = ComplexityScorer::new();

        let context = TaskContext::new("Fix typo in README")
            .with_labels(vec!["documentation".to_string()])
            .with_file_count(1);

        let score = scorer.score(&context);
        assert!(
            score.score <= 50,
            "Simple task should have low score, got {}",
            score.score
        );
        // Score around 37 → Standard tier (31-60)
        assert_eq!(score.tier(), ComplexityTier::Standard);
    }

    #[test]
    fn test_complex_task() {
        let scorer = ComplexityScorer::new();

        let context = TaskContext::new("Refactor authentication system for multi-tenant support")
            .with_labels(vec!["architecture".to_string(), "complex".to_string()])
            .with_file_count(15)
            .with_blocks(3)
            .as_feature();

        let score = scorer.score(&context);
        assert!(
            score.score >= 60,
            "Complex task should have high score, got {}",
            score.score
        );
        assert!(score.is_complex());
        assert_eq!(score.tier(), ComplexityTier::Premium);
    }

    #[test]
    fn test_moderate_task() {
        let scorer = ComplexityScorer::new();

        let context = TaskContext::new("Update user profile page styling").with_file_count(3);

        let score = scorer.score(&context);
        // Should be moderate (not simple, not complex)
        assert!(score.score > 30 && score.score < 70);
        assert_eq!(score.tier(), ComplexityTier::Standard);
    }

    #[test]
    fn test_quick_score() {
        let scorer = ComplexityScorer::new();

        let simple = scorer.quick_score("Fix typo", &[]);
        assert!(simple <= 50);

        let complex = scorer.quick_score(
            "Refactor architecture for scalability",
            &["complex".to_string()],
        );
        assert!(complex >= 50);
    }

    #[test]
    fn test_reasoning_override() {
        let scorer = ComplexityScorer::new();

        let context = TaskContext::new("Simple task").with_reasoning(true);

        let score = scorer.score(&context);
        assert!(
            score.score >= 50,
            "Reasoning requirement should boost score"
        );
    }

    #[test]
    fn test_file_count_progression() {
        let scorer = ComplexityScorer::new();

        let scores: Vec<u32> = (0..=25)
            .map(|count| {
                let context = TaskContext::new("Task").with_file_count(count);
                scorer.score(&context).file_count_score
            })
            .collect();

        // Scores should generally increase
        for i in 1..scores.len() {
            assert!(
                scores[i] >= scores[i - 1] || scores[i] == 100,
                "File count {} score {} should be >= {} score {}",
                i,
                scores[i],
                i - 1,
                scores[i - 1]
            );
        }
    }

    #[test]
    fn test_blocks_progression() {
        let scorer = ComplexityScorer::new();

        let no_blocks = TaskContext::new("Task").with_blocks(0);
        let few_blocks = TaskContext::new("Task").with_blocks(3);
        let many_blocks = TaskContext::new("Task").with_blocks(10);

        let score_no = scorer.score(&no_blocks);
        let score_few = scorer.score(&few_blocks);
        let score_many = scorer.score(&many_blocks);

        assert!(score_many.score > score_few.score);
        assert!(score_few.score > score_no.score);
    }

    #[test]
    fn test_complexity_tier_display() {
        assert_eq!(ComplexityTier::Budget.to_string(), "budget");
        assert_eq!(ComplexityTier::Standard.to_string(), "standard");
        assert_eq!(ComplexityTier::Premium.to_string(), "premium");
    }

    #[test]
    fn test_record_assignment() {
        let db = forge_cost::CostDatabase::open_in_memory().unwrap();
        let score = ComplexityScorer::new().score(
            &TaskContext::new("Refactor the authentication architecture")
                .with_labels(vec!["architecture".to_string()]),
        );

        let id = score
            .record_assignment(&db, "bd-123", "claude-opus")
            .unwrap();
        assert!(id > 0);

        let conn = db.connection();
        let conn = conn.lock().unwrap();
        let row: (i64, String, String) = conn
            .query_row(
                "SELECT predicted_score, predicted_tier, assigned_model
                 FROM task_assignments WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row.0, score.score as i64);
        assert_eq!(row.1, score.tier().to_string());
        assert_eq!(row.2, "claude-opus");
    }

    #[test]
    fn test_empty_context() {
        let scorer = ComplexityScorer::new();
        let context = TaskContext::default();

        let score = scorer.score(&context);
        // Should default to moderate complexity
        assert!(score.score >= 30 && score.score <= 70);
    }

    #[test]
    fn test_high_complexity_keywords() {
        let scorer = ComplexityScorer::new();

        for keyword in &["refactor", "architecture", "security", "integration"] {
            let context = TaskContext::new(format!("{} the system", keyword));
            let score = scorer.score(&context);
            assert!(
                score.score > 45,
                "Keyword '{}' should increase complexity, got {}",
                keyword,
                score.score
            );
        }
    }

    #[test]
    fn test_low_complexity_keywords() {
        let scorer = ComplexityScorer::new();

        for keyword in &["fix typo", "update docs", "minor tweak"] {
            let context = TaskContext::new(format!("{} in the codebase", keyword));
            let score = scorer.score(&context);
            assert!(
                score.score < 60,
                "Keyword '{}' should decrease complexity, got {}",
                keyword,
                score.score
            );
        }
    }

    fn insert_calibration_sample(
        db: &forge_cost::CostDatabase,
        bead_id: &str,
        score: u32,
        tier: &str,
        cost: f64,
    ) {
        db.insert_task_assignment(&forge_cost::TaskAssignment::new(
            bead_id, score, tier, "model",
        ))
        .unwrap();
        db.record_task_event(
            bead_id,
            "completed",
            Some("worker"),
            Some("model"),
            0.0,
            10,
            None,
        )
        .unwrap();
        db.insert_api_calls(&[forge_cost::ApiCall::new(
            Utc::now(),
            "worker",
            "model",
            10,
            10,
            cost,
        )
        .with_bead(bead_id)])
            .unwrap();
    }

    #[test]
    fn test_calibration_requires_minimum_samples_in_every_tier() {
        let db = Arc::new(forge_cost::CostDatabase::open_in_memory().unwrap());
        insert_calibration_sample(&db, "budget-1", 10, "budget", 0.10);
        insert_calibration_sample(&db, "standard-1", 50, "standard", 0.20);
        insert_calibration_sample(&db, "premium-1", 90, "premium", 0.30);
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yaml");

        let job = ComplexityCalibrationJob::new(Arc::clone(&db), ComplexityConfig::default())
            .with_config_path(&config_path)
            .with_min_samples(2);
        let report = job.run_once().unwrap();

        assert!(!report.changed);
        assert!(
            report
                .skipped_reason
                .as_deref()
                .unwrap()
                .contains("fewer than 2")
        );
        assert!(!config_path.exists());
    }

    #[test]
    fn test_calibration_writes_overlay_and_emits_activity() {
        let db = Arc::new(forge_cost::CostDatabase::open_in_memory().unwrap());
        for index in 0..2 {
            insert_calibration_sample(&db, &format!("budget-{index}"), 10, "budget", 0.10);
            insert_calibration_sample(&db, &format!("standard-{index}"), 50, "standard", 0.20);
            insert_calibration_sample(&db, &format!("premium-{index}"), 90, "premium", 0.30);
        }

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yaml");
        let mut user_config = ForgeConfig::default();
        user_config.theme.name = Some("cyberpunk".to_string());
        user_config.workers.default_model = "opus".to_string();
        user_config.save_to(&config_path).unwrap();
        let job = ComplexityCalibrationJob::new(Arc::clone(&db), ComplexityConfig::default())
            .with_config_path(&config_path)
            .with_min_samples(2);
        let mut events = job.subscribe();
        let report = job.run_once().unwrap();

        assert!(report.changed);
        assert_eq!(report.old_thresholds, CalibratedThresholds::new(30, 60));
        assert_eq!(report.new_thresholds, CalibratedThresholds::new(35, 65));
        assert!(report.estimated_savings_usd > 0.0);
        assert!(report.message.contains("estimated savings"));

        let saved = ForgeConfig::load_from_with_error(&config_path).unwrap();
        assert!(saved.auto_calibrate);
        assert_eq!(saved.theme.name.as_deref(), Some("cyberpunk"));
        assert_eq!(saved.workers.default_model, "opus");
        assert_eq!(saved.calibrated_thresholds, Some(report.new_thresholds));
        let event = events.try_recv().unwrap();
        assert_eq!(event.changes.len(), 2);
        assert!(event.message.contains("Budget cutoff 30 -> 35"));
        assert!(event.message.contains("Standard cutoff 60 -> 65"));
    }

    #[test]
    fn test_calibration_respects_disabled_flag() {
        let db = Arc::new(forge_cost::CostDatabase::open_in_memory().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yaml");
        let mut config = ForgeConfig::default();
        config.auto_calibrate = false;
        config.save_to(&config_path).unwrap();

        let job = ComplexityCalibrationJob::new(db, ComplexityConfig::default())
            .with_config_path(&config_path);
        let report = job.run_once().unwrap();

        assert!(!report.changed);
        assert_eq!(
            report.skipped_reason.as_deref(),
            Some("auto_calibrate is disabled")
        );
        assert!(
            ForgeConfig::load_from_with_error(&config_path)
                .unwrap()
                .calibrated_thresholds
                .is_none()
        );
    }
}
