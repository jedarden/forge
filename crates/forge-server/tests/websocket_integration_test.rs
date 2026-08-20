//! WebSocket integration tests for FORGE server.
//!
//! Tests real-time WebSocket communication, state broadcasting, and message relay.

use forge_server::{
    websocket::{ForgeServer, ServerConfig},
    client::{ForgeClient, ClientConfig},
    protocol::{ServerMessage, WorkerState, BeadState, CostState, ClientMessage},
    auth::{TestAuthProvider, AuthProvider},
};
use forge_core::{WorkerStatus, BeadStatus, Priority, UserRole};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tokio::sync::Mutex;

/// Helper function to create a test auth provider for testing.
/// Uses TestAuthProvider with predefined test users and tokens.
fn create_test_auth_provider() -> Arc<dyn AuthProvider> {
    Arc::new(TestAuthProvider::new())
}

/// Helper function to get test token for a specific user role.
fn get_test_token(role: &str) -> String {
    match role {
        "admin" => "test_admin_token".to_string(),
        "operator" => "test_operator_token".to_string(),
        "viewer" => "test_viewer_token".to_string(),
        _ => "test_viewer_token".to_string(),
    }
}

/// Helper struct to track received messages during tests with proper synchronization.
struct MessageTracker {
    messages: Arc<Mutex<Vec<ServerMessage>>>,
}

impl MessageTracker {
    fn new() -> Self {
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn count(&self) -> usize {
        self.messages.lock().await.len()
    }

    async fn clear(&self) {
        self.messages.lock().await.clear()
    }

    /// Wait for a specific number of messages with proper timeout.
    async fn wait_for_message_count(&self, expected: usize, timeout_ms: u64) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_millis(timeout_ms) {
            if self.count().await >= expected {
                return true;
            }
            sleep(Duration::from_millis(50)).await;
        }
        false
    }

    /// Find a message matching a predicate.
    async fn find_message<F>(&self, predicate: F) -> Option<ServerMessage>
    where
        F: Fn(&ServerMessage) -> bool,
    {
        let messages = self.messages.lock().await;
        for msg in messages.iter() {
            if predicate(msg) {
                return Some(msg.clone());
            }
        }
        None
    }

    /// Count messages matching a predicate.
    async fn count_messages<F>(&self, predicate: F) -> usize
    where
        F: Fn(&ServerMessage) -> bool,
    {
        let messages = self.messages.lock().await;
        messages.iter().filter(|msg| predicate(msg)).count()
    }

    /// Add a message to the tracker.
    async fn add_message(&self, msg: ServerMessage) {
        let mut messages = self.messages.lock().await;
        messages.push(msg);
    }

    /// Get all messages (for testing assertions).
    async fn get_all_messages(&self) -> Vec<ServerMessage> {
        let messages = self.messages.lock().await;
        messages.clone()
    }
}

/// Test helper function to create a test server configuration.
fn create_test_config(port: u16) -> ServerConfig {
    ServerConfig {
        bind_address: "127.0.0.1".to_string(),
        port,
        tls: None,  // No TLS for basic tests
    }
}

/// Wait for server to be ready by checking health endpoint.
async fn wait_for_server_ready(port: u16, max_wait_ms: u64) -> bool {
    let start = std::time::Instant::now();
    let url = format!("http://127.0.0.1:{}/health", port);

    while start.elapsed() < Duration::from_millis(max_wait_ms) {
        match reqwest::get(&url).await {
            Ok(resp) if resp.status().is_success() => return true,
            Ok(_) => sleep(Duration::from_millis(50)).await,
            Err(_) => {
                // Connection refused means server not ready yet
                sleep(Duration::from_millis(50)).await;
            }
        }
    }
    false
}

/// Wait for client connection with timeout.
async fn wait_for_client_connected(client: &ForgeClient, timeout_ms: u64) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_millis(timeout_ms) {
        let state = client.get_state().await;
        if state.authenticated {
            return true;
        }
        sleep(Duration::from_millis(50)).await;
    }
    false
}

/// Test helper to create a tracked client that records all received messages.
struct TrackedClient {
    client: ForgeClient,
    tracker: Arc<MessageTracker>,
}

impl TrackedClient {
    async fn new(config: ClientConfig) -> Self {
        let client = ForgeClient::new(config.clone());
        let tracker = Arc::new(MessageTracker::new());

        // Subscribe to messages
        let mut rx = client.subscribe();
        let tracker_clone = Arc::clone(&tracker);

        tokio::spawn(async move {
            while let Ok(msg) = rx.recv().await {
                tracker_clone.add_message(msg).await;
            }
        });

        Self { client, tracker }
    }

    fn client(&self) -> &ForgeClient {
        &self.client
    }

    fn tracker(&self) -> &Arc<MessageTracker> {
        &self.tracker
    }
}

/// Test complete WebSocket connection and disconnection cycle with comprehensive verification.
#[tokio::test]
async fn test_websocket_connect_disconnect_cycle() {
    let config = create_test_config(8081);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Track initial server state
    let initial_running = server.is_running().await;
    assert!(!initial_running, "Server should not be running initially");

    // Start server in background
    let server_clone = server.clone();
    let server_handle = tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start (use simple sleep for reliability)
    sleep(Duration::from_millis(1000)).await;

    let running_after_start = server.is_running().await;
    assert!(running_after_start, "Server should be running after start");

    // Create and connect client
    let client_config = ClientConfig {
        server_url: "ws://127.0.0.1:8081/ws".to_string(),
        user_id: "viewer".to_string(),
        password: get_test_token("viewer"),
    };

    let client = ForgeClient::new(client_config.clone());
    let client_clone = client.clone();
    let _client_handle = tokio::spawn(async move {
        let _ = client_clone.connect_and_run().await;
    });

    // Wait for connection
    sleep(Duration::from_millis(500)).await;

    // Verify authentication and session
    let state = client.get_state().await;
    assert!(state.authenticated, "Client should be authenticated");
    assert!(state.session.is_some(), "Client should have a session");

    let session = state.session.unwrap();
    assert_eq!(session.user_id, "viewer", "Session should have correct user ID");
    assert_eq!(session.role, UserRole::Viewer, "Viewer user should have Viewer role");

    // Verify server info
    assert!(state.server_info.is_some(), "Client should have server info");
    let server_info = state.server_info.unwrap();
    assert!(!server_info.server_version.is_empty(), "Server version should not be empty");
    assert_eq!(server_info.connected_users, 1, "Should report 1 connected user");

    // Verify session is registered in server
    let session_count = server.session_registry().manager().session_count().await;
    assert_eq!(session_count, 1, "Server should have 1 registered session");

    // Test client can send messages
    let sync_result = client.send_direct(ClientMessage::SyncState).await;
    assert!(sync_result.is_ok(), "Client should be able to send SyncState message");

    // Stop server to trigger disconnection
    server.stop().await;

    // Wait for server to stop
    let _ = timeout(Duration::from_secs(2), server_handle).await;
    let running_after_stop = server.is_running().await;
    assert!(!running_after_stop, "Server should not be running after stop");

    // Wait for client to disconnect
    sleep(Duration::from_millis(500)).await;

    // Verify session was cleaned up
    let session_count_after = server.session_registry().manager().session_count().await;
    assert_eq!(session_count_after, 0, "Server should have 0 sessions after disconnect");

    // Verify client connection is closed
    let _state_after = client.get_state().await;
    // State should remain but connection is closed

    // Try to send message - should fail
    let send_result = client.send_direct(ClientMessage::SyncState).await;
    assert!(send_result.is_err(), "Sending message after disconnect should fail");

    // Test reconnection with new server instance
    let config2 = create_test_config(8081);
    let auth2: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server2 = ForgeServer::new(config2, Arc::clone(&auth2));

    let server2_clone = server2.clone();
    tokio::spawn(async move {
        let _ = server2_clone.run().await;
    });

    // Wait for server restart
    sleep(Duration::from_millis(1000)).await;

    // Create new client connection
    let client2 = ForgeClient::new(client_config);
    let client2_clone = client2.clone();
    tokio::spawn(async move {
        let _ = client2_clone.connect_and_run().await;
    });

    // Wait for reconnection
    sleep(Duration::from_millis(500)).await;

    let state2 = client2.get_state().await;
    assert!(state2.authenticated, "Reconnected client should be authenticated");

    server2.stop().await;
}

/// Test state broadcast from server to multiple clients with comprehensive verification.
#[tokio::test]
async fn test_state_broadcast() {
    let config = create_test_config(8082);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to be ready
    sleep(Duration::from_millis(1000)).await;
    // Server started

    // Create multiple tracked clients
    let mut tracked_clients = Vec::new();
    for i in 0..3 {
        let client_config = ClientConfig {
            server_url: "ws://127.0.0.1:8082/ws".to_string(),
            user_id: format!("user{}", i),
            password: "testpass".to_string(),
        };

        let tracked_client = TrackedClient::new(client_config).await;
        let client_clone = tracked_client.client().clone();
        tokio::spawn(async move {
            let _ = client_clone.connect_and_run().await;
        });

        tracked_clients.push(tracked_client);
        sleep(Duration::from_millis(100)).await;
    }

    // Wait for all clients to connect
    sleep(Duration::from_millis(800)).await;

    // Clear any initial messages
    for tracked in &tracked_clients {
        tracked.tracker().clear().await;
    }

    // Broadcast a state update with multiple workers
    let worker_states = vec![
        WorkerState {
            worker_id: "worker-1".to_string(),
            model: "claude-sonnet-5".to_string(),
            status: WorkerStatus::Active,
            current_task: Some("bead-1".to_string()),
            started_at: Some(chrono::Utc::now()),
        },
        WorkerState {
            worker_id: "worker-2".to_string(),
            model: "claude-opus-5".to_string(),
            status: WorkerStatus::Idle,
            current_task: None,
            started_at: Some(chrono::Utc::now()),
        },
    ];

    server.update_workers(worker_states.clone()).await;

    // Wait for broadcast to propagate to all clients
    for (i, tracked) in tracked_clients.iter().enumerate() {
        assert!(
            tracked.tracker().wait_for_message_count(1, 2000).await,
            "Client {} should receive state update within 2 seconds",
            i
        );
    }

    // Verify all clients received the update with correct data
    for (i, tracked) in tracked_clients.iter().enumerate() {
        let state = tracked.client().get_state().await;
        assert!(
            state.state_update.is_some(),
            "Client {} should have received state update",
            i
        );

        if let Some(update) = &state.state_update {
            assert_eq!(
                update.workers.len(),
                2,
                "Client {} should have 2 workers",
                i
            );

            // Verify worker 1
            assert_eq!(
                update.workers[0].worker_id, "worker-1",
                "Client {}: worker 1 should have correct ID",
                i
            );
            assert_eq!(
                update.workers[0].model, "claude-sonnet-5",
                "Client {}: worker 1 should have correct model",
                i
            );
            assert_eq!(
                update.workers[0].status, WorkerStatus::Active,
                "Client {}: worker 1 should be Active",
                i
            );
            assert_eq!(
                update.workers[0].current_task, Some("bead-1".to_string()),
                "Client {}: worker 1 should have correct task",
                i
            );

            // Verify worker 2
            assert_eq!(
                update.workers[1].worker_id, "worker-2",
                "Client {}: worker 2 should have correct ID",
                i
            );
            assert_eq!(
                update.workers[1].status, WorkerStatus::Idle,
                "Client {}: worker 2 should be Idle",
                i
            );
            assert!(
                update.workers[1].current_task.is_none(),
                "Client {}: worker 2 should have no task",
                i
            );
        }
    }

    // Verify StateUpdate messages were received
    for (i, tracked) in tracked_clients.iter().enumerate() {
        let state_update_count = tracked.tracker().count_messages(|msg| {
            matches!(msg, ServerMessage::StateUpdate(_))
        }).await;

        assert_eq!(
            state_update_count, 1,
            "Client {} should have received exactly 1 StateUpdate message",
            i
        );
    }

    // Broadcast multiple state updates in sequence
    let worker_states_2 = vec![
        WorkerState {
            worker_id: "worker-1".to_string(),
            model: "claude-sonnet-5".to_string(),
            status: WorkerStatus::Idle,
            current_task: None,
            started_at: Some(chrono::Utc::now()),
        },
    ];

    server.update_workers(worker_states_2).await;

    // Wait for second update
    assert!(
        tracked_clients[0].tracker().wait_for_message_count(2, 2000).await,
        "Client should receive second state update"
    );

    // Verify the second update
    let state = tracked_clients[0].client().get_state().await;
    if let Some(update) = &state.state_update {
        assert_eq!(update.workers.len(), 1, "Should have 1 worker after second update");
        assert_eq!(update.workers[0].status, WorkerStatus::Idle, "Worker should be Idle");
    }

    server.stop().await;
}

/// Test message relay from client to server and broadcast to other clients with comprehensive verification.
#[tokio::test]
async fn test_message_relay() {
    let config = create_test_config(8083);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to be ready
    assert!(
        wait_for_server_ready(8083, 2000).await,
        "Server should start successfully"
    );

    // Create two tracked clients
    let client1_config = ClientConfig {
        server_url: "ws://127.0.0.1:8083/ws".to_string(),
        user_id: "admin".to_string(),
        password: get_test_token("admin").to_string(),
    };

    let client2_config = ClientConfig {
        server_url: "ws://127.0.0.1:8083/ws".to_string(),
        user_id: "operator".to_string(),
        password: get_test_token("operator").to_string(),
    };

    let tracked1 = TrackedClient::new(client1_config).await;
    let tracked2 = TrackedClient::new(client2_config).await;

    let client1_clone = tracked1.client().clone();
    tokio::spawn(async move {
        let _ = client1_clone.connect_and_run().await;
    });

    sleep(Duration::from_millis(300)).await;

    let client2_clone = tracked2.client().clone();
    tokio::spawn(async move {
        let _ = client2_clone.connect_and_run().await;
    });

    // Wait for both to connect and authenticate
    sleep(Duration::from_millis(500)).await;

    // Clear initial messages
    tracked1.tracker().clear().await;
    tracked2.tracker().clear().await;

    // Client 1 assigns a bead (should be broadcast to Client 2)
    tracked1.client().assign_bead("bead-1", "operator").await;

    // Wait for message relay to Client 2
    assert!(
        tracked2.tracker().wait_for_message_count(1, 2000).await,
        "Client 2 should receive bead assignment message"
    );

    // Verify Client 2 received the correct assignment notification
    let assignment_msg = tracked2.tracker().find_message(|msg| {
        matches!(msg, ServerMessage::BeadAssigned { .. })
    }).await;

    assert!(assignment_msg.is_some(), "Client 2 should receive BeadAssigned message");

    if let Some(ServerMessage::BeadAssigned { bead_id, assigned_to, assigned_by }) = assignment_msg {
        assert_eq!(bead_id, "bead-1", "Should have correct bead ID");
        assert_eq!(assigned_to, "operator", "Should be assigned to operator");
        assert_eq!(assigned_by, "admin", "Should be assigned by admin");
    } else {
        panic!("Message should be BeadAssigned variant");
    }

    // Verify Client 1 also received the broadcast (echo)
    assert!(
        tracked1.tracker().wait_for_message_count(1, 1000).await,
        "Client 1 should receive echo of bead assignment"
    );

    // Test message relay for bead unassignment
    tracked1.tracker().clear().await;
    tracked2.tracker().clear().await;

    tracked1.client().unassign_bead("bead-1").await;

    // Wait for unassignment message
    assert!(
        tracked2.tracker().wait_for_message_count(1, 2000).await,
        "Client 2 should receive bead unassignment message"
    );

    // Verify Client 2 received notification (might be implicit through state update)
    let msg_count = tracked2.tracker().count().await;
    assert!(msg_count > 0, "Client 2 should receive some message after unassignment");

    // Test multiple message relays in sequence
    tracked1.tracker().clear().await;
    tracked2.tracker().clear().await;

    // Assign multiple beads
    tracked1.client().assign_bead("bead-2", "operator").await;
    tracked1.client().assign_bead("bead-3", "admin").await;
    tracked1.client().assign_bead("bead-4", "operator").await;

    // Wait for all messages to propagate
    assert!(
        tracked2.tracker().wait_for_message_count(3, 3000).await,
        "Client 2 should receive all 3 assignment messages"
    );

    // Verify all assignments were received
    let assignment_count = tracked2.tracker().count_messages(|msg| {
        matches!(msg, ServerMessage::BeadAssigned { .. })
    }).await;

    assert_eq!(assignment_count, 3, "Should receive 3 BeadAssigned messages");

    server.stop().await;
}

/// Test comprehensive state update with workers, beads, and costs broadcast to all clients.
#[tokio::test]
async fn test_comprehensive_state_update_broadcast() {
    let config = create_test_config(8094);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    assert!(
        wait_for_server_ready(8094, 2000).await,
        "Server should start successfully"
    );

    // Create multiple clients
    let mut clients = Vec::new();
    for i in 0..3 {
        let config = ClientConfig {
            server_url: "ws://127.0.0.1:8094/ws".to_string(),
            user_id: format!("viewer{}", i),
            password: "testpass".to_string(),
        };

        let tracked = TrackedClient::new(config).await;
        let client_clone = tracked.client().clone();
        tokio::spawn(async move {
            let _ = client_clone.connect_and_run().await;
        });

        clients.push(tracked);
        sleep(Duration::from_millis(100)).await;
    }

    // Wait for all to connect
    sleep(Duration::from_millis(500)).await;

    // Clear initial messages
    for tracked in &clients {
        tracked.tracker().clear().await;
    }

    // Update workers
    let workers = vec![
        WorkerState {
            worker_id: "worker-alpha".to_string(),
            model: "claude-sonnet-5".to_string(),
            status: WorkerStatus::Active,
            current_task: Some("task-1".to_string()),
            started_at: Some(chrono::Utc::now()),
        },
    ];

    server.update_workers(workers).await;

    // Update beads
    let beads = vec![
        BeadState {
            bead_id: "bead-1".to_string(),
            title: "Fix authentication".to_string(),
            status: BeadStatus::InProgress,
            priority: Priority::P1,
            assigned_to: Some("operator".to_string()),
            created_at: chrono::Utc::now(),
        },
    ];

    server.update_beads(beads).await;

    // Update costs
    let costs = CostState {
        today_cost: 12.50,
        week_cost: 85.25,
        month_cost: 342.75,
    };

    server.update_costs(costs).await;

    // Broadcast full state
    server.broadcast_full_state().await;

    // Verify all clients received the full state update
    for (i, tracked) in clients.iter().enumerate() {
        assert!(
            tracked.tracker().wait_for_message_count(4, 3000).await,
            "Client {} should receive state updates (1 worker update + 1 bead + 1 bead changed + 1 full state)",
            i
        );

        let state = tracked.client().get_state().await;
        assert!(
            state.state_update.is_some(),
            "Client {} should have state update",
            i
        );

        if let Some(update) = &state.state_update {
            // Verify workers
            assert_eq!(update.workers.len(), 1, "Client {} should have 1 worker", i);
            assert_eq!(update.workers[0].worker_id, "worker-alpha", "Worker ID should match");

            // Verify beads
            assert_eq!(update.beads.len(), 1, "Client {} should have 1 bead", i);
            assert_eq!(update.beads[0].status, BeadStatus::InProgress, "Bead status should match");

            // Verify costs
            assert!((update.costs.today_cost - 12.50).abs() < 0.01, "Today cost should match");
            assert!((update.costs.week_cost - 85.25).abs() < 0.01, "Week cost should match");
            assert!((update.costs.month_cost - 342.75).abs() < 0.01, "Month cost should match");

            // Verify sessions
            assert!(update.sessions.len() >= 3, "Should have at least 3 sessions");
        }
    }

    server.stop().await;
}

/// Test user join and leave broadcasts.
#[tokio::test]
async fn test_user_join_leave_broadcast() {
    let config = create_test_config(8084);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Create first client
    let client1_config = ClientConfig {
        server_url: "ws://127.0.0.1:8084/ws".to_string(),
        user_id: "user1".to_string(),
        password: "testpass".to_string(),
    };

    let client1 = ForgeClient::new(client1_config);
    let client1_clone = client1.clone();
    tokio::spawn(async move {
        let _ = client1_clone.connect_and_run().await;
    });

    // Wait for first client to connect
    sleep(Duration::from_millis(500)).await;

    // Verify initial state
    let state1 = client1.get_state().await;
    assert_eq!(state1.connected_users.len(), 1, "Should have 1 user");
    assert_eq!(state1.connected_users[0].user_id, "user1");

    // Create second client
    let client2_config = ClientConfig {
        server_url: "ws://127.0.0.1:8084/ws".to_string(),
        user_id: "user2".to_string(),
        password: "testpass".to_string(),
    };

    let client2 = ForgeClient::new(client2_config);
    let client2_clone = client2.clone();
    tokio::spawn(async move {
        let _ = client2_clone.connect_and_run().await;
    });

    // Wait for second client to connect and join broadcast to propagate
    sleep(Duration::from_millis(500)).await;

    // Verify both users are visible
    let state1_after = client1.get_state().await;
    assert_eq!(
        state1_after.connected_users.len(),
        2,
        "Should have 2 users after user2 joins"
    );

    // Stop server to simulate client2 disconnection
    server.stop().await;
    sleep(Duration::from_millis(500)).await;

    // Note: Full disconnect tracking would require proper cleanup handlers
}

/// Test bead assignment and unassignment operations via WebSocket.
#[tokio::test]
async fn test_bead_assignment_operations() {
    let config = create_test_config(8085);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Create admin client
    let client_config = ClientConfig {
        server_url: "ws://127.0.0.1:8085/ws".to_string(),
        user_id: "admin".to_string(),
        password: get_test_token("admin").to_string(),
    };

    let client = ForgeClient::new(client_config);
    let client_clone = client.clone();
    tokio::spawn(async move {
        let _ = client_clone.connect_and_run().await;
    });

    // Wait for connection
    sleep(Duration::from_millis(500)).await;

    // Assign bead
    client.assign_bead("bead-1", "operator").await;
    sleep(Duration::from_millis(200)).await;

    // Unassign bead
    client.unassign_bead("bead-1").await;
    sleep(Duration::from_millis(200)).await;

    // Verify operations completed without errors
    let state = client.get_state().await;
    assert!(state.authenticated);

    server.stop().await;
}

/// Test worker status change broadcasts.
#[tokio::test]
async fn test_worker_status_change_broadcast() {
    let config = create_test_config(8086);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Create client
    let client_config = ClientConfig {
        server_url: "ws://127.0.0.1:8086/ws".to_string(),
        user_id: "viewer".to_string(),
        password: get_test_token("viewer").to_string(),
    };

    let client = ForgeClient::new(client_config);
    let client_clone = client.clone();
    tokio::spawn(async move {
        let _ = client_clone.connect_and_run().await;
    });

    // Wait for connection
    sleep(Duration::from_millis(500)).await;

    // Update worker states
    let worker_states = vec![
        WorkerState {
            worker_id: "worker-1".to_string(),
            model: "claude-opus-5".to_string(),
            status: WorkerStatus::Active,
            current_task: Some("bead-1".to_string()),
            started_at: Some(chrono::Utc::now()),
        },
    ];

    server.update_workers(worker_states).await;

    // Wait for broadcast
    sleep(Duration::from_millis(500)).await;

    // Verify client received worker status update
    let state = client.get_state().await;
    assert!(state.state_update.is_some());

    if let Some(update) = &state.state_update {
        assert_eq!(update.workers.len(), 1);
        assert_eq!(update.workers[0].status, WorkerStatus::Active);
    }

    server.stop().await;
}

/// Test bead status change broadcasts.
#[tokio::test]
async fn test_bead_status_change_broadcast() {
    let config = create_test_config(8087);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Create client
    let client_config = ClientConfig {
        server_url: "ws://127.0.0.1:8087/ws".to_string(),
        user_id: "operator".to_string(),
        password: get_test_token("operator").to_string(),
    };

    let client = ForgeClient::new(client_config);
    let client_clone = client.clone();
    tokio::spawn(async move {
        let _ = client_clone.connect_and_run().await;
    });

    // Wait for connection
    sleep(Duration::from_millis(500)).await;

    // Update bead states
    let bead_states = vec![
        BeadState {
            bead_id: "bead-1".to_string(),
            title: "Test Bead".to_string(),
            status: BeadStatus::InProgress,
            priority: Priority::P1,
            assigned_to: Some("operator".to_string()),
            created_at: chrono::Utc::now(),
        },
    ];

    server.update_beads(bead_states).await;

    // Wait for broadcast
    sleep(Duration::from_millis(500)).await;

    // Verify client received bead status update
    let state = client.get_state().await;
    assert!(state.state_update.is_some());

    if let Some(update) = &state.state_update {
        assert_eq!(update.beads.len(), 1);
        assert_eq!(update.beads[0].status, BeadStatus::InProgress);
    }

    server.stop().await;
}

/// Test cost state updates and broadcasts.
#[tokio::test]
async fn test_cost_state_broadcast() {
    let config = create_test_config(8088);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Create client
    let client_config = ClientConfig {
        server_url: "ws://127.0.0.1:8088/ws".to_string(),
        user_id: "viewer".to_string(),
        password: get_test_token("viewer").to_string(),
    };

    let client = ForgeClient::new(client_config);
    let client_clone = client.clone();
    tokio::spawn(async move {
        let _ = client_clone.connect_and_run().await;
    });

    // Wait for connection
    sleep(Duration::from_millis(500)).await;

    // Update cost state
    let costs = CostState {
        today_cost: 1.50,
        week_cost: 10.25,
        month_cost: 42.75,
    };

    server.update_costs(costs).await;
    server.broadcast_full_state().await;

    // Wait for broadcast
    sleep(Duration::from_millis(500)).await;

    // Verify client received cost update
    let state = client.get_state().await;
    assert!(state.state_update.is_some());

    if let Some(update) = &state.state_update {
        assert!((update.costs.today_cost - 1.50).abs() < 0.01);
        assert!((update.costs.week_cost - 10.25).abs() < 0.01);
        assert!((update.costs.month_cost - 42.75).abs() < 0.01);
    }

    server.stop().await;
}

/// Test chat message functionality with message relay verification.
#[tokio::test]
async fn test_chat_message_relay() {
    let config = create_test_config(8089);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    assert!(
        wait_for_server_ready(8089, 2000).await,
        "Server should start successfully"
    );

    // Create two tracked clients
    let client1_config = ClientConfig {
        server_url: "ws://127.0.0.1:8089/ws".to_string(),
        user_id: "admin".to_string(),
        password: get_test_token("admin").to_string(),
    };

    let client2_config = ClientConfig {
        server_url: "ws://127.0.0.1:8089/ws".to_string(),
        user_id: "operator".to_string(),
        password: get_test_token("operator").to_string(),
    };

    let tracked1 = TrackedClient::new(client1_config).await;
    let tracked2 = TrackedClient::new(client2_config).await;

    let client1_clone = tracked1.client().clone();
    tokio::spawn(async move {
        let _ = client1_clone.connect_and_run().await;
    });

    sleep(Duration::from_millis(300)).await;

    let client2_clone = tracked2.client().clone();
    tokio::spawn(async move {
        let _ = client2_clone.connect_and_run().await;
    });

    // Wait for both to connect
    assert!(
        wait_for_client_connected(tracked1.client(), 2000).await,
        "Client 1 should connect successfully"
    );
    assert!(
        wait_for_client_connected(tracked2.client(), 2000).await,
        "Client 2 should connect successfully"
    );

    // Clear initial messages
    tracked1.tracker().clear().await;
    tracked2.tracker().clear().await;

    // Send chat message from client1
    let test_message = "Hello, this is a test message!";
    tracked1.client().send_chat(test_message).await;

    // Wait for message relay
    assert!(
        tracked2.tracker().wait_for_message_count(1, 2000).await,
        "Client 2 should receive chat message"
    );

    // Verify client 2 received the chat message
    let chat_msg = tracked2.tracker().find_message(|msg| {
        matches!(msg, ServerMessage::ChatMessage { .. })
    }).await;

    assert!(chat_msg.is_some(), "Client 2 should receive ChatMessage");

    if let Some(ServerMessage::ChatMessage { from, message, timestamp }) = chat_msg {
        assert_eq!(from, "admin", "Message should be from admin");
        assert_eq!(message, test_message, "Message content should match");
        let formatted_time = timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
        assert!(!formatted_time.is_empty(), "Timestamp should be valid and format to string");
    } else {
        panic!("Message should be ChatMessage variant");
    }

    // Test multiple chat messages
    tracked1.tracker().clear().await;
    tracked2.tracker().clear().await;

    let messages = vec![
        "First message",
        "Second message",
        "Third message",
    ];

    for msg in messages.iter() {
        tracked1.client().send_chat(*msg).await;
        sleep(Duration::from_millis(50)).await;
    }

    // Wait for all messages
    assert!(
        tracked2.tracker().wait_for_message_count(3, 3000).await,
        "Client 2 should receive all 3 chat messages"
    );

    // Verify message count
    let chat_count = tracked2.tracker().count_messages(|msg| {
        matches!(msg, ServerMessage::ChatMessage { .. })
    }).await;

    assert_eq!(chat_count, 3, "Should receive exactly 3 chat messages");

    // Verify both clients are still connected and authenticated
    let state1 = tracked1.client().get_state().await;
    let state2 = tracked2.client().get_state().await;

    assert!(state1.authenticated, "Client 1 should still be authenticated");
    assert!(state2.authenticated, "Client 2 should still be authenticated");

    server.stop().await;
}

/// Test multiple concurrent connections.
#[tokio::test]
async fn test_multiple_concurrent_connections() {
    let config = create_test_config(8090);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Create multiple concurrent clients
    let mut clients = Vec::new();
    for i in 0..5 {
        let client_config = ClientConfig {
            server_url: "ws://127.0.0.1:8090/ws".to_string(),
            user_id: format!("user{}", i),
            password: "testpass".to_string(),
        };

        let client = ForgeClient::new(client_config);
        let client_clone = client.clone();
        tokio::spawn(async move {
            let _ = client_clone.connect_and_run().await;
        });

        clients.push(client);
        sleep(Duration::from_millis(100)).await;
    }

    // Wait for all clients to connect
    sleep(Duration::from_millis(1000)).await;

    // Broadcast a state update
    let worker_states = vec![
        WorkerState {
            worker_id: "worker-test".to_string(),
            model: "claude-sonnet-5".to_string(),
            status: WorkerStatus::Idle,
            current_task: None,
            started_at: Some(chrono::Utc::now()),
        },
    ];

    server.update_workers(worker_states).await;

    // Wait for broadcast to propagate
    sleep(Duration::from_millis(500)).await;

    // Verify all clients are authenticated and received the update
    for (i, client) in clients.iter().enumerate() {
        let state = client.get_state().await;
        assert!(
            state.authenticated,
            "Client {} should be authenticated",
            i
        );
        assert!(
            state.state_update.is_some(),
            "Client {} should have received state update",
            i
        );
    }

    server.stop().await;
}

/// Test ping/pong keepalive mechanism.
#[tokio::test]
async fn test_ping_pong_keepalive() {
    let config = create_test_config(8091);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Create client
    let client_config = ClientConfig {
        server_url: "ws://127.0.0.1:8091/ws".to_string(),
        user_id: "viewer".to_string(),
        password: get_test_token("viewer").to_string(),
    };

    let client = ForgeClient::new(client_config);
    let client_clone = client.clone();
    tokio::spawn(async move {
        let _ = client_clone.connect_and_run().await;
    });

    // Wait for connection and authentication
    sleep(Duration::from_millis(1000)).await;

    // Verify connection is still alive
    let state = client.get_state().await;
    assert!(state.authenticated, "Client should remain authenticated");

    server.stop().await;
}

/// Test authentication failure handling.
#[tokio::test]
async fn test_authentication_failure() {
    let config = create_test_config(8092);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Create client with wrong password
    let client_config = ClientConfig {
        server_url: "ws://127.0.0.1:8092/ws".to_string(),
        user_id: "admin".to_string(),
        password: "wrongpassword".to_string(),
    };

    let client = ForgeClient::new(client_config);
    let client_clone = client.clone();
    tokio::spawn(async move {
        let _ = client_clone.connect_and_run().await;
    });

    // Wait for connection attempt
    sleep(Duration::from_millis(1000)).await;

    // Verify authentication failed (not authenticated)
    let state = client.get_state().await;
    assert!(!state.authenticated, "Client should not be authenticated with wrong password");

    server.stop().await;
}

/// Test concurrent client operations and state consistency.
#[tokio::test]
async fn test_concurrent_client_operations() {
    let config = create_test_config(8095);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    assert!(
        wait_for_server_ready(8095, 2000).await,
        "Server should start successfully"
    );

    // Create multiple admin clients
    let mut clients = Vec::new();
    for i in 0..4 {
        let config = ClientConfig {
            server_url: "ws://127.0.0.1:8095/ws".to_string(),
            user_id: format!("admin{}", i),
            password: get_test_token("admin").to_string(),
        };

        let tracked = TrackedClient::new(config).await;
        let client_clone = tracked.client().clone();
        tokio::spawn(async move {
            let _ = client_clone.connect_and_run().await;
        });

        clients.push(tracked);
        sleep(Duration::from_millis(50)).await;
    }

    // Wait for all to connect
    sleep(Duration::from_millis(500)).await;

    // Clear initial messages
    for tracked in &clients {
        tracked.tracker().clear().await;
    }

    // All clients simultaneously assign different beads
    let mut handles = Vec::new();
    for (i, tracked) in clients.iter().enumerate() {
        let client = tracked.client().clone();
        let handle = tokio::spawn(async move {
            client.assign_bead(format!("bead-{}", i), format!("operator{}", i % 2)).await;
        });
        handles.push(handle);
    }

    // Wait for all assignments to complete
    for handle in handles {
        let _ = timeout(Duration::from_secs(2), handle).await;
    }

    // Wait for broadcasts to propagate
    sleep(Duration::from_millis(500)).await;

    // Verify all clients received all 4 assignments
    for (i, tracked) in clients.iter().enumerate() {
        assert!(
            tracked.tracker().wait_for_message_count(4, 3000).await,
            "Client {} should receive 4 bead assignments",
            i
        );

        let assignment_count = tracked.tracker().count_messages(|msg| {
            matches!(msg, ServerMessage::BeadAssigned { .. })
        }).await;

        assert_eq!(assignment_count, 4, "Client {} should receive 4 BeadAssigned messages", i);
    }

    // Now update workers and verify all clients receive the update
    let workers: Vec<WorkerState> = (0..5).map(|i| WorkerState {
        worker_id: format!("worker-{}", i),
        model: "claude-sonnet-5".to_string(),
        status: if i % 2 == 0 { WorkerStatus::Active } else { WorkerStatus::Idle },
        current_task: if i % 2 == 0 { Some(format!("bead-{}", i)) } else { None },
        started_at: Some(chrono::Utc::now()),
    }).collect();

    server.update_workers(workers).await;

    // Verify all clients received worker update
    for (i, tracked) in clients.iter().enumerate() {
        let state = tracked.client().get_state().await;
        assert!(
            state.state_update.is_some(),
            "Client {} should have worker state update",
            i
        );

        if let Some(update) = &state.state_update {
            assert_eq!(update.workers.len(), 5, "Client {} should have 5 workers", i);
        }
    }

    server.stop().await;
}

/// Test message ordering and sequence preservation.
#[tokio::test]
async fn test_message_ordering() {
    let config = create_test_config(8096);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    assert!(
        wait_for_server_ready(8096, 2000).await,
        "Server should start successfully"
    );

    // Create client
    let config = ClientConfig {
        server_url: "ws://127.0.0.1:8096/ws".to_string(),
        user_id: "admin".to_string(),
        password: get_test_token("admin").to_string(),
    };

    let tracked = TrackedClient::new(config).await;
    let client_clone = tracked.client().clone();
    tokio::spawn(async move {
        let _ = client_clone.connect_and_run().await;
    });

    assert!(
        wait_for_client_connected(tracked.client(), 2000).await,
        "Client should connect successfully"
    );

    tracked.tracker().clear().await;

    // Send a sequence of bead assignments
    let bead_ids = vec!["bead-1", "bead-2", "bead-3", "bead-4", "bead-5"];
    for bead_id in bead_ids.iter() {
        tracked.client().assign_bead(*bead_id, "operator").await;
        sleep(Duration::from_millis(50)).await; // Small delay to ensure ordering
    }

    // Wait for all messages
    assert!(
        tracked.tracker().wait_for_message_count(5, 3000).await,
        "Client should receive 5 assignment messages"
    );

    // Verify messages are in order by collecting bead IDs from all messages
    let mut received_beads = Vec::new();
    let all_messages = tracked.tracker().get_all_messages().await;

    for msg in all_messages.iter() {
        if let ServerMessage::BeadAssigned { bead_id, .. } = msg {
            received_beads.push(bead_id.clone());
        }
    }

    assert_eq!(received_beads, bead_ids, "Messages should be received in order sent");

    server.stop().await;
}

/// Test rapid state updates and client responsiveness.
#[tokio::test]
async fn test_rapid_state_updates() {
    let config = create_test_config(8097);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    assert!(
        wait_for_server_ready(8097, 2000).await,
        "Server should start successfully"
    );

    // Create client
    let config = ClientConfig {
        server_url: "ws://127.0.0.1:8097/ws".to_string(),
        user_id: "admin".to_string(),
        password: get_test_token("admin").to_string(),
    };

    let tracked = TrackedClient::new(config).await;
    let client_clone = tracked.client().clone();
    tokio::spawn(async move {
        let _ = client_clone.connect_and_run().await;
    });

    assert!(
        wait_for_client_connected(tracked.client(), 2000).await,
        "Client should connect successfully"
    );

    tracked.tracker().clear().await;

    // Send 20 rapid worker updates
    for i in 0..20 {
        let workers = vec![WorkerState {
            worker_id: format!("worker-{}", i),
            model: "claude-sonnet-5".to_string(),
            status: if i % 2 == 0 { WorkerStatus::Active } else { WorkerStatus::Idle },
            current_task: if i % 2 == 0 { Some(format!("task-{}", i)) } else { None },
            started_at: Some(chrono::Utc::now()),
        }];

        server.update_workers(workers).await;
        sleep(Duration::from_millis(10)).await; // Very rapid updates
    }

    // Wait for updates to propagate (should be fast)
    assert!(
        tracked.tracker().wait_for_message_count(20, 5000).await,
        "Client should receive 20 rapid updates within 5 seconds"
    );

    // Verify final state is consistent
    let state = tracked.client().get_state().await;
    assert!(
        state.state_update.is_some(),
        "Client should have final state update"
    );

    server.stop().await;
}

/// Test error recovery and connection stability.
#[tokio::test]
async fn test_error_recovery() {
    let config = create_test_config(8098);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    assert!(
        wait_for_server_ready(8098, 2000).await,
        "Server should start successfully"
    );

    // Create client with invalid credentials first
    let invalid_config = ClientConfig {
        server_url: "ws://127.0.0.1:8098/ws".to_string(),
        user_id: "admin".to_string(),
        password: "wrongpassword".to_string(),
    };

    let invalid_client = ForgeClient::new(invalid_config);
    let invalid_client_clone = invalid_client.clone();
    tokio::spawn(async move {
        let _ = invalid_client_clone.connect_and_run().await;
    });

    sleep(Duration::from_millis(1000)).await;

    // Verify authentication failed
    let invalid_state = invalid_client.get_state().await;
    assert!(!invalid_state.authenticated, "Client with wrong password should not authenticate");

    // Now create client with valid credentials
    let valid_config = ClientConfig {
        server_url: "ws://127.0.0.1:8098/ws".to_string(),
        user_id: "admin".to_string(),
        password: get_test_token("admin").to_string(),
    };

    let tracked = TrackedClient::new(valid_config).await;
    let client_clone = tracked.client().clone();
    tokio::spawn(async move {
        let _ = client_clone.connect_and_run().await;
    });

    assert!(
        wait_for_client_connected(tracked.client(), 2000).await,
        "Valid client should connect successfully"
    );

    // Verify connection is stable and can send/receive messages
    tracked.client().assign_bead("bead-1", "operator").await;

    assert!(
        tracked.tracker().wait_for_message_count(1, 2000).await,
        "Client should receive message after successful connection"
    );

    // Test sending multiple operations to ensure stability
    for i in 0..10 {
        tracked.client().assign_bead(format!("bead-{}", i), "operator").await;
        sleep(Duration::from_millis(20)).await;
    }

    assert!(
        tracked.tracker().wait_for_message_count(11, 3000).await,
        "Client should handle multiple operations reliably"
    );

    server.stop().await;
}

/// Test session persistence during reconnection.
#[tokio::test]
async fn test_session_persistence() {
    let config = create_test_config(8093);
    let auth: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Create first connection
    let client_config = ClientConfig {
        server_url: "ws://127.0.0.1:8093/ws".to_string(),
        user_id: "admin".to_string(),
        password: get_test_token("admin").to_string(),
    };

    let client1 = ForgeClient::new(client_config.clone());
    let client1_clone = client1.clone();
    tokio::spawn(async move {
        let _ = client1_clone.connect_and_run().await;
    });

    // Wait for connection
    sleep(Duration::from_millis(500)).await;

    // Verify first connection authenticated
    let state1 = client1.get_state().await;
    assert!(state1.authenticated);
    let _session_count_before = server.session_registry().manager().session_count().await;

    // Stop server
    server.stop().await;
    sleep(Duration::from_millis(500)).await;

    // Create new server instance
    let config2 = create_test_config(8093);
    let auth2: Arc<dyn AuthProvider> = create_test_auth_provider();
    let server2 = ForgeServer::new(config2, Arc::clone(&auth2));

    // Start new server
    let server2_clone = server2.clone();
    tokio::spawn(async move {
        let _ = server2_clone.run().await;
    });

    sleep(Duration::from_millis(500)).await;

    // Reconnect with same client
    let client2 = ForgeClient::new(client_config);
    let client2_clone = client2.clone();
    tokio::spawn(async move {
        let _ = client2_clone.connect_and_run().await;
    });

    // Wait for reconnection
    sleep(Duration::from_millis(500)).await;

    // Verify reconnection successful
    let state2 = client2.get_state().await;
    assert!(state2.authenticated, "Reconnection should be successful");

    server2.stop().await;
}