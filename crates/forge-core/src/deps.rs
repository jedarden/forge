//! Dependency checking module for FORGE.
//!
//! This module checks for required external dependencies (binaries) at startup
//! and provides clear error messages with installation instructions.
//!
//! ## Required Dependencies
//!
//! - **tmux**: For worker process management
//! - **git**: For version control integration
//!
//! ## Optional Dependencies
//!
//! - **br** (beads_rust): For bead/task management (graceful degradation if missing)
//! - **jq**: For JSON processing (graceful degradation if missing)
//!
//! ## Example
//!
//! ```no_run
//! use forge_core::deps::{check_dependencies, DependencyCheck};
//!
//! fn main() {
//!     let result = check_dependencies();
//!     if !result.is_ready() {
//!         eprintln!("{}", result.format_errors());
//!         if result.has_critical_missing() {
//!             std::process::exit(1);
//!         }
//!     }
//! }
//! ```

use std::process::Command;

/// A required or optional dependency.
#[derive(Debug, Clone)]
pub struct Dependency {
    /// Name of the binary to check
    pub name: &'static str,
    /// Whether this dependency is required (true) or optional (false)
    pub required: bool,
    /// Description of what this dependency is used for
    pub purpose: &'static str,
    /// Installation instructions
    pub install_instructions: &'static str,
}

/// Result of checking a single dependency.
#[derive(Debug, Clone)]
pub struct DependencyStatus {
    /// The dependency that was checked
    pub dep: Dependency,
    /// Whether the dependency was found
    pub found: bool,
    /// Version string if found (optional)
    pub version: Option<String>,
}

impl DependencyStatus {
    /// Check if this dependency is OK (found or optional).
    pub fn is_ok(&self) -> bool {
        self.found || !self.dep.required
    }
}

/// Result of checking all dependencies.
#[derive(Debug)]
pub struct DependencyCheck {
    /// Status of each dependency
    pub statuses: Vec<DependencyStatus>,
}

impl DependencyCheck {
    /// Check if all required dependencies are met.
    pub fn is_ready(&self) -> bool {
        self.statuses.iter().all(|s| s.is_ok())
    }

    /// Check if any critical (required) dependencies are missing.
    pub fn has_critical_missing(&self) -> bool {
        self.statuses.iter().any(|s| s.dep.required && !s.found)
    }

    /// Get all missing required dependencies.
    pub fn missing_required(&self) -> Vec<&DependencyStatus> {
        self.statuses
            .iter()
            .filter(|s| s.dep.required && !s.found)
            .collect()
    }

    /// Get all missing optional dependencies.
    pub fn missing_optional(&self) -> Vec<&DependencyStatus> {
        self.statuses
            .iter()
            .filter(|s| !s.dep.required && !s.found)
            .collect()
    }

    /// Get all found dependencies.
    pub fn found(&self) -> Vec<&DependencyStatus> {
        self.statuses.iter().filter(|s| s.found).collect()
    }

    /// Format error messages for display to the user.
    pub fn format_errors(&self) -> String {
        let mut output = String::new();

        let missing_required = self.missing_required();
        let missing_optional = self.missing_optional();

        if !missing_required.is_empty() {
            output.push_str("❌ Missing required dependencies:\n\n");
            for status in &missing_required {
                output.push_str(&format!(
                    "  • {} - {}\n    Install: {}\n\n",
                    status.dep.name, status.dep.purpose, status.dep.install_instructions
                ));
            }
            output.push_str("FORGE cannot start without these dependencies.\n");
        }

        if !missing_optional.is_empty() {
            if !missing_required.is_empty() {
                output.push('\n');
            }
            output.push_str("⚠️  Missing optional dependencies (limited functionality):\n\n");
            for status in &missing_optional {
                output.push_str(&format!(
                    "  • {} - {}\n    Install: {}\n\n",
                    status.dep.name, status.dep.purpose, status.dep.install_instructions
                ));
            }
        }

        output
    }

    /// Format a summary for startup display (compact version).
    pub fn format_summary(&self) -> String {
        let found_count = self.statuses.iter().filter(|s| s.found).count();
        let total_count = self.statuses.len();
        let required_count = self.statuses.iter().filter(|s| s.dep.required).count();
        let found_required = self
            .statuses
            .iter()
            .filter(|s| s.dep.required && s.found)
            .count();

        if self.is_ready() {
            if found_count == total_count {
                format!("✅ All {} dependencies found", total_count)
            } else {
                format!(
                    "✅ {} dependencies ready ({}/{} optional missing)",
                    found_required,
                    total_count - found_count,
                    total_count - required_count
                )
            }
        } else {
            format!(
                "❌ Missing {} required dependencies",
                required_count - found_required
            )
        }
    }
}

/// List of all dependencies to check.
const DEPENDENCIES: &[Dependency] = &[
    Dependency {
        name: "tmux",
        required: true,
        purpose: "Worker process management (spawn, monitor, kill)",
        install_instructions: "apt install tmux  |  brew install tmux  |  pacman -S tmux",
    },
    Dependency {
        name: "git",
        required: true,
        purpose: "Version control integration",
        install_instructions: "apt install git  |  brew install git  |  pacman -S git",
    },
    Dependency {
        name: "br",
        required: false,
        purpose: "Bead/task management (beads_rust CLI)",
        install_instructions: "cargo install beads_rust  |  See: https://github.com/jedarden/beads_rust",
    },
    Dependency {
        name: "jq",
        required: false,
        purpose: "JSON processing for log parsing",
        install_instructions: "apt install jq  |  brew install jq  |  pacman -S jq",
    },
];

/// Check if a binary exists and is executable.
fn check_binary(name: &str) -> Option<String> {
    // Use `which` on Unix-like systems
    let output = Command::new("which").arg(name).output().ok()?;

    if output.status.success() {
        // Binary found, try to get version
        let version = get_version(name);
        Some(version.unwrap_or_else(|| "installed".to_string()))
    } else {
        None
    }
}

/// Try to get version string for a binary.
fn get_version(name: &str) -> Option<String> {
    // Common version flags to try
    let version_args = ["--version", "-v", "-V", "version"];

    for arg in &version_args {
        if let Ok(output) = Command::new(name).arg(arg).output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                // Get first line of output (version usually on first line)
                let version_line = stdout
                    .lines()
                    .next()
                    .or_else(|| stderr.lines().next())
                    .unwrap_or("")
                    .trim();

                if !version_line.is_empty() {
                    return Some(version_line.to_string());
                }
            }
        }
    }

    None
}

/// Check all dependencies and return the result.
pub fn check_dependencies() -> DependencyCheck {
    let statuses: Vec<DependencyStatus> = DEPENDENCIES
        .iter()
        .map(|dep| {
            let version = check_binary(dep.name);
            DependencyStatus {
                dep: dep.clone(),
                found: version.is_some(),
                version,
            }
        })
        .collect();

    DependencyCheck { statuses }
}

/// Check dependencies and print results to stderr if there are issues.
///
/// Returns `true` if all required dependencies are met.
pub fn check_and_report() -> bool {
    let result = check_dependencies();

    if !result.is_ready() {
        eprintln!("{}", result.format_errors());
        return false;
    }

    // Log optional missing dependencies as warnings
    let missing_optional = result.missing_optional();
    if !missing_optional.is_empty() {
        eprintln!("⚠️  Some optional features are unavailable:");
        for status in missing_optional {
            eprintln!("    {} not found ({} will not work)", status.dep.name, status.dep.purpose);
        }
        eprintln!();
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dependency_check_finds_common_tools() {
        let result = check_dependencies();

        // At minimum, we should be able to check dependencies
        assert!(!result.statuses.is_empty());

        // Git should be available in most environments
        let git = result.statuses.iter().find(|s| s.dep.name == "git");
        assert!(git.is_some());
    }

    #[test]
    fn test_format_summary_with_all_found() {
        let result = DependencyCheck {
            statuses: vec![
                DependencyStatus {
                    dep: Dependency {
                        name: "test",
                        required: true,
                        purpose: "testing",
                        install_instructions: "apt install test",
                    },
                    found: true,
                    version: Some("1.0.0".to_string()),
                },
            ],
        };

        let summary = result.format_summary();
        assert!(summary.contains("✅"));
        assert!(summary.contains("1"));
    }

    #[test]
    fn test_format_errors_with_missing() {
        let result = DependencyCheck {
            statuses: vec![
                DependencyStatus {
                    dep: Dependency {
                        name: "missing-tool",
                        required: true,
                        purpose: "test purpose",
                        install_instructions: "apt install missing-tool",
                    },
                    found: false,
                    version: None,
                },
            ],
        };

        let errors = result.format_errors();
        assert!(errors.contains("❌"));
        assert!(errors.contains("missing-tool"));
        assert!(errors.contains("apt install missing-tool"));
    }

    #[test]
    fn test_is_ready_with_optional_missing() {
        let result = DependencyCheck {
            statuses: vec![
                DependencyStatus {
                    dep: Dependency {
                        name: "required-tool",
                        required: true,
                        purpose: "critical",
                        install_instructions: "apt install required-tool",
                    },
                    found: true,
                    version: Some("1.0".to_string()),
                },
                DependencyStatus {
                    dep: Dependency {
                        name: "optional-tool",
                        required: false,
                        purpose: "nice-to-have",
                        install_instructions: "apt install optional-tool",
                    },
                    found: false,
                    version: None,
                },
            ],
        };

        // Should be ready even with optional missing
        assert!(result.is_ready());
        assert!(!result.has_critical_missing());
    }

    #[test]
    fn test_not_ready_with_required_missing() {
        let result = DependencyCheck {
            statuses: vec![
                DependencyStatus {
                    dep: Dependency {
                        name: "required-tool",
                        required: true,
                        purpose: "critical",
                        install_instructions: "apt install required-tool",
                    },
                    found: false,
                    version: None,
                },
            ],
        };

        assert!(!result.is_ready());
        assert!(result.has_critical_missing());
    }
}
