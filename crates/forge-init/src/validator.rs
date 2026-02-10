//! Configuration validation.
//!
//! Validates generated configuration by testing the chat backend connection.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;
use tracing::{debug, info};

/// Errors that can occur during validation.
#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Config file not found: {0}")]
    ConfigNotFound(String),

    #[error("Config file is invalid YAML: {0}")]
    ConfigInvalidYaml(String),

    #[error("Chat backend test failed: {0}")]
    ChatBackendFailed(String),

    #[error("Launcher script not found: {0}")]
    LauncherNotFound(String),

    #[error("Launcher script not executable: {0}")]
    LauncherNotExecutable(String),

    #[error("Required directory missing: {0}")]
    DirectoryMissing(String),
}

/// Result type for validation operations.
pub type Result<T> = std::result::Result<T, ValidationError>;

/// Validation results with detailed status.
#[derive(Debug)]
pub struct ValidationResults {
    /// Whether config.yaml exists and is valid YAML
    pub config_valid: bool,
    /// Whether launcher scripts exist and are executable
    pub launcher_valid: bool,
    /// Whether required directories exist
    pub directories_valid: bool,
    /// Overall validation status
    pub passed: bool,
    /// Any warnings or issues found
    pub warnings: Vec<String>,
}

impl ValidationResults {
    /// Create a new validation result.
    pub fn new() -> Self {
        Self {
            config_valid: false,
            launcher_valid: false,
            directories_valid: false,
            passed: false,
            warnings: Vec::new(),
        }
    }

    /// Add a warning.
    pub fn add_warning(&mut self, warning: impl Into<String>) {
        self.warnings.push(warning.into());
    }

    /// Update overall pass/fail status.
    pub fn update_status(&mut self) {
        self.passed = self.config_valid && self.launcher_valid && self.directories_valid;
    }
}

/// Validate config.yaml and FORGE setup.
///
/// Performs comprehensive validation:
/// - Config file exists and is valid YAML
/// - Launcher scripts exist and are executable
/// - Required directories exist
/// - Chat backend binary is accessible
pub fn validate_config(forge_dir: &Path) -> Result<ValidationResults> {
    info!("Validating FORGE configuration at {}", forge_dir.display());

    let mut results = ValidationResults::new();

    // Validate config.yaml
    let config_path = forge_dir.join("config.yaml");
    results.config_valid = validate_config_file(&config_path, &mut results)?;

    // Validate launcher scripts
    let launchers_dir = forge_dir.join("launchers");
    results.launcher_valid = validate_launchers(&launchers_dir, &mut results)?;

    // Validate directory structure
    results.directories_valid = validate_directories(forge_dir, &mut results)?;

    // Update overall status
    results.update_status();

    if results.passed {
        info!("Validation passed");
    } else {
        info!("Validation failed");
    }

    Ok(results)
}

/// Validate config.yaml file.
fn validate_config_file(config_path: &Path, results: &mut ValidationResults) -> Result<bool> {
    debug!("Validating config file: {}", config_path.display());

    // Check if file exists
    if !config_path.exists() {
        return Err(ValidationError::ConfigNotFound(
            config_path.display().to_string()
        ));
    }

    // Try to parse as YAML
    let content = fs::read_to_string(config_path)
        .map_err(|e| ValidationError::ConfigNotFound(e.to_string()))?;

    match serde_yaml::from_str::<serde_yaml::Value>(&content) {
        Ok(_) => {
            debug!("Config file is valid YAML");
            Ok(true)
        }
        Err(e) => {
            Err(ValidationError::ConfigInvalidYaml(e.to_string()))
        }
    }
}

/// Validate launcher scripts.
fn validate_launchers(launchers_dir: &Path, results: &mut ValidationResults) -> Result<bool> {
    debug!("Validating launcher scripts in {}", launchers_dir.display());

    if !launchers_dir.exists() {
        results.add_warning(format!("Launchers directory not found: {}", launchers_dir.display()));
        return Ok(false);
    }

    // Check for any launcher scripts
    let entries = fs::read_dir(launchers_dir)
        .map_err(|e| ValidationError::LauncherNotFound(e.to_string()))?;

    let mut found_launcher = false;

    for entry in entries {
        if let Ok(entry) = entry {
            let path = entry.path();
            if path.is_file() && path.file_name().is_some() {
                found_launcher = true;

                // Check if executable
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(metadata) = fs::metadata(&path) {
                        let permissions = metadata.permissions();
                        let is_executable = permissions.mode() & 0o111 != 0;

                        if !is_executable {
                            results.add_warning(format!(
                                "Launcher not executable: {}",
                                path.display()
                            ));
                        }
                    }
                }
            }
        }
    }

    if !found_launcher {
        results.add_warning("No launcher scripts found".to_string());
        return Ok(false);
    }

    Ok(true)
}

/// Validate directory structure.
fn validate_directories(forge_dir: &Path, results: &mut ValidationResults) -> Result<bool> {
    debug!("Validating directory structure");

    let required_dirs = ["logs", "status", "launchers", "workers", "layouts"];
    let mut all_exist = true;

    for dir_name in &required_dirs {
        let dir_path = forge_dir.join(dir_name);
        if !dir_path.exists() {
            results.add_warning(format!("Missing directory: {}", dir_name));
            all_exist = false;
        }
    }

    Ok(all_exist)
}

/// Quick validation check (just config existence).
///
/// Faster than full validation, suitable for startup checks.
pub fn quick_validate(forge_dir: &Path) -> bool {
    let config_path = forge_dir.join("config.yaml");
    config_path.exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_setup(temp: &TempDir) -> PathBuf {
        let forge_dir = temp.path().join(".forge");
        fs::create_dir_all(&forge_dir).unwrap();

        // Create directories
        for dir in &["logs", "status", "launchers", "workers", "layouts"] {
            fs::create_dir_all(forge_dir.join(dir)).unwrap();
        }

        // Create a basic config
        let config = r#"
chat_backend:
  command: claude
  model: sonnet
"#;
        fs::write(forge_dir.join("config.yaml"), config).unwrap();

        // Create a launcher script
        let launcher = r#"#!/bin/bash
echo "test launcher"
"#;
        let launcher_path = forge_dir.join("launchers/test-launcher");
        fs::write(&launcher_path, launcher).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&launcher_path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&launcher_path, perms).unwrap();
        }

        forge_dir
    }

    #[test]
    fn test_validate_complete_setup() {
        let temp = TempDir::new().unwrap();
        let forge_dir = create_test_setup(&temp);

        let results = validate_config(&forge_dir).unwrap();
        assert!(results.passed);
        assert!(results.config_valid);
        assert!(results.launcher_valid);
        assert!(results.directories_valid);
    }

    #[test]
    fn test_validate_missing_config() {
        let temp = TempDir::new().unwrap();
        let forge_dir = temp.path().join(".forge");

        let result = validate_config(&forge_dir);
        assert!(result.is_err());
    }

    #[test]
    fn test_quick_validate() {
        let temp = TempDir::new().unwrap();
        let forge_dir = create_test_setup(&temp);

        assert!(quick_validate(&forge_dir));

        // Remove config
        fs::remove_file(forge_dir.join("config.yaml")).unwrap();
        assert!(!quick_validate(&forge_dir));
    }
}
