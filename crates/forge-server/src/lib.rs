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

pub mod assignment;
pub mod auth;
pub mod cert_gen;
pub mod client;
pub mod oauth_auth;
pub mod protocol;
pub mod server_config;
pub mod session;
pub mod tls_validation;
pub mod websocket;

pub use assignment::BeadAssignmentTracker;
pub use auth::{AuthProvider, AuthResult, TestAuthProvider};
pub use oauth_auth::{OAuthAuthProvider, OAuthConfig, OAuthProvider};
pub use session::{SessionManager, SessionRegistry};

// SimpleAuth has been removed - use OAuthAuthProvider for all deployments
// For testing, use OAuthAuthProvider::with_defaults() which provides test credentials
pub use client::{ClientConfig, ClientStateSnapshot, ConnectedUser, ForgeClient};
pub use protocol::{ClientMessage, ServerMessage, ServerState, StateUpdate};
pub use server_config::{
    ServerYamlConfig, load_server_yaml_config, merge_config_with_cli_overrides,
};
pub use websocket::{ForgeServer, ServerConfig, TlsConfig, create_server};

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

    #[error("config load error: {0}")]
    ConfigLoadError(String),

    #[error("config parse error: {0}")]
    ConfigParseError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

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

    #[error(
        "domain mismatch: certificate is for '{cert_domain}' but server is configured for '{server_domain}'"
    )]
    DomainMismatch {
        cert_domain: String,
        server_domain: String,
    },

    #[error("certificate chain error: {0}")]
    CertificateChainError(String),

    #[error("key mismatch: private key does not match certificate")]
    KeyMismatch,

    #[error("TLS validation failed: {0}")]
    TlsValidationFailed(String),

    /// A TLS handshake with a peer failed mid-negotiation. Raised by
    /// [`ForgeClient`] when the connector is valid but the exchange with the
    /// server did not complete.
    #[error("TLS handshake error: {0}")]
    TlsHandshakeError(String),

    /// The TLS connector could not be built at all (bad config, no root
    /// store). Raised by [`ForgeClient`] before any connection is attempted.
    #[error("TLS configuration error: {0}")]
    TlsConfigurationError(String),
}

impl From<ServerError> for ForgeError {
    fn from(err: ServerError) -> Self {
        ForgeError::Internal {
            message: err.to_string(),
        }
    }
}
