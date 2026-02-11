//! Tmux session discovery for workers.
//!
//! This module provides utilities for discovering active worker sessions from tmux,
//! parsing session names to extract worker types, and returning structured data
//! about active and idle workers.
//!
//! # Session Naming Convention
//!
//! Worker sessions follow the pattern: `<executor>-<suffix>`
//!
//! Where executor patterns are:
//! - `claude-code-glm-47` → GLM-4.7 model via z.ai proxy
//! - `claude-code-sonnet` → Claude Sonnet 4.5
//! - `claude-code-opus` → Claude Opus 4.5
//! - `claude-code-haiku` → Claude Haiku 4.5
//! - `opencode-glm-47` → OpenCode with GLM-4.7
//!
//! # Example
//!
//! ```no_run
//! use forge_worker::discovery::{discover_workers, WorkerType};
//!
//! #[tokio::main]
//! async fn main() -> forge_core::Result<()> {
//!     let result = discover_workers().await?;
//!
//!     for worker in &result.workers {
//!         println!("{}: {} ({})",
//!             worker.session_name,
//!             worker.worker_type,
//!             if worker.is_attached { "attached" } else { "detached" }
//!         );
//!     }
//!
//!     Ok(())
//! }
//! ```

use chrono::{DateTime, TimeZone, Utc};
use forge_core::{ForgeError, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use tokio::process::Command;
use tracing::{debug, instrument, warn};

/// Worker type based on the model/executor being used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerType {
    /// GLM-4.7 model via z.ai proxy (Claude Code or OpenCode)
    Glm47,
    /// Claude Sonnet 4.5
    Sonnet,
    /// Claude Opus 4.5/4.6
    Opus,
    /// Claude Haiku 4.5
    Haiku,
    /// Unknown/unrecognized worker type
    Unknown,
}

impl WorkerType {
    /// Parse a worker type from a session name.
    ///
    /// Matches against known executor patterns:
    /// - `claude-code-glm-47-*` or `opencode-glm-47-*` → Glm47
    /// - `claude-code-sonnet-*` → Sonnet
    /// - `claude-code-opus-*` → Opus
    /// - `claude-code-haiku-*` → Haiku
    pub fn from_session_name(name: &str) -> Self {
        if name.contains("glm-47") || name.contains("glm47") {
            Self::Glm47
        } else if name.contains("sonnet") {
            Self::Sonnet
        } else if name.contains("opus") {
            Self::Opus
        } else if name.contains("haiku") {
            Self::Haiku
        } else {
            Self::Unknown
        }
    }

    /// Get a short display name for the worker type.
    pub fn short_name(&self) -> &'static str {
        match self {
            Self::Glm47 => "GLM-4.7",
            Self::Sonnet => "Sonnet",
            Self::Opus => "Opus",
            Self::Haiku => "Haiku",
            Self::Unknown => "Unknown",
        }
    }

    /// Get the approximate cost tier (for display purposes).
    pub fn cost_tier(&self) -> &'static str {
        match self {
            Self::Opus => "high",
            Self::Sonnet => "medium",
            Self::Haiku | Self::Glm47 => "low",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for WorkerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.short_name())
    }
}

/// Information about a discovered worker session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredWorker {
    /// The tmux session name
    pub session_name: String,
    /// Parsed worker type from the session name
    pub worker_type: WorkerType,
    /// Unix timestamp when the session was created
    pub created_at: DateTime<Utc>,
    /// Unix timestamp of last activity in the session
    pub last_activity: DateTime<Utc>,
    /// Whether a client is currently attached to this session
    pub is_attached: bool,
    /// The worker suffix (e.g., "alpha", "bravo", "charlie")
    pub suffix: String,
    /// The executor prefix (e.g., "claude-code-glm-47", "opencode-glm-47")
    pub executor: String,
}

impl DiscoveredWorker {
    /// Check if this worker appears to be idle based on activity time.
    ///
    /// A worker is considered idle if there has been no activity for the given duration.
    pub fn is_idle(&self, idle_threshold_secs: i64) -> bool {
        let now = Utc::now();
        let inactive_secs = (now - self.last_activity).num_seconds();
        inactive_secs > idle_threshold_secs
    }

    /// Get the human-readable session age.
    pub fn age(&self) -> String {
        let now = Utc::now();
        let duration = now - self.created_at;

        if duration.num_days() > 0 {
            format!("{}d", duration.num_days())
        } else if duration.num_hours() > 0 {
            format!("{}h", duration.num_hours())
        } else if duration.num_minutes() > 0 {
            format!("{}m", duration.num_minutes())
        } else {
            format!("{}s", duration.num_seconds())
        }
    }
}

/// Result of a worker discovery operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoveryResult {
    /// All discovered worker sessions
    pub workers: Vec<DiscoveredWorker>,
    /// Count by worker type
    pub by_type: std::collections::HashMap<WorkerType, usize>,
    /// Total attached (active) sessions
    pub attached_count: usize,
    /// Total detached (potentially idle) sessions
    pub detached_count: usize,
}

impl DiscoveryResult {
    /// Get workers of a specific type.
    pub fn workers_of_type(&self, worker_type: WorkerType) -> Vec<&DiscoveredWorker> {
        self.workers
            .iter()
            .filter(|w| w.worker_type == worker_type)
            .collect()
    }

    /// Get attached (active) workers.
    pub fn attached_workers(&self) -> Vec<&DiscoveredWorker> {
        self.workers.iter().filter(|w| w.is_attached).collect()
    }

    /// Get detached workers.
    pub fn detached_workers(&self) -> Vec<&DiscoveredWorker> {
        self.workers.iter().filter(|w| !w.is_attached).collect()
    }

    /// Get idle workers based on activity threshold.
    pub fn idle_workers(&self, idle_threshold_secs: i64) -> Vec<&DiscoveredWorker> {
        self.workers
            .iter()
            .filter(|w| w.is_idle(idle_threshold_secs))
            .collect()
    }
}

/// Known worker session prefixes for filtering.
const WORKER_PREFIXES: &[&str] = &[
    "claude-code-glm-47-",
    "claude-code-sonnet-",
    "claude-code-opus-",
    "claude-code-haiku-",
    "opencode-glm-47-",
];

/// Parse a session line from tmux list-sessions output.
///
/// Expected format from: `tmux list-sessions -F '#{session_name}:#{session_created}:#{session_activity}:#{session_attached}'`
/// Example: `claude-code-glm-47-alpha:1770587037:1770587037:0`
fn parse_session_line(line: &str) -> Option<DiscoveredWorker> {
    let parts: Vec<&str> = line.split(':').collect();
    if parts.len() < 4 {
        warn!("Invalid session line format: {}", line);
        return None;
    }

    let session_name = parts[0].to_string();

    // Filter to only known worker sessions
    let is_worker = WORKER_PREFIXES.iter().any(|p| session_name.starts_with(p));
    if !is_worker {
        return None;
    }

    // Parse timestamps
    let created_ts: i64 = parts[1].parse().ok()?;
    let activity_ts: i64 = parts[2].parse().ok()?;
    let attached: i32 = parts[3].parse().ok()?;

    let created_at = Utc.timestamp_opt(created_ts, 0).single()?;
    let last_activity = Utc.timestamp_opt(activity_ts, 0).single()?;

    // Extract executor and suffix
    let (executor, suffix) = extract_executor_and_suffix(&session_name)?;
    let worker_type = WorkerType::from_session_name(&session_name);

    Some(DiscoveredWorker {
        session_name,
        worker_type,
        created_at,
        last_activity,
        is_attached: attached > 0,
        suffix,
        executor,
    })
}

/// Extract executor prefix and suffix from a session name.
///
/// Example: "claude-code-glm-47-alpha" → ("claude-code-glm-47", "alpha")
fn extract_executor_and_suffix(session_name: &str) -> Option<(String, String)> {
    for prefix in WORKER_PREFIXES {
        if session_name.starts_with(prefix) {
            let suffix = session_name.strip_prefix(prefix)?;
            // Remove trailing hyphen from prefix for clean executor name
            let executor = prefix.trim_end_matches('-').to_string();
            return Some((executor, suffix.to_string()));
        }
    }
    None
}

/// Discover all active worker sessions from tmux.
///
/// This queries `tmux list-sessions` and parses the output to find
/// worker sessions matching known patterns.
#[instrument(level = "debug")]
pub async fn discover_workers() -> Result<DiscoveryResult> {
    let output = Command::new("tmux")
        .args([
            "list-sessions",
            "-F",
            "#{session_name}:#{session_created}:#{session_activity}:#{session_attached}",
        ])
        .output()
        .await
        .map_err(|e| ForgeError::WorkerSpawn {
            worker_id: "discovery".into(),
            message: format!("Failed to list tmux sessions: {}", e),
        })?;

    if !output.status.success() {
        // No tmux server or no sessions is not an error
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("no server running") || stderr.contains("no sessions") {
            debug!("No tmux sessions found");
            return Ok(DiscoveryResult::default());
        }

        return Err(ForgeError::WorkerSpawn {
            worker_id: "discovery".into(),
            message: format!("tmux list-sessions failed: {}", stderr.trim()),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let workers: Vec<DiscoveredWorker> = stdout.lines().filter_map(parse_session_line).collect();

    // Build statistics
    let mut by_type = std::collections::HashMap::new();
    let mut attached_count = 0;
    let mut detached_count = 0;

    for worker in &workers {
        *by_type.entry(worker.worker_type).or_insert(0) += 1;
        if worker.is_attached {
            attached_count += 1;
        } else {
            detached_count += 1;
        }
    }

    debug!(
        "Discovered {} worker sessions ({} attached, {} detached)",
        workers.len(),
        attached_count,
        detached_count
    );

    Ok(DiscoveryResult {
        workers,
        by_type,
        attached_count,
        detached_count,
    })
}

/// Discover workers of a specific type.
#[instrument(level = "debug", skip_all, fields(worker_type = %worker_type))]
pub async fn discover_workers_by_type(worker_type: WorkerType) -> Result<Vec<DiscoveredWorker>> {
    let result = discover_workers().await?;
    Ok(result
        .workers
        .into_iter()
        .filter(|w| w.worker_type == worker_type)
        .collect())
}

/// Get a summary of discovered workers for display.
pub async fn discovery_summary() -> Result<String> {
    let result = discover_workers().await?;

    if result.workers.is_empty() {
        return Ok("No worker sessions found".to_string());
    }

    let mut summary = format!(
        "Workers: {} total ({} attached, {} detached)\n",
        result.workers.len(),
        result.attached_count,
        result.detached_count
    );

    for (worker_type, count) in &result.by_type {
        summary.push_str(&format!("  {}: {}\n", worker_type, count));
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_type_from_session_name() {
        assert_eq!(
            WorkerType::from_session_name("claude-code-glm-47-alpha"),
            WorkerType::Glm47
        );
        assert_eq!(
            WorkerType::from_session_name("opencode-glm-47-bravo"),
            WorkerType::Glm47
        );
        assert_eq!(
            WorkerType::from_session_name("claude-code-sonnet-test"),
            WorkerType::Sonnet
        );
        assert_eq!(
            WorkerType::from_session_name("claude-code-opus-main"),
            WorkerType::Opus
        );
        assert_eq!(
            WorkerType::from_session_name("claude-code-haiku-cheap"),
            WorkerType::Haiku
        );
        assert_eq!(
            WorkerType::from_session_name("random-session"),
            WorkerType::Unknown
        );
    }

    #[test]
    fn test_worker_type_display() {
        assert_eq!(WorkerType::Glm47.to_string(), "GLM-4.7");
        assert_eq!(WorkerType::Sonnet.to_string(), "Sonnet");
        assert_eq!(WorkerType::Opus.to_string(), "Opus");
        assert_eq!(WorkerType::Haiku.to_string(), "Haiku");
    }

    #[test]
    fn test_extract_executor_and_suffix() {
        let (exec, suffix) = extract_executor_and_suffix("claude-code-glm-47-alpha").unwrap();
        assert_eq!(exec, "claude-code-glm-47");
        assert_eq!(suffix, "alpha");

        let (exec, suffix) = extract_executor_and_suffix("claude-code-opus-bravo").unwrap();
        assert_eq!(exec, "claude-code-opus");
        assert_eq!(suffix, "bravo");

        let (exec, suffix) = extract_executor_and_suffix("opencode-glm-47-test").unwrap();
        assert_eq!(exec, "opencode-glm-47");
        assert_eq!(suffix, "test");

        assert!(extract_executor_and_suffix("random-session").is_none());
    }

    #[test]
    fn test_parse_session_line() {
        let line = "claude-code-glm-47-alpha:1770587037:1770587594:1";
        let worker = parse_session_line(line).unwrap();

        assert_eq!(worker.session_name, "claude-code-glm-47-alpha");
        assert_eq!(worker.worker_type, WorkerType::Glm47);
        assert!(worker.is_attached);
        assert_eq!(worker.suffix, "alpha");
        assert_eq!(worker.executor, "claude-code-glm-47");
    }

    #[test]
    fn test_parse_session_line_detached() {
        let line = "claude-code-opus-bravo:1770587143:1770587143:0";
        let worker = parse_session_line(line).unwrap();

        assert_eq!(worker.worker_type, WorkerType::Opus);
        assert!(!worker.is_attached);
    }

    #[test]
    fn test_parse_session_line_non_worker() {
        // Should return None for non-worker sessions
        let line = "alpha:1770470255:1770587594:1";
        assert!(parse_session_line(line).is_none());

        let line = "pool-optimizer:1770582005:1770582005:0";
        assert!(parse_session_line(line).is_none());
    }

    #[test]
    fn test_parse_session_line_invalid() {
        // Invalid format
        assert!(parse_session_line("invalid").is_none());
        assert!(parse_session_line("claude-code-glm-47-alpha:invalid:123:0").is_none());
    }

    #[test]
    fn test_discovered_worker_age() {
        let now = Utc::now();
        let worker = DiscoveredWorker {
            session_name: "test".into(),
            worker_type: WorkerType::Glm47,
            created_at: now - chrono::Duration::hours(2),
            last_activity: now,
            is_attached: false,
            suffix: "test".into(),
            executor: "claude-code-glm-47".into(),
        };

        assert!(worker.age().contains('h'));
    }

    #[test]
    fn test_discovered_worker_idle() {
        let now = Utc::now();

        // Active worker
        let active = DiscoveredWorker {
            session_name: "active".into(),
            worker_type: WorkerType::Glm47,
            created_at: now,
            last_activity: now,
            is_attached: false,
            suffix: "test".into(),
            executor: "claude-code-glm-47".into(),
        };
        assert!(!active.is_idle(300)); // 5 minute threshold

        // Idle worker
        let idle = DiscoveredWorker {
            session_name: "idle".into(),
            worker_type: WorkerType::Glm47,
            created_at: now - chrono::Duration::hours(1),
            last_activity: now - chrono::Duration::minutes(10),
            is_attached: false,
            suffix: "test".into(),
            executor: "claude-code-glm-47".into(),
        };
        assert!(idle.is_idle(300)); // 5 minute threshold
    }

    #[test]
    fn test_discovery_result_filters() {
        let now = Utc::now();
        let workers = vec![
            DiscoveredWorker {
                session_name: "claude-code-glm-47-alpha".into(),
                worker_type: WorkerType::Glm47,
                created_at: now,
                last_activity: now,
                is_attached: true,
                suffix: "alpha".into(),
                executor: "claude-code-glm-47".into(),
            },
            DiscoveredWorker {
                session_name: "claude-code-opus-bravo".into(),
                worker_type: WorkerType::Opus,
                created_at: now,
                last_activity: now - chrono::Duration::minutes(10),
                is_attached: false,
                suffix: "bravo".into(),
                executor: "claude-code-opus".into(),
            },
        ];

        let mut by_type = std::collections::HashMap::new();
        by_type.insert(WorkerType::Glm47, 1);
        by_type.insert(WorkerType::Opus, 1);

        let result = DiscoveryResult {
            workers,
            by_type,
            attached_count: 1,
            detached_count: 1,
        };

        assert_eq!(result.workers_of_type(WorkerType::Glm47).len(), 1);
        assert_eq!(result.workers_of_type(WorkerType::Opus).len(), 1);
        assert_eq!(result.workers_of_type(WorkerType::Sonnet).len(), 0);

        assert_eq!(result.attached_workers().len(), 1);
        assert_eq!(result.detached_workers().len(), 1);

        assert_eq!(result.idle_workers(300).len(), 1); // Only opus is idle
    }
}
