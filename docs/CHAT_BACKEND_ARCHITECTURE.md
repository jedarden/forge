# Chat Backend Architecture

This document describes the architecture and configuration of the `forge-chat` crate, which provides the AI conversational interface for the FORGE control panel.

## Overview

The chat backend enables natural language interaction with the FORGE dashboard, allowing users to:
- Query worker status, tasks, and costs
- Execute actions (spawn workers, kill workers, reassign tasks)
- Get AI-powered analysis and recommendations

## Architecture Diagram

```
                                    ┌─────────────────────┐
                                    │   ChatBackend       │
                                    │   (Entry Point)     │
                                    └─────────┬───────────┘
                                              │
                    ┌─────────────────────────┼─────────────────────────┐
                    │                         │                         │
            ┌───────▼───────┐         ┌───────▼───────┐         ┌───────▼───────┐
            │ RateLimiter   │         │ ContextProvider│        │ ToolRegistry   │
            │ (10 cmd/min)  │         │ (Dashboard     │        │ (Read/Action   │
            │               │         │  State)        │        │  Tools)        │
            └───────────────┘         └───────────────┘         └───────────────┘
                                              │
                                    ┌─────────▼───────────┐
                                    │   ChatProvider      │
                                    │   (Pluggable)       │
                                    └─────────┬───────────┘
                    ┌─────────────────────────┼─────────────────────────┐
                    │                         │                         │
            ┌───────▼───────┐         ┌───────▼───────┐         ┌───────▼───────┐
            │ ClaudeApiProv │         │ ClaudeCliProv │         │ MockProvider   │
            │ (HTTP API)    │         │ (CLI Process) │         │ (Testing)      │
            └───────────────┘         └───────────────┘         └───────────────┘
```

## Module Structure

```
crates/forge-chat/src/
├── lib.rs               # Public API re-exports
├── backend.rs           # ChatBackend main entry point
├── config.rs            # Configuration structures
├── provider.rs          # ChatProvider trait + implementations
├── claude_api.rs        # Claude API provider (HTTP)
├── claude_api_types.rs  # API request/response types
├── context.rs           # Dashboard context injection
├── tools.rs             # Tool definitions and registry
├── audit.rs             # JSONL audit logging
├── rate_limit.rs        # Rate limiting (sliding window)
└── error.rs             # Error types
```

## Configuration

### ChatConfig (Root Configuration)

```rust
pub struct ChatConfig {
    pub provider: ProviderConfig,      // Which AI provider to use
    pub rate_limit: RateLimitConfig,   // Rate limiting settings
    pub audit: AuditConfig,            // Audit logging settings
    pub confirmations: ConfirmationConfig, // Action confirmation rules
}
```

### ProviderConfig (Provider Selection)

The provider is configured using a tagged enum with three variants:

```rust
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ProviderConfig {
    ClaudeApi(ClaudeApiConfig),  // Direct HTTP API
    ClaudeCli(ClaudeCliConfig),  // CLI subprocess
    Mock(MockConfig),            // Testing mock
}
```

#### Provider Detection Priority

When no explicit configuration is provided:
1. **claude-cli**: If `claude-code` or `claude` binary exists in PATH
2. **claude-api**: If `ANTHROPIC_API_KEY` environment variable is set
3. **Error**: If neither is available

### ClaudeApiConfig (HTTP API Provider)

```rust
pub struct ClaudeApiConfig {
    pub api_key_env: String,       // Default: "ANTHROPIC_API_KEY"
    pub api_base_url: String,      // Default: "https://api.anthropic.com"
    pub model: String,             // Default: "claude-sonnet-4-5-20250929"
    pub max_tokens: u32,           // Default: 1000
    pub temperature: f32,          // Default: 0.2
    pub timeout_secs: u64,         // Default: 30
}
```

**Example JSON Configuration:**
```json
{
  "provider": {
    "type": "claude-api",
    "api_key_env": "ANTHROPIC_API_KEY",
    "api_base_url": "https://api.anthropic.com",
    "model": "claude-opus-4-5",
    "max_tokens": 2000,
    "temperature": 0.3,
    "timeout_secs": 60
  }
}
```

### ClaudeCliConfig (CLI Provider)

```rust
pub struct ClaudeCliConfig {
    pub binary_path: String,       // Default: "claude-code"
    pub model: String,             // Default: "sonnet"
    pub config_dir: Option<String>, // Default: None (~/.claude)
    pub timeout_secs: u64,         // Default: 30
    pub headless: bool,            // Default: true
    pub extra_args: Vec<String>,   // Default: []
}
```

**Example JSON Configuration:**
```json
{
  "provider": {
    "type": "claude-cli",
    "binary_path": "/usr/local/bin/claude-code",
    "model": "sonnet",
    "config_dir": "/home/user/.claude",
    "timeout_secs": 60,
    "headless": true,
    "extra_args": ["--debug"]
  }
}
```

### Headless Mode

The `headless` flag (default: `true`) configures the CLI provider to run without interactive terminal features:

**When `headless: true` (default):**
- Adds `--headless` flag to claude-cli invocation
- CLI operates in batch mode without TUI elements
- Suitable for background processing and automation
- No user prompts or interactive confirmations

**When `headless: false`:**
- CLI may show interactive prompts
- Requires terminal access
- Suitable for development/debugging

**CLI Invocation Example:**
```bash
# headless: true
claude-code --model sonnet --headless [extra_args]

# headless: false
claude-code --model sonnet [extra_args]
```

### MockConfig (Testing Provider)

```rust
pub struct MockConfig {
    pub model: String,             // Default: "mock-model"
    pub response: String,          // Default: "This is a mock response."
    pub delay_ms: u64,             // Default: 0
}
```

### RateLimitConfig

```rust
pub struct RateLimitConfig {
    pub max_per_minute: u32,       // Default: 10
    pub max_per_hour: u32,         // Default: 100
}
```

### AuditConfig

```rust
pub struct AuditConfig {
    pub enabled: bool,             // Default: true
    pub log_file: PathBuf,         // Default: ~/.forge/chat-audit.jsonl
    pub log_level: AuditLogLevel,  // Default: All
}

pub enum AuditLogLevel {
    All,           // Log everything
    CommandsOnly,  // Log commands without responses
    ErrorsOnly,    // Log only errors
}
```

### ConfirmationConfig

```rust
pub struct ConfirmationConfig {
    pub required_for: Vec<String>,     // Actions requiring confirmation
    pub high_cost_threshold: f64,      // USD threshold (default: 10.0)
    pub bulk_operation_threshold: u32, // Count threshold (default: 5)
}
```

**Default actions requiring confirmation:**
- `kill_worker`
- `kill_all_workers`
- `pause_workers`

## Provider Interface

### ChatProvider Trait

All providers implement the `ChatProvider` trait:

```rust
#[async_trait]
pub trait ChatProvider: Send + Sync {
    /// Process a prompt with context and tools
    async fn process(
        &self,
        prompt: &str,
        context: &DashboardContext,
        tools: &[ProviderTool],
    ) -> Result<ProviderResponse>;

    /// Get provider name for logging
    fn name(&self) -> &str;

    /// Whether streaming is supported
    fn supports_streaming(&self) -> bool { false }

    /// Estimated cost per request
    fn estimated_cost(&self) -> Option<f64> { None }

    /// Model name being used
    fn model(&self) -> &str;
}
```

### ProviderResponse

```rust
pub struct ProviderResponse {
    pub text: String,              // AI response text
    pub tool_calls: Vec<ToolCall>, // Tool invocations
    pub duration_ms: u64,          // Request duration
    pub cost_usd: Option<f64>,     // Estimated cost
    pub finish_reason: FinishReason,
    pub usage: Option<TokenUsage>, // Token consumption
}

pub enum FinishReason {
    Stop,           // Normal completion
    ToolCall,       // Stopped to execute tools
    MaxTokens,      // Hit token limit
    Error(String),  // Error occurred
}

pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
}
```

### Provider Implementations

#### ClaudeApiProvider (HTTP)
- Makes direct HTTP calls to `https://api.anthropic.com/v1/messages`
- Uses `reqwest` for HTTP client
- Supports prompt caching tokens
- Includes cost estimation based on model tier

#### ClaudeCliProvider (CLI)
- Spawns claude-code/claude binary as subprocess
- Communicates via stdin/stdout JSON messages
- Supports headless mode for automation
- Automatically detects binary location

#### MockProvider (Testing)
- Returns predefined responses
- Supports multiple response sequences
- Can simulate delays and errors
- Tracks all calls for test assertions

## Tool System

### Tool Categories

**Read-Only Tools (no confirmation):**
- `get_worker_status` - Query worker pool state
- `get_task_queue` - Get ready beads/tasks
- `get_cost_analytics` - Get spending data
- `get_subscription_usage` - Get quota tracking
- `get_activity_log` - Get recent events

**Action Tools (may require confirmation):**
- `spawn_worker` - Spawn new workers (confirms if count > 2)
- `kill_worker` - Kill a worker (always confirms)
- `assign_task` - Reassign task to model (confirms if in-progress)
- `pause_workers` - Pause all workers (confirms if duration > 10min)
- `resume_workers` - Resume paused workers

### ChatTool Trait

```rust
#[async_trait]
pub trait ChatTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    fn requires_confirmation(&self) -> bool { false }

    async fn execute(
        &self,
        params: serde_json::Value,
        context: &DashboardContext,
    ) -> Result<ToolResult>;

    async fn get_confirmation(
        &self,
        params: &serde_json::Value,
        context: &DashboardContext,
    ) -> Option<ActionConfirmation> { None }
}
```

### Tool Results

```rust
pub struct ToolResult {
    pub success: bool,
    pub data: serde_json::Value,
    pub message: String,
    pub side_effects: Vec<SideEffect>,
}

pub struct SideEffect {
    pub effect_type: String,    // "spawn", "kill", "assign"
    pub description: String,
    pub data: Option<serde_json::Value>,
}
```

## Context Injection

### DashboardContext

The context provides current dashboard state to the AI:

```rust
pub struct DashboardContext {
    pub workers: Vec<WorkerInfo>,
    pub tasks: Vec<TaskInfo>,
    pub costs_today: CostAnalytics,
    pub costs_projected: CostAnalytics,
    pub subscriptions: Vec<SubscriptionInfo>,
    pub recent_events: Vec<EventInfo>,
    pub timestamp: DateTime<Utc>,
}
```

### ContextSource Trait

```rust
#[async_trait]
pub trait ContextSource: Send + Sync {
    async fn gather(&self) -> Result<DashboardContext>;
}
```

### ContextProvider

Wraps a `ContextSource` with caching (5-second default):

```rust
pub struct ContextProvider {
    source: Arc<dyn ContextSource>,
    cache: RwLock<Option<CachedContext>>,
    cache_duration_secs: u64,
}
```

## Rate Limiting

Uses a sliding window algorithm with two windows:

- **Per-minute window**: Default 10 commands
- **Per-hour window**: Default 100 commands

```rust
pub struct RateLimiter {
    config: RateLimitConfig,
    minute_window: Mutex<VecDeque<Instant>>,
    hour_window: Mutex<VecDeque<Instant>>,
}
```

**Error returned when exceeded:**
```rust
ChatError::RateLimitExceeded(limit: u32, wait_secs: u64)
```

## Audit Logging

All interactions are logged to JSONL format:

```rust
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub command: String,
    pub response: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub side_effects: Vec<SideEffect>,
    pub cost_usd: Option<f64>,
    pub duration_ms: u64,
    pub success: bool,
    pub error: Option<String>,
}
```

**Log file location:** `~/.forge/chat-audit.jsonl`

## Error Types

```rust
pub enum ChatError {
    RateLimitExceeded(u32, u64),    // limit, wait_seconds
    ApiError(String),
    ToolNotFound(String),
    ToolExecutionFailed(String),
    ConfirmationRequired(String),   // Serialized ActionConfirmation
    ActionCancelled,
    ContextError(String),
    ConfigError(String),
    AuditError(String),
    IoError(std::io::Error),
    JsonError(serde_json::Error),
    HttpError(reqwest::Error),
    CoreError(forge_core::ForgeError),
}
```

## Usage Examples

### Basic Usage

```rust
use forge_chat::{ChatBackend, ChatConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create with default config (auto-detects provider)
    let config = ChatConfig::default();
    let backend = ChatBackend::new(config).await?;

    // Process a command
    let response = backend.process_command("Why is glm-delta idle?").await?;
    println!("{}", response.text);

    Ok(())
}
```

### Custom Provider

```rust
use forge_chat::{
    ChatBackend, ChatConfig, ProviderConfig, ClaudeApiConfig
};

let config = ChatConfig {
    provider: ProviderConfig::ClaudeApi(ClaudeApiConfig {
        model: "claude-opus-4-5".to_string(),
        max_tokens: 2000,
        ..Default::default()
    }),
    ..Default::default()
};

let backend = ChatBackend::new(config).await?;
```

### With Mock Provider (Testing)

```rust
use forge_chat::{ChatBackend, ChatConfig, MockProvider};

let config = ChatConfig::default();
let mock = MockProvider::new()
    .with_response("Test response")
    .with_delay(100);

let backend = ChatBackend::with_provider(config, Box::new(mock)).await?;
```

### Custom Context Source

```rust
use forge_chat::{ChatBackend, ChatConfig, ContextSource, DashboardContext};

struct MyContextSource { /* ... */ }

#[async_trait]
impl ContextSource for MyContextSource {
    async fn gather(&self) -> Result<DashboardContext> {
        // Gather real dashboard state
    }
}

let backend = ChatBackend::with_context_source(
    ChatConfig::default(),
    MyContextSource { /* ... */ }
).await?;
```

## Full Configuration Example

```json
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
```

## System Prompt

The default system prompt configures the AI as a FORGE dashboard assistant:

```
You are the conversational interface for a distributed worker control panel called FORGE.

Context:
- The user manages a pool of coding agents (workers) across multiple workspaces
- Workers process "beads" (tasks) using different LLM models (Sonnet, Opus, GLM, etc.)
- The system optimizes costs across subscriptions and pay-per-token APIs
- You have access to real-time dashboard state and can execute commands

Your role:
- Answer questions about worker status, costs, tasks, and subscriptions
- Execute commands safely (confirm destructive operations)
- Provide analysis and recommendations
- Be concise (max 5 sentences unless asked for details)
- Use tables, progress bars, and formatting when helpful
- Explain your reasoning when making recommendations

Response format:
- Use markdown for formatting
- Tables for comparisons
- Status indicators: ✓ (success), ✗ (failure), ◐ (in progress)
- Progress bars: ████▌ 66%
- Keep responses under 10 lines (TUI space is limited)

Safety rules:
- Always confirm before killing workers
- Explain cost implications for model changes
- Warn about context loss when reassigning in-progress tasks
- Rate limit: User can run max 10 commands/minute
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ANTHROPIC_API_KEY` | API key for Claude API provider | (required for claude-api) |
| `FORGE_CHAT_PROVIDER` | Override provider type (claude-api, claude-cli, mock) | (auto-detect) |

## Dependencies

```toml
[dependencies]
forge-core = { path = "../forge-core" }
forge-worker = { path = "../forge-worker" }
forge-cost = { path = "../forge-cost" }
thiserror = "2.0"
anyhow = "1.0"
tokio = { features = ["sync", "time", "fs"] }
serde = { features = ["derive"] }
serde_json = "1.0"
reqwest = { features = ["json", "rustls-tls"] }
chrono = { features = ["serde"] }
async-trait = "0.1"
tracing = "0.1"
```

## Testing

The crate includes comprehensive tests:

- **Unit tests**: Each module has inline tests
- **Integration tests**: `tests/integration_tests.rs`, `tests/provider_integration_tests.rs`
- **Mock server tests**: Uses `wiremock` for HTTP API testing

Run tests:
```bash
cargo test -p forge-chat
```
