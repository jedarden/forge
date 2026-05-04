//! Integration tests for FORGE team collaboration features.
//!
//! Tests multi-user session support, authentication, role-based access control,
//! and bead assignment tracking.

use forge_server::{
    auth::{SimpleAuth, AuthProvider, check_permission, PermissionAction},
    session::SessionManager,
    assignment::BeadAssignmentTracker,
    protocol::{ClientMessage, ServerMessage},
};
use forge_core::UserRole;
use tokio::time::{sleep, Duration};

/// Test authentication with default users.
#[tokio::test]
async fn test_authentication_default_users() {
    let auth = SimpleAuth::default().with_defaults().await;

    // Test admin user
    let result = auth.authenticate("admin", "admin123").await;
    assert!(result.is_ok());
    let auth_result = result.unwrap();
    assert_eq!(auth_result.user_id, "admin");
    assert_eq!(auth_result.role, UserRole::Admin);

    // Test operator user
    let result = auth.authenticate("operator", "operator123").await;
    assert!(result.is_ok());
    let auth_result = result.unwrap();
    assert_eq!(auth_result.user_id, "operator");
    assert_eq!(auth_result.role, UserRole::Operator);

    // Test viewer user
    let result = auth.authenticate("viewer", "viewer123").await;
    assert!(result.is_ok());
    let auth_result = result.unwrap();
    assert_eq!(auth_result.user_id, "viewer");
    assert_eq!(auth_result.role, UserRole::Viewer);

    // Test invalid password
    let result = auth.authenticate("admin", "wrong").await;
    assert!(result.is_err());

    // Test non-existent user
    let result = auth.authenticate("nobody", "password").await;
    assert!(result.is_err());
}

/// Test role-based permission checks.
#[test]
fn test_role_permissions() {
    // Viewer permissions
    assert!(check_permission(UserRole::Viewer, PermissionAction::View));
    assert!(!check_permission(UserRole::Viewer, PermissionAction::SpawnWorkers));
    assert!(!check_permission(UserRole::Viewer, PermissionAction::KillWorkers));
    assert!(!check_permission(UserRole::Viewer, PermissionAction::AssignBeads));
    assert!(!check_permission(UserRole::Viewer, PermissionAction::ModifyConfig));
    assert!(!check_permission(UserRole::Viewer, PermissionAction::ManageUsers));

    // Operator permissions
    assert!(check_permission(UserRole::Operator, PermissionAction::View));
    assert!(check_permission(UserRole::Operator, PermissionAction::SpawnWorkers));
    assert!(check_permission(UserRole::Operator, PermissionAction::KillWorkers));
    assert!(check_permission(UserRole::Operator, PermissionAction::AssignBeads));
    assert!(!check_permission(UserRole::Operator, PermissionAction::ModifyConfig));
    assert!(!check_permission(UserRole::Operator, PermissionAction::ManageUsers));

    // Admin permissions
    assert!(check_permission(UserRole::Admin, PermissionAction::View));
    assert!(check_permission(UserRole::Admin, PermissionAction::SpawnWorkers));
    assert!(check_permission(UserRole::Admin, PermissionAction::KillWorkers));
    assert!(check_permission(UserRole::Admin, PermissionAction::AssignBeads));
    assert!(check_permission(UserRole::Admin, PermissionAction::ModifyConfig));
    assert!(check_permission(UserRole::Admin, PermissionAction::ManageUsers));
}

/// Test session creation and management.
#[tokio::test]
async fn test_session_management() {
    let manager = SessionManager::new();

    // Create sessions for different users
    let admin_session = manager.create_session("admin", "Admin User", UserRole::Admin).await.unwrap();
    let operator_session = manager.create_session("operator", "Operator User", UserRole::Operator).await.unwrap();
    let viewer_session = manager.create_session("viewer", "Viewer User", UserRole::Viewer).await.unwrap();

    // Verify sessions
    assert_eq!(manager.session_count().await, 3);

    // Get all sessions
    let all_sessions = manager.all_sessions().await;
    assert_eq!(all_sessions.len(), 3);

    // Get specific session
    let retrieved = manager.get_session(&admin_session.session_id).await;
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().user_id, "admin");

    // Remove a session
    let removed = manager.remove_session(&viewer_session.session_id).await;
    assert!(removed.is_some());
    assert_eq!(manager.session_count().await, 2);

    // Update activity
    manager.update_activity(&operator_session.session_id).await.unwrap();
    let session = manager.get_session(&operator_session.session_id).await.unwrap();
    assert!(!session.is_stale(3600)); // 1 hour timeout
}

/// Test bead assignment tracking.
#[tokio::test]
async fn test_bead_assignment() {
    let tracker = BeadAssignmentTracker::new();

    // Assign beads to users
    tracker.assign("bead-1", "user-a", "admin").await.unwrap();
    tracker.assign("bead-2", "user-b", "admin").await.unwrap();
    tracker.assign("bead-3", "user-a", "admin").await.unwrap();

    // Check assignments
    let user_a_beads = tracker.user_assignments("user-a").await;
    assert_eq!(user_a_beads.len(), 2);

    let user_b_beads = tracker.user_assignments("user-b").await;
    assert_eq!(user_b_beads.len(), 1);

    // Get specific assignment
    let assignment = tracker.get_assignment("bead-1").await;
    assert!(assignment.is_some());
    assert_eq!(assignment.unwrap().assigned_to, Some("user-a".to_string()));

    // Unassign a bead
    let removed = tracker.unassign("bead-2").await.unwrap();
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().bead_id, "bead-2");

    // Verify unassignment
    let user_b_beads = tracker.user_assignments("user-b").await;
    assert_eq!(user_b_beads.len(), 0);

    // Test reassignment
    tracker.assign("bead-1", "user-a", "admin").await.unwrap();
    let reassigned = tracker.reassign("bead-1", "user-a", "user-c", "admin").await;
    assert!(reassigned.is_ok());
    assert_eq!(reassigned.unwrap().assigned_to, Some("user-c".to_string()));
}

/// Test concurrent session management.
#[tokio::test]
async fn test_concurrent_sessions() {
    let manager = SessionManager::new();

    // Create multiple sessions concurrently
    let mut handles = Vec::new();
    for i in 0..10 {
        let manager = manager.clone();
        let handle = tokio::spawn(async move {
            manager.create_session(
                &format!("user-{}", i),
                &format!("User {}", i),
                UserRole::Operator,
            ).await
        });
        handles.push(handle);
    }

    // Wait for all sessions to be created
    for handle in handles {
        assert!(handle.await.unwrap().is_ok());
    }

    // Verify all sessions were created
    sleep(Duration::from_millis(100)).await;
    assert_eq!(manager.session_count().await, 10);
}

/// Test permission checking via session manager.
#[tokio::test]
async fn test_session_permission_checks() {
    let manager = SessionManager::new();

    // Create sessions with different roles
    let viewer = manager.create_session("viewer", "Viewer", UserRole::Viewer).await.unwrap();
    let operator = manager.create_session("operator", "Operator", UserRole::Operator).await.unwrap();
    let admin = manager.create_session("admin", "Admin", UserRole::Admin).await.unwrap();

    // Check viewer permissions
    assert!(manager.check_permission(&viewer.session_id, PermissionAction::View).await.unwrap());
    assert!(!manager.check_permission(&viewer.session_id, PermissionAction::SpawnWorkers).await.unwrap());
    assert!(!manager.check_permission(&viewer.session_id, PermissionAction::AssignBeads).await.unwrap());

    // Check operator permissions
    assert!(manager.check_permission(&operator.session_id, PermissionAction::View).await.unwrap());
    assert!(manager.check_permission(&operator.session_id, PermissionAction::SpawnWorkers).await.unwrap());
    assert!(manager.check_permission(&operator.session_id, PermissionAction::AssignBeads).await.unwrap());
    assert!(!manager.check_permission(&operator.session_id, PermissionAction::ModifyConfig).await.unwrap());

    // Check admin permissions
    assert!(manager.check_permission(&admin.session_id, PermissionAction::View).await.unwrap());
    assert!(manager.check_permission(&admin.session_id, PermissionAction::ModifyConfig).await.unwrap());
    assert!(manager.check_permission(&admin.session_id, PermissionAction::ManageUsers).await.unwrap());
}

/// Test stale session cleanup.
#[tokio::test]
async fn test_stale_session_cleanup() {
    let manager = SessionManager::new();

    // Create a session
    let session = manager.create_session("user", "User", UserRole::Viewer).await.unwrap();
    assert_eq!(manager.session_count().await, 1);

    // Sessions should not be stale immediately
    let retrieved = manager.get_session(&session.session_id).await.unwrap();
    assert!(!retrieved.is_stale(3600)); // 1 hour timeout

    // Cleanup should not remove active sessions
    let stale = manager.cleanup_stale().await;
    assert_eq!(stale.len(), 0);
    assert_eq!(manager.session_count().await, 1);
}

/// Test assignment counts.
#[tokio::test]
async fn test_assignment_counts() {
    let tracker = BeadAssignmentTracker::new();

    // Assign various beads
    tracker.assign("bead-1", "user-a", "admin").await.unwrap();
    tracker.assign("bead-2", "user-a", "admin").await.unwrap();
    tracker.assign("bead-3", "user-a", "admin").await.unwrap();
    tracker.assign("bead-4", "user-b", "admin").await.unwrap();
    tracker.assign("bead-5", "user-b", "admin").await.unwrap();
    tracker.assign("bead-6", "user-c", "admin").await.unwrap();

    // Check counts
    let counts = tracker.assignment_counts().await;
    assert_eq!(counts.get("user-a"), Some(&3));
    assert_eq!(counts.get("user-b"), Some(&2));
    assert_eq!(counts.get("user-c"), Some(&1));
}
