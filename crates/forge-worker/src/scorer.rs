//! Task value scoring system for prioritizing bead assignment.
//!
//! This module implements a scoring algorithm (0-100 scale) to prioritize
//! task assignment based on multiple factors: priority, blockers, age, and labels.
//!
//! ## Scoring Formula
//!
//! Total Score = (Priority × 0.4) + (Blockers × 0.3) + (Age × 0.2) + (Labels × 0.1)
//!
//! - **Priority**: P0=40, P1=32, P2=24, P3=16, P4=8 points
//! - **Blockers**: 10 points per blocked task, max 30
//! - **Age**: 1 point per hour since creation, max 20
//! - **Labels**: "critical"=10, "urgent"=7, "important"=4 points
//!
//! ## Usage
//!
//! ```no_run
//! use forge_worker::scorer::{TaskScorer, ScoringConfig};
//!
//! // Create scorer with default weights
//! let scorer = TaskScorer::new();
//!
//! // Or with custom configuration
//! let config = ScoringConfig {
//!     priority_weight: 0.5,
//!     blockers_weight: 0.25,
//!     age_weight: 0.15,
//!     labels_weight: 0.1,
//!     ..Default::default()
//! };
//! let scorer = TaskScorer::with_config(config);
//!
//! // Calculate score for a bead
//! let score = scorer.score(
//!     0,                          // priority (P0)
//!     3,                          // dependent_count (blocking 3 other tasks)
//!     Some(24),                   // age in hours
//!     &["critical".to_string()],  // labels
//! );
//! ```

use serde::{Deserialize, Serialize};

/// Default weight for priority component in scoring.
const DEFAULT_PRIORITY_WEIGHT: f64 = 0.4;

/// Default weight for blockers component in scoring.
const DEFAULT_BLOCKERS_WEIGHT: f64 = 0.3;

/// Default weight for age component in scoring.
const DEFAULT_AGE_WEIGHT: f64 = 0.2;

/// Default weight for labels component in scoring.
const DEFAULT_LABELS_WEIGHT: f64 = 0.1;

/// Maximum points for blockers component.
const MAX_BLOCKERS_POINTS: u32 = 30;

/// Maximum points for age component.
const MAX_AGE_POINTS: u32 = 20;

/// Maximum points for labels component.
const MAX_LABELS_POINTS: u32 = 10;

/// Points per blocked task.
const POINTS_PER_BLOCKER: u32 = 10;

/// Points per hour of age.
const POINTS_PER_HOUR: u32 = 1;

/// Label priority values.
const LABEL_CRITICAL_POINTS: u32 = 10;
const LABEL_URGENT_POINTS: u32 = 7;
const LABEL_IMPORTANT_POINTS: u32 = 4;

/// Configuration for the task scoring algorithm.
///
/// This struct allows customization of the weights applied to each
/// scoring component. All weights should sum to approximately 1.0
/// for consistent scoring behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringConfig {
    /// Weight for priority component (default: 0.4)
    pub priority_weight: f64,

    /// Weight for blockers component (default: 0.3)
    pub blockers_weight: f64,

    /// Weight for age component (default: 0.2)
    pub age_weight: f64,

    /// Weight for labels component (default: 0.1)
    pub labels_weight: f64,

    /// Maximum age in hours to consider (default: 20)
    pub max_age_hours: u32,

    /// Maximum blockers to count (default: 3)
    pub max_blockers: u32,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            priority_weight: DEFAULT_PRIORITY_WEIGHT,
            blockers_weight: DEFAULT_BLOCKERS_WEIGHT,
            age_weight: DEFAULT_AGE_WEIGHT,
            labels_weight: DEFAULT_LABELS_WEIGHT,
            max_age_hours: MAX_AGE_POINTS,
            max_blockers: MAX_BLOCKERS_POINTS / POINTS_PER_BLOCKER,
        }
    }
}

impl ScoringConfig {
    /// Create a new scoring config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate that weights sum to approximately 1.0.
    pub fn validate(&self) -> Result<(), String> {
        let sum = self.priority_weight + self.blockers_weight + self.age_weight + self.labels_weight;
        let tolerance = 0.01;

        if (sum - 1.0).abs() > tolerance {
            return Err(format!(
                "Scoring weights must sum to 1.0, got {:.3}",
                sum
            ));
        }

        Ok(())
    }

    /// Load configuration from a YAML file.
    pub fn from_yaml(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Save configuration to a YAML file.
    pub fn to_yaml(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_yaml::to_string(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

/// A task scorer that calculates priority scores for beads.
///
/// The scorer uses a weighted formula combining multiple factors:
/// - Priority level (P0-P4)
/// - Number of tasks blocked by this task
/// - Age of the task
/// - Special labels (critical, urgent, important)
#[derive(Debug, Clone)]
pub struct TaskScorer {
    config: ScoringConfig,
}

impl Default for TaskScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskScorer {
    /// Create a new scorer with default configuration.
    pub fn new() -> Self {
        Self {
            config: ScoringConfig::default(),
        }
    }

    /// Create a scorer with custom configuration.
    pub fn with_config(config: ScoringConfig) -> Self {
        Self { config }
    }

    /// Get the current configuration.
    pub fn config(&self) -> &ScoringConfig {
        &self.config
    }

    /// Calculate the total score for a task.
    ///
    /// # Arguments
    ///
    /// * `priority` - Priority level (0-4, where 0 is P0/critical)
    /// * `dependent_count` - Number of other tasks blocked by this task
    /// * `age_hours` - Optional age of the task in hours (None = 0)
    /// * `labels` - Labels attached to the task
    ///
    /// # Returns
    ///
    /// A score from 0 to 100.
    pub fn score(
        &self,
        priority: u8,
        dependent_count: usize,
        age_hours: Option<u32>,
        labels: &[String],
    ) -> u32 {
        let priority_score = self.calculate_priority_score(priority);
        let blockers_score = self.calculate_blockers_score(dependent_count);
        let age_score = self.calculate_age_score(age_hours);
        let labels_score = self.calculate_labels_score(labels);

        // Apply weights and sum
        let weighted_score = (priority_score as f64 * self.config.priority_weight)
            + (blockers_score as f64 * self.config.blockers_weight)
            + (age_score as f64 * self.config.age_weight)
            + (labels_score as f64 * self.config.labels_weight);

        // Clamp to 0-100
        weighted_score.round().min(100.0).max(0.0) as u32
    }

    /// Calculate the priority component score.
    ///
    /// Per the specification:
    /// - P0 = 40 points
    /// - P1 = 32 points
    /// - P2 = 24 points
    /// - P3 = 16 points
    /// - P4+ = 8 points
    fn calculate_priority_score(&self, priority: u8) -> u32 {
        match priority {
            0 => 40, // P0 Critical
            1 => 32, // P1 High
            2 => 24, // P2 Medium
            3 => 16, // P3 Low
            _ => 8,  // P4+ Backlog
        }
    }

    /// Calculate the blockers component score.
    ///
    /// 10 points per blocked task, max 30 points.
    fn calculate_blockers_score(&self, dependent_count: usize) -> u32 {
        let capped = (dependent_count as u32).min(self.config.max_blockers);
        (capped * POINTS_PER_BLOCKER).min(MAX_BLOCKERS_POINTS)
    }

    /// Calculate the age component score.
    ///
    /// 1 point per hour since creation, max 20 points.
    fn calculate_age_score(&self, age_hours: Option<u32>) -> u32 {
        let hours = age_hours.unwrap_or(0).min(self.config.max_age_hours);
        (hours * POINTS_PER_HOUR).min(MAX_AGE_POINTS)
    }

    /// Calculate the labels component score.
    ///
    /// Points for special labels:
    /// - "critical" = 10 points
    /// - "urgent" = 7 points
    /// - "important" = 4 points
    ///
    /// Only the highest-value label is counted.
    fn calculate_labels_score(&self, labels: &[String]) -> u32 {
        let mut max_label_score = 0u32;

        for label in labels {
            let label_lower = label.to_lowercase();
            let score = if label_lower == "critical" {
                LABEL_CRITICAL_POINTS
            } else if label_lower == "urgent" {
                LABEL_URGENT_POINTS
            } else if label_lower == "important" {
                LABEL_IMPORTANT_POINTS
            } else {
                0
            };
            max_label_score = max_label_score.max(score);
        }

        max_label_score.min(MAX_LABELS_POINTS)
    }

    /// Parse age from a timestamp string.
    ///
    /// Accepts ISO 8601 format timestamps and calculates hours since creation.
    pub fn parse_age_hours(created_at: &str) -> Option<u32> {
        // Try parsing ISO 8601 timestamp
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(created_at) {
            let now = chrono::Utc::now();
            let duration = now.signed_duration_since(dt);
            return Some(duration.num_hours().max(0) as u32);
        }

        // Try chrono's datetime parser for other formats
        if let Ok(dt) = chrono::DateTime::parse_from_str(created_at, "%Y-%m-%d %H:%M:%S %:z") {
            let now = chrono::Utc::now();
            let duration = now.signed_duration_since(dt);
            return Some(duration.num_hours().max(0) as u32);
        }

        // Try without timezone
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(created_at, "%Y-%m-%d %H:%M:%S") {
            let now = chrono::Utc::now().naive_utc();
            let duration = now.signed_duration_since(dt);
            return Some(duration.num_hours().max(0) as u32);
        }

        None
    }

    /// Compare two tasks by score for sorting (highest first).
    pub fn compare_by_score(
        &self,
        a_priority: u8,
        a_dependents: usize,
        a_age: Option<u32>,
        a_labels: &[String],
        b_priority: u8,
        b_dependents: usize,
        b_age: Option<u32>,
        b_labels: &[String],
    ) -> std::cmp::Ordering {
        let score_a = self.score(a_priority, a_dependents, a_age, a_labels);
        let score_b = self.score(b_priority, b_dependents, b_age, b_labels);

        // Higher score comes first
        score_b.cmp(&score_a)
    }
}

/// A scored bead ready for display or sorting.
#[derive(Debug, Clone)]
pub struct ScoredBead {
    /// The calculated score (0-100)
    pub score: u32,
    /// Breakdown of score components
    pub components: ScoreComponents,
}

/// Breakdown of individual score components.
#[derive(Debug, Clone, Default)]
pub struct ScoreComponents {
    /// Priority component score
    pub priority: u32,
    /// Blockers component score
    pub blockers: u32,
    /// Age component score
    pub age: u32,
    /// Labels component score
    pub labels: u32,
}

impl TaskScorer {
    /// Calculate a scored bead with component breakdown.
    pub fn score_with_components(
        &self,
        priority: u8,
        dependent_count: usize,
        age_hours: Option<u32>,
        labels: &[String],
    ) -> ScoredBead {
        let components = ScoreComponents {
            priority: self.calculate_priority_score(priority),
            blockers: self.calculate_blockers_score(dependent_count),
            age: self.calculate_age_score(age_hours),
            labels: self.calculate_labels_score(labels),
        };

        let weighted_score = (components.priority as f64 * self.config.priority_weight)
            + (components.blockers as f64 * self.config.blockers_weight)
            + (components.age as f64 * self.config.age_weight)
            + (components.labels as f64 * self.config.labels_weight);

        let score = weighted_score.round().min(100.0).max(0.0) as u32;

        ScoredBead { score, components }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_weights_sum_to_one() {
        let config = ScoringConfig::default();
        let sum = config.priority_weight
            + config.blockers_weight
            + config.age_weight
            + config.labels_weight;
        assert!((sum - 1.0).abs() < 0.01, "Weights should sum to 1.0");
    }

    #[test]
    fn test_config_validation() {
        let valid_config = ScoringConfig::default();
        assert!(valid_config.validate().is_ok());

        let invalid_config = ScoringConfig {
            priority_weight: 0.5,
            blockers_weight: 0.5,
            age_weight: 0.5,
            labels_weight: 0.5,
            ..Default::default()
        };
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_priority_score() {
        let scorer = TaskScorer::new();

        assert_eq!(scorer.calculate_priority_score(0), 40); // P0
        assert_eq!(scorer.calculate_priority_score(1), 32); // P1
        assert_eq!(scorer.calculate_priority_score(2), 24); // P2
        assert_eq!(scorer.calculate_priority_score(3), 16); // P3
        assert_eq!(scorer.calculate_priority_score(4), 8);  // P4
        assert_eq!(scorer.calculate_priority_score(99), 8); // Invalid
    }

    #[test]
    fn test_blockers_score() {
        let scorer = TaskScorer::new();

        assert_eq!(scorer.calculate_blockers_score(0), 0);
        assert_eq!(scorer.calculate_blockers_score(1), 10);
        assert_eq!(scorer.calculate_blockers_score(2), 20);
        assert_eq!(scorer.calculate_blockers_score(3), 30);
        assert_eq!(scorer.calculate_blockers_score(5), 30); // Capped
    }

    #[test]
    fn test_age_score() {
        let scorer = TaskScorer::new();

        assert_eq!(scorer.calculate_age_score(None), 0);
        assert_eq!(scorer.calculate_age_score(Some(0)), 0);
        assert_eq!(scorer.calculate_age_score(Some(5)), 5);
        assert_eq!(scorer.calculate_age_score(Some(20)), 20);
        assert_eq!(scorer.calculate_age_score(Some(100)), 20); // Capped
    }

    #[test]
    fn test_labels_score() {
        let scorer = TaskScorer::new();

        assert_eq!(scorer.calculate_labels_score(&[]), 0);
        assert_eq!(scorer.calculate_labels_score(&["critical".to_string()]), 10);
        assert_eq!(scorer.calculate_labels_score(&["urgent".to_string()]), 7);
        assert_eq!(scorer.calculate_labels_score(&["important".to_string()]), 4);
        assert_eq!(scorer.calculate_labels_score(&["other".to_string()]), 0);

        // Case insensitive
        assert_eq!(scorer.calculate_labels_score(&["CRITICAL".to_string()]), 10);
        assert_eq!(scorer.calculate_labels_score(&["Urgent".to_string()]), 7);

        // Max wins
        assert_eq!(
            scorer.calculate_labels_score(&["critical".to_string(), "urgent".to_string()]),
            10
        );
    }

    #[test]
    fn test_total_score_p0_wins_over_p2_with_same_bonuses() {
        let scorer = TaskScorer::new();

        // P0 with same bonuses as P2 should always win
        let p0_score = scorer.score(0, 2, Some(10), &["urgent".to_string()]);
        let p2_score = scorer.score(2, 2, Some(10), &["urgent".to_string()]);

        assert!(
            p0_score > p2_score,
            "P0 ({}) with same bonuses should be higher than P2 ({})",
            p0_score,
            p2_score
        );
    }

    #[test]
    fn test_total_score_p0_base_is_higher_than_p2_base() {
        let scorer = TaskScorer::new();

        // Base P0 score (no bonuses) should be higher than base P2 score (no bonuses)
        let p0_base = scorer.score(0, 0, None, &[]);
        let p2_base = scorer.score(2, 0, None, &[]);

        assert!(
            p0_base > p2_base,
            "P0 base ({}) should be higher than P2 base ({})",
            p0_base,
            p2_base
        );
    }

    #[test]
    fn test_total_score_blocker_boost() {
        let scorer = TaskScorer::new();

        let no_blockers = scorer.score(1, 0, Some(10), &[]);
        let three_blockers = scorer.score(1, 3, Some(10), &[]);

        assert!(
            three_blockers > no_blockers,
            "Task with blockers ({}) should score higher than without ({})",
            three_blockers,
            no_blockers
        );
    }

    #[test]
    fn test_total_score_age_boost() {
        let scorer = TaskScorer::new();

        let fresh = scorer.score(1, 0, Some(0), &[]);
        let old = scorer.score(1, 0, Some(20), &[]);

        assert!(
            old > fresh,
            "Old task ({}) should score higher than fresh ({})",
            old,
            fresh
        );
    }

    #[test]
    fn test_score_bounded() {
        let scorer = TaskScorer::new();

        // Max possible score
        let max_score = scorer.score(0, 3, Some(20), &["critical".to_string()]);
        assert!(max_score <= 100, "Score {} should be <= 100", max_score);

        // Min possible score
        let min_score = scorer.score(4, 0, None, &[]);
        assert!(min_score >= 0, "Score {} should be >= 0", min_score);
    }

    #[test]
    fn test_score_with_components() {
        let scorer = TaskScorer::new();

        let scored = scorer.score_with_components(0, 2, Some(10), &["critical".to_string()]);

        assert_eq!(scored.components.priority, 40);
        assert_eq!(scored.components.blockers, 20);
        assert_eq!(scored.components.age, 10);
        assert_eq!(scored.components.labels, 10);

        // Verify weighted sum
        let expected: f64 = (40.0 * 0.4) + (20.0 * 0.3) + (10.0 * 0.2) + (10.0 * 0.1);
        assert_eq!(scored.score, expected.round() as u32);
    }

    #[test]
    fn test_parse_age_hours() {
        // Valid ISO 8601
        let past = chrono::Utc::now() - chrono::Duration::hours(5);
        let past_str = past.to_rfc3339();
        assert_eq!(TaskScorer::parse_age_hours(&past_str), Some(5));

        // Invalid format
        assert_eq!(TaskScorer::parse_age_hours("invalid"), None);

        // Empty string
        assert_eq!(TaskScorer::parse_age_hours(""), None);
    }

    #[test]
    fn test_compare_by_score() {
        let scorer = TaskScorer::new();

        // P0 should come before P2
        let ordering = scorer.compare_by_score(
            0, 0, None, &[],    // P0
            2, 0, None, &[],    // P2
        );
        assert_eq!(ordering, std::cmp::Ordering::Less); // Less means a comes first

        // Higher score comes first
        let ordering = scorer.compare_by_score(
            1, 3, Some(20), &["critical".to_string()],  // High score
            3, 0, None, &[],                            // Low score
        );
        assert_eq!(ordering, std::cmp::Ordering::Less); // High score comes first
    }

    #[test]
    fn test_custom_config() {
        let config = ScoringConfig {
            priority_weight: 0.5,
            blockers_weight: 0.2,
            age_weight: 0.2,
            labels_weight: 0.1,
            max_age_hours: 50,
            max_blockers: 5,
        };

        // Verify config validation works
        assert!(config.validate().is_ok());

        let scorer = TaskScorer::with_config(config);

        // Note: The calculate_* methods have hard caps (MAX_AGE_POINTS, MAX_BLOCKERS_POINTS)
        // that apply regardless of config for consistency with the spec.

        // Age is capped at both max_age_hours AND MAX_AGE_POINTS (20)
        assert_eq!(scorer.calculate_age_score(Some(15)), 15);
        assert_eq!(scorer.calculate_age_score(Some(20)), 20); // MAX_AGE_POINTS
        assert_eq!(scorer.calculate_age_score(Some(40)), 20); // Capped at MAX_AGE_POINTS

        // Blockers is capped at both max_blockers*10 AND MAX_BLOCKERS_POINTS (30)
        assert_eq!(scorer.calculate_blockers_score(2), 20);
        assert_eq!(scorer.calculate_blockers_score(3), 30); // MAX_BLOCKERS_POINTS
        assert_eq!(scorer.calculate_blockers_score(5), 30); // Capped at MAX_BLOCKERS_POINTS
    }
}
