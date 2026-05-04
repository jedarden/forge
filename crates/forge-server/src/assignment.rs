//! Bead assignment tracking for team collaboration.
//!
//! Manages bead assignments to users, enabling shared task queues.

use forge_core::{BeadAssignment, ForgeError, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Tracker for bead assignments.
#[derive(Clone)]
pub struct BeadAssignmentTracker {
    assignments: Arc<RwLock<HashMap<String, BeadAssignment>>>, // bead_id -> assignment
    user_assignments: Arc<RwLock<HashMap<String, Vec<String>>>>, // user_id -> bead_ids
}

impl BeadAssignmentTracker {
    /// Create a new assignment tracker.
    pub fn new() -> Self {
        Self {
            assignments: Arc::new(RwLock::new(HashMap::new())),
            user_assignments: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Assign a bead to a user.
    pub async fn assign(
        &self,
        bead_id: impl Into<String>,
        to_user: impl Into<String>,
        by_user: impl Into<String>,
    ) -> Result<BeadAssignment> {
        let bead_id = bead_id.into();
        let to_user = to_user.into();
        let by_user = by_user.into();

        // Check if already assigned
        {
            let assignments = self.assignments.read().await;
            if let Some(existing) = assignments.get(&bead_id) {
                if existing.assigned_to.as_deref() == Some(&to_user) {
                    return Ok(existing.clone());
                }
            }
        }

        let assignment = BeadAssignment::assigned(&bead_id, &to_user, &by_user, forge_core::AssignmentPriority::Normal);

        // Add to assignments
        {
            let mut assignments = self.assignments.write().await;
            assignments.insert(bead_id.clone(), assignment.clone());
        }

        // Add to user_assignments
        {
            let mut user_assignments = self.user_assignments.write().await;
            user_assignments.entry(to_user.clone())
                .or_insert_with(Vec::new)
                .push(bead_id.clone());
        }

        Ok(assignment)
    }

    /// Unassign a bead.
    pub async fn unassign(&self, bead_id: impl Into<String>) -> Result<Option<BeadAssignment>> {
        let bead_id = bead_id.into();

        let assignment = {
            let mut assignments = self.assignments.write().await;
            assignments.remove(&bead_id)
        };

        if let Some(ref assignment) = assignment {
            // Remove from user_assignments
            if let Some(ref assigned_to) = assignment.assigned_to {
                let mut user_assignments = self.user_assignments.write().await;
                if let Some(bead_ids) = user_assignments.get_mut(assigned_to) {
                    bead_ids.retain(|id| id != &bead_id);
                    if bead_ids.is_empty() {
                        user_assignments.remove(assigned_to);
                    }
                }
            }
        }

        Ok(assignment)
    }

    /// Get assignment for a bead.
    pub async fn get_assignment(&self, bead_id: &str) -> Option<BeadAssignment> {
        let assignments = self.assignments.read().await;
        assignments.get(bead_id).cloned()
    }

    /// Get all beads assigned to a user.
    pub async fn user_assignments(&self, user_id: &str) -> Vec<BeadAssignment> {
        let bead_ids = {
            let user_assignments = self.user_assignments.read().await;
            user_assignments.get(user_id).cloned().unwrap_or_default()
        };

        let assignments = self.assignments.read().await;
        bead_ids.iter()
            .filter_map(|id| assignments.get(id).cloned())
            .collect()
    }

    /// Get all assignments.
    pub async fn all_assignments(&self) -> Vec<BeadAssignment> {
        let assignments = self.assignments.read().await;
        assignments.values().cloned().collect()
    }

    /// Reassign a bead from one user to another.
    pub async fn reassign(
        &self,
        bead_id: impl Into<String>,
        from_user: impl Into<String>,
        to_user: impl Into<String>,
        by_user: impl Into<String>,
    ) -> Result<BeadAssignment> {
        let bead_id = bead_id.into();
        let from_user = from_user.into();

        // Verify current assignment
        {
            let assignments = self.assignments.read().await;
            if let Some(existing) = assignments.get(&bead_id) {
                if existing.assigned_to.as_deref() != Some(&from_user) {
                    return Err(ForgeError::ConfigValidation {
                        message: format!(
                            "bead {} is assigned to {}, not {}",
                            bead_id,
                            existing.assigned_to.as_deref().unwrap_or(&"<unassigned>".to_string()),
                            from_user
                        )
                    });
                }
            }
        }

        // Unassign from old user
        self.unassign(&bead_id).await?;

        // Assign to new user
        self.assign(&bead_id, to_user, by_user).await
    }

    /// Get count of assignments per user.
    pub async fn assignment_counts(&self) -> HashMap<String, usize> {
        let user_assignments = self.user_assignments.read().await;
        user_assignments.iter()
            .map(|(user, beads)| (user.clone(), beads.len()))
            .collect()
    }

    /// Clear all assignments (for testing).
    pub async fn clear(&self) {
        let mut assignments = self.assignments.write().await;
        let mut user_assignments = self.user_assignments.write().await;
        assignments.clear();
        user_assignments.clear();
    }
}

impl Default for BeadAssignmentTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_assign_bead() {
        let tracker = BeadAssignmentTracker::new();

        let assignment = tracker.assign("bead-1", "user-a", "admin").await.unwrap();

        assert_eq!(assignment.bead_id, "bead-1");
        assert_eq!(assignment.assigned_to, Some("user-a".to_string()));
        assert_eq!(assignment.assigned_by, Some("admin".to_string()));

        let retrieved = tracker.get_assignment("bead-1").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().assigned_to, Some("user-a".to_string()));
    }

    #[tokio::test]
    async fn test_unassign_bead() {
        let tracker = BeadAssignmentTracker::new();

        tracker.assign("bead-1", "user-a", "admin").await.unwrap();
        let removed = tracker.unassign("bead-1").await.unwrap();

        assert!(removed.is_some());
        assert_eq!(removed.unwrap().bead_id, "bead-1");

        let retrieved = tracker.get_assignment("bead-1").await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_user_assignments() {
        let tracker = BeadAssignmentTracker::new();

        tracker.assign("bead-1", "user-a", "admin").await.unwrap();
        tracker.assign("bead-2", "user-a", "admin").await.unwrap();
        tracker.assign("bead-3", "user-b", "admin").await.unwrap();

        let user_a_assignments = tracker.user_assignments("user-a").await;
        assert_eq!(user_a_assignments.len(), 2);

        let user_b_assignments = tracker.user_assignments("user-b").await;
        assert_eq!(user_b_assignments.len(), 1);
    }

    #[tokio::test]
    async fn test_reassign_bead() {
        let tracker = BeadAssignmentTracker::new();

        tracker.assign("bead-1", "user-a", "admin").await.unwrap();
        let new_assignment = tracker.reassign("bead-1", "user-a", "user-b", "admin").await.unwrap();

        assert_eq!(new_assignment.assigned_to, Some("user-b".to_string()));

        let user_a_assignments = tracker.user_assignments("user-a").await;
        assert_eq!(user_a_assignments.len(), 0);

        let user_b_assignments = tracker.user_assignments("user-b").await;
        assert_eq!(user_b_assignments.len(), 1);
    }

    #[tokio::test]
    async fn test_assignment_counts() {
        let tracker = BeadAssignmentTracker::new();

        tracker.assign("bead-1", "user-a", "admin").await.unwrap();
        tracker.assign("bead-2", "user-a", "admin").await.unwrap();
        tracker.assign("bead-3", "user-b", "admin").await.unwrap();

        let counts = tracker.assignment_counts().await;
        assert_eq!(counts.get("user-a"), Some(&2));
        assert_eq!(counts.get("user-b"), Some(&1));
    }
}
