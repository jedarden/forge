//! Cost analytics panel for the FORGE TUI.
//!
//! This module provides rich cost visualization with:
//! - Per-model breakdown table
//! - Sparkline trends
//! - Cost breakdown chart by priority
//! - Summary stats
//! - Color-coded budget alerts
//! - Optimization recommendations
//! - Savings achieved display

use chrono::{NaiveDate, Utc};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table, Widget},
};

use crate::log_watcher::RealtimeMetrics;
use forge_cost::{CostBreakdown, DailyCost, ModelCost, OptimizationRecommendation, ProjectedCost};

/// Budget alert severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetAlertLevel {
    /// Under budget - everything is fine
    Normal,
    /// Approaching budget threshold (70-90%)
    Warning,
    /// At or exceeding budget (90%+)
    Critical,
    /// Budget exceeded
    Exceeded,
}

impl BudgetAlertLevel {
    /// Get the color for this alert level.
    pub fn color(&self) -> Color {
        match self {
            BudgetAlertLevel::Normal => Color::Green,
            BudgetAlertLevel::Warning => Color::Yellow,
            BudgetAlertLevel::Critical => Color::LightRed,
            BudgetAlertLevel::Exceeded => Color::Red,
        }
    }

    /// Get icon for this alert level.
    pub fn icon(&self) -> &'static str {
        match self {
            BudgetAlertLevel::Normal => "✓",
            BudgetAlertLevel::Warning => "⚠",
            BudgetAlertLevel::Critical => "⚠",
            BudgetAlertLevel::Exceeded => "✗",
        }
    }

    /// Get description for this alert level.
    pub fn description(&self) -> &'static str {
        match self {
            BudgetAlertLevel::Normal => "On Track",
            BudgetAlertLevel::Warning => "Approaching Limit",
            BudgetAlertLevel::Critical => "Near Limit",
            BudgetAlertLevel::Exceeded => "Over Budget",
        }
    }

    /// Determine alert level from usage percentage.
    pub fn from_percentage(pct: f64) -> Self {
        if pct >= 100.0 {
            BudgetAlertLevel::Exceeded
        } else if pct >= 90.0 {
            BudgetAlertLevel::Critical
        } else if pct >= 70.0 {
            BudgetAlertLevel::Warning
        } else {
            BudgetAlertLevel::Normal
        }
    }
}

/// Budget configuration for cost tracking.
#[derive(Debug, Clone)]
pub struct BudgetConfig {
    /// Monthly budget limit in USD
    pub monthly_limit: f64,
    /// Daily budget limit in USD (optional, calculated from monthly if not set)
    pub daily_limit: Option<f64>,
    /// Warning threshold percentage (default 70%)
    pub warning_threshold: f64,
    /// Critical threshold percentage (default 90%)
    pub critical_threshold: f64,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            monthly_limit: 500.0, // $500/month default
            daily_limit: None,
            warning_threshold: 70.0,
            critical_threshold: 90.0,
        }
    }
}

impl BudgetConfig {
    /// Create a new budget config with a monthly limit.
    pub fn new(monthly_limit: f64) -> Self {
        Self {
            monthly_limit,
            ..Default::default()
        }
    }

    /// Set daily limit.
    pub fn with_daily_limit(mut self, limit: f64) -> Self {
        self.daily_limit = Some(limit);
        self
    }

    /// Get effective daily limit (calculated or explicit).
    pub fn effective_daily_limit(&self) -> f64 {
        self.daily_limit.unwrap_or(self.monthly_limit / 30.0)
    }

    /// Get alert level for current spending.
    pub fn get_monthly_alert(&self, current_spend: f64) -> BudgetAlertLevel {
        let pct = (current_spend / self.monthly_limit) * 100.0;
        BudgetAlertLevel::from_percentage(pct)
    }

    /// Get alert level for daily spending.
    pub fn get_daily_alert(&self, current_spend: f64) -> BudgetAlertLevel {
        let limit = self.effective_daily_limit();
        let pct = (current_spend / limit) * 100.0;
        BudgetAlertLevel::from_percentage(pct)
    }
}

/// Cost data prepared for TUI display.
#[derive(Debug, Default)]
pub struct CostPanelData {
    /// Today's costs
    pub today: Option<DailyCost>,
    /// Current month's total cost
    pub monthly_total: f64,
    /// Monthly cost by model
    pub monthly_by_model: Vec<ModelCost>,
    /// Daily costs for trend (last 7-14 days)
    pub daily_trend: Vec<(NaiveDate, f64)>,
    /// Projected costs
    pub projected: Option<ProjectedCost>,
    /// Budget configuration
    pub budget: BudgetConfig,
    /// Optimization recommendations
    pub recommendations: Vec<OptimizationRecommendation>,
    /// Total potential savings from optimizations
    pub potential_savings: f64,
    /// Savings achieved from previous optimizations
    pub savings_achieved: f64,
    /// Subscription utilization percentage
    pub subscription_utilization: f64,
    /// Real-time metrics from log parsing
    pub realtime: RealtimeMetrics,
    /// Last update timestamp
    pub last_update: Option<chrono::DateTime<Utc>>,
    /// Whether data is loading
    pub is_loading: bool,
    /// Error message if any
    pub error: Option<String>,
}

impl CostPanelData {
    /// Create new empty cost panel data.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set loading state.
    pub fn loading() -> Self {
        Self {
            is_loading: true,
            ..Default::default()
        }
    }

    /// Set error state.
    pub fn with_error(error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Default::default()
        }
    }

    /// Update with today's cost data.
    pub fn set_today(&mut self, today: DailyCost) {
        self.today = Some(today);
        self.last_update = Some(Utc::now());
        self.is_loading = false;
    }

    /// Update with monthly data.
    pub fn set_monthly(&mut self, total: f64, by_model: Vec<ModelCost>) {
        self.monthly_total = total;
        self.monthly_by_model = by_model;
    }

    /// Update with daily trend data.
    pub fn set_daily_trend(&mut self, trend: Vec<(NaiveDate, f64)>) {
        self.daily_trend = trend;
    }

    /// Update with projected costs.
    pub fn set_projected(&mut self, projected: ProjectedCost) {
        self.projected = Some(projected);
    }

    /// Set budget configuration.
    pub fn set_budget(&mut self, budget: BudgetConfig) {
        self.budget = budget;
    }

    /// Set optimization recommendations.
    pub fn set_recommendations(&mut self, recommendations: Vec<OptimizationRecommendation>) {
        self.potential_savings = recommendations.iter().map(|r| r.estimated_savings).sum();
        self.recommendations = recommendations;
    }

    /// Set savings achieved.
    pub fn set_savings_achieved(&mut self, savings: f64) {
        self.savings_achieved = savings;
    }

    /// Set subscription utilization.
    pub fn set_subscription_utilization(&mut self, utilization: f64) {
        self.subscription_utilization = utilization;
    }

    /// Set real-time metrics from log parsing.
    pub fn set_realtime(&mut self, realtime: RealtimeMetrics) {
        self.realtime = realtime;
    }

    /// Check if we have optimization recommendations.
    pub fn has_recommendations(&self) -> bool {
        !self.recommendations.is_empty()
    }

    /// Get the top recommendation by priority.
    pub fn top_recommendation(&self) -> Option<&OptimizationRecommendation> {
        self.recommendations.first()
    }

    /// Get today's total cost.
    pub fn today_total(&self) -> f64 {
        self.today.as_ref().map(|t| t.total_cost_usd).unwrap_or(0.0)
    }

    /// Get today's call count.
    pub fn today_calls(&self) -> i64 {
        self.today.as_ref().map(|t| t.call_count).unwrap_or(0)
    }

    /// Get today's token count.
    pub fn today_tokens(&self) -> i64 {
        self.today.as_ref().map(|t| t.total_tokens).unwrap_or(0)
    }

    /// Get monthly budget usage percentage.
    pub fn monthly_usage_pct(&self) -> f64 {
        if self.budget.monthly_limit > 0.0 {
            (self.monthly_total / self.budget.monthly_limit) * 100.0
        } else {
            0.0
        }
    }

    /// Get daily budget usage percentage.
    pub fn daily_usage_pct(&self) -> f64 {
        let limit = self.budget.effective_daily_limit();
        if limit > 0.0 {
            (self.today_total() / limit) * 100.0
        } else {
            0.0
        }
    }

    /// Get monthly budget alert level.
    pub fn monthly_alert(&self) -> BudgetAlertLevel {
        self.budget.get_monthly_alert(self.monthly_total)
    }

    /// Get daily budget alert level.
    pub fn daily_alert(&self) -> BudgetAlertLevel {
        self.budget.get_daily_alert(self.today_total())
    }

    /// Check if we have any data.
    pub fn has_data(&self) -> bool {
        self.today.is_some() || self.monthly_total > 0.0 || !self.monthly_by_model.is_empty() || self.realtime.has_data()
    }
}

/// Renders a sparkline from a series of values.
pub fn render_sparkline(values: &[f64], width: usize) -> String {
    if values.is_empty() {
        return " ".repeat(width);
    }

    let blocks = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = values.iter().cloned().fold(0.0_f64, f64::max);
    let min = values.iter().cloned().fold(f64::MAX, f64::min);
    let range = max - min;

    // Sample values to fit width
    let step = values.len() as f64 / width as f64;
    let mut result = String::with_capacity(width);

    for i in 0..width {
        let idx = ((i as f64) * step).min(values.len() as f64 - 1.0) as usize;
        let val = values[idx];

        let normalized = if range > 0.0 {
            ((val - min) / range).clamp(0.0, 1.0)
        } else {
            0.5
        };

        let block_idx = ((normalized * 7.0).round() as usize).min(7);
        result.push(blocks[block_idx]);
    }

    result
}

/// Renders a horizontal bar chart.
pub fn render_bar(
    value: f64,
    max: f64,
    width: usize,
    filled_char: char,
    empty_char: char,
) -> String {
    let pct = if max > 0.0 {
        (value / max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = (pct * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);

    format!(
        "{}{}",
        filled_char.to_string().repeat(filled),
        empty_char.to_string().repeat(empty)
    )
}

/// Format a dollar amount for display.
pub fn format_usd(amount: f64) -> String {
    if amount >= 1000.0 {
        format!("${:.2}K", amount / 1000.0)
    } else if amount >= 100.0 {
        format!("${:.1}", amount)
    } else if amount >= 10.0 {
        format!("${:.2}", amount)
    } else if amount >= 1.0 {
        format!("${:.3}", amount)
    } else {
        format!("${:.4}", amount)
    }
}

/// Format token count for display.
pub fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        format!("{}", tokens)
    }
}

/// Truncate model name for display.
pub fn truncate_model_name(model: &str, max_len: usize) -> String {
    // Common abbreviations
    let name = model
        .replace("claude-", "")
        .replace("opus-4-5-20251101", "opus-4.5")
        .replace("opus-4-6", "opus-4.6")
        .replace("sonnet-4-5-20250929", "sonnet-4.5")
        .replace("haiku-4-5-20251001", "haiku-4.5")
        .replace("glm-4.7", "GLM-4.7")
        .replace("deepseek-", "ds-");

    if name.len() <= max_len {
        name
    } else if max_len > 2 {
        format!("{}…", &name[..max_len - 1])
    } else {
        name[..max_len].to_string()
    }
}

/// Cost analytics panel widget.
pub struct CostPanel<'a> {
    data: &'a CostPanelData,
    focused: bool,
}

impl<'a> CostPanel<'a> {
    /// Create a new cost panel.
    pub fn new(data: &'a CostPanelData) -> Self {
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

    /// Render the summary section.
    fn render_summary(&self, area: Rect, buf: &mut Buffer) {
        let mut lines = Vec::new();

        // Today's costs with alert
        let daily_alert = self.data.daily_alert();
        let today_line = Line::from(vec![
            Span::raw("Today: "),
            Span::styled(
                format_usd(self.data.today_total()),
                Style::default()
                    .fg(daily_alert.color())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" / "),
            Span::styled(
                format_usd(self.data.budget.effective_daily_limit()),
                Style::default().fg(Color::Gray),
            ),
            Span::raw(" "),
            Span::styled(
                format!("{} {}", daily_alert.icon(), daily_alert.description()),
                Style::default().fg(daily_alert.color()),
            ),
        ]);
        lines.push(today_line);

        // Monthly costs with alert
        let monthly_alert = self.data.monthly_alert();
        let monthly_line = Line::from(vec![
            Span::raw("Month: "),
            Span::styled(
                format_usd(self.data.monthly_total),
                Style::default()
                    .fg(monthly_alert.color())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" / "),
            Span::styled(
                format_usd(self.data.budget.monthly_limit),
                Style::default().fg(Color::Gray),
            ),
            Span::raw(" "),
            Span::styled(
                format!("{} {}", monthly_alert.icon(), monthly_alert.description()),
                Style::default().fg(monthly_alert.color()),
            ),
        ]);
        lines.push(monthly_line);

        // Budget progress bar
        let pct = self.data.monthly_usage_pct().min(100.0);
        let bar = render_bar(pct, 100.0, 20, '█', '░');
        let bar_color = monthly_alert.color();
        lines.push(Line::from(vec![
            Span::raw("Budget: "),
            Span::styled(bar, Style::default().fg(bar_color)),
            Span::raw(format!(" {:.1}%", self.data.monthly_usage_pct())),
        ]));

        // Projected costs
        if let Some(ref proj) = self.data.projected {
            lines.push(Line::from(""));
            let proj_alert = BudgetAlertLevel::from_percentage(
                (proj.projected_total / self.data.budget.monthly_limit) * 100.0,
            );
            lines.push(Line::from(vec![
                Span::raw("Projected: "),
                Span::styled(
                    format_usd(proj.projected_total),
                    Style::default().fg(proj_alert.color()),
                ),
                Span::raw(format!(" ({} days left)", proj.days_remaining)),
            ]));
            lines.push(Line::from(vec![
                Span::raw("Daily Rate: "),
                Span::styled(
                    format!("{}/day", format_usd(proj.daily_rate)),
                    Style::default().fg(Color::Cyan),
                ),
            ]));
        }

        // Stats
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("Calls: "),
            Span::styled(
                format!("{}", self.data.today_calls()),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw("  Tokens: "),
            Span::styled(
                format_tokens(self.data.today_tokens()),
                Style::default().fg(Color::Cyan),
            ),
        ]));

        // Savings section
        if self.data.savings_achieved > 0.0 || self.data.potential_savings > 0.0 {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Savings: ", Style::default().fg(Color::Green)),
                Span::styled(
                    format_usd(self.data.savings_achieved),
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" achieved"),
            ]));

            if self.data.potential_savings > 0.01 {
                lines.push(Line::from(vec![
                    Span::raw("         "),
                    Span::styled(
                        format_usd(self.data.potential_savings),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw(" potential"),
                ]));
            }
        }

        // Subscription utilization
        if self.data.subscription_utilization > 0.0 {
            lines.push(Line::from(vec![
                Span::raw("Sub Util: "),
                Span::styled(
                    format!("{:.0}%", self.data.subscription_utilization),
                    Style::default().fg(if self.data.subscription_utilization >= 90.0 {
                        Color::Green
                    } else if self.data.subscription_utilization >= 70.0 {
                        Color::Yellow
                    } else {
                        Color::LightRed
                    }),
                ),
            ]));
        }

        // Real-time metrics from log parsing
        if self.data.realtime.has_data() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("⚡ Live", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::raw(" "),
                Span::styled(
                    format!("{} calls", self.data.realtime.total_calls),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(" | "),
                Span::styled(
                    format_usd(self.data.realtime.total_cost),
                    Style::default().fg(Color::Green),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::raw("   Tokens: "),
                Span::styled(
                    format_tokens(self.data.realtime.total_tokens()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw("  Avg: "),
                Span::styled(
                    format_usd(self.data.realtime.avg_cost_per_call()),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("/call"),
            ]));
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }

    /// Render the model breakdown table.
    fn render_model_table(&self, area: Rect, buf: &mut Buffer) {
        // Get today's model breakdown or monthly
        let models: Vec<&CostBreakdown> = if let Some(ref today) = self.data.today {
            today.by_model.iter().collect()
        } else {
            Vec::new()
        };

        if models.is_empty() {
            let msg =
                Paragraph::new("No model data available").style(Style::default().fg(Color::Gray));
            msg.render(area, buf);
            return;
        }

        // Find max cost for scaling bars
        let max_cost = models
            .iter()
            .map(|m| m.total_cost_usd)
            .fold(0.0_f64, f64::max);

        // Build table rows
        let rows: Vec<Row> = models
            .iter()
            .take(6) // Limit to 6 models
            .map(|m| {
                let bar = render_bar(m.total_cost_usd, max_cost, 8, '▓', '░');
                Row::new(vec![
                    truncate_model_name(&m.model, 12),
                    format!("{:>3}", m.call_count),
                    format_tokens(m.input_tokens + m.output_tokens),
                    format_usd(m.total_cost_usd),
                    bar,
                ])
            })
            .collect();

        let header = Row::new(vec!["Model", "#", "Tokens", "Cost", ""])
            .style(
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::BOLD),
            )
            .bottom_margin(0);

        let widths = [
            Constraint::Length(12),
            Constraint::Length(4),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(8),
        ];

        let table = Table::new(rows, widths).header(header).column_spacing(1);

        ratatui::widgets::Widget::render(table, area, buf);
    }

    /// Render the sparkline trend.
    fn render_trend(&self, area: Rect, buf: &mut Buffer) {
        if self.data.daily_trend.is_empty() {
            let msg = Paragraph::new("No trend data").style(Style::default().fg(Color::Gray));
            msg.render(area, buf);
            return;
        }

        let values: Vec<f64> = self.data.daily_trend.iter().map(|(_, v)| *v).collect();
        let sparkline = render_sparkline(&values, area.width.saturating_sub(10) as usize);

        // Get date range
        let first_date = self.data.daily_trend.first().map(|(d, _)| *d);
        let last_date = self.data.daily_trend.last().map(|(d, _)| *d);

        let mut lines = Vec::new();

        if let (Some(first), Some(last)) = (first_date, last_date) {
            lines.push(Line::from(vec![
                Span::raw("Trend "),
                Span::styled(
                    format!("{} - {}", first.format("%m/%d"), last.format("%m/%d")),
                    Style::default().fg(Color::Gray),
                ),
            ]));
        } else {
            lines.push(Line::from("Trend"));
        }

        lines.push(Line::from(Span::styled(
            sparkline,
            Style::default().fg(Color::Cyan),
        )));

        // Min/max labels
        let min_val = values.iter().cloned().fold(f64::MAX, f64::min);
        let max_val = values.iter().cloned().fold(0.0_f64, f64::max);
        lines.push(Line::from(vec![
            Span::styled(format_usd(min_val), Style::default().fg(Color::Green)),
            Span::raw(" - "),
            Span::styled(format_usd(max_val), Style::default().fg(Color::Red)),
        ]));

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }

    /// Render optimization recommendations.
    fn render_recommendations(&self, area: Rect, buf: &mut Buffer) {
        if self.data.recommendations.is_empty() {
            let msg =
                Paragraph::new("No recommendations").style(Style::default().fg(Color::Gray));
            msg.render(area, buf);
            return;
        }

        let mut lines = Vec::new();

        // Show top 3 recommendations
        for rec in self.data.recommendations.iter().take(3) {
            let (icon, color) = match rec.recommendation_type {
                forge_cost::RecommendationType::AccelerateSubscription => ("🚀", Color::Cyan),
                forge_cost::RecommendationType::MaxOutSubscription => ("⚡", Color::Yellow),
                forge_cost::RecommendationType::ModelDowngrade => ("📉", Color::Blue),
                forge_cost::RecommendationType::EnableCaching => ("💾", Color::Green),
                forge_cost::RecommendationType::BatchTasks => ("📦", Color::Magenta),
                forge_cost::RecommendationType::OffPeakScheduling => ("🌙", Color::Blue),
                forge_cost::RecommendationType::SubscriptionDepleted => ("❌", Color::Red),
            };

            // Truncate title if too long
            let title = if rec.title.len() > 35 {
                format!("{}…", &rec.title[..32])
            } else {
                rec.title.clone()
            };

            lines.push(Line::from(vec![
                Span::styled(icon, Style::default().fg(color)),
                Span::raw(" "),
                Span::styled(title, Style::default().fg(Color::White)),
            ]));

            // Show savings if significant
            if rec.estimated_savings > 0.01 {
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(
                        format!("Save {}", format_usd(rec.estimated_savings)),
                        Style::default().fg(Color::Green),
                    ),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}

impl Widget for CostPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Border style based on focus
        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let title_style = if self.focused {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(" Cost Analytics ", title_style));

        let inner = block.inner(area);
        block.render(area, buf);

        // Handle special states
        if self.data.is_loading {
            let loading =
                Paragraph::new("Loading cost data...").style(Style::default().fg(Color::Yellow));
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
                "No cost data available.\n\n\
                 Cost tracking requires:\n\
                 - forge-cost database initialized\n\
                 - Worker logs being parsed\n\n\
                 Run: forge costs init",
            )
            .style(Style::default().fg(Color::Gray));
            no_data.render(inner, buf);
            return;
        }

        // Layout: Summary | Table | Trend | Recommendations
        let rec_height = if self.data.has_recommendations() { 5 } else { 0 };
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(10),     // Summary (increased for savings)
                Constraint::Min(5),         // Model table
                Constraint::Length(4),      // Trend sparkline
                Constraint::Length(rec_height), // Recommendations
            ])
            .split(inner);

        self.render_summary(layout[0], buf);
        self.render_model_table(layout[1], buf);
        self.render_trend(layout[2], buf);
        if rec_height > 0 {
            self.render_recommendations(layout[3], buf);
        }
    }
}

/// Compact cost summary widget for overview panel.
pub struct CostSummaryCompact<'a> {
    data: &'a CostPanelData,
}

impl<'a> CostSummaryCompact<'a> {
    /// Create a new compact cost summary.
    pub fn new(data: &'a CostPanelData) -> Self {
        Self { data }
    }
}

impl Widget for CostSummaryCompact<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.data.is_loading {
            let loading = Paragraph::new("Loading...").style(Style::default().fg(Color::Yellow));
            loading.render(area, buf);
            return;
        }

        if !self.data.has_data() {
            let no_data = Paragraph::new("No cost data").style(Style::default().fg(Color::Gray));
            no_data.render(area, buf);
            return;
        }

        let daily_alert = self.data.daily_alert();
        let monthly_alert = self.data.monthly_alert();

        let mut lines = Vec::new();

        // Today's costs
        lines.push(Line::from(vec![Span::styled(
            format!("{} Today:", daily_alert.icon()),
            Style::default().fg(daily_alert.color()),
        )]));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format_usd(self.data.today_total()),
                Style::default()
                    .fg(daily_alert.color())
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(""));

        // Monthly costs
        lines.push(Line::from(vec![Span::styled(
            format!("{} Month:", monthly_alert.icon()),
            Style::default().fg(monthly_alert.color()),
        )]));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format_usd(self.data.monthly_total),
                Style::default()
                    .fg(monthly_alert.color())
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        // Budget bar
        let pct = self.data.monthly_usage_pct().min(100.0);
        let bar = render_bar(pct, 100.0, 12, '█', '░');
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(bar, Style::default().fg(monthly_alert.color())),
        ]));

        // Projected
        if let Some(ref proj) = self.data.projected {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("Proj: "),
                Span::styled(
                    format_usd(proj.projected_total),
                    Style::default().fg(Color::Cyan),
                ),
            ]));
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_alert_level() {
        assert_eq!(
            BudgetAlertLevel::from_percentage(50.0),
            BudgetAlertLevel::Normal
        );
        assert_eq!(
            BudgetAlertLevel::from_percentage(75.0),
            BudgetAlertLevel::Warning
        );
        assert_eq!(
            BudgetAlertLevel::from_percentage(95.0),
            BudgetAlertLevel::Critical
        );
        assert_eq!(
            BudgetAlertLevel::from_percentage(105.0),
            BudgetAlertLevel::Exceeded
        );
    }

    #[test]
    fn test_budget_config() {
        let config = BudgetConfig::new(500.0);
        assert_eq!(config.monthly_limit, 500.0);
        assert!((config.effective_daily_limit() - 500.0 / 30.0).abs() < 0.001);

        let config_daily = config.with_daily_limit(20.0);
        assert_eq!(config_daily.effective_daily_limit(), 20.0);
    }

    #[test]
    fn test_render_sparkline() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let sparkline = render_sparkline(&values, 5);
        // Use chars().count() since sparkline uses multi-byte unicode characters
        assert_eq!(sparkline.chars().count(), 5);
        assert!(sparkline.contains('▁'));
        assert!(sparkline.contains('█'));
    }

    #[test]
    fn test_render_sparkline_empty() {
        let values: Vec<f64> = vec![];
        let sparkline = render_sparkline(&values, 10);
        // Use chars().count() since we check character count, not byte count
        assert_eq!(sparkline.chars().count(), 10);
        assert!(sparkline.trim().is_empty());
    }

    #[test]
    fn test_render_bar() {
        let bar = render_bar(50.0, 100.0, 10, '█', '░');
        assert_eq!(bar.chars().filter(|&c| c == '█').count(), 5);
        assert_eq!(bar.chars().filter(|&c| c == '░').count(), 5);
    }

    #[test]
    fn test_format_usd() {
        assert_eq!(format_usd(0.0012), "$0.0012");
        assert_eq!(format_usd(1.234), "$1.234");
        assert_eq!(format_usd(12.34), "$12.34");
        assert_eq!(format_usd(123.4), "$123.4");
        assert_eq!(format_usd(1234.5), "$1.23K");
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(1500), "1.5K");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }

    #[test]
    fn test_truncate_model_name() {
        assert_eq!(
            truncate_model_name("claude-opus-4-5-20251101", 15),
            "opus-4.5"
        );
        assert_eq!(
            truncate_model_name("claude-sonnet-4-5-20250929", 15),
            "sonnet-4.5"
        );
        assert_eq!(truncate_model_name("glm-4.7", 10), "GLM-4.7");
    }

    #[test]
    fn test_cost_panel_data() {
        let mut data = CostPanelData::new();
        assert!(!data.has_data());

        data.monthly_total = 100.0;
        assert!(data.has_data());

        data.set_budget(BudgetConfig::new(500.0));
        assert!((data.monthly_usage_pct() - 20.0).abs() < 0.001);
        assert_eq!(data.monthly_alert(), BudgetAlertLevel::Normal);
    }

    #[test]
    fn test_cost_panel_data_with_today() {
        let mut data = CostPanelData::new();
        let today = DailyCost {
            date: Utc::now().date_naive(),
            total_cost_usd: 15.0,
            call_count: 50,
            total_tokens: 100000,
            by_model: vec![],
        };
        data.set_today(today);
        data.set_budget(BudgetConfig::new(500.0));

        assert!(data.has_data());
        assert_eq!(data.today_total(), 15.0);
        assert_eq!(data.today_calls(), 50);
        assert_eq!(data.today_tokens(), 100000);
    }
}
