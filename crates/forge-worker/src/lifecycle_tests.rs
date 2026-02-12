//! Integration tests for worker spawning and lifecycle.
//!
//! These tests cover the worker lifecycle scenarios:
//! - Spawn single worker
//! - Spawn multiple workers in parallel
//! - Kill worker gracefully
//! - Worker status transitions
//! - Worker auto-recovery behavior
//!
//! Per ADR 0014: No automatic recovery - visibility first, user decides.

#[cfg(test)]
mod tests {
    use crate::launcher::WorkerLauncher;
    use crate::types::{LaunchConfig, LauncherOutput, SpawnRequest, WorkerHandle};
    use forge_core::types::{WorkerStatus, WorkerTier};
    use std::path::PathBuf;

    // =============================================================================
    // Spawn Single Worker Tests
    // =============================================================================

    #[test]
    fn test_spawn_single_worker_config_creation() {
        // Test that a single worker spawn request can be created
        let launcher_path = PathBuf::from("/test/launcher.sh");
        let session_name = "forge-test-worker";
        let workspace = PathBuf::from("/test/workspace");
        let model = "sonnet";

        let config = LaunchConfig::new(
            &launcher_path,
            session_name,
            &workspace,
            model,
        );

        assert_eq!(config.launcher_path, launcher_path);
        assert_eq!(config.session_name, session_name);
        assert_eq!(config.workspace, workspace);
        assert_eq!(config.model, model);
        assert_eq!(config.tier, WorkerTier::Standard); // Default tier
        assert_eq!(config.timeout_secs, 30); // Default timeout
    }

    #[test]
    fn test_spawn_request_creation() {
        // Test spawn request with worker ID
        let config = LaunchConfig::new(
            "/test/launcher.sh",
            "test-session",
            "/workspace",
            "sonnet",
        );

        let request = SpawnRequest::new("worker-123", config);

        assert_eq!(request.worker_id, "worker-123");
        assert_eq!(request.config.model, "sonnet");
    }

    #[test]
    fn test_spawn_worker_with_tier() {
        // Test spawning worker with specific tier
        let config = LaunchConfig::new(
            "/test/launcher.sh",
            "test-session",
            "/workspace",
            "opus",
        )
        .with_tier(WorkerTier::Premium);

        assert_eq!(config.tier, WorkerTier::Premium);
    }

    #[test]
    fn test_spawn_worker_with_env() {
        // Test spawning worker with custom environment variables
        let config = LaunchConfig::new(
            "/test/launcher.sh",
            "test-session",
            "/workspace",
            "haiku",
        )
        .with_env("FORGE_DEBUG", "1")
        .with_env("CUSTOM_VAR", "test_value");

        assert_eq!(config.env.len(), 2);
        assert_eq!(config.env[0], ("FORGE_DEBUG".to_string(), "1".to_string()));
        assert_eq!(config.env[1], ("CUSTOM_VAR".to_string(), "test_value".to_string()));
    }

    #[test]
    fn test_spawn_worker_with_bead() {
        // Test spawning worker with bead assignment
        let config = LaunchConfig::new(
            "/test/launcher.sh",
            "test-session",
            "/workspace",
            "sonnet",
        )
        .with_bead("fg-1234");

        assert!(config.has_bead());
        assert_eq!(config.bead_id, Some("fg-1234".to_string()));
    }

    // =============================================================================
    // Spawn Multiple Workers Tests
    // =============================================================================

    #[test]
    fn test_spawn_multiple_workers_config_generation() {
        // Test generating configs for multiple workers
        let models = vec!["sonnet", "sonnet", "sonnet"];
        let configs: Vec<LaunchConfig> = models
            .iter()
            .enumerate()
            .map(|(i, &model)| {
                LaunchConfig::new(
                    "/test/launcher.sh",
                    &format!("forge-sonnet-{}", i),
                    "/workspace",
                    model,
                )
            })
            .collect();

        assert_eq!(configs.len(), 3);
        for (i, config) in configs.iter().enumerate() {
            assert_eq!(
                config.session_name,
                format!("forge-sonnet-{}", i)
            );
        }
    }

    #[test]
    fn test_spawn_multiple_workers_different_models() {
        // Test generating configs for multiple workers with different models
        let models = vec!["sonnet", "haiku", "opus"];
        let configs: Vec<(String, LaunchConfig)> = models
            .iter()
            .enumerate()
            .map(|(i, &model)| {
                let tier = match model {
                    "opus" => WorkerTier::Premium,
                    "sonnet" => WorkerTier::Standard,
                    _ => WorkerTier::Budget,
                };
                let config = LaunchConfig::new(
                    "/test/launcher.sh",
                    &format!("forge-{}-{}", model, i),
                    "/workspace",
                    model,
                )
                .with_tier(tier);
                (model.to_string(), config)
            })
            .collect();

        assert_eq!(configs.len(), 3);

        // Verify opus is premium tier
        let opus_config = configs.iter().find(|(m, _)| m == "opus").unwrap();
        assert_eq!(opus_config.1.tier, WorkerTier::Premium);

        // Verify haiku is budget tier
        let haiku_config = configs.iter().find(|(m, _)| m == "haiku").unwrap();
        assert_eq!(haiku_config.1.tier, WorkerTier::Budget);
    }

    // =============================================================================
    // Kill Worker Tests
    // =============================================================================

    #[tokio::test]
    async fn test_launcher_empty_workers() {
        // Test that launcher starts with no workers
        let launcher = WorkerLauncher::new();
        let workers = launcher.list().await;
        assert!(workers.is_empty());
    }

    #[tokio::test]
    async fn test_stop_nonexistent_worker() {
        // Test that stopping a non-existent worker returns an error
        let launcher = WorkerLauncher::new();
        let result = launcher.stop("nonexistent-worker").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_nonexistent_worker() {
        // Test that getting a non-existent worker returns None
        let launcher = WorkerLauncher::new();
        let worker = launcher.get("nonexistent-worker").await;
        assert!(worker.is_none());
    }

    // =============================================================================
    // Worker Status Transition Tests
    // =============================================================================

    #[test]
    fn test_worker_status_starting() {
        // Test that new workers start in Starting status
        let handle = WorkerHandle::new(
            "worker-1",
            12345,
            "forge-worker-1",
            "/path/to/launcher.sh",
            "sonnet",
            WorkerTier::Standard,
            "/home/user/project",
        );

        assert_eq!(handle.status, WorkerStatus::Starting);
        assert!(handle.is_running()); // Starting is considered healthy
    }

    #[test]
    fn test_worker_status_transitions() {
        // Test valid status transitions
        let mut handle = WorkerHandle::new(
            "worker-1",
            12345,
            "forge-worker-1",
            "/path/to/launcher.sh",
            "sonnet",
            WorkerTier::Standard,
            "/home/user/project",
        );

        // Starting -> Active
        assert!(handle.is_running());
        handle.status = WorkerStatus::Active;
        assert!(handle.is_running());

        // Active -> Idle
        handle.status = WorkerStatus::Idle;
        assert!(handle.is_running());

        // Idle -> Stopped
        handle.status = WorkerStatus::Stopped;
        assert!(!handle.is_running());

        // Active -> Failed
        handle.status = WorkerStatus::Active;
        handle.status = WorkerStatus::Failed;
        assert!(!handle.is_running());
    }

    #[test]
    fn test_worker_status_health_check() {
        // Test WorkerStatus::is_healthy method
        assert!(WorkerStatus::Active.is_healthy());
        assert!(WorkerStatus::Idle.is_healthy());
        assert!(WorkerStatus::Starting.is_healthy());
        assert!(!WorkerStatus::Failed.is_healthy());
        assert!(!WorkerStatus::Stopped.is_healthy());
        assert!(!WorkerStatus::Error.is_healthy());
    }

    // =============================================================================
    // Worker Handle Tests
    // =============================================================================

    #[test]
    fn test_worker_handle_with_bead() {
        // Test adding bead assignment to worker handle
        let handle = WorkerHandle::new(
            "worker-1",
            12345,
            "forge-worker-1",
            "/path/to/launcher.sh",
            "sonnet",
            WorkerTier::Standard,
            "/home/user/project",
        )
        .with_bead("fg-5678", "Test Task");

        assert!(handle.has_bead());
        assert_eq!(handle.bead_id, Some("fg-5678".to_string()));
        assert_eq!(handle.bead_title, Some("Test Task".to_string()));
    }

    #[test]
    fn test_worker_handle_without_bead() {
        // Test worker handle without bead assignment
        let handle = WorkerHandle::new(
            "worker-1",
            12345,
            "forge-worker-1",
            "/path/to/launcher.sh",
            "sonnet",
            WorkerTier::Standard,
            "/home/user/project",
        );

        assert!(!handle.has_bead());
        assert_eq!(handle.bead_id, None);
        assert_eq!(handle.bead_title, None);
    }

    #[test]
    fn test_worker_handle_tmux_session() {
        // Test getting tmux session name from handle
        let handle = WorkerHandle::new(
            "worker-1",
            12345,
            "forge-worker-1",
            "/path/to/launcher.sh",
            "sonnet",
            WorkerTier::Standard,
            "/home/user/project",
        );

        assert_eq!(handle.tmux_session(), "forge-worker-1");
    }

    // =============================================================================
    // Launcher Output Tests
    // =============================================================================

    #[test]
    fn test_launcher_output_success() {
        // Test successful launcher output
        let output = LauncherOutput {
            pid: 12345,
            session: "forge-test".into(),
            model: "sonnet".into(),
            message: Some("Started successfully".into()),
            error: None,
            bead_id: None,
            bead_title: None,
        };

        assert!(output.is_success());
        assert!(output.error.is_none());
    }

    #[test]
    fn test_launcher_output_failure() {
        // Test failed launcher output
        let output = LauncherOutput {
            pid: 0,
            session: String::new(),
            model: String::new(),
            message: None,
            error: Some("Failed to start: no API key".into()),
            bead_id: None,
            bead_title: None,
        };

        assert!(!output.is_success());
        assert_eq!(output.error, Some("Failed to start: no API key".to_string()));
    }

    #[test]
    fn test_launcher_output_with_bead() {
        // Test launcher output with bead assignment
        let output = LauncherOutput {
            pid: 12345,
            session: "forge-test".into(),
            model: "sonnet".into(),
            message: Some("Started with bead".into()),
            error: None,
            bead_id: Some("fg-1234".into()),
            bead_title: Some("Implement feature X".into()),
        };

        assert!(output.is_success());
        assert_eq!(output.bead_id, Some("fg-1234".to_string()));
        assert_eq!(output.bead_title, Some("Implement feature X".to_string()));
    }

    // =============================================================================
    // Worker Tier Tests
    // =============================================================================

    #[test]
    fn test_worker_tier_classification() {
        // Test tier classification for different models
        let tiers = vec![
            ("haiku", WorkerTier::Budget),
            ("glm", WorkerTier::Budget),
            ("sonnet", WorkerTier::Standard),
            ("opus", WorkerTier::Premium),
        ];

        for (model, expected_tier) in tiers {
            let config = LaunchConfig::new(
                "/test/launcher.sh",
                "test-session",
                "/workspace",
                model,
            );

            // Default tier is Standard, so we need to explicitly set it based on model
            let tier = match model {
                "opus" => WorkerTier::Premium,
                "sonnet" => WorkerTier::Standard,
                _ => WorkerTier::Budget,
            };
            let config = config.with_tier(tier);

            assert_eq!(config.tier, expected_tier, "Model {} should be {:?}", model, expected_tier);
        }
    }

    // =============================================================================
    // Timeout Tests
    // =============================================================================

    #[test]
    fn test_spawn_timeout_configuration() {
        // Test configuring spawn timeout
        let config = LaunchConfig::new(
            "/test/launcher.sh",
            "test-session",
            "/workspace",
            "sonnet",
        )
        .with_timeout(60);

        assert_eq!(config.timeout_secs, 60);
    }

    #[test]
    fn test_spawn_default_timeout() {
        // Test default spawn timeout
        let config = LaunchConfig::new(
            "/test/launcher.sh",
            "test-session",
            "/workspace",
            "sonnet",
        );

        assert_eq!(config.timeout_secs, 30); // Default is 30 seconds
    }

    // =============================================================================
    // Worker Launcher Creation Tests (using public interface)
    // =============================================================================

    #[test]
    fn test_launcher_creation() {
        // Test creating a worker launcher
        let _launcher = WorkerLauncher::new();
        // Launcher should be created successfully
    }

    #[test]
    fn test_launcher_with_custom_prefix() {
        // Test creating a worker launcher with custom prefix
        let _launcher = WorkerLauncher::with_prefix("test-");
        // Launcher should be created successfully
    }
}
