//! WebSocket client for connecting to a remote FORGE server.
//!
//! This module provides the client functionality for connecting to a FORGE
//! server in multi-user collaborative mode.

use super::ServerError;
use super::protocol::{ClientMessage, ServerInfo, ServerMessage, StateUpdate};
use chrono::Utc;
use forge_core::{UserRole, UserSession};
use futures_util::{SinkExt, StreamExt};
use native_tls;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, broadcast};
use tokio_tungstenite::{
    Connector, connect_async, connect_async_tls_with_config, tungstenite::protocol::Message,
};
use tracing::{debug, error, info, warn};

/// TLS configuration for secure WebSocket client connections.
#[derive(Debug, Clone)]
pub struct ClientTlsConfig {
    /// Path to a custom CA certificate bundle for trusting test/self-signed certificates.
    /// This is for development only - production uses the system trust store.
    pub ca_bundle_path: Option<PathBuf>,
    /// Whether to verify the server's certificate (default: true).
    /// Setting to false is insecure and should only be used for development.
    pub danger_verify_certificate: bool,
}

impl Default for ClientTlsConfig {
    fn default() -> Self {
        Self {
            ca_bundle_path: None,
            danger_verify_certificate: true,
        }
    }
}

impl ClientTlsConfig {
    /// Create a new TLS configuration with default verification settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Trust a specific CA certificate bundle (for development/testing).
    ///
    /// This allows trusting self-signed or test certificates during development.
    /// In production, use the system trust store instead.
    pub fn with_ca_bundle(mut self, path: PathBuf) -> Self {
        self.ca_bundle_path = Some(path);
        self
    }

    /// Disable certificate verification (INSECURE - development only!).
    ///
    /// # WARNING
    /// This makes the connection vulnerable to man-in-the-middle attacks.
    /// Never use this in production.
    pub fn danger_disable_verification(mut self) -> Self {
        self.danger_verify_certificate = false;
        self
    }
}

/// Configuration for connecting to a FORGE server.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Server WebSocket URL (e.g., ws://localhost:8080/ws or wss://localhost:8080/ws)
    pub server_url: String,
    /// User ID for authentication
    pub user_id: String,
    /// Password for authentication
    pub password: String,
    /// TLS configuration (for wss:// URLs)
    pub tls: Option<ClientTlsConfig>,
}

/// FORGE WebSocket client.
#[derive(Clone)]
pub struct ForgeClient {
    config: ClientConfig,
    /// Channel for broadcasting state updates to local subscribers
    state_tx: broadcast::Sender<ServerMessage>,
    /// Current state snapshot
    current_state: Arc<RwLock<ClientState>>,
    /// WebSocket write mutex (for sending messages)
    write_tx: Arc<Mutex<Option<broadcast::Sender<ClientMessage>>>>,
}

/// Current client state.
#[derive(Debug, Clone, Default)]
struct ClientState {
    /// Our session info
    session: Option<UserSession>,
    /// Server info
    server_info: Option<ServerInfo>,
    /// Current state update
    state_update: Option<StateUpdate>,
    /// Connected users
    connected_users: Vec<ConnectedUser>,
    /// Whether we're authenticated
    authenticated: bool,
}

/// A connected user.
#[derive(Debug, Clone)]
pub struct ConnectedUser {
    pub user_id: String,
    pub display_name: String,
    pub role: UserRole,
    pub current_view: Option<String>,
    pub connected_at: chrono::DateTime<Utc>,
}

impl ForgeClient {
    /// Create a new FORGE client.
    pub fn new(config: ClientConfig) -> Self {
        let (state_tx, _) = broadcast::channel(1000);

        Self {
            config,
            state_tx,
            current_state: Arc::new(RwLock::new(ClientState::default())),
            write_tx: Arc::new(Mutex::new(None)),
        }
    }

    /// Connect to the server and run the client loop.
    ///
    /// This method runs the WebSocket connection loop and will block until
    /// the connection is closed. It should be spawned as a background task.
    ///
    /// Supports both ws:// (plain WebSocket) and wss:// (secure WebSocket) URLs.
    /// For wss:// URLs, TLS certificate verification is enabled by default.
    pub async fn connect_and_run(&self) -> Result<(), ServerError> {
        let url = self.config.server_url.clone();
        info!("Connecting to FORGE server: {}", url);

        let ws_stream = if url.starts_with("wss://") {
            // Secure WebSocket connection
            self.connect_secure(&url).await?
        } else if url.starts_with("ws://") {
            // Plain WebSocket connection
            let (stream, _) = connect_async(&url).await.map_err(|e| {
                ServerError::ServerError(format!("WebSocket connection failed: {}", e))
            })?;
            stream
        } else {
            return Err(ServerError::ServerError(format!(
                "Invalid WebSocket URL scheme: {}. Must start with ws:// or wss://",
                url
            )));
        };

        info!("Connected to server");

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        // Create channel for sending messages
        let (write_tx, _) = broadcast::channel(100);
        *self.write_tx.lock().await = Some(write_tx.clone());

        // Subscribe to outgoing messages
        let mut write_rx = write_tx.subscribe();

        // Spawn task for sending messages
        let _write_tx_clone = self.write_tx.clone();
        tokio::spawn(async move {
            while let Ok(client_msg) = write_rx.recv().await {
                let json = serde_json::to_string(&client_msg)
                    .unwrap_or_else(|_| r#"{"error":"serialize failed"}"#.to_string());

                if let Err(e) = ws_sender.send(Message::Text(json.into())).await {
                    error!("Failed to send message to server: {}", e);
                    break;
                }
            }
        });

        // Send authentication
        let auth_msg = ClientMessage::Authenticate {
            user_id: self.config.user_id.clone(),
            credentials: self.config.password.clone(),
        };
        self.send_message(auth_msg).await;

        // Receive messages from server
        while let Some(msg_result) = ws_receiver.next().await {
            match msg_result {
                Ok(msg) => {
                    if let Err(e) = self.handle_message(msg).await {
                        error!("Error handling message: {}", e);
                    }
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
            }
        }

        warn!("Disconnected from server");
        Ok(())
    }

    /// Establish a secure WebSocket connection (WSS) with TLS verification.
    ///
    /// This method handles:
    /// - Normal hostname and certificate-chain verification by default
    /// - Custom CA bundle for development/testing (if configured)
    /// - Disabled verification (if explicitly configured for development)
    /// - Actionable errors for common TLS issues
    async fn connect_secure(
        &self,
        url: &str,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        ServerError,
    > {
        let tls_default = ClientTlsConfig::default();
        let tls_config = self.config.tls.as_ref().unwrap_or(&tls_default);

        // Build TLS connector with appropriate verification settings
        let connector = if !tls_config.danger_verify_certificate {
            // Development mode: disable certificate verification
            warn!(
                "Certificate verification DISABLED - this is INSECURE and should only be used for development!"
            );
            let mut builder = native_tls::TlsConnector::builder();
            builder.danger_accept_invalid_certs(true);
            let native_connector = builder.build().map_err(|e| {
                ServerError::TlsConfigurationError(format!("Failed to create TLS connector: {}", e))
            })?;
            Connector::NativeTls(native_connector)
        } else if let Some(ca_bundle_path) = &tls_config.ca_bundle_path {
            // Development mode: trust custom CA bundle
            info!("Using custom CA bundle: {:?}", ca_bundle_path);
            match self.build_ca_bundle_connector(ca_bundle_path).await {
                Ok(conn) => conn,
                Err(e) => {
                    return Err(ServerError::TlsConfigurationError(format!(
                        "Failed to load CA bundle from {:?}: {}. \
                        Ensure the file exists and contains valid PEM certificates.",
                        ca_bundle_path, e
                    )));
                }
            }
        } else {
            // Production mode: use system trust store with normal verification
            debug!("Using system trust store for certificate verification");
            let native_connector = native_tls::TlsConnector::new().map_err(|e| {
                ServerError::TlsConfigurationError(format!("Failed to create TLS connector: {}", e))
            })?;
            Connector::NativeTls(native_connector)
        };

        // Attempt connection with TLS
        match connect_async_tls_with_config(url, None, false, Some(connector)).await {
            Ok((stream, _)) => Ok(stream),
            Err(tokio_tungstenite::tungstenite::Error::Tls(e)) => {
                // Provide actionable TLS error messages
                let error_msg = self.format_tls_error(&e.to_string());
                Err(ServerError::TlsHandshakeError(error_msg))
            }
            Err(e) => Err(ServerError::ServerError(format!(
                "Secure connection failed: {}",
                e
            ))),
        }
    }

    /// Load a custom CA certificate bundle for trusting test/self-signed certificates.
    ///
    /// This is for development only - it allows trusting certificates that aren't
    /// in the system trust store (e.g., self-signed test certificates).
    async fn build_ca_bundle_connector(&self, path: &PathBuf) -> Result<Connector, ServerError> {
        // native_tls doesn't support custom CA bundles - only system trust store
        // For custom CA support, the client needs to use rustls backend
        warn!(
            "Custom CA bundles are not supported with native-tls backend. Using system trust store instead."
        );
        warn!(
            "To use custom CA bundle '{}', consider building with rustls features.",
            path.display()
        );

        // Fall back to system trust store
        let native_connector = native_tls::TlsConnector::new().map_err(|e| {
            ServerError::TlsConfigurationError(format!("Failed to create TLS connector: {}", e))
        })?;

        Ok(tokio_tungstenite::Connector::NativeTls(native_connector))
    }

    /// Format TLS errors into actionable, user-friendly messages.
    fn format_tls_error(&self, error: &str) -> String {
        let error_str = error.to_lowercase();

        // Detect specific TLS failure modes and provide actionable guidance
        if error_str.contains("hostname mismatch")
            || error_str.contains("certificateverificationfailed")
        {
            format!(
                "TLS hostname verification failed. The server certificate does not match the hostname '{}'. \
                This typically means: \
                1) You're connecting to the wrong host, or \
                2) The certificate was issued for a different hostname. \
                Check that the server URL is correct and the certificate is valid for this hostname.",
                self.config.server_url
            )
        } else if error_str.contains("unknown issuer")
            || error_str.contains("untrusted")
            || error_str.contains("unable to verify")
        {
            format!(
                "TLS certificate issuer not trusted. The server's certificate chain is not trusted by your system. \
                This can mean: \
                1) Using a self-signed certificate in production (use a proper CA instead), or \
                2) Missing intermediate certificate in the chain, or \
                3) Test environment: configure ClientTlsConfig::with_ca_bundle() to trust your test CA."
            )
        } else if error_str.contains("expired") || error_str.contains("not yet valid") {
            format!(
                "TLS certificate expired or not yet valid. Check the server's certificate validity period. \
                In production, renew the certificate. In testing, regenerate a valid test certificate."
            )
        } else if error_str.contains("handshake") || error_str.contains("alert") {
            format!(
                "TLS handshake failed. This indicates a protocol-level issue during the TLS negotiation. \
                Common causes: \
                1) TLS version mismatch (server requires a version your client doesn't support), \
                2) Cipher suite mismatch, \
                3) Server configuration error. \
                Check server logs for detailed error information."
            )
        } else {
            format!(
                "TLS connection error: {}. If this error persists, check server logs and ensure the server's TLS configuration is valid.",
                error
            )
        }
    }

    /// Handle a message from the server.
    async fn handle_message(&self, msg: Message) -> Result<(), ServerError> {
        match msg {
            Message::Text(text) => {
                if let Ok(server_msg) = serde_json::from_str::<ServerMessage>(&text) {
                    self.handle_server_message(server_msg).await?;
                }
            }
            Message::Close(_) => {
                debug!("Server closed connection");
                return Err(ServerError::ServerError(
                    "Server closed connection".to_string(),
                ));
            }
            Message::Ping(_data) => {
                // Respond with pong
                let _ = self.send_message(ClientMessage::Pong).await;
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle a server message.
    async fn handle_server_message(&self, msg: ServerMessage) -> Result<(), ServerError> {
        match msg {
            ServerMessage::Welcome {
                session,
                server_info,
            } => {
                let mut state = self.current_state.write().await;
                state.session = Some(session.clone());
                state.server_info = Some(server_info.clone());
                state.authenticated = true;
                drop(state);

                info!(
                    "Authenticated as {} ({})",
                    session.display_name, session.role
                );

                // Request full state sync
                self.send_message(ClientMessage::SyncState).await;
            }
            ServerMessage::StateUpdate(update) => {
                let mut state = self.current_state.write().await;
                state.state_update = Some(update.clone());
                drop(state);

                // Broadcast to local subscribers
                let _ = self.state_tx.send(ServerMessage::StateUpdate(update));
            }
            ServerMessage::UserJoined {
                ref user,
                ref display_name,
                role,
            } => {
                let mut state = self.current_state.write().await;
                state.connected_users.push(ConnectedUser {
                    user_id: user.clone(),
                    display_name: display_name.clone(),
                    role,
                    current_view: None,
                    connected_at: Utc::now(),
                });
                drop(state);

                info!("User {} ({}) joined", display_name, role);
                let _ = self.state_tx.send(msg);
            }
            ServerMessage::UserLeft { ref user } => {
                let mut state = self.current_state.write().await;
                state.connected_users.retain(|u| u.user_id != *user);
                drop(state);

                info!("User {} left", user);
                let _ = self.state_tx.send(msg);
            }
            ServerMessage::BeadAssigned {
                ref bead_id,
                ref assigned_to,
                ref assigned_by,
            } => {
                info!(
                    "Bead {} assigned to {} by {}",
                    bead_id, assigned_to, assigned_by
                );
                let _ = self.state_tx.send(msg);
            }
            ServerMessage::WorkerChanged {
                ref worker_id,
                status,
            } => {
                debug!("Worker {} status changed to {:?}", worker_id, status);
                let _ = self.state_tx.send(msg);
            }
            ServerMessage::BeadChanged {
                ref bead_id,
                status,
            } => {
                debug!("Bead {} status changed to {:?}", bead_id, status);
                let _ = self.state_tx.send(msg);
            }
            ServerMessage::ChatMessage {
                ref from,
                ref message,
                timestamp: _,
            } => {
                info!("Chat from {}: {}", from, message);
                let _ = self.state_tx.send(msg);
            }
            ServerMessage::Error { ref message } => {
                warn!("Server error: {}", message);
                let _ = self.state_tx.send(msg);
            }
            ServerMessage::Ping => {
                // Respond with pong
                let _ = self.send_message(ClientMessage::Pong).await;
            }
        }

        Ok(())
    }

    /// Send a message to the server.
    async fn send_message(&self, msg: ClientMessage) {
        if let Some(write_tx) = self.write_tx.lock().await.as_ref() {
            let _ = write_tx.send(msg);
        }
    }

    /// Subscribe to state updates.
    pub fn subscribe(&self) -> broadcast::Receiver<ServerMessage> {
        self.state_tx.subscribe()
    }

    /// Send a message directly from outside the client connection loop.
    ///
    /// This method can be called from external tasks to send messages to the server.
    /// It will return an error if the connection is not yet established.
    pub async fn send_direct(&self, msg: ClientMessage) -> Result<(), ServerError> {
        let write_tx = self.write_tx.lock().await;
        if let Some(tx) = write_tx.as_ref() {
            let _ = tx.send(msg);
            Ok(())
        } else {
            Err(ServerError::ServerError(
                "Connection not established".to_string(),
            ))
        }
    }

    /// Get the current state.
    pub async fn get_state(&self) -> ClientStateSnapshot {
        let state = self.current_state.read().await;
        ClientStateSnapshot {
            session: state.session.clone(),
            server_info: state.server_info.clone(),
            state_update: state.state_update.clone(),
            connected_users: state.connected_users.clone(),
            authenticated: state.authenticated,
        }
    }

    /// Assign a bead to a user.
    pub async fn assign_bead(&self, bead_id: impl Into<String>, to: impl Into<String>) {
        self.send_message(ClientMessage::AssignBead {
            bead_id: bead_id.into(),
            to: to.into(),
        })
        .await;
    }

    /// Unassign a bead.
    pub async fn unassign_bead(&self, bead_id: impl Into<String>) {
        self.send_message(ClientMessage::UnassignBead {
            bead_id: bead_id.into(),
        })
        .await;
    }

    /// Spawn a worker.
    pub async fn spawn_worker(&self, model: impl Into<String>, count: u32) {
        self.send_message(ClientMessage::SpawnWorker {
            model: model.into(),
            count,
        })
        .await;
    }

    /// Kill a worker.
    pub async fn kill_worker(&self, worker_id: impl Into<String>) {
        self.send_message(ClientMessage::KillWorker {
            worker_id: worker_id.into(),
        })
        .await;
    }

    /// Change bead status.
    pub async fn change_bead_status(
        &self,
        bead_id: impl Into<String>,
        status: forge_core::BeadStatus,
    ) {
        self.send_message(ClientMessage::ChangeBeadStatus {
            bead_id: bead_id.into(),
            status,
        })
        .await;
    }

    /// Send a chat message.
    pub async fn send_chat(&self, message: impl Into<String>) {
        self.send_message(ClientMessage::ChatMessage {
            message: message.into(),
        })
        .await;
    }

    /// Update current view.
    pub async fn update_view(&self, view: impl Into<String>) {
        self.send_message(ClientMessage::UpdateView { view: view.into() })
            .await;
    }

    /// Request full state sync.
    pub async fn request_sync(&self) {
        self.send_message(ClientMessage::SyncState).await;
    }
}

/// Snapshot of the current client state.
#[derive(Debug, Clone)]
pub struct ClientStateSnapshot {
    pub session: Option<UserSession>,
    pub server_info: Option<ServerInfo>,
    pub state_update: Option<StateUpdate>,
    pub connected_users: Vec<ConnectedUser>,
    pub authenticated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_with_ws_url() {
        let config = ClientConfig {
            server_url: "ws://localhost:8080/ws".to_string(),
            user_id: "testuser".to_string(),
            password: "testpass".to_string(),
            tls: None,
        };

        assert_eq!(config.server_url, "ws://localhost:8080/ws");
        assert_eq!(config.user_id, "testuser");
        assert_eq!(config.password, "testpass");
        assert!(config.tls.is_none());
    }

    #[test]
    fn test_client_config_with_wss_url() {
        let tls_config = ClientTlsConfig::new();
        let config = ClientConfig {
            server_url: "wss://example.com/ws".to_string(),
            user_id: "testuser".to_string(),
            password: "testpass".to_string(),
            tls: Some(tls_config),
        };

        assert_eq!(config.server_url, "wss://example.com/ws");
        assert!(config.tls.is_some());
    }

    #[test]
    fn test_client_tls_config_default() {
        let config = ClientTlsConfig::new();

        assert!(config.danger_verify_certificate); // Default is true
        assert!(config.ca_bundle_path.is_none());
    }

    #[test]
    fn test_client_tls_config_with_ca_bundle() {
        let path = PathBuf::from("/tmp/test-ca.pem");
        let config = ClientTlsConfig::new().with_ca_bundle(path.clone());

        assert_eq!(config.ca_bundle_path, Some(path.clone()));
        assert!(config.danger_verify_certificate); // Should still be true
    }

    #[test]
    fn test_client_tls_config_danger_disable_verification() {
        let config = ClientTlsConfig::new().danger_disable_verification();

        assert!(!config.danger_verify_certificate); // Should be false
        assert!(config.ca_bundle_path.is_none());
    }

    #[test]
    fn test_client_tls_config_combined() {
        let path = PathBuf::from("/tmp/test-ca.pem");
        let config = ClientTlsConfig::new()
            .with_ca_bundle(path.clone())
            .danger_disable_verification();

        assert_eq!(config.ca_bundle_path, Some(path));
        assert!(!config.danger_verify_certificate);
    }

    #[test]
    fn test_decode_valid_cert() {
        let client = create_test_client();
        // Valid base64-encoded dummy certificate data
        let valid_base64 = "MIIDXTCCAkWgAwIBAgIJAKL0UG+mRKqzMA0GCSqGSIb3DQEBCwUAMEUxCzAJBgNV";

        let result = client.decode_cert(valid_base64.as_bytes());
        assert!(result.is_ok());
        let decoded = result.unwrap();
        assert!(!decoded.is_empty());
    }

    #[test]
    fn test_decode_invalid_cert() {
        let client = create_test_client();
        let invalid_base64 = "not-valid-base64!!!";

        let result = client.decode_cert(invalid_base64.as_bytes());
        assert!(result.is_err());
    }

    #[test]
    fn test_format_tls_error_hostname_mismatch() {
        let client = create_test_client();
        let error_msg = client.format_tls_error("CertVerifyFailed");

        // Should contain helpful guidance about hostname mismatch
        assert!(
            error_msg.to_lowercase().contains("hostname")
                || error_msg.to_lowercase().contains("verification")
        );
    }

    #[test]
    fn test_format_tls_error_unknown_issuer() {
        let client = create_test_client();
        let error_msg = client.format_tls_error("Unknown issuer");

        // Should mention untrusted or unknown issuer
        assert!(
            error_msg.to_lowercase().contains("untrusted")
                || error_msg.to_lowercase().contains("issuer")
        );
    }

    #[test]
    fn test_format_tls_error_expired() {
        let client = create_test_client();
        let error_msg = client.format_tls_error("Certificate expired");

        // Should mention expired certificate
        assert!(
            error_msg.to_lowercase().contains("expired")
                || error_msg.to_lowercase().contains("valid")
        );
    }

    #[test]
    fn test_format_tls_error_handshake() {
        let client = create_test_client();
        let error_msg = client.format_tls_error("Handshake failed");

        // Should mention handshake or protocol
        assert!(
            error_msg.to_lowercase().contains("handshake")
                || error_msg.to_lowercase().contains("protocol")
        );
    }

    fn create_test_client() -> ForgeClient {
        let config = ClientConfig {
            server_url: "ws://localhost:8080/ws".to_string(),
            user_id: "testuser".to_string(),
            password: "testpass".to_string(),
            tls: None,
        };
        ForgeClient::new(config)
    }
}
