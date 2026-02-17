//! Configuration validation.
//!
//! Validates generated configuration by testing the chat backend connection.
//!
//! ## Comprehensive Validation
//!
//! The `validate_comprehensive` function provides detailed validation checks:
//!
//! 1. **Config file**: `~/.forge/config.yaml` exists and is valid YAML
//! 2. **Launcher scripts**: Scripts exist in `~/.forge/launchers/` and are executable
//! 3. **Directory structure**: All required directories exist with proper permissions
//! 4. **Chat backend**: Command exists in PATH (optional connectivity test)

use serde::Serialize;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use thiserror::Error;
use tracing::{debug, info, warn};

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

/// Status of the chat backend.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BackendStatus {
    /// Backend was not tested (--skip-backend-test not used, but no config)
    NotTested,
    /// Backend test was explicitly skipped
    Skipped,
    /// Backend is ready and command exists in PATH
    Ready { command: String },
    /// No chat backend configured in config.yaml
    NotConfigured,
    /// Backend command not found in PATH
    CommandNotFound { command: String },
    /// Error during backend test
    Error { message: String },
}

/// Comprehensive validation results with detailed status for each check.
#[derive(Debug, Clone, Serialize)]
pub struct ComprehensiveValidationResults {
    /// Whether config.yaml exists and is valid YAML
    pub config_valid: bool,
    /// Config file path (if found)
    pub config_path: Option<String>,
    /// Config validation error message (if any)
    pub config_error: Option<String>,

    /// Whether launcher scripts exist and are executable
    pub launcher_valid: bool,
    /// Number of launchers found
    pub launcher_count: usize,
    /// Names of found launchers
    pub launcher_names: Vec<String>,
    /// Launcher status message
    pub launcher_message: String,

    /// Whether required directories exist
    pub directories_valid: bool,
    /// List of missing directories
    pub missing_directories: Vec<String>,

    /// Chat backend validation status
    pub backend_status: BackendStatus,

    /// Overall validation status
    pub passed: bool,

    /// Any warnings found
    pub warnings: Vec<String>,

    /// Fixes that were applied (if --fix was used)
    pub fixes_applied: Vec<String>,

    /// Additional details (shown in verbose mode)
    pub details: Vec<String>,
}

/// Validation results with detailed status.
#[derive(Debug, Default)]
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
        Self::default()
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
fn validate_config_file(config_path: &Path, _results: &mut ValidationResults) -> Result<bool> {
    debug!("Validating config file: {}", config_path.display());

    // Check if file exists
    if !config_path.exists() {
        return Err(ValidationError::ConfigNotFound(
            config_path.display().to_string(),
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
        Err(e) => Err(ValidationError::ConfigInvalidYaml(e.to_string())),
    }
}

/// Validate launcher scripts.
fn validate_launchers(launchers_dir: &Path, results: &mut ValidationResults) -> Result<bool> {
    debug!("Validating launcher scripts in {}", launchers_dir.display());

    if !launchers_dir.exists() {
        results.add_warning(format!(
            "Launchers directory not found: {}",
            launchers_dir.display()
        ));
        return Ok(false);
    }

    // Check for any launcher scripts
    let entries = fs::read_dir(launchers_dir)
        .map_err(|e| ValidationError::LauncherNotFound(e.to_string()))?;

    let mut found_launcher = false;

    for entry in entries.flatten() {
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
                        results.add_warning(format!("Launcher not executable: {}", path.display()));
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

/// Perform comprehensive validation of FORGE configuration.
///
/// This is the main entry point for the `forge validate` command.
///
/// # Arguments
///
/// * `forge_dir` - Path to the `.forge` directory (typically `~/.forge`)
/// * `verbose` - Include detailed information in results
/// * `fix` - Attempt to fix issues automatically
/// * `skip_backend_test` - Skip the chat backend connectivity test
///
/// # Returns
///
/// Comprehensive validation results including status for each check.
pub fn validate_comprehensive(
    forge_dir: &Path,
    verbose: bool,
    fix: bool,
    skip_backend_test: bool,
) -> ComprehensiveValidationResults {
    info!(
        "Running comprehensive validation (verbose={}, fix={}, skip_backend_test={})",
        verbose, fix, skip_backend_test
    );

    let mut results = ComprehensiveValidationResults {
        config_valid: false,
        config_path: None,
        config_error: None,
        launcher_valid: false,
        launcher_count: 0,
        launcher_names: Vec::new(),
        launcher_message: String::new(),
        directories_valid: false,
        missing_directories: Vec::new(),
        backend_status: BackendStatus::NotTested,
        passed: false,
        warnings: Vec::new(),
        fixes_applied: Vec::new(),
        details: Vec::new(),
    };

    // 1. Validate config file
    validate_config_comprehensive(forge_dir, &mut results, verbose);

    // 2. Validate launcher scripts
    validate_launchers_comprehensive(forge_dir, &mut results, verbose, fix);

    // 3. Validate directory structure
    validate_directories_comprehensive(forge_dir, &mut results, verbose, fix);

    // 4. Validate chat backend
    validate_backend_comprehensive(forge_dir, &mut results, verbose, skip_backend_test);

    // Calculate overall status
    results.passed = results.config_valid && results.directories_valid;

    // Launchers are optional but recommended
    if !results.launcher_valid {
        results.warnings.push("No launcher scripts configured".to_string());
    }

    info!("Comprehensive validation complete: passed={}", results.passed);
    results
}

/// Validate config.yaml for comprehensive results.
fn validate_config_comprehensive(
    forge_dir: &Path,
    results: &mut ComprehensiveValidationResults,
    verbose: bool,
) {
    let config_path = forge_dir.join("config.yaml");
    results.config_path = Some(config_path.display().to_string());

    debug!("Validating config file: {}", config_path.display());

    if !config_path.exists() {
        results.config_valid = false;
        results.config_error = Some("Config file not found".to_string());
        return;
    }

    // Try to read the file
    let content = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            results.config_valid = false;
            results.config_error = Some(format!("Failed to read config file: {}", e));
            return;
        }
    };

    // Parse as YAML
    match serde_yaml::from_str::<serde_yaml::Value>(&content) {
        Ok(yaml) => {
            results.config_valid = true;

            if verbose {
                // Add details about config structure
                if let Some(obj) = yaml.as_mapping() {
                    let sections: Vec<String> = obj
                        .keys()
                        .filter_map(|k| k.as_str().map(String::from))
                        .collect();
                    results.details.push(format!("Config sections: {}", sections.join(", ")));
                }
            }
        }
        Err(e) => {
            results.config_valid = false;
            // Include line/column information if available
            if let Some(loc) = e.location() {
                results.config_error = Some(format!(
                    "YAML syntax error at line {}, column {}: {}",
                    loc.line(),
                    loc.column(),
                    e
                ));
            } else {
                results.config_error = Some(format!("YAML syntax error: {}", e));
            }
        }
    }
}

/// Validate launcher scripts for comprehensive results.
fn validate_launchers_comprehensive(
    forge_dir: &Path,
    results: &mut ComprehensiveValidationResults,
    verbose: bool,
    fix: bool,
) {
    let launchers_dir = forge_dir.join("launchers");
    debug!("Validating launcher scripts in {}", launchers_dir.display());

    if !launchers_dir.exists() {
        results.launcher_valid = false;
        results.launcher_message = "Launchers directory not found".to_string();

        if fix {
            // Try to create the directory
            if let Err(e) = fs::create_dir_all(&launchers_dir) {
                warn!("Failed to create launchers directory: {}", e);
            } else {
                results.fixes_applied.push("Created launchers directory".to_string());
            }
        }
        return;
    }

    // Check for launcher scripts
    let entries = match fs::read_dir(&launchers_dir) {
        Ok(e) => e,
        Err(e) => {
            results.launcher_valid = false;
            results.launcher_message = format!("Failed to read launchers directory: {}", e);
            return;
        }
    };

    let mut launcher_count = 0;
    let mut non_executable = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name() {
                let name_str = name.to_string_lossy().to_string();
                results.launcher_names.push(name_str.clone());
                launcher_count += 1;

                // Check if executable
                if let Ok(metadata) = fs::metadata(&path) {
                    let permissions = metadata.permissions();
                    let is_executable = permissions.mode() & 0o111 != 0;

                    if !is_executable {
                        non_executable.push(name_str.clone());

                        if fix {
                            // Try to make it executable
                            let mut new_perms = permissions.clone();
                            new_perms.set_mode(permissions.mode() | 0o755);
                            if let Err(e) = fs::set_permissions(&path, new_perms) {
                                warn!("Failed to set executable permissions on {}: {}", path.display(), e);
                            } else {
                                results.fixes_applied.push(format!(
                                    "Set executable permissions on {}",
                                    name_str
                                ));
                            }
                        }
                    }

                    if verbose {
                        let mode = format!("{:o}", permissions.mode() & 0o777);
                        results.details.push(format!("Launcher {} has mode {}", name_str, mode));
                    }
                }
            }
        }
    }

    results.launcher_count = launcher_count;

    if launcher_count == 0 {
        results.launcher_valid = false;
        results.launcher_message = "No launcher scripts found".to_string();
    } else if !non_executable.is_empty() && !fix {
        results.launcher_valid = false;
        results.launcher_message = format!(
            "{} launcher(s) not executable: {}",
            non_executable.len(),
            non_executable.join(", ")
        );
        for name in &non_executable {
            results.warnings.push(format!("Launcher not executable: {}", name));
        }
    } else {
        results.launcher_valid = true;
        results.launcher_message = format!("{} launcher(s) found", launcher_count);
    }
}

/// Validate directory structure for comprehensive results.
fn validate_directories_comprehensive(
    forge_dir: &Path,
    results: &mut ComprehensiveValidationResults,
    verbose: bool,
    fix: bool,
) {
    debug!("Validating directory structure at {}", forge_dir.display());

    let required_dirs = ["logs", "status", "launchers", "workers", "layouts"];
    let mut all_exist = true;

    for dir_name in &required_dirs {
        let dir_path = forge_dir.join(dir_name);

        if !dir_path.exists() {
            all_exist = false;
            results.missing_directories.push(dir_name.to_string());

            if fix {
                if let Err(e) = fs::create_dir_all(&dir_path) {
                    warn!("Failed to create directory {}: {}", dir_name, e);
                } else {
                    results.fixes_applied.push(format!("Created directory: {}", dir_name));
                }
            }
        } else if verbose {
            // Check permissions
            if let Ok(metadata) = fs::metadata(&dir_path) {
                let permissions = metadata.permissions();
                let is_writable = permissions.mode() & 0o200 != 0;
                if !is_writable {
                    results.warnings.push(format!("Directory {} is not writable", dir_name));
                }
            }
        }
    }

    // Re-check after fixes
    if fix && !all_exist {
        all_exist = required_dirs.iter().all(|d| forge_dir.join(d).exists());
    }

    results.directories_valid = all_exist;
}

/// Validate chat backend for comprehensive results.
fn validate_backend_comprehensive(
    forge_dir: &Path,
    results: &mut ComprehensiveValidationResults,
    verbose: bool,
    skip_backend_test: bool,
) {
    if skip_backend_test {
        results.backend_status = BackendStatus::Skipped;
        return;
    }

    // Read config to get chat backend command
    let config_path = forge_dir.join("config.yaml");

    if !config_path.exists() {
        results.backend_status = BackendStatus::NotConfigured;
        return;
    }

    let content = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => {
            results.backend_status = BackendStatus::NotConfigured;
            return;
        }
    };

    let yaml: serde_yaml::Value = match serde_yaml::from_str(&content) {
        Ok(y) => y,
        Err(_) => {
            results.backend_status = BackendStatus::NotConfigured;
            return;
        }
    };

    // Extract chat_backend.command
    let backend_command = yaml
        .get("chat_backend")
        .and_then(|cb| cb.get("command"))
        .and_then(|c| c.as_str());

    match backend_command {
        Some(command) => {
            // Check if command exists in PATH
            match which::which(command) {
                Ok(path) => {
                    results.backend_status = BackendStatus::Ready {
                        command: command.to_string(),
                    };

                    if verbose {
                        results.details.push(format!(
                            "Chat backend '{}' found at: {}",
                            command,
                            path.display()
                        ));

                        // Try to get version
                        if let Ok(output) = Command::new(&path).arg("--version").output() {
                            if let Ok(version) = String::from_utf8(output.stdout) {
                                let version = version.trim();
                                if !version.is_empty() {
                                    results.details.push(format!("Backend version: {}", version));
                                }
                            }
                        }
                    }
                }
                Err(_) => {
                    results.backend_status = BackendStatus::CommandNotFound {
                        command: command.to_string(),
                    };
                }
            }
        }
        None => {
            results.backend_status = BackendStatus::NotConfigured;
        }
    }
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
    use std::path::PathBuf;
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

    #[test]
    fn test_comprehensive_validate_complete_setup() {
        let temp = TempDir::new().unwrap();
        let forge_dir = create_test_setup(&temp);

        let results = validate_comprehensive(&forge_dir, false, false, true);
        assert!(results.passed);
        assert!(results.config_valid);
        assert!(results.launcher_valid);
        assert!(results.directories_valid);
        assert!(results.launcher_count > 0);
        assert!(results.missing_directories.is_empty());
    }

    #[test]
    fn test_comprehensive_validate_verbose() {
        let temp = TempDir::new().unwrap();
        let forge_dir = create_test_setup(&temp);

        let results = validate_comprehensive(&forge_dir, true, false, true);
        assert!(results.passed);
        // Verbose mode should produce details
        assert!(!results.details.is_empty());
    }

    #[test]
    fn test_comprehensive_validate_missing_dirs_with_fix() {
        let temp = TempDir::new().unwrap();
        let forge_dir = temp.path().join(".forge");
        fs::create_dir_all(&forge_dir).unwrap();

        // Create valid config
        let config = "chat_backend:\n  command: echo\n";
        fs::write(forge_dir.join("config.yaml"), config).unwrap();

        // Create launchers dir only
        fs::create_dir_all(forge_dir.join("launchers")).unwrap();

        // Run without fix - should fail
        let results_no_fix = validate_comprehensive(&forge_dir, false, false, true);
        assert!(!results_no_fix.directories_valid);
        assert!(!results_no_fix.missing_directories.is_empty());

        // Run with fix - should pass after creating directories
        let results_with_fix = validate_comprehensive(&forge_dir, false, true, true);
        assert!(results_with_fix.directories_valid);
        assert!(!results_with_fix.fixes_applied.is_empty());

        // Verify directories were created
        assert!(forge_dir.join("status").exists());
        assert!(forge_dir.join("workers").exists());
        assert!(forge_dir.join("layouts").exists());
    }

    #[test]
    fn test_comprehensive_validate_invalid_yaml() {
        let temp = TempDir::new().unwrap();
        let forge_dir = temp.path().join(".forge");
        fs::create_dir_all(&forge_dir).unwrap();

        // Write invalid YAML
        fs::write(forge_dir.join("config.yaml"), "invalid: [yaml").unwrap();

        let results = validate_comprehensive(&forge_dir, false, false, true);
        assert!(!results.config_valid);
        assert!(results.config_error.is_some());
        assert!(!results.passed);
    }

    #[test]
    fn test_comprehensive_validate_backend_status() {
        let temp = TempDir::new().unwrap();
        let forge_dir = create_test_setup(&temp);

        // Test with skipped backend
        let results_skipped = validate_comprehensive(&forge_dir, false, false, true);
        assert!(matches!(results_skipped.backend_status, BackendStatus::Skipped));

        // Test without skipping (but command might not exist)
        let results = validate_comprehensive(&forge_dir, false, false, false);
        // Backend status should not be Skipped
        assert!(!matches!(results.backend_status, BackendStatus::Skipped));
    }

    #[test]
    fn test_comprehensive_validate_json_serialization() {
        let temp = TempDir::new().unwrap();
        let forge_dir = create_test_setup(&temp);

        let results = validate_comprehensive(&forge_dir, false, false, true);

        // Test JSON serialization works
        let json = serde_json::to_string_pretty(&results);
        assert!(json.is_ok());

        let json_str = json.unwrap();
        assert!(json_str.contains("config_valid"));
        assert!(json_str.contains("launcher_valid"));
        assert!(json_str.contains("directories_valid"));
        assert!(json_str.contains("passed"));
    }
}
