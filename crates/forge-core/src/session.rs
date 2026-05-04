//! User session types and management for team collaboration.
//!
//! This module provides types and functionality for managing multiple users
//! connected to a FORGE instance, including role-based access control and
//! session tracking.

use crate::{ForgeError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{debug, info, warn};

/// User role determining permissions in team collaboration mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    /// Administrator - full access to all operations
    Admin,
    /// Operator - can perform worker operations and manage tasks
    Operator,
    /// Viewer - read-only access to dashboard
    Viewer,
}

impl UserRole {
    /// Check if this role can perform worker spawn operations.
    pub fn can_spawn_workers(&self) -> bool {
        matches!(self, Self::Admin | Self::Operator)
    }

    /// Check if this role can perform worker kill operations.
    pub fn can_kill_workers(&self) -> bool {
        matches!(self, Self::Admin | Self::Operator)
    }

    /// Check if this role can modify configuration.
    pub fn can_modify_config(&self) -> bool {
        matches!(self, Self::Admin)
    }

    /// Check if this role can assign beads/tasks.
    pub fn can_assign_beads(&self) -> bool {
        matches!(self, Self::Admin | Self::Operator)
    }

    /// Check if this role can kill other users' sessions.
    pub fn can_manage_sessions(&self) -> bool {
        matches!(self, Self::Admin)
    }

    /// Check if this role can manage users (add/remove users).
    pub fn can_manage_users(&self) -> bool {
        matches!(self, Self::Admin)
    }

    /// Get the display name for this role.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Admin => "Admin",
            Self::Operator => "Operator",
            Self::Viewer => "Viewer",
        }
    }
}

impl std::fmt::Display for UserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

impl std::str::FromStr for UserRole {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "admin" => Ok(Self::Admin),
            "operator" => Ok(Self::Operator),
            "viewer" => Ok(Self::Viewer),
            _ => Err(format!("unknown role: {}", s)),
        }
    }
}

/// Status of a user session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Session is actively receiving updates
    Active,
    /// Session is idle (no recent activity)
    Idle,
    /// Session has disconnected
    Disconnected,
}

impl SessionStatus {
    /// Get the display indicator for this status.
    pub fn indicator(&self) -> &'static str {
        match self {
            Self::Active => "●",
            Self::Idle => "○",
            Self::Disconnected => "○",
        }
    }
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Idle => write!(f, "idle"),
            Self::Disconnected => write!(f, "disconnected"),
        }
    }
}

/// Information about a connected user session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSession {
    /// Unique session ID
    pub session_id: String,
    /// User identifier (username, email, etc.)
    pub user_id: String,
    /// Display name for the user
    pub display_name: String,
    /// User role
    pub role: UserRole,
    /// Current session status
    pub status: SessionStatus,
    /// When the session was created
    pub connected_at: DateTime<Utc>,
    /// Last activity timestamp
    pub last_activity: DateTime<Utc>,
    /// Current view the user is viewing
    pub current_view: Option<String>,
    /// Client information (hostname, etc.)
    pub client_info: Option<ClientInfo>,
}

/// Client information for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    /// Client hostname or IP
    pub hostname: String,
    /// Client application/version
    pub client: String,
    /// Terminal size (if available)
    pub terminal_size: Option<(u16, u16)>,
}

impl UserSession {
    /// Create a new user session.
    pub fn new(
        session_id: impl Into<String>,
        user_id: impl Into<String>,
        display_name: impl Into<String>,
        role: UserRole,
    ) -> Self {
        let now = Utc::now();
        Self {
            session_id: session_id.into(),
            user_id: user_id.into(),
            display_name: display_name.into(),
            role,
            status: SessionStatus::Active,
            connected_at: now,
            last_activity: now,
            current_view: None,
            client_info: None,
        }
    }

    /// Update the session's last activity timestamp.
    pub fn update_activity(&mut self) {
        self.last_activity = Utc::now();
        self.status = SessionStatus::Active;
    }

    /// Mark the session as idle based on timeout.
    pub fn check_idle(&mut self, idle_timeout_seconds: i64) {
        let idle_duration = Utc::now() - self.last_activity;
        if idle_duration.num_seconds() > idle_timeout_seconds {
            self.status = SessionStatus::Idle;
        }
    }

    /// Set the current view for this session.
    pub fn set_view(&mut self, view: impl Into<String>) {
        self.current_view = Some(view.into());
        self.update_activity();
    }

    /// Set client information.
    pub fn set_client_info(&mut self, info: ClientInfo) {
        self.client_info = Some(info);
    }

    /// Check if the session is stale (should be disconnected).
    pub fn is_stale(&self, stale_timeout_seconds: i64) -> bool {
        let idle_duration = Utc::now() - self.last_activity;
        idle_duration.num_seconds() > stale_timeout_seconds && self.status == SessionStatus::Disconnected
    }
}

/// Manager for user sessions in team collaboration mode.
#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, UserSession>>>,
    idle_timeout_seconds: i64,
    stale_timeout_seconds: i64,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    /// Create a new session manager with default timeouts.
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            idle_timeout_seconds: 300, // 5 minutes
            stale_timeout_seconds: 3600, // 1 hour
        }
    }

    /// Create a new session manager with custom timeouts.
    pub fn with_timeouts(idle_timeout_seconds: i64, stale_timeout_seconds: i64) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            idle_timeout_seconds,
            stale_timeout_seconds,
        }
    }

    /// Register a new user session.
    pub fn register_session(&self, session: UserSession) -> Result<()> {
        let mut sessions = self.sessions.lock().map_err(|e| {
            ForgeError::session_error(format!("failed to acquire lock: {}", e))
        })?;

        let session_id = session.session_id.clone();
        let user_id = session.user_id.clone();

        sessions.insert(session_id.clone(), session);

        info!(
            session_id = %session_id,
            user_id = %user_id,
            "Registered user session"
        );

        Ok(())
    }

    /// Unregister a session (user disconnects).
    pub fn unregister_session(&self, session_id: &str) -> Result<Option<UserSession>> {
        let mut sessions = self.sessions.lock().map_err(|e| {
            ForgeError::session_error(format!("failed to acquire lock: {}", e))
        })?;

        let session = sessions.remove(session_id);

        if let Some(ref s) = session {
            info!(
                session_id = %session_id,
                user_id = %s.user_id,
                "Unregistered user session"
            );
        }

        Ok(session)
    }

    /// Update activity for a session.
    pub fn update_activity(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().map_err(|e| {
            ForgeError::session_error(format!("failed to acquire lock: {}", e))
        })?;

        if let Some(session) = sessions.get_mut(session_id) {
            session.update_activity();
        }

        Ok(())
    }

    /// Update the current view for a session.
    pub fn update_view(&self, session_id: &str, view: impl Into<String>) -> Result<()> {
        let mut sessions = self.sessions.lock().map_err(|e| {
            ForgeError::session_error(format!("failed to acquire lock: {}", e))
        })?;

        if let Some(session) = sessions.get_mut(session_id) {
            session.set_view(view);
        }

        Ok(())
    }

    /// Get all active sessions.
    pub fn get_sessions(&self) -> Result<Vec<UserSession>> {
        let sessions = self.sessions.lock().map_err(|e| {
            ForgeError::session_error(format!("failed to acquire lock: {}", e))
        })?;

        // Update idle status before returning
        let mut result = Vec::new();
        for session in sessions.values() {
            let mut s = session.clone();
            s.check_idle(self.idle_timeout_seconds);
            result.push(s);
        }

        Ok(result)
    }

    /// Get a specific session by ID.
    pub fn get_session(&self, session_id: &str) -> Result<Option<UserSession>> {
        let sessions = self.sessions.lock().map_err(|e| {
            ForgeError::session_error(format!("failed to acquire lock: {}", e))
        })?;

        Ok(sessions.get(session_id).cloned())
    }

    /// Get sessions for a specific user.
    pub fn get_user_sessions(&self, user_id: &str) -> Result<Vec<UserSession>> {
        let sessions = self.sessions.lock().map_err(|e| {
            ForgeError::session_error(format!("failed to acquire lock: {}", e))
        })?;

        let user_sessions: Vec<_> = sessions
            .values()
            .filter(|s| s.user_id == user_id)
            .cloned()
            .collect();

        Ok(user_sessions)
    }

    /// Check if a user has permission to perform an action.
    pub fn check_permission(&self, session_id: &str, action: SessionAction) -> Result<bool> {
        let session = self.get_session(session_id)?
            .ok_or_else(|| ForgeError::session_error(format!("session not found: {}", session_id)))?;

        Ok(match action {
            SessionAction::SpawnWorker => session.role.can_spawn_workers(),
            SessionAction::KillWorker => session.role.can_kill_workers(),
            SessionAction::ModifyConfig => session.role.can_modify_config(),
            SessionAction::AssignBead => session.role.can_assign_beads(),
            SessionAction::ManageSessions => session.role.can_manage_sessions(),
        })
    }

    /// Clean up stale sessions.
    pub fn cleanup_stale(&self) -> Result<usize> {
        let mut sessions = self.sessions.lock().map_err(|e| {
            ForgeError::session_error(format!("failed to acquire lock: {}", e))
        })?;

        let initial_count = sessions.len();
        sessions.retain(|_, session| !session.is_stale(self.stale_timeout_seconds));
        let removed = initial_count - sessions.len();

        if removed > 0 {
            info!(removed, "Cleaned up stale sessions");
        }

        Ok(removed)
    }

    /// Get session count.
    pub fn session_count(&self) -> Result<usize> {
        let sessions = self.sessions.lock().map_err(|e| {
            ForgeError::session_error(format!("failed to acquire lock: {}", e))
        })?;

        Ok(sessions.len())
    }

    /// Check if multi-user mode is active (any sessions).
    pub fn is_multi_user(&self) -> Result<bool> {
        Ok(self.session_count()? > 0)
    }
}

/// Actions that require permission checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    SpawnWorker,
    KillWorker,
    ModifyConfig,
    AssignBead,
    ManageSessions,
}

impl std::fmt::Display for SessionAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnWorker => write!(f, "spawn_worker"),
            Self::KillWorker => write!(f, "kill_worker"),
            Self::ModifyConfig => write!(f, "modify_config"),
            Self::AssignBead => write!(f, "assign_bead"),
            Self::ManageSessions => write!(f, "manage_sessions"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_role_permissions() {
        assert!(UserRole::Admin.can_spawn_workers());
        assert!(UserRole::Admin.can_kill_workers());
        assert!(UserRole::Admin.can_modify_config());
        assert!(UserRole::Admin.can_assign_beads());
        assert!(UserRole::Admin.can_manage_sessions());

        assert!(UserRole::Operator.can_spawn_workers());
        assert!(UserRole::Operator.can_kill_workers());
        assert!(!UserRole::Operator.can_modify_config());
        assert!(UserRole::Operator.can_assign_beads());
        assert!(!UserRole::Operator.can_manage_sessions());

        assert!(!UserRole::Viewer.can_spawn_workers());
        assert!(!UserRole::Viewer.can_kill_workers());
        assert!(!UserRole::Viewer.can_modify_config());
        assert!(!UserRole::Viewer.can_assign_beads());
        assert!(!UserRole::Viewer.can_manage_sessions());
    }

    #[test]
    fn test_user_role_from_str() {
        assert_eq!("admin".parse::<UserRole>().unwrap(), UserRole::Admin);
        assert_eq!("operator".parse::<UserRole>().unwrap(), UserRole::Operator);
        assert_eq!("viewer".parse::<UserRole>().unwrap(), UserRole::Viewer);
        assert!("unknown".parse::<UserRole>().is_err());
    }

    #[test]
    fn test_user_session_creation() {
        let session = UserSession::new("test-session", "user1", "Test User", UserRole::Operator);

        assert_eq!(session.session_id, "test-session");
        assert_eq!(session.user_id, "user1");
        assert_eq!(session.display_name, "Test User");
        assert_eq!(session.role, UserRole::Operator);
        assert_eq!(session.status, SessionStatus::Active);
    }

    #[test]
    fn test_session_manager() {
        let manager = SessionManager::new();

        let session = UserSession::new("s1", "user1", "User One", UserRole::Admin);
        manager.register_session(session).unwrap();

        assert_eq!(manager.session_count().unwrap(), 1);

        let sessions = manager.get_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].user_id, "user1");

        manager.unregister_session("s1").unwrap();
        assert_eq!(manager.session_count().unwrap(), 0);
    }

    #[test]
    fn test_permission_check() {
        let manager = SessionManager::new();

        let admin_session = UserSession::new("admin", "admin", "Admin", UserRole::Admin);
        let viewer_session = UserSession::new("viewer", "viewer", "Viewer", UserRole::Viewer);

        manager.register_session(admin_session).unwrap();
        manager.register_session(viewer_session).unwrap();

        assert!(manager.check_permission("admin", SessionAction::ModifyConfig).unwrap());
        assert!(!manager.check_permission("viewer", SessionAction::ModifyConfig).unwrap());
    }

    #[test]
    fn test_session_idle_check() {
        let mut session = UserSession::new("s1", "user1", "User", UserRole::Viewer);
        assert_eq!(session.status, SessionStatus::Active);

        // Set last activity to 10 minutes ago
        session.last_activity = Utc::now() - chrono::Duration::minutes(10);
        session.check_idle(300); // 5 minute timeout

        assert_eq!(session.status, SessionStatus::Idle);
    }

    #[test]
    fn test_session_view_update() {
        let manager = SessionManager::new();

        let session = UserSession::new("s1", "user1", "User", UserRole::Viewer);
        manager.register_session(session).unwrap();

        manager.update_view("s1", "Workers").unwrap();

        let retrieved = manager.get_session("s1").unwrap().unwrap();
        assert_eq!(retrieved.current_view, Some("Workers".to_string()));
    }
}
