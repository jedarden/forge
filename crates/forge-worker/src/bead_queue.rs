//! Bead queue reading and management.
//!
//! This module provides functionality for reading bead queues from workspaces
//! and managing bead allocation to workers. It extends the existing bead module
//! with queue-specific operations for launcher integration.

use crate::types::{LaunchConfig, SpawnRequest};
use forge_core::types::BeadId;
use forge_core::{ForgeError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Bead queue reader for parsing .beads/*.jsonl files.
#[derive(Debug)]
pub struct BeadQueueReader {
    /// Workspace path
    workspace: PathBuf,
    /// Bead data file path (.beads/issues.jsonl)
    bead_file: PathBuf,
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

    /// Get the display string for this bead.
    pub fn display(&self) -> String {
        format!("{} [P{}] {}", self.id, self.priority, self.title)
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
        let bead_file = workspace.join(".beads/issues.jsonl");

        // Verify workspace exists
        if !workspace.exists() {
            return Err(ForgeError::WorkspaceNotFound { path: workspace });
        }

        Ok(Self {
            workspace,
            bead_file,
            ready_cache: Vec::new(),
            assignments: HashMap::new(),
        })
    }

    /// Check if this workspace has a beads database.
    pub fn has_beads(&self) -> bool {
        self.bead_file.exists()
    }

    /// Read beads from the JSONL file.
    pub fn read_beads(&mut self) -> Result<Vec<QueuedBead>> {
        if !self.has_beads() {
            debug!("No beads file found at {:?}", self.bead_file);
            return Ok(Vec::new());
        }

        let file = fs::File::open(&self.bead_file)
            .map_err(|e| ForgeError::io("opening beads file", &self.bead_file, e))?;

        let reader = std::io::BufReader::new(file);
        let mut beads = Vec::new();

        for line in reader.lines() {
            let line =
                line.map_err(|e| ForgeError::io("reading beads file", &self.bead_file, e))?;

            if line.trim().is_empty() {
                continue;
            }

            // Parse JSONL entry
            match serde_json::from_str::<serde_json::Value>(&line) {
                Ok(value) => {
                    if let Ok(bead) = Self::parse_bead(&value, &self.workspace) {
                        beads.push(bead);
                    }
                }
                Err(e) => {
                    warn!("Failed to parse bead JSONL entry: {}", e);
                    continue;
                }
            }
        }

        info!("Read {} beads from {:?}", beads.len(), self.bead_file);
        Ok(beads)
    }

    /// Parse a bead from JSON value.
    fn parse_bead(value: &serde_json::Value, workspace: &Path) -> Result<QueuedBead> {
        let id = value["id"]
            .as_str()
            .ok_or_else(|| ForgeError::parse("bead missing id field"))?
            .to_string();

        let title = value["title"]
            .as_str()
            .ok_or_else(|| ForgeError::parse("bead missing title field"))?
            .to_string();

        let description = value["description"].as_str().unwrap_or("").to_string();
        let status = value["status"].as_str().unwrap_or("open").to_string();
        let priority = value["priority"].as_u64().unwrap_or(2) as u8;
        let issue_type = value["issue_type"].as_str().unwrap_or("task").to_string();

        let labels = value["labels"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // Check dependencies
        let dependency_count = if let Some(deps) = value.get("dependencies") {
            deps.as_array().map(|a| a.len()).unwrap_or(0)
        } else {
            0
        };

        let is_ready = dependency_count == 0 && status == "open";

        Ok(QueuedBead {
            id,
            title,
            description,
            status,
            priority,
            issue_type,
            labels,
            dependency_count,
            is_ready,
            workspace: workspace.to_path_buf(),
        })
    }

    /// Get ready beads, sorted by priority.
    pub fn get_ready_beads(&mut self) -> Result<Vec<QueuedBead>> {
        let beads = self.read_beads()?;
        let ready: Vec<_> = beads.into_iter().filter(|b| b.is_allocatable()).collect();

        // Sort by priority score (P0 first)
        let mut sorted = ready;
        sorted.sort_by(|a, b| {
            b.priority_score()
                .cmp(&a.priority_score())
                .then_with(|| a.id.cmp(&b.id))
        });

        self.ready_cache = sorted.clone();
        Ok(sorted)
    }

    /// Get the next ready bead for allocation.
    pub fn pop_ready_bead(&mut self) -> Option<QueuedBead> {
        if let Ok(mut ready) = self.get_ready_beads() {
            // Filter out already assigned beads
            ready.retain(|b| !self.assignments.contains_key(&b.id));

            ready.pop()
        } else {
            None
        }
    }

    /// Assign a bead to a worker.
    pub fn assign_bead(&mut self, bead_id: BeadId, worker_id: String) -> Result<()> {
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

        SpawnRequest { worker_id, config }
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
    pub fn pop_next_ready(&mut self) -> Option<(BeadId, QueuedBead, PathBuf)> {
        let mut candidates = Vec::new();

        for reader in &mut self.readers {
            if let Some(bead) = reader.pop_ready_bead() {
                candidates.push((bead.id.clone(), bead, reader.workspace.clone()));
            }
        }

        // Sort by priority across all workspaces
        candidates.sort_by(|a, b| b.1.priority_score().cmp(&a.1.priority_score()));

        candidates.pop()
    }

    /// Get all ready beads across all workspaces.
    pub fn get_all_ready(&mut self) -> Vec<(BeadId, QueuedBead, PathBuf)> {
        let mut ready = Vec::new();

        for reader in &mut self.readers {
            if let Ok(beads) = reader.get_ready_beads() {
                for bead in beads {
                    ready.push((bead.id.clone(), bead, reader.workspace.clone()));
                }
            }
        }

        ready.sort_by(|a, b| b.1.priority_score().cmp(&a.1.priority_score()));

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
}
