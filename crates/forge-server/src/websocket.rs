//! WebSocket server for FORGE multi-user real-time updates.
//!
//! Provides a WebSocket server that clients can connect to for real-time
//! state synchronization and collaborative features.

use super::protocol::{ServerMessage, ClientMessage, ServerInfo, StateUpdate, WorkerState, BeadState, CostState};
use super::session::SessionRegistry;
use super::assignment::BeadAssignmentTracker;
use super::auth::AuthProvider;
use crate::ServerError;
use forge_core::{WorkerStatus, BeadStatus, Priority, audit::{AuditLogger, AuditEvent, EventType}};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use tracing::{debug, info, warn, error};
use chrono::Utc;

/// Configuration for the FORGE server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_address: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".to_string(),
            port: 8080,
        }
    }
}

/// FORGE WebSocket server.
pub struct ForgeServer {
    config: ServerConfig,
    auth: Arc<dyn AuthProvider>,
    session_registry: SessionRegistry,
    assignment_tracker: BeadAssignmentTracker,
    state_broadcast: broadcast::Sender<ServerMessage>,
    current_state: Arc<RwLock<ServerStateSnapshot>>,
    /// Whether this server is running
    running: Arc<RwLock<bool>>,
    /// Audit logger for compliance tracking
    audit_logger: Option<Arc<AuditLogger>>,
}

/// Snapshot of current server state for broadcasting.
#[derive(Debug, Clone)]
pub struct ServerStateSnapshot {
    workers: Vec<WorkerSnapshot>,
    beads: Vec<BeadSnapshot>,
    costs: CostSnapshot,
}

#[derive(Debug, Clone)]
struct WorkerSnapshot {
    worker_id: String,
    model: String,
    status: WorkerStatus,
    current_task: Option<String>,
}

#[derive(Debug, Clone)]
struct BeadSnapshot {
    bead_id: String,
    title: String,
    status: BeadStatus,
    assigned_to: Option<String>,
}

#[derive(Debug, Clone)]
struct CostSnapshot {
    today_cost: f64,
    week_cost: f64,
    month_cost: f64,
}

impl ForgeServer {
    /// Create a new FORGE server.
    pub fn new(
        config: ServerConfig,
        auth: Arc<dyn AuthProvider>,
    ) -> Self {
        let (tx, _) = broadcast::channel(1000);

        Self {
            config,
            auth,
            session_registry: SessionRegistry::new(),
            assignment_tracker: BeadAssignmentTracker::new(),
            state_broadcast: tx,
            current_state: Arc::new(RwLock::new(ServerStateSnapshot {
                workers: Vec::new(),
                beads: Vec::new(),
                costs: CostSnapshot {
                    today_cost: 0.0,
                    week_cost: 0.0,
                    month_cost: 0.0,
                },
            })),
            running: Arc::new(RwLock::new(false)),
            audit_logger: None,
        }
    }

    /// Create a new FORGE server with audit logging.
    pub fn with_audit_logger(
        config: ServerConfig,
        auth: Arc<dyn AuthProvider>,
        audit_logger: Arc<AuditLogger>,
    ) -> Self {
        let (tx, _) = broadcast::channel(1000);

        Self {
            config,
            auth,
            session_registry: SessionRegistry::new(),
            assignment_tracker: BeadAssignmentTracker::new(),
            state_broadcast: tx,
            current_state: Arc::new(RwLock::new(ServerStateSnapshot {
                workers: Vec::new(),
                beads: Vec::new(),
                costs: CostSnapshot {
                    today_cost: 0.0,
                    week_cost: 0.0,
                    month_cost: 0.0,
                },
            })),
            running: Arc::new(RwLock::new(false)),
            audit_logger: Some(audit_logger),
        }
    }

    /// Get the session registry.
    pub fn session_registry(&self) -> &SessionRegistry {
        &self.session_registry
    }

    /// Get the assignment tracker.
    pub fn assignment_tracker(&self) -> &BeadAssignmentTracker {
        &self.assignment_tracker
    }

    /// Subscribe to state updates.
    pub fn subscribe(&self) -> broadcast::Receiver<ServerMessage> {
        self.state_broadcast.subscribe()
    }

    /// Broadcast a message to all connected clients.
    pub fn broadcast(&self, message: ServerMessage) {
        let _ = self.state_broadcast.send(message);
    }

    /// Update the current server state.
    pub async fn update_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ServerStateSnapshot) -> R,
    {
        let mut state = self.current_state.write().await;
        f(&mut state)
    }

    /// Update worker states and broadcast to clients.
    pub async fn update_workers(&self, workers: Vec<WorkerState>) {
        self.update_state(|state| {
            state.workers = workers.iter().map(|w| WorkerSnapshot {
                worker_id: w.worker_id.clone(),
                model: w.model.clone(),
                status: w.status,
                current_task: w.current_task.clone(),
            }).collect();
        }).await;

        self.broadcast(ServerMessage::StateUpdate(StateUpdate {
            timestamp: Utc::now(),
            workers: workers.clone(),
            beads: {
                let state = self.current_state.read().await;
                state.beads.iter().map(|b| BeadState {
                    bead_id: b.bead_id.clone(),
                    title: b.title.clone(),
                    status: b.status,
                    priority: Priority::P2, // Default priority
                    assigned_to: b.assigned_to.clone(),
                    created_at: Utc::now(),
                }).collect()
            },
            costs: {
                let state = self.current_state.read().await;
                CostState {
                    today_cost: state.costs.today_cost,
                    week_cost: state.costs.week_cost,
                    month_cost: state.costs.month_cost,
                }
            },
            sessions: Vec::new(), // Will be filled by broadcast
        }));
    }

    /// Update bead states and broadcast to clients.
    pub async fn update_beads(&self, beads: Vec<BeadState>) {
        self.update_state(|state| {
            state.beads = beads.iter().map(|b| BeadSnapshot {
                bead_id: b.bead_id.clone(),
                title: b.title.clone(),
                status: b.status,
                assigned_to: b.assigned_to.clone(),
            }).collect();
        }).await;

        // Broadcast individual bead changes
        for bead in &beads {
            self.broadcast(ServerMessage::BeadChanged {
                bead_id: bead.bead_id.clone(),
                status: bead.status,
            });
        }
    }

    /// Update cost state and broadcast to clients.
    pub async fn update_costs(&self, costs: CostState) {
        self.update_state(|state| {
            state.costs = CostSnapshot {
                today_cost: costs.today_cost,
                week_cost: costs.week_cost,
                month_cost: costs.month_cost,
            };
        }).await;
    }

    /// Broadcast full state update to all clients.
    pub async fn broadcast_full_state(&self) {
        let (workers, beads, costs, sessions) = {
            let state = self.current_state.read().await;
            let all_sessions = self.session_registry.manager().all_sessions().await;
            (
                state.workers.iter().map(|w| WorkerState {
                    worker_id: w.worker_id.clone(),
                    model: w.model.clone(),
                    status: w.status,
                    current_task: w.current_task.clone(),
                    started_at: None,
                }).collect::<Vec<_>>(),
                state.beads.iter().map(|b| BeadState {
                    bead_id: b.bead_id.clone(),
                    title: b.title.clone(),
                    status: b.status,
                    priority: Priority::P2,
                    assigned_to: b.assigned_to.clone(),
                    created_at: Utc::now(),
                }).collect::<Vec<_>>(),
                CostState {
                    today_cost: state.costs.today_cost,
                    week_cost: state.costs.week_cost,
                    month_cost: state.costs.month_cost,
                },
                all_sessions.into_iter().map(|s| crate::protocol::SessionSummary {
                    user_id: s.user_id.clone(),
                    display_name: s.display_name.clone(),
                    role: s.role,
                    current_view: s.current_view.clone(),
                    connected_at: s.connected_at,
                }).collect::<Vec<_>>(),
            )
        };

        self.broadcast(ServerMessage::StateUpdate(StateUpdate {
            timestamp: Utc::now(),
            workers,
            beads,
            costs,
            sessions,
        }));
    }

    /// Check if server is running.
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// Stop the server.
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }

    /// Start the server.
    pub async fn run(self) -> Result<(), ServerError> {
        // Set running flag
        {
            let mut running = self.running.write().await;
            *running = true;
        }

        let app = Router::new()
            .route("/ws", get(ws_handler))
            .route("/health", get(health_handler))
            .with_state(self.clone());

        let addr = format!("{}:{}", self.config.bind_address, self.config.port);
        info!("FORGE server listening on {}", addr);

        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| ServerError::Io(e))?;

        axum::serve(listener, app)
            .await
            .map_err(|e| ServerError::ServerError(e.to_string()))?;

        // Clear running flag on shutdown
        {
            let mut running = self.running.write().await;
            *running = false;
        }

        Ok(())
    }

    /// Handle a WebSocket connection.
    async fn handle_connection(
        self,
        socket: WebSocket,
        addr: SocketAddr,
    ) -> Result<(), ServerError> {
        let (mut sender, mut receiver) = socket.split();
        let mut session_id: Option<String> = None;
        let _rx = self.subscribe();

        // Send initial welcome after auth
        let mut authenticated = false;

        while let Some(result) = receiver.next().await {
            match result {
                Ok(msg) => {
                    match msg {
                        Message::Text(text) => {
                            if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                                match client_msg {
                                    ClientMessage::Authenticate { user_id, credentials } => {
                                        match self.auth.authenticate(&user_id, &credentials).await {
                                            Ok(auth_result) => {
                                                let session = self.session_registry.manager()
                                                    .create_session(
                                                        &user_id,
                                                        &auth_result.display_name,
                                                        auth_result.role,
                                                    )
                                                    .await?;

                                                session_id = Some(session.session_id.clone());

                                                let welcome = ServerMessage::Welcome {
                                                    session: session.clone(),
                                                    server_info: ServerInfo {
                                                        server_version: env!("CARGO_PKG_VERSION").to_string(),
                                                        connected_users: self.session_registry.manager().session_count().await,
                                                        active_workers: {
                                                            let state = self.current_state.read().await;
                                                            state.workers.len()
                                                        },
                                                        pending_beads: {
                                                            let state = self.current_state.read().await;
                                                            state.beads.iter().filter(|b| b.status == BeadStatus::Open).count()
                                                        },
                                                    },
                                                };

                                                let _ = sender.send(Message::Text(
                                                    serde_json::to_string(&welcome).unwrap().into()
                                                )).await;

                                                // Broadcast user joined
                                                self.broadcast(ServerMessage::UserJoined {
                                                    user: user_id.clone(),
                                                    display_name: auth_result.display_name,
                                                    role: auth_result.role,
                                                });

                                                authenticated = true;
                                                info!("User {} authenticated from {}", user_id, addr);
                                            }
                                            Err(e) => {
                                                let error_msg = ServerMessage::Error {
                                                    message: format!("Authentication failed: {}", e),
                                                };
                                                let _ = sender.send(Message::Text(
                                                    serde_json::to_string(&error_msg).unwrap().into()
                                                )).await;
                                            }
                                        }
                                    }
                                    ClientMessage::Pong => {
                                        // Keep connection alive
                                        if let Some(ref sid) = session_id {
                                            let _ = self.session_registry.manager().update_activity(sid).await;
                                        }
                                        if authenticated {
                                            // Respond with ping to keep connection alive
                                            let _ = sender.send(Message::Text(
                                                serde_json::to_string(&ServerMessage::Ping).unwrap().into()
                                            )).await;
                                        }
                                    }
                                    ClientMessage::UpdateView { view } => {
                                        if let Some(ref sid) = session_id {
                                            let _ = self.session_registry.manager().update_view(sid, &view).await;
                                        }
                                    }
                                    ClientMessage::AssignBead { bead_id, to } => {
                                        if let Some(ref sid) = session_id {
                                            if let Some(session) = self.session_registry.manager().get_session(sid).await {
                                                if session.role.can_assign_beads() {
                                                    if let Ok(assignment) = self.assignment_tracker.assign(
                                                        &bead_id, &to, &session.user_id
                                                    ).await {
                                                        // Log the bead assignment to audit log
                                                        if let Some(ref logger) = self.audit_logger {
                                                            let _ = logger.log(AuditEvent::new(
                                                                EventType::UserAction,
                                                                &session.user_id,
                                                                "bead_assignment",
                                                                &bead_id,
                                                            )
                                                            .with_new_value(format!("assigned_to={}", &to))
                                                            .with_metadata(format!("assigned_by={}", &session.user_id)));
                                                        }
                                                        self.broadcast(ServerMessage::BeadAssigned {
                                                            bead_id,
                                                            assigned_to: assignment.assigned_to.unwrap_or_default(),
                                                            assigned_by: assignment.assigned_by.unwrap_or_default(),
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    ClientMessage::UnassignBead { bead_id } => {
                                        if let Some(ref sid) = session_id {
                                            if let Some(session) = self.session_registry.manager().get_session(sid).await {
                                                if session.role.can_assign_beads() {
                                                    if let Ok(Some(assignment)) = self.assignment_tracker.unassign(&bead_id).await {
                                                        // Log the bead unassignment to audit log
                                                        if let Some(ref logger) = self.audit_logger {
                                                            let _ = logger.log(AuditEvent::new(
                                                                EventType::UserAction,
                                                                &session.user_id,
                                                                "bead_unassignment",
                                                                &bead_id,
                                                            )
                                                            .with_old_value(format!("was_assigned_to={}", &assignment.assigned_to.as_deref().unwrap_or("<none>"))));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    _ => {
                                        debug!("Unhandled client message: {:?}", client_msg);
                                    }
                                }
                            }
                        }
                        Message::Close(_) => {
                            debug!("Client {} closing connection", addr);
                            break;
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    warn!("WebSocket error from {}: {}", addr, e);
                    break;
                }
            }
        }

        // Cleanup on disconnect
        if let Some(sid) = session_id {
            if let Some(session) = self.session_registry.manager().remove_session(&sid).await {
                let user_id = session.user_id.clone();
                self.broadcast(ServerMessage::UserLeft {
                    user: user_id.clone(),
                });
                info!("User {} disconnected from {}", user_id, addr);
            }
        }

        Ok(())
    }
}

impl Clone for ForgeServer {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            auth: Arc::clone(&self.auth),
            session_registry: SessionRegistry::new(),
            assignment_tracker: self.assignment_tracker.clone(),
            state_broadcast: self.state_broadcast.clone(),
            current_state: Arc::clone(&self.current_state),
            running: Arc::clone(&self.running),
            audit_logger: self.audit_logger.clone(),
        }
    }
}

/// WebSocket handler for Axum.
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(server): State<ForgeServer>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, server))
}

/// Health check handler.
async fn health_handler() -> &'static str {
    "OK"
}

/// Handle a WebSocket connection.
async fn handle_socket(socket: WebSocket, server: ForgeServer) {
    let addr = "0.0.0.0:0".parse().unwrap(); // Placeholder
    if let Err(e) = server.handle_connection(socket, addr).await {
        error!("WebSocket handler error: {}", e);
    }
}

/// Create a server with default auth provider.
pub async fn create_server(config: ServerConfig) -> ForgeServer {
    use super::auth::SimpleAuth;

    let auth = Arc::new(SimpleAuth::default().with_defaults().await);

    // Try to initialize audit logger
    let audit_db_path = forge_config::config_path()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.forge/config.yaml"))
        .parent()
        .unwrap_or(&std::path::PathBuf::from("~/.forge"))
        .join("audit.db");

    let audit_logger = if let Ok(logger) = AuditLogger::open(audit_db_path) {
        Some(Arc::new(logger))
    } else {
        tracing::warn!("Failed to initialize audit logger, running without audit logging");
        None
    };

    if let Some(ref logger) = audit_logger {
        ForgeServer::with_audit_logger(config, auth, Arc::clone(logger))
    } else {
        ForgeServer::new(config, auth)
    }
}
