// Clippy allows for TUI code patterns that are intentional
#![allow(clippy::collapsible_if)]
#![allow(clippy::unnecessary_map_or)]
#![allow(clippy::vec_init_then_push)]
#![allow(clippy::for_kv_map)]
#![allow(clippy::manual_div_ceil)]
#![allow(clippy::nonminimal_bool)]
#![allow(clippy::should_implement_trait)]
#![allow(clippy::derivable_impls)]

//! Terminal UI for FORGE.
//!
//! This crate provides the Ratatui-based terminal interface for FORGE.
//!
//! ## Features
//!
//! - Multi-view dashboard with hotkey navigation
//! - Real-time worker status monitoring
//! - Task queue visualization with bead integration
//! - Cost analytics display
//! - Conversational chat interface
//! - Log streaming with ring buffer
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
pub mod bead;
pub mod cost_panel;
pub mod data;
pub mod event;
#[cfg(test)]
mod integration_tests;
pub mod log;
pub mod log_watcher;
pub mod metrics_panel;
pub mod status;
pub mod subscription_panel;
pub mod theme;
pub mod view;
pub mod widget;

pub use widget::{
    HotkeyHints, ProgressBar, ProgressColorMode, ProgressFillStyle, QuickAction, QuickActionType,
    QuickActionsPanel, SparklineDirection, SparklineWidget, StatusIndicator, render_sparkline,
    render_sparkline_i64,
};

pub use app::{App, AppResult};
pub use bead::{Bead, BeadManager, BeadStats};
pub use cost_panel::{
    BudgetAlertLevel, BudgetConfig, CostPanel, CostPanelData, CostSummaryCompact,
};
pub use data::{DataManager, WorkerData};
pub use event::{AppEvent, InputHandler, WorkerExecutor};
pub use log::{LogBuffer, LogEntry, LogEvent, LogLevel, LogTailer, LogTailerConfig};
pub use log_watcher::{
    LogWatcher, LogWatcherConfig, LogWatcherError, LogWatcherEvent, RealtimeMetrics,
    DEFAULT_DEBOUNCE_MS, DEFAULT_LOG_DIR, DEFAULT_POLL_INTERVAL_MS,
};
pub use metrics_panel::{MetricsPanel, MetricsPanelData, MetricsSummaryCompact};
pub use status::{StatusEvent, StatusWatcher, StatusWatcherConfig, WorkerStatusFile};
pub use subscription_panel::{
    SubscriptionAction, SubscriptionData, SubscriptionPanel, SubscriptionService,
    SubscriptionStatus, SubscriptionSummaryCompact, format_subscription_summary,
};
pub use theme::{Theme, ThemeColors, ThemeManager, ThemeName};
pub use view::{FocusPanel, LayoutMode, View};
