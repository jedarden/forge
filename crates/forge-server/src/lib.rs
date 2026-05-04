//! FORGE Server - Team collaboration and multi-user session support.
//!
//! This crate provides:
//! - Session management for multiple connected users
//! - Role-based access control (RBAC)
//! - WebSocket server for real-time updates
//! - HTTP API for FORGE operations
//! - Bead assignment tracking
//!
//! ## Architecture
//!
//! The server runs alongside the FORGE TUI, enabling:
//! - Multiple users to observe the same FORGE instance
//! - Named user sessions with attribution on actions
//! - Shared bead queue with assignment capabilities
//! - Real-time state synchronization via WebSocket

pub mod auth;
pub mod session;
pub mod websocket;
pub mod assignment;
pub mod protocol;
pub mod client;

pub use session::{SessionManager, SessionRegistry};
pub use assignment::BeadAssignmentTracker;
pub use auth::{AuthProvider, AuthResult, SimpleAuth};
pub use protocol::{ServerMessage, ClientMessage, StateUpdate, ServerState};
pub use websocket::{ForgeServer, ServerConfig, create_server};
pub use client::{ForgeClient, ClientConfig, ClientStateSnapshot, ConnectedUser};

use forge_core::ForgeError;

/// FORGE server error type.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("server error: {0}")]
    ServerError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<ServerError> for ForgeError {
    fn from(err: ServerError) -> Self {
        ForgeError::Server { message: err.to_string() }
    }
}
