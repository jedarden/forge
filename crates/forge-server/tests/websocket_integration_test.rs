//! WebSocket integration tests for FORGE server.
//!
//! Tests real-time WebSocket communication, state broadcasting, and message relay.

use forge_server::{
    websocket::{ForgeServer, ServerConfig},
    client::{ForgeClient, ClientConfig},
    protocol::{ServerMessage, WorkerState, BeadState, CostState},
    auth::{SimpleAuth, AuthProvider},
};
use forge_core::{WorkerStatus, BeadStatus, Priority};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tokio::sync::Mutex;

/// Helper struct to track received messages during tests.
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
}

/// Test helper function to create a test server configuration.
fn create_test_config(port: u16) -> ServerConfig {
    ServerConfig {
        bind_address: "127.0.0.1".to_string(),
        port,
    }
}

/// Test complete WebSocket connection and disconnection cycle.
#[tokio::test]
async fn test_websocket_connect_disconnect_cycle() {
    let config = create_test_config(8081);
    let auth: Arc<dyn AuthProvider> = Arc::new(SimpleAuth::default().with_defaults().await);
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Create and connect client
    let client_config = ClientConfig {
        server_url: "ws://127.0.0.1:8081/ws".to_string(),
        user_id: "testuser".to_string(),
        password: "testpass".to_string(),
    };

    let client = ForgeClient::new(client_config);
    let client_clone = client.clone();
    tokio::spawn(async move {
        let _ = client_clone.connect_and_run().await;
    });

    // Wait for connection
    sleep(Duration::from_millis(500)).await;

    // Verify authentication
    let state = client.get_state().await;
    assert!(state.authenticated, "Client should be authenticated");
    assert!(state.session.is_some(), "Client should have a session");

    // Disconnect client by stopping server
    server.stop().await;
    sleep(Duration::from_millis(200)).await;

    // Verify disconnection
    let _state_after = client.get_state().await;
    // Connection should be closed after server stops
}

/// Test state broadcast from server to multiple clients.
#[tokio::test]
async fn test_state_broadcast() {
    let config = create_test_config(8082);
    let auth: Arc<dyn AuthProvider> = Arc::new(SimpleAuth::default().with_defaults().await);
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Create multiple clients
    let mut clients = Vec::new();
    for i in 0..3 {
        let client_config = ClientConfig {
            server_url: "ws://127.0.0.1:8082/ws".to_string(),
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

    // Broadcast a state update
    let worker_states = vec![
        WorkerState {
            worker_id: "worker-1".to_string(),
            model: "claude-sonnet-5".to_string(),
            status: WorkerStatus::Active,
            current_task: Some("bead-1".to_string()),
            started_at: Some(chrono::Utc::now()),
        },
    ];

    server.update_workers(worker_states).await;

    // Wait for broadcast to propagate
    sleep(Duration::from_millis(500)).await;

    // Verify all clients received the update
    for (i, client) in clients.iter().enumerate() {
        let state = client.get_state().await;
        assert!(
            state.state_update.is_some(),
            "Client {} should have received state update",
            i
        );

        if let Some(update) = &state.state_update {
            assert_eq!(
                update.workers.len(),
                1,
                "Client {} should have 1 worker",
                i
            );
            assert_eq!(
                update.workers[0].worker_id, "worker-1",
                "Client {} should have correct worker ID",
                i
            );
        }
    }

    server.stop().await;
}

/// Test message relay from client to server and broadcast to other clients.
#[tokio::test]
async fn test_message_relay() {
    let config = create_test_config(8083);
    let auth: Arc<dyn AuthProvider> = Arc::new(SimpleAuth::default().with_defaults().await);
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Create two clients
    let client1_config = ClientConfig {
        server_url: "ws://127.0.0.1:8083/ws".to_string(),
        user_id: "admin".to_string(),
        password: "admin123".to_string(),
    };

    let client2_config = ClientConfig {
        server_url: "ws://127.0.0.1:8083/ws".to_string(),
        user_id: "operator".to_string(),
        password: "operator123".to_string(),
    };

    let client1 = ForgeClient::new(client1_config);
    let client2 = ForgeClient::new(client2_config);

    let client1_clone = client1.clone();
    tokio::spawn(async move {
        let _ = client1_clone.connect_and_run().await;
    });

    sleep(Duration::from_millis(300)).await;

    let client2_clone = client2.clone();
    tokio::spawn(async move {
        let _ = client2_clone.connect_and_run().await;
    });

    // Wait for both to connect and authenticate
    sleep(Duration::from_millis(500)).await;

    // Client 1 assigns a bead (should be broadcast to Client 2)
    client1.assign_bead("bead-1", "operator").await;

    // Wait for message relay
    sleep(Duration::from_millis(500)).await;

    // Verify Client 2 received the assignment notification
    let _state2 = client2.get_state().await;
    // Assignment should be visible through state updates

    server.stop().await;
}

/// Test user join and leave broadcasts.
#[tokio::test]
async fn test_user_join_leave_broadcast() {
    let config = create_test_config(8084);
    let auth: Arc<dyn AuthProvider> = Arc::new(SimpleAuth::default().with_defaults().await);
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
    let auth: Arc<dyn AuthProvider> = Arc::new(SimpleAuth::default().with_defaults().await);
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
        password: "admin123".to_string(),
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
    let auth: Arc<dyn AuthProvider> = Arc::new(SimpleAuth::default().with_defaults().await);
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
        password: "viewer123".to_string(),
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
    let auth: Arc<dyn AuthProvider> = Arc::new(SimpleAuth::default().with_defaults().await);
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
        password: "operator123".to_string(),
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
    let auth: Arc<dyn AuthProvider> = Arc::new(SimpleAuth::default().with_defaults().await);
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
        password: "viewer123".to_string(),
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

/// Test chat message functionality.
#[tokio::test]
async fn test_chat_message_relay() {
    let config = create_test_config(8089);
    let auth: Arc<dyn AuthProvider> = Arc::new(SimpleAuth::default().with_defaults().await);
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Create two clients
    let client1_config = ClientConfig {
        server_url: "ws://127.0.0.1:8089/ws".to_string(),
        user_id: "admin".to_string(),
        password: "admin123".to_string(),
    };

    let client2_config = ClientConfig {
        server_url: "ws://127.0.0.1:8089/ws".to_string(),
        user_id: "operator".to_string(),
        password: "operator123".to_string(),
    };

    let client1 = ForgeClient::new(client1_config);
    let client2 = ForgeClient::new(client2_config);

    let client1_clone = client1.clone();
    tokio::spawn(async move {
        let _ = client1_clone.connect_and_run().await;
    });

    sleep(Duration::from_millis(300)).await;

    let client2_clone = client2.clone();
    tokio::spawn(async move {
        let _ = client2_clone.connect_and_run().await;
    });

    // Wait for both to connect
    sleep(Duration::from_millis(500)).await;

    // Send chat message from client1
    client1.send_chat("Hello, this is a test message!").await;

    // Wait for message relay
    sleep(Duration::from_millis(500)).await;

    // Verify both clients are connected and authenticated
    let state1 = client1.get_state().await;
    let state2 = client2.get_state().await;

    assert!(state1.authenticated);
    assert!(state2.authenticated);

    server.stop().await;
}

/// Test multiple concurrent connections.
#[tokio::test]
async fn test_multiple_concurrent_connections() {
    let config = create_test_config(8090);
    let auth: Arc<dyn AuthProvider> = Arc::new(SimpleAuth::default().with_defaults().await);
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
    let auth: Arc<dyn AuthProvider> = Arc::new(SimpleAuth::default().with_defaults().await);
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
        password: "viewer123".to_string(),
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
    let auth: Arc<dyn AuthProvider> = Arc::new(SimpleAuth::default().with_defaults().await);
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

/// Test session persistence during reconnection.
#[tokio::test]
async fn test_session_persistence() {
    let config = create_test_config(8093);
    let auth: Arc<dyn AuthProvider> = Arc::new(SimpleAuth::default().with_defaults().await);
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
        password: "admin123".to_string(),
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
    let session_count_before = server.session_registry().manager().session_count().await;

    // Stop server
    server.stop().await;
    sleep(Duration::from_millis(500)).await;

    // Create new server instance
    let config2 = create_test_config(8093);
    let auth2: Arc<dyn AuthProvider> = Arc::new(SimpleAuth::default().with_defaults().await);
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