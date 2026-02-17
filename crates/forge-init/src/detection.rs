//! CLI tool detection for FORGE onboarding.
//!
//! Detects available AI CLI tools (claude-code, opencode, aider) and determines
//! their capabilities for use as FORGE chat backends and worker launchers.

use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::guidance::{PathDiagnostics, RejectionReason};

/// Errors that can occur during CLI tool detection.
#[derive(Debug, Error)]
pub enum DetectionError {
    #[error("Failed to execute command: {0}")]
    CommandFailed(String),

    #[error("Failed to parse version: {0}")]
    VersionParseFailed(String),

    #[error("PATH search failed: {0}")]
    PathSearchFailed(String),
}

/// Result type for detection operations.
pub type Result<T> = std::result::Result<T, DetectionError>;

/// Information about a detected CLI tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliToolDetection {
    /// Tool name (e.g., "claude-code", "opencode", "aider").
    pub name: String,

    /// Full path to the executable binary.
    pub binary_path: PathBuf,

    /// Tool version string (e.g., "1.2.3").
    pub version: Option<String>,

    /// Whether the tool supports headless mode (--output-format stream-json).
    pub headless_support: bool,

    /// Whether the tool supports permission skipping (--dangerously-skip-permissions).
    pub skip_permissions: bool,

    /// Whether this tool requires an API key in environment.
    pub api_key_required: bool,

    /// The environment variable name for the API key (e.g., "ANTHROPIC_API_KEY").
    pub api_key_env_var: Option<String>,

    /// Whether the required API key was found in environment.
    pub api_key_detected: bool,

    /// Overall tool status (ready to use, missing key, etc.).
    pub status: ToolStatus,
}

/// Status of a detected CLI tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    /// Tool is fully configured and ready to use.
    Ready,

    /// Tool is installed but missing required API key.
    MissingApiKey,

    /// Tool is installed but doesn't support required features.
    IncompatibleVersion,

    /// Tool binary found but cannot execute.
    NotExecutable,
}

impl CliToolDetection {
    /// Create a new detection result.
    pub fn new(name: impl Into<String>, binary_path: PathBuf) -> Self {
        Self {
            name: name.into(),
            binary_path,
            version: None,
            headless_support: false,
            skip_permissions: false,
            api_key_required: false,
            api_key_env_var: None,
            api_key_detected: false,
            status: ToolStatus::NotExecutable,
        }
    }

    /// Set the version.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Set headless support.
    pub fn with_headless_support(mut self, supported: bool) -> Self {
        self.headless_support = supported;
        self
    }

    /// Set permission skipping support.
    pub fn with_skip_permissions(mut self, supported: bool) -> Self {
        self.skip_permissions = supported;
        self
    }

    /// Set API key requirements.
    pub fn with_api_key(mut self, required: bool, env_var: Option<String>, detected: bool) -> Self {
        self.api_key_required = required;
        self.api_key_env_var = env_var;
        self.api_key_detected = detected;
        self
    }

    /// Set the tool status.
    pub fn with_status(mut self, status: ToolStatus) -> Self {
        self.status = status;
        self
    }

    /// Check if the tool is ready to use.
    pub fn is_ready(&self) -> bool {
        self.status == ToolStatus::Ready
    }

    /// Get a human-readable status message.
    pub fn status_message(&self) -> &'static str {
        match self.status {
            ToolStatus::Ready => "Ready",
            ToolStatus::MissingApiKey => "Missing API key",
            ToolStatus::IncompatibleVersion => "Incompatible version",
            ToolStatus::NotExecutable => "Not executable",
        }
    }
}

/// Detect all available CLI tools on the system.
///
/// Searches PATH for known CLI tool binaries and probes each one to determine
/// its capabilities and configuration status.
///
/// # Returns
///
/// A vector of detected tools, sorted by readiness (ready tools first).
///
/// # Examples
///
/// ```no_run
/// use forge_init::detection::detect_cli_tools;
///
/// let tools = detect_cli_tools().unwrap();
/// for tool in tools {
///     println!("{}: {}", tool.name, tool.status_message());
/// }
/// ```
pub fn detect_cli_tools() -> Result<Vec<CliToolDetection>> {
    let (tools, _diagnostics) = detect_cli_tools_with_diagnostics()?;
    Ok(tools)
}

/// Detect all available CLI tools with detailed diagnostics.
///
/// Returns both the detected tools and diagnostic information about the search,
/// useful for providing helpful error messages when no tools are found.
///
/// # Returns
///
/// A tuple containing:
/// - A vector of detected tools, sorted by readiness (ready tools first)
/// - Diagnostic information about the PATH search and any rejected candidates
pub fn detect_cli_tools_with_diagnostics() -> Result<(Vec<CliToolDetection>, PathDiagnostics)> {
    info!("Starting CLI tool detection with diagnostics");

    let mut tools = Vec::new();
    let mut diagnostics = PathDiagnostics::new();

    // Detect Claude Code
    match detect_claude_code_with_diagnostics(&mut diagnostics)? {
        Some(tool) => tools.push(tool),
        None => {}
    }

    // Detect OpenCode
    match detect_opencode_with_diagnostics(&mut diagnostics)? {
        Some(tool) => tools.push(tool),
        None => {}
    }

    // Detect Aider
    match detect_aider_with_diagnostics(&mut diagnostics)? {
        Some(tool) => tools.push(tool),
        None => {}
    }

    // Sort by status (ready tools first)
    tools.sort_by_key(|t| match t.status {
        ToolStatus::Ready => 0,
        ToolStatus::MissingApiKey => 1,
        ToolStatus::IncompatibleVersion => 2,
        ToolStatus::NotExecutable => 3,
    });

    info!("Detected {} CLI tools", tools.len());
    Ok((tools, diagnostics))
}


/// Detect Claude Code CLI tool with diagnostic collection.
fn detect_claude_code_with_diagnostics(
    diagnostics: &mut PathDiagnostics,
) -> Result<Option<CliToolDetection>> {
    debug!("Detecting Claude Code with diagnostics");

    let binary_path = match which::which("claude") {
        Ok(path) => path,
        Err(_) => {
            debug!("Claude Code not found in PATH");
            diagnostics.add_rejection("claude", RejectionReason::NotFound);
            return Ok(None);
        }
    };

    let mut tool = CliToolDetection::new("claude-code", binary_path.clone());

    // Get version
    let mut version: Option<String> = None;
    if let Ok(output) = Command::new(&binary_path).arg("--version").output()
        && let Ok(version_str) = String::from_utf8(output.stdout)
    {
        version = Some(version_str.trim().to_string());
        tool = tool.with_version(version_str.trim());
    }

    // Check for headless support
    let mut has_headless = false;
    let mut has_skip_perms = false;
    if let Ok(output) = Command::new(&binary_path).arg("--help").output() {
        let help_text = String::from_utf8_lossy(&output.stdout);
        has_headless = help_text.contains("--output-format");
        has_skip_perms = help_text.contains("--dangerously-skip-permissions");

        tool = tool
            .with_headless_support(has_headless)
            .with_skip_permissions(has_skip_perms);
    }

    // Claude CLI handles its own authentication - no API key check needed
    tool = tool.with_api_key(
        false, // API key NOT required (CLI handles auth)
        None,  // No environment variable needed
        true,  // Always "detected" since CLI handles it
    );

    // Determine overall status and record rejection if not ready
    let status = if !has_headless || !has_skip_perms {
        let missing_feature = if !has_headless {
            "headless mode (--output-format)"
        } else {
            "permission skipping (--dangerously-skip-permissions)"
        };
        diagnostics.add_rejection(
            "claude",
            RejectionReason::IncompatibleVersion {
                path: binary_path.clone(),
                version: version.clone(),
                missing_feature: missing_feature.to_string(),
            },
        );
        ToolStatus::IncompatibleVersion
    } else {
        ToolStatus::Ready
    };

    tool = tool.with_status(status);

    info!(
        "Claude Code detected: {} - {}",
        binary_path.display(),
        tool.status_message()
    );
    Ok(Some(tool))
}

/// Detect OpenCode CLI tool with diagnostic collection.
fn detect_opencode_with_diagnostics(
    diagnostics: &mut PathDiagnostics,
) -> Result<Option<CliToolDetection>> {
    debug!("Detecting OpenCode with diagnostics");

    let binary_path = match which::which("opencode") {
        Ok(path) => path,
        Err(_) => {
            debug!("OpenCode not found in PATH");
            diagnostics.add_rejection("opencode", RejectionReason::NotFound);
            return Ok(None);
        }
    };

    let mut tool = CliToolDetection::new("opencode", binary_path.clone());

    // Get version
    let mut version: Option<String> = None;
    if let Ok(output) = Command::new(&binary_path).arg("--version").output()
        && let Ok(version_str) = String::from_utf8(output.stdout)
    {
        version = Some(version_str.trim().to_string());
        tool = tool.with_version(version_str.trim());
    }

    // Check for headless support - OpenCode uses "serve" command
    let mut has_headless = false;
    if let Ok(output) = Command::new(&binary_path).arg("--help").output() {
        let help_text = String::from_utf8_lossy(&output.stdout);
        has_headless = help_text.contains("serve") || help_text.contains("headless");

        tool = tool
            .with_headless_support(has_headless)
            .with_skip_permissions(true); // Not applicable for OpenCode
    }

    // OpenCode CLI handles its own authentication - no API key check needed
    tool = tool.with_api_key(
        false, // API key NOT required (CLI handles auth)
        None,  // No environment variable needed
        true,  // Always "detected" since CLI handles it
    );

    // Determine status and record rejection if not ready
    let status = if !has_headless {
        diagnostics.add_rejection(
            "opencode",
            RejectionReason::IncompatibleVersion {
                path: binary_path.clone(),
                version: version.clone(),
                missing_feature: "headless/serve mode".to_string(),
            },
        );
        ToolStatus::IncompatibleVersion
    } else {
        ToolStatus::Ready
    };

    tool = tool.with_status(status);

    info!(
        "OpenCode detected: {} - {}",
        binary_path.display(),
        tool.status_message()
    );
    Ok(Some(tool))
}

/// Detect Aider CLI tool with diagnostic collection.
fn detect_aider_with_diagnostics(
    diagnostics: &mut PathDiagnostics,
) -> Result<Option<CliToolDetection>> {
    debug!("Detecting Aider with diagnostics");

    let binary_path = match which::which("aider") {
        Ok(path) => path,
        Err(_) => {
            debug!("Aider not found in PATH");
            diagnostics.add_rejection("aider", RejectionReason::NotFound);
            return Ok(None);
        }
    };

    let mut tool = CliToolDetection::new("aider", binary_path.clone());

    // Get version
    let mut version: Option<String> = None;
    if let Ok(output) = Command::new(&binary_path).arg("--version").output()
        && let Ok(version_str) = String::from_utf8(output.stdout)
    {
        version = Some(version_str.trim().to_string());
        tool = tool.with_version(version_str.trim());
    }

    // Aider doesn't have the same headless mode as Claude Code
    tool = tool
        .with_headless_support(false)
        .with_skip_permissions(false);

    // Aider can use multiple API keys (OpenAI, Anthropic)
    let openai_key = std::env::var("OPENAI_API_KEY").is_ok();
    let anthropic_key = std::env::var("ANTHROPIC_API_KEY").is_ok();
    let has_api_key = openai_key || anthropic_key;

    tool = tool.with_api_key(
        true,
        Some("OPENAI_API_KEY or ANTHROPIC_API_KEY".to_string()),
        has_api_key,
    );

    // Aider requires different integration approach - record rejection
    let status = if !has_api_key {
        diagnostics.add_rejection(
            "aider",
            RejectionReason::MissingApiKey {
                path: binary_path.clone(),
                env_var: "OPENAI_API_KEY or ANTHROPIC_API_KEY".to_string(),
            },
        );
        ToolStatus::MissingApiKey
    } else {
        // Even with API key, Aider isn't fully compatible yet
        diagnostics.add_rejection(
            "aider",
            RejectionReason::IncompatibleVersion {
                path: binary_path.clone(),
                version: version.clone(),
                missing_feature: "FORGE-compatible headless mode (requires custom integration)"
                    .to_string(),
            },
        );
        ToolStatus::IncompatibleVersion
    };

    tool = tool.with_status(status);

    warn!(
        "Aider detected but not fully compatible: {} - {}",
        binary_path.display(),
        tool.status_message()
    );
    Ok(Some(tool))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_tool_detection_new() {
        let tool = CliToolDetection::new("test-tool", PathBuf::from("/usr/bin/test"));

        assert_eq!(tool.name, "test-tool");
        assert_eq!(tool.binary_path, PathBuf::from("/usr/bin/test"));
        assert!(tool.version.is_none());
        assert!(!tool.headless_support);
        assert_eq!(tool.status, ToolStatus::NotExecutable);
    }

    #[test]
    fn test_cli_tool_detection_builder() {
        let tool = CliToolDetection::new("claude-code", PathBuf::from("/usr/bin/claude"))
            .with_version("1.2.3")
            .with_headless_support(true)
            .with_skip_permissions(true)
            .with_api_key(true, Some("ANTHROPIC_API_KEY".to_string()), true)
            .with_status(ToolStatus::Ready);

        assert_eq!(tool.version, Some("1.2.3".to_string()));
        assert!(tool.headless_support);
        assert!(tool.skip_permissions);
        assert!(tool.api_key_required);
        assert!(tool.api_key_detected);
        assert_eq!(tool.status, ToolStatus::Ready);
        assert!(tool.is_ready());
    }

    #[test]
    fn test_tool_status_message() {
        let ready = CliToolDetection::new("test", PathBuf::from("/bin/test"))
            .with_status(ToolStatus::Ready);
        assert_eq!(ready.status_message(), "Ready");

        let missing_key = CliToolDetection::new("test", PathBuf::from("/bin/test"))
            .with_status(ToolStatus::MissingApiKey);
        assert_eq!(missing_key.status_message(), "Missing API key");
    }
}
