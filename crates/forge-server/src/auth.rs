//! Authentication and authorization for FORGE server.
//!
//! Provides role-based access control and authentication provider traits.
//! Production deployments should use OAuth2 authentication via OAuthAuthProvider.

use crate::ServerError;
use forge_core::UserRole;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Result of an authentication attempt.
#[derive(Debug, Clone)]
pub struct AuthResult {
    pub user_id: String,
    pub display_name: String,
    pub role: UserRole,
}

/// Authentication provider trait.
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// Authenticate a user with credentials.
    async fn authenticate(&self, user_id: &str, credentials: &str) -> Result<AuthResult, ServerError>;
}

/// Test authentication provider for integration testing.
///
/// This provider simulates OAuth-like authentication without requiring
/// actual OAuth providers. It accepts bearer tokens and maps them to test users.
/// This is only intended for testing purposes - production deployments must use OAuthAuthProvider.
pub struct TestAuthProvider {
    users: Arc<RwLock<HashMap<String, TestUser>>>,
}

#[derive(Clone)]
struct TestUser {
    user_id: String,
    display_name: String,
    role: UserRole,
}

impl TestAuthProvider {
    /// Create a new test auth provider with default test users.
    pub fn new() -> Self {
        let mut users = HashMap::new();

        // Default test users that accept bearer tokens
        users.insert("test_admin_token".to_string(), TestUser {
            user_id: "admin".to_string(),
            display_name: "Test Admin".to_string(),
            role: UserRole::Admin,
        });

        users.insert("test_operator_token".to_string(), TestUser {
            user_id: "operator".to_string(),
            display_name: "Test Operator".to_string(),
            role: UserRole::Operator,
        });

        users.insert("test_viewer_token".to_string(), TestUser {
            user_id: "viewer".to_string(),
            display_name: "Test Viewer".to_string(),
            role: UserRole::Viewer,
        });

        Self {
            users: Arc::new(RwLock::new(users)),
        }
    }
}

impl Default for TestAuthProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AuthProvider for TestAuthProvider {
    async fn authenticate(&self, _user_id: &str, credentials: &str) -> Result<AuthResult, ServerError> {
        let users = self.users.read().await;

        // Accept bearer tokens for testing
        let test_user = users.get(credentials)
            .ok_or_else(|| ServerError::AuthenticationFailed("invalid test token".to_string()))?;

        Ok(AuthResult {
            user_id: test_user.user_id.clone(),
            display_name: test_user.display_name.clone(),
            role: test_user.role,
        })
    }
}

/// Check if a user has permission to perform an action.
pub fn check_permission(role: UserRole, action: PermissionAction) -> bool {
    match action {
        PermissionAction::View => true, // All roles can view
        PermissionAction::SpawnWorkers => role.can_spawn_workers(),
        PermissionAction::KillWorkers => role.can_kill_workers(),
        PermissionAction::AssignBeads => role.can_assign_beads(),
        PermissionAction::ModifyConfig => role.can_modify_config(),
        PermissionAction::ManageUsers => role.can_manage_users(),
    }
}

/// Actions that require permission checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionAction {
    View,
    SpawnWorkers,
    KillWorkers,
    AssignBeads,
    ModifyConfig,
    ManageUsers,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_checks() {
        assert!(check_permission(UserRole::Viewer, PermissionAction::View));
        assert!(!check_permission(UserRole::Viewer, PermissionAction::SpawnWorkers));
        assert!(!check_permission(UserRole::Viewer, PermissionAction::ModifyConfig));

        assert!(check_permission(UserRole::Operator, PermissionAction::View));
        assert!(check_permission(UserRole::Operator, PermissionAction::SpawnWorkers));
        assert!(!check_permission(UserRole::Operator, PermissionAction::ModifyConfig));

        assert!(check_permission(UserRole::Admin, PermissionAction::View));
        assert!(check_permission(UserRole::Admin, PermissionAction::SpawnWorkers));
        assert!(check_permission(UserRole::Admin, PermissionAction::ModifyConfig));
        assert!(check_permission(UserRole::Admin, PermissionAction::ManageUsers));
    }
}
