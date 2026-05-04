//! Session management for FORGE multi-user support.
//!
//! Manages active user sessions, tracks connections, and provides session lookup.

use crate::ServerError;
use forge_core::{UserSession, UserRole};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Session manager for tracking active user sessions.
#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, UserSession>>>,
    user_sessions: Arc<RwLock<HashMap<String, Vec<String>>>>, // user_id -> session_ids
}

impl SessionManager {
    /// Create a new session manager.
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            user_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session for a user.
    pub async fn create_session(
        &self,
        user_id: impl Into<String>,
        display_name: impl Into<String>,
        role: UserRole,
    ) -> Result<UserSession, ServerError> {
        let session_id = Uuid::new_v4().to_string();
        let user_id = user_id.into();
        let display_name = display_name.into();

        let session = UserSession::new(
            &session_id,
            &user_id,
            &display_name,
            role,
        );

        // Add to sessions map
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(session_id.clone(), session.clone());
        }

        // Add to user_sessions map
        {
            let mut user_sessions = self.user_sessions.write().await;
            user_sessions.entry(user_id.clone())
                .or_insert_with(Vec::new)
                .push(session_id.clone());
        }

        Ok(session)
    }

    /// Get a session by ID.
    pub async fn get_session(&self, session_id: &str) -> Option<UserSession> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// Remove a session (user disconnect).
    pub async fn remove_session(&self, session_id: &str) -> Option<UserSession> {
        let session = {
            let mut sessions = self.sessions.write().await;
            sessions.remove(session_id)
        };

        if let Some(ref session) = session {
            // Remove from user_sessions
            let mut user_sessions = self.user_sessions.write().await;
            if let Some(ids) = user_sessions.get_mut(&session.user_id) {
                ids.retain(|id| id != session_id);
                if ids.is_empty() {
                    user_sessions.remove(&session.user_id);
                }
            }
        }

        session
    }

    /// Update session activity.
    pub async fn update_activity(&self, session_id: &str) -> Result<(), ServerError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(session_id)
            .ok_or_else(|| ServerError::SessionNotFound(session_id.to_string()))?;
        session.update_activity();
        Ok(())
    }

    /// Update session's current view.
    pub async fn update_view(&self, session_id: &str, view: impl Into<String>) -> Result<(), ServerError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(session_id)
            .ok_or_else(|| ServerError::SessionNotFound(session_id.to_string()))?;
        session.set_view(view);
        Ok(())
    }

    /// Get all active sessions.
    pub async fn all_sessions(&self) -> Vec<UserSession> {
        let sessions = self.sessions.read().await;
        sessions.values().cloned().collect()
    }

    /// Get count of active sessions.
    pub async fn session_count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.len()
    }

    /// Get sessions for a specific user.
    pub async fn user_sessions(&self, user_id: &str) -> Vec<UserSession> {
        let session_ids = {
            let user_sessions = self.user_sessions.read().await;
            user_sessions.get(user_id).cloned().unwrap_or_default()
        };

        let sessions = self.sessions.read().await;
        session_ids.iter()
            .filter_map(|id| sessions.get(id).cloned())
            .collect()
    }

    /// Clean up stale sessions (no activity for 5 minutes).
    pub async fn cleanup_stale(&self) -> Vec<UserSession> {
        let stale_ids = {
            let sessions = self.sessions.read().await;
            sessions.iter()
                .filter(|(_, s)| s.is_stale())
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };

        let mut removed = Vec::new();
        for session_id in stale_ids {
            if let Some(session) = self.remove_session(&session_id).await {
                removed.push(session);
            }
        }

        removed
    }

    /// Check if a user has permission based on their session role.
    pub async fn check_permission(&self, session_id: &str, action: super::auth::PermissionAction) -> Result<bool, ServerError> {
        let session = self.get_session(session_id).await
            .ok_or_else(|| ServerError::SessionNotFound(session_id.to_string()))?;

        Ok(super::auth::check_permission(session.role, action))
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Session registry for tracking session metadata across the server.
pub struct SessionRegistry {
    manager: SessionManager,
    connected_at: std::time::Instant,
}

impl SessionRegistry {
    /// Create a new session registry.
    pub fn new() -> Self {
        Self {
            manager: SessionManager::new(),
            connected_at: std::time::Instant::now(),
        }
    }

    /// Get the session manager.
    pub fn manager(&self) -> &SessionManager {
        &self.manager
    }

    /// Get server uptime.
    pub fn uptime(&self) -> std::time::Duration {
        self.connected_at.elapsed()
    }

    /// Get server statistics.
    pub async fn stats(&self) -> SessionStats {
        let sessions = self.manager.all_sessions().await;

        let viewers = sessions.iter().filter(|s| s.role == UserRole::Viewer).count();
        let operators = sessions.iter().filter(|s| s.role == UserRole::Operator).count();
        let admins = sessions.iter().filter(|s| s.role == UserRole::Admin).count();

        SessionStats {
            total_sessions: sessions.len(),
            viewers,
            operators,
            admins,
            uptime: self.uptime(),
        }
    }
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Session statistics.
#[derive(Debug, Clone)]
pub struct SessionStats {
    pub total_sessions: usize,
    pub viewers: usize,
    pub operators: usize,
    pub admins: usize,
    pub uptime: std::time::Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_creation() {
        let manager = SessionManager::new();

        let session = manager.create_session("user1", "User One", UserRole::Operator).await.unwrap();

        assert_eq!(session.user_id, "user1");
        assert_eq!(session.display_name, "User One");
        assert_eq!(session.role, UserRole::Operator);
        assert!(!session.is_stale());
    }

    #[tokio::test]
    async fn test_session_retrieval() {
        let manager = SessionManager::new();

        manager.create_session("user1", "User One", UserRole::Operator).await.unwrap();

        let sessions = manager.all_sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].user_id, "user1");
    }

    #[tokio::test]
    async fn test_session_removal() {
        let manager = SessionManager::new();

        let session = manager.create_session("user1", "User One", UserRole::Operator).await.unwrap();

        assert_eq!(manager.session_count().await, 1);

        let removed = manager.remove_session(&session.session_id).await;
        assert!(removed.is_some());
        assert_eq!(manager.session_count().await, 0);
    }

    #[tokio::test]
    async fn test_session_activity() {
        let manager = SessionManager::new();

        let session_id = {
            let session = manager.create_session("user1", "User One", UserRole::Operator).await.unwrap();
            session.session_id.clone()
        };

        manager.update_activity(&session_id).await.unwrap();

        let session = manager.get_session(&session_id).await.unwrap();
        // Session should still be active (just updated)
        assert!(!session.is_stale());
    }

    #[tokio::test]
    async fn test_permission_check() {
        use crate::auth::PermissionAction;

        let manager = SessionManager::new();

        let viewer_session = manager.create_session("viewer", "Viewer", UserRole::Viewer).await.unwrap();
        let admin_session = manager.create_session("admin", "Admin", UserRole::Admin).await.unwrap();

        // Viewer can't spawn workers
        let can_spawn = manager.check_permission(&viewer_session.session_id, PermissionAction::SpawnWorkers).await.unwrap();
        assert!(!can_spawn);

        // Admin can spawn workers
        let can_spawn = manager.check_permission(&admin_session.session_id, PermissionAction::SpawnWorkers).await.unwrap();
        assert!(can_spawn);
    }

    #[tokio::test]
    async fn test_multiple_sessions_per_user() {
        let manager = SessionManager::new();

        manager.create_session("user1", "User One", UserRole::Operator).await.unwrap();
        manager.create_session("user1", "User One", UserRole::Operator).await.unwrap();

        let sessions = manager.user_sessions("user1").await;
        assert_eq!(sessions.len(), 2);
    }
}
