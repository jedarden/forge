//! Custom widgets for the FORGE TUI.
//!
//! Provides reusable widget components for the dashboard.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

/// A progress bar widget for displaying usage metrics.
#[derive(Debug, Clone)]
pub struct ProgressBar {
    /// Current value
    value: u64,
    /// Maximum value
    max: u64,
    /// Label to display
    label: String,
    /// Color for the filled portion
    fill_color: Color,
    /// Width of the bar in characters
    width: u16,
}

impl ProgressBar {
    /// Create a new progress bar.
    pub fn new(value: u64, max: u64) -> Self {
        Self {
            value,
            max,
            label: String::new(),
            fill_color: Color::Green,
            width: 20,
        }
    }

    /// Set the label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Set the fill color.
    pub fn fill_color(mut self, color: Color) -> Self {
        self.fill_color = color;
        self
    }

    /// Set the width.
    pub fn width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }

    /// Get the color based on percentage.
    pub fn auto_color(&self) -> Color {
        let pct = if self.max > 0 {
            (self.value as f64 / self.max as f64) * 100.0
        } else {
            0.0
        };

        if pct >= 90.0 {
            Color::Red
        } else if pct >= 70.0 {
            Color::Yellow
        } else {
            Color::Green
        }
    }

    /// Render the progress bar as a string.
    pub fn render_string(&self) -> String {
        let pct = if self.max > 0 {
            (self.value as f64 / self.max as f64).min(1.0)
        } else {
            0.0
        };

        let filled = (pct * self.width as f64).round() as usize;
        let empty = self.width as usize - filled;

        let bar = format!(
            "{}{}",
            "▓".repeat(filled),
            "░".repeat(empty)
        );

        if self.label.is_empty() {
            format!("{} {:.0}%", bar, pct * 100.0)
        } else {
            format!("{}: {} {}/{} ({:.0}%)", self.label, bar, self.value, self.max, pct * 100.0)
        }
    }
}

/// A status indicator widget.
#[derive(Debug, Clone)]
pub struct StatusIndicator {
    /// The status text
    status: String,
    /// The indicator color
    color: Color,
}

impl StatusIndicator {
    /// Create a new healthy status indicator.
    pub fn healthy(text: impl Into<String>) -> Self {
        Self {
            status: text.into(),
            color: Color::Green,
        }
    }

    /// Create a new warning status indicator.
    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            status: text.into(),
            color: Color::Yellow,
        }
    }

    /// Create a new error status indicator.
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            status: text.into(),
            color: Color::Red,
        }
    }

    /// Create a new idle status indicator.
    pub fn idle(text: impl Into<String>) -> Self {
        Self {
            status: text.into(),
            color: Color::Blue,
        }
    }

    /// Render as a span.
    pub fn as_span(&self) -> Span<'_> {
        Span::styled(
            format!("● {}", self.status),
            Style::default().fg(self.color),
        )
    }
}

/// A panel widget with focus highlighting.
#[derive(Debug)]
pub struct FocusablePanel<'a> {
    /// Panel title
    title: &'a str,
    /// Panel content
    content: &'a str,
    /// Whether the panel is focused
    focused: bool,
    /// Border color when not focused
    border_color: Color,
    /// Border color when focused
    focus_color: Color,
}

impl<'a> FocusablePanel<'a> {
    /// Create a new focusable panel.
    pub fn new(title: &'a str, content: &'a str) -> Self {
        Self {
            title,
            content,
            focused: false,
            border_color: Color::DarkGray,
            focus_color: Color::Cyan,
        }
    }

    /// Set whether the panel is focused.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Set the border color when not focused.
    pub fn border_color(mut self, color: Color) -> Self {
        self.border_color = color;
        self
    }

    /// Set the focus color.
    pub fn focus_color(mut self, color: Color) -> Self {
        self.focus_color = color;
        self
    }
}

impl Widget for FocusablePanel<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let border_style = if self.focused {
            Style::default().fg(self.focus_color)
        } else {
            Style::default().fg(self.border_color)
        };

        let title_style = if self.focused {
            Style::default().fg(self.focus_color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let paragraph = Paragraph::new(self.content)
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(Span::styled(format!(" {} ", self.title), title_style)),
            );

        paragraph.render(area, buf);
    }
}

/// A hotkey hint widget for the footer.
#[derive(Debug)]
pub struct HotkeyHints {
    /// The hints to display
    hints: Vec<(char, String)>,
}

impl HotkeyHints {
    /// Create a new hotkey hints widget.
    pub fn new() -> Self {
        Self { hints: Vec::new() }
    }

    /// Add a hint.
    pub fn hint(mut self, key: char, description: impl Into<String>) -> Self {
        self.hints.push((key, description.into()));
        self
    }

    /// Render as a line of spans.
    pub fn as_line(&self) -> Line<'_> {
        let mut spans = Vec::new();
        for (key, desc) in &self.hints {
            spans.push(Span::styled(
                format!("[{}]", key),
                Style::default().fg(Color::Yellow),
            ));
            spans.push(Span::raw(format!("{} ", desc)));
        }
        Line::from(spans)
    }
}

impl Default for HotkeyHints {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_bar_render() {
        let bar = ProgressBar::new(50, 100).width(10);
        let rendered = bar.render_string();
        assert!(rendered.contains("▓▓▓▓▓░░░░░"));
        assert!(rendered.contains("50%"));
    }

    #[test]
    fn test_progress_bar_with_label() {
        let bar = ProgressBar::new(75, 100).width(10).label("Usage");
        let rendered = bar.render_string();
        assert!(rendered.contains("Usage:"));
        assert!(rendered.contains("75/100"));
    }

    #[test]
    fn test_progress_bar_zero_max() {
        let bar = ProgressBar::new(10, 0).width(10);
        let rendered = bar.render_string();
        assert!(rendered.contains("0%"));
    }

    #[test]
    fn test_progress_bar_auto_color() {
        let bar_low = ProgressBar::new(50, 100);
        assert_eq!(bar_low.auto_color(), Color::Green);

        let bar_med = ProgressBar::new(75, 100);
        assert_eq!(bar_med.auto_color(), Color::Yellow);

        let bar_high = ProgressBar::new(95, 100);
        assert_eq!(bar_high.auto_color(), Color::Red);
    }

    #[test]
    fn test_status_indicator() {
        let healthy = StatusIndicator::healthy("Active");
        assert_eq!(healthy.color, Color::Green);

        let warning = StatusIndicator::warning("Idle");
        assert_eq!(warning.color, Color::Yellow);

        let error = StatusIndicator::error("Failed");
        assert_eq!(error.color, Color::Red);
    }

    #[test]
    fn test_hotkey_hints() {
        let hints = HotkeyHints::new()
            .hint('w', "Workers")
            .hint('t', "Tasks");

        let line = hints.as_line();
        assert_eq!(line.spans.len(), 4); // 2 keys + 2 descriptions
    }
}
