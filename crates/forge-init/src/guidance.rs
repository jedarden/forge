//! Installation guidance for CLI tools.
//!
//! Provides detailed installation instructions, platform-specific hints,
//! and diagnostic information when no CLI tools are detected.

use std::env;
use std::fmt::Write as FmtWrite;
use std::path::PathBuf;

/// Platform type for installation hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOS,
    Linux,
    Windows,
    Unknown,
}

impl Platform {
    /// Detect the current platform.
    pub fn detect() -> Self {
        if cfg!(target_os = "macos") {
            Platform::MacOS
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::Unknown
        }
    }

    /// Get the platform display name.
    pub fn name(&self) -> &'static str {
        match self {
            Platform::MacOS => "macOS",
            Platform::Linux => "Linux",
            Platform::Windows => "Windows",
            Platform::Unknown => "Unknown",
        }
    }
}

/// Information about why a tool candidate was rejected.
#[derive(Debug, Clone)]
pub struct RejectionInfo {
    /// Tool name.
    pub tool_name: String,
    /// Reason for rejection.
    pub reason: RejectionReason,
}

/// Reasons why a tool was not available.
#[derive(Debug, Clone)]
pub enum RejectionReason {
    /// Tool binary not found in PATH.
    NotFound,
    /// Binary found but not executable.
    NotExecutable(PathBuf),
    /// Version incompatible (missing required features).
    IncompatibleVersion {
        path: PathBuf,
        version: Option<String>,
        missing_feature: String,
    },
    /// Missing required API key.
    MissingApiKey {
        path: PathBuf,
        env_var: String,
    },
}

impl RejectionReason {
    /// Get a human-readable description of the rejection.
    pub fn description(&self) -> String {
        match self {
            RejectionReason::NotFound => "Not found in PATH".to_string(),
            RejectionReason::NotExecutable(path) => {
                format!("Found at {} but not executable", path.display())
            }
            RejectionReason::IncompatibleVersion {
                path,
                version,
                missing_feature,
            } => {
                let ver = version.as_deref().unwrap_or("unknown");
                format!(
                    "Found at {} (v{}), but missing feature: {}",
                    path.display(),
                    ver,
                    missing_feature
                )
            }
            RejectionReason::MissingApiKey { path, env_var } => {
                format!(
                    "Found at {}, but missing API key: {}",
                    path.display(),
                    env_var
                )
            }
        }
    }
}

/// Diagnostic information about the PATH search.
#[derive(Debug, Clone)]
pub struct PathDiagnostics {
    /// Directories searched in PATH.
    pub searched_directories: Vec<PathBuf>,
    /// Tools that were found but rejected.
    pub rejections: Vec<RejectionInfo>,
}

impl PathDiagnostics {
    /// Get the PATH directories as a vector.
    pub fn get_path_directories() -> Vec<PathBuf> {
        env::var_os("PATH")
            .map(|path| env::split_paths(&path).collect())
            .unwrap_or_default()
    }

    /// Create new diagnostics with current PATH.
    pub fn new() -> Self {
        Self {
            searched_directories: Self::get_path_directories(),
            rejections: Vec::new(),
        }
    }

    /// Add a rejection.
    pub fn add_rejection(&mut self, tool_name: impl Into<String>, reason: RejectionReason) {
        self.rejections.push(RejectionInfo {
            tool_name: tool_name.into(),
            reason,
        });
    }
}

impl Default for PathDiagnostics {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate the complete installation guidance message.
///
/// This includes:
/// - Detailed installation commands for each supported tool
/// - Platform-specific hints for prerequisites
/// - Diagnostic information about what was searched
/// - Next steps after installation
pub fn generate_guidance(diagnostics: Option<&PathDiagnostics>) -> String {
    let platform = Platform::detect();
    let mut output = String::new();

    // Header
    writeln!(output, "\n❌ No compatible CLI tools found!").unwrap();
    writeln!(output).unwrap();
    writeln!(output, "FORGE requires one of these AI CLI tools:").unwrap();
    writeln!(output).unwrap();

    // Claude Code section
    writeln!(output, "📦 Claude Code (Recommended)").unwrap();
    writeln!(output, "   Install: npm install -g @anthropic-ai/claude-code").unwrap();
    writeln!(output, "   Docs: https://docs.anthropic.com/en/docs/claude-code").unwrap();
    writeln!(
        output,
        "   Features: Full headless support, subscription API, multi-model"
    )
    .unwrap();
    writeln!(output).unwrap();

    // OpenCode section
    writeln!(output, "📦 OpenCode").unwrap();
    writeln!(output, "   Install: pip install opencode-ai").unwrap();
    writeln!(output, "   Docs: https://github.com/opencode-ai/opencode").unwrap();
    writeln!(output, "   Features: Multi-provider support, open source").unwrap();
    writeln!(output).unwrap();

    // Platform-specific hints
    writeln!(output, "💻 Platform Hints ({})", platform.name()).unwrap();
    match platform {
        Platform::MacOS => {
            writeln!(output, "   If npm not found:").unwrap();
            writeln!(output, "     brew install node").unwrap();
            writeln!(output, "   If pip not found:").unwrap();
            writeln!(output, "     brew install python").unwrap();
        }
        Platform::Linux => {
            writeln!(output, "   If npm not found (Debian/Ubuntu):").unwrap();
            writeln!(output, "     sudo apt install nodejs npm").unwrap();
            writeln!(output, "   If npm not found (Fedora/RHEL):").unwrap();
            writeln!(output, "     sudo dnf install nodejs npm").unwrap();
            writeln!(output, "   If pip not found:").unwrap();
            writeln!(output, "     sudo apt install python3-pip  # or: dnf install python3-pip").unwrap();
        }
        Platform::Windows => {
            writeln!(output, "   If npm not found:").unwrap();
            writeln!(
                output,
                "     Download Node.js from: https://nodejs.org/en/download/"
            )
            .unwrap();
            writeln!(output, "     Or via winget: winget install OpenJS.NodeJS.LTS").unwrap();
            writeln!(output, "   If pip not found:").unwrap();
            writeln!(
                output,
                "     Download Python from: https://www.python.org/downloads/"
            )
            .unwrap();
        }
        Platform::Unknown => {
            writeln!(
                output,
                "   Ensure Node.js (npm) or Python (pip) is installed"
            )
            .unwrap();
        }
    }
    writeln!(output).unwrap();

    // Diagnostic output if available
    if let Some(diag) = diagnostics {
        writeln!(output, "🔍 Diagnostic Information").unwrap();

        // Show PATH directories searched
        writeln!(
            output,
            "   Searched {} directories in PATH:",
            diag.searched_directories.len()
        )
        .unwrap();
        for (i, dir) in diag.searched_directories.iter().take(10).enumerate() {
            writeln!(output, "     {}. {}", i + 1, dir.display()).unwrap();
        }
        if diag.searched_directories.len() > 10 {
            writeln!(
                output,
                "     ... and {} more",
                diag.searched_directories.len() - 10
            )
            .unwrap();
        }
        writeln!(output).unwrap();

        // Show rejections if any
        if !diag.rejections.is_empty() {
            writeln!(output, "   Candidate tools found but not usable:").unwrap();
            for rejection in &diag.rejections {
                writeln!(
                    output,
                    "     • {}: {}",
                    rejection.tool_name,
                    rejection.reason.description()
                )
                .unwrap();
            }
            writeln!(output).unwrap();
        }
    }

    // Next steps
    writeln!(output, "💡 After installation, run: forge init").unwrap();
    writeln!(output).unwrap();

    output
}

/// Generate a compact version of guidance suitable for error messages.
pub fn generate_compact_guidance() -> String {
    let mut output = String::new();

    writeln!(output, "\n❌ No compatible CLI tools found!").unwrap();
    writeln!(output).unwrap();
    writeln!(output, "Quick install options:").unwrap();
    writeln!(output, "  Claude Code: npm install -g @anthropic-ai/claude-code").unwrap();
    writeln!(output, "  OpenCode:    pip install opencode-ai").unwrap();
    writeln!(output).unwrap();
    writeln!(
        output,
        "Run 'forge init --help' for detailed installation guidance."
    )
    .unwrap();

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detect() {
        let platform = Platform::detect();
        // Just verify it returns a valid platform
        assert!(!platform.name().is_empty());
    }

    #[test]
    fn test_rejection_reason_description() {
        let not_found = RejectionReason::NotFound;
        assert_eq!(not_found.description(), "Not found in PATH");

        let not_exec = RejectionReason::NotExecutable(PathBuf::from("/usr/bin/claude"));
        assert!(not_exec.description().contains("/usr/bin/claude"));

        let incompatible = RejectionReason::IncompatibleVersion {
            path: PathBuf::from("/usr/bin/claude"),
            version: Some("1.0.0".to_string()),
            missing_feature: "headless mode".to_string(),
        };
        assert!(incompatible.description().contains("headless mode"));

        let missing_key = RejectionReason::MissingApiKey {
            path: PathBuf::from("/usr/bin/aider"),
            env_var: "ANTHROPIC_API_KEY".to_string(),
        };
        assert!(missing_key.description().contains("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn test_path_diagnostics_new() {
        let diag = PathDiagnostics::new();
        // PATH should have at least some directories
        // Note: This might be empty in some CI environments
        assert!(diag.rejections.is_empty());
    }

    #[test]
    fn test_generate_guidance_without_diagnostics() {
        let guidance = generate_guidance(None);

        // Check for required sections
        assert!(guidance.contains("No compatible CLI tools found"));
        assert!(guidance.contains("Claude Code (Recommended)"));
        assert!(guidance.contains("npm install -g @anthropic-ai/claude-code"));
        assert!(guidance.contains("OpenCode"));
        assert!(guidance.contains("pip install opencode-ai"));
        assert!(guidance.contains("Platform Hints"));
        assert!(guidance.contains("After installation, run: forge init"));
    }

    #[test]
    fn test_generate_guidance_with_diagnostics() {
        let mut diag = PathDiagnostics::new();
        diag.add_rejection(
            "claude",
            RejectionReason::IncompatibleVersion {
                path: PathBuf::from("/usr/local/bin/claude"),
                version: Some("0.9.0".to_string()),
                missing_feature: "headless mode".to_string(),
            },
        );

        let guidance = generate_guidance(Some(&diag));

        assert!(guidance.contains("Diagnostic Information"));
        assert!(guidance.contains("claude"));
        assert!(guidance.contains("headless mode"));
    }

    #[test]
    fn test_generate_compact_guidance() {
        let compact = generate_compact_guidance();

        assert!(compact.contains("No compatible CLI tools found"));
        assert!(compact.contains("npm install -g @anthropic-ai/claude-code"));
        assert!(compact.contains("pip install opencode-ai"));
        assert!(compact.contains("forge init --help"));
    }
}
