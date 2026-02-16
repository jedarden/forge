//! Custom widgets for the FORGE TUI.
//!
//! Provides reusable widget components for the dashboard.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
};

/// Action type for quick actions panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickActionType {
    /// Spawn a new worker
    Spawn,
    /// Kill an existing worker
    Kill,
    /// Refresh data
    Refresh,
    /// View navigation
    View,
    /// Configuration
    Configure,
}

impl QuickActionType {
    /// Returns the color for this action type.
    pub fn color(&self) -> Color {
        match self {
            QuickActionType::Spawn => Color::Green,
            QuickActionType::Kill => Color::Red,
            QuickActionType::Refresh => Color::Cyan,
            QuickActionType::View => Color::Blue,
            QuickActionType::Configure => Color::Yellow,
        }
    }

    /// Returns the display name for this action type.
    pub fn name(&self) -> &'static str {
        match self {
            QuickActionType::Spawn => "Spawn",
            QuickActionType::Kill => "Kill",
            QuickActionType::Refresh => "Refresh",
            QuickActionType::View => "View",
            QuickActionType::Configure => "Configure",
        }
    }
}

/// A quick action item with hotkey and description.
#[derive(Debug, Clone)]
pub struct QuickAction {
    /// The hotkey character
    pub hotkey: char,
    /// Action description
    pub description: String,
    /// Action type for color coding
    pub action_type: QuickActionType,
}

impl QuickAction {
    /// Create a new quick action.
    pub fn new(hotkey: char, description: impl Into<String>, action_type: QuickActionType) -> Self {
        Self {
            hotkey,
            description: description.into(),
            action_type,
        }
    }

    /// Render as a line of spans.
    pub fn as_line(&self) -> Line<'_> {
        Line::from(vec![
            Span::styled(
                format!("[{}]", self.hotkey),
                Style::default()
                    .fg(self.action_type.color())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(&self.description, Style::default().fg(Color::White)),
        ])
    }
}

/// Quick Actions panel widget with two-column grid layout.
///
/// Displays actions in a grid with color-coding by type:
/// - Spawn (Green): G/S/O/H for GLM, Sonnet, Opus, Haiku
/// - Kill (Red): K for kill worker
/// - Refresh (Cyan): R for refresh
/// - View (Blue): W/T/A/L for Workers, Tasks, Activity, Logs
/// - Configure (Yellow): M/B/C for Menu, Budget, WorkerConfig
#[derive(Debug)]
pub struct QuickActionsPanel<'a> {
    /// Actions to display
    actions: &'a [QuickAction],
    /// Whether the panel is focused
    focused: bool,
}

impl<'a> QuickActionsPanel<'a> {
    /// Create a new quick actions panel with default actions.
    pub fn new() -> Self {
        // Create a static Vec with default actions
        // Note: This leaks memory slightly but is acceptable for a long-lived TUI
        static DEFAULT_ACTIONS: std::sync::OnceLock<Vec<QuickAction>> = std::sync::OnceLock::new();
        let actions = DEFAULT_ACTIONS.get_or_init(|| {
            vec![
                // Spawn actions (Green)
                QuickAction::new('G', "Spawn GLM", QuickActionType::Spawn),
                QuickAction::new('S', "Spawn Sonnet", QuickActionType::Spawn),
                QuickAction::new('O', "Spawn Opus", QuickActionType::Spawn),
                QuickAction::new('H', "Spawn Haiku", QuickActionType::Spawn),
                // Kill action (Red)
                QuickAction::new('K', "Kill Worker", QuickActionType::Kill),
                // Refresh action (Cyan)
                QuickAction::new('R', "Refresh", QuickActionType::Refresh),
                // View actions (Blue)
                QuickAction::new('W', "Workers View", QuickActionType::View),
                QuickAction::new('T', "Tasks View", QuickActionType::View),
                QuickAction::new('A', "Activity View", QuickActionType::View),
                QuickAction::new('L', "Logs View", QuickActionType::View),
                // Configure actions (Yellow)
                QuickAction::new('M', "Open Menu", QuickActionType::Configure),
                QuickAction::new('B', "Budget Config", QuickActionType::Configure),
                QuickAction::new('C', "Worker Config", QuickActionType::Configure),
            ]
        });
        Self {
            actions,
            focused: false,
        }
    }

    /// Create a new quick actions panel with custom actions.
    pub fn with_actions(actions: &'a [QuickAction]) -> Self {
        Self {
            actions,
            focused: false,
        }
    }

    /// Set whether the panel is focused.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Returns the default set of quick actions.
    pub fn default_actions() -> Vec<QuickAction> {
        vec![
            // Spawn actions (Green)
            QuickAction::new('G', "Spawn GLM", QuickActionType::Spawn),
            QuickAction::new('S', "Spawn Sonnet", QuickActionType::Spawn),
            QuickAction::new('O', "Spawn Opus", QuickActionType::Spawn),
            QuickAction::new('H', "Spawn Haiku", QuickActionType::Spawn),
            // Kill action (Red)
            QuickAction::new('K', "Kill Worker", QuickActionType::Kill),
            // Refresh action (Cyan)
            QuickAction::new('R', "Refresh", QuickActionType::Refresh),
            // View actions (Blue)
            QuickAction::new('W', "Workers View", QuickActionType::View),
            QuickAction::new('T', "Tasks View", QuickActionType::View),
            QuickAction::new('A', "Activity View", QuickActionType::View),
            QuickAction::new('L', "Logs View", QuickActionType::View),
            // Configure actions (Yellow)
            QuickAction::new('M', "Open Menu", QuickActionType::Configure),
            QuickAction::new('B', "Budget Config", QuickActionType::Configure),
            QuickAction::new('C', "Worker Config", QuickActionType::Configure),
        ]
    }

    /// Render actions in a two-column grid format as lines.
    pub fn render_lines(&self) -> Vec<Line<'_>> {
        let mut lines = Vec::new();

        // Split actions into two columns
        let mid = (self.actions.len() + 1) / 2;
        let left_col = &self.actions[..mid];
        let right_col = &self.actions[mid..];

        // Add section header
        lines.push(Line::from(vec![Span::styled(
            "Quick Actions",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(""));

        // Render in two columns
        for (left, right) in left_col.iter().zip(right_col.iter()) {
            let left_line = self.format_action(left);
            let right_line = self.format_action(right);

            // Combine into a single line with padding
            lines.push(Line::from(vec![
                Span::raw(format!("{:<28}", left_line)),
                Span::raw(right_line),
            ]));
        }

        // Handle odd number of actions
        if left_col.len() > right_col.len() {
            if let Some(last) = left_col.last() {
                lines.push(Line::from(self.format_action(last)));
            }
        }

        lines
    }

    /// Format a single action for display.
    fn format_action(&self, action: &QuickAction) -> String {
        let color_marker = match action.action_type {
            QuickActionType::Spawn => "●",
            QuickActionType::Kill => "●",
            QuickActionType::Refresh => "●",
            QuickActionType::View => "●",
            QuickActionType::Configure => "●",
        };

        format!(
            "[{}] {} {}",
            action.hotkey, color_marker, action.description
        )
    }
}

impl<'a> Default for QuickActionsPanel<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Widget for QuickActionsPanel<'a> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        // Enhanced focus indicator: "▶" arrow for focused (clearer than diamond), "▪" for unfocused
        let focus_icon = if self.focused { "▶" } else { "▪" };

        // Border type: Double border for focused (highly visible), Normal for unfocused
        let border_type = if self.focused {
            BorderType::Double
        } else {
            BorderType::Plain
        };

        // Border style: bright cyan with bold for focused, dim for unfocused
        let border_style = if self.focused {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // Title style: bold + underlined for focused, dim for unfocused
        let title_style = if self.focused {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::Rgb(80, 80, 80))
        };

        // Content style: normal for focused, significantly dimmed for unfocused
        let content_style = if self.focused {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Rgb(80, 80, 80))
        };

        // Build legend line
        let legend = vec![
            Span::styled(
                "● Spawn",
                Style::default().fg(QuickActionType::Spawn.color()),
            ),
            Span::raw(" "),
            Span::styled("● Kill", Style::default().fg(QuickActionType::Kill.color())),
            Span::raw(" "),
            Span::styled(
                "● Refresh",
                Style::default().fg(QuickActionType::Refresh.color()),
            ),
            Span::raw(" "),
            Span::styled("● View", Style::default().fg(QuickActionType::View.color())),
            Span::raw(" "),
            Span::styled(
                "● Config",
                Style::default().fg(QuickActionType::Configure.color()),
            ),
        ];

        let mut lines = self.render_lines();
        lines.push(Line::from(""));
        lines.push(Line::from(legend));

        let paragraph = Paragraph::new(lines)
            .style(content_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(border_type)
                    .border_style(border_style)
                    .title(Span::styled(
                        format!(" {} Quick Actions ", focus_icon),
                        title_style,
                    )),
            );

        paragraph.render(area, buf);
    }
}

/// Unicode fill style for progress bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressFillStyle {
    /// Classic block style (▓░)
    Blocks,
    /// Smooth block style (█░)
    Smooth,
    /// Fine-grained (█▓▒░)
    Fine,
    /// Minimal (━ )
    Minimal,
    /// Heavy (■□)
    Heavy,
    /// Rounded (●○)
    Rounded,
    /// Vertical (▇▆▅▄▃▂▁ )
    Vertical,
}

impl Default for ProgressFillStyle {
    fn default() -> Self {
        Self::Blocks
    }
}

impl ProgressFillStyle {
    /// Returns the (filled, empty) characters for this style.
    pub fn chars(self) -> (char, char) {
        match self {
            ProgressFillStyle::Blocks => ('▓', '░'),
            ProgressFillStyle::Smooth => ('█', '░'),
            ProgressFillStyle::Fine => ('█', '░'),
            ProgressFillStyle::Minimal => ('━', ' '),
            ProgressFillStyle::Heavy => ('■', '□'),
            ProgressFillStyle::Rounded => ('●', '○'),
            ProgressFillStyle::Vertical => ('▇', ' '),
        }
    }

    /// Returns the partial fill characters for fine-grained rendering.
    /// Returns 8 levels from empty to full.
    pub fn partial_chars(self) -> Option<[char; 8]> {
        match self {
            ProgressFillStyle::Fine => Some(['░', '▒', '▓', '█', '█', '█', '█', '█']),
            ProgressFillStyle::Vertical => Some([' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇']),
            _ => None,
        }
    }
}

/// Color mode for progress bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressColorMode {
    /// Single color for the entire bar
    Solid,
    /// Auto-color based on percentage (green < 70%, yellow < 90%, red >= 90%)
    Auto,
    /// Gradient from green to yellow to red based on fill position
    Gradient,
    /// Custom gradient between two colors
    CustomGradient(Color, Color),
}

impl Default for ProgressColorMode {
    fn default() -> Self {
        Self::Solid
    }
}

/// A progress bar widget for displaying usage metrics.
///
/// Features:
/// - Horizontal display with customizable width
/// - Color-coded by percentage (auto, solid, or gradient modes)
/// - Optional labels with value/max display
/// - Multiple Unicode fill styles (blocks, smooth, fine, minimal, heavy, rounded, vertical)
/// - Fine-grained rendering for smooth progress visualization
/// - Flicker-free rendering via Widget trait
#[derive(Debug, Clone)]
pub struct ProgressBar {
    /// Current value
    value: u64,
    /// Maximum value
    max: u64,
    /// Label to display
    label: String,
    /// Color for the filled portion (used in Solid mode)
    fill_color: Color,
    /// Width of the bar in characters
    width: u16,
    /// Fill style (Unicode characters)
    fill_style: ProgressFillStyle,
    /// Color mode (solid, auto, gradient)
    color_mode: ProgressColorMode,
    /// Whether to show the numeric value
    show_value: bool,
    /// Whether to show the percentage
    show_percent: bool,
    /// Whether to use fine-grained rendering (when supported by fill style)
    fine_grained: bool,
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
            fill_style: ProgressFillStyle::default(),
            color_mode: ProgressColorMode::Solid,
            show_value: false,
            show_percent: true,
            fine_grained: false,
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

    /// Set the fill style (Unicode characters).
    pub fn fill_style(mut self, style: ProgressFillStyle) -> Self {
        self.fill_style = style;
        self
    }

    /// Set the color mode.
    pub fn color_mode(mut self, mode: ProgressColorMode) -> Self {
        self.color_mode = mode;
        self
    }

    /// Enable auto-color mode based on percentage.
    pub fn enable_auto_color(mut self) -> Self {
        self.color_mode = ProgressColorMode::Auto;
        self
    }

    /// Enable gradient color mode.
    pub fn gradient(mut self) -> Self {
        self.color_mode = ProgressColorMode::Gradient;
        self
    }

    /// Enable custom gradient between two colors.
    pub fn custom_gradient(mut self, start: Color, end: Color) -> Self {
        self.color_mode = ProgressColorMode::CustomGradient(start, end);
        self
    }

    /// Show or hide the numeric value.
    pub fn show_value(mut self, show: bool) -> Self {
        self.show_value = show;
        self
    }

    /// Show or hide the percentage.
    pub fn show_percent(mut self, show: bool) -> Self {
        self.show_percent = show;
        self
    }

    /// Enable fine-grained rendering (for supported fill styles).
    pub fn fine_grained(mut self, enabled: bool) -> Self {
        self.fine_grained = enabled;
        self
    }

    /// Get the color based on percentage.
    pub fn auto_color(&self) -> Color {
        self.compute_auto_color()
    }

    /// Internal method to compute auto color.
    fn compute_auto_color(&self) -> Color {
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

    /// Get the color at a specific position in the bar (for gradients).
    fn color_at_position(&self, position: f64) -> Color {
        match self.color_mode {
            ProgressColorMode::Solid => self.fill_color,
            ProgressColorMode::Auto => self.compute_auto_color(),
            ProgressColorMode::Gradient => {
                // Gradient from green to yellow to red
                if position < 0.7 {
                    Color::Green
                } else if position < 0.9 {
                    Color::Yellow
                } else {
                    Color::Red
                }
            }
            ProgressColorMode::CustomGradient(start, end) => {
                // Simple interpolation - for full RGB support, we'd need more complex logic
                if position < 0.5 { start } else { end }
            }
        }
    }

    /// Render the progress bar as a string.
    pub fn render_string(&self) -> String {
        let pct = if self.max > 0 {
            (self.value as f64 / self.max as f64).min(1.0)
        } else {
            0.0
        };

        let (filled_char, empty_char) = self.fill_style.chars();

        if self.fine_grained {
            // Fine-grained rendering with 8 levels per position
            if let Some(partial_chars) = self.fill_style.partial_chars() {
                let bar = self.render_fine_grained(pct, partial_chars);
                return self.format_output(&bar, pct);
            }
        }

        // Standard rendering
        let filled = (pct * self.width as f64).round() as usize;
        let empty = (self.width as usize).saturating_sub(filled);

        let bar = format!(
            "{}{}",
            filled_char.to_string().repeat(filled),
            empty_char.to_string().repeat(empty)
        );

        self.format_output(&bar, pct)
    }

    /// Render with fine-grained progress display.
    fn render_fine_grained(&self, pct: f64, chars: [char; 8]) -> String {
        let total_units = self.width as usize * 8;
        let filled_units = (pct * total_units as f64).round() as usize;
        let full_chars = filled_units / 8;
        let partial_level = filled_units % 8;
        let empty_chars = (self.width as usize)
            .saturating_sub(full_chars)
            .saturating_sub(1);

        let mut result = String::with_capacity(self.width as usize);

        // Add full characters
        for _ in 0..full_chars {
            result.push(chars[7]);
        }

        // Add partial character if needed
        if full_chars + empty_chars < self.width as usize {
            result.push(chars[partial_level]);
        }

        // Add empty characters
        for _ in 0..empty_chars {
            result.push(chars[0]);
        }

        result
    }

    /// Format the output string with label, bar, and stats.
    fn format_output(&self, bar: &str, pct: f64) -> String {
        let mut parts = Vec::new();

        if !self.label.is_empty() {
            parts.push(format!("{}:", self.label));
        }

        parts.push(bar.to_string());

        if self.show_value {
            parts.push(format!("{}/{}", self.value, self.max));
        }

        if self.show_percent {
            parts.push(format!("{:.0}%", pct * 100.0));
        }

        parts.join(" ")
    }
}

impl Widget for ProgressBar {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.width < 2 || area.height == 0 {
            return;
        }

        let pct = if self.max > 0 {
            (self.value as f64 / self.max as f64).min(1.0)
        } else {
            0.0
        };

        let (filled_char, empty_char) = self.fill_style.chars();
        let bar_width = area.width.min(self.width) as usize;

        // Determine the color based on mode
        let bar_color = match self.color_mode {
            ProgressColorMode::Solid => self.fill_color,
            ProgressColorMode::Auto => self.auto_color(),
            ProgressColorMode::Gradient => self.color_at_position(pct),
            ProgressColorMode::CustomGradient(_, _) => self.color_at_position(pct),
        };

        if self.fine_grained {
            if let Some(partial_chars) = self.fill_style.partial_chars() {
                self.render_fine_grained_to_buffer(area, buf, pct, partial_chars, bar_color);
                return;
            }
        }

        // Standard rendering to buffer
        let filled = (pct * bar_width as f64).round() as usize;
        let empty = bar_width.saturating_sub(filled);

        let mut x = area.left();
        let y = area.top();

        // Render filled portion
        for _ in 0..filled {
            if x < area.right() {
                buf[(x, y)]
                    .set_char(filled_char)
                    .set_style(Style::default().fg(bar_color));
                x += 1;
            }
        }

        // Render empty portion
        for _ in 0..empty {
            if x < area.right() {
                buf[(x, y)]
                    .set_char(empty_char)
                    .set_style(Style::default().fg(Color::DarkGray));
                x += 1;
            }
        }

        // Render label and stats if space permits
        let stats_width = if self.label.is_empty() {
            0
        } else {
            self.label.len() + 2
        };
        let value_width = if self.show_value {
            format!("{}/{}", self.value, self.max).len()
        } else {
            0
        };
        let percent_width = if self.show_percent { 5 } else { 0 }; // "100%"

        let total_stats_width = stats_width
            + value_width
            + if value_width > 0 && percent_width > 0 {
                1
            } else {
                0
            }
            + percent_width;

        if total_stats_width > 0 && area.width > bar_width as u16 + total_stats_width as u16 {
            x = area.left() + bar_width as u16 + 1;

            if !self.label.is_empty() {
                let label_str = format!("{}: ", self.label);
                for (i, ch) in label_str.chars().enumerate() {
                    if x + (i as u16) < area.right() {
                        buf[(x + (i as u16), y)]
                            .set_char(ch)
                            .set_style(Style::default().fg(Color::White));
                    }
                }
                x += label_str.len() as u16;
            }

            if self.show_value {
                let value_str = format!("{}/{}", self.value, self.max);
                for (i, ch) in value_str.chars().enumerate() {
                    if x + (i as u16) < area.right() {
                        buf[(x + (i as u16), y)]
                            .set_char(ch)
                            .set_style(Style::default().fg(Color::White));
                    }
                }
                x += value_str.len() as u16;

                if self.show_percent {
                    if x < area.right() {
                        buf[(x, y)]
                            .set_char(' ')
                            .set_style(Style::default().fg(Color::White));
                        x += 1;
                    }
                }
            }

            if self.show_percent {
                let percent_str = format!("{:.0}%", pct * 100.0);
                for (i, ch) in percent_str.chars().enumerate() {
                    if x + (i as u16) < area.right() {
                        buf[(x + (i as u16), y)]
                            .set_char(ch)
                            .set_style(Style::default().fg(bar_color));
                    }
                }
            }
        }
    }
}

impl ProgressBar {
    /// Render fine-grained progress directly to buffer.
    fn render_fine_grained_to_buffer(
        &self,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
        pct: f64,
        chars: [char; 8],
        color: Color,
    ) {
        let bar_width = area.width.min(self.width) as usize;
        let total_units = bar_width * 8;
        let filled_units = (pct * total_units as f64).round() as usize;

        let mut x = area.left();
        let y = area.top();

        for i in 0..bar_width {
            if x >= area.right() {
                break;
            }

            let start_unit = i * 8;
            let end_unit = start_unit + 8;
            let segment_units = filled_units.saturating_sub(start_unit).min(end_unit) - start_unit;
            let level = if segment_units == 0 {
                0
            } else if segment_units >= 8 {
                7
            } else {
                segment_units
            };

            let ch = chars[level];
            let fg_color = if level > 0 { color } else { Color::DarkGray };

            buf[(x, y)]
                .set_char(ch)
                .set_style(Style::default().fg(fg_color));

            x += 1;
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
        // Focus indicator icon: "◆" for focused, "◇" for unfocused
        let focus_icon = if self.focused { "◆" } else { "◇" };

        // Border style: bright for focused, dim for unfocused
        let border_style = if self.focused {
            Style::default().fg(Color::LightCyan)
        } else {
            Style::default().fg(self.border_color)
        };

        // Title style: bold + bright for focused, dim for unfocused
        let title_style = if self.focused {
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // Content style: normal for focused, dim for unfocused
        let content_style = if self.focused {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let paragraph = Paragraph::new(self.content)
            .style(content_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(Span::styled(
                        format!(" {} {} ", focus_icon, self.title),
                        title_style,
                    )),
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

/// Sparkline widget for displaying trend data using Unicode block characters.
///
/// Uses 8-level Unicode blocks (▁▂▃▄▅▆▇█) to render compact trend visualizations.
/// Automatically scales values to fit the display area.
#[derive(Debug, Clone)]
pub struct SparklineWidget<'a> {
    /// Data values to render
    data: &'a [u64],
    /// Widget style
    style: Style,
    /// Direction of rendering
    direction: SparklineDirection,
    /// Optional label/title
    label: Option<&'a str>,
    /// Whether to show min/max values
    show_range: bool,
}

/// Direction for sparkline rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SparklineDirection {
    /// Render left to right
    LeftToRight,
    /// Render right to left
    RightToLeft,
}

impl Default for SparklineDirection {
    fn default() -> Self {
        Self::LeftToRight
    }
}

impl<'a> SparklineWidget<'a> {
    /// Create a new sparkline widget.
    pub fn new(data: &'a [u64]) -> Self {
        Self {
            data,
            style: Style::default().fg(Color::Green),
            direction: SparklineDirection::default(),
            label: None,
            show_range: false,
        }
    }

    /// Set the style (color, modifiers).
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the foreground color.
    pub fn color(mut self, color: Color) -> Self {
        self.style = self.style.fg(color);
        self
    }

    /// Set the rendering direction.
    pub fn direction(mut self, direction: SparklineDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Set the label text.
    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    /// Show min/max range below the sparkline.
    pub fn show_range(mut self, show: bool) -> Self {
        self.show_range = show;
        self
    }

    /// Render the sparkline as a string for use in text contexts.
    ///
    /// This is useful when you need the sparkline as plain text rather than
    /// rendering it directly to a buffer.
    pub fn render_string(&self, width: usize) -> String {
        if self.data.is_empty() {
            return " ".repeat(width);
        }

        // Unicode block characters for 8 levels
        const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

        let max_val = *self.data.iter().max().unwrap_or(&1);
        let min_val = *self.data.iter().min().unwrap_or(&0);
        let range = if max_val > min_val {
            (max_val - min_val) as f64
        } else {
            1.0
        };

        // Sample data to fit width
        let step = if width > 1 {
            self.data.len() as f64 / width as f64
        } else {
            0.0
        };

        let mut result = String::with_capacity(width);

        match self.direction {
            SparklineDirection::LeftToRight => {
                for i in 0..width {
                    let idx = if step > 0.0 {
                        ((i as f64) * step).floor() as usize
                    } else {
                        0
                    };
                    let idx = idx.min(self.data.len().saturating_sub(1));
                    let val = self.data[idx];

                    let normalized = if range > 0.0 {
                        ((val - min_val) as f64 / range).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };

                    let block_idx = (normalized * 7.0).round() as usize;
                    let block_idx = block_idx.min(7);
                    result.push(BLOCKS[block_idx]);
                }
            }
            SparklineDirection::RightToLeft => {
                for i in (0..width).rev() {
                    let idx = if step > 0.0 {
                        ((i as f64) * step).floor() as usize
                    } else {
                        0
                    };
                    let idx = idx.min(self.data.len().saturating_sub(1));
                    let val = self.data[idx];

                    let normalized = if range > 0.0 {
                        ((val - min_val) as f64 / range).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };

                    let block_idx = (normalized * 7.0).round() as usize;
                    let block_idx = block_idx.min(7);
                    result.push(BLOCKS[block_idx]);
                }
                result = result.chars().rev().collect();
            }
        }

        result
    }

    /// Get the range (min, max) of the data.
    pub fn range(&self) -> (u64, u64) {
        if self.data.is_empty() {
            return (0, 0);
        }
        let min = *self.data.iter().min().unwrap_or(&0);
        let max = *self.data.iter().max().unwrap_or(&0);
        (min, max)
    }
}

impl<'a> Widget for SparklineWidget<'a> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.width < 2 || area.height == 0 {
            return;
        }

        let width = area.width.saturating_sub(2) as usize; // Leave space for border
        let sparkline_str = self.render_string(width);

        let mut lines = Vec::new();

        // Add label if present
        if let Some(label) = self.label {
            lines.push(Line::from(vec![Span::styled(
                label,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]));
            lines.push(Line::from(Span::styled(&sparkline_str, self.style)));
        } else {
            lines.push(Line::from(Span::styled(&sparkline_str, self.style)));
        }

        // Add range if enabled
        if self.show_range && !self.data.is_empty() {
            let (min, max) = self.range();
            lines.push(Line::from(vec![
                Span::styled(format!("{}", min), Style::default().fg(Color::DarkGray)),
                Span::raw(" - "),
                Span::styled(format!("{}", max), Style::default().fg(Color::White)),
            ]));
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}

/// Convenience function to render a sparkline as a string.
///
/// This is a simpler API for quick sparkline rendering when you don't need
/// the full widget functionality.
///
/// # Arguments
///
/// * `values` - Slice of values to render
/// * `width` - Target width in characters
///
/// # Returns
///
/// A string containing the sparkline using Unicode block characters.
///
/// # Example
///
/// ```
/// use forge_tui::widget::render_sparkline;
///
/// let data = vec![1, 5, 3, 8, 4, 7, 2, 6];
/// let sparkline = render_sparkline(&data, 8);
/// assert_eq!(sparkline.chars().count(), 8);
/// ```
pub fn render_sparkline(values: &[u64], width: usize) -> String {
    SparklineWidget::new(values).render_string(width)
}

/// Convenience function to render a sparkline from signed integers.
///
/// Converts i64 values to u64 for rendering. Negative values are treated as zero.
pub fn render_sparkline_i64(values: &[i64], width: usize) -> String {
    let unsigned: Vec<u64> = values.iter().map(|&v| v.max(0) as u64).collect();
    SparklineWidget::new(&unsigned).render_string(width)
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
        let bar = ProgressBar::new(75, 100)
            .width(10)
            .label("Usage")
            .show_value(true);
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
    fn test_progress_bar_enable_auto_color_method() {
        let bar = ProgressBar::new(50, 100).enable_auto_color();
        assert_eq!(bar.color_mode, ProgressColorMode::Auto);
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
        let hints = HotkeyHints::new().hint('w', "Workers").hint('t', "Tasks");

        let line = hints.as_line();
        assert_eq!(line.spans.len(), 4); // 2 keys + 2 descriptions
    }

    // Sparkline widget tests
    #[test]
    fn test_sparkline_render_empty() {
        let data: Vec<u64> = vec![];
        let sparkline = SparklineWidget::new(&data).render_string(10);
        assert_eq!(sparkline.len(), 10);
        assert!(sparkline.chars().all(|c| c == ' '));
    }

    #[test]
    fn test_sparkline_render_basic() {
        let data = vec![1, 2, 3, 4, 5];
        let sparkline = SparklineWidget::new(&data).render_string(5);
        assert_eq!(sparkline.chars().count(), 5);
        // Should contain some block characters
        assert!(sparkline.contains('▁'));
        assert!(sparkline.contains('█'));
    }

    #[test]
    fn test_sparkline_width_limit() {
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let sparkline = SparklineWidget::new(&data).render_string(3);
        assert_eq!(sparkline.chars().count(), 3);
    }

    #[test]
    fn test_sparkline_all_same_values() {
        let data = vec![5, 5, 5, 5, 5];
        let sparkline = SparklineWidget::new(&data).render_string(5);
        assert_eq!(sparkline.chars().count(), 5);
        // All same values should still render something
    }

    #[test]
    fn test_sparkline_range() {
        let data = vec![10, 20, 30, 40, 50];
        let widget = SparklineWidget::new(&data);
        let (min, max) = widget.range();
        assert_eq!(min, 10);
        assert_eq!(max, 50);
    }

    #[test]
    fn test_sparkline_empty_range() {
        let data: Vec<u64> = vec![];
        let widget = SparklineWidget::new(&data);
        let (min, max) = widget.range();
        assert_eq!(min, 0);
        assert_eq!(max, 0);
    }

    #[test]
    fn test_sparkline_color() {
        let data = vec![1, 2, 3];
        let widget = SparklineWidget::new(&data).color(Color::Cyan);
        assert_eq!(widget.style.fg, Some(Color::Cyan));
    }

    #[test]
    fn test_sparkline_label() {
        let data = vec![1, 2, 3];
        let widget = SparklineWidget::new(&data).label("Trend");
        assert_eq!(widget.label, Some("Trend"));
    }

    #[test]
    fn test_sparkline_direction() {
        let data = vec![1, 2, 3, 4, 5];
        let widget_ltr = SparklineWidget::new(&data).direction(SparklineDirection::LeftToRight);
        let widget_rtl = SparklineWidget::new(&data).direction(SparklineDirection::RightToLeft);
        assert_eq!(widget_ltr.direction, SparklineDirection::LeftToRight);
        assert_eq!(widget_rtl.direction, SparklineDirection::RightToLeft);
    }

    #[test]
    fn test_render_sparkline_convenience() {
        let data = vec![1, 5, 3, 8, 4, 7, 2, 6];
        let sparkline = render_sparkline(&data, 10);
        assert_eq!(sparkline.chars().count(), 10);
    }

    #[test]
    fn test_render_sparkline_i64_convenience() {
        let data = vec![1i64, 5, 3, 8, 4, 7, 2, 6];
        let sparkline = render_sparkline_i64(&data, 8);
        assert_eq!(sparkline.chars().count(), 8);
    }

    #[test]
    fn test_render_sparkline_i64_negative() {
        let data = vec![-1i64, -5, 0, 3, 5];
        let sparkline = render_sparkline_i64(&data, 5);
        // Negative values should be treated as zero, so we should still get a valid sparkline
        assert_eq!(sparkline.chars().count(), 5);
    }

    #[test]
    fn test_sparkline_wide_range() {
        let data = vec![0, 100, 1000, 10000, 100000];
        let sparkline = SparklineWidget::new(&data).render_string(10);
        assert_eq!(sparkline.chars().count(), 10);
        // Should contain both empty and full blocks
        assert!(sparkline.contains('▁'));
        assert!(sparkline.contains('█'));
    }

    // Enhanced progress bar tests

    #[test]
    fn test_progress_bar_fill_style_smooth() {
        let bar = ProgressBar::new(50, 100)
            .width(10)
            .fill_style(ProgressFillStyle::Smooth);
        let rendered = bar.render_string();
        assert!(rendered.contains('█'));
        assert!(rendered.contains('░'));
    }

    #[test]
    fn test_progress_bar_fill_style_minimal() {
        let bar = ProgressBar::new(50, 100)
            .width(10)
            .fill_style(ProgressFillStyle::Minimal);
        let rendered = bar.render_string();
        assert!(rendered.contains('━'));
    }

    #[test]
    fn test_progress_bar_fill_style_heavy() {
        let bar = ProgressBar::new(50, 100)
            .width(10)
            .fill_style(ProgressFillStyle::Heavy);
        let rendered = bar.render_string();
        assert!(rendered.contains('■'));
        assert!(rendered.contains('□'));
    }

    #[test]
    fn test_progress_bar_fill_style_rounded() {
        let bar = ProgressBar::new(50, 100)
            .width(10)
            .fill_style(ProgressFillStyle::Rounded);
        let rendered = bar.render_string();
        assert!(rendered.contains('●'));
        assert!(rendered.contains('○'));
    }

    #[test]
    fn test_progress_bar_fine_grained() {
        let bar = ProgressBar::new(50, 100)
            .width(10)
            .fill_style(ProgressFillStyle::Fine)
            .fine_grained(true);
        let rendered = bar.render_string();
        // Fine-grained should use partial characters
        assert!(!rendered.is_empty());
    }

    #[test]
    fn test_progress_bar_vertical_style() {
        let bar = ProgressBar::new(50, 100)
            .width(10)
            .fill_style(ProgressFillStyle::Vertical)
            .fine_grained(true);
        let rendered = bar.render_string();
        // Vertical style with fine-grained should use block heights
        assert!(!rendered.is_empty());
    }

    #[test]
    fn test_progress_bar_color_mode_solid() {
        let bar = ProgressBar::new(50, 100)
            .fill_color(Color::Blue)
            .color_mode(ProgressColorMode::Solid);
        assert_eq!(bar.color_mode, ProgressColorMode::Solid);
    }

    #[test]
    fn test_progress_bar_color_mode_auto() {
        let bar = ProgressBar::new(50, 100).enable_auto_color();
        assert_eq!(bar.color_mode, ProgressColorMode::Auto);
    }

    #[test]
    fn test_progress_bar_color_mode_gradient() {
        let bar = ProgressBar::new(50, 100).gradient();
        assert_eq!(bar.color_mode, ProgressColorMode::Gradient);
    }

    #[test]
    fn test_progress_bar_custom_gradient() {
        let bar = ProgressBar::new(50, 100).custom_gradient(Color::Cyan, Color::Magenta);
        assert_eq!(
            bar.color_mode,
            ProgressColorMode::CustomGradient(Color::Cyan, Color::Magenta)
        );
    }

    #[test]
    fn test_progress_bar_show_value() {
        let bar = ProgressBar::new(50, 100)
            .width(10)
            .show_value(true)
            .show_percent(false);
        let rendered = bar.render_string();
        assert!(rendered.contains("50/100"));
    }

    #[test]
    fn test_progress_bar_hide_percent() {
        let bar = ProgressBar::new(50, 100).width(10).show_percent(false);
        let rendered = bar.render_string();
        assert!(!rendered.contains('%'));
    }

    #[test]
    fn test_progress_bar_full() {
        let bar = ProgressBar::new(100, 100).width(10);
        let rendered = bar.render_string();
        assert!(rendered.contains("100%"));
        // Should be completely filled
        assert!(!rendered.contains('░'));
    }

    #[test]
    fn test_progress_bar_empty() {
        let bar = ProgressBar::new(0, 100).width(10);
        let rendered = bar.render_string();
        assert!(rendered.contains("0%"));
        assert!(!rendered.contains('▓'));
    }

    #[test]
    fn test_progress_bar_clamp_value() {
        // Value exceeding max should be clamped
        let bar = ProgressBar::new(150, 100).width(10);
        let rendered = bar.render_string();
        // Should show 100% even though value > max
        assert!(rendered.contains("100%"));
    }

    #[test]
    fn test_progress_fill_style_chars() {
        assert_eq!(ProgressFillStyle::Blocks.chars(), ('▓', '░'));
        assert_eq!(ProgressFillStyle::Smooth.chars(), ('█', '░'));
        assert_eq!(ProgressFillStyle::Minimal.chars(), ('━', ' '));
        assert_eq!(ProgressFillStyle::Heavy.chars(), ('■', '□'));
        assert_eq!(ProgressFillStyle::Rounded.chars(), ('●', '○'));
        assert_eq!(ProgressFillStyle::Vertical.chars(), ('▇', ' '));
    }

    #[test]
    fn test_progress_fill_style_partial_chars() {
        // Fine style should have partial chars
        assert!(ProgressFillStyle::Fine.partial_chars().is_some());
        let chars = ProgressFillStyle::Fine.partial_chars().unwrap();
        assert_eq!(chars[0], '░');
        assert_eq!(chars[7], '█');

        // Vertical style should have partial chars
        assert!(ProgressFillStyle::Vertical.partial_chars().is_some());
        let chars = ProgressFillStyle::Vertical.partial_chars().unwrap();
        assert_eq!(chars[0], ' ');
        assert_eq!(chars[7], '▇');

        // Blocks style should not have partial chars
        assert!(ProgressFillStyle::Blocks.partial_chars().is_none());
    }

    #[test]
    fn test_progress_bar_render_width() {
        let bar = ProgressBar::new(50, 100).width(5);
        let rendered = bar.render_string();
        // Count the fill characters (should be 5)
        let fill_count = rendered.chars().filter(|c| *c == '▓' || *c == '░').count();
        assert_eq!(fill_count, 5);
    }

    #[test]
    fn test_progress_bar_zero_width() {
        let bar = ProgressBar::new(50, 100).width(0);
        let rendered = bar.render_string();
        // Should still render without crashing
        assert!(!rendered.is_empty());
    }

    #[test]
    fn test_progress_bar_with_all_options() {
        let bar = ProgressBar::new(75, 100)
            .width(20)
            .label("Progress")
            .fill_style(ProgressFillStyle::Smooth)
            .fill_color(Color::Cyan)
            .color_mode(ProgressColorMode::Gradient)
            .show_value(true)
            .show_percent(true);

        let rendered = bar.render_string();
        assert!(rendered.contains("Progress:"));
        assert!(rendered.contains("75/100"));
        assert!(rendered.contains("75%"));
    }
}
