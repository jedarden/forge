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
pub mod metrics_panel;
pub mod status;
pub mod subscription_panel;
pub mod view;
pub mod widget;

pub use widget::{
    render_sparkline, render_sparkline_i64, HotkeyHints, ProgressBar, ProgressColorMode,
    ProgressFillStyle, QuickAction, QuickActionsPanel, QuickActionType, SparklineDirection,
    SparklineWidget, StatusIndicator,
};

pub use app::{App, AppResult};
pub use bead::{Bead, BeadManager, BeadStats};
pub use cost_panel::{BudgetAlertLevel, BudgetConfig, CostPanel, CostPanelData, CostSummaryCompact};
pub use data::{DataManager, WorkerData};
pub use event::{AppEvent, InputHandler, WorkerExecutor};
pub use log::{LogBuffer, LogEntry, LogEvent, LogLevel, LogTailer, LogTailerConfig};
pub use metrics_panel::{MetricsPanel, MetricsPanelData, MetricsSummaryCompact};
pub use status::{StatusEvent, StatusWatcher, StatusWatcherConfig, WorkerStatusFile};
pub use subscription_panel::{
    format_subscription_summary, SubscriptionAction, SubscriptionData, SubscriptionPanel,
    SubscriptionService, SubscriptionStatus, SubscriptionSummaryCompact,
};
pub use view::{FocusPanel, LayoutMode, View};
