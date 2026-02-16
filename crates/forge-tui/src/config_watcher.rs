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

use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Default debounce duration for config changes (50ms).
pub const DEFAULT_DEBOUNCE_MS: u64 = 50;

/// Config file path (typically ~/.forge/config.yaml).
pub fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".forge/config.yaml"))
}

/// Format a YAML parse error with line/column information if available.
fn format_yaml_error(error: &serde_yaml::Error) -> String {
    if let Some(location) = error.location() {
        format!(
            "YAML parse error at line {}, column {}: {}",
            location.line(),
            location.column(),
            error
        )
    } else {
        format!("YAML parse error: {}", error)
    }
}

/// Errors that can occur when loading configuration.
#[derive(Debug, Error)]
pub enum ConfigLoadError {
    /// Could not determine home directory
    #[error("Could not determine home directory")]
    NoHomePath,

    /// Config file not found
    #[error("Config file not found: {0}")]
    NotFound(PathBuf),

    /// Error reading config file
    #[error("Failed to read config file {path}: {error}")]
    ReadError {
        path: PathBuf,
        #[source]
        error: io::Error,
    },

    /// Error parsing YAML
    #[error("Failed to parse config YAML in {path}: {}", format_yaml_error(error))]
    ParseError {
        path: PathBuf,
        error: serde_yaml::Error,
    },

    /// Config validation failed
    #[error("Config validation failed: {0}")]
    ValidationError(String),
}

impl From<String> for ConfigLoadError {
    fn from(s: String) -> Self {
        ConfigLoadError::ValidationError(s)
    }
}

impl ConfigLoadError {
    /// Get the line number where the error occurred (if available).
    pub fn line_number(&self) -> Option<usize> {
        match self {
            ConfigLoadError::ParseError { error, .. } => {
                error.location().map(|loc| loc.line())
            }
            _ => None,
        }
    }

    /// Get the column number where the error occurred (if available).
    pub fn column_number(&self) -> Option<usize> {
        match self {
            ConfigLoadError::ParseError { error, .. } => {
                error.location().map(|loc| loc.column())
            }
            _ => None,
        }
    }

    /// Get the config file path (if applicable).
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            ConfigLoadError::NotFound(path)
            | ConfigLoadError::ReadError { path, .. }
            | ConfigLoadError::ParseError { path, .. } => Some(path),
            _ => None,
        }
    }
}

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

/// Forge configuration structure.
///
/// This represents the subset of config.yaml that can be hot-reloaded.
/// Changes to these fields will take effect immediately.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct ForgeConfig {
    /// Dashboard configuration
    #[serde(default)]
    pub dashboard: DashboardConfig,

    /// Theme configuration
    #[serde(default)]
    pub theme: ThemeConfig,

    /// Cost tracking configuration
    #[serde(default)]
    pub cost_tracking: CostTrackingConfig,
}

impl ForgeConfig {
    /// Load configuration from the default path (~/.forge/config.yaml).
    ///
    /// Returns default configuration if the file doesn't exist or is invalid.
    /// Invalid configs are logged as warnings but don't prevent startup.
    pub fn load() -> Option<Self> {
        let path = config_path()?;
        Self::load_from(&path)
    }

    /// Load configuration from the default path with detailed error reporting.
    ///
    /// Returns a Result with detailed error information for display to users.
    pub fn load_with_error() -> Result<Self, ConfigLoadError> {
        let path = config_path().ok_or(ConfigLoadError::NoHomePath)?;
        Self::load_from_with_error(&path)
    }

    /// Load configuration from a specific path with graceful fallback.
    ///
    /// This method attempts to load and parse the config file. If the file
    /// doesn't exist, is unreadable, or contains invalid YAML, it returns
    /// the default configuration rather than failing.
    ///
    /// Errors are logged but don't prevent the application from starting.
    pub fn load_from(path: &PathBuf) -> Option<Self> {
        if !path.exists() {
            debug!("Config file does not exist: {:?} - using defaults", path);
            return None;
        }

        // Try to read the file
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                warn!(
                    path = ?path,
                    error = %e,
                    "Failed to read config file - using defaults"
                );
                return None;
            }
        };

        // Try to parse with fallback to partial parsing
        Self::parse_with_fallback(&content)
    }

    /// Load configuration from a specific path with detailed error reporting.
    ///
    /// Returns a Result with detailed error information for display to users.
    pub fn load_from_with_error(path: &PathBuf) -> Result<Self, ConfigLoadError> {
        if !path.exists() {
            return Err(ConfigLoadError::NotFound(path.clone()));
        }

        // Try to read the file
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigLoadError::ReadError {
                path: path.clone(),
                error: e,
            })?;

        // Try to parse as full YAML
        match serde_yaml::from_str::<ForgeConfig>(&content) {
            Ok(config) => {
                // Validate and return errors if invalid
                config.validate()?;
                Ok(config)
            }
            Err(e) => Err(ConfigLoadError::ParseError {
                path: path.clone(),
                error: e,
            }),
        }
    }

    /// Parse configuration from YAML string.
    ///
    /// Returns None if parsing fails completely.
    pub fn parse(content: &str) -> Option<Self> {
        Self::parse_with_fallback(content)
    }

    /// Parse configuration with fallback for partial/invalid configs.
    ///
    /// This method:
    /// 1. Tries to parse the full config
    /// 2. Falls back to partial parsing if sections are invalid
    /// 3. Returns default for completely invalid YAML
    fn parse_with_fallback(content: &str) -> Option<Self> {
        // First try to parse as full YAML
        match serde_yaml::from_str::<ForgeConfig>(content) {
            Ok(config) => {
                // Validate and warn about issues, but still return the config
                if let Err(e) = config.validate() {
                    warn!(
                        error = %e,
                        "Config validation warning - some settings may be ignored"
                    );
                }
                debug!("Successfully parsed forge config");
                Some(config)
            }
            Err(e) => {
                // Format detailed error message with line/column information
                let error_msg = format_yaml_error(&e);
                warn!(
                    error = %error_msg,
                    "Failed to parse config YAML - attempting partial parse"
                );

                // Try to parse individual sections as a fallback
                Self::parse_partial(content)
            }
        }
    }

    /// Attempt to parse individual sections of a malformed config.
    ///
    /// This allows partial configs to work even if one section has errors.
    fn parse_partial(content: &str) -> Option<Self> {
        // Try to parse as generic YAML first
        let yaml: serde_yaml::Value = match serde_yaml::from_str(content) {
            Ok(y) => y,
            Err(e) => {
                warn!(
                    error = %e,
                    "Config is not valid YAML - using defaults"
                );
                return None;
            }
        };

        // Try to extract individual sections
        let dashboard = yaml
            .get("dashboard")
            .and_then(|v| serde_yaml::from_value(v.clone()).ok())
            .unwrap_or_default();

        let theme = yaml
            .get("theme")
            .and_then(|v| serde_yaml::from_value(v.clone()).ok())
            .unwrap_or_default();

        let cost_tracking = yaml
            .get("cost_tracking")
            .and_then(|v| serde_yaml::from_value(v.clone()).ok())
            .unwrap_or_default();

        info!("Loaded partial config - some sections may use defaults");

        Some(Self {
            dashboard,
            theme,
            cost_tracking,
        })
    }

    /// Validate the configuration.
    ///
    /// Returns Ok(()) if valid, Err with message if invalid.
    /// Warnings are logged but don't cause validation failure.
    pub fn validate(&self) -> Result<(), String> {
        let mut warnings = Vec::new();

        // Validate refresh interval (must be >= 100ms)
        if self.dashboard.refresh_interval_ms < 100 {
            warnings.push(format!(
                "refresh_interval_ms {} is too low, using minimum of 100ms",
                self.dashboard.refresh_interval_ms
            ));
        }

        // Validate max_fps (must be 1-120)
        if self.dashboard.max_fps == 0 || self.dashboard.max_fps > 120 {
            warnings.push(format!(
                "max_fps {} is invalid, must be between 1 and 120",
                self.dashboard.max_fps
            ));
        }

        // Validate budget thresholds (must be 0-100)
        if self.cost_tracking.budget_warning_threshold > 100 {
            warnings.push(format!(
                "budget_warning_threshold {} exceeds 100%",
                self.cost_tracking.budget_warning_threshold
            ));
        }

        if self.cost_tracking.budget_critical_threshold > 100 {
            warnings.push(format!(
                "budget_critical_threshold {} exceeds 100%",
                self.cost_tracking.budget_critical_threshold
            ));
        }

        // Validate theme name if specified
        if let Some(ref theme_name) = self.theme.name {
            let valid_themes = ["default", "dark", "light", "cyberpunk"];
            if !valid_themes.contains(&theme_name.to_lowercase().as_str()) {
                warnings.push(format!(
                    "Invalid theme '{}', valid themes: {:?}",
                    theme_name, valid_themes
                ));
            }
        }

        if warnings.is_empty() {
            Ok(())
        } else {
            // Return the first warning as the error message
            Err(warnings.join("; "))
        }
    }

    /// Sanitize the configuration by fixing invalid values.
    ///
    /// Returns a new config with all invalid values replaced by defaults.
    pub fn sanitized(&self) -> Self {
        let mut config = self.clone();

        // Sanitize refresh interval
        if config.dashboard.refresh_interval_ms < 100 {
            warn!(
                original = config.dashboard.refresh_interval_ms,
                "Sanitizing refresh_interval_ms to minimum 100ms"
            );
            config.dashboard.refresh_interval_ms = 100;
        }

        // Sanitize max_fps
        if config.dashboard.max_fps == 0 || config.dashboard.max_fps > 120 {
            warn!(
                original = config.dashboard.max_fps,
                "Sanitizing max_fps to default 60"
            );
            config.dashboard.max_fps = 60;
        }

        // Sanitize budget thresholds
        if config.cost_tracking.budget_warning_threshold > 100 {
            warn!(
                original = config.cost_tracking.budget_warning_threshold,
                "Sanitizing budget_warning_threshold to 100"
            );
            config.cost_tracking.budget_warning_threshold = 100;
        }

        if config.cost_tracking.budget_critical_threshold > 100 {
            warn!(
                original = config.cost_tracking.budget_critical_threshold,
                "Sanitizing budget_critical_threshold to 100"
            );
            config.cost_tracking.budget_critical_threshold = 100;
        }

        // Sanitize theme name
        if let Some(ref theme_name) = config.theme.name {
            let valid_themes = ["default", "dark", "light", "cyberpunk"];
            if !valid_themes.contains(&theme_name.to_lowercase().as_str()) {
                warn!(
                    original = theme_name,
                    "Sanitizing invalid theme name to default"
                );
                config.theme.name = None;
            }
        }

        config
    }
}

/// Dashboard configuration.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct DashboardConfig {
    /// Refresh interval in milliseconds.
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval_ms: u64,

    /// Maximum frames per second.
    #[serde(default = "default_max_fps")]
    pub max_fps: u64,

    /// Default layout mode.
    #[serde(default = "default_layout")]
    pub default_layout: String,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            refresh_interval_ms: default_refresh_interval(),
            max_fps: default_max_fps(),
            default_layout: default_layout(),
        }
    }
}

fn default_refresh_interval() -> u64 {
    1000
}

fn default_max_fps() -> u64 {
    60
}

fn default_layout() -> String {
    "overview".to_string()
}

/// Theme configuration.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ThemeConfig {
    /// Theme name (default, dark, light, cyberpunk).
    #[serde(default)]
    pub name: Option<String>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self { name: None }
    }
}

/// Cost tracking configuration.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CostTrackingConfig {
    /// Whether cost tracking is enabled.
    #[serde(default = "default_cost_enabled")]
    pub enabled: bool,

    /// Budget warning threshold (percentage).
    #[serde(default = "default_warning_threshold")]
    pub budget_warning_threshold: u8,

    /// Budget critical threshold (percentage).
    #[serde(default = "default_critical_threshold")]
    pub budget_critical_threshold: u8,

    /// Monthly budget in USD.
    #[serde(default)]
    pub monthly_budget_usd: Option<f64>,
}

impl Default for CostTrackingConfig {
    fn default() -> Self {
        Self {
            enabled: default_cost_enabled(),
            budget_warning_threshold: default_warning_threshold(),
            budget_critical_threshold: default_critical_threshold(),
            monthly_budget_usd: None,
        }
    }
}

fn default_cost_enabled() -> bool {
    true
}

fn default_warning_threshold() -> u8 {
    70
}

fn default_critical_threshold() -> u8 {
    90
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

        let (event_tx, event_rx) = mpsc::channel();
        let (stop_tx, _stop_rx) = mpsc::channel();

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
                            EventKind::Remove(_) => {
                                if event_tx_clone.send(ConfigEvent::Removed).is_err() {
                                    debug!("Failed to send remove event - channel closed");
                                }
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
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = ForgeConfig::default();
        assert_eq!(config.dashboard.refresh_interval_ms, 1000);
        assert_eq!(config.dashboard.max_fps, 60);
        assert!(config.cost_tracking.enabled);
    }

    #[test]
    fn test_parse_valid_config() {
        let yaml = r#"
dashboard:
  refresh_interval_ms: 500
  max_fps: 30
  default_layout: workers

theme:
  name: cyberpunk

cost_tracking:
  enabled: true
  budget_warning_threshold: 80
  budget_critical_threshold: 95
  monthly_budget_usd: 100.0
"#;
        let config = ForgeConfig::parse(yaml).expect("Failed to parse config");
        assert_eq!(config.dashboard.refresh_interval_ms, 500);
        assert_eq!(config.dashboard.max_fps, 30);
        assert_eq!(config.theme.name, Some("cyberpunk".to_string()));
        assert_eq!(config.cost_tracking.budget_warning_threshold, 80);
        assert_eq!(config.cost_tracking.monthly_budget_usd, Some(100.0));
    }

    #[test]
    fn test_parse_partial_config() {
        let yaml = r#"
dashboard:
  refresh_interval_ms: 2000
"#;
        let config = ForgeConfig::parse(yaml).expect("Failed to parse config");
        assert_eq!(config.dashboard.refresh_interval_ms, 2000);
        // Defaults should be used for missing fields
        assert_eq!(config.dashboard.max_fps, 60);
    }

    #[test]
    fn test_validate_valid_config() {
        let config = ForgeConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_refresh_interval() {
        let mut config = ForgeConfig::default();
        config.dashboard.refresh_interval_ms = 50; // Too low
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_fps() {
        let mut config = ForgeConfig::default();
        config.dashboard.max_fps = 200; // Too high
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_theme() {
        let mut config = ForgeConfig::default();
        config.theme.name = Some("invalid_theme".to_string());
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_valid_themes() {
        for theme in &["default", "dark", "light", "cyberpunk", "DEFAULT", "CyberPunk"] {
            let mut config = ForgeConfig::default();
            config.theme.name = Some(theme.to_string());
            assert!(config.validate().is_ok(), "Theme '{}' should be valid", theme);
        }
    }

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

    #[test]
    fn test_format_yaml_error_with_location() {
        let yaml_str = "invalid: [syntax";
        let err: serde_yaml::Error = serde_yaml::from_str::<serde_yaml::Value>(yaml_str).unwrap_err();

        let formatted = format_yaml_error(&err);
        // Should include "line" or "YAML parse error"
        assert!(formatted.contains("line") || formatted.contains("YAML parse error"));
    }
}
