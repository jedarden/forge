//! Performance metrics visualization panel for the FORGE TUI.
//!
//! This module provides rich performance visualization with:
//! - Summary statistics (tasks completed, avg duration, success rate)
//! - Tasks per hour histogram
//! - Model efficiency comparison table
//! - Trend sparklines
//! - Color-coded performance indicators

use chrono::{DateTime, Utc};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table, Widget},
};

use forge_cost::{DailyStat, HourlyStat, ModelPerformance, WorkerEfficiency};

/// Performance data prepared for TUI display.
#[derive(Debug, Default)]
pub struct MetricsPanelData {
    /// Today's performance statistics
    pub today: Option<DailyStat>,
    /// Hourly statistics for the last 24 hours
    pub hourly_stats: Vec<HourlyStat>,
    /// Model performance comparison
    pub model_performance: Vec<ModelPerformance>,
    /// Worker efficiency ranking
    pub worker_efficiency: Vec<WorkerEfficiency>,
    /// Last update timestamp
    pub last_update: Option<DateTime<Utc>>,
    /// Whether data is loading
    pub is_loading: bool,
    /// Error message if any
    pub error: Option<String>,
}

impl MetricsPanelData {
    /// Create new empty metrics panel data.
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

    /// Update with today's performance data.
    pub fn set_today(&mut self, today: DailyStat) {
        self.today = Some(today);
        self.last_update = Some(Utc::now());
        self.is_loading = false;
    }

    /// Update with hourly statistics.
    pub fn set_hourly_stats(&mut self, stats: Vec<HourlyStat>) {
        self.hourly_stats = stats;
    }

    /// Update with model performance data.
    pub fn set_model_performance(&mut self, models: Vec<ModelPerformance>) {
        self.model_performance = models;
    }

    /// Update with worker efficiency data.
    pub fn set_worker_efficiency(&mut self, workers: Vec<WorkerEfficiency>) {
        self.worker_efficiency = workers;
    }

    /// Get today's completed tasks.
    pub fn today_completed(&self) -> i64 {
        self.today.as_ref()
            .map(|t| t.tasks_completed)
            .unwrap_or(0)
    }

    /// Get today's failed tasks.
    pub fn today_failed(&self) -> i64 {
        self.today.as_ref()
            .map(|t| t.tasks_failed)
            .unwrap_or(0)
    }

    /// Get today's total tasks.
    pub fn today_total(&self) -> i64 {
        self.today_completed() + self.today_failed()
    }

    /// Get today's success rate.
    pub fn today_success_rate(&self) -> f64 {
        self.today.as_ref()
            .map(|t| t.success_rate * 100.0)
            .unwrap_or(100.0)
    }

    /// Get today's average cost per task.
    pub fn today_avg_cost(&self) -> f64 {
        self.today.as_ref()
            .map(|t| t.avg_cost_per_task)
            .unwrap_or(0.0)
    }

    /// Get tasks per hour for today.
    pub fn tasks_per_hour(&self) -> f64 {
        self.today.as_ref()
            .map(|t| {
                let total_tasks = t.tasks_completed + t.tasks_failed;
                if total_tasks == 0 {
                    0.0
                } else {
                    // Assume 24 hours for daily stats
                    total_tasks as f64 / 24.0
                }
            })
            .unwrap_or(0.0)
    }

    /// Check if we have any data.
    pub fn has_data(&self) -> bool {
        self.today.is_some()
            || !self.hourly_stats.is_empty()
            || !self.model_performance.is_empty()
            || !self.worker_efficiency.is_empty()
    }
}

/// Renders a sparkline from a series of values.
pub fn render_sparkline(values: &[i64], width: usize) -> String {
    if values.is_empty() {
        return " ".repeat(width);
    }

    let blocks = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = values.iter().cloned().fold(0i64, i64::max);
    let min = values.iter().cloned().fold(i64::MAX, i64::min);
    let range = (max - min) as f64;

    // Sample values to fit width
    let step = values.len() as f64 / width as f64;
    let mut result = String::with_capacity(width);

    for i in 0..width {
        let idx = ((i as f64) * step).min(values.len() as f64 - 1.0) as usize;
        let val = values[idx];

        let normalized = if range > 0.0 {
            ((val - min) as f64 / range).clamp(0.0, 1.0)
        } else {
            0.5
        };

        let block_idx = ((normalized * 7.0).round() as usize).min(7);
        result.push(blocks[block_idx]);
    }

    result
}

/// Renders a horizontal bar chart.
pub fn render_bar(value: f64, max: f64, width: usize, filled_char: char, empty_char: char) -> String {
    let pct = if max > 0.0 { (value / max).clamp(0.0, 1.0) } else { 0.0 };
    let filled = (pct * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);

    format!(
        "{}{}",
        filled_char.to_string().repeat(filled),
        empty_char.to_string().repeat(empty)
    )
}

/// Format a duration in seconds to human-readable form.
pub fn format_duration(secs: i64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        let mins = secs / 60;
        let remaining_secs = secs % 60;
        format!("{}m {}s", mins, remaining_secs)
    } else {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        format!("{}h {}m", hours, mins)
    }
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
    } else if amount > 0.0 {
        format!("${:.4}", amount)
    } else {
        "$0.00".to_string()
    }
}

/// Truncate model name for display.
pub fn truncate_model_name(model: &str, max_len: usize) -> String {
    // Common abbreviations
    let name = model
        .replace("claude-opus-4-5-20251101", "Opus-4.5")
        .replace("claude-opus-4-6", "Opus-4.6")
        .replace("claude-sonnet-4-5-20250929", "Sonnet-4.5")
        .replace("claude-haiku-4-5-20251001", "Haiku-4.5")
        .replace("glm-4.7", "GLM-4.7")
        .replace("deepseek-", "DS-");

    if name.len() <= max_len {
        name
    } else if max_len > 2 {
        format!("{}…", &name[..max_len - 1])
    } else {
        name[..max_len].to_string()
    }
}

/// Performance metrics panel widget.
pub struct MetricsPanel<'a> {
    data: &'a MetricsPanelData,
    focused: bool,
}

impl<'a> MetricsPanel<'a> {
    /// Create a new metrics panel.
    pub fn new(data: &'a MetricsPanelData) -> Self {
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

        // Main stats row
        let completed = self.data.today_completed();
        let failed = self.data.today_failed();
        let success_rate = self.data.today_success_rate();
        let total = self.data.today_total();

        let success_color = if success_rate >= 90.0 {
            Color::Green
        } else if success_rate >= 70.0 {
            Color::Yellow
        } else {
            Color::Red
        };

        lines.push(Line::from(vec![
            Span::raw("Tasks: "),
            Span::styled(
                format!("{}", completed),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" done, "),
            Span::styled(
                format!("{}", failed),
                Style::default().fg(Color::Red),
            ),
            Span::raw(" failed of "),
            Span::styled(
                format!("{}", total),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(" total"),
        ]));

        lines.push(Line::from(vec![
            Span::raw("Success: "),
            Span::styled(
                format!("{:.1}%", success_rate),
                Style::default().fg(success_color).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::raw("Avg Cost: "),
            Span::styled(
                format_usd(self.data.today_avg_cost()),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw("/task"),
        ]));

        // Tasks per hour
        let tph = self.data.tasks_per_hour();
        lines.push(Line::from(vec![
            Span::raw("Throughput: "),
            Span::styled(
                format!("{:.1} tasks/hr", tph),
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ),
        ]));

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }

    /// Render the tasks per hour histogram.
    fn render_histogram(&self, area: Rect, buf: &mut Buffer) {
        if self.data.hourly_stats.is_empty() {
            let msg = Paragraph::new("No hourly data")
                .style(Style::default().fg(Color::Gray));
            msg.render(area, buf);
            return;
        }

        let mut lines = Vec::new();
        lines.push(Line::from(vec![
            Span::styled("Tasks/Hour", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" (last 24h)"),
        ]));
        lines.push(Line::from(""));

        // Get tasks completed per hour
        let values: Vec<i64> = self.data.hourly_stats
            .iter()
            .map(|h| h.tasks_completed)
            .collect();

        let max_val = values.iter().cloned().fold(0i64, i64::max);

        // Render horizontal histogram with bars
        for (i, &val) in values.iter().enumerate() {
            // Only show every 4th hour label to save space
            let show_label = i % 4 == 0;
            let hour_label = if show_label {
                format!("{:>2}h", i)
            } else {
                "   ".to_string()
            };

            let bar_width = (area.width.saturating_sub(8) as usize).max(10);
            let bar = render_bar(val as f64, max_val as f64, bar_width, '█', '░');

            let bar_color = if val == 0 {
                Color::DarkGray
            } else if val >= (max_val / 2) {
                Color::Green
            } else {
                Color::Cyan
            };

            lines.push(Line::from(vec![
                Span::raw(hour_label),
                Span::raw(" "),
                Span::styled(bar, Style::default().fg(bar_color)),
                Span::styled(
                    format!(" {:>3}", val),
                    Style::default().fg(Color::Gray),
                ),
            ]));
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }

    /// Render the model efficiency comparison table.
    fn render_model_comparison(&self, area: Rect, buf: &mut Buffer) {
        if self.data.model_performance.is_empty() {
            let msg = Paragraph::new("No model data")
                .style(Style::default().fg(Color::Gray));
            msg.render(area, buf);
            return;
        }

        // Sort by tasks completed (descending)
        let mut models = self.data.model_performance.clone();
        models.sort_by(|a, b| b.tasks_completed.cmp(&a.tasks_completed));

        let rows: Vec<Row> = models
            .iter()
            .take(6)
            .map(|m| {
                let success_pct = (m.success_rate * 100.0) as i64;
                let success_color = if m.success_rate >= 0.9 {
                    Color::Green
                } else if m.success_rate >= 0.7 {
                    Color::Yellow
                } else {
                    Color::Red
                };

                Row::new(vec![
                    truncate_model_name(&m.model, 12),
                    format!("{}", m.tasks_completed),
                    format!("{:.0}%", success_pct),
                    format_usd(m.avg_cost_per_task),
                ])
            })
            .collect();

        let header = Row::new(vec!["Model", "Done", "Success", "Avg Cost"])
            .style(Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD))
            .bottom_margin(0);

        let widths = [
            Constraint::Length(12),
            Constraint::Length(6),
            Constraint::Length(7),
            Constraint::Length(8),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .column_spacing(1);

        ratatui::widgets::Widget::render(table, area, buf);
    }

    /// Render the trend sparkline.
    fn render_trend(&self, area: Rect, buf: &mut Buffer) {
        if self.data.hourly_stats.is_empty() {
            let msg = Paragraph::new("No trend data")
                .style(Style::default().fg(Color::Gray));
            msg.render(area, buf);
            return;
        }

        let values: Vec<i64> = self.data.hourly_stats
            .iter()
            .map(|h| h.tasks_completed)
            .collect();

        let sparkline = render_sparkline(&values, area.width.saturating_sub(10) as usize);

        let mut lines = Vec::new();
        lines.push(Line::from(vec![
            Span::styled("Task Trend", Style::default().fg(Color::Cyan)),
            Span::raw(" (24h)"),
        ]));
        lines.push(Line::from(Span::styled(
            sparkline,
            Style::default().fg(Color::Green),
        )));

        let min_val = values.iter().cloned().fold(i64::MAX, i64::min);
        let max_val = values.iter().cloned().fold(0i64, i64::max);
        lines.push(Line::from(vec![
            Span::styled(format!("{}", min_val), Style::default().fg(Color::DarkGray)),
            Span::raw(" - "),
            Span::styled(format!("{}", max_val), Style::default().fg(Color::White)),
        ]));

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}

impl Widget for MetricsPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Border style based on focus
        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let title_style = if self.focused {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(" Performance Metrics ", title_style));

        let inner = block.inner(area);
        block.render(area, buf);

        // Handle special states
        if self.data.is_loading {
            let loading = Paragraph::new("Loading metrics...")
                .style(Style::default().fg(Color::Yellow));
            loading.render(inner, buf);
            return;
        }

        if let Some(ref error) = self.data.error {
            let err = Paragraph::new(format!("Error: {}", error))
                .style(Style::default().fg(Color::Red));
            err.render(inner, buf);
            return;
        }

        if !self.data.has_data() {
            let no_data = Paragraph::new(
                "No performance data available.\n\n\
                 Metrics tracking requires:\n\
                 - forge-cost database initialized\n\
                 - Worker activity being logged\n\n\
                 Run: forge costs init"
            )
            .style(Style::default().fg(Color::Gray));
            no_data.render(inner, buf);
            return;
        }

        // Layout: Summary | Histogram | Model Comparison | Trend
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Summary
                Constraint::Min(8),     // Histogram
                Constraint::Min(6),     // Model comparison
                Constraint::Length(3),  // Trend
            ])
            .split(inner);

        self.render_summary(layout[0], buf);
        self.render_histogram(layout[1], buf);
        self.render_model_comparison(layout[2], buf);
        self.render_trend(layout[3], buf);
    }
}

/// Compact metrics summary widget for overview panel.
pub struct MetricsSummaryCompact<'a> {
    data: &'a MetricsPanelData,
}

impl<'a> MetricsSummaryCompact<'a> {
    /// Create a new compact metrics summary.
    pub fn new(data: &'a MetricsPanelData) -> Self {
        Self { data }
    }
}

impl Widget for MetricsSummaryCompact<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.data.is_loading {
            let loading = Paragraph::new("Loading...")
                .style(Style::default().fg(Color::Yellow));
            loading.render(area, buf);
            return;
        }

        if !self.data.has_data() {
            let no_data = Paragraph::new("No metrics")
                .style(Style::default().fg(Color::Gray));
            no_data.render(area, buf);
            return;
        }

        let completed = self.data.today_completed();
        let failed = self.data.today_failed();
        let success_rate = self.data.today_success_rate();
        let tph = self.data.tasks_per_hour();

        let success_color = if success_rate >= 90.0 {
            Color::Green
        } else if success_rate >= 70.0 {
            Color::Yellow
        } else {
            Color::Red
        };

        let mut lines = Vec::new();

        // Tasks completed
        lines.push(Line::from(vec![
            Span::styled("✓", Style::default().fg(Color::Green)),
            Span::raw(" Done: "),
            Span::styled(
                format!("{}", completed),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
        ]));

        // Failed tasks
        if failed > 0 {
            lines.push(Line::from(vec![
                Span::styled("✗", Style::default().fg(Color::Red)),
                Span::raw(" Failed: "),
                Span::styled(
                    format!("{}", failed),
                    Style::default().fg(Color::Red),
                ),
            ]));
        }

        // Success rate
        lines.push(Line::from(vec![
            Span::raw("Rate: "),
            Span::styled(
                format!("{:.0}%", success_rate),
                Style::default().fg(success_color).add_modifier(Modifier::BOLD),
            ),
        ]));

        // Throughput
        lines.push(Line::from(vec![
            Span::raw("Rate: "),
            Span::styled(
                format!("{:.0}/hr", tph),
                Style::default().fg(Color::Cyan),
            ),
        ]));

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn test_render_sparkline() {
        let values = vec![1, 2, 3, 4, 5];
        let sparkline = render_sparkline(&values, 5);
        assert_eq!(sparkline.chars().count(), 5);
        assert!(sparkline.contains('▁'));
        assert!(sparkline.contains('█'));
    }

    #[test]
    fn test_render_sparkline_empty() {
        let values: Vec<i64> = vec![];
        let sparkline = render_sparkline(&values, 10);
        assert_eq!(sparkline.chars().count(), 10);
        assert!(sparkline.trim().is_empty());
    }

    #[test]
    fn test_render_bar() {
        let bar = render_bar(5.0, 10.0, 10, '█', '░');
        assert_eq!(bar.chars().filter(|&c| c == '█').count(), 5);
        assert_eq!(bar.chars().filter(|&c| c == '░').count(), 5);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3661), "1h 1m");
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
    fn test_truncate_model_name() {
        assert_eq!(truncate_model_name("claude-opus-4-5-20251101", 15), "Opus-4.5");
        assert_eq!(truncate_model_name("claude-sonnet-4-5-20250929", 15), "Sonnet-4.5");
        assert_eq!(truncate_model_name("glm-4.7", 10), "GLM-4.7");
    }

    #[test]
    fn test_metrics_panel_data() {
        let data = MetricsPanelData::new();
        assert!(!data.has_data());
        assert_eq!(data.today_completed(), 0);
        assert_eq!(data.today_failed(), 0);
    }

    #[test]
    fn test_metrics_panel_data_with_today() {
        let mut data = MetricsPanelData::new();
        let today = DailyStat::new(chrono::Utc::now().date_naive());
        data.set_today(today);

        assert!(data.has_data());
    }
}
