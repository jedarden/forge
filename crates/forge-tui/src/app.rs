//! Main application state and logic for the FORGE TUI.
//!
//! The `App` struct manages overall application state, view switching,
//! and coordinates between different components.

use std::io;

use crossterm::event::{self, Event, KeyEvent};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame, Terminal,
};

use crate::event::{AppEvent, InputHandler};
use crate::view::{FocusPanel, View};

/// Result type for app operations.
pub type AppResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Main application state.
#[derive(Debug)]
pub struct App {
    /// Current active view
    current_view: View,
    /// Previous view (for back navigation)
    previous_view: Option<View>,
    /// Current focused panel within the view
    focus_panel: FocusPanel,
    /// Input handler for key events
    input_handler: InputHandler,
    /// Whether the app should quit
    should_quit: bool,
    /// Whether to show the help overlay
    show_help: bool,
    /// Chat input buffer
    chat_input: String,
    /// Status message to display
    status_message: Option<String>,
    /// List scroll position for current view
    scroll_offset: usize,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// Create a new app instance with default state.
    pub fn new() -> Self {
        Self {
            current_view: View::default(),
            previous_view: None,
            focus_panel: FocusPanel::default(),
            input_handler: InputHandler::new(),
            should_quit: false,
            show_help: false,
            chat_input: String::new(),
            status_message: None,
            scroll_offset: 0,
        }
    }

    /// Returns the current view.
    pub fn current_view(&self) -> View {
        self.current_view
    }

    /// Returns the current focus panel.
    pub fn focus_panel(&self) -> FocusPanel {
        self.focus_panel
    }

    /// Returns whether the app should quit.
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Returns whether help overlay is visible.
    pub fn show_help(&self) -> bool {
        self.show_help
    }

    /// Switch to a specific view.
    pub fn switch_view(&mut self, view: View) {
        if self.current_view != view {
            self.previous_view = Some(self.current_view);
            self.current_view = view;
            self.scroll_offset = 0;

            // Set appropriate default focus for the view
            self.focus_panel = match view {
                View::Overview => FocusPanel::WorkerPool,
                View::Workers => FocusPanel::WorkerPool,
                View::Tasks => FocusPanel::TaskQueue,
                View::Costs => FocusPanel::CostBreakdown,
                View::Metrics => FocusPanel::MetricsCharts,
                View::Logs => FocusPanel::ActivityLog,
                View::Chat => FocusPanel::ChatInput,
            };

            // Update input handler for chat mode
            self.input_handler.set_chat_mode(view == View::Chat);

            self.status_message = Some(format!(
                "{} (Press {} to return here)",
                view.title(),
                view.hotkey()
            ));
        }
    }

    /// Go to the next view in the cycle.
    pub fn next_view(&mut self) {
        let next = self.current_view.next();
        self.switch_view(next);
    }

    /// Go to the previous view in the cycle.
    pub fn prev_view(&mut self) {
        let prev = self.current_view.prev();
        self.switch_view(prev);
    }

    /// Go back to the previous view (if any).
    pub fn go_back(&mut self) {
        if let Some(prev) = self.previous_view.take() {
            self.switch_view(prev);
        }
    }

    /// Handle a key event.
    pub fn handle_key_event(&mut self, key: KeyEvent) {
        let event = self.input_handler.handle_key(key);
        self.handle_app_event(event);
    }

    /// Handle an application event.
    pub fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::SwitchView(view) => self.switch_view(view),
            AppEvent::NextView => self.next_view(),
            AppEvent::PrevView => self.prev_view(),
            AppEvent::ShowHelp => self.show_help = true,
            AppEvent::HideHelp => self.show_help = false,
            AppEvent::Quit => self.should_quit = true,
            AppEvent::ForceQuit => self.should_quit = true,
            AppEvent::Refresh => {
                self.status_message = Some("Refreshed".to_string());
            }
            AppEvent::Cancel => {
                if self.show_help {
                    self.show_help = false;
                } else if self.current_view == View::Chat {
                    self.chat_input.clear();
                    self.go_back();
                }
            }
            AppEvent::NavigateUp => {
                if self.scroll_offset > 0 {
                    self.scroll_offset -= 1;
                }
            }
            AppEvent::NavigateDown => {
                self.scroll_offset += 1;
            }
            AppEvent::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
            }
            AppEvent::PageDown => {
                self.scroll_offset += 10;
            }
            AppEvent::GoToTop => {
                self.scroll_offset = 0;
            }
            AppEvent::GoToBottom => {
                // In a real impl, this would go to the end of the list
                self.scroll_offset = 100;
            }
            AppEvent::TextInput(c) => {
                self.chat_input.push(c);
            }
            AppEvent::Backspace => {
                self.chat_input.pop();
            }
            AppEvent::Submit => {
                if !self.chat_input.is_empty() {
                    self.status_message = Some(format!("Executing: {}", self.chat_input));
                    self.chat_input.clear();
                }
            }
            AppEvent::Select | AppEvent::Toggle | AppEvent::FocusNext | AppEvent::FocusPrev => {
                // Panel-specific handling - to be implemented
            }
            AppEvent::None => {}
        }
    }

    /// Run the main application loop.
    pub fn run(&mut self) -> AppResult<()> {
        // Setup terminal
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        crossterm::execute!(
            stdout,
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Main loop
        let result = self.run_loop(&mut terminal);

        // Restore terminal
        crossterm::terminal::disable_raw_mode()?;
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        result
    }

    /// The inner event loop.
    fn run_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> AppResult<()> {
        while !self.should_quit {
            // Draw UI
            terminal.draw(|frame| self.draw(frame))?;

            // Handle events
            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key_event(key);
                }
            }
        }
        Ok(())
    }

    /// Draw the UI.
    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area();

        // Main layout: header, content, footer
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(10),   // Content
                Constraint::Length(2), // Footer
            ])
            .split(area);

        self.draw_header(frame, chunks[0]);
        self.draw_content(frame, chunks[1]);
        self.draw_footer(frame, chunks[2]);

        // Draw help overlay if active
        if self.show_help {
            self.draw_help_overlay(frame, area);
        }
    }

    /// Draw the header bar.
    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let title = format!(" FORGE Dashboard - {} ", self.current_view.title());
        let title_len = title.len();

        let header = Paragraph::new(Line::from(vec![
            Span::styled(title, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" ".repeat(area.width.saturating_sub(title_len as u16 + 25) as usize)),
            Span::styled(format!("{}", now), Style::default().fg(Color::Gray)),
            Span::raw("  "),
            Span::styled("[System: Healthy]", Style::default().fg(Color::Green)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

        frame.render_widget(header, area);
    }

    /// Draw the main content area based on current view.
    fn draw_content(&self, frame: &mut Frame, area: Rect) {
        match self.current_view {
            View::Overview => self.draw_overview(frame, area),
            View::Workers => self.draw_workers(frame, area),
            View::Tasks => self.draw_tasks(frame, area),
            View::Costs => self.draw_costs(frame, area),
            View::Metrics => self.draw_metrics(frame, area),
            View::Logs => self.draw_logs(frame, area),
            View::Chat => self.draw_chat(frame, area),
        }
    }

    /// Draw the footer with hotkey hints.
    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let hints = vec![
            Span::styled("[o]", Style::default().fg(Color::Yellow)),
            Span::raw("Overview "),
            Span::styled("[w]", Style::default().fg(Color::Yellow)),
            Span::raw("Workers "),
            Span::styled("[t]", Style::default().fg(Color::Yellow)),
            Span::raw("Tasks "),
            Span::styled("[c]", Style::default().fg(Color::Yellow)),
            Span::raw("Costs "),
            Span::styled("[m]", Style::default().fg(Color::Yellow)),
            Span::raw("Metrics "),
            Span::styled("[l]", Style::default().fg(Color::Yellow)),
            Span::raw("Logs "),
            Span::styled("[:]", Style::default().fg(Color::Yellow)),
            Span::raw("Chat "),
            Span::styled("[?]", Style::default().fg(Color::Yellow)),
            Span::raw("Help "),
            Span::styled("[q]", Style::default().fg(Color::Yellow)),
            Span::raw("Quit"),
        ];

        let footer = Paragraph::new(Line::from(hints))
            .style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::TOP));

        frame.render_widget(footer, area);
    }

    /// Draw the Overview/Dashboard view.
    fn draw_overview(&self, frame: &mut Frame, area: Rect) {
        // Split into top and bottom sections
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        // Top section: Workers and Subscriptions side by side
        let top_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[0]);

        self.draw_panel(
            frame,
            top_chunks[0],
            "Worker Pool",
            "Total: 24 (18 active, 6 idle)\nUnhealthy: 0\n\nGLM-4.7:   8 active, 3 idle\nSonnet:    6 active, 2 idle\nOpus:      3 active, 1 idle\nHaiku:     1 active, 0 idle",
            self.focus_panel == FocusPanel::WorkerPool,
        );

        self.draw_panel(
            frame,
            top_chunks[1],
            "Subscriptions",
            "Claude Pro: 72/100 (72%)\n▓▓▓▓▓▓▓▓▓▓▓▓░░░░░\nResets in: 12h 34m\n\nGLM-4.7: 430/1000 (43%)\n▓▓▓▓▓▓▓▓░░░░░░░░░░",
            self.focus_panel == FocusPanel::Subscriptions,
        );

        // Bottom section: Task Queue and Activity Log
        let bottom_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);

        self.draw_panel(
            frame,
            bottom_chunks[0],
            "Task Queue (10 ready, 3 blocked)",
            "[P0] bd-abc: Fetch order history\n[P0] bd-def: Analyze failures\n[P1] bd-ghi: Test BitcoinHourly\n[P2] bd-jkl: Update validation",
            self.focus_panel == FocusPanel::TaskQueue,
        );

        self.draw_panel(
            frame,
            bottom_chunks[1],
            "Activity Log",
            "14:23:45 ✓ worker-glm-03 completed bd-xyz\n14:23:12 ⟳ Spawned worker-sonnet-07\n14:22:58 ✓ worker-opus-01 completed bd-mno\n14:22:45 ⚠ worker-glm-02 idle for 5m",
            self.focus_panel == FocusPanel::ActivityLog,
        );
    }

    /// Draw the Workers view.
    fn draw_workers(&self, frame: &mut Frame, area: Rect) {
        self.draw_panel(
            frame,
            area,
            "Worker Pool Management",
            "┌─────────────────┬──────────┬──────────┬─────────────┐\n\
             │ Worker ID       │ Model    │ Status   │ Task        │\n\
             ├─────────────────┼──────────┼──────────┼─────────────┤\n\
             │ worker-glm-01   │ GLM-4.7  │ Active   │ bd-abc      │\n\
             │ worker-glm-02   │ GLM-4.7  │ Idle     │ -           │\n\
             │ worker-sonnet-01│ Sonnet   │ Active   │ bd-def      │\n\
             │ worker-opus-01  │ Opus     │ Active   │ bd-ghi      │\n\
             └─────────────────┴──────────┴──────────┴─────────────┘\n\n\
             [G] Spawn GLM  [S] Spawn Sonnet  [O] Spawn Opus  [K] Kill",
            true,
        );
    }

    /// Draw the Tasks view.
    fn draw_tasks(&self, frame: &mut Frame, area: Rect) {
        self.draw_panel(
            frame,
            area,
            "Task Queue & Bead Management",
            "Ready: 10 | Blocked: 3 | In Progress: 15 | Completed: 42\n\n\
             ┌──────┬────────────────────────────────┬──────┐\n\
             │ ID   │ Title                          │ Prio │\n\
             ├──────┼────────────────────────────────┼──────┤\n\
             │bd-abc│ Fetch order history from Kalsh │ P0   │\n\
             │bd-def│ Analyze execution failures     │ P0   │\n\
             │bd-ghi│ Test BitcoinHourly strategy    │ P1   │\n\
             │bd-jkl│ Identify duplicate orders      │ P1   │\n\
             └──────┴────────────────────────────────┴──────┘\n\n\
             [A] Assign  [P] Priority  [M] Model  [C] Close",
            true,
        );
    }

    /// Draw the Costs view.
    fn draw_costs(&self, frame: &mut Frame, area: Rect) {
        self.draw_panel(
            frame,
            area,
            "Cost Analytics",
            "Today's Total: $24.56          Budget: $50.00\n\
             ▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░  49%\n\n\
             Breakdown by Model:\n\
               GLM-4.7:    $8.45  (34%)\n\
               Sonnet 4.5: $10.22 (42%)\n\
               Opus 4.6:   $5.12  (21%)\n\
               Haiku 4:    $0.77  (3%)\n\n\
             Cost per Task: $0.58 (42 tasks)\n\n\
             Month-to-Date: $172.34 / $1,500",
            true,
        );
    }

    /// Draw the Metrics view.
    fn draw_metrics(&self, frame: &mut Frame, area: Rect) {
        self.draw_panel(
            frame,
            area,
            "Performance Metrics",
            "Tasks Completed Today: 42\n\
             Avg Task Duration: 8m 34s\n\
             Worker Utilization: 75%\n\n\
             Tasks/Hour:\n\
             12:00 ▓▓▓▓▓▓ 6\n\
             13:00 ▓▓▓▓▓▓▓▓▓ 9\n\
             14:00 ▓▓▓▓▓▓▓ 7\n\n\
             Model Efficiency:\n\
               GLM-4.7:  $0.42/task (best)\n\
               Sonnet:   $0.68/task\n\
               Opus:     $1.28/task",
            true,
        );
    }

    /// Draw the Logs view.
    fn draw_logs(&self, frame: &mut Frame, area: Rect) {
        self.draw_panel(
            frame,
            area,
            "Activity Log",
            "14:23:45 [INFO]  worker-glm-03 completed task bd-xyz in 2m 34s\n\
             14:23:12 [INFO]  Spawned worker-sonnet-07 for /kalshi-improvement\n\
             14:22:58 [INFO]  worker-opus-01 completed task bd-mno in 8m 12s\n\
             14:22:45 [WARN]  worker-glm-02 idle for 5m, reassigning\n\
             14:22:30 [INFO]  Subscription optimization: switching 3 tasks to GLM\n\
             14:22:15 [INFO]  worker-haiku-01 completed task bd-pqr in 1m 05s\n\
             14:22:00 [WARN]  Claude Pro usage at 70%, considering GLM fallback\n\
             14:21:45 [INFO]  Spawned worker-glm-04 for /control-panel\n\
             14:21:30 [INFO]  worker-sonnet-02 started task bd-stu\n\
             14:21:15 [ERROR] worker-glm-05 connection timeout, restarting",
            true,
        );
    }

    /// Draw the Chat view.
    fn draw_chat(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(3)])
            .split(area);

        // Chat history
        self.draw_panel(
            frame,
            chunks[0],
            "Chat",
            "Type commands or ask questions. Examples:\n\n\
             > show workers\n\
             > spawn 2 glm workers\n\
             > show P0 tasks\n\
             > costs today\n\
             > help\n\n\
             Press Esc to exit chat mode.",
            false,
        );

        // Input field
        let input_style = if self.focus_panel == FocusPanel::ChatInput {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Gray)
        };

        let cursor = if self.input_handler.is_chat_mode() {
            "█"
        } else {
            ""
        };

        let input = Paragraph::new(format!("> {}{}", self.chat_input, cursor))
            .style(input_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(" Input "),
            );

        frame.render_widget(input, chunks[1]);
    }

    /// Draw a panel with optional highlight.
    fn draw_panel(&self, frame: &mut Frame, area: Rect, title: &str, content: &str, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let title_style = if focused {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let panel = Paragraph::new(content)
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(Span::styled(format!(" {} ", title), title_style)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(panel, area);
    }

    /// Draw the help overlay.
    fn draw_help_overlay(&self, frame: &mut Frame, area: Rect) {
        // Calculate centered overlay area
        let overlay_width = 60.min(area.width.saturating_sub(4));
        let overlay_height = 20.min(area.height.saturating_sub(4));
        let overlay_x = (area.width - overlay_width) / 2;
        let overlay_y = (area.height - overlay_height) / 2;

        let overlay_area = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

        // Clear background
        frame.render_widget(Clear, overlay_area);

        let help_text = "\
FORGE Hotkey Reference

View Navigation:
  o        Overview (dashboard)
  w        Workers view
  t        Tasks view
  c        Costs view
  m        Metrics view
  l        Logs view
  :        Chat input mode
  Tab      Cycle views forward
  Shift+Tab Cycle views backward

General:
  ?  h     Show this help
  q        Quit
  Esc      Cancel / Close
  Ctrl+C   Force quit
  Ctrl+L   Refresh
  r        Refresh

Navigation:
  ↑ k      Move up
  ↓ j      Move down
  PgUp     Page up
  PgDn     Page down

Press any key to close this help.";

        let help = Paragraph::new(help_text)
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(Span::styled(
                        " Help ",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ))
                    .style(Style::default().bg(Color::Black)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(help, overlay_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_creation() {
        let app = App::new();
        assert_eq!(app.current_view(), View::Overview);
        assert!(!app.should_quit());
        assert!(!app.show_help());
    }

    #[test]
    fn test_view_switching() {
        let mut app = App::new();
        assert_eq!(app.current_view(), View::Overview);

        app.switch_view(View::Workers);
        assert_eq!(app.current_view(), View::Workers);

        app.switch_view(View::Tasks);
        assert_eq!(app.current_view(), View::Tasks);
    }

    #[test]
    fn test_view_cycling() {
        let mut app = App::new();
        assert_eq!(app.current_view(), View::Overview);

        app.next_view();
        assert_eq!(app.current_view(), View::Workers);

        app.next_view();
        assert_eq!(app.current_view(), View::Tasks);

        app.prev_view();
        assert_eq!(app.current_view(), View::Workers);
    }

    #[test]
    fn test_chat_mode() {
        let mut app = App::new();
        assert!(!app.input_handler.is_chat_mode());

        app.switch_view(View::Chat);
        assert!(app.input_handler.is_chat_mode());
        assert_eq!(app.focus_panel(), FocusPanel::ChatInput);

        // Simulate text input
        app.handle_app_event(AppEvent::TextInput('h'));
        app.handle_app_event(AppEvent::TextInput('i'));
        assert_eq!(app.chat_input, "hi");

        // Backspace
        app.handle_app_event(AppEvent::Backspace);
        assert_eq!(app.chat_input, "h");
    }

    #[test]
    fn test_quit_handling() {
        let mut app = App::new();
        assert!(!app.should_quit());

        app.handle_app_event(AppEvent::Quit);
        assert!(app.should_quit());
    }

    #[test]
    fn test_help_toggle() {
        let mut app = App::new();
        assert!(!app.show_help());

        app.handle_app_event(AppEvent::ShowHelp);
        assert!(app.show_help());

        app.handle_app_event(AppEvent::Cancel);
        assert!(!app.show_help());
    }

    #[test]
    fn test_focus_panel_on_view_switch() {
        let mut app = App::new();

        app.switch_view(View::Workers);
        assert_eq!(app.focus_panel(), FocusPanel::WorkerPool);

        app.switch_view(View::Tasks);
        assert_eq!(app.focus_panel(), FocusPanel::TaskQueue);

        app.switch_view(View::Costs);
        assert_eq!(app.focus_panel(), FocusPanel::CostBreakdown);

        app.switch_view(View::Logs);
        assert_eq!(app.focus_panel(), FocusPanel::ActivityLog);

        app.switch_view(View::Chat);
        assert_eq!(app.focus_panel(), FocusPanel::ChatInput);
    }

    #[test]
    fn test_navigation() {
        let mut app = App::new();
        assert_eq!(app.scroll_offset, 0);

        app.handle_app_event(AppEvent::NavigateDown);
        assert_eq!(app.scroll_offset, 1);

        app.handle_app_event(AppEvent::NavigateUp);
        assert_eq!(app.scroll_offset, 0);

        // Can't go below 0
        app.handle_app_event(AppEvent::NavigateUp);
        assert_eq!(app.scroll_offset, 0);

        app.handle_app_event(AppEvent::PageDown);
        assert_eq!(app.scroll_offset, 10);
    }
}
