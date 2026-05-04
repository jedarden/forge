//! Protocol for FORGE server-client communication.
//!
//! Defines the message types sent over WebSocket between the server and clients.

use forge_core::{UserRole, UserSession, WorkerStatus, BeadStatus, Priority};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Message sent from server to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ServerMessage {
    /// Welcome message with session info
    Welcome {
        session: UserSession,
        server_info: ServerInfo,
    },
    /// State update (workers, beads, etc.)
    StateUpdate(StateUpdate),
    /// User joined the session
    UserJoined { user: String, display_name: String, role: UserRole },
    /// User left the session
    UserLeft { user: String },
    /// Bead assigned notification
    BeadAssigned { bead_id: String, assigned_to: String, assigned_by: String },
    /// Worker status changed
    WorkerChanged { worker_id: String, status: WorkerStatus },
    /// Bead status changed
    BeadChanged { bead_id: String, status: BeadStatus },
    /// Chat message from another user
    ChatMessage { from: String, message: String, timestamp: DateTime<Utc> },
    /// Error occurred
    Error { message: String },
    /// Ping to keep connection alive
    Ping,
}

/// Message sent from client to server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ClientMessage {
    /// Authenticate with credentials
    Authenticate { user_id: String, credentials: String },
    /// Request full state sync
    SyncState,
    /// Assign bead to user
    AssignBead { bead_id: String, to: String },
    /// Unassign bead
    UnassignBead { bead_id: String },
    /// Spawn worker
    SpawnWorker { model: String, count: u32 },
    /// Kill worker
    KillWorker { worker_id: String },
    /// Change bead status
    ChangeBeadStatus { bead_id: String, status: BeadStatus },
    /// Send chat message
    ChatMessage { message: String },
    /// Update current view
    UpdateView { view: String },
    /// Ping response
    Pong,
}

/// Server information sent on connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub server_version: String,
    pub connected_users: usize,
    pub active_workers: usize,
    pub pending_beads: usize,
}

/// State update broadcast to all clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateUpdate {
    pub timestamp: DateTime<Utc>,
    pub workers: Vec<WorkerState>,
    pub beads: Vec<BeadState>,
    pub costs: CostState,
    pub sessions: Vec<SessionSummary>,
}

/// Worker state in the update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerState {
    pub worker_id: String,
    pub model: String,
    pub status: WorkerStatus,
    pub current_task: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
}

/// Bead state in the update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeadState {
    pub bead_id: String,
    pub title: String,
    pub status: BeadStatus,
    pub priority: Priority,
    pub assigned_to: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Cost state in the update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostState {
    pub today_cost: f64,
    pub week_cost: f64,
    pub month_cost: f64,
}

/// Session summary for broadcast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub user_id: String,
    pub display_name: String,
    pub role: UserRole,
    pub current_view: Option<String>,
    pub connected_at: DateTime<Utc>,
}

/// Current server state snapshot.
#[derive(Debug, Clone)]
pub struct ServerState {
    pub workers: Vec<WorkerState>,
    pub beads: Vec<BeadState>,
    pub costs: CostState,
    pub sessions: Vec<UserSession>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            workers: Vec::new(),
            beads: Vec::new(),
            costs: CostState {
                today_cost: 0.0,
                week_cost: 0.0,
                month_cost: 0.0,
            },
            sessions: Vec::new(),
        }
    }

    pub fn with_workers(mut self, workers: Vec<WorkerState>) -> Self {
        self.workers = workers;
        self
    }

    pub fn with_beads(mut self, beads: Vec<BeadState>) -> Self {
        self.beads = beads;
        self
    }

    pub fn with_costs(mut self, costs: CostState) -> Self {
        self.costs = costs;
        self
    }

    pub fn with_sessions(mut self, sessions: Vec<UserSession>) -> Self {
        self.sessions = sessions;
        self
    }
}

impl Default for ServerState {
    fn default() -> Self {
        Self::new()
    }
}
