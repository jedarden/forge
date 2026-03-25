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

use serde::{Deserialize, Serialize};

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
        }
    }
}

impl ComplexityConfig {
    /// Create a new config with default values.
    pub fn new() -> Self {
        Self::default()
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

        Ok(())
    }
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
}

impl ComplexityScore {
    /// Get the recommended model tier for this complexity.
    pub fn tier(&self) -> ComplexityTier {
        match self.score {
            0..=30 => ComplexityTier::Budget,
            31..=60 => ComplexityTier::Standard,
            61..=100 => ComplexityTier::Premium,
            _ => ComplexityTier::Standard,
        }
    }

    /// Check if this is a simple task.
    pub fn is_simple(&self) -> bool {
        self.score <= 30
    }

    /// Check if this is a complex task.
    pub fn is_complex(&self) -> bool {
        self.score >= 61
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation() {
        let config = ComplexityConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_simple_task() {
        let scorer = ComplexityScorer::new();

        let context = TaskContext::new("Fix typo in README")
            .with_labels(vec!["documentation".to_string()])
            .with_file_count(1);

        let score = scorer.score(&context);
        assert!(score.score <= 50, "Simple task should have low score, got {}", score.score);
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
        assert!(score.score >= 60, "Complex task should have high score, got {}", score.score);
        assert!(score.is_complex());
        assert_eq!(score.tier(), ComplexityTier::Premium);
    }

    #[test]
    fn test_moderate_task() {
        let scorer = ComplexityScorer::new();

        let context = TaskContext::new("Update user profile page styling")
            .with_file_count(3);

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

        let complex = scorer.quick_score("Refactor architecture for scalability", &["complex".to_string()]);
        assert!(complex >= 50);
    }

    #[test]
    fn test_reasoning_override() {
        let scorer = ComplexityScorer::new();

        let context = TaskContext::new("Simple task")
            .with_reasoning(true);

        let score = scorer.score(&context);
        assert!(score.score >= 50, "Reasoning requirement should boost score");
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
                i, scores[i], i - 1, scores[i - 1]
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
}
