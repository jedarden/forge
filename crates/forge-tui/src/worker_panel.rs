//! Worker Panel widget with color-coded health indicators.
//!
//! This module provides a rich widget for displaying worker information
//! with proper color-coded health status indicators.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::data::WorkerData;
use forge_worker::health::{HealthLevel, WorkerHealthStatus};

/// A worker panel widget with color-coded health indicators.
pub struct WorkerPanel<'a> {
    data: &'a WorkerData,
    focused: bool,
}

impl<'a> WorkerPanel<'a> {
    /// Create a new worker panel.
    pub fn new(data: &'a WorkerData) -> Self {
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

    /// Get the color for a health level.
    fn health_color(health: &WorkerHealthStatus) -> Color {
        match health.health_level() {
            HealthLevel::Healthy => Color::Green,
            HealthLevel::Degraded => Color::Yellow,
            HealthLevel::Unhealthy => Color::Red,
        }
    }

    /// Get styled health indicator spans.
    fn health_indicator_spans(health: Option<&WorkerHealthStatus>) -> Vec<Span<'static>> {
        match health {
            Some(h) => {
                let color = Self::health_color(h);
                let indicator = h.health_indicator();
                vec![
                    Span::styled(indicator, Style::default().fg(color).add_modifier(Modifier::BOLD)),
                ]
            }
            None => vec![Span::styled("?", Style::default().fg(Color::DarkGray))],
        }
    }

    /// Build the content lines for the panel.
    fn build_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        if !self.data.is_loaded() {
            lines.push(Line::from(Span::styled(
                "Loading worker data...",
                Style::default().fg(Color::DarkGray),
            )));
            return lines;
        }

        if !self.data.has_any_workers() {
            lines.push(Line::from(Span::styled(
                "No workers found.\n\n\
                 Workers will appear here when they register\n\
                 status files in ~/.forge/status/\n\n\
                 [G] Spawn GLM  [S] Spawn Sonnet  [O] Spawn Opus  [K] Kill",
                Style::default().fg(Color::DarkGray),
            )));
            return lines;
        }

        // Show health summary if we have health data
        if !self.data.health_status.is_empty() {
            let (healthy, degraded, unhealthy) = self.data.health_counts();
            let total = healthy + degraded + unhealthy;

            let mut summary_spans = vec![Span::raw("Health: ")];

            // Healthy count (green)
            summary_spans.push(Span::styled(
                format!("{} ", healthy),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ));
            summary_spans.push(Span::styled("●", Style::default().fg(Color::Green)));
            summary_spans.push(Span::raw(" | "));

            // Degraded count (yellow)
            summary_spans.push(Span::styled(
                format!("{} ", degraded),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
            summary_spans.push(Span::styled("◐", Style::default().fg(Color::Yellow)));
            summary_spans.push(Span::raw(" | "));

            // Unhealthy count (red)
            summary_spans.push(Span::styled(
                format!("{} ", unhealthy),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));
            summary_spans.push(Span::styled("○", Style::default().fg(Color::Red)));
            summary_spans.push(Span::raw(format!(" | {} total", total)));

            lines.push(Line::from(summary_spans));
            lines.push(Line::raw(""));
        }

        // Table header
        lines.push(Line::from(vec![
            Span::styled("┌───┬─────────────────┬──────────┬──────────┬─────────────┐",
                Style::default().fg(Color::DarkGray)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("│ ", Style::default().fg(Color::DarkGray)),
            Span::styled("H", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled("Worker ID", Style::default().fg(Color::White)),
            Span::styled("       │ ", Style::default().fg(Color::DarkGray)),
            Span::styled("Model", Style::default().fg(Color::White)),
            Span::styled("    │ ", Style::default().fg(Color::DarkGray)),
            Span::styled("Status", Style::default().fg(Color::White)),
            Span::styled("   │ ", Style::default().fg(Color::DarkGray)),
            Span::styled("Task", Style::default().fg(Color::White)),
            Span::styled("        │", Style::default().fg(Color::DarkGray)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("├───┼─────────────────┼──────────┼──────────┼─────────────┤",
                Style::default().fg(Color::DarkGray)),
        ]));

        // Sort workers by ID
        let mut workers: Vec<_> = self.data.workers.values().collect();
        workers.sort_by(|a, b| a.worker_id.cmp(&b.worker_id));

        // Show up to 10 workers
        for worker in workers.iter().take(10) {
            let health = self.data.get_health(&worker.worker_id);
            let health_spans = Self::health_indicator_spans(health);

            let worker_id = truncate_string(&worker.worker_id, 15);
            let model = format_model_name_short(&worker.model);
            let status = format_status(&worker.status);
            let task = worker
                .current_task
                .as_deref()
                .map(|t| truncate_string(t, 11))
                .unwrap_or_else(|| "-".to_string());

            let mut row_spans = vec![
                Span::styled("│ ", Style::default().fg(Color::DarkGray)),
            ];
            row_spans.extend(health_spans);
            row_spans.extend(vec![
                Span::styled(format!(" │ {:<15} │ {:<8} │ {:<8} │ {:<11} │",
                    worker_id, model, status, task),
                    Style::default().fg(Color::White)),
            ]);

            lines.push(Line::from(row_spans));
        }

        // Table footer
        lines.push(Line::from(vec![
            Span::styled("└───┴─────────────────┴──────────┴──────────┴─────────────┘",
                Style::default().fg(Color::DarkGray)),
        ]));

        // Show count if more than 10
        if self.data.workers.len() > 10 {
            lines.push(Line::from(Span::styled(
                format!("\n... and {} more workers", self.data.workers.len() - 10),
                Style::default().fg(Color::DarkGray),
            )));
        }

        // Hotkey hints
        lines.push(Line::from(Span::styled(
            "\n[G] Spawn GLM  [S] Spawn Sonnet  [O] Spawn Opus  [K] Kill",
            Style::default().fg(Color::DarkGray),
        )));

        lines
    }
}

impl Widget for WorkerPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
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

        let lines = self.build_lines();
        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(Span::styled(" Worker Pool Management ", title_style)),
        );

        paragraph.render(area, buf);
    }
}

/// Format model name for table (shorter version).
fn format_model_name_short(model: &str) -> String {
    let lower = model.to_lowercase();
    let name = if lower.contains("glm-47") || lower.contains("glm47") || lower.contains("glm-4.7") {
        "GLM-4.7"
    } else if lower.contains("sonnet") {
        "Sonnet"
    } else if lower.contains("opus") {
        "Opus"
    } else if lower.contains("haiku") {
        "Haiku"
    } else if model.is_empty() {
        "Unknown"
    } else {
        model
    };
    truncate_string(name, 8)
}

/// Format worker status for display.
fn format_status(status: &forge_core::types::WorkerStatus) -> String {
    match status {
        forge_core::types::WorkerStatus::Active => "Active".to_string(),
        forge_core::types::WorkerStatus::Idle => "Idle".to_string(),
        forge_core::types::WorkerStatus::Starting => "Starting".to_string(),
        forge_core::types::WorkerStatus::Failed => "Failed".to_string(),
        forge_core::types::WorkerStatus::Stopped => "Stopped".to_string(),
        forge_core::types::WorkerStatus::Error => "Error".to_string(),
    }
}

/// Truncate a string to a maximum length.
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len > 2 {
        format!("{}…", &s[..max_len - 1])
    } else {
        s[..max_len].to_string()
    }
}

/// Format a compact health summary for narrow displays.
pub fn format_health_summary_narrow(data: &WorkerData) -> String {
    if !data.is_loaded() {
        return "Loading...".to_string();
    }

    if !data.has_any_workers() {
        return "No workers".to_string();
    }

    let (healthy, degraded, unhealthy) = data.health_counts();
    let total = data.workers.len();

    // Compact format for narrow displays
    format!(
        "{} workers: {}● {}◐ {}○",
        total, healthy, degraded, unhealthy
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("hello", 10), "hello");
        assert_eq!(truncate_string("hello world", 5), "hell…");
        assert_eq!(truncate_string("ab", 2), "ab");
    }

    #[test]
    fn test_format_status() {
        use forge_core::types::WorkerStatus;
        assert_eq!(format_status(&WorkerStatus::Active), "Active");
        assert_eq!(format_status(&WorkerStatus::Idle), "Idle");
        assert_eq!(format_status(&WorkerStatus::Failed), "Failed");
    }

    #[test]
    fn test_format_model_name_short() {
        assert_eq!(format_model_name_short("claude-code-glm-47"), "GLM-4.7");
        assert_eq!(format_model_name_short("sonnet-4.5"), "Sonnet");
        assert_eq!(format_model_name_short("opus"), "Opus");
        assert_eq!(format_model_name_short("haiku"), "Haiku");
        assert_eq!(format_model_name_short(""), "Unknown");
    }
}
