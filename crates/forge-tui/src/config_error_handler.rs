//! Configuration error handling and recovery.
//!
//! This module provides graceful handling of invalid config files, including:
//! - Detailed error messages with line numbers
//! - Automatic backup before repairs
//! - Interactive repair options (reset to defaults or manual fix)
//! - Non-crashing error recovery

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tracing::{error, info, warn};

/// Config error details with location information.
#[derive(Debug, Clone)]
pub struct ConfigError {
    /// Error message
    pub message: String,
    /// Line number where error occurred (if available)
    pub line: Option<usize>,
    /// Column number where error occurred (if available)
    pub column: Option<usize>,
    /// Path to the config file
    pub path: PathBuf,
}

impl ConfigError {
    /// Create a new config error from a YAML parse error.
    pub fn from_yaml_error(error: &serde_yaml::Error, path: PathBuf) -> Self {
        let (line, column) = if let Some(location) = error.location() {
            (Some(location.line()), Some(location.column()))
        } else {
            (None, None)
        };

        Self {
            message: error.to_string(),
            line,
            column,
            path,
        }
    }

    /// Create a generic config error.
    pub fn new(message: String, path: PathBuf) -> Self {
        Self {
            message,
            line: None,
            column: None,
            path,
        }
    }

    /// Format the error as a user-friendly message.
    pub fn format(&self) -> String {
        let location = if let (Some(line), Some(col)) = (self.line, self.column) {
            format!(" at line {}, column {}", line, col)
        } else if let Some(line) = self.line {
            format!(" at line {}", line)
        } else {
            String::new()
        };

        format!(
            "Configuration error in {}{}:\n  {}",
            self.path.display(),
            location,
            self.message
        )
    }
}

/// Config recovery options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryOption {
    /// Reset config to defaults
    ResetToDefaults,
    /// Exit and let user fix manually
    ExitToFix,
    /// Use default config without modifying file
    UseDefaults,
}

/// Handle a config file error with interactive recovery.
///
/// This function:
/// 1. Displays the error with line numbers
/// 2. Creates a backup of the invalid config
/// 3. Offers recovery options to the user
/// 4. Executes the chosen recovery action
///
/// Returns true if the app should continue, false if it should exit.
pub fn handle_config_error_interactive(error: &ConfigError) -> io::Result<bool> {
    eprintln!("\n❌ {}", error.format());
    eprintln!();

    // Create backup
    if let Err(e) = backup_config(&error.path) {
        eprintln!("⚠️  Warning: Could not create backup: {}", e);
        eprintln!();
    } else {
        let backup_path = get_backup_path(&error.path);
        eprintln!("📁 Backup created: {}", backup_path.display());
        eprintln!();
    }

    // Show recovery options
    eprintln!("Recovery options:");
    eprintln!("  1) Reset to default configuration (overwrites current file)");
    eprintln!("  2) Use defaults without modifying file (temporary)");
    eprintln!("  3) Exit and fix manually");
    eprintln!();
    eprint!("Choose option [1-3]: ");
    io::stdout().flush()?;

    let mut choice = String::new();
    io::stdin().read_line(&mut choice)?;

    match choice.trim() {
        "1" => {
            if let Err(e) = reset_to_defaults(&error.path) {
                eprintln!("❌ Failed to reset config: {}", e);
                eprintln!("   You can manually restore from: {}", get_backup_path(&error.path).display());
                return Ok(false);
            }
            eprintln!("✅ Configuration reset to defaults");
            eprintln!("   Previous config backed up to: {}", get_backup_path(&error.path).display());
            eprintln!();
            Ok(true)
        }
        "2" => {
            eprintln!("⚠️  Using default configuration (file not modified)");
            eprintln!("   Fix the config file manually or delete it to regenerate");
            eprintln!();
            Ok(true)
        }
        "3" | "" => {
            eprintln!("Exiting. Please fix the config file and try again.");
            eprintln!("Backup available at: {}", get_backup_path(&error.path).display());
            Ok(false)
        }
        _ => {
            eprintln!("Invalid choice. Exiting.");
            Ok(false)
        }
    }
}

/// Create a backup of the config file.
fn backup_config(config_path: &Path) -> io::Result<()> {
    if !config_path.exists() {
        return Ok(());
    }

    let backup_path = get_backup_path(config_path);
    fs::copy(config_path, &backup_path)?;
    info!("Created config backup: {}", backup_path.display());
    Ok(())
}

/// Get the backup path for a config file.
fn get_backup_path(config_path: &Path) -> PathBuf {
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let parent = config_path.parent().unwrap_or(Path::new("."));
    let filename = config_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config");
    parent.join(format!("{}.backup.{}", filename, timestamp))
}

/// Reset config file to defaults.
fn reset_to_defaults(config_path: &Path) -> io::Result<()> {
    let default_config = generate_default_config();
    fs::write(config_path, default_config)?;
    info!("Reset config to defaults: {}", config_path.display());
    Ok(())
}

/// Generate a default config.yaml file.
fn generate_default_config() -> String {
    r#"# FORGE Configuration
# This file was automatically generated after a config error.

dashboard:
  refresh_interval_ms: 1000
  max_fps: 60
  default_layout: overview

theme:
  name: default

cost_tracking:
  enabled: true
  budget_warning_threshold: 70
  budget_critical_threshold: 90
  # monthly_budget_usd: 100.0

# Chat backend configuration (optional)
# Uncomment and configure if using the chat feature:
# chat_backend:
#   command: claude-code
#   args:
#     - --headless
#     - --model
#     - sonnet
#   model: sonnet
"#
    .to_string()
}

/// Non-interactive error handling for automated/headless environments.
///
/// This function logs the error and uses defaults without user interaction.
pub fn handle_config_error_non_interactive(error: &ConfigError) -> io::Result<()> {
    error!("{}", error.format());

    // Create backup
    if let Err(e) = backup_config(&error.path) {
        warn!("Could not create backup: {}", e);
    } else {
        let backup_path = get_backup_path(&error.path);
        info!("Backup created: {}", backup_path.display());
    }

    warn!("Using default configuration (file not modified)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_config_error_formatting() {
        let error = ConfigError {
            message: "Invalid YAML syntax".to_string(),
            line: Some(10),
            column: Some(5),
            path: PathBuf::from("/home/user/.forge/config.yaml"),
        };

        let formatted = error.format();
        assert!(formatted.contains("line 10"));
        assert!(formatted.contains("column 5"));
        assert!(formatted.contains("Invalid YAML syntax"));
    }

    #[test]
    fn test_backup_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");
        fs::write(&config_path, "test content").unwrap();

        backup_config(&config_path).unwrap();

        // Find backup file
        let entries: Vec<_> = fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();

        let backup_exists = entries.iter().any(|e| {
            e.file_name()
                .to_str()
                .map(|s| s.starts_with("config.yaml.backup."))
                .unwrap_or(false)
        });

        assert!(backup_exists, "Backup file should exist");
    }

    #[test]
    fn test_reset_to_defaults() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");
        fs::write(&config_path, "invalid yaml: [").unwrap();

        reset_to_defaults(&config_path).unwrap();

        let content = fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("dashboard:"));
        assert!(content.contains("theme:"));
        assert!(content.contains("cost_tracking:"));
    }

    #[test]
    fn test_default_config_is_valid_yaml() {
        let config = generate_default_config();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&config).unwrap();
        assert!(parsed.get("dashboard").is_some());
        assert!(parsed.get("theme").is_some());
        assert!(parsed.get("cost_tracking").is_some());
    }
}
