//! Bead assignment tracking for team collaboration.
//!
//! This module provides functionality for tracking which user is assigned
//! to which bead/task, enabling coordination across multiple users.

use crate::{ForgeError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

/// Assignment record for a bead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeadAssignment {
    /// Bead ID
    pub bead_id: String,
    /// User ID currently assigned to this bead
    pub assigned_to: Option<String>,
    /// When the assignment was made
    pub assigned_at: Option<DateTime<Utc>>,
    /// User ID who made the assignment
    pub assigned_by: Option<String>,
    /// Assignment priority (for team coordination)
    pub priority: AssignmentPriority,
    /// Assignment status
    pub status: AssignmentStatus,
}

/// Priority level for bead assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentPriority {
    /// Critical - needs immediate attention
    Critical,
    /// High - should be worked on soon
    High,
    /// Normal - standard priority
    Normal,
    /// Low - can wait
    Low,
}

impl AssignmentPriority {
    /// Get the numeric score for sorting (higher = more important).
    pub fn score(&self) -> u32 {
        match self {
            Self::Critical => 100,
            Self::High => 75,
            Self::Normal => 50,
            Self::Low => 25,
        }
    }
}

impl std::fmt::Display for AssignmentPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Critical => write!(f, "critical"),
            Self::High => write!(f, "high"),
            Self::Normal => write!(f, "normal"),
            Self::Low => write!(f, "low"),
        }
    }
}

impl std::str::FromStr for AssignmentPriority {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "critical" => Ok(Self::Critical),
            "high" => Ok(Self::High),
            "normal" => Ok(Self::Normal),
            "low" => Ok(Self::Low),
            _ => Err(format!("unknown priority: {}", s)),
        }
    }
}

/// Status of a bead assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentStatus {
    /// Bead is unassigned
    Unassigned,
    /// Bead is assigned to a user
    Assigned,
    /// Bead is actively being worked on
    InProgress,
    /// Bead work is complete (pending review)
    Completed,
    /// Bead is blocked
    Blocked,
}

impl std::fmt::Display for AssignmentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unassigned => write!(f, "unassigned"),
            Self::Assigned => write!(f, "assigned"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
            Self::Blocked => write!(f, "blocked"),
        }
    }
}

impl std::str::FromStr for AssignmentStatus {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "unassigned" => Ok(Self::Unassigned),
            "assigned" => Ok(Self::Assigned),
            "in_progress" | "in-progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "blocked" => Ok(Self::Blocked),
            _ => Err(format!("unknown status: {}", s)),
        }
    }
}

impl BeadAssignment {
    /// Create a new unassigned bead record.
    pub fn new(bead_id: impl Into<String>) -> Self {
        Self {
            bead_id: bead_id.into(),
            assigned_to: None,
            assigned_at: None,
            assigned_by: None,
            priority: AssignmentPriority::Normal,
            status: AssignmentStatus::Unassigned,
        }
    }

    /// Create a new assigned bead record.
    pub fn assigned(
        bead_id: impl Into<String>,
        to_user: impl Into<String>,
        by_user: impl Into<String>,
        priority: AssignmentPriority,
    ) -> Self {
        Self {
            bead_id: bead_id.into(),
            assigned_to: Some(to_user.into()),
            assigned_at: Some(Utc::now()),
            assigned_by: Some(by_user.into()),
            priority,
            status: AssignmentStatus::Assigned,
        }
    }

    /// Check if this bead is assigned to a specific user.
    pub fn is_assigned_to(&self, user_id: &str) -> bool {
        self.assigned_to.as_deref() == Some(user_id)
    }

    /// Check if this bead is unassigned.
    pub fn is_unassigned(&self) -> bool {
        self.assigned_to.is_none() || self.status == AssignmentStatus::Unassigned
    }

    /// Mark this bead as in progress.
    pub fn mark_in_progress(&mut self, user_id: impl Into<String>) {
        self.assigned_to = Some(user_id.into());
        self.assigned_at = Some(Utc::now());
        self.status = AssignmentStatus::InProgress;
    }

    /// Mark this bead as completed.
    pub fn mark_completed(&mut self) {
        self.status = AssignmentStatus::Completed;
    }

    /// Unassign this bead.
    pub fn unassign(&mut self) {
        self.assigned_to = None;
        self.assigned_at = None;
        self.assigned_by = None;
        self.status = AssignmentStatus::Unassigned;
    }
}

/// Manager for bead assignments in team collaboration mode.
#[derive(Clone)]
pub struct AssignmentManager {
    assignments: Arc<Mutex<HashMap<String, BeadAssignment>>>,
    storage_path: Option<PathBuf>,
}

impl AssignmentManager {
    /// Create a new in-memory assignment manager.
    pub fn new() -> Self {
        Self {
            assignments: Arc::new(Mutex::new(HashMap::new())),
            storage_path: None,
        }
    }

    /// Create a new assignment manager with file-backed storage.
    pub fn with_storage<P: AsRef<Path>>(path: P) -> Result<Self> {
        let storage_path = path.as_ref().to_path_buf();
        let mut manager = Self {
            assignments: Arc::new(Mutex::new(HashMap::new())),
            storage_path: Some(storage_path.clone()),
        };

        // Load existing assignments if file exists
        if storage_path.exists() {
            manager.load()?;
        }

        info!(
            path = %storage_path.display(),
            "Assignment manager initialized with storage"
        );

        Ok(manager)
    }

    /// Assign a bead to a user.
    pub fn assign(
        &self,
        bead_id: impl Into<String>,
        to_user: impl Into<String>,
        by_user: impl Into<String>,
        priority: AssignmentPriority,
    ) -> Result<()> {
        let bead_id = bead_id.into();
        let to_user = to_user.into();
        let by_user = by_user.into();

        let mut assignments = self.assignments.lock().map_err(|e| {
            ForgeError::assignment_error(format!("failed to acquire lock: {}", e))
        })?;

        let assignment = BeadAssignment::assigned(
            bead_id.clone(),
            to_user.clone(),
            by_user.clone(),
            priority,
        );

        assignments.insert(bead_id.clone(), assignment.clone());

        info!(
            bead_id = %bead_id,
            to_user = %to_user,
            by_user = %by_user,
            priority = %priority,
            "Assigned bead to user"
        );

        // Persist if storage is configured
        if let Some(ref path) = self.storage_path {
            self.save_inner(&assignments, path)?;
        }

        Ok(())
    }

    /// Unassign a bead.
    pub fn unassign(&self, bead_id: impl Into<String>, by_user: impl Into<String>) -> Result<()> {
        let bead_id = bead_id.into();
        let by_user = by_user.into();

        let mut assignments = self.assignments.lock().map_err(|e| {
            ForgeError::assignment_error(format!("failed to acquire lock: {}", e))
        })?;

        if let Some(assignment) = assignments.get_mut(&bead_id) {
            assignment.unassign();

            info!(
                bead_id = %bead_id,
                by_user = %by_user,
                "Unassigned bead"
            );

            // Persist if storage is configured
            if let Some(ref path) = self.storage_path {
                self.save_inner(&assignments, path)?;
            }

            Ok(())
        } else {
            Err(ForgeError::assignment_error(format!(
                "bead not found: {}",
                bead_id
            )))
        }
    }

    /// Mark a bead as in progress.
    pub fn mark_in_progress(
        &self,
        bead_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Result<()> {
        let bead_id = bead_id.into();
        let user_id = user_id.into();

        let mut assignments = self.assignments.lock().map_err(|e| {
            ForgeError::assignment_error(format!("failed to acquire lock: {}", e))
        })?;

        if let Some(assignment) = assignments.get_mut(&bead_id) {
            assignment.mark_in_progress(&user_id);

            debug!(
                bead_id = %bead_id,
                user_id = %user_id,
                "Marked bead as in progress"
            );

            // Persist if storage is configured
            if let Some(ref path) = self.storage_path {
                self.save_inner(&assignments, path)?;
            }

            Ok(())
        } else {
            // Create a new assignment if it doesn't exist
            let mut assignment = BeadAssignment::new(bead_id.clone());
            assignment.mark_in_progress(&user_id);
            assignments.insert(bead_id.clone(), assignment);

            // Persist if storage is configured
            if let Some(ref path) = self.storage_path {
                self.save_inner(&assignments, path)?;
            }

            Ok(())
        }
    }

    /// Mark a bead as completed.
    pub fn mark_completed(&self, bead_id: impl Into<String>) -> Result<()> {
        let bead_id = bead_id.into();

        let mut assignments = self.assignments.lock().map_err(|e| {
            ForgeError::assignment_error(format!("failed to acquire lock: {}", e))
        })?;

        if let Some(assignment) = assignments.get_mut(&bead_id) {
            assignment.mark_completed();

            info!(
                bead_id = %bead_id,
                "Marked bead as completed"
            );

            // Persist if storage is configured
            if let Some(ref path) = self.storage_path {
                self.save_inner(&assignments, path)?;
            }

            Ok(())
        } else {
            Err(ForgeError::assignment_error(format!(
                "bead not found: {}",
                bead_id
            )))
        }
    }

    /// Get assignment for a specific bead.
    pub fn get_assignment(&self, bead_id: &str) -> Result<Option<BeadAssignment>> {
        let assignments = self.assignments.lock().map_err(|e| {
            ForgeError::assignment_error(format!("failed to acquire lock: {}", e))
        })?;

        Ok(assignments.get(bead_id).cloned())
    }

    /// Get all assignments.
    pub fn get_all_assignments(&self) -> Result<Vec<BeadAssignment>> {
        let assignments = self.assignments.lock().map_err(|e| {
            ForgeError::assignment_error(format!("failed to acquire lock: {}", e))
        })?;

        Ok(assignments.values().cloned().collect())
    }

    /// Get assignments for a specific user.
    pub fn get_user_assignments(&self, user_id: &str) -> Result<Vec<BeadAssignment>> {
        let assignments = self.assignments.lock().map_err(|e| {
            ForgeError::assignment_error(format!("failed to acquire lock: {}", e))
        })?;

        let user_assignments: Vec<_> = assignments
            .values()
            .filter(|a| a.is_assigned_to(user_id))
            .cloned()
            .collect();

        Ok(user_assignments)
    }

    /// Get unassigned beads sorted by priority.
    pub fn get_unassigned(&self) -> Result<Vec<BeadAssignment>> {
        let assignments = self.assignments.lock().map_err(|e| {
            ForgeError::assignment_error(format!("failed to acquire lock: {}", e))
        })?;

        let mut unassigned: Vec<_> = assignments
            .values()
            .filter(|a| a.is_unassigned())
            .cloned()
            .collect();

        // Sort by priority (descending)
        unassigned.sort_by(|a, b| b.priority.score().cmp(&a.priority.score()));

        Ok(unassigned)
    }

    /// Get assignments by status.
    pub fn get_by_status(&self, status: AssignmentStatus) -> Result<Vec<BeadAssignment>> {
        let assignments = self.assignments.lock().map_err(|e| {
            ForgeError::assignment_error(format!("failed to acquire lock: {}", e))
        })?;

        let filtered: Vec<_> = assignments
            .values()
            .filter(|a| a.status == status)
            .cloned()
            .collect();

        Ok(filtered)
    }

    /// Set priority for a bead.
    pub fn set_priority(
        &self,
        bead_id: impl Into<String>,
        priority: AssignmentPriority,
    ) -> Result<()> {
        let bead_id = bead_id.into();

        let mut assignments = self.assignments.lock().map_err(|e| {
            ForgeError::assignment_error(format!("failed to acquire lock: {}", e))
        })?;

        if let Some(assignment) = assignments.get_mut(&bead_id) {
            assignment.priority = priority;

            debug!(
                bead_id = %bead_id,
                priority = %priority,
                "Set bead priority"
            );

            // Persist if storage is configured
            if let Some(ref path) = self.storage_path {
                self.save_inner(&assignments, path)?;
            }

            Ok(())
        } else {
            Err(ForgeError::assignment_error(format!(
                "bead not found: {}",
                bead_id
            )))
        }
    }

    /// Remove a bead from assignments (when closed/deleted).
    pub fn remove_assignment(&self, bead_id: impl Into<String>) -> Result<()> {
        let bead_id = bead_id.into();

        let mut assignments = self.assignments.lock().map_err(|e| {
            ForgeError::assignment_error(format!("failed to acquire lock: {}", e))
        })?;

        assignments.remove(&bead_id);

        debug!(bead_id = %bead_id, "Removed bead assignment");

        // Persist if storage is configured
        if let Some(ref path) = self.storage_path {
            self.save_inner(&assignments, path)?;
        }

        Ok(())
    }

    /// Load assignments from storage file.
    fn load(&mut self) -> Result<()> {
        let path = self.storage_path.as_ref().ok_or_else(|| {
            ForgeError::assignment_error("no storage path configured")
        })?;

        let content = fs::read_to_string(path).map_err(|e| ForgeError::io(
            "read",
            path,
            e,
        ))?;

        let loaded: HashMap<String, BeadAssignment> =
            serde_yaml::from_str(&content).map_err(|e| ForgeError::YamlParse {
                context: format!("assignments file at {:?}", path),
                message: e.to_string(),
            })?;

        let mut assignments = self.assignments.lock().map_err(|e| {
            ForgeError::assignment_error(format!("failed to acquire lock: {}", e))
        })?;

        *assignments = loaded;

        info!(
            path = %path.display(),
            count = assignments.len(),
            "Loaded assignments from storage"
        );

        Ok(())
    }

    /// Save assignments to storage file (inner version that doesn't need mut self).
    fn save_inner(
        &self,
        assignments: &HashMap<String, BeadAssignment>,
        path: &Path,
    ) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| ForgeError::io("create_dir_all", parent, e))?;
        }

        let yaml = serde_yaml::to_string(assignments).map_err(|e| ForgeError::YamlParse {
            context: "assignments serialization".to_string(),
            message: e.to_string(),
        })?;

        fs::write(path, yaml)
            .map_err(|e| ForgeError::io("write", path, e))?;

        debug!(
            path = %path.display(),
            count = assignments.len(),
            "Saved assignments to storage"
        );

        Ok(())
    }

    /// Force save assignments to storage.
    pub fn save(&self) -> Result<()> {
        let assignments = self.assignments.lock().map_err(|e| {
            ForgeError::assignment_error(format!("failed to acquire lock: {}", e))
        })?;

        if let Some(ref path) = self.storage_path {
            self.save_inner(&assignments, path)?;
        }

        Ok(())
    }

    /// Get assignment statistics.
    pub fn stats(&self) -> Result<AssignmentStats> {
        let assignments = self.assignments.lock().map_err(|e| {
            ForgeError::assignment_error(format!("failed to acquire lock: {}", e))
        })?;

        let mut stats = AssignmentStats::default();

        for assignment in assignments.values() {
            stats.total += 1;

            match assignment.status {
                AssignmentStatus::Unassigned => stats.unassigned += 1,
                AssignmentStatus::Assigned => stats.assigned += 1,
                AssignmentStatus::InProgress => stats.in_progress += 1,
                AssignmentStatus::Completed => stats.completed += 1,
                AssignmentStatus::Blocked => stats.blocked += 1,
            }

            // Count by user
            if let Some(ref user) = assignment.assigned_to {
                *stats.by_user.entry(user.clone()).or_insert(0) += 1;
            }
        }

        Ok(stats)
    }
}

impl Default for AssignmentManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about bead assignments.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssignmentStats {
    pub total: usize,
    pub unassigned: usize,
    pub assigned: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub blocked: usize,
    pub by_user: HashMap<String, usize>,
}

// Add assignment_error method to ForgeError
impl ForgeError {
    pub fn assignment_error(message: impl Into<String>) -> Self {
        Self::Internal {
            message: format!("Assignment error: {}", message.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_bead_assignment_new() {
        let assignment = BeadAssignment::new("bd-123");

        assert_eq!(assignment.bead_id, "bd-123");
        assert!(assignment.is_unassigned());
        assert!(assignment.assigned_to.is_none());
    }

    #[test]
    fn test_bead_assignment_assigned() {
        let assignment = BeadAssignment::assigned(
            "bd-123",
            "user1",
            "admin",
            AssignmentPriority::High,
        );

        assert_eq!(assignment.bead_id, "bd-123");
        assert!(assignment.is_assigned_to("user1"));
        assert_eq!(assignment.status, AssignmentStatus::Assigned);
        assert_eq!(assignment.priority, AssignmentPriority::High);
    }

    #[test]
    fn test_bead_assignment_progression() {
        let mut assignment = BeadAssignment::new("bd-123");

        assert!(assignment.is_unassigned());

        assignment.mark_in_progress("user1");
        assert!(assignment.is_assigned_to("user1"));
        assert_eq!(assignment.status, AssignmentStatus::InProgress);

        assignment.mark_completed();
        assert_eq!(assignment.status, AssignmentStatus::Completed);

        assignment.unassign();
        assert!(assignment.is_unassigned());
    }

    #[test]
    fn test_assignment_manager() {
        let manager = AssignmentManager::new();

        manager.assign(
            "bd-1",
            "user1",
            "admin",
            AssignmentPriority::Normal,
        ).unwrap();

        let assignment = manager.get_assignment("bd-1").unwrap().unwrap();
        assert!(assignment.is_assigned_to("user1"));

        manager.unassign("bd-1", "admin").unwrap();

        let assignment = manager.get_assignment("bd-1").unwrap().unwrap();
        assert!(assignment.is_unassigned());
    }

    #[test]
    fn test_assignment_manager_with_storage() {
        let dir = tempdir().unwrap();
        let storage_path = dir.path().join("assignments.yaml");

        let manager1 = AssignmentManager::with_storage(&storage_path).unwrap();
        manager1.assign(
            "bd-1",
            "user1",
            "admin",
            AssignmentPriority::Critical,
        ).unwrap();

        // Create a new manager and verify data persists
        let manager2 = AssignmentManager::with_storage(&storage_path).unwrap();
        let assignment = manager2.get_assignment("bd-1").unwrap().unwrap();

        assert!(assignment.is_assigned_to("user1"));
        assert_eq!(assignment.priority, AssignmentPriority::Critical);
    }

    #[test]
    fn test_get_user_assignments() {
        let manager = AssignmentManager::new();

        manager.assign("bd-1", "user1", "admin", AssignmentPriority::Normal).unwrap();
        manager.assign("bd-2", "user2", "admin", AssignmentPriority::Normal).unwrap();
        manager.assign("bd-3", "user1", "admin", AssignmentPriority::Normal).unwrap();

        let user1_assignments = manager.get_user_assignments("user1").unwrap();
        assert_eq!(user1_assignments.len(), 2);

        let user2_assignments = manager.get_user_assignments("user2").unwrap();
        assert_eq!(user2_assignments.len(), 1);
    }

    #[test]
    fn test_get_unassigned() {
        let manager = AssignmentManager::new();

        manager.assign("bd-1", "user1", "admin", AssignmentPriority::Normal).unwrap();
        manager.assign("bd-2", "user2", "admin", AssignmentPriority::Normal).unwrap();
        manager.assign("bd-3", "user3", "admin", AssignmentPriority::Critical).unwrap();

        // Unassign bd-2
        manager.unassign("bd-2", "admin").unwrap();

        let unassigned = manager.get_unassigned().unwrap();
        assert_eq!(unassigned.len(), 1);
        assert_eq!(unassigned[0].bead_id, "bd-2");
    }

    #[test]
    fn test_assignment_stats() {
        let manager = AssignmentManager::new();

        manager.assign("bd-1", "user1", "admin", AssignmentPriority::Normal).unwrap();
        manager.assign("bd-2", "user2", "admin", AssignmentPriority::Normal).unwrap();

        let mut stats = manager.stats().unwrap();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.assigned, 2);
        assert_eq!(stats.by_user.get("user1"), Some(&1));
        assert_eq!(stats.by_user.get("user2"), Some(&1));

        manager.mark_in_progress("bd-1", "user1").unwrap();

        stats = manager.stats().unwrap();
        assert_eq!(stats.in_progress, 1);
    }

    #[test]
    fn test_priority_from_str() {
        assert_eq!("critical".parse::<AssignmentPriority>().unwrap(), AssignmentPriority::Critical);
        assert_eq!("high".parse::<AssignmentPriority>().unwrap(), AssignmentPriority::High);
        assert_eq!("normal".parse::<AssignmentPriority>().unwrap(), AssignmentPriority::Normal);
        assert_eq!("low".parse::<AssignmentPriority>().unwrap(), AssignmentPriority::Low);
        assert!("unknown".parse::<AssignmentPriority>().is_err());
    }

    #[test]
    fn test_status_from_str() {
        assert_eq!("unassigned".parse::<AssignmentStatus>().unwrap(), AssignmentStatus::Unassigned);
        assert_eq!("assigned".parse::<AssignmentStatus>().unwrap(), AssignmentStatus::Assigned);
        assert_eq!("in-progress".parse::<AssignmentStatus>().unwrap(), AssignmentStatus::InProgress);
        assert_eq!("in_progress".parse::<AssignmentStatus>().unwrap(), AssignmentStatus::InProgress);
        assert_eq!("completed".parse::<AssignmentStatus>().unwrap(), AssignmentStatus::Completed);
        assert_eq!("blocked".parse::<AssignmentStatus>().unwrap(), AssignmentStatus::Blocked);
    }
}
