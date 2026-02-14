//! Performance panel widget for FORGE internal metrics.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::perf_metrics::{HealthStatus, PerfAlertType, PerfMetrics};

pub struct PerfPanel<'a> {
    metrics: &'a PerfMetrics,
    focused: bool,
}

impl<'a> PerfPanel<'a> {
    pub fn new(metrics: &'a PerfMetrics) -> Self {
        Self { metrics, focused: false }
    }
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
}

impl Widget for PerfPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let focus_icon = if self.focused { "◆" } else { "◇" };
        let border_style = if self.focused { Style::default().fg(Color::LightCyan) } else { Style::default().fg(Color::DarkGray) };
        let title_style = if self.focused { Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::DarkGray) };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(format!(" {} FORGE Performance ", focus_icon), title_style));

        let inner = block.inner(area);
        block.render(area, buf);

        let health = self.metrics.health_status();
        let fps = self.metrics.current_fps();
        let frames = self.metrics.total_frames();
        let events = self.metrics.total_events();
        let memory_mb = self.metrics.memory_mb();
        let avg_loop = self.metrics.avg_event_loop_us();
        let p95_loop = self.metrics.p95_event_loop_us();
        let avg_render = self.metrics.avg_render_us();
        let p95_render = self.metrics.p95_render_us();

        let health_color = match health {
            HealthStatus::Excellent => Color::Green,
            HealthStatus::Good => Color::Cyan,
            HealthStatus::Fair => Color::Yellow,
            HealthStatus::Poor => Color::Red,
        };

        let loop_samples = self.metrics.event_loop_samples();
        let loop_sparkline = if !loop_samples.is_empty() {
            render_sparkline(&loop_samples, inner.width.saturating_sub(2) as usize)
        } else { "No data".to_string() };

        let render_samples = self.metrics.render_time_samples();
        let render_sparkline = if !render_samples.is_empty() {
            render_sparkline(&render_samples, inner.width.saturating_sub(2) as usize)
        } else { "No data".to_string() };

        let lines = vec![
            Line::from(vec![
                Span::styled("Health: ", Style::default().fg(Color::Gray)),
                Span::styled(health.label(), Style::default().fg(health_color).add_modifier(Modifier::BOLD)),
                Span::raw("   "),
                Span::styled(format!("{:.1} FPS", fps), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Event Loop ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(Span::styled(loop_sparkline, Style::default().fg(Color::Green))),
            Line::from(vec![
                Span::styled("Avg: ", Style::default().fg(Color::Gray)),
                Span::styled(format_us(avg_loop), Style::default().fg(Color::White)),
                Span::raw("  "),
                Span::styled("P95: ", Style::default().fg(Color::Gray)),
                Span::styled(format_us(p95_loop), if p95_loop < 16667 { Color::Green } else { Color::Red }),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Render Time ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(Span::styled(render_sparkline, Style::default().fg(Color::Magenta))),
            Line::from(vec![
                Span::styled("Avg: ", Style::default().fg(Color::Gray)),
                Span::styled(format_us(avg_render), Style::default().fg(Color::White)),
                Span::raw("  "),
                Span::styled("P95: ", Style::default().fg(Color::Gray)),
                Span::styled(format_us(p95_render), if p95_render < 8000 { Color::Green } else { Color::Yellow }),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Memory: ", Style::default().fg(Color::Gray)),
                Span::styled(format!("{} MB", memory_mb), if memory_mb < 200 { Color::Green } else if memory_mb < 500 { Color::Yellow } else { Color::Red }),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Frames: ", Style::default().fg(Color::Gray)),
                Span::styled(format!("{}", frames), Style::default().fg(Color::White)),
                Span::raw("  "),
                Span::styled("Events: ", Style::default().fg(Color::Gray)),
                Span::styled(format!("{}", events), Style::default().fg(Color::White)),
            ]),
        ];

        let paragraph = Paragraph::new(lines);
        paragraph.render(inner, buf);
    }
}

fn render_sparkline(values: &[u64], width: usize) -> String {
    if values.is_empty() { return " ".repeat(width); }
    let blocks = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = values.iter().cloned().fold(0u64, u64::max);
    let min = values.iter().cloned().fold(u64::MAX, u64::min);
    let range = (max.saturating_sub(min)) as f64;
    let step = values.len() as f64 / width as f64;
    let mut result = String::with_capacity(width);
    for i in 0..width {
        let idx = ((i as f64) * step).min(values.len() as f64 - 1.0) as usize;
        let val = values[idx];
        let normalized = if range > 0.0 { ((val.saturating_sub(min)) as f64 / range).clamp(0.0, 1.0) } else { 0.5 };
        result.push(blocks[((normalized * 7.0).round() as usize).min(7)]);
    }
    result
}

fn format_us(us: u64) -> String {
    if us < 1_000 { format!("{}μs", us) }
    else if us < 1_000_000 { format!("{:.2}ms", us as f64 / 1_000.0) }
    else { format!("{:.2}s", us as f64 / 1_000_000.0) }
}
