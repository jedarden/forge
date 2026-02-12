//! Activity Log panel for real-time streaming display.
//!
//! This module provides a real-time activity log panel with:
//! - Ring buffer for last N entries (configurable, default 100)
//! - Color-coded log levels (info=white, warn=yellow, error=red)
//! - Auto-scroll to latest entry with manual override
//! - Scroll pause when user navigates up
//!
//! ## Architecture
//!
//! The panel receives activity events from multiple sources:
//! - Worker status changes (spawn, stop, transition)
//! - Task pickup/completion events
//! - API call events (model, tokens, cost)
//! - Health check results
//! - Error and warning events

use chrono::{DateTime, Local, Utc};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use std::collections::VecDeque;

use crate::log::LogLevel;

/// Default ring buffer capacity for activity log (100 entries per spec).
pub const DEFAULT_ACTIVITY_CAPACITY: usize = 100;

/// Activity event types for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityEventType {
    /// Worker spawned
    WorkerSpawn,
    /// Worker stopped
    WorkerStop,
    /// Worker status transition
    WorkerTransition,
    /// Task picked up
    TaskPickup,
    /// Task completed
    TaskComplete,
    /// Task failed
    TaskFailed,
    /// API call event
    ApiCall,
    /// Health check result
    HealthCheck,
    /// General info
    Info,
    /// Warning event
    Warning,
    /// Error event
    Error,
}

impl ActivityEventType {
    /// Get the icon/symbol for this event type.
    pub fn icon(&self) -> &'static str {
        match self {
            ActivityEventType::WorkerSpawn => "▶",
            ActivityEventType::WorkerStop => "⏹",
            ActivityEventType::WorkerTransition => "⟳",
            ActivityEventType::TaskPickup => "⤴",
            ActivityEventType::TaskComplete => "✓",
            ActivityEventType::TaskFailed => "✗",
            ActivityEventType::ApiCall => "⚡",
            ActivityEventType::HealthCheck => "♥",
            ActivityEventType::Info => "●",
            ActivityEventType::Warning => "⚠",
            ActivityEventType::Error => "✖",
        }
    }

    /// Get the color for this event type.
    pub fn color(&self) -> Color {
        match self {
            ActivityEventType::WorkerSpawn => Color::Green,
            ActivityEventType::WorkerStop => Color::Yellow,
            ActivityEventType::WorkerTransition => Color::Cyan,
            ActivityEventType::TaskPickup => Color::Blue,
            ActivityEventType::TaskComplete => Color::Green,
            ActivityEventType::TaskFailed => Color::Red,
            ActivityEventType::ApiCall => Color::Magenta,
            ActivityEventType::HealthCheck => Color::Green,
            ActivityEventType::Info => Color::White,
            ActivityEventType::Warning => Color::Yellow,
            ActivityEventType::Error => Color::Red,
        }
    }

    /// Convert from LogLevel to ActivityEventType.
    pub fn from_log_level(level: LogLevel) -> Self {
        match level {
            LogLevel::Trace | LogLevel::Debug | LogLevel::Info => ActivityEventType::Info,
            LogLevel::Warn => ActivityEventType::Warning,
            LogLevel::Error => ActivityEventType::Error,
        }
    }
}

/// A single activity log entry.
#[derive(Debug, Clone)]
pub struct ActivityEntry {
    /// Timestamp of the event
    pub timestamp: DateTime<Utc>,
    /// Type of activity
    pub event_type: ActivityEventType,
    /// Source of the event (worker ID, component, etc.)
    pub source: Option<String>,
    /// Log message
    pub message: String,
}

impl ActivityEntry {
    /// Create a new activity entry.
    pub fn new(event_type: ActivityEventType, message: impl Into<String>) -> Self {
        Self {
            timestamp: Utc::now(),
            event_type,
            source: None,
            message: message.into(),
        }
    }

    /// Add a source to the entry.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Format the entry for display.
    pub fn format_display(&self) -> String {
        let time = self.timestamp.with_timezone(&Local).format("%H:%M:%S");
        if let Some(ref source) = self.source {
            format!("[{}] {}", source, self.message)
        } else {
            self.message.clone()
        }
    }
}

/// Activity log data with ring buffer.
#[derive(Debug, Clone)]
pub struct ActivityLogData {
    /// Ring buffer of entries
    entries: VecDeque<ActivityEntry>,
    /// Maximum capacity
    capacity: usize,
    /// Total entries ever added
    total_added: usize,
    /// Current scroll offset (0 = auto-scroll to bottom)
    scroll_offset: usize,
    /// Whether auto-scroll is paused due to user navigation
    auto_scroll_paused: bool,
    /// Last update timestamp
    last_update: Option<DateTime<Utc>>,
}

impl Default for ActivityLogData {
    fn default() -> Self {
        Self::new(DEFAULT_ACTIVITY_CAPACITY)
    }
}

impl ActivityLogData {
    /// Create a new activity log data with the specified capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
            total_added: 0,
            scroll_offset: 0,
            auto_scroll_paused: false,
            last_update: None,
        }
    }

    /// Create with default capacity (100 entries).
    pub fn with_default_capacity() -> Self {
        Self::default()
    }

    /// Push a new entry to the log.
    pub fn push(&mut self, entry: ActivityEntry) {
        // If at capacity, remove oldest
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
        self.total_added += 1;
        self.last_update = Some(Utc::now());

        // If auto-scroll is active, scroll offset stays at 0 (bottom)
        // If paused, increment offset to keep view stable
        if self.auto_scroll_paused && self.scroll_offset > 0 {
            self.scroll_offset += 1;
        }
    }

    /// Push multiple entries.
    pub fn push_batch(&mut self, entries: impl IntoIterator<Item = ActivityEntry>) {
        for entry in entries {
            self.push(entry);
        }
    }

    /// Get the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get the capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get total entries ever added.
    pub fn total_added(&self) -> usize {
        self.total_added
    }

    /// Scroll up by N lines.
    pub fn scroll_up(&mut self, n: usize) {
        let max_scroll = self.entries.len().saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + n).min(max_scroll);
        if self.scroll_offset > 0 {
            self.auto_scroll_paused = true;
        }
    }

    /// Scroll down by N lines.
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
        if self.scroll_offset == 0 {
            self.auto_scroll_paused = false;
        }
    }

    /// Scroll to the bottom (resume auto-scroll).
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll_paused = false;
    }

    /// Scroll to the top.
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = self.entries.len().saturating_sub(1);
        self.auto_scroll_paused = true;
    }

    /// Check if auto-scroll is paused.
    pub fn is_auto_scroll_paused(&self) -> bool {
        self.auto_scroll_paused
    }

    /// Get current scroll offset.
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Get visible entries for rendering.
    ///
    /// Returns entries in reverse order (newest first) with scroll offset applied.
    pub fn visible_entries(&self, max_lines: usize) -> Vec<&ActivityEntry> {
        let total = self.entries.len();
        if total == 0 {
            return Vec::new();
        }

        // Calculate start index based on scroll offset
        // scroll_offset = 0 means show newest entries (bottom)
        // scroll_offset > 0 means scroll back from newest
        let start_idx = if self.scroll_offset >= total {
            0
        } else {
            total.saturating_sub(self.scroll_offset + max_lines)
        };

        // Get entries from newest to oldest, bounded by max_lines
        let end_idx = total.saturating_sub(self.scroll_offset);

        self.entries
            .iter()
            .skip(start_idx)
            .take(end_idx.saturating_sub(start_idx).min(max_lines))
            .collect()
    }

    /// Get all entries (oldest first).
    pub fn iter(&self) -> impl Iterator<Item = &ActivityEntry> {
        self.entries.iter()
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.scroll_offset = 0;
        self.auto_scroll_paused = false;
    }

    /// Get the last entry.
    pub fn last(&self) -> Option<&ActivityEntry> {
        self.entries.back()
    }

    /// Check if we have any entries.
    pub fn has_entries(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Get last update timestamp.
    pub fn last_update(&self) -> Option<DateTime<Utc>> {
        self.last_update
    }
}

/// Activity log panel widget.
pub struct ActivityPanel<'a> {
    data: &'a ActivityLogData,
    focused: bool,
}

impl<'a> ActivityPanel<'a> {
    /// Create a new activity panel.
    pub fn new(data: &'a ActivityLogData) -> Self {
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

    /// Format an entry for display with colors.
    fn format_entry_line(&self, entry: &ActivityEntry) -> Line<'static> {
        let _time = entry.timestamp.with_timezone(&Local).format("%H:%M:%S");
        let icon = entry.event_type.icon();
        let color = entry.event_type.color();

        let mut spans = Vec::new();

        // Timestamp (dim)
        spans.push(Span::styled(
            format!("{} ", _time),
            Style::default().fg(Color::DarkGray),
        ));

        // Icon with event color
        spans.push(Span::styled(
            format!("{} ", icon),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));

        // Source (if present) in cyan
        if let Some(ref source) = entry.source {
            spans.push(Span::styled(
                format!("[{}] ", source),
                Style::default().fg(Color::Cyan),
            ));
        }

        // Message in appropriate color based on event type
        let msg_color = match entry.event_type {
            ActivityEventType::Error => Color::Red,
            ActivityEventType::Warning => Color::Yellow,
            _ => Color::White,
        };
        spans.push(Span::styled(entry.message.clone(), Style::default().fg(msg_color)));

        Line::from(spans)
    }
}

impl Widget for ActivityPanel<'_> {
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

        // Build title with status indicators
        let title = if self.data.is_auto_scroll_paused() {
            format!(
                " Activity Log [paused - {}/{}] ",
                self.data.scroll_offset(),
                self.data.len()
            )
        } else if self.data.has_entries() {
            format!(" Activity Log [{}] ", self.data.len())
        } else {
            " Activity Log ".to_string()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(title, title_style));

        let inner = block.inner(area);
        block.render(area, buf);

        // Handle empty state
        if !self.data.has_entries() {
            let empty_msg = Paragraph::new(
                "No recent activity.\n\n\
                 Activity will appear here as workers\n\
                 start, complete tasks, and update status.\n\n\
                 [↑/↓] Scroll  [G] Top  [Shift+G] Bottom",
            )
            .style(Style::default().fg(Color::Gray));
            empty_msg.render(inner, buf);
            return;
        }

        // Calculate how many lines we can display
        let max_lines = inner.height as usize;

        // Get visible entries (in reverse order for display)
        let visible = self.data.visible_entries(max_lines);

        // Build lines from entries (newest at bottom)
        let lines: Vec<Line> = visible.into_iter().rev().map(|e| self.format_entry_line(e)).collect();

        let paragraph = Paragraph::new(lines);
        paragraph.render(inner, buf);
    }
}

/// Compact activity summary for overview panel.
pub struct ActivitySummaryCompact<'a> {
    data: &'a ActivityLogData,
}

impl<'a> ActivitySummaryCompact<'a> {
    /// Create a new compact activity summary.
    pub fn new(data: &'a ActivityLogData) -> Self {
        Self { data }
    }
}

impl Widget for ActivitySummaryCompact<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.data.has_entries() {
            let no_activity =
                Paragraph::new("No activity").style(Style::default().fg(Color::Gray));
            no_activity.render(area, buf);
            return;
        }

        // Show last 3 entries in compact form
        let entries: Vec<_> = self.data.entries.iter().rev().take(3).collect();

        let mut lines = Vec::new();
        for entry in entries {
            let time = entry.timestamp.with_timezone(&Local).format("%H:%M");
            let icon = entry.event_type.icon();
            let color = entry.event_type.color();

            // Truncate message if needed
            let max_msg_len = (area.width as usize).saturating_sub(15);
            let msg = if entry.message.len() > max_msg_len {
                format!("{}…", &entry.message[..max_msg_len.saturating_sub(1)])
            } else {
                entry.message.clone()
            };

            lines.push(Line::from(vec![
                Span::styled(time.to_string(), Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(icon.to_string(), Style::default().fg(color)),
                Span::raw(" "),
                Span::styled(msg, Style::default().fg(Color::White)),
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
    fn test_activity_event_type_colors() {
        assert_eq!(ActivityEventType::Error.color(), Color::Red);
        assert_eq!(ActivityEventType::Warning.color(), Color::Yellow);
        assert_eq!(ActivityEventType::Info.color(), Color::White);
        assert_eq!(ActivityEventType::WorkerSpawn.color(), Color::Green);
    }

    #[test]
    fn test_activity_log_data_new() {
        let data = ActivityLogData::new(100);
        assert_eq!(data.capacity(), 100);
        assert_eq!(data.len(), 0);
        assert!(data.is_empty());
        assert!(!data.is_auto_scroll_paused());
    }

    #[test]
    fn test_activity_log_data_push() {
        let mut data = ActivityLogData::new(10);

        data.push(ActivityEntry::new(ActivityEventType::Info, "Test message"));
        assert_eq!(data.len(), 1);
        assert_eq!(data.total_added(), 1);

        data.push(ActivityEntry::new(ActivityEventType::Warning, "Warning!"));
        assert_eq!(data.len(), 2);
        assert_eq!(data.total_added(), 2);
    }

    #[test]
    fn test_activity_log_data_ring_buffer() {
        let mut data = ActivityLogData::new(3);

        for i in 1..=5 {
            data.push(ActivityEntry::new(ActivityEventType::Info, format!("Entry {}", i)));
        }

        assert_eq!(data.len(), 3);
        assert_eq!(data.total_added(), 5);

        // Should have entries 3, 4, 5
        let messages: Vec<_> = data.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(messages, vec!["Entry 3", "Entry 4", "Entry 5"]);
    }

    #[test]
    fn test_activity_log_data_scroll() {
        let mut data = ActivityLogData::new(100);

        // Add 10 entries
        for i in 1..=10 {
            data.push(ActivityEntry::new(ActivityEventType::Info, format!("Entry {}", i)));
        }

        // Initially at bottom (scroll_offset = 0)
        assert_eq!(data.scroll_offset(), 0);
        assert!(!data.is_auto_scroll_paused());

        // Scroll up 3 lines
        data.scroll_up(3);
        assert_eq!(data.scroll_offset(), 3);
        assert!(data.is_auto_scroll_paused());

        // Scroll down 1 line
        data.scroll_down(1);
        assert_eq!(data.scroll_offset(), 2);
        assert!(data.is_auto_scroll_paused());

        // Scroll to bottom resumes auto-scroll
        data.scroll_to_bottom();
        assert_eq!(data.scroll_offset(), 0);
        assert!(!data.is_auto_scroll_paused());

        // Scroll to top
        data.scroll_to_top();
        assert_eq!(data.scroll_offset(), 9); // 10 entries - 1
        assert!(data.is_auto_scroll_paused());
    }

    #[test]
    fn test_activity_log_data_auto_scroll_pause_on_new_entry() {
        let mut data = ActivityLogData::new(10);

        // Add some entries
        for i in 1..=5 {
            data.push(ActivityEntry::new(ActivityEventType::Info, format!("Entry {}", i)));
        }

        // Scroll up to pause auto-scroll
        data.scroll_up(2);
        assert!(data.is_auto_scroll_paused());
        assert_eq!(data.scroll_offset(), 2);

        // Add new entry - scroll offset should increase to keep view stable
        data.push(ActivityEntry::new(ActivityEventType::Info, "Entry 6"));
        assert_eq!(data.scroll_offset(), 3); // Increased to keep same entries visible

        // Scroll to bottom and add - offset should stay 0
        data.scroll_to_bottom();
        data.push(ActivityEntry::new(ActivityEventType::Info, "Entry 7"));
        assert_eq!(data.scroll_offset(), 0);
    }

    #[test]
    fn test_activity_entry_with_source() {
        let entry = ActivityEntry::new(ActivityEventType::WorkerSpawn, "Worker started")
            .with_source("worker-1");

        assert_eq!(entry.source, Some("worker-1".to_string()));
        assert_eq!(entry.message, "Worker started");
    }

    #[test]
    fn test_visible_entries() {
        let mut data = ActivityLogData::new(100);

        // Add 10 entries
        for i in 1..=10 {
            data.push(ActivityEntry::new(ActivityEventType::Info, format!("Entry {}", i)));
        }

        // At bottom (scroll_offset = 0), should show newest entries
        let visible = data.visible_entries(5);
        assert_eq!(visible.len(), 5);
        // The last 5 entries should be visible (6, 7, 8, 9, 10)
        assert_eq!(visible.last().unwrap().message, "Entry 10");

        // Scrolled up by 3, should show older entries
        data.scroll_up(3);
        let visible = data.visible_entries(5);
        // Now showing entries 3-7 (10 - 3 - 5 + 1 to 10 - 3)
        assert_eq!(visible.len(), 5);
    }

    #[test]
    fn test_from_log_level() {
        assert_eq!(
            ActivityEventType::from_log_level(LogLevel::Info),
            ActivityEventType::Info
        );
        assert_eq!(
            ActivityEventType::from_log_level(LogLevel::Warn),
            ActivityEventType::Warning
        );
        assert_eq!(
            ActivityEventType::from_log_level(LogLevel::Error),
            ActivityEventType::Error
        );
    }
}
