//! FORGE Server - Team collaboration and multi-user session support.
//!
//! This crate provides:
//! - Session management for multiple connected users
//! - Role-based access control (RBAC)
//! - OAuth2 authentication (GitHub, Google, GitLab)
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
//!
//! ## Authentication
//!
//! **Production deployments should use OAuth2 authentication** via `OAuthAuthProvider`.
//! The server supports OAuth2 with multiple providers:
//! - GitHub OAuth (recommended for development)
//! - Google OAuth
//! - GitLab OAuth
//!
//! Configuration is loaded from `~/.forge/oauth.yaml` (or your FORGE config directory).
//!
//! ### Example OAuth Configuration
//!
//! ```yaml
//! provider: GitHub  # GitHub, Google, or GitLab
//! client_id: "your_oauth_client_id"
//! client_secret: "your_oauth_client_secret"  # Optional for token validation
//! user_roles:
//!   "github_username": "Admin"  # Admin, Operator, or Viewer
//! display_names:
//!   "github_username": "Full Name"
//! ```
//!
//! For testing purposes, `SimpleAuth` is available but deprecated for production use.

pub mod auth;
pub mod session;
pub mod websocket;
pub mod assignment;
pub mod protocol;
pub mod client;
pub mod oauth_auth;
pub mod tls_validation;
pub mod cert_gen;

pub use session::{SessionManager, SessionRegistry};
pub use assignment::BeadAssignmentTracker;
pub use auth::{AuthProvider, AuthResult};
pub use oauth_auth::{OAuthAuthProvider, OAuthConfig, OAuthProvider};

// SimpleAuth is kept for backward compatibility and testing only
// Use OAuthAuthProvider for production deployments
#[deprecated(note = "Use OAuthAuthProvider for production. SimpleAuth is for testing only.")]
pub use auth::SimpleAuth;
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

    // TLS-specific errors with detailed context
    #[error("certificate load error: failed to load certificate from '{0}': {1}")]
    CertificateLoadError(String, String),

    #[error("private key load error: failed to load private key from '{0}': {1}")]
    PrivateKeyLoadError(String, String),

    #[error("invalid PEM format: {0}")]
    InvalidPemFormat(String),

    #[error("expired certificate: certificate expires on {0} ({1} days ago)")]
    ExpiredCertificate(String, i64),

    #[error("certificate expiring soon: certificate expires on {0} ({1} days remaining)")]
    CertificateExpiringSoon(String, i64),

    #[error("domain mismatch: certificate is for '{cert_domain}' but server is configured for '{server_domain}'")]
    DomainMismatch { cert_domain: String, server_domain: String },

    #[error("certificate chain error: {0}")]
    CertificateChainError(String),

    #[error("key mismatch: private key does not match certificate")]
    KeyMismatch,

    #[error("TLS validation failed: {0}")]
    TlsValidationFailed(String),
}

impl From<ServerError> for ForgeError {
    fn from(err: ServerError) -> Self {
        ForgeError::Internal { message: err.to_string() }
    }
}
