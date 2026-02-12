//! Shared type definitions used across FORGE crates.
//!
//! This module provides common types that are used by multiple FORGE components,
//! ensuring consistent representation across the codebase.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Unique identifier for a worker.
pub type WorkerId = String;

/// Unique identifier for a bead/task.
pub type BeadId = String;

/// Worker status as reported in status files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    /// Worker is running and processing tasks
    Active,
    /// Worker is running but idle (no current task)
    #[default]
    Idle,
    /// Worker process has failed or crashed
    Failed,
    /// Worker was intentionally stopped
    Stopped,
    /// Worker status file is corrupted or unreadable
    Error,
    /// Worker is starting up
    Starting,
}

impl WorkerStatus {
    /// Returns true if the worker is considered healthy.
    pub fn is_healthy(&self) -> bool {
        matches!(self, Self::Active | Self::Idle | Self::Starting)
    }

    /// Returns the status indicator emoji for TUI display.
    pub fn indicator(&self) -> &'static str {
        match self {
            Self::Active => "✅",
            Self::Idle => "💤",
            Self::Failed => "❌",
            Self::Stopped => "⏹️",
            Self::Error => "⚠️",
            Self::Starting => "🔄",
        }
    }
}

impl std::fmt::Display for WorkerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Idle => write!(f, "idle"),
            Self::Failed => write!(f, "failed"),
            Self::Stopped => write!(f, "stopped"),
            Self::Error => write!(f, "error"),
            Self::Starting => write!(f, "starting"),
        }
    }
}

/// Bead/task priority levels (P0 = critical, P4 = backlog).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum Priority {
    /// Critical priority (P0) - blocking, must be done immediately
    #[serde(rename = "0")]
    P0,
    /// High priority (P1) - important, should be done soon
    #[serde(rename = "1")]
    P1,
    /// Normal priority (P2) - standard work
    #[serde(rename = "2")]
    #[default]
    P2,
    /// Low priority (P3) - nice to have
    #[serde(rename = "3")]
    P3,
    /// Backlog (P4) - someday/maybe
    #[serde(rename = "4")]
    P4,
}

impl Priority {
    /// Returns the score contribution for task value scoring.
    ///
    /// Per ADR 0007: Priority contributes 0-40 points.
    pub fn score(&self) -> u32 {
        match self {
            Self::P0 => 40,
            Self::P1 => 30,
            Self::P2 => 20,
            Self::P3 => 10,
            Self::P4 => 5,
        }
    }

    /// Returns the worker tier recommended for this priority.
    pub fn recommended_tier(&self) -> WorkerTier {
        match self {
            Self::P0 | Self::P1 => WorkerTier::Premium,
            Self::P2 => WorkerTier::Standard,
            Self::P3 | Self::P4 => WorkerTier::Budget,
        }
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::P0 => write!(f, "P0"),
            Self::P1 => write!(f, "P1"),
            Self::P2 => write!(f, "P2"),
            Self::P3 => write!(f, "P3"),
            Self::P4 => write!(f, "P4"),
        }
    }
}

/// Worker tier for model routing.
///
/// Per ADR 0003, workers are grouped into tiers for cost optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerTier {
    /// Premium tier (Opus, GPT-4) - for critical/complex tasks
    Premium,
    /// Standard tier (Sonnet) - for most tasks
    Standard,
    /// Budget tier (Haiku) - for simple/low-priority tasks
    Budget,
}

impl std::fmt::Display for WorkerTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Premium => write!(f, "premium"),
            Self::Standard => write!(f, "standard"),
            Self::Budget => write!(f, "budget"),
        }
    }
}

/// Bead/task status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BeadStatus {
    /// Task is open and ready to be worked on
    #[default]
    Open,
    /// Task is currently being worked on
    InProgress,
    /// Task is completed
    Closed,
    /// Task is blocked by dependencies
    Blocked,
    /// Task is deferred to a later date
    Deferred,
}

impl std::fmt::Display for BeadStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Closed => write!(f, "closed"),
            Self::Blocked => write!(f, "blocked"),
            Self::Deferred => write!(f, "deferred"),
        }
    }
}

/// Type of bead/issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BeadType {
    /// Generic task
    #[default]
    Task,
    /// Bug fix
    Bug,
    /// New feature
    Feature,
    /// Epic (collection of related tasks)
    Epic,
    /// Research/investigation
    Research,
    /// Documentation
    Docs,
}

impl std::fmt::Display for BeadType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Task => write!(f, "task"),
            Self::Bug => write!(f, "bug"),
            Self::Feature => write!(f, "feature"),
            Self::Epic => write!(f, "epic"),
            Self::Research => write!(f, "research"),
            Self::Docs => write!(f, "docs"),
        }
    }
}

/// Model identifier with tier classification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model identifier (e.g., "sonnet", "opus", "haiku")
    pub id: String,
    /// Display name
    pub name: String,
    /// Worker tier for this model
    pub tier: WorkerTier,
    /// Cost per million input tokens in USD
    pub cost_per_million_input: f64,
    /// Cost per million output tokens in USD
    pub cost_per_million_output: f64,
}

impl ModelInfo {
    /// Create a new model info with default costs.
    pub fn new(id: impl Into<String>, name: impl Into<String>, tier: WorkerTier) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            tier,
            cost_per_million_input: 0.0,
            cost_per_million_output: 0.0,
        }
    }

    /// Set input/output costs and return self for chaining.
    pub fn with_costs(mut self, input: f64, output: f64) -> Self {
        self.cost_per_million_input = input;
        self.cost_per_million_output = output;
        self
    }
}

/// Timestamp type used throughout FORGE.
pub type Timestamp = DateTime<Utc>;

/// Get the current UTC timestamp.
pub fn now() -> Timestamp {
    Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_status_healthy() {
        assert!(WorkerStatus::Active.is_healthy());
        assert!(WorkerStatus::Idle.is_healthy());
        assert!(!WorkerStatus::Failed.is_healthy());
    }

    #[test]
    fn test_priority_score() {
        assert_eq!(Priority::P0.score(), 40);
        assert_eq!(Priority::P2.score(), 20);
        assert_eq!(Priority::P4.score(), 5);
    }

    #[test]
    fn test_priority_tier() {
        assert_eq!(Priority::P0.recommended_tier(), WorkerTier::Premium);
        assert_eq!(Priority::P2.recommended_tier(), WorkerTier::Standard);
        assert_eq!(Priority::P4.recommended_tier(), WorkerTier::Budget);
    }

    #[test]
    fn test_status_display() {
        assert_eq!(WorkerStatus::Active.to_string(), "active");
        assert_eq!(Priority::P0.to_string(), "P0");
        assert_eq!(BeadStatus::InProgress.to_string(), "in_progress");
    }

    #[test]
    fn test_model_info() {
        let model = ModelInfo::new("sonnet", "Claude Sonnet 4.5", WorkerTier::Standard)
            .with_costs(3.0, 15.0);

        assert_eq!(model.id, "sonnet");
        assert_eq!(model.tier, WorkerTier::Standard);
        assert_eq!(model.cost_per_million_input, 3.0);
    }
}
