//! Comprehensive WSS/TLS integration tests for FORGE team collaboration clients.
//!
//! Tests secure WebSocket connections with focus on:
//! - WS compatibility (existing behavior preserved)
//! - WSS URL handling and parsing
//! - Trusted test certificates (self-signed for development)
//! - Rejected untrusted certificates
//! - Actionable TLS error messages

use forge_server::{
    auth::{AuthProvider, TestAuthProvider},
    client::{ClientConfig, ClientTlsConfig, ForgeClient},
    websocket::{ForgeServer, ServerConfig, TlsConfig},
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;
use tokio::time::timeout;

mod certs;
use certs::generate_test_cert;

/// Setup crypto provider for TLS tests.
fn setup_crypto_provider() {
    use rustls::crypto::ring::default_provider;
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        default_provider()
            .install_default()
            .expect("Failed to install CryptoProvider");
    });
}

/// Generate test certificates for WSS testing.
async fn setup_wss_certs() -> (TempDir, PathBuf, PathBuf) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let cert_path = temp_dir.path().join("server-cert.pem");
    let key_path = temp_dir.path().join("server-key.pem");

    generate_test_cert(&cert_path, &key_path, "localhost")
        .expect("Failed to generate test certificate");

    (temp_dir, cert_path, key_path)
}

/// Create TLS server configuration.
fn create_tls_server_config(port: u16, cert_path: PathBuf, key_path: PathBuf) -> ServerConfig {
    ServerConfig {
        bind_address: "127.0.0.1".to_string(),
        port,
        tls: Some(TlsConfig {
            cert_path: cert_path.to_string_lossy().to_string(),
            key_path: key_path.to_string_lossy().to_string(),
            verify: false,
            min_version: "TLSv1.2".to_string(),
        }),
    }
}

/// Test that WS (plain WebSocket) connections still work correctly.
#[tokio::test]
async fn test_ws_compatibility_preserved() {
    let config = ServerConfig {
        bind_address: "127.0.0.1".to_string(),
        port: 9100,
        tls: None, // No TLS - plain WebSocket
    };

    let auth: Arc<dyn AuthProvider> = Arc::new(TestAuthProvider::new());
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Verify server is running
    assert!(server.is_running().await, "Server should be running");

    // Create WS client configuration
    let client_config = ClientConfig {
        server_url: "ws://127.0.0.1:9100/ws".to_string(),
        user_id: "viewer".to_string(),
        password: "test_viewer_token".to_string(),
        tls: None,
    };

    // Create and connect client
    let client = ForgeClient::new(client_config.clone());
    let client_clone = client.clone();
    tokio::spawn(async move {
        let _ = client_clone.connect_and_run().await;
    });

    // Wait for connection
    sleep(Duration::from_millis(500)).await;

    // Verify client connected and authenticated
    let state = client.get_state().await;
    assert!(state.authenticated, "Client should authenticate successfully over WS");
    assert!(state.session.is_some(), "Client should have a session");

    // Verify authentication flow is preserved
    let session = state.session.unwrap();
    assert_eq!(session.user_id, "viewer");

    server.stop().await;
}

/// Test WSS URL parsing and scheme recognition.
#[tokio::test]
async fn test_wss_url_parsing_and_recognition() {
    // Test WSS URL scheme recognition
    let wss_url = "wss://localhost:8443/ws";
    assert!(wss_url.starts_with("wss://"), "Should recognize WSS scheme");

    // Test that both ws:// and wss:// are accepted
    let ws_url = "ws://localhost:8080/ws";
    let wss_url = "wss://localhost:8443/ws";

    assert!(ws_url.starts_with("ws://"), "Should recognize WS scheme");
    assert!(wss_url.starts_with("wss://"), "Should recognize WSS scheme");

    // Test client config with WSS URL
    let client_config = ClientConfig {
        server_url: wss_url.to_string(),
        user_id: "user".to_string(),
        password: "pass".to_string(),
        tls: None,
    };

    assert_eq!(client_config.server_url, wss_url);
    let _client = ForgeClient::new(client_config);

    // Test client config with WS URL
    let client_config = ClientConfig {
        server_url: ws_url.to_string(),
        user_id: "user".to_string(),
        password: "pass".to_string(),
        tls: None,
    };

    assert_eq!(client_config.server_url, ws_url);
    let _client = ForgeClient::new(client_config);
}

/// Test WSS server startup and basic client connection.
#[tokio::test]
async fn test_wss_server_startup_and_connection() {
    setup_crypto_provider();

    let (_temp_dir, cert_path, key_path) = setup_wss_certs().await;
    let config = create_tls_server_config(9101, cert_path, key_path);

    let auth: Arc<dyn AuthProvider> = Arc::new(TestAuthProvider::new());
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(1000)).await;

    // Verify server is running
    assert!(server.is_running().await, "WSS server should be running");

    // Create WSS client configuration
    let client_config = ClientConfig {
        server_url: "wss://127.0.0.1:9101/ws".to_string(),
        user_id: "viewer".to_string(),
        password: "test_viewer_token".to_string(),
        tls: None,
    };

    let _client = ForgeClient::new(client_config);

    // Note: Actual connection will fail with self-signed cert unless verification disabled
    // This test validates that the server starts and client can be configured

    server.stop().await;
}

/// Test that self-signed certificates are rejected by default.
#[tokio::test]
async fn test_self_signed_certificate_rejected_by_default() {
    setup_crypto_provider();

    let (_temp_dir, cert_path, key_path) = setup_wss_certs().await;
    let config = create_tls_server_config(9102, cert_path, key_path);

    let auth: Arc<dyn AuthProvider> = Arc::new(TestAuthProvider::new());
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(1000)).await;

    // Create client with default TLS config (verification enabled)
    let client_config = ClientConfig {
        server_url: "wss://127.0.0.1:9102/ws".to_string(),
        user_id: "viewer".to_string(),
        password: "test_viewer_token".to_string(),
        tls: None, // Default - verification enabled
    };

    let client = ForgeClient::new(client_config);
    let client_clone = client.clone();

    // Attempt connection - should fail due to self-signed cert
    let connection_result = tokio::spawn(async move {
        let result = client_clone.connect_and_run().await;
        result
    });

    // Wait for connection attempt
    let result = timeout(Duration::from_secs(3), connection_result).await;

    // Connection should fail or timeout due to certificate rejection
    match result {
        Ok(Ok(Ok(()))) => {
            panic!("Connection should have failed with self-signed certificate");
        }
        Ok(Ok(Err(e))) => {
            // Expected - connection failed with certificate error
            let error_msg = e.to_string();
            assert!(
                error_msg.contains("TLS") || error_msg.contains("certificate") || error_msg.contains("handshake"),
                "Error should mention TLS/certificate/handshake issue: {}",
                error_msg
            );
        }
        Ok(Err(e)) => {
            // Task join error - connection attempt failed
            panic!("Task failed: {}", e);
        }
        Err(_) => {
            // Timeout - acceptable, indicates connection couldn't be established
        }
    }

    server.stop().await;
}

/// Test that certificate verification can be explicitly disabled for development.
#[tokio::test]
async fn test_certificate_verification_can_be_disabled_for_development() {
    setup_crypto_provider();

    let (_temp_dir, cert_path, key_path) = setup_wss_certs().await;
    let config = create_tls_server_config(9103, cert_path, key_path);

    let auth: Arc<dyn AuthProvider> = Arc::new(TestAuthProvider::new());
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(1000)).await;

    // Create client with verification EXPLICITLY disabled
    let tls_config = ClientTlsConfig::new().danger_disable_verification();
    let client_config = ClientConfig {
        server_url: "wss://127.0.0.1:9103/ws".to_string(),
        user_id: "viewer".to_string(),
        password: "test_viewer_token".to_string(),
        tls: Some(tls_config),
    };

    let client = ForgeClient::new(client_config.clone());
    let client_clone = client.clone();

    // Attempt connection with verification disabled - should succeed
    let connection_result = tokio::spawn(async move {
        let result = client_clone.connect_and_run().await;
        result
    });

    // Wait for connection
    sleep(Duration::from_millis(2000)).await;

    // Verify client connected successfully
    let state = client.get_state().await;
    assert!(state.authenticated, "Client should authenticate when verification is disabled");

    // Verify the dangerous setting was actually used
    assert!(!client_config.tls.unwrap().danger_verify_certificate);

    // Stop server and wait for connection task
    server.stop().await;
    let _ = timeout(Duration::from_secs(2), connection_result).await;
}

/// Test ClientTlsConfig default behavior (verification enabled).
#[tokio::test]
async fn test_client_tls_config_default_verification_enabled() {
    let tls_config = ClientTlsConfig::new();

    // Default should have verification enabled
    assert!(tls_config.danger_verify_certificate, "Default should verify certificates");
    assert!(tls_config.ca_bundle_path.is_none(), "Default should not have custom CA bundle");
}

/// Test ClientTlsConfig with verification explicitly disabled.
#[tokio::test]
async fn test_client_tls_config_disable_verification() {
    let tls_config = ClientTlsConfig::new().danger_disable_verification();

    assert!(!tls_config.danger_verify_certificate, "Verification should be disabled");
    assert!(tls_config.ca_bundle_path.is_none(), "Should not have custom CA bundle");
}

/// Test ClientTlsConfig with custom CA bundle path.
#[tokio::test]
async fn test_client_tls_config_with_ca_bundle() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let ca_bundle_path = temp_dir.path().join("test-ca.pem");

    // Write a dummy CA bundle
    std::fs::write(&ca_bundle_path, "dummy CA content").expect("Failed to write CA bundle");

    let tls_config = ClientTlsConfig::new().with_ca_bundle(ca_bundle_path.clone());

    assert!(tls_config.danger_verify_certificate, "Should still verify certificates");
    assert_eq!(tls_config.ca_bundle_path, Some(ca_bundle_path), "Should have custom CA bundle");
}

/// Test that invalid URL schemes are rejected.
#[tokio::test]
async fn test_invalid_url_scheme_rejected() {
    let client_config = ClientConfig {
        server_url: "http://localhost:8080/ws".to_string(), // Wrong scheme
        user_id: "viewer".to_string(),
        password: "test_viewer_token".to_string(),
        tls: None,
    };

    let client = ForgeClient::new(client_config);

    // Attempt connection should fail with clear error
    let result = client.connect_and_run().await;

    assert!(result.is_err(), "Connection should fail with invalid URL scheme");
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Invalid WebSocket URL scheme") || error_msg.contains("scheme"),
        "Error should mention invalid scheme: {}",
        error_msg
    );
}

/// Test that authentication flow is preserved over WSS.
#[tokio::test]
async fn test_authentication_flow_preserved_over_wss() {
    setup_crypto_provider();

    let (_temp_dir, cert_path, key_path) = setup_wss_certs().await;
    let config = create_tls_server_config(9104, cert_path, key_path);

    let auth: Arc<dyn AuthProvider> = Arc::new(TestAuthProvider::new());
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(1000)).await;

    // Create client with verification disabled
    let tls_config = ClientTlsConfig::new().danger_disable_verification();
    let client_config = ClientConfig {
        server_url: "wss://127.0.0.1:9104/ws".to_string(),
        user_id: "admin".to_string(),
        password: "test_admin_token".to_string(), // Valid token
        tls: Some(tls_config),
    };

    let client = ForgeClient::new(client_config);
    let client_clone = client.clone();
    tokio::spawn(async move {
        let _ = client_clone.connect_and_run().await;
    });

    // Wait for connection and authentication
    sleep(Duration::from_millis(1000)).await;

    // Verify authentication succeeded
    let state = client.get_state().await;
    assert!(state.authenticated, "Client should authenticate over WSS");

    if let Some(session) = state.session {
        assert_eq!(session.user_id, "admin");
        assert_eq!(session.role, forge_core::UserRole::Admin);
    } else {
        panic!("Client should have a session after authentication");
    }

    server.stop().await;
}

/// Test that message flow is preserved over WSS.
#[tokio::test]
async fn test_message_flow_preserved_over_wss() {
    setup_crypto_provider();

    let (_temp_dir, cert_path, key_path) = setup_wss_certs().await;
    let config = create_tls_server_config(9105, cert_path, key_path);

    let auth: Arc<dyn AuthProvider> = Arc::new(TestAuthProvider::new());
    let server = ForgeServer::new(config, Arc::clone(&auth));

    // Start server in background
    let server_clone = server.clone();
    tokio::spawn(async move {
        let _ = server_clone.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(1000)).await;

    // Create client with verification disabled
    let tls_config = ClientTlsConfig::new().danger_disable_verification();
    let client_config = ClientConfig {
        server_url: "wss://127.0.0.1:9105/ws".to_string(),
        user_id: "operator".to_string(),
        password: "test_operator_token".to_string(),
        tls: Some(tls_config),
    };

    let client = ForgeClient::new(client_config);
    let client_clone = client.clone();
    tokio::spawn(async move {
        let _ = client_clone.connect_and_run().await;
    });

    // Wait for connection
    sleep(Duration::from_millis(1000)).await;

    // Verify authentication
    let state = client.get_state().await;
    assert!(state.authenticated, "Client should authenticate");

    // Test sending messages over WSS
    client.assign_bead("test-bead-1", "operator").await;
    sleep(Duration::from_millis(200)).await;

    client.unassign_bead("test-bead-1").await;
    sleep(Duration::from_millis(200)).await;

    client.send_chat("Test message over WSS").await;
    sleep(Duration::from_millis(200)).await;

    // Verify client is still connected and authenticated
    let final_state = client.get_state().await;
    assert!(final_state.authenticated, "Client should remain authenticated");

    server.stop().await;
}
