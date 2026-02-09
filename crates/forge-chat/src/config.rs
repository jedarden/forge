//! Configuration for the chat backend.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Chat backend configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatConfig {
    /// Provider configuration
    #[serde(default)]
    pub provider: ProviderConfig,

    /// Rate limit configuration
    pub rate_limit: RateLimitConfig,

    /// Audit logging configuration
    pub audit: AuditConfig,

    /// Tool confirmations configuration
    pub confirmations: ConfirmationConfig,
}

/// Provider configuration for the chat backend.
///
/// This allows configuring different AI backends with their specific settings.
/// The provider type is determined by which config field is set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ProviderConfig {
    /// Claude API via direct HTTP requests
    ClaudeApi(ClaudeApiConfig),

    /// Claude CLI via stdin/stdout
    ClaudeCli(ClaudeCliConfig),

    /// Mock provider for testing
    Mock(MockConfig),
}

impl ProviderConfig {
    /// Get the provider type name.
    pub fn type_name(&self) -> &str {
        match self {
            ProviderConfig::ClaudeApi(_) => "claude-api",
            ProviderConfig::ClaudeCli(_) => "claude-cli",
            ProviderConfig::Mock(_) => "mock",
        }
    }

    /// Detect and create a default provider config based on available environment.
    ///
    /// Priority:
    /// 1. claude-cli if the binary exists in PATH
    /// 2. claude-api if ANTHROPIC_API_KEY is set
    /// 3. Error if neither is available
    pub fn detect_default() -> Result<Self, String> {
        // First check if claude-code or claude binary exists
        if let Some(cli_config) = Self::detect_claude_cli() {
            return Ok(ProviderConfig::ClaudeCli(cli_config));
        }

        // Then check for API key
        if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            return Ok(ProviderConfig::ClaudeApi(ClaudeApiConfig::default()));
        }

        Err(
            "No suitable provider found. Please either install claude-code/claude CLI \
             or set ANTHROPIC_API_KEY environment variable."
                .to_string(),
        )
    }

    /// Detect claude-cli configuration.
    fn detect_claude_cli() -> Option<ClaudeCliConfig> {
        // Check for claude-code first (preferred)
        if Self::command_exists("claude-code") {
            return Some(ClaudeCliConfig {
                binary_path: "claude-code".to_string(),
                ..Default::default()
            });
        }

        // Check for claude
        if Self::command_exists("claude") {
            return Some(ClaudeCliConfig {
                binary_path: "claude".to_string(),
                ..Default::default()
            });
        }

        None
    }

    /// Check if a command exists in PATH.
    fn command_exists(cmd: &str) -> bool {
        #[cfg(unix)]
        {
            use std::process::Command;
            Command::new("which")
                .arg(cmd)
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }

        #[cfg(windows)]
        {
            use std::process::Command;
            Command::new("where")
                .arg(cmd)
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        // Try to detect a default, fall back to claude-api
        Self::detect_default().unwrap_or_else(|_| {
            ProviderConfig::ClaudeApi(ClaudeApiConfig::default())
        })
    }
}

/// Configuration for Claude API provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeApiConfig {
    /// Environment variable containing the API key.
    /// Default: ANTHROPIC_API_KEY
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,

    /// API base URL.
    /// Default: https://api.anthropic.com
    #[serde(default = "default_api_base_url")]
    pub api_base_url: String,

    /// Model to use.
    /// Default: claude-sonnet-4-5-20250929
    #[serde(default = "default_model")]
    pub model: String,

    /// Maximum tokens for responses.
    /// Default: 1000
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    /// Temperature for responses (0.0 - 1.0).
    /// Default: 0.2
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Response timeout in seconds.
    /// Default: 30
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_api_key_env() -> String {
    "ANTHROPIC_API_KEY".to_string()
}

fn default_api_base_url() -> String {
    "https://api.anthropic.com".to_string()
}

fn default_model() -> String {
    "claude-sonnet-4-5-20250929".to_string()
}

fn default_max_tokens() -> u32 {
    1000
}

fn default_temperature() -> f32 {
    0.2
}

fn default_timeout_secs() -> u64 {
    30
}

impl Default for ClaudeApiConfig {
    fn default() -> Self {
        Self {
            api_key_env: default_api_key_env(),
            api_base_url: default_api_base_url(),
            model: default_model(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            timeout_secs: default_timeout_secs(),
        }
    }
}

/// Configuration for Claude CLI provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCliConfig {
    /// Path to the claude-code or claude binary.
    /// Default: claude-code
    #[serde(default = "default_binary_path")]
    pub binary_path: String,

    /// Model to use (sonnet, opus, haiku).
    /// Default: sonnet
    #[serde(default = "default_cli_model")]
    pub model: String,

    /// Claude config directory.
    /// Default: ~/.claude
    #[serde(default = "default_config_dir")]
    pub config_dir: Option<String>,

    /// Timeout in seconds.
    /// Default: 30
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,

    /// Whether to run in headless mode.
    /// Default: true
    #[serde(default = "default_headless")]
    pub headless: bool,

    /// Additional arguments to pass to claude-cli.
    /// Default: []
    #[serde(default)]
    pub extra_args: Vec<String>,
}

fn default_binary_path() -> String {
    "claude-code".to_string()
}

fn default_cli_model() -> String {
    "sonnet".to_string()
}

fn default_config_dir() -> Option<String> {
    None
}

fn default_headless() -> bool {
    true
}

impl Default for ClaudeCliConfig {
    fn default() -> Self {
        Self {
            binary_path: default_binary_path(),
            model: default_cli_model(),
            config_dir: default_config_dir(),
            timeout_secs: default_timeout_secs(),
            headless: default_headless(),
            extra_args: vec![],
        }
    }
}

/// Configuration for mock provider (testing only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockConfig {
    /// Model name to report.
    /// Default: mock-model
    #[serde(default = "default_mock_model")]
    pub model: String,

    /// Response text to return.
    /// Default: "This is a mock response."
    #[serde(default = "default_mock_response")]
    pub response: String,

    /// Response delay in milliseconds.
    /// Default: 0
    #[serde(default)]
    pub delay_ms: u64,
}

fn default_mock_model() -> String {
    "mock-model".to_string()
}

fn default_mock_response() -> String {
    "This is a mock response.".to_string()
}

impl Default for MockConfig {
    fn default() -> Self {
        Self {
            model: default_mock_model(),
            response: default_mock_response(),
            delay_ms: 0,
        }
    }
}

/// Provider type enum (legacy, for backward compatibility).
///
/// This enum is deprecated in favor of `ProviderConfig` but is kept
/// for serialization compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderType {
    /// Claude API via direct HTTP requests
    ClaudeApi,

    /// Claude CLI via stdin/stdout
    ClaudeCli,

    /// Mock provider for testing
    Mock,
}

impl Default for ProviderType {
    fn default() -> Self {
        Self::ClaudeApi
    }
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClaudeApi => write!(f, "claude-api"),
            Self::ClaudeCli => write!(f, "claude-cli"),
            Self::Mock => write!(f, "mock"),
        }
    }
}

impl std::str::FromStr for ProviderType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "claude-api" | "claude_api" | "api" => Ok(Self::ClaudeApi),
            "claude-cli" | "claude_cli" | "cli" => Ok(Self::ClaudeCli),
            "mock" => Ok(Self::Mock),
            _ => Err(format!("Unknown provider type: {}", s)),
        }
    }
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            provider: ProviderConfig::default(),
            rate_limit: RateLimitConfig::default(),
            audit: AuditConfig::default(),
            confirmations: ConfirmationConfig::default(),
        }
    }
}

/// Rate limiting configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum commands per minute
    pub max_per_minute: u32,

    /// Maximum commands per hour
    pub max_per_hour: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_per_minute: 10,
            max_per_hour: 100,
        }
    }
}

/// Audit logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    /// Whether audit logging is enabled
    pub enabled: bool,

    /// Path to the audit log file
    pub log_file: PathBuf,

    /// Log level (all, commands_only, errors_only)
    pub log_level: AuditLogLevel,
}

impl Default for AuditConfig {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            enabled: true,
            log_file: home.join(".forge").join("chat-audit.jsonl"),
            log_level: AuditLogLevel::All,
        }
    }
}

/// Audit log level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditLogLevel {
    /// Log all commands and responses
    All,
    /// Log only commands (not responses)
    CommandsOnly,
    /// Log only errors
    ErrorsOnly,
}

/// Confirmation settings for destructive operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationConfig {
    /// Actions that require confirmation
    pub required_for: Vec<String>,

    /// Cost threshold for requiring confirmation (USD)
    pub high_cost_threshold: f64,

    /// Count threshold for bulk operations
    pub bulk_operation_threshold: u32,
}

impl Default for ConfirmationConfig {
    fn default() -> Self {
        Self {
            required_for: vec![
                "kill_worker".to_string(),
                "kill_all_workers".to_string(),
                "pause_workers".to_string(),
            ],
            high_cost_threshold: 10.0,
            bulk_operation_threshold: 5,
        }
    }
}

impl ChatConfig {
    /// Create a new config with custom provider.
    pub fn with_provider(mut self, provider: ProviderConfig) -> Self {
        self.provider = provider;
        self
    }

    /// Create a new config with custom rate limit.
    pub fn with_rate_limit(mut self, max_per_minute: u32) -> Self {
        self.rate_limit.max_per_minute = max_per_minute;
        self
    }

    /// Create a new config with custom audit log path.
    pub fn with_audit_log(mut self, path: impl Into<PathBuf>) -> Self {
        self.audit.log_file = path.into();
        self
    }

    /// Disable audit logging.
    pub fn disable_audit(mut self) -> Self {
        self.audit.enabled = false;
        self
    }
}

// Use dirs crate for home directory
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| std::env::var("USERPROFILE").ok().map(PathBuf::from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_claude_api_config_default() {
        let config = ClaudeApiConfig::default();
        assert_eq!(config.api_key_env, "ANTHROPIC_API_KEY");
        assert_eq!(config.api_base_url, "https://api.anthropic.com");
        assert_eq!(config.model, "claude-sonnet-4-5-20250929");
        assert_eq!(config.max_tokens, 1000);
        assert_eq!(config.temperature, 0.2);
        assert_eq!(config.timeout_secs, 30);
    }

    #[test]
    fn test_claude_cli_config_default() {
        let config = ClaudeCliConfig::default();
        assert_eq!(config.binary_path, "claude-code");
        assert_eq!(config.model, "sonnet");
        assert_eq!(config.timeout_secs, 30);
        assert!(config.headless);
        assert!(config.extra_args.is_empty());
        assert!(config.config_dir.is_none());
    }

    #[test]
    fn test_mock_config_default() {
        let config = MockConfig::default();
        assert_eq!(config.model, "mock-model");
        assert_eq!(config.response, "This is a mock response.");
        assert_eq!(config.delay_ms, 0);
    }

    #[test]
    fn test_provider_config_type_name() {
        let api_config = ProviderConfig::ClaudeApi(ClaudeApiConfig::default());
        assert_eq!(api_config.type_name(), "claude-api");

        let cli_config = ProviderConfig::ClaudeCli(ClaudeCliConfig::default());
        assert_eq!(cli_config.type_name(), "claude-cli");

        let mock_config = ProviderConfig::Mock(MockConfig::default());
        assert_eq!(mock_config.type_name(), "mock");
    }

    #[test]
    fn test_provider_config_serialization_claude_api() {
        let config = ProviderConfig::ClaudeApi(ClaudeApiConfig {
            model: "claude-opus-4-5".to_string(),
            max_tokens: 2000,
            temperature: 0.5,
            ..Default::default()
        });

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("claude-api"));
        assert!(json.contains("claude-opus-4-5"));
        assert!(json.contains("\"max_tokens\":2000"));
        assert!(json.contains("\"temperature\":0.5"));

        // Deserialize back
        let deserialized: ProviderConfig = serde_json::from_str(&json).unwrap();
        match deserialized {
            ProviderConfig::ClaudeApi(api_config) => {
                assert_eq!(api_config.model, "claude-opus-4-5");
                assert_eq!(api_config.max_tokens, 2000);
                assert_eq!(api_config.temperature, 0.5);
            }
            _ => panic!("Expected ClaudeApi config"),
        }
    }

    #[test]
    fn test_provider_config_serialization_claude_cli() {
        let config = ProviderConfig::ClaudeCli(ClaudeCliConfig {
            binary_path: "/usr/local/bin/claude".to_string(),
            model: "opus".to_string(),
            headless: false,
            extra_args: vec!["--debug".to_string(), "--verbose".to_string()],
            ..Default::default()
        });

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("claude-cli"));
        assert!(json.contains("/usr/local/bin/claude"));
        assert!(json.contains("\"model\":\"opus\""));
        assert!(json.contains("\"headless\":false"));
        assert!(json.contains("--debug"));
        assert!(json.contains("--verbose"));

        // Deserialize back
        let deserialized: ProviderConfig = serde_json::from_str(&json).unwrap();
        match deserialized {
            ProviderConfig::ClaudeCli(cli_config) => {
                assert_eq!(cli_config.binary_path, "/usr/local/bin/claude");
                assert_eq!(cli_config.model, "opus");
                assert!(!cli_config.headless);
                assert_eq!(cli_config.extra_args.len(), 2);
            }
            _ => panic!("Expected ClaudeCli config"),
        }
    }

    #[test]
    fn test_provider_config_serialization_mock() {
        let config = ProviderConfig::Mock(MockConfig {
            model: "custom-mock".to_string(),
            response: "Custom response".to_string(),
            delay_ms: 100,
        });

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("mock"));
        assert!(json.contains("custom-mock"));
        assert!(json.contains("Custom response"));
        assert!(json.contains("\"delay_ms\":100"));

        // Deserialize back
        let deserialized: ProviderConfig = serde_json::from_str(&json).unwrap();
        match deserialized {
            ProviderConfig::Mock(mock_config) => {
                assert_eq!(mock_config.model, "custom-mock");
                assert_eq!(mock_config.response, "Custom response");
                assert_eq!(mock_config.delay_ms, 100);
            }
            _ => panic!("Expected Mock config"),
        }
    }

    #[test]
    fn test_chat_config_default() {
        let config = ChatConfig::default();
        // Verify provider is set to default (ClaudeApi or ClaudeCli depending on environment)
        match &config.provider {
            ProviderConfig::ClaudeApi(_) | ProviderConfig::ClaudeCli(_) => {
                // Expected
            }
            _ => panic!("Expected ClaudeApi or ClaudeCli provider"),
        }
    }

    #[test]
    fn test_chat_config_serialization() {
        let config = ChatConfig {
            provider: ProviderConfig::ClaudeApi(ClaudeApiConfig {
                model: "claude-opus-4-5".to_string(),
                ..Default::default()
            }),
            rate_limit: RateLimitConfig {
                max_per_minute: 20,
                max_per_hour: 200,
            },
            ..Default::default()
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("provider"));
        assert!(json.contains("claude-api"));
        assert!(json.contains("rate_limit"));
        assert!(json.contains("\"max_per_minute\":20"));

        // Deserialize back
        let deserialized: ChatConfig = serde_json::from_str(&json).unwrap();
        match deserialized.provider {
            ProviderConfig::ClaudeApi(api_config) => {
                assert_eq!(api_config.model, "claude-opus-4-5");
            }
            _ => panic!("Expected ClaudeApi provider"),
        }
        assert_eq!(deserialized.rate_limit.max_per_minute, 20);
    }

    #[test]
    fn test_chat_config_with_provider() {
        let base_config = ChatConfig::default();
        let new_provider = ProviderConfig::Mock(MockConfig {
            response: "Test response".to_string(),
            ..Default::default()
        });

        let config = base_config.with_provider(new_provider);
        match config.provider {
            ProviderConfig::Mock(mock_config) => {
                assert_eq!(mock_config.response, "Test response");
            }
            _ => panic!("Expected Mock provider"),
        }
    }

    #[test]
    fn test_chat_config_with_rate_limit() {
        let config = ChatConfig::default().with_rate_limit(50);
        assert_eq!(config.rate_limit.max_per_minute, 50);
    }

    #[test]
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.max_per_minute, 10);
        assert_eq!(config.max_per_hour, 100);
    }

    #[test]
    fn test_audit_config_default() {
        let config = AuditConfig::default();
        assert!(config.enabled);
        assert!(config.log_file.ends_with("chat-audit.jsonl"));
        assert_eq!(config.log_level, AuditLogLevel::All);
    }

    #[test]
    fn test_confirmation_config_default() {
        let config = ConfirmationConfig::default();
        assert!(config.required_for.contains(&"kill_worker".to_string()));
        assert_eq!(config.high_cost_threshold, 10.0);
        assert_eq!(config.bulk_operation_threshold, 5);
    }

    #[test]
    fn test_provider_type_legacy() {
        // Test that the legacy ProviderType enum still works
        let api_type = ProviderType::ClaudeApi;
        assert_eq!(api_type.to_string(), "claude-api");

        let cli_type = ProviderType::ClaudeCli;
        assert_eq!(cli_type.to_string(), "claude-cli");

        let mock_type = ProviderType::Mock;
        assert_eq!(mock_type.to_string(), "mock");
    }

    #[test]
    fn test_provider_type_from_str() {
        assert_eq!(
            ProviderType::from_str("claude-api").unwrap(),
            ProviderType::ClaudeApi
        );
        assert_eq!(
            ProviderType::from_str("claude-cli").unwrap(),
            ProviderType::ClaudeCli
        );
        assert_eq!(
            ProviderType::from_str("mock").unwrap(),
            ProviderType::Mock
        );
        assert!(ProviderType::from_str("invalid").is_err());
    }

    #[test]
    fn test_full_json_config_example() {
        let json = r#"
{
  "provider": {
    "type": "claude-cli",
    "binary_path": "/usr/local/bin/claude-code",
    "model": "sonnet",
    "config_dir": "/home/user/.claude",
    "timeout_secs": 60,
    "headless": true,
    "extra_args": ["--debug"]
  },
  "rate_limit": {
    "max_per_minute": 15,
    "max_per_hour": 150
  },
  "audit": {
    "enabled": true,
    "log_file": "/home/user/.forge/chat-audit.jsonl",
    "log_level": "all"
  },
  "confirmations": {
    "required_for": ["kill_worker", "deploy"],
    "high_cost_threshold": 5.0,
    "bulk_operation_threshold": 10
  }
}
"#;

        let config: ChatConfig = serde_json::from_str(json).unwrap();

        match config.provider {
            ProviderConfig::ClaudeCli(cli_config) => {
                assert_eq!(cli_config.binary_path, "/usr/local/bin/claude-code");
                assert_eq!(cli_config.model, "sonnet");
                assert_eq!(cli_config.config_dir.as_ref().unwrap(), "/home/user/.claude");
                assert_eq!(cli_config.timeout_secs, 60);
                assert!(cli_config.headless);
                assert_eq!(cli_config.extra_args.len(), 1);
                assert_eq!(cli_config.extra_args[0], "--debug");
            }
            _ => panic!("Expected ClaudeCli provider"),
        }

        assert_eq!(config.rate_limit.max_per_minute, 15);
        assert_eq!(config.rate_limit.max_per_hour, 150);
        assert!(config.audit.enabled);
        assert_eq!(config.confirmations.high_cost_threshold, 5.0);
    }
}
