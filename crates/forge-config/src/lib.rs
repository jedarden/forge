//! Configuration management for FORGE.
//!
//! This crate handles loading, validating, and managing FORGE configuration
//! from `~/.forge/config.yaml`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use thiserror::Error;

/// Default config file path (~/.forge/config.yaml).
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
            ConfigLoadError::ParseError { error, .. } => error.location().map(|loc| loc.line()),
            _ => None,
        }
    }

    /// Get the column number where the error occurred (if available).
    pub fn column_number(&self) -> Option<usize> {
        match self {
            ConfigLoadError::ParseError { error, .. } => error.location().map(|loc| loc.column()),
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

/// Forge configuration structure.
///
/// This represents the subset of config.yaml that can be hot-reloaded.
/// Changes to these fields will take effect immediately.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ForgeConfig {
    /// Enable the periodic complexity-threshold calibration job.
    #[serde(default = "default_auto_calibrate")]
    pub auto_calibrate: bool,

    /// Thresholds produced by the automatic calibration job.
    ///
    /// This is intentionally separate from user-authored settings. The
    /// calibration job updates this overlay instead of replacing any of the
    /// other configuration sections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibrated_thresholds: Option<CalibratedThresholds>,

    /// Dashboard configuration
    #[serde(default)]
    pub dashboard: DashboardConfig,

    /// Theme configuration
    #[serde(default)]
    pub theme: ThemeConfig,

    /// Cost tracking configuration
    #[serde(default)]
    pub cost_tracking: CostTrackingConfig,

    /// Auto-recovery configuration
    #[serde(default)]
    pub auto_recovery: AutoRecoveryConfig,

    /// Worker defaults configuration
    #[serde(default)]
    pub workers: WorkerConfig,

    /// Worker pool configuration (warm spares per model tier, failover policy).
    #[serde(default)]
    pub worker_pool: WorkerPoolConfig,

    /// Notification configuration
    #[serde(default)]
    pub notifications: NotificationsConfig,
}

impl Default for ForgeConfig {
    fn default() -> Self {
        Self {
            auto_calibrate: default_auto_calibrate(),
            calibrated_thresholds: None,
            dashboard: DashboardConfig::default(),
            theme: ThemeConfig::default(),
            cost_tracking: CostTrackingConfig::default(),
            auto_recovery: AutoRecoveryConfig::default(),
            workers: WorkerConfig::default(),
            worker_pool: WorkerPoolConfig::default(),
            notifications: NotificationsConfig::default(),
        }
    }
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
            tracing::debug!("Config file does not exist: {:?} - using defaults", path);
            return None;
        }

        // Try to read the file
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
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
        let content = std::fs::read_to_string(path).map_err(|e| ConfigLoadError::ReadError {
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
                    tracing::warn!(
                        error = %e,
                        "Config validation warning - some settings may be ignored"
                    );
                }
                tracing::debug!("Successfully parsed forge config");
                Some(config)
            }
            Err(e) => {
                // Format detailed error message with line/column information
                let error_msg = format_yaml_error(&e);
                tracing::warn!(
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
                tracing::warn!(
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

        let auto_calibrate = yaml
            .get("auto_calibrate")
            .and_then(|v| serde_yaml::from_value(v.clone()).ok())
            .unwrap_or_else(default_auto_calibrate);

        let calibrated_thresholds = yaml
            .get("calibrated_thresholds")
            .and_then(|v| serde_yaml::from_value(v.clone()).ok());

        let theme = yaml
            .get("theme")
            .and_then(|v| serde_yaml::from_value(v.clone()).ok())
            .unwrap_or_default();

        let cost_tracking = yaml
            .get("cost_tracking")
            .and_then(|v| serde_yaml::from_value(v.clone()).ok())
            .unwrap_or_default();

        let auto_recovery = yaml
            .get("auto_recovery")
            .and_then(|v| serde_yaml::from_value(v.clone()).ok())
            .unwrap_or_default();

        let workers = yaml
            .get("workers")
            .and_then(|v| serde_yaml::from_value(v.clone()).ok())
            .unwrap_or_default();

        let worker_pool = yaml
            .get("worker_pool")
            .and_then(|v| serde_yaml::from_value(v.clone()).ok())
            .unwrap_or_default();

        let notifications = yaml
            .get("notifications")
            .and_then(|v| serde_yaml::from_value(v.clone()).ok())
            .unwrap_or_default();

        tracing::info!("Loaded partial config - some sections may use defaults");

        Some(Self {
            auto_calibrate,
            calibrated_thresholds,
            dashboard,
            theme,
            cost_tracking,
            auto_recovery,
            workers,
            worker_pool,
            notifications,
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

        if let Some(thresholds) = self.calibrated_thresholds
            && (thresholds.budget == 0
                || thresholds.budget >= thresholds.standard
                || thresholds.standard >= 100)
        {
            warnings.push(format!(
                "calibrated_thresholds must satisfy 0 < budget < standard < 100, got {} and {}",
                thresholds.budget, thresholds.standard
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

        // Validate max workers
        if self.workers.max_workers == 0 {
            warnings.push("max_workers must be at least 1".to_string());
        }

        // Validate worker pool settings (only when enabled — a disabled pool
        // with a bad policy string is inert and defaults are valid anyway).
        if self.worker_pool.enabled {
            if !WorkerPoolConfig::VALID_POLICIES
                .contains(&self.worker_pool.recovery_policy.to_lowercase().as_str())
            {
                warnings.push(format!(
                    "worker_pool.recovery_policy '{}' is invalid, valid policies: {:?}",
                    self.worker_pool.recovery_policy,
                    WorkerPoolConfig::VALID_POLICIES
                ));
            }

            for name in self.worker_pool.tiers.keys() {
                if !WorkerPoolConfig::VALID_TIERS.contains(&name.to_lowercase().as_str()) {
                    warnings.push(format!(
                        "worker_pool.tiers key '{}' is invalid, valid tiers: {:?}",
                        name,
                        WorkerPoolConfig::VALID_TIERS
                    ));
                }
            }
            for (name, tier) in &self.worker_pool.tiers {
                if tier.size > WorkerPoolConfig::MAX_TIER_SIZE {
                    warnings.push(format!(
                        "worker_pool.tiers.{}.size {} exceeds the maximum of {}",
                        name,
                        tier.size,
                        WorkerPoolConfig::MAX_TIER_SIZE
                    ));
                }
            }
            if self.worker_pool.backoff_base_secs == 0 {
                warnings.push("worker_pool.backoff_base_secs must be at least 1".to_string());
            }
            if self.worker_pool.backoff_max_secs < self.worker_pool.backoff_base_secs {
                warnings.push(format!(
                    "worker_pool.backoff_max_secs ({}) must be >= backoff_base_secs ({})",
                    self.worker_pool.backoff_max_secs, self.worker_pool.backoff_base_secs
                ));
            }
        }

        // Validate default model
        let valid_models = ["sonnet", "opus", "haiku", "glm"];
        if !valid_models.contains(&self.workers.default_model.to_lowercase().as_str()) {
            warnings.push(format!(
                "Invalid default_model '{}', valid models: {:?}",
                self.workers.default_model, valid_models
            ));
        }

        // Validate sonnet costs (must be non-negative)
        if self.cost_tracking.sonnet_cost_per_1k_input < 0.0 {
            warnings.push("sonnet_cost_per_1k_input must be non-negative".to_string());
        }
        if self.cost_tracking.sonnet_cost_per_1k_output < 0.0 {
            warnings.push("sonnet_cost_per_1k_output must be non-negative".to_string());
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
            tracing::warn!(
                original = config.dashboard.refresh_interval_ms,
                "Sanitizing refresh_interval_ms to minimum 100ms"
            );
            config.dashboard.refresh_interval_ms = 100;
        }

        // Sanitize max_fps
        if config.dashboard.max_fps == 0 || config.dashboard.max_fps > 120 {
            tracing::warn!(
                original = config.dashboard.max_fps,
                "Sanitizing max_fps to default 60"
            );
            config.dashboard.max_fps = 60;
        }

        // Sanitize budget thresholds
        if config.cost_tracking.budget_warning_threshold > 100 {
            tracing::warn!(
                original = config.cost_tracking.budget_warning_threshold,
                "Sanitizing budget_warning_threshold to 100"
            );
            config.cost_tracking.budget_warning_threshold = 100;
        }

        if config.cost_tracking.budget_critical_threshold > 100 {
            tracing::warn!(
                original = config.cost_tracking.budget_critical_threshold,
                "Sanitizing budget_critical_threshold to 100"
            );
            config.cost_tracking.budget_critical_threshold = 100;
        }

        // Sanitize theme name
        if let Some(ref theme_name) = config.theme.name {
            let valid_themes = ["default", "dark", "light", "cyberpunk"];
            if !valid_themes.contains(&theme_name.to_lowercase().as_str()) {
                tracing::warn!(
                    original = theme_name,
                    "Sanitizing invalid theme name to default"
                );
                config.theme.name = None;
            }
        }

        // Sanitize max workers
        if config.workers.max_workers == 0 {
            tracing::warn!(
                original = config.workers.max_workers,
                "Sanitizing max_workers to minimum 1"
            );
            config.workers.max_workers = 1;
        }

        // Sanitize worker pool settings
        if !WorkerPoolConfig::VALID_POLICIES
            .contains(&config.worker_pool.recovery_policy.to_lowercase().as_str())
        {
            tracing::warn!(
                original = config.worker_pool.recovery_policy,
                "Sanitizing invalid worker_pool.recovery_policy to 'alert'"
            );
            config.worker_pool.recovery_policy = "alert".to_string();
        }

        if config.worker_pool.backoff_base_secs == 0 {
            config.worker_pool.backoff_base_secs = default_pool_backoff_base();
        }
        if config.worker_pool.backoff_max_secs < config.worker_pool.backoff_base_secs {
            tracing::warn!(
                original = config.worker_pool.backoff_max_secs,
                base = config.worker_pool.backoff_base_secs,
                "Sanitizing worker_pool.backoff_max_secs to match backoff_base_secs"
            );
            config.worker_pool.backoff_max_secs = config.worker_pool.backoff_base_secs;
        }

        // Drop invalid tier entries and clamp oversized tiers
        config.worker_pool.tiers.retain(|name, _| {
            WorkerPoolConfig::VALID_TIERS.contains(&name.to_lowercase().as_str())
        });
        for (name, tier) in &mut config.worker_pool.tiers {
            if tier.size > WorkerPoolConfig::MAX_TIER_SIZE {
                tracing::warn!(
                    tier = name,
                    original = tier.size,
                    "Clamping worker pool tier size to maximum"
                );
                tier.size = WorkerPoolConfig::MAX_TIER_SIZE;
            }
        }

        // Sanitize default model
        let valid_models = ["sonnet", "opus", "haiku", "glm"];
        if !valid_models.contains(&config.workers.default_model.to_lowercase().as_str()) {
            tracing::warn!(
                original = config.workers.default_model,
                "Sanitizing invalid default_model to sonnet"
            );
            config.workers.default_model = "sonnet".to_string();
        }

        // Sanitize sonnet costs
        if config.cost_tracking.sonnet_cost_per_1k_input < 0.0 {
            config.cost_tracking.sonnet_cost_per_1k_input = 0.003;
        }
        if config.cost_tracking.sonnet_cost_per_1k_output < 0.0 {
            config.cost_tracking.sonnet_cost_per_1k_output = 0.015;
        }

        config
    }

    /// Save configuration to a file.
    ///
    /// Writes the configuration as YAML to the specified path.
    /// Creates parent directories if they don't exist.
    pub fn save_to(&self, path: &PathBuf) -> Result<(), ConfigLoadError> {
        // Create parent directory if needed
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                ConfigLoadError::ValidationError(format!(
                    "Failed to create config directory: {}",
                    e
                ))
            })?;
        }

        // Serialize to YAML
        let yaml = serde_yaml::to_string(self).map_err(|e| {
            ConfigLoadError::ValidationError(format!("Failed to serialize config: {}", e))
        })?;

        // Write to file
        std::fs::write(path, yaml).map_err(|e| {
            ConfigLoadError::ValidationError(format!("Failed to write config file: {}", e))
        })?;

        tracing::info!("Saved configuration to {:?}", path);
        Ok(())
    }

    /// Save configuration to the default path (~/.forge/config.yaml).
    pub fn save(&self) -> Result<(), ConfigLoadError> {
        let path = config_path().ok_or(ConfigLoadError::NoHomePath)?;
        self.save_to(&path)
    }
}

/// Complexity cutoffs generated from empirical task outcomes.
///
/// `budget` and `standard` are inclusive upper bounds. Premium tasks have a
/// score above `standard`. These values are an overlay and do not replace any
/// user-authored complexity settings.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub struct CalibratedThresholds {
    /// Inclusive maximum score routed to the budget tier.
    #[serde(alias = "budget_threshold", alias = "budget_max_score")]
    pub budget: u32,

    /// Inclusive maximum score routed to the standard tier.
    #[serde(alias = "standard_threshold", alias = "standard_max_score")]
    pub standard: u32,
}

impl CalibratedThresholds {
    /// Create valid, ordered thresholds.
    pub fn new(budget: u32, standard: u32) -> Self {
        let budget = budget.clamp(1, 98);
        let standard = standard.clamp(budget.saturating_add(1), 99);
        Self { budget, standard }
    }
}

/// Dashboard configuration.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
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

    /// Log retention period in days (0 = forever).
    #[serde(default = "default_log_retention_days")]
    pub log_retention_days: u64,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            refresh_interval_ms: default_refresh_interval(),
            max_fps: default_max_fps(),
            default_layout: default_layout(),
            log_retention_days: default_log_retention_days(),
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

fn default_log_retention_days() -> u64 {
    7
}

/// Theme configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ThemeConfig {
    /// Theme name (default, dark, light, cyberpunk).
    #[serde(default)]
    pub name: Option<String>,
}

/// Worker configuration.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct WorkerConfig {
    /// Maximum concurrent workers.
    #[serde(default = "default_max_workers")]
    pub max_workers: u64,

    /// Default model for new workers.
    #[serde(default = "default_model")]
    pub default_model: String,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            max_workers: default_max_workers(),
            default_model: default_model(),
        }
    }
}

fn default_max_workers() -> u64 {
    10
}

fn default_model() -> String {
    "sonnet".to_string()
}

/// Worker pool configuration.
///
/// The pool keeps a configurable number of ready ("warm spare") workers per
/// model tier and recovers them automatically when health monitoring reports
/// a member dead or unhealthy. Automation is opt-in: `enabled` defaults to
/// `false` and the recovery policy defaults to `alert` (visibility only),
/// consistent with ADR 0014.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct WorkerPoolConfig {
    /// Enable the worker pool. Disabled pools maintain nothing.
    #[serde(default)]
    pub enabled: bool,

    /// Desired ready workers per model tier, keyed by tier name
    /// ("premium", "standard", "budget"). Tiers absent from the map are not pooled.
    #[serde(default)]
    pub tiers: HashMap<String, PoolTierConfig>,

    /// What the pool does when a member is detected dead or unhealthy:
    /// "restart" (respawn in place), "replace" (promote a warm spare, then
    /// refill), or "alert" (report only).
    #[serde(default = "default_pool_recovery_policy")]
    pub recovery_policy: String,

    /// Maximum recovery attempts per pooled worker before it is retired.
    #[serde(default = "default_pool_max_retries")]
    pub max_retries: u32,

    /// Base delay in seconds for exponential backoff between recovery
    /// attempts (delay = base * 2^(attempt-1), capped by `backoff_max_secs`).
    #[serde(default = "default_pool_backoff_base")]
    pub backoff_base_secs: u64,

    /// Upper bound in seconds for exponential backoff between recovery attempts.
    #[serde(default = "default_pool_backoff_max")]
    pub backoff_max_secs: u64,

    /// Ready workers idle longer than this many seconds are torn down when
    /// the tier holds more ready workers than its configured size.
    #[serde(default = "default_pool_idle_timeout")]
    pub idle_timeout_secs: u64,

    /// How often the pool reconciles capacity and health (in seconds).
    #[serde(default = "default_pool_reconcile_interval")]
    pub reconcile_interval_secs: u64,
}

impl WorkerPoolConfig {
    /// Recovery policies accepted by `recovery_policy`.
    pub const VALID_POLICIES: [&'static str; 3] = ["restart", "replace", "alert"];

    /// Tier names accepted as keys of `tiers`.
    pub const VALID_TIERS: [&'static str; 3] = ["premium", "standard", "budget"];

    /// Safety cap for a single tier's size.
    pub const MAX_TIER_SIZE: usize = 64;
}

impl Default for WorkerPoolConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tiers: HashMap::new(),
            recovery_policy: default_pool_recovery_policy(),
            max_retries: default_pool_max_retries(),
            backoff_base_secs: default_pool_backoff_base(),
            backoff_max_secs: default_pool_backoff_max(),
            idle_timeout_secs: default_pool_idle_timeout(),
            reconcile_interval_secs: default_pool_reconcile_interval(),
        }
    }
}

/// Per-tier worker pool settings.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct PoolTierConfig {
    /// Number of ready workers to keep warm for this tier.
    #[serde(default)]
    pub size: usize,

    /// Model to launch workers with. Defaults to the tier's stock model
    /// (premium → opus, standard → sonnet, budget → haiku).
    #[serde(default)]
    pub model: Option<String>,

    /// Workspace new pool workers are launched into. Defaults to the home directory.
    #[serde(default)]
    pub workspace: Option<PathBuf>,

    /// Launcher script used to spawn pool workers.
    /// Defaults to `~/.forge/launcher.sh`.
    #[serde(default)]
    pub launcher: Option<PathBuf>,
}

/// Stock model for a pool tier name.
///
/// Used when a tier entry does not override `model`: premium → opus,
/// standard → sonnet, budget → haiku. Unknown tier names fall back to
/// the standard stock model.
pub fn stock_tier_model(tier: &str) -> &'static str {
    match tier.to_lowercase().as_str() {
        "premium" => "opus",
        "budget" => "haiku",
        _ => "sonnet",
    }
}

/// Launch settings for one pooled tier, with all defaults resolved.
///
/// Produced by [`WorkerPoolConfig::resolve_tier`]; consumers (the worker
/// pool) should not have to re-apply defaults themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTierConfig {
    /// Tier name ("premium", "standard", or "budget").
    pub name: String,

    /// Number of ready workers to keep warm (already clamped to
    /// [`WorkerPoolConfig::MAX_TIER_SIZE`]).
    pub size: usize,

    /// Model to launch workers with.
    pub model: String,

    /// Workspace new pool workers are launched into.
    pub workspace: PathBuf,

    /// Launcher script used to spawn pool workers.
    pub launcher: PathBuf,
}

impl WorkerPoolConfig {
    /// Resolve the launch settings for a tier by name.
    ///
    /// Returns `None` when the tier is not configured or its size is zero
    /// (zero-size tiers are not pooled). The returned size is clamped to
    /// [`WorkerPoolConfig::MAX_TIER_SIZE`].
    pub fn resolve_tier(&self, name: &str) -> Option<ResolvedTierConfig> {
        let tier = self.tiers.get(name)?;
        if tier.size == 0 {
            return None;
        }
        Some(ResolvedTierConfig {
            name: name.to_string(),
            size: tier.size.min(Self::MAX_TIER_SIZE),
            model: tier.resolved_model(name),
            workspace: tier.resolved_workspace(),
            launcher: tier.resolved_launcher(),
        })
    }
}

impl PoolTierConfig {
    /// Model for this tier, falling back to the tier's stock model.
    pub fn resolved_model(&self, tier_name: &str) -> String {
        self.model
            .clone()
            .unwrap_or_else(|| stock_tier_model(tier_name).to_string())
    }

    /// Workspace for pool workers, falling back to the home directory.
    pub fn resolved_workspace(&self) -> PathBuf {
        self.workspace
            .clone()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
    }

    /// Launcher script for pool workers, falling back to
    /// `~/.forge/launcher.sh`.
    pub fn resolved_launcher(&self) -> PathBuf {
        self.launcher.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".forge/launcher.sh")
        })
    }
}

fn default_pool_recovery_policy() -> String {
    "alert".to_string()
}

fn default_pool_max_retries() -> u32 {
    3
}

fn default_pool_backoff_base() -> u64 {
    5
}

fn default_pool_backoff_max() -> u64 {
    300
}

fn default_pool_idle_timeout() -> u64 {
    1800
}

fn default_pool_reconcile_interval() -> u64 {
    30
}

/// Cost tracking configuration.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
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

    /// Cost per 1K input tokens for Sonnet (USD).
    #[serde(default = "default_sonnet_input_cost")]
    pub sonnet_cost_per_1k_input: f64,

    /// Cost per 1K output tokens for Sonnet (USD).
    #[serde(default = "default_sonnet_output_cost")]
    pub sonnet_cost_per_1k_output: f64,
}

impl Default for CostTrackingConfig {
    fn default() -> Self {
        Self {
            enabled: default_cost_enabled(),
            budget_warning_threshold: default_warning_threshold(),
            budget_critical_threshold: default_critical_threshold(),
            monthly_budget_usd: None,
            sonnet_cost_per_1k_input: default_sonnet_input_cost(),
            sonnet_cost_per_1k_output: default_sonnet_output_cost(),
        }
    }
}

/// Auto-recovery configuration.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct AutoRecoveryConfig {
    /// Enable/disable auto-recovery.
    #[serde(default)]
    pub enabled: bool,

    /// How often to run recovery checks (in seconds).
    #[serde(default = "default_recovery_check_interval")]
    pub check_interval_secs: u64,

    /// Policy for dead worker restart: "disabled", "notify", or "auto".
    #[serde(default = "default_recovery_policy")]
    pub dead_worker_policy: String,

    /// Maximum restart attempts for dead workers.
    #[serde(default = "default_max_restart_attempts")]
    pub max_restart_attempts: u8,

    /// Policy for memory leak: "disabled", "notify", or "auto".
    #[serde(default = "default_recovery_policy")]
    pub memory_leak_policy: String,

    /// Memory threshold in MB before considering it a leak.
    #[serde(default = "default_memory_threshold")]
    pub memory_threshold_mb: u64,

    /// Policy for stuck tasks: "disabled", "notify", or "auto".
    #[serde(default = "default_recovery_policy")]
    pub stuck_task_policy: String,

    /// Time in minutes before a task is considered stuck.
    #[serde(default = "default_stuck_timeout")]
    pub stuck_task_timeout_mins: i64,

    /// Policy for stale assignees: "disabled", "notify", or "auto".
    #[serde(default = "default_stale_assignee_policy")]
    pub stale_assignee_policy: String,

    /// Time in minutes before an assignee is considered stale.
    #[serde(default = "default_stale_timeout")]
    pub stale_assignee_timeout_mins: i64,
}

impl Default for AutoRecoveryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            check_interval_secs: default_recovery_check_interval(),
            dead_worker_policy: default_recovery_policy(),
            max_restart_attempts: default_max_restart_attempts(),
            memory_leak_policy: default_recovery_policy(),
            memory_threshold_mb: default_memory_threshold(),
            stuck_task_policy: default_recovery_policy(),
            stuck_task_timeout_mins: default_stuck_timeout(),
            stale_assignee_policy: default_stale_assignee_policy(),
            stale_assignee_timeout_mins: default_stale_timeout(),
        }
    }
}

/// Notification configuration for alerts.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct NotificationsConfig {
    /// Enable terminal bell for critical alerts.
    #[serde(default = "default_bell_enabled")]
    pub bell_on_critical: bool,

    /// Enable terminal bell for warning alerts.
    #[serde(default)]
    pub bell_on_warning: bool,

    /// Minimum interval between bells (in seconds) to avoid spam.
    #[serde(default = "default_bell_interval")]
    pub bell_interval_secs: u64,

    /// Enable visual flash for critical alerts.
    #[serde(default = "default_visual_flash")]
    pub visual_flash_on_critical: bool,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            bell_on_critical: default_bell_enabled(),
            bell_on_warning: false,
            bell_interval_secs: default_bell_interval(),
            visual_flash_on_critical: default_visual_flash(),
        }
    }
}

fn default_bell_enabled() -> bool {
    true
}

fn default_bell_interval() -> u64 {
    30 // Minimum 30 seconds between bells
}

fn default_visual_flash() -> bool {
    true
}

fn default_recovery_check_interval() -> u64 {
    30
}

fn default_recovery_policy() -> String {
    "notify".to_string()
}

fn default_stale_assignee_policy() -> String {
    "auto".to_string()
}

fn default_max_restart_attempts() -> u8 {
    3
}

fn default_memory_threshold() -> u64 {
    2048
}

fn default_stuck_timeout() -> i64 {
    30
}

fn default_stale_timeout() -> i64 {
    60
}

fn default_cost_enabled() -> bool {
    true
}

fn default_auto_calibrate() -> bool {
    true
}

fn default_warning_threshold() -> u8 {
    70
}

fn default_critical_threshold() -> u8 {
    90
}

fn default_sonnet_input_cost() -> f64 {
    0.003
}

fn default_sonnet_output_cost() -> f64 {
    0.015
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ForgeConfig::default();
        assert!(config.auto_calibrate);
        assert!(config.calibrated_thresholds.is_none());
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
    fn test_calibration_config_defaults_and_round_trip() {
        let defaults = ForgeConfig::parse("auto_calibrate: false\n").unwrap();
        assert!(!defaults.auto_calibrate);
        assert!(defaults.calibrated_thresholds.is_none());

        let config =
            ForgeConfig::parse("calibrated_thresholds:\n  budget: 24\n  standard: 55\n").unwrap();
        assert_eq!(
            config.calibrated_thresholds,
            Some(CalibratedThresholds::new(24, 55))
        );
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
        for theme in &[
            "default",
            "dark",
            "light",
            "cyberpunk",
            "DEFAULT",
            "CyberPunk",
        ] {
            let mut config = ForgeConfig::default();
            config.theme.name = Some(theme.to_string());
            assert!(
                config.validate().is_ok(),
                "Theme '{}' should be valid",
                theme
            );
        }
    }

    #[test]
    fn test_default_notifications_config() {
        let config = NotificationsConfig::default();
        assert!(config.bell_on_critical);
        assert!(!config.bell_on_warning);
        assert_eq!(config.bell_interval_secs, 30);
        assert!(config.visual_flash_on_critical);
    }

    #[test]
    fn test_parse_notifications_config() {
        let yaml = r#"
notifications:
  bell_on_critical: false
  bell_on_warning: true
  bell_interval_secs: 60
  visual_flash_on_critical: false
"#;
        let config = ForgeConfig::parse(yaml).expect("Failed to parse config");
        assert!(!config.notifications.bell_on_critical);
        assert!(config.notifications.bell_on_warning);
        assert_eq!(config.notifications.bell_interval_secs, 60);
        assert!(!config.notifications.visual_flash_on_critical);
    }

    #[test]
    fn test_notifications_config_defaults_when_missing() {
        let yaml = r#"
dashboard:
  refresh_interval_ms: 1000
"#;
        let config = ForgeConfig::parse(yaml).expect("Failed to parse config");
        // Notifications should use defaults
        assert!(config.notifications.bell_on_critical);
        assert!(!config.notifications.bell_on_warning);
        assert_eq!(config.notifications.bell_interval_secs, 30);
    }

    // ============================================================
    // Worker pool configuration tests
    // ============================================================

    #[test]
    fn test_worker_pool_defaults() {
        let config = WorkerPoolConfig::default();
        // Opt-in per ADR 0014: pool disabled and alert-only by default.
        assert!(!config.enabled);
        assert_eq!(config.recovery_policy, "alert");
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.backoff_base_secs, 5);
        assert_eq!(config.backoff_max_secs, 300);
        assert_eq!(config.idle_timeout_secs, 1800);
        assert_eq!(config.reconcile_interval_secs, 30);
        assert!(config.tiers.is_empty());
    }

    #[test]
    fn test_worker_pool_section_missing_uses_defaults() {
        let yaml = "dashboard:\n  max_fps: 30\n";
        let config = ForgeConfig::parse(yaml).expect("Failed to parse config");
        assert!(!config.worker_pool.enabled);
        assert!(config.worker_pool.tiers.is_empty());
    }

    #[test]
    fn test_parse_worker_pool_config() {
        let yaml = r#"
worker_pool:
  enabled: true
  recovery_policy: replace
  max_retries: 5
  backoff_base_secs: 10
  backoff_max_secs: 600
  idle_timeout_secs: 900
  reconcile_interval_secs: 15
  tiers:
    premium:
      size: 1
      model: opus
    standard:
      size: 2
    budget:
      size: 0
      workspace: /home/user/project
      launcher: /home/user/.forge/launcher.sh
"#;
        let config = ForgeConfig::parse(yaml).expect("Failed to parse config");
        let pool = &config.worker_pool;
        assert!(pool.enabled);
        assert_eq!(pool.recovery_policy, "replace");
        assert_eq!(pool.max_retries, 5);
        assert_eq!(pool.backoff_base_secs, 10);
        assert_eq!(pool.backoff_max_secs, 600);
        assert_eq!(pool.idle_timeout_secs, 900);
        assert_eq!(pool.reconcile_interval_secs, 15);

        assert_eq!(pool.tiers.len(), 3);
        assert_eq!(pool.tiers["premium"].size, 1);
        assert_eq!(pool.tiers["premium"].model.as_deref(), Some("opus"));
        assert_eq!(pool.tiers["standard"].size, 2);
        assert_eq!(pool.tiers["standard"].model, None);
        assert_eq!(
            pool.tiers["budget"].workspace,
            Some("/home/user/project".into())
        );
        assert_eq!(
            pool.tiers["budget"].launcher,
            Some("/home/user/.forge/launcher.sh".into())
        );
    }

    #[test]
    fn test_validate_rejects_bad_pool_policy() {
        let mut config = ForgeConfig::default();
        config.worker_pool.enabled = true;
        config.worker_pool.recovery_policy = "explode".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_bad_backoff_ordering() {
        let mut config = ForgeConfig::default();
        config.worker_pool.enabled = true;
        config.worker_pool.backoff_base_secs = 10;
        config.worker_pool.backoff_max_secs = 5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_invalid_tier_key() {
        let mut config = ForgeConfig::default();
        config.worker_pool.enabled = true;
        config
            .worker_pool
            .tiers
            .insert("turbo".to_string(), PoolTierConfig::default());
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_ignores_pool_when_disabled() {
        let mut config = ForgeConfig::default();
        config.worker_pool.enabled = false;
        config.worker_pool.recovery_policy = "explode".to_string();
        config.worker_pool.backoff_base_secs = 10;
        config.worker_pool.backoff_max_secs = 5;
        // Disabled pool: policy/backoff misconfigurations are not fatal.
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_sanitize_fixes_pool_policy_and_backoff() {
        let mut config = ForgeConfig::default();
        config.worker_pool.enabled = true;
        config.worker_pool.recovery_policy = "yolo".to_string();
        config.worker_pool.backoff_base_secs = 0;
        config.worker_pool.backoff_max_secs = 0;

        let sanitized = config.sanitized();
        assert_eq!(sanitized.worker_pool.recovery_policy, "alert");
        assert_eq!(sanitized.worker_pool.backoff_base_secs, 5);
        // backoff_max_secs (0) < base (5) → raised to base
        assert_eq!(sanitized.worker_pool.backoff_max_secs, 5);
    }

    #[test]
    fn test_sanitize_drops_invalid_tier_keys_and_clamps_size() {
        let mut config = ForgeConfig::default();
        config.worker_pool.enabled = true;
        config.worker_pool.tiers.insert(
            "turbo".to_string(),
            PoolTierConfig {
                size: 9_999,
                ..Default::default()
            },
        );
        config.worker_pool.tiers.insert(
            "standard".to_string(),
            PoolTierConfig {
                size: 9_999,
                ..Default::default()
            },
        );

        let sanitized = config.sanitized();
        assert!(!sanitized.worker_pool.tiers.contains_key("turbo"));
        let standard = sanitized.worker_pool.tiers.get("standard").unwrap();
        assert_eq!(standard.size, WorkerPoolConfig::MAX_TIER_SIZE);
    }

    #[test]
    fn test_resolve_tier_applies_stock_defaults() {
        let yaml = r#"
worker_pool:
  enabled: true
  tiers:
    premium:
      size: 1
    standard:
      size: 2
      model: glm
    budget:
      size: 0
"#;
        let config = ForgeConfig::parse(yaml).expect("Failed to parse config");

        let premium = config.worker_pool.resolve_tier("premium").unwrap();
        assert_eq!(premium.name, "premium");
        assert_eq!(premium.size, 1);
        assert_eq!(premium.model, "opus");
        assert_eq!(
            premium.launcher,
            dirs::home_dir().unwrap().join(".forge/launcher.sh")
        );

        // Explicit model override wins over the stock model.
        let standard = config.worker_pool.resolve_tier("standard").unwrap();
        assert_eq!(standard.model, "glm");

        // Zero-size tiers are not pooled.
        assert!(config.worker_pool.resolve_tier("budget").is_none());
        // Unconfigured tiers are not pooled either.
        assert!(config.worker_pool.resolve_tier("turbo").is_none());
    }

    #[test]
    fn test_resolve_tier_clamps_size_to_maximum() {
        let mut config = WorkerPoolConfig::default();
        config.tiers.insert(
            "standard".to_string(),
            PoolTierConfig {
                size: WorkerPoolConfig::MAX_TIER_SIZE + 10,
                ..Default::default()
            },
        );
        let resolved = config.resolve_tier("standard").unwrap();
        assert_eq!(resolved.size, WorkerPoolConfig::MAX_TIER_SIZE);
    }

    #[test]
    fn test_stock_tier_model_mapping() {
        assert_eq!(stock_tier_model("premium"), "opus");
        assert_eq!(stock_tier_model("Premium"), "opus");
        assert_eq!(stock_tier_model("standard"), "sonnet");
        assert_eq!(stock_tier_model("budget"), "haiku");
        assert_eq!(stock_tier_model("unknown"), "sonnet");
    }

    #[test]
    fn test_worker_pool_round_trips_through_yaml() {
        let mut config = ForgeConfig::default();
        config.worker_pool.enabled = true;
        config.worker_pool.recovery_policy = "restart".to_string();
        config.worker_pool.tiers.insert(
            "standard".to_string(),
            PoolTierConfig {
                size: 2,
                model: Some("sonnet".to_string()),
                workspace: Some("/tmp/ws".into()),
                launcher: None,
            },
        );

        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: ForgeConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.worker_pool, config.worker_pool);
    }
}
