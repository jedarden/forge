//! Worker performance tracking for time-series metrics extraction.
//!
//! This module provides:
//! - [`WorkerPerfTracker`] - Track task completion times and error rates
//! - [`TaskPerfMetrics`] - Per-task performance metrics
//! - [`WorkerPerfSummary`] - Aggregated worker performance over time windows
//!
//! ## Example
//!
//! ```no_run
//! use forge_core::worker_perf::{WorkerPerfTracker, TaskEvent};
//!
//! fn main() {
//!     let mut tracker = WorkerPerfTracker::new();
//!
//!     // Record task start
//!     tracker.record_task_start("bd-2d97", "worker-1", "claude-opus");
//!
//!     // Later... record task completion
//!     tracker.record_task_complete("bd-2d97", true, 150_000, 50000, 0.025);
//!
//!     // Get performance summary
//!     let summary = tracker.worker_summary("worker-1");
//!     println!("Tasks: {}, Errors: {:.2}",
//!         summary.tasks_completed,
//!         summary.error_rate * 100.0);
//! }
//! ```

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::time::Duration;

/// Performance metrics for a single task.
#[derive(Debug, Clone)]
pub struct TaskPerfMetrics {
    /// Task/bead ID
    pub task_id: String,

    /// Worker that executed the task
    pub worker_id: String,

    /// Model used for execution
    pub model: String,

    /// Task start timestamp
    pub start_time: DateTime<Utc>,

    /// Task end timestamp
    pub end_time: Option<DateTime<Utc>>,

    /// Duration in milliseconds
    pub duration_ms: Option<u64>,

    /// Whether the task succeeded
    pub success: bool,

    /// Input tokens consumed
    pub input_tokens: i64,

    /// Output tokens consumed
    pub output_tokens: i64,

    /// Total cost in USD
    pub cost_usd: f64,
}

impl TaskPerfMetrics {
    /// Create new task metrics.
    pub fn new(task_id: impl Into<String>, worker_id: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            worker_id: worker_id.into(),
            model: String::new(),
            start_time: Utc::now(),
            end_time: None,
            duration_ms: None,
            success: false,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: 0.0,
        }
    }

    /// Set model used.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Mark task as completed.
    pub fn completed(mut self, success: bool, input_tokens: i64, output_tokens: i64, cost_usd: f64) -> Self {
        let end_time = Utc::now();
        self.end_time = Some(end_time);
        self.success = success;
        self.input_tokens = input_tokens;
        self.output_tokens = output_tokens;
        self.cost_usd = cost_usd;

        let duration = end_time.signed_duration_since(self.start_time);
        self.duration_ms = Some(duration.num_milliseconds().max(0) as u64);

        self
    }

    /// Check if task is still running.
    pub fn is_running(&self) -> bool {
        self.end_time.is_none()
    }

    /// Get total tokens.
    pub fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens
    }

    /// Get duration as Duration.
    pub fn duration(&self) -> Option<Duration> {
        self.duration_ms.map(|ms| Duration::from_millis(ms))
    }
}

/// Event types for task lifecycle tracking.
#[derive(Debug, Clone)]
pub enum TaskEvent {
    /// Task started execution
    Started {
        task_id: String,
        worker_id: String,
        model: String,
    },

    /// Task completed (success or failure)
    Completed {
        task_id: String,
        worker_id: String,
        success: bool,
        duration_ms: u64,
        input_tokens: i64,
        output_tokens: i64,
        cost_usd: f64,
    },

    /// Task failed with error
    Failed {
        task_id: String,
        worker_id: String,
        error: String,
    },
}

/// Aggregated performance summary for a worker.
#[derive(Debug, Clone)]
pub struct WorkerPerfSummary {
    /// Worker identifier
    pub worker_id: String,

    /// Total tasks completed
    pub tasks_completed: i64,

    /// Total tasks failed
    pub tasks_failed: i64,

    /// Total tasks (completed + failed)
    pub total_tasks: i64,

    /// Success rate (0.0 - 1.0)
    pub success_rate: f64,

    /// Average task duration in milliseconds
    pub avg_duration_ms: f64,

    /// P95 task duration in milliseconds
    pub p95_duration_ms: f64,

    /// Total input tokens
    pub total_input_tokens: i64,

    /// Total output tokens
    pub total_output_tokens: i64,

    /// Total cost in USD
    pub total_cost_usd: f64,

    /// Average cost per task
    pub avg_cost_per_task: f64,

    /// Tokens per hour throughput
    pub tokens_per_hour: f64,

    /// Tasks per hour throughput
    pub tasks_per_hour: f64,

    /// Total active time in seconds
    pub active_time_secs: i64,

    /// Last update timestamp
    pub last_updated: DateTime<Utc>,
}

impl WorkerPerfSummary {
    /// Calculate error rate (0.0 - 1.0).
    pub fn error_rate(&self) -> f64 {
        if self.total_tasks == 0 {
            0.0
        } else {
            self.tasks_failed as f64 / self.total_tasks as f64
        }
    }

    /// Get total tokens.
    pub fn total_tokens(&self) -> i64 {
        self.total_input_tokens + self.total_output_tokens
    }
}

/// Tracks performance metrics for workers over time.
#[derive(Debug)]
pub struct WorkerPerfTracker {
    /// Active tasks being tracked
    active_tasks: HashMap<String, TaskPerfMetrics>,

    /// Completed task history per worker
    completed_tasks: HashMap<String, Vec<TaskPerfMetrics>>,

    /// Per-worker aggregated summaries
    worker_summaries: HashMap<String, WorkerPerfSummary>,

    /// Maximum history size per worker
    max_history: usize,

    /// Track active time per worker (for tokens/hour calculation)
    worker_active_time_ms: HashMap<String, u64>,
}

impl WorkerPerfTracker {
    /// Create new performance tracker.
    pub fn new() -> Self {
        Self {
            active_tasks: HashMap::new(),
            completed_tasks: HashMap::new(),
            worker_summaries: HashMap::new(),
            max_history: 1000, // Keep last 1000 tasks per worker
            worker_active_time_ms: HashMap::new(),
        }
    }

    /// Create tracker with custom history limit.
    pub fn with_history(max_history: usize) -> Self {
        Self {
            active_tasks: HashMap::new(),
            completed_tasks: HashMap::new(),
            worker_summaries: HashMap::new(),
            max_history,
            worker_active_time_ms: HashMap::new(),
        }
    }

    /// Get total tokens across all completed tasks.
    pub fn total_tokens(&self) -> i64 {
        self.completed_tasks.values().flatten().map(|t| t.total_tokens()).sum()
    }
}
