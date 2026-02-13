//! Subscription status tracking panel for the FORGE TUI.
//!
//! This module provides subscription usage visualization with:
//! - Usage table for Claude Pro, ChatGPT Plus, Cursor Pro, DeepSeek
//! - Color-coded progress bars showing usage percentage
//! - Reset timers showing when quotas refresh
//! - Recommended actions based on usage patterns

use chrono::{DateTime, Duration, Utc};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table, Widget},
};

/// Subscription service types supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubscriptionService {
    /// Claude Pro subscription
    ClaudePro,
    /// ChatGPT Plus subscription
    ChatGPTPlus,
    /// Cursor Pro subscription
    CursorPro,
    /// DeepSeek API subscription
    DeepSeekAPI,
}

impl SubscriptionService {
    /// Display name for the service.
    pub fn display_name(&self) -> &'static str {
        match self {
            SubscriptionService::ClaudePro => "Claude Pro",
            SubscriptionService::ChatGPTPlus => "ChatGPT Plus",
            SubscriptionService::CursorPro => "Cursor Pro",
            SubscriptionService::DeepSeekAPI => "DeepSeek API",
        }
    }

    /// Short name for compact display.
    pub fn short_name(&self) -> &'static str {
        match self {
            SubscriptionService::ClaudePro => "Claude",
            SubscriptionService::ChatGPTPlus => "ChatGPT",
            SubscriptionService::CursorPro => "Cursor",
            SubscriptionService::DeepSeekAPI => "DeepSeek",
        }
    }

    /// All subscription services.
    pub const ALL: [SubscriptionService; 4] = [
        SubscriptionService::ClaudePro,
        SubscriptionService::ChatGPTPlus,
        SubscriptionService::CursorPro,
        SubscriptionService::DeepSeekAPI,
    ];
}

/// Recommended action for a subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionAction {
    /// On track with normal usage - no action needed
    OnPace,
    /// Accelerate usage to maximize value before reset
    Accelerate,
    /// Max out - use all remaining quota
    MaxOut,
    /// Active pay-per-use - just monitor spending
    Active,
    /// Paused or inactive subscription
    Paused,
    /// Over quota - wait for reset
    OverQuota,
}

impl SubscriptionAction {
    /// Icon for the action.
    pub fn icon(&self) -> &'static str {
        match self {
            SubscriptionAction::OnPace => "📊",
            SubscriptionAction::Accelerate => "🚀",
            SubscriptionAction::MaxOut => "⚠️",
            SubscriptionAction::Active => "💰",
            SubscriptionAction::Paused => "⏸️",
            SubscriptionAction::OverQuota => "🛑",
        }
    }

    /// Description of the action.
    pub fn description(&self) -> &'static str {
        match self {
            SubscriptionAction::OnPace => "On-Pace",
            SubscriptionAction::Accelerate => "Accel",
            SubscriptionAction::MaxOut => "MaxOut",
            SubscriptionAction::Active => "Active",
            SubscriptionAction::Paused => "Paused",
            SubscriptionAction::OverQuota => "Over",
        }
    }

    /// Color for the action.
    pub fn color(&self) -> Color {
        match self {
            SubscriptionAction::OnPace => Color::Green,
            SubscriptionAction::Accelerate => Color::Cyan,
            SubscriptionAction::MaxOut => Color::Yellow,
            SubscriptionAction::Active => Color::Green,
            SubscriptionAction::Paused => Color::Gray,
            SubscriptionAction::OverQuota => Color::Red,
        }
    }
}

/// Reset period type for subscriptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetPeriod {
    /// Hourly reset (e.g., ChatGPT 40 messages per 3 hours)
    Hourly(u32),
    /// Daily reset
    Daily,
    /// Weekly reset
    Weekly,
    /// Monthly reset
    Monthly,
    /// No reset (pay-per-use)
    PayPerUse,
}

impl ResetPeriod {
    /// Format the reset period for display.
    pub fn display(&self) -> String {
        match self {
            ResetPeriod::Hourly(hours) => format!("{}hr", hours),
            ResetPeriod::Daily => "Daily".to_string(),
            ResetPeriod::Weekly => "Weekly".to_string(),
            ResetPeriod::Monthly => "Monthly".to_string(),
            ResetPeriod::PayPerUse => "Pay/Use".to_string(),
        }
    }
}

/// Status of a single subscription.
#[derive(Debug, Clone)]
pub struct SubscriptionStatus {
    /// Service type
    pub service: SubscriptionService,
    /// Current usage (messages, requests, or tokens)
    pub current_usage: u64,
    /// Usage limit (None for pay-per-use)
    pub limit: Option<u64>,
    /// Unit of measurement (messages, requests, tokens)
    pub unit: String,
    /// When the quota resets
    pub resets_at: Option<DateTime<Utc>>,
    /// Reset period type
    pub reset_period: ResetPeriod,
    /// Whether the subscription is active
    pub is_active: bool,
    /// Daily spend for pay-per-use subscriptions
    pub daily_spend: Option<f64>,
    /// Last updated timestamp
    pub last_updated: DateTime<Utc>,
}

impl SubscriptionStatus {
    /// Create a new subscription status.
    pub fn new(service: SubscriptionService) -> Self {
        Self {
            service,
            current_usage: 0,
            limit: None,
            unit: "msgs".to_string(),
            resets_at: None,
            reset_period: ResetPeriod::Monthly,
            is_active: false,
            daily_spend: None,
            last_updated: Utc::now(),
        }
    }

    /// Set usage and limit.
    pub fn with_usage(mut self, current: u64, limit: u64, unit: &str) -> Self {
        self.current_usage = current;
        self.limit = Some(limit);
        self.unit = unit.to_string();
        self
    }

    /// Set reset time.
    pub fn with_reset(mut self, resets_at: DateTime<Utc>, period: ResetPeriod) -> Self {
        self.resets_at = Some(resets_at);
        self.reset_period = period;
        self
    }

    /// Set as pay-per-use with daily spend.
    pub fn with_pay_per_use(mut self, daily_spend: f64) -> Self {
        self.limit = None;
        self.daily_spend = Some(daily_spend);
        self.reset_period = ResetPeriod::PayPerUse;
        self
    }

    /// Set active status.
    pub fn with_active(mut self, active: bool) -> Self {
        self.is_active = active;
        self
    }

    /// Get usage percentage (0-100).
    pub fn usage_pct(&self) -> f64 {
        match self.limit {
            Some(limit) if limit > 0 => (self.current_usage as f64 / limit as f64) * 100.0,
            _ => 0.0,
        }
    }

    /// Get remaining usage.
    pub fn remaining(&self) -> Option<u64> {
        self.limit.map(|l| l.saturating_sub(self.current_usage))
    }

    /// Get time until reset.
    pub fn time_until_reset(&self) -> Option<Duration> {
        self.resets_at.map(|r| {
            let now = Utc::now();
            if r > now { r - now } else { Duration::zero() }
        })
    }

    /// Format time until reset for display.
    pub fn format_reset_timer(&self) -> String {
        match self.time_until_reset() {
            Some(duration) if duration > Duration::zero() => {
                let days = duration.num_days();
                let hours = duration.num_hours() % 24;
                let minutes = duration.num_minutes() % 60;

                if days > 0 {
                    format!("{}d {}h", days, hours)
                } else if hours > 0 {
                    format!("{}h {}m", hours, minutes)
                } else {
                    format!("{}m", minutes)
                }
            }
            Some(_) => "Resetting...".to_string(),
            None => match self.reset_period {
                ResetPeriod::PayPerUse => "Monthly".to_string(),
                _ => "-".to_string(),
            },
        }
    }

    /// Determine the recommended action based on usage patterns.
    pub fn recommended_action(&self) -> SubscriptionAction {
        if !self.is_active {
            return SubscriptionAction::Paused;
        }

        // Pay-per-use subscriptions are always "Active"
        if matches!(self.reset_period, ResetPeriod::PayPerUse) {
            return SubscriptionAction::Active;
        }

        let usage_pct = self.usage_pct();

        // Check if over quota
        if usage_pct >= 100.0 {
            return SubscriptionAction::OverQuota;
        }

        // Calculate expected usage based on time remaining
        if let Some(duration) = self.time_until_reset() {
            let total_hours = match self.reset_period {
                ResetPeriod::Hourly(h) => h as f64,
                ResetPeriod::Daily => 24.0,
                ResetPeriod::Weekly => 24.0 * 7.0,
                ResetPeriod::Monthly => 24.0 * 30.0,
                ResetPeriod::PayPerUse => return SubscriptionAction::Active,
            };

            let hours_remaining =
                duration.num_hours() as f64 + (duration.num_minutes() % 60) as f64 / 60.0;
            let hours_elapsed = total_hours - hours_remaining;
            let expected_pct = if total_hours > 0.0 {
                (hours_elapsed / total_hours) * 100.0
            } else {
                100.0
            };

            // Compare actual vs expected
            let ratio = if expected_pct > 0.0 {
                usage_pct / expected_pct
            } else {
                0.0
            };

            // Determine action based on ratio and remaining time
            if usage_pct >= 95.0 {
                SubscriptionAction::MaxOut
            } else if ratio < 0.5 && usage_pct < 50.0 && hours_remaining < total_hours * 0.3 {
                // Less than 30% time left but less than 50% used
                SubscriptionAction::MaxOut
            } else if ratio < 0.7 && usage_pct < 70.0 {
                SubscriptionAction::Accelerate
            } else {
                SubscriptionAction::OnPace
            }
        } else {
            // No reset time info - assume on pace
            SubscriptionAction::OnPace
        }
    }

    /// Get color for usage bar based on percentage.
    pub fn usage_color(&self) -> Color {
        let pct = self.usage_pct();
        if pct >= 95.0 {
            Color::Red
        } else if pct >= 80.0 {
            Color::LightRed
        } else if pct >= 60.0 {
            Color::Yellow
        } else if pct >= 40.0 {
            Color::Cyan
        } else {
            Color::Green
        }
    }
}

/// Collection of all subscription statuses.
#[derive(Debug, Default)]
pub struct SubscriptionData {
    /// Subscription statuses by service
    pub subscriptions: Vec<SubscriptionStatus>,
    /// Whether data is loading
    pub is_loading: bool,
    /// Error message if any
    pub error: Option<String>,
    /// Last update timestamp
    pub last_updated: Option<DateTime<Utc>>,
}

impl SubscriptionData {
    /// Create new empty subscription data.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with loading state.
    pub fn loading() -> Self {
        Self {
            is_loading: true,
            ..Default::default()
        }
    }

    /// Create with demo/mock data for display.
    pub fn with_demo_data() -> Self {
        let now = Utc::now();

        let subscriptions = vec![
            // Claude Pro: 328/500 messages, resets in 16 days
            SubscriptionStatus::new(SubscriptionService::ClaudePro)
                .with_usage(328, 500, "msgs")
                .with_reset(
                    now + Duration::days(16) + Duration::hours(9),
                    ResetPeriod::Monthly,
                )
                .with_active(true),
            // ChatGPT Plus: 12/40 messages per 3 hours
            SubscriptionStatus::new(SubscriptionService::ChatGPTPlus)
                .with_usage(12, 40, "msg/3hr")
                .with_reset(
                    now + Duration::hours(23) + Duration::minutes(14),
                    ResetPeriod::Hourly(3),
                )
                .with_active(true),
            // Cursor Pro: 487/500 requests
            SubscriptionStatus::new(SubscriptionService::CursorPro)
                .with_usage(487, 500, "reqs")
                .with_reset(
                    now + Duration::days(8) + Duration::hours(3),
                    ResetPeriod::Monthly,
                )
                .with_active(true),
            // DeepSeek: Pay-per-use
            SubscriptionStatus::new(SubscriptionService::DeepSeekAPI)
                .with_pay_per_use(0.02)
                .with_active(true),
        ];

        Self {
            subscriptions,
            is_loading: false,
            error: None,
            last_updated: Some(now),
        }
    }

    /// Get subscription by service type.
    pub fn get(&self, service: SubscriptionService) -> Option<&SubscriptionStatus> {
        self.subscriptions.iter().find(|s| s.service == service)
    }

    /// Check if any subscription is active.
    pub fn has_active(&self) -> bool {
        self.subscriptions.iter().any(|s| s.is_active)
    }

    /// Count active subscriptions.
    pub fn active_count(&self) -> usize {
        self.subscriptions.iter().filter(|s| s.is_active).count()
    }

    /// Check if we have data.
    pub fn has_data(&self) -> bool {
        !self.subscriptions.is_empty()
    }
}

/// Render a horizontal progress bar.
pub fn render_usage_bar(pct: f64, width: usize, color: Color) -> Line<'static> {
    let pct = pct.clamp(0.0, 100.0);
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);

    let filled_str: String = "█".repeat(filled);
    let empty_str: String = "░".repeat(empty);

    Line::from(vec![
        Span::styled(filled_str, Style::default().fg(color)),
        Span::styled(empty_str, Style::default().fg(Color::DarkGray)),
    ])
}

/// Subscription panel widget.
pub struct SubscriptionPanel<'a> {
    data: &'a SubscriptionData,
    focused: bool,
}

impl<'a> SubscriptionPanel<'a> {
    /// Create a new subscription panel.
    pub fn new(data: &'a SubscriptionData) -> Self {
        Self {
            data,
            focused: false,
        }
    }

    /// Set focus state.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Render the subscription table.
    fn render_table(&self, area: Rect, buf: &mut Buffer) {
        if self.data.subscriptions.is_empty() {
            let msg = Paragraph::new("No subscription data available")
                .style(Style::default().fg(Color::Gray));
            msg.render(area, buf);
            return;
        }

        // Build table rows
        let rows: Vec<Row> = self
            .data
            .subscriptions
            .iter()
            .map(|sub| {
                let service = sub.service.display_name();

                // Usage column with bar
                let usage_str = if let Some(limit) = sub.limit {
                    format!("{}/{}", sub.current_usage, limit)
                } else if let Some(spend) = sub.daily_spend {
                    format!("${:.2}/d", spend)
                } else {
                    "-".to_string()
                };

                // Limit/unit column
                let limit_str = if sub.limit.is_some() {
                    sub.unit.clone()
                } else {
                    "∞".to_string()
                };

                // Reset timer
                let reset_str = sub.format_reset_timer();

                // Action with icon
                let action = sub.recommended_action();
                let action_str = format!("{} {}", action.icon(), action.description());

                Row::new(vec![
                    service.to_string(),
                    usage_str,
                    limit_str,
                    reset_str,
                    action_str,
                ])
                .style(if sub.is_active {
                    Style::default()
                } else {
                    Style::default().fg(Color::DarkGray)
                })
            })
            .collect();

        let header = Row::new(vec!["Service", "Usage", "Limit", "Resets", "Action"])
            .style(
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::BOLD),
            )
            .bottom_margin(0);

        let widths = [
            Constraint::Length(12),
            Constraint::Length(9),
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Length(11),
        ];

        let table = Table::new(rows, widths).header(header).column_spacing(1);

        ratatui::widgets::Widget::render(table, area, buf);
    }

    /// Render usage bars below the table.
    #[allow(dead_code)]
    fn render_usage_bars(&self, area: Rect, buf: &mut Buffer) {
        let mut lines = Vec::new();

        for sub in &self.data.subscriptions {
            if sub.limit.is_some() {
                let pct = sub.usage_pct();
                let color = sub.usage_color();
                let bar_width = 15.min(area.width.saturating_sub(20) as usize);

                // Service name and bar
                let bar = render_usage_bar(pct, bar_width, color);
                lines.push(Line::from(vec![Span::styled(
                    format!("{:<12}", sub.service.short_name()),
                    Style::default().fg(Color::White),
                )]));
                lines.push(bar);
                lines.push(Line::from(vec![Span::styled(
                    format!(
                        "{:>5}/{:<5} ({:.0}%)",
                        sub.current_usage,
                        sub.limit.unwrap_or(0),
                        pct
                    ),
                    Style::default().fg(Color::Gray),
                )]));
                lines.push(Line::from(""));
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "No usage data",
                Style::default().fg(Color::Gray),
            )));
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}

impl Widget for SubscriptionPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Focus indicator icon: "◆" for focused, "◇" for unfocused
        let focus_icon = if self.focused { "◆" } else { "◇" };

        // Border style: bright cyan for focused, dim for unfocused
        let border_style = if self.focused {
            Style::default().fg(Color::LightCyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // Title style: bold + bright for focused, dim for unfocused
        let title_style = if self.focused {
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(
                format!(" {} Subscription Status ", focus_icon),
                title_style,
            ));

        let inner = block.inner(area);
        block.render(area, buf);

        // Handle special states
        if self.data.is_loading {
            let loading = Paragraph::new("Loading subscription data...")
                .style(Style::default().fg(Color::Yellow));
            loading.render(inner, buf);
            return;
        }

        if let Some(ref error) = self.data.error {
            let err =
                Paragraph::new(format!("Error: {}", error)).style(Style::default().fg(Color::Red));
            err.render(inner, buf);
            return;
        }

        if !self.data.has_data() {
            let no_data = Paragraph::new(
                "No subscription data.\n\n\
                 Configure subscriptions in:\n\
                 ~/.forge/subscriptions.yaml",
            )
            .style(Style::default().fg(Color::Gray));
            no_data.render(inner, buf);
            return;
        }

        // Render the table
        self.render_table(inner, buf);
    }
}

/// Compact subscription summary widget for overview panel.
pub struct SubscriptionSummaryCompact<'a> {
    data: &'a SubscriptionData,
}

impl<'a> SubscriptionSummaryCompact<'a> {
    /// Create a new compact subscription summary.
    pub fn new(data: &'a SubscriptionData) -> Self {
        Self { data }
    }
}

impl Widget for SubscriptionSummaryCompact<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.data.is_loading {
            let loading = Paragraph::new("Loading...").style(Style::default().fg(Color::Yellow));
            loading.render(area, buf);
            return;
        }

        if !self.data.has_data() {
            let no_data =
                Paragraph::new("No subscription data").style(Style::default().fg(Color::Gray));
            no_data.render(area, buf);
            return;
        }

        let mut lines = Vec::new();

        for sub in &self.data.subscriptions {
            if !sub.is_active {
                continue;
            }

            let action = sub.recommended_action();

            if let Some(limit) = sub.limit {
                let pct = sub.usage_pct();
                let bar_width = 9.min(area.width.saturating_sub(14) as usize);
                let filled = ((pct / 100.0) * bar_width as f64).round() as usize;
                let empty = bar_width.saturating_sub(filled);

                let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{:<12}", sub.service.short_name()),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(bar, Style::default().fg(sub.usage_color())),
                    Span::raw(" "),
                    Span::styled(
                        format!("{}/{}", sub.current_usage, limit),
                        Style::default().fg(Color::Gray),
                    ),
                ]));

                lines.push(Line::from(vec![
                    Span::raw("             "),
                    Span::styled(sub.format_reset_timer(), Style::default().fg(Color::Gray)),
                    Span::raw("  "),
                    Span::styled(
                        format!("{} {}", action.icon(), action.description()),
                        Style::default().fg(action.color()),
                    ),
                ]));
            } else if let Some(spend) = sub.daily_spend {
                // Pay-per-use display
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{:<12}", sub.service.short_name()),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled("Pay/Use", Style::default().fg(Color::Gray)),
                    Span::raw("  "),
                    Span::styled(format!("${:.2}/d", spend), Style::default().fg(Color::Cyan)),
                ]));

                lines.push(Line::from(vec![
                    Span::raw("             "),
                    Span::styled("Monthly", Style::default().fg(Color::Gray)),
                    Span::raw("  "),
                    Span::styled(
                        format!("{} {}", action.icon(), action.description()),
                        Style::default().fg(action.color()),
                    ),
                ]));
            }

            lines.push(Line::from(""));
        }

        // Remove trailing empty line
        if lines.last().map_or(false, |l| l.spans.is_empty()) {
            lines.pop();
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}

/// Format subscription data for text-based display in the Utilization panel.
pub fn format_subscription_summary(data: &SubscriptionData) -> String {
    if data.is_loading {
        return "Loading subscription data...".to_string();
    }

    if !data.has_data() {
        return "No subscriptions configured.\n\n\
                Configure subscriptions in:\n\
                ~/.forge/subscriptions.yaml"
            .to_string();
    }

    let mut lines = Vec::new();

    lines.push("Subscription Status:".to_string());
    lines.push(String::new());

    for sub in &data.subscriptions {
        if !sub.is_active {
            continue;
        }

        let action = sub.recommended_action();

        if let Some(limit) = sub.limit {
            let pct = sub.usage_pct();
            let bar_width: usize = 10;
            let filled = ((pct / 100.0) * bar_width as f64).round() as usize;
            let empty = bar_width.saturating_sub(filled);
            let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(empty));

            lines.push(format!(
                "{} {} {}/{}",
                sub.service.short_name(),
                bar,
                sub.current_usage,
                limit
            ));
            lines.push(format!(
                "  Reset: {}  {} {}",
                sub.format_reset_timer(),
                action.icon(),
                action.description()
            ));
        } else if let Some(spend) = sub.daily_spend {
            lines.push(format!(
                "{} [Pay/Use] ${:.2}/day",
                sub.service.short_name(),
                spend
            ));
            lines.push(format!(
                "  Billing: Monthly  {} {}",
                action.icon(),
                action.description()
            ));
        }

        lines.push(String::new());
    }

    // Remove trailing empty line
    if lines.last().map_or(false, |l| l.is_empty()) {
        lines.pop();
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_service_names() {
        assert_eq!(SubscriptionService::ClaudePro.display_name(), "Claude Pro");
        assert_eq!(SubscriptionService::ClaudePro.short_name(), "Claude");
        assert_eq!(
            SubscriptionService::DeepSeekAPI.display_name(),
            "DeepSeek API"
        );
    }

    #[test]
    fn test_subscription_action() {
        assert_eq!(SubscriptionAction::OnPace.icon(), "📊");
        assert_eq!(SubscriptionAction::MaxOut.description(), "MaxOut");
        assert_eq!(SubscriptionAction::Active.color(), Color::Green);
    }

    #[test]
    fn test_reset_period_display() {
        assert_eq!(ResetPeriod::Hourly(3).display(), "3hr");
        assert_eq!(ResetPeriod::Daily.display(), "Daily");
        assert_eq!(ResetPeriod::Monthly.display(), "Monthly");
        assert_eq!(ResetPeriod::PayPerUse.display(), "Pay/Use");
    }

    #[test]
    fn test_subscription_status_usage_pct() {
        let status =
            SubscriptionStatus::new(SubscriptionService::ClaudePro).with_usage(250, 500, "msgs");

        assert!((status.usage_pct() - 50.0).abs() < 0.01);
        assert_eq!(status.remaining(), Some(250));
    }

    #[test]
    fn test_subscription_status_pay_per_use() {
        let status = SubscriptionStatus::new(SubscriptionService::DeepSeekAPI)
            .with_pay_per_use(0.05)
            .with_active(true);

        assert!((status.usage_pct() - 0.0).abs() < 0.01);
        assert_eq!(status.remaining(), None);
        assert_eq!(status.recommended_action(), SubscriptionAction::Active);
    }

    #[test]
    fn test_subscription_status_inactive() {
        let status = SubscriptionStatus::new(SubscriptionService::ChatGPTPlus)
            .with_usage(10, 40, "msgs")
            .with_active(false);

        assert_eq!(status.recommended_action(), SubscriptionAction::Paused);
    }

    #[test]
    fn test_subscription_status_over_quota() {
        let status = SubscriptionStatus::new(SubscriptionService::CursorPro)
            .with_usage(510, 500, "reqs")
            .with_reset(Utc::now() + Duration::days(5), ResetPeriod::Monthly)
            .with_active(true);

        assert!(status.usage_pct() >= 100.0);
        assert_eq!(status.recommended_action(), SubscriptionAction::OverQuota);
    }

    #[test]
    fn test_subscription_status_colors() {
        // Low usage - green
        let low =
            SubscriptionStatus::new(SubscriptionService::ClaudePro).with_usage(100, 500, "msgs");
        assert_eq!(low.usage_color(), Color::Green);

        // High usage - red
        let high =
            SubscriptionStatus::new(SubscriptionService::ClaudePro).with_usage(480, 500, "msgs");
        assert_eq!(high.usage_color(), Color::Red);
    }

    #[test]
    fn test_subscription_data_demo() {
        let data = SubscriptionData::with_demo_data();

        assert!(data.has_data());
        assert!(!data.is_loading);
        assert!(data.has_active());
        assert_eq!(data.active_count(), 4);

        // Check Claude Pro
        let claude = data.get(SubscriptionService::ClaudePro).unwrap();
        assert_eq!(claude.current_usage, 328);
        assert_eq!(claude.limit, Some(500));
    }

    #[test]
    fn test_format_reset_timer() {
        let now = Utc::now();

        // Days and hours
        let status = SubscriptionStatus::new(SubscriptionService::ClaudePro).with_reset(
            now + Duration::days(5) + Duration::hours(12),
            ResetPeriod::Monthly,
        );
        let timer = status.format_reset_timer();
        assert!(timer.contains("d"));

        // Hours and minutes
        let status = SubscriptionStatus::new(SubscriptionService::ChatGPTPlus).with_reset(
            now + Duration::hours(2) + Duration::minutes(30),
            ResetPeriod::Hourly(3),
        );
        let timer = status.format_reset_timer();
        assert!(timer.contains("h") || timer.contains("m"));

        // Pay-per-use
        let status =
            SubscriptionStatus::new(SubscriptionService::DeepSeekAPI).with_pay_per_use(0.05);
        let timer = status.format_reset_timer();
        assert_eq!(timer, "Monthly");
    }

    #[test]
    fn test_render_usage_bar() {
        let bar = render_usage_bar(50.0, 10, Color::Green);
        assert_eq!(bar.spans.len(), 2);
    }

    #[test]
    fn test_format_subscription_summary() {
        let data = SubscriptionData::with_demo_data();
        let summary = format_subscription_summary(&data);

        assert!(summary.contains("Claude"));
        assert!(summary.contains("ChatGPT"));
        assert!(summary.contains("Cursor"));
        assert!(summary.contains("DeepSeek"));
    }

    #[test]
    fn test_format_subscription_summary_loading() {
        let data = SubscriptionData::loading();
        let summary = format_subscription_summary(&data);

        assert!(summary.contains("Loading"));
    }

    #[test]
    fn test_format_subscription_summary_empty() {
        let data = SubscriptionData::new();
        let summary = format_subscription_summary(&data);

        assert!(summary.contains("No subscriptions"));
    }
}
