//! Authentication and authorization for FORGE server.
//!
//! Provides a simple authentication mechanism with role-based access control.

use crate::ServerError;
use forge_core::UserRole;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

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

/// Simple in-memory authentication provider.
///
/// For production use, replace with a proper auth system (OAuth, JWT, etc.).
pub struct SimpleAuth {
    users: Arc<RwLock<HashMap<String, UserCredentials>>>,
}

#[derive(Clone)]
struct UserCredentials {
    password_hash: String,  // Argon2 hashed password
    display_name: String,
    role: UserRole,
}

impl SimpleAuth {
    /// Create a new simple auth provider.
    pub fn new() -> Self {
        Self {
            users: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a user to the auth provider.
    ///
    /// Passwords are hashed using Argon2 before storage.
    pub async fn add_user(&self, user_id: impl Into<String>, password: impl Into<String>, display_name: impl Into<String>, role: UserRole) {
        let password = password.into();
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let password_hash = argon2.hash_password(password.as_bytes(), &salt)
            .expect("failed to hash password")
            .to_string();

        let mut users = self.users.write().await;
        users.insert(user_id.into(), UserCredentials {
            password_hash,
            display_name: display_name.into(),
            role,
        });
    }

    /// Initialize with default users for development.
    pub async fn with_defaults(self) -> Self {
        // Default admin user: admin/admin123
        self.add_user("admin", "admin123", "Administrator", UserRole::Admin).await;
        // Default operator: operator/operator123
        self.add_user("operator", "operator123", "Operator User", UserRole::Operator).await;
        // Default viewer: viewer/viewer123
        self.add_user("viewer", "viewer123", "Viewer User", UserRole::Viewer).await;
        self
    }
}

impl Default for SimpleAuth {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AuthProvider for SimpleAuth {
    async fn authenticate(&self, user_id: &str, credentials: &str) -> Result<AuthResult, ServerError> {
        let users = self.users.read().await;

        let user = users.get(user_id)
            .ok_or_else(|| ServerError::AuthenticationFailed(format!("user not found: {}", user_id)))?;

        // Parse and verify the password hash using constant-time comparison
        let parsed_hash = PasswordHash::new(&user.password_hash)
            .map_err(|_| ServerError::AuthenticationFailed("invalid password hash format".to_string()))?;

        let argon2 = Argon2::default();
        argon2.verify_password(credentials.as_bytes(), &parsed_hash)
            .map_err(|_| ServerError::AuthenticationFailed("invalid credentials".to_string()))?;

        Ok(AuthResult {
            user_id: user_id.to_string(),
            display_name: user.display_name.clone(),
            role: user.role,
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

    #[tokio::test]
    async fn test_simple_auth() {
        let auth = SimpleAuth::default();
        auth.add_user("testuser", "password123", "Test User", UserRole::Operator).await;

        let result = auth.authenticate("testuser", "password123").await;
        assert!(result.is_ok());
        let auth_result = result.unwrap();
        assert_eq!(auth_result.user_id, "testuser");
        assert_eq!(auth_result.display_name, "Test User");
        assert_eq!(auth_result.role, UserRole::Operator);

        // Test invalid password
        let result = auth.authenticate("testuser", "wrong").await;
        assert!(result.is_err());

        // Test non-existent user
        let result = auth.authenticate("nobody", "password").await;
        assert!(result.is_err());
    }

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
