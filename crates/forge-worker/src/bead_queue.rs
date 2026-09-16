//! Bead queue reading and management.
//!
//! This module provides functionality for reading bead queues from workspaces
//! and managing bead allocation to workers. It extends the existing bead module
//! with queue-specific operations for launcher integration.
//!
//! Reads go through [`forge_core::bead_store`], which understands both the
//! current bead-rs checkpoint layout and the legacy flat `issues.jsonl`
//! format. Bead mutations stay with the `bead` CLI (ADR 0007/0020); this
//! module never writes to the store.

use crate::complexity::{ComplexityScorer, TaskContext};
use crate::scorer::{ScoredBead, TaskScorer};
use crate::types::{LaunchConfig, SpawnRequest};
use forge_core::bead_store::{self, StoreBead};
use forge_core::types::BeadId;
use forge_core::{ForgeError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info};

/// Bead queue reader for parsing a workspace's bead store.
#[derive(Debug)]
pub struct BeadQueueReader {
    /// Workspace path
    workspace: PathBuf,
    /// Cached ready beads
    ready_cache: Vec<QueuedBead>,
    /// Bead assignment tracking (bead_id -> worker_id)
    assignments: HashMap<BeadId, String>,
}

/// A bead from the queue with allocation metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedBead {
    /// Unique bead identifier
    pub id: BeadId,
    /// Title of the bead
    pub title: String,
    /// Description of the bead
    pub description: String,
    /// Current status (open, in_progress, closed, blocked, deferred)
    pub status: String,
    /// Priority (0-4, where 0 is critical)
    pub priority: u8,
    /// Issue type (task, bug, feature, etc.)
    pub issue_type: String,
    /// Labels
    pub labels: Vec<String>,
    /// Number of dependencies this bead is blocked by
    pub dependency_count: usize,
    /// Number of beads that depend on this one (for scoring)
    #[serde(default)]
    pub dependent_count: usize,
    /// Creation timestamp (ISO 8601 format)
    #[serde(default)]
    pub created_at: Option<String>,
    /// Whether this bead is ready to work on
    pub is_ready: bool,
    /// Workspace path
    pub workspace: PathBuf,
}

impl QueuedBead {
    /// Check if this bead is ready to be allocated.
    pub fn is_allocatable(&self) -> bool {
        self.is_ready && self.status == "open"
    }

    /// Get the priority score for sorting (P0=40, P1=30, etc.).
    pub fn priority_score(&self) -> u32 {
        match self.priority {
            0 => 40,
            1 => 30,
            2 => 20,
            3 => 10,
            _ => 5,
        }
    }

    /// Calculate the full task value score using the TaskScorer.
    ///
    /// This computes a score from 0-100 based on:
    /// - Priority (40% weight): P0=40, P1=32, P2=24, P3=16, P4=8
    /// - Blockers (30% weight): 10 points per blocked task, max 30
    /// - Age (20% weight): 1 point per hour, max 20
    /// - Labels (10% weight): critical=10, urgent=7, important=4
    pub fn calculate_score(&self, scorer: &TaskScorer) -> ScoredBead {
        let age_hours = self
            .created_at
            .as_ref()
            .and_then(|s| TaskScorer::parse_age_hours(s));

        scorer.score_with_components(self.priority, self.dependent_count, age_hours, &self.labels)
    }

    /// Calculate score using default scorer.
    pub fn score(&self) -> u32 {
        let scorer = TaskScorer::new();
        self.calculate_score(&scorer).score
    }

    /// Get the display string for this bead.
    pub fn display(&self) -> String {
        format!("{} [P{}] {}", self.id, self.priority, self.title)
    }

    /// Get the display string with score.
    pub fn display_with_score(&self) -> String {
        let score = self.score();
        format!(
            "{} [P{}] [Score:{}] {}",
            self.id, self.priority, score, self.title
        )
    }
}

/// Bead allocation request for spawning a worker.
#[derive(Debug, Clone)]
pub struct BeadAllocation {
    /// Bead to allocate
    pub bead_id: BeadId,
    /// Worker ID to allocate to
    pub worker_id: String,
    /// Launch configuration for the worker
    pub config: LaunchConfig,
}

impl BeadQueueReader {
    /// Create a new bead queue reader for a workspace.
    pub fn new(workspace: impl Into<PathBuf>) -> Result<Self> {
        let workspace = workspace.into();

        // Verify workspace exists
        if !workspace.exists() {
            return Err(ForgeError::WorkspaceNotFound { path: workspace });
        }

        Ok(Self {
            workspace,
            ready_cache: Vec::new(),
            assignments: HashMap::new(),
        })
    }

    /// Check if this workspace has a beads database.
    pub fn has_beads(&self) -> bool {
        bead_store::detect_format(&self.workspace).is_some()
    }

    /// Read beads from the workspace's bead store.
    pub fn read_beads(&mut self) -> Result<Vec<QueuedBead>> {
        if !self.has_beads() {
            debug!("No bead store found in {:?}", self.workspace);
            return Ok(Vec::new());
        }

        let store_beads = bead_store::read_all_beads(&self.workspace)?;
        let index = bead_store::build_index(&store_beads);

        let beads: Vec<QueuedBead> = store_beads
            .iter()
            .map(|bead| {
                // Only `blocks` edges gate readiness; `related` edges don't.
                let dependency_count = bead.blocking_dependency_ids().len();
                let dependent_count = bead_store::count_dependents(&store_beads, &bead.id);
                let is_ready = bead_store::is_ready(bead, &index);
                self.to_queued_bead(bead, dependency_count, dependent_count, is_ready)
            })
            .collect();

        info!("Read {} beads from {:?}", beads.len(), self.workspace);
        Ok(beads)
    }

    /// Convert a normalized store bead into a queue entry.
    fn to_queued_bead(
        &self,
        bead: &StoreBead,
        dependency_count: usize,
        dependent_count: usize,
        is_ready: bool,
    ) -> QueuedBead {
        QueuedBead {
            id: bead.id.clone(),
            title: bead.title.clone(),
            description: bead.description.clone(),
            status: bead.status.clone(),
            priority: bead.priority,
            issue_type: if bead.issue_type.is_empty() {
                "task".to_string()
            } else {
                bead.issue_type.clone()
            },
            labels: bead.labels.clone(),
            dependency_count,
            dependent_count,
            created_at: bead.created_at.clone(),
            is_ready,
            workspace: self.workspace.clone(),
        }
    }

    /// Get ready beads, sorted by score (highest first).
    ///
    /// Uses the TaskScorer to calculate scores based on priority,
    /// blockers, age, and labels. Higher-scored tasks appear first.
    pub fn get_ready_beads(&mut self) -> Result<Vec<QueuedBead>> {
        let beads = self.read_beads()?;
        let ready: Vec<_> = beads.into_iter().filter(|b| b.is_allocatable()).collect();

        // Sort by score (highest first), then by priority, then by id for stability
        let scorer = TaskScorer::new();
        let mut sorted = ready;
        sorted.sort_by(|a, b| {
            let score_a = a.calculate_score(&scorer).score;
            let score_b = b.calculate_score(&scorer).score;

            score_b
                .cmp(&score_a)
                .then_with(|| a.priority.cmp(&b.priority))
                .then_with(|| a.id.cmp(&b.id))
        });

        self.ready_cache = sorted.clone();
        Ok(sorted)
    }

    /// Get the next ready bead for allocation.
    ///
    /// Returns the highest-scoring ready bead that is not already assigned,
    /// or `None` when the queue is empty or fully assigned.
    pub fn pop_ready_bead(&mut self) -> Option<QueuedBead> {
        if let Ok(mut ready) = self.get_ready_beads() {
            // Filter out already assigned beads
            ready.retain(|b| !self.assignments.contains_key(&b.id));

            // `get_ready_beads` sorts highest-score first, so the next bead to
            // allocate is at the front of the list.
            ready.into_iter().next()
        } else {
            None
        }
    }

    /// Fetch a single bead by ID, regardless of readiness or assignment state.
    ///
    /// This is the context-fetch step of the bead-aware launcher pipeline: it
    /// returns everything needed to build a task prompt for the bead.
    pub fn get_bead(&mut self, bead_id: &BeadId) -> Result<Option<QueuedBead>> {
        Ok(self.read_beads()?.into_iter().find(|b| &b.id == bead_id))
    }

    /// Assign a bead to a worker.
    ///
    /// Assignment is the bead-level lock that prevents duplicate work: a bead
    /// already assigned to a different worker is rejected with
    /// [`ForgeError::BeadAlreadyAssigned`]. Re-assigning the same worker is
    /// idempotent.
    pub fn assign_bead(&mut self, bead_id: BeadId, worker_id: String) -> Result<()> {
        if let Some(existing) = self.assignments.get(&bead_id) {
            if *existing != worker_id {
                return Err(ForgeError::BeadAlreadyAssigned {
                    bead_id,
                    worker_id: existing.clone(),
                });
            }
            debug!("Bead {} already assigned to {}", bead_id, worker_id);
            return Ok(());
        }

        info!("Assigning bead {} to worker {}", bead_id, worker_id);
        self.assignments.insert(bead_id, worker_id);
        Ok(())
    }

    /// Check if a bead is assigned.
    pub fn is_assigned(&self, bead_id: &BeadId) -> bool {
        self.assignments.contains_key(bead_id)
    }

    /// Get the worker assigned to a bead.
    pub fn get_assigned_worker(&self, bead_id: &BeadId) -> Option<&String> {
        self.assignments.get(bead_id)
    }

    /// Remove a bead assignment (e.g., when worker completes or fails).
    pub fn unassign_bead(&mut self, bead_id: &BeadId) -> Option<String> {
        info!("Unassigning bead {}", bead_id);
        self.assignments.remove(bead_id)
    }

    /// Get all current assignments.
    pub fn get_assignments(&self) -> &HashMap<BeadId, String> {
        &self.assignments
    }

    /// Create a spawn request for a bead allocation.
    pub fn create_spawn_request(&self, bead: &QueuedBead, config: LaunchConfig) -> SpawnRequest {
        let worker_id = format!("forge-{}-{}", bead.id, config.model);

        // Capture the scorer's prediction without changing the caller-selected
        // launch tier or model. The launcher persists this record immediately
        // before it validates and executes the launcher script.
        let mut context = TaskContext::new(&bead.title)
            .with_description(&bead.description)
            .with_labels(bead.labels.clone())
            .with_blocks(bead.dependent_count);
        if bead.issue_type.eq_ignore_ascii_case("bug") {
            context = context.as_bug();
        } else if bead.issue_type.eq_ignore_ascii_case("feature") {
            context = context.as_feature();
        }
        let scorer = forge_config::ForgeConfig::load()
            .map(|config| ComplexityScorer::from_forge_config(&config))
            .unwrap_or_default();
        let complexity = scorer.score(&context);
        let assignment =
            complexity.task_assignment_with_config(&bead.id, &config.model, scorer.config());

        let config = config.with_bead(bead.id.clone());

        SpawnRequest::new(worker_id, config).with_task_assignment(assignment)
    }
}

/// Multi-workspace bead queue manager.
#[derive(Debug)]
pub struct BeadQueueManager {
    /// Individual workspace readers
    readers: Vec<BeadQueueReader>,
}

impl BeadQueueManager {
    /// Create a new bead queue manager.
    pub fn new() -> Self {
        Self {
            readers: Vec::new(),
        }
    }

    /// Add a workspace to monitor.
    pub fn add_workspace(&mut self, workspace: impl Into<PathBuf>) -> Result<()> {
        let reader = BeadQueueReader::new(workspace.into())?;
        self.readers.push(reader);
        Ok(())
    }

    /// Get the next ready bead from all workspaces.
    ///
    /// Returns the bead with the highest score across all workspaces.
    pub fn pop_next_ready(&mut self) -> Option<(BeadId, QueuedBead, PathBuf)> {
        let mut candidates = Vec::new();
        let scorer = TaskScorer::new();

        for reader in &mut self.readers {
            if let Some(bead) = reader.pop_ready_bead() {
                candidates.push((bead.id.clone(), bead, reader.workspace.clone()));
            }
        }

        // Sort by score across all workspaces (highest first)
        candidates.sort_by(|a, b| {
            let score_a = a.1.calculate_score(&scorer).score;
            let score_b = b.1.calculate_score(&scorer).score;
            score_b.cmp(&score_a)
        });

        candidates.into_iter().next()
    }

    /// Get all ready beads across all workspaces.
    ///
    /// Returns beads sorted by score (highest first).
    pub fn get_all_ready(&mut self) -> Vec<(BeadId, QueuedBead, PathBuf)> {
        let mut ready = Vec::new();
        let scorer = TaskScorer::new();

        for reader in &mut self.readers {
            if let Ok(beads) = reader.get_ready_beads() {
                for bead in beads {
                    ready.push((bead.id.clone(), bead, reader.workspace.clone()));
                }
            }
        }

        // Sort by score (highest first)
        ready.sort_by(|a, b| {
            let score_a = a.1.calculate_score(&scorer).score;
            let score_b = b.1.calculate_score(&scorer).score;
            score_b.cmp(&score_a)
        });

        ready
    }

    /// Assign a bead to a worker.
    pub fn assign_bead(&mut self, bead_id: &BeadId, worker_id: String) -> Result<()> {
        for reader in &mut self.readers {
            if reader.has_beads()
                && let Ok(beads) = reader.read_beads()
                && beads.iter().any(|b| &b.id == bead_id)
            {
                return reader.assign_bead(bead_id.clone(), worker_id);
            }
        }
        Err(ForgeError::BeadNotFound {
            bead_id: bead_id.clone(),
        })
    }

    /// Get the number of monitored workspaces.
    pub fn workspace_count(&self) -> usize {
        self.readers.len()
    }
}

impl Default for BeadQueueManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_workspace() -> TempDir {
        let dir = TempDir::new().unwrap();
        let beads_dir = dir.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();

        // Create a test issues.jsonl file
        let issues_file = beads_dir.join("issues.jsonl");
        let mut file = fs::File::create(issues_file).unwrap();

        writeln!(file, r#"{{"id":"test-1","title":"Test bead","description":"A test","status":"open","priority":0,"issue_type":"task","labels":[],"dependencies":[]}}"#).unwrap();
        writeln!(file, r#"{{"id":"test-2","title":"Blocked bead","description":"Blocked","status":"open","priority":1,"issue_type":"task","labels":[],"dependencies":["test-1"]}}"#).unwrap();

        dir
    }

    #[test]
    fn test_bead_queue_reader_creation() {
        let dir = create_test_workspace();
        let reader = BeadQueueReader::new(dir.path()).unwrap();
        assert!(reader.has_beads());
    }

    #[test]
    fn test_read_beads() {
        let dir = create_test_workspace();
        let mut reader = BeadQueueReader::new(dir.path()).unwrap();
        let beads = reader.read_beads().unwrap();

        assert_eq!(beads.len(), 2);
        assert_eq!(beads[0].id, "test-1");
        assert_eq!(beads[1].id, "test-2");
    }

    #[test]
    fn test_ready_beads_filtering() {
        let dir = create_test_workspace();
        let mut reader = BeadQueueReader::new(dir.path()).unwrap();
        let ready = reader.get_ready_beads().unwrap();

        // Only test-1 should be ready (test-2 has a dependency)
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "test-1");
    }

    #[test]
    fn test_bead_allocation() {
        let dir = create_test_workspace();
        let mut reader = BeadQueueReader::new(dir.path()).unwrap();

        reader
            .assign_bead("test-1".to_string(), "worker-1".to_string())
            .unwrap();
        assert!(reader.is_assigned(&"test-1".to_string()));
        assert_eq!(
            reader.get_assigned_worker(&"test-1".to_string()),
            Some(&"worker-1".to_string())
        );
    }

    #[test]
    fn test_bead_unassignment() {
        let dir = create_test_workspace();
        let mut reader = BeadQueueReader::new(dir.path()).unwrap();

        reader
            .assign_bead("test-1".to_string(), "worker-1".to_string())
            .unwrap();
        let worker = reader.unassign_bead(&"test-1".to_string());
        assert_eq!(worker, Some("worker-1".to_string()));
        assert!(!reader.is_assigned(&"test-1".to_string()));
    }

    #[test]
    fn test_priority_score() {
        let dir = create_test_workspace();
        let mut reader = BeadQueueReader::new(dir.path()).unwrap();
        let beads = reader.read_beads().unwrap();

        assert_eq!(beads[0].priority_score(), 40); // P0 = 40
        assert_eq!(beads[1].priority_score(), 30); // P1 = 30
    }

    #[test]
    fn test_queue_manager() {
        let dir1 = create_test_workspace();
        let dir2 = create_test_workspace();

        let mut manager = BeadQueueManager::new();
        manager.add_workspace(dir1.path()).unwrap();
        manager.add_workspace(dir2.path()).unwrap();

        assert_eq!(manager.workspace_count(), 2);
    }

    #[test]
    fn test_pop_next_ready() {
        let dir = create_test_workspace();
        let mut manager = BeadQueueManager::new();
        manager.add_workspace(dir.path()).unwrap();

        let bead = manager.pop_next_ready();
        assert!(bead.is_some());
        assert_eq!(bead.unwrap().0, "test-1");
    }

    #[test]
    fn test_spawn_request_includes_complexity_prediction() {
        let dir = create_test_workspace();
        let mut reader = BeadQueueReader::new(dir.path()).unwrap();
        let bead = reader.get_ready_beads().unwrap().pop().unwrap();
        let config = LaunchConfig::new(
            "/path/to/launcher.sh",
            "test-session",
            dir.path().to_path_buf(),
            "claude-opus",
        );

        let request = reader.create_spawn_request(&bead, config);
        let assignment = request.task_assignment.unwrap();

        assert_eq!(assignment.bead_id, "test-1");
        assert_eq!(assignment.assigned_model, "claude-opus");
        assert_eq!(request.config.bead_id.as_deref(), Some("test-1"));
        // The prediction reflects the bead's real dependent count: test-2
        // declares a blocks edge against test-1, so it is 1, not 0.
        assert_eq!(bead.dependent_count, 1);
        assert_eq!(
            assignment.predicted_score,
            ComplexityScorer::new()
                .score(
                    &TaskContext::new("Test bead")
                        .with_description("A test")
                        .with_labels(Vec::new())
                        .with_blocks(bead.dependent_count),
                )
                .score
        );
    }

    #[test]
    fn test_bead_score_calculation() {
        let dir = create_test_workspace();
        let mut reader = BeadQueueReader::new(dir.path()).unwrap();
        let beads = reader.read_beads().unwrap();

        // P0 should have higher score than P1
        let p0_score = beads[0].score();
        let p1_score = beads[1].score();
        assert!(
            p0_score > p1_score,
            "P0 score ({}) should be > P1 score ({})",
            p0_score,
            p1_score
        );
    }

    #[test]
    fn test_bead_with_dependents_scores_higher() {
        let scorer = TaskScorer::new();

        // Create beads with different dependent counts
        let bead_no_deps = QueuedBead {
            id: "test-a".to_string(),
            title: "No deps".to_string(),
            description: String::new(),
            status: "open".to_string(),
            priority: 1,
            issue_type: "task".to_string(),
            labels: vec![],
            dependency_count: 0,
            dependent_count: 0,
            created_at: None,
            is_ready: true,
            workspace: PathBuf::from("/test"),
        };

        let bead_with_deps = QueuedBead {
            id: "test-b".to_string(),
            title: "Has deps".to_string(),
            description: String::new(),
            status: "open".to_string(),
            priority: 1,
            issue_type: "task".to_string(),
            labels: vec![],
            dependency_count: 0,
            dependent_count: 3,
            created_at: None,
            is_ready: true,
            workspace: PathBuf::from("/test"),
        };

        let score_no_deps = bead_no_deps.calculate_score(&scorer).score;
        let score_with_deps = bead_with_deps.calculate_score(&scorer).score;

        assert!(
            score_with_deps > score_no_deps,
            "Bead with dependents ({}) should score > without ({})",
            score_with_deps,
            score_no_deps
        );
    }

    #[test]
    fn test_p0_scores_higher_than_p2_with_same_bonuses() {
        let scorer = TaskScorer::new();

        // P0 with no bonuses
        let p0_bead = QueuedBead {
            id: "p0".to_string(),
            title: "Critical".to_string(),
            description: String::new(),
            status: "open".to_string(),
            priority: 0,
            issue_type: "task".to_string(),
            labels: vec![],
            dependency_count: 0,
            dependent_count: 2,
            created_at: None,
            is_ready: true,
            workspace: PathBuf::from("/test"),
        };

        // P2 with same bonuses
        let p2_bead = QueuedBead {
            id: "p2".to_string(),
            title: "Medium".to_string(),
            description: String::new(),
            status: "open".to_string(),
            priority: 2,
            issue_type: "task".to_string(),
            labels: vec![],
            dependency_count: 0,
            dependent_count: 2,
            created_at: None,
            is_ready: true,
            workspace: PathBuf::from("/test"),
        };

        let p0_score = p0_bead.calculate_score(&scorer).score;
        let p2_score = p2_bead.calculate_score(&scorer).score;

        assert!(
            p0_score > p2_score,
            "P0 ({}) with same bonuses should score higher than P2 ({})",
            p0_score,
            p2_score
        );
    }

    #[test]
    fn test_display_with_score() {
        let bead = QueuedBead {
            id: "fg-123".to_string(),
            title: "Test task".to_string(),
            description: String::new(),
            status: "open".to_string(),
            priority: 0,
            issue_type: "task".to_string(),
            labels: vec![],
            dependency_count: 0,
            dependent_count: 0,
            created_at: None,
            is_ready: true,
            workspace: PathBuf::from("/test"),
        };

        let display = bead.display_with_score();
        assert!(display.contains("fg-123"));
        assert!(display.contains("[P0]"));
        assert!(display.contains("[Score:"));
        assert!(display.contains("Test task"));
    }

    /// Workspace with two ready beads at different priorities plus a blocked one.
    fn create_multi_priority_workspace() -> TempDir {
        let dir = TempDir::new().unwrap();
        let beads_dir = dir.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();

        let mut file = fs::File::create(beads_dir.join("issues.jsonl")).unwrap();
        writeln!(file, r#"{{"id":"p-low","title":"Low priority","description":"","status":"open","priority":3,"issue_type":"task","labels":[],"dependencies":[]}}"#).unwrap();
        writeln!(file, r#"{{"id":"p-high","title":"High priority","description":"","status":"open","priority":0,"issue_type":"bug","labels":[],"dependencies":[]}}"#).unwrap();
        writeln!(file, r#"{{"id":"p-blocked","title":"Blocked","description":"","status":"open","priority":0,"issue_type":"task","labels":[],"dependencies":["p-high"]}}"#).unwrap();

        dir
    }

    #[test]
    fn test_pop_ready_bead_returns_highest_priority() {
        let dir = create_multi_priority_workspace();
        let mut reader = BeadQueueReader::new(dir.path()).unwrap();

        let bead = reader.pop_ready_bead().expect("queue should not be empty");
        assert_eq!(bead.id, "p-high");
        // Popping hands the bead out; the assignment is what consumes it.
        reader
            .assign_bead("p-high".to_string(), "worker-1".to_string())
            .unwrap();

        // The P3 bead is next; the blocked bead is never returned.
        let bead = reader.pop_ready_bead().expect("queue should not be empty");
        assert_eq!(bead.id, "p-low");
        reader
            .assign_bead("p-low".to_string(), "worker-1".to_string())
            .unwrap();

        assert!(reader.pop_ready_bead().is_none());
    }

    #[test]
    fn test_assign_bead_prevents_duplicate_assignment() {
        let dir = create_test_workspace();
        let mut reader = BeadQueueReader::new(dir.path()).unwrap();

        reader
            .assign_bead("test-1".to_string(), "worker-1".to_string())
            .unwrap();

        // A second worker must be refused: the assignment is the lock.
        let err = reader
            .assign_bead("test-1".to_string(), "worker-2".to_string())
            .unwrap_err();
        match err {
            ForgeError::BeadAlreadyAssigned {
                ref bead_id,
                ref worker_id,
            } => {
                assert_eq!(bead_id, "test-1");
                assert_eq!(worker_id, "worker-1");
            }
            other => panic!("expected BeadAlreadyAssigned, got: {}", other),
        }

        // The original worker keeps the lock, and re-assignment is idempotent.
        assert_eq!(
            reader.get_assigned_worker(&"test-1".to_string()),
            Some(&"worker-1".to_string())
        );
        reader
            .assign_bead("test-1".to_string(), "worker-1".to_string())
            .unwrap();
    }

    #[test]
    fn test_pop_ready_bead_skips_assigned_beads() {
        let dir = create_multi_priority_workspace();
        let mut reader = BeadQueueReader::new(dir.path()).unwrap();

        reader
            .assign_bead("p-high".to_string(), "worker-1".to_string())
            .unwrap();

        let bead = reader.pop_ready_bead().expect("queue should not be empty");
        assert_eq!(bead.id, "p-low");
    }

    #[test]
    fn test_get_bead_fetches_context() {
        let dir = create_test_workspace();
        let mut reader = BeadQueueReader::new(dir.path()).unwrap();

        let bead = reader
            .get_bead(&"test-2".to_string())
            .unwrap()
            .expect("bead should exist");
        assert_eq!(bead.id, "test-2");
        assert_eq!(bead.title, "Blocked bead");

        assert!(
            reader
                .get_bead(&"does-not-exist".to_string())
                .unwrap()
                .is_none()
        );
    }

    /// A bead-rs workspace: config.json plus a checkpoint whose active root
    /// carries the same kinds of records the queue must allocate.
    fn create_bead_rs_workspace() -> TempDir {
        let dir = TempDir::new().unwrap();
        let checkpoint = dir.path().join(".beads/checkpoint");
        let objects = checkpoint.join("objects");
        fs::create_dir_all(&objects).unwrap();

        fs::write(
            dir.path().join(".beads/config.json"),
            r#"{"prefix":"forge","uuid":"6d33e860"}"#,
        )
        .unwrap();

        let root_sha = "073fc7bbf1d714799316ad50cd08bf1be4418b04dc8934f183cf2ab88908614b";
        let mut root = fs::File::create(objects.join(format!("{root_sha}.jsonl"))).unwrap();
        writeln!(
            root,
            r#"{{"record_type":"issue","issue":{{"id":"rs-high","title":"High priority","description":"","base_status":"open","priority":0,"issue_type":"bug","labels":[],"assignee":null,"manual_blocked":false,"dependencies":[],"created_at":"2026-09-01T00:00:00Z"}}}}"#
        )
        .unwrap();
        writeln!(
            root,
            r#"{{"record_type":"issue","issue":{{"id":"rs-blocked","title":"Blocked","description":"","base_status":"open","priority":0,"issue_type":"task","labels":[],"assignee":null,"manual_blocked":false,"dependencies":[{{"blocker":"rs-high","kind":"blocks"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            root,
            r#"{{"record_type":"issue","issue":{{"id":"rs-claimed","title":"Claimed elsewhere","description":"","base_status":"in_progress","priority":0,"issue_type":"task","labels":[],"assignee":"other-worker","manual_blocked":false,"dependencies":[]}}}}"#
        )
        .unwrap();
        writeln!(
            root,
            r#"{{"record_type":"event","event":{{"id":"evt-1","kind":"created"}}}}"#
        )
        .unwrap();

        fs::write(
            checkpoint.join("current.json"),
            format!(
                r#"{{"active_root":{{"path":"objects/{root_sha}.jsonl","sha256":"{root_sha}"}},"generation_id":"gen-1","issue_count":3,"mode":"monolithic","schema_version":1}}"#
            ),
        )
        .unwrap();

        dir
    }

    #[test]
    fn test_bead_rs_store_reads_through_queue_reader() {
        let dir = create_bead_rs_workspace();
        let mut reader = BeadQueueReader::new(dir.path()).unwrap();

        // Format detection works through the queue reader.
        assert!(reader.has_beads());

        let beads = reader.read_beads().unwrap();
        assert_eq!(beads.len(), 3, "event records must not surface as beads");

        // base_status maps onto the queue's status field.
        let claimed = beads.iter().find(|b| b.id == "rs-claimed").unwrap();
        assert_eq!(claimed.status, "in_progress");

        // Only rs-high is allocatable: rs-blocked has an unfinished blocks
        // edge, rs-claimed is already assigned in the store.
        let ready = reader.get_ready_beads().unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "rs-high");

        // Blocked bead carries its dependency count for display/scoring.
        let blocked = beads.iter().find(|b| b.id == "rs-blocked").unwrap();
        assert_eq!(blocked.dependency_count, 1);
        assert_eq!(blocked.dependent_count, 0);
        assert_eq!(
            beads
                .iter()
                .find(|b| b.id == "rs-high")
                .unwrap()
                .dependent_count,
            1
        );
    }
}
