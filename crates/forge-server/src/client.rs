//! WebSocket client for connecting to a remote FORGE server.
//!
//! This module provides the client functionality for connecting to a FORGE
//! server in multi-user collaborative mode.

use super::protocol::{ServerMessage, ClientMessage, StateUpdate, ServerInfo};
use super::ServerError;
use forge_core::{UserSession, UserRole};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{debug, info, warn, error};
use chrono::Utc;

/// Configuration for connecting to a FORGE server.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Server WebSocket URL (e.g., ws://localhost:8080/ws)
    pub server_url: String,
    /// User ID for authentication
    pub user_id: String,
    /// Password for authentication
    pub password: String,
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
    pub async fn connect_and_run(&self) -> Result<(), ServerError> {
        let url = self.config.server_url.clone();
        info!("Connecting to FORGE server: {}", url);

        let (ws_stream, _) = connect_async(&url)
            .await
            .map_err(|e| ServerError::ServerError(format!("Connection failed: {}", e)))?;

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
                return Err(ServerError::ServerError("Server closed connection".to_string()));
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
            ServerMessage::Welcome { session, server_info } => {
                let mut state = self.current_state.write().await;
                state.session = Some(session.clone());
                state.server_info = Some(server_info.clone());
                state.authenticated = true;
                drop(state);

                info!("Authenticated as {} ({})", session.display_name, session.role);

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
            ServerMessage::UserJoined { ref user, ref display_name, role } => {
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
            ServerMessage::BeadAssigned { ref bead_id, ref assigned_to, ref assigned_by } => {
                info!("Bead {} assigned to {} by {}", bead_id, assigned_to, assigned_by);
                let _ = self.state_tx.send(msg);
            }
            ServerMessage::WorkerChanged { ref worker_id, status } => {
                debug!("Worker {} status changed to {:?}", worker_id, status);
                let _ = self.state_tx.send(msg);
            }
            ServerMessage::BeadChanged { ref bead_id, status } => {
                debug!("Bead {} status changed to {:?}", bead_id, status);
                let _ = self.state_tx.send(msg);
            }
            ServerMessage::ChatMessage { ref from, ref message, timestamp: _ } => {
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
            Err(ServerError::ServerError("Connection not established".to_string()))
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
        }).await;
    }

    /// Unassign a bead.
    pub async fn unassign_bead(&self, bead_id: impl Into<String>) {
        self.send_message(ClientMessage::UnassignBead {
            bead_id: bead_id.into(),
        }).await;
    }

    /// Spawn a worker.
    pub async fn spawn_worker(&self, model: impl Into<String>, count: u32) {
        self.send_message(ClientMessage::SpawnWorker {
            model: model.into(),
            count,
        }).await;
    }

    /// Kill a worker.
    pub async fn kill_worker(&self, worker_id: impl Into<String>) {
        self.send_message(ClientMessage::KillWorker {
            worker_id: worker_id.into(),
        }).await;
    }

    /// Change bead status.
    pub async fn change_bead_status(&self, bead_id: impl Into<String>, status: forge_core::BeadStatus) {
        self.send_message(ClientMessage::ChangeBeadStatus {
            bead_id: bead_id.into(),
            status,
        }).await;
    }

    /// Send a chat message.
    pub async fn send_chat(&self, message: impl Into<String>) {
        self.send_message(ClientMessage::ChatMessage {
            message: message.into(),
        }).await;
    }

    /// Update current view.
    pub async fn update_view(&self, view: impl Into<String>) {
        self.send_message(ClientMessage::UpdateView {
            view: view.into(),
        }).await;
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
    fn test_client_config() {
        let config = ClientConfig {
            server_url: "ws://localhost:8080/ws".to_string(),
            user_id: "testuser".to_string(),
            password: "testpass".to_string(),
        };

        assert_eq!(config.server_url, "ws://localhost:8080/ws");
        assert_eq!(config.user_id, "testuser");
        assert_eq!(config.password, "testpass");
    }
}
