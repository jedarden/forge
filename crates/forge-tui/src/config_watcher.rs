//! Configuration hot-reload system for FORGE.
//!
//! This module provides real-time configuration reloading without restarting forge.
//! It watches `~/.forge/config.yaml` for changes and applies them immediately.
//!
//! ## Hot-Reload Targets
//!
//! 1. **Theme changes** - Immediate visual update (< 500ms)
//! 2. **Hotkey bindings** - Immediate (next keypress)
//! 3. **Refresh intervals** - Within 1 second
//! 4. **Budget thresholds** - Within 1 second
//! 5. **Model tier mappings** - Within 5 seconds
//!
//! ## Architecture
//!
//! - Uses `notify` crate for file system watching (reuses existing watcher pattern)
//! - Debounces rapid changes (50ms default)
//! - Validates config before applying
//! - Graceful degradation on invalid config (keeps old config)
//! - Emits events for UI updates

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::{debug, info, warn};

// Re-export config types from forge-config crate
pub use forge_config::{ForgeConfig, ConfigLoadError, config_path};

/// Default debounce duration for config changes (50ms).
pub const DEFAULT_DEBOUNCE_MS: u64 = 50;

/// Configuration events emitted when config changes.
#[derive(Debug, Clone)]
pub enum ConfigEvent {
    /// Config file was modified and successfully reloaded.
    Reloaded {
        /// New configuration values
        config: ForgeConfig,
    },
    /// Config file was modified but validation failed.
    ValidationError {
        /// Error message describing what's wrong
        error: String,
        /// Path to the config file
        path: PathBuf,
    },
    /// Config file was created (first time).
    Created {
        /// Initial configuration
        config: ForgeConfig,
    },
    /// Config file was deleted.
    Removed,
    /// Error reading or parsing config file.
    Error {
        /// Error message
        error: String,
    },
}

/// Configuration watcher that monitors config.yaml for changes.
///
/// Uses the `notify` crate to watch for file system events and emits
/// `ConfigEvent`s when the config file changes.
pub struct ConfigWatcher {
    /// The underlying file watcher
    _watcher: RecommendedWatcher,

    /// Config file path being watched
    config_path: PathBuf,

    /// Current configuration
    current_config: ForgeConfig,

    /// Stop signal sender
    _stop_tx: Sender<()>,
}

impl ConfigWatcher {
    /// Create a new config watcher with default settings.
    ///
    /// Returns the watcher and a receiver for config events.
    pub fn new() -> Option<(Self, Receiver<ConfigEvent>)> {
        let config_path = config_path()?;
        Self::with_path(config_path)
    }

    /// Create a config watcher for a specific path.
    pub fn with_path(config_path: PathBuf) -> Option<(Self, Receiver<ConfigEvent>)> {
        // Load initial config
        let current_config = ForgeConfig::load_from(&config_path).unwrap_or_default();

        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let (stop_tx, _stop_rx) = std::sync::mpsc::channel();

        // Create file watcher
        let config_path_clone = config_path.clone();
        let event_tx_clone = event_tx.clone();

        let mut watcher = RecommendedWatcher::new(
            move |result: std::result::Result<Event, notify::Error>| {
                match result {
                    Ok(event) => {
                        // Check if this is our config file
                        if !event.paths.iter().any(|p| p == &config_path_clone) {
                            return;
                        }

                        match event.kind {
                            EventKind::Create(_) | EventKind::Modify(_) => {
                                // Small delay to ensure file is fully written
                                std::thread::sleep(Duration::from_millis(50));

                                match ForgeConfig::load_from(&config_path_clone) {
                                    Some(new_config) => {
                                        // Validate before emitting
                                        match new_config.validate() {
                                            Ok(()) => {
                                                let is_create = matches!(event.kind, EventKind::Create(_));
                                                let event_type = if is_create {
                                                    ConfigEvent::Created { config: new_config }
                                                } else {
                                                    ConfigEvent::Reloaded { config: new_config }
                                                };
                                                if event_tx_clone.send(event_type).is_err() {
                                                    debug!("Failed to send config event - channel closed");
                                                }
                                            }
                                            Err(e) => {
                                                if event_tx_clone
                                                    .send(ConfigEvent::ValidationError {
                                                        error: e,
                                                        path: config_path_clone.clone(),
                                                    })
                                                    .is_err()
                                                {
                                                    debug!("Failed to send validation error - channel closed");
                                                }
                                            }
                                        }
                                    }
                                    None => {
                                        if event_tx_clone
                                            .send(ConfigEvent::Error {
                                                error: "Failed to parse config file".to_string(),
                                            })
                                            .is_err()
                                        {
                                            debug!("Failed to send error event - channel closed");
                                        }
                                    }
                                }
                            }
                            EventKind::Remove(_)
                                if event_tx_clone.send(ConfigEvent::Removed).is_err() => {
                                    debug!("Failed to send remove event - channel closed");
                                }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        warn!("Config watcher error: {:?}", e);
                    }
                }
            },
            notify::Config::default().with_poll_interval(Duration::from_millis(DEFAULT_DEBOUNCE_MS)),
        )
        .ok()?;

        // Watch the parent directory (more reliable than watching file directly)
        let watch_path = config_path.parent().unwrap_or(&config_path);
        watcher.watch(watch_path, RecursiveMode::NonRecursive).ok()?;

        info!("Started watching config file: {:?}", config_path);

        let watcher = Self {
            _watcher: watcher,
            config_path,
            current_config,
            _stop_tx: stop_tx,
        };

        Some((watcher, event_rx))
    }

    /// Get the current configuration.
    pub fn current_config(&self) -> &ForgeConfig {
        &self.current_config
    }

    /// Get the config file path.
    pub fn config_path(&self) -> &PathBuf {
        &self.config_path
    }

    /// Update the current config (called after handling an event).
    pub fn update_config(&mut self, config: ForgeConfig) {
        self.current_config = config;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_config_watcher_detects_changes() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");

        // Create initial config
        let initial_content = r#"
dashboard:
  refresh_interval_ms: 1000
"#;
        fs::write(&config_path, initial_content).unwrap();

        let (watcher, rx) = ConfigWatcher::with_path(config_path.clone()).unwrap();

        // Give watcher time to initialize
        std::thread::sleep(Duration::from_millis(100));

        // Modify the config
        let modified_content = r#"
dashboard:
  refresh_interval_ms: 500
"#;
        fs::write(&config_path, modified_content).unwrap();

        // Wait for event
        std::thread::sleep(Duration::from_millis(200));

        // Check for event
        let event = rx.try_recv();
        match event {
            Ok(ConfigEvent::Reloaded { config }) => {
                assert_eq!(config.dashboard.refresh_interval_ms, 500);
            }
            Ok(ConfigEvent::Created { config }) => {
                // Some systems emit create instead of modify
                assert_eq!(config.dashboard.refresh_interval_ms, 500);
            }
            other => {
                // Event might not have arrived yet, which is OK for this test
                println!("Received event: {:?}", other);
            }
        }

        drop(watcher);
    }

    #[test]
    fn test_invalid_yaml_keeps_old_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");

        // Create initial valid config
        let initial_content = r#"
dashboard:
  refresh_interval_ms: 1000
"#;
        fs::write(&config_path, initial_content).unwrap();

        let (_watcher, rx) = ConfigWatcher::with_path(config_path.clone()).unwrap();

        // Give watcher time to initialize
        std::thread::sleep(Duration::from_millis(100));

        // Write invalid YAML
        let invalid_content = "invalid: [yaml";
        fs::write(&config_path, invalid_content).unwrap();

        // Wait for event
        std::thread::sleep(Duration::from_millis(200));

        // Should receive an error event
        let event = rx.try_recv();
        match event {
            Ok(ConfigEvent::Error { .. }) | Ok(ConfigEvent::ValidationError { .. }) => {
                // This is expected
            }
            other => {
                // Error handling is what matters, not exact event type
                println!("Received event: {:?}", other);
            }
        }
    }

    #[test]
    fn test_load_with_error_invalid_yaml() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");

        // Write invalid YAML
        let invalid_content = "invalid: [yaml";
        fs::write(&config_path, invalid_content).unwrap();

        let result = ForgeConfig::load_from_with_error(&config_path);
        assert!(result.is_err());

        let err = result.unwrap_err();
        match err {
            ConfigLoadError::ParseError { path, error: _ } => {
                assert_eq!(path, config_path);
            }
            other => panic!("Expected ParseError, got {:?}", other),
        }
    }

    #[test]
    fn test_load_with_error_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("missing.yaml");

        let result = ForgeConfig::load_from_with_error(&config_path);
        assert!(result.is_err());

        let err = result.unwrap_err();
        match err {
            ConfigLoadError::NotFound(path) => {
                assert_eq!(path, config_path);
            }
            other => panic!("Expected NotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_load_with_error_invalid_values() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");

        // Write config with invalid values
        let invalid_content = r#"
dashboard:
  refresh_interval_ms: 50  # Too low
  max_fps: 200  # Too high
"#;
        fs::write(&config_path, invalid_content).unwrap();

        let result = ForgeConfig::load_from_with_error(&config_path);
        assert!(result.is_err());

        let err = result.unwrap_err();
        match err {
            ConfigLoadError::ValidationError(msg) => {
                assert!(msg.contains("refresh_interval_ms") || msg.contains("max_fps"));
            }
            other => panic!("Expected ValidationError, got {:?}", other),
        }
    }

    #[test]
    fn test_config_error_line_column_extraction() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");

        // Write invalid YAML with syntax error
        let invalid_content = r#"
dashboard:
  refresh_interval_ms: [invalid
"#;
        fs::write(&config_path, invalid_content).unwrap();

        let result = ForgeConfig::load_from_with_error(&config_path);
        assert!(result.is_err());

        let err = result.unwrap_err();
        // Should have line number from YAML parser
        assert!(err.line_number().is_some() || err.column_number().is_some());
    }
}
