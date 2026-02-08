//! Terminal UI for FORGE.
//!
//! This crate provides the Ratatui-based terminal interface for FORGE.
//!
//! ## Features
//!
//! - Multi-view dashboard with hotkey navigation
//! - Real-time worker status monitoring
//! - Task queue visualization
//! - Cost analytics display
//! - Conversational chat interface
//!
//! ## Hotkeys
//!
//! - `o` - Overview (dashboard)
//! - `w` - Workers view
//! - `t` - Tasks view
//! - `c` - Costs view
//! - `m` - Metrics view
//! - `l` - Logs view
//! - `:` - Chat input
//! - `?` or `h` - Help
//! - `q` - Quit
//! - `Tab` - Cycle views
//! - `Esc` - Cancel/back

pub mod app;
pub mod event;
pub mod status;
pub mod view;
pub mod widget;

pub use app::{App, AppResult};
pub use status::{StatusEvent, StatusWatcher, StatusWatcherConfig, WorkerStatusFile};
pub use view::View;
