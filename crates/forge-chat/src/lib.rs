//! # forge-chat
//!
//! AI conversational chat backend for the FORGE control panel.
//!
//! This crate provides:
//! - [`ChatBackend`] - Main chat backend with Claude API integration
//! - [`ChatTool`] - Trait for defining chat tools (read-only and action tools)
//! - [`RateLimiter`] - Rate limiting (10 commands/min by default)
//! - [`AuditLogger`] - JSONL audit logging for all commands
//! - [`ContextProvider`] - Dashboard context injection
//!
//! ## Tool Categories
//!
//! ### Read-Only Tools
//! - `worker_status` - Get current worker pool state
//! - `task_queue` - Get ready beads/tasks
//! - `cost_analytics` - Get spending data
//! - `subscription_usage` - Get quota tracking
//!
//! ### Action Tools (require confirmation)
//! - `spawn_worker` - Spawn new workers
//! - `kill_worker` - Kill a worker
//! - `assign_task` - Reassign task to different model
//!
//! ## Example
//!
//! ```no_run
//! use forge_chat::{ChatBackend, ChatConfig};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Create chat backend with default config
//!     let config = ChatConfig::default();
//!     let backend = ChatBackend::new(config).await?;
//!
//!     // Process a user command
//!     let response = backend.process_command("Why is glm-delta idle?").await?;
//!     println!("{}", response.text);
//!
//!     Ok(())
//! }
//! ```

pub mod audit;
pub mod backend;
pub mod claude_api;
pub mod claude_api_types;
pub mod config;
pub mod context;
pub mod error;
pub mod provider;
pub mod rate_limit;
pub mod tools;

// Re-export main types
pub use audit::{AuditEntry, AuditLogger};
pub use backend::{ChatBackend, ChatResponse};
pub use config::{
    ChatConfig, ClaudeApiConfig, ClaudeCliConfig, MockConfig, ProviderConfig, ProviderType,
};
pub use context::{ContextProvider, DashboardContext};
pub use error::{ChatError, Result};
pub use claude_api::ClaudeApiProvider;
pub use provider::{
    ChatProvider, ClaudeCliProvider, FinishReason, MockProvider,
    ProviderResponse, ProviderTool, TokenUsage,
};
pub use rate_limit::RateLimiter;
pub use tools::{
    ActionConfirmation, ChatTool, ToolCall, ToolRegistry, ToolResult,
};
