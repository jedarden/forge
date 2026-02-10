//! Main application state and logic for the FORGE TUI.
//!
//! The `App` struct manages overall application state, view switching,
//! and coordinates between different components.

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEvent};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame, Terminal,
};

use crate::cost_panel::CostPanel;
use crate::data::DataManager;
use crate::event::{AppEvent, InputHandler};
use crate::metrics_panel::MetricsPanel;
use crate::theme::ThemeManager;
use crate::view::{FocusPanel, LayoutMode, View};
use crate::widget::QuickActionsPanel;

/// Result type for app operations.
pub type AppResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Target frame rate (60 FPS = ~16.67ms per frame).
const TARGET_FPS: u64 = 60;
const FRAME_DURATION: Duration = Duration::from_millis(1000 / TARGET_FPS);

/// Data polling intervals (optimized for performance).
const DATA_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Header timestamp cache duration (update every second).
const TIMESTAMP_CACHE_DURATION: Duration = Duration::from_secs(1);

/// Main application state.
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
    /// Data manager for real worker/task data
    data_manager: DataManager,
    /// Theme manager for color themes
    theme_manager: ThemeManager,
    /// Dirty flag - whether UI needs redraw
    dirty: bool,
    /// Cached terminal size for layout recalculation
    cached_size: Option<Rect>,
    /// Last data poll time
    last_poll_time: Instant,
    /// Cached timestamp for header (updated every second)
    cached_timestamp: Option<String>,
    last_timestamp_update: Instant,
    /// Cached layout mode based on terminal width
    cached_layout_mode: Option<LayoutMode>,
    /// Last terminal width for layout mode detection
    last_terminal_width: u16,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// Create a new app instance with default state.
    pub fn new() -> Self {
        let now = Instant::now();
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
            data_manager: DataManager::new(),
            theme_manager: ThemeManager::load_config(),
            dirty: true,
            cached_size: None,
            last_poll_time: now,
            cached_timestamp: None,
            last_timestamp_update: now,
            cached_layout_mode: None,
            last_terminal_width: 0,
        }
    }

    /// Create a new app with a custom status directory (for testing).
    #[allow(dead_code)]
    pub fn with_status_dir(status_dir: std::path::PathBuf) -> Self {
        let now = Instant::now();
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
            data_manager: DataManager::with_status_dir(status_dir),
            theme_manager: ThemeManager::new(),
            dirty: true,
            cached_size: None,
            last_poll_time: now,
            cached_timestamp: None,
            last_timestamp_update: now,
            cached_layout_mode: None,
            last_terminal_width: 0,
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

    /// Mark the UI as dirty (needs redraw).
    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Check if UI needs redraw and clear the dirty flag.
    fn take_dirty(&mut self) -> bool {
        if self.dirty {
            self.dirty = false;
            true
        } else {
            false
        }
    }

    /// Get cached timestamp or update if expired.
    fn get_cached_timestamp(&mut self) -> String {
        if self.last_timestamp_update.elapsed() >= TIMESTAMP_CACHE_DURATION {
            self.cached_timestamp = Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
            self.last_timestamp_update = Instant::now();
        }
        self.cached_timestamp.clone().unwrap_or_default()
    }

    /// Get layout mode with caching.
    fn get_layout_mode(&mut self, width: u16) -> LayoutMode {
        if self.last_terminal_width != width || self.cached_layout_mode.is_none() {
            self.cached_layout_mode = Some(LayoutMode::from_width(width));
            self.last_terminal_width = width;
        }
        self.cached_layout_mode.unwrap_or(LayoutMode::Narrow)
    }

    /// Check if terminal size changed.
    fn size_changed(&self, area: Rect) -> bool {
        match self.cached_size {
            Some(cached) => cached.width != area.width || cached.height != area.height,
            None => true,
        }
    }

    /// Update cached terminal size.
    fn update_cached_size(&mut self, area: Rect) {
        self.cached_size = Some(area);
    }

    /// Switch to a specific view.
    pub fn switch_view(&mut self, view: View) {
        if self.current_view != view {
            self.previous_view = Some(self.current_view);
            self.current_view = view;
            self.scroll_offset = 0;
            self.mark_dirty();

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
            AppEvent::ShowHelp => {
                self.show_help = true;
                self.mark_dirty();
            }
            AppEvent::HideHelp => {
                self.show_help = false;
                self.mark_dirty();
            }
            AppEvent::Quit => self.should_quit = true,
            AppEvent::ForceQuit => self.should_quit = true,
            AppEvent::Refresh => {
                self.status_message = Some("Refreshed".to_string());
                self.mark_dirty();
            }
            AppEvent::Cancel => {
                if self.show_help {
                    self.show_help = false;
                } else if self.current_view == View::Chat {
                    self.chat_input.clear();
                    self.go_back();
                }
                self.mark_dirty();
            }
            AppEvent::NavigateUp => {
                if self.scroll_offset > 0 {
                    self.scroll_offset -= 1;
                    self.mark_dirty();
                }
            }
            AppEvent::NavigateDown => {
                self.scroll_offset += 1;
                self.mark_dirty();
            }
            AppEvent::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
                self.mark_dirty();
            }
            AppEvent::PageDown => {
                self.scroll_offset += 10;
                self.mark_dirty();
            }
            AppEvent::GoToTop => {
                self.scroll_offset = 0;
                self.mark_dirty();
            }
            AppEvent::GoToBottom => {
                // In a real impl, this would go to the end of the list
                self.scroll_offset = 100;
                self.mark_dirty();
            }
            AppEvent::TextInput(c) => {
                self.chat_input.push(c);
                self.mark_dirty();
            }
            AppEvent::Backspace => {
                self.chat_input.pop();
                self.mark_dirty();
            }
            AppEvent::Submit => {
                if !self.chat_input.is_empty() {
                    self.status_message = Some(format!("Executing: {}", self.chat_input));
                    self.chat_input.clear();
                }
                self.mark_dirty();
            }
            AppEvent::Select | AppEvent::Toggle | AppEvent::FocusNext | AppEvent::FocusPrev => {
                // Panel-specific handling - to be implemented
            }
            AppEvent::SpawnWorker(executor) => {
                self.status_message = Some(format!(
                    "Spawning {} worker...",
                    executor.name()
                ));
                self.mark_dirty();
                // TODO: Implement actual worker spawning
            }
            AppEvent::KillWorker => {
                self.status_message = Some("Kill worker - not yet implemented".to_string());
                self.mark_dirty();
                // TODO: Implement actual worker killing
            }
            AppEvent::OpenConfig => {
                self.status_message = Some("Opening configuration menu...".to_string());
                self.mark_dirty();
                // TODO: Implement config menu
            }
            AppEvent::OpenBudgetConfig => {
                self.status_message = Some("Opening budget configuration...".to_string());
                self.mark_dirty();
                // TODO: Implement budget config
            }
            AppEvent::OpenWorkerConfig => {
                self.status_message = Some("Opening worker configuration...".to_string());
                self.mark_dirty();
                // TODO: Implement worker config
            }
            AppEvent::CycleTheme => {
                let new_theme = self.theme_manager.cycle_theme();
                self.status_message = Some(format!("Theme: {}", new_theme.display_name()));
                self.mark_dirty();
            }
            AppEvent::Update => {
                self.trigger_update();
            }
            AppEvent::None => {}
        }
    }

    /// Trigger forge update/rebuild and restart.
    fn trigger_update(&mut self) {
        use std::process::Command;
        use std::env;

        self.status_message = Some("Rebuilding forge...".to_string());
        self.mark_dirty();

        // Get the forge source directory (assuming we're in a workspace)
        let forge_src = env::var("FORGE_SRC")
            .unwrap_or_else(|_| "/home/coder/forge".to_string());

        // Spawn background rebuild process
        match Command::new("sh")
            .arg("-c")
            .arg(format!(
                "cd {} && cargo build --release 2>&1 | tail -5 && cp target/release/forge ~/.cargo/bin/forge",
                forge_src
            ))
            .spawn()
        {
            Ok(mut child) => {
                // Wait for build to complete
                match child.wait() {
                    Ok(status) if status.success() => {
                        self.status_message = Some("Update complete! Restarting...".to_string());
                        self.mark_dirty();

                        // Give user time to see the message
                        std::thread::sleep(std::time::Duration::from_secs(1));

                        // Exit gracefully and let the user restart manually
                        // (exec in Rust TUI is tricky due to terminal state)
                        self.status_message = Some("Update complete! Please restart forge.".to_string());
                        self.mark_dirty();
                    }
                    Ok(_) => {
                        self.status_message = Some("Update failed! Check cargo build output.".to_string());
                        self.mark_dirty();
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Update error: {}", e));
                        self.mark_dirty();
                    }
                }
            }
            Err(e) => {
                self.status_message = Some(format!("Failed to start update: {}", e));
                self.mark_dirty();
            }
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

    /// The inner event loop with frame-rate limiting and optimized polling.
    fn run_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> AppResult<()> {
        while !self.should_quit {
            let frame_start = Instant::now();

            // Optimized data polling - only poll if interval elapsed
            let needs_poll = self.last_poll_time.elapsed() >= DATA_POLL_INTERVAL;
            if needs_poll {
                let data_changed = self.data_manager.poll_updates();
                self.last_poll_time = Instant::now();

                // Mark dirty if data actually changed
                if data_changed {
                    self.mark_dirty();
                }
            }

            // Only draw if dirty or at minimum rate (timestamp updates every second)
            let needs_redraw = self.take_dirty() ||
                self.last_timestamp_update.elapsed() >= TIMESTAMP_CACHE_DURATION;

            if needs_redraw {
                terminal.draw(|frame| self.draw(frame))?;
            }

            // Calculate remaining time in frame for event handling
            let elapsed = frame_start.elapsed();
            let timeout_for_events = if elapsed < FRAME_DURATION {
                FRAME_DURATION - elapsed
            } else {
                Duration::ZERO
            };

            // Handle events with adaptive timeout
            let event_timeout = if timeout_for_events > Duration::ZERO {
                timeout_for_events
            } else {
                // If frame took too long, use shorter timeout
                Duration::from_millis(10)
            };

            if event::poll(event_timeout)? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key_event(key);
                }
            }

            // Frame-rate limiting: sleep if frame was too fast
            let frame_elapsed = frame_start.elapsed();
            if frame_elapsed < FRAME_DURATION {
                let sleep_time = FRAME_DURATION - frame_elapsed;
                std::thread::sleep(sleep_time);
            }
        }
        Ok(())
    }

    /// Draw the UI.
    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Update cached size if changed
        if self.size_changed(area) {
            self.update_cached_size(area);
        }

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

    /// Draw the header bar with cached timestamp.
    fn draw_header(&mut self, frame: &mut Frame, area: Rect) {
        let now = self.get_cached_timestamp();
        let theme = self.theme_manager.current();
        let title = format!(" FORGE Dashboard - {} ", self.current_view.title());
        let title_len = title.len();

        // Terminal dimensions display
        let dimensions = format!("{}×{}", frame.area().width, frame.area().height);

        // Determine system status from real data
        let (status_text, status_color) = if let Some(err) = self.data_manager.init_error() {
            (format!("[Error: {}]", truncate_status_error(err)), theme.colors.status_error)
        } else if !self.data_manager.is_ready() {
            ("[Loading...]".to_string(), theme.colors.status_warning)
        } else {
            let counts = self.data_manager.worker_counts();
            if counts.unhealthy() > 0 {
                (format!("[{} unhealthy]", counts.unhealthy()), theme.colors.status_warning)
            } else if counts.total == 0 {
                ("[No workers]".to_string(), theme.colors.text_dim)
            } else {
                (format!("[{} workers]", counts.total), theme.colors.status_healthy)
            }
        };

        // Calculate spacing to right-align timestamp, dimensions, and status
        let right_content_len = now.len() + 2 + dimensions.len() + 2 + status_text.len();
        let spacing = area.width.saturating_sub(title_len as u16 + right_content_len as u16 + 2) as usize;

        let header = Paragraph::new(Line::from(vec![
            Span::styled(title, Style::default().fg(theme.colors.header).add_modifier(Modifier::BOLD)),
            Span::raw(" ".repeat(spacing)),
            Span::styled(now, Style::default().fg(theme.colors.text_dim)),
            Span::raw("  "),
            Span::styled(dimensions, Style::default().fg(theme.colors.text_dim)),
            Span::raw("  "),
            Span::styled(status_text, Style::default().fg(status_color)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.colors.border_dim)),
        );

        frame.render_widget(header, area);
    }

    /// Draw the main content area based on current view.
    fn draw_content(&mut self, frame: &mut Frame, area: Rect) {
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
        let theme = self.theme_manager.current();
        let hotkey_style = Style::default().fg(theme.colors.hotkey);
        let hints = vec![
            Span::styled("[o]", hotkey_style),
            Span::raw("Overview "),
            Span::styled("[w]", hotkey_style),
            Span::raw("Workers "),
            Span::styled("[t]", hotkey_style),
            Span::raw("Tasks "),
            Span::styled("[c]", hotkey_style),
            Span::raw("Costs "),
            Span::styled("[m]", hotkey_style),
            Span::raw("Metrics "),
            Span::styled("[l]", hotkey_style),
            Span::raw("Logs "),
            Span::styled("[:]", hotkey_style),
            Span::raw("Chat "),
            Span::styled("[?]", hotkey_style),
            Span::raw("Help "),
            Span::styled("[C]", hotkey_style),
            Span::raw("Theme "),
            Span::styled("[q]", hotkey_style),
            Span::raw("Quit"),
        ];

        let dims_text = format!("{}x{}", area.width, area.height);

        let footer = Paragraph::new(Line::from(hints))
            .style(Style::default().fg(theme.colors.text_dim))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .title(Span::styled(dims_text, Style::default().fg(theme.colors.border_dim)))
                    .title_alignment(ratatui::layout::Alignment::Right),
            );

        frame.render_widget(footer, area);
    }

    /// Draw the Overview/Dashboard view with cached layout mode.
    ///
    /// Layout adapts based on terminal width:
    /// - Ultra-wide (199+): 3-column layout with all 6 panels
    /// - Wide (120-198): 2-column layout with 4 panels
    /// - Narrow (<120): Single-column with stacked panels
    fn draw_overview(&mut self, frame: &mut Frame, area: Rect) {
        let layout_mode = self.get_layout_mode(area.width);

        match layout_mode {
            LayoutMode::UltraWide => self.draw_overview_ultrawide(frame, area),
            LayoutMode::Wide => self.draw_overview_wide(frame, area),
            LayoutMode::Narrow => self.draw_overview_narrow(frame, area),
        }
    }

    /// Draw ultra-wide 3-column layout (199+ cols).
    ///
    /// Layout: 66 | 66 | 65 columns (with borders accounting for 2 chars each)
    /// Left: Workers + Subscriptions (stacked)
    /// Middle: Tasks + Activity (stacked)
    /// Right: Costs + Actions (stacked)
    fn draw_overview_ultrawide(&mut self, frame: &mut Frame, area: Rect) {
        // Calculate column widths: 66 + 66 + 65 = 197, borders use remaining
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(66),
                Constraint::Length(66),
                Constraint::Min(65),
            ])
            .split(area);

        // Each column has 2 panels stacked vertically (50/50)
        let left_panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(columns[0]);

        let middle_panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(columns[1]);

        let right_panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(columns[2]);

        // Left column: Workers + Subscriptions
        let worker_summary = self.data_manager.worker_data.format_worker_pool_summary();
        self.draw_panel(
            frame,
            left_panels[0],
            "Worker Pool",
            &worker_summary,
            self.focus_panel == FocusPanel::WorkerPool,
        );

        let subscriptions_content = crate::subscription_panel::format_subscription_summary(
            &self.data_manager.subscription_data,
        );
        self.draw_panel(
            frame,
            left_panels[1],
            "Subscriptions",
            &subscriptions_content,
            self.focus_panel == FocusPanel::Subscriptions,
        );

        // Middle column: Tasks + Activity
        let task_queue_content = self.data_manager.bead_manager.format_task_queue_summary();
        self.draw_panel(
            frame,
            middle_panels[0],
            "Task Queue",
            &task_queue_content,
            self.focus_panel == FocusPanel::TaskQueue,
        );

        let activity_log = self.data_manager.worker_data.format_activity_log();
        self.draw_panel(
            frame,
            middle_panels[1],
            "Activity Log",
            &activity_log,
            self.focus_panel == FocusPanel::ActivityLog,
        );

        // Right column: Costs + Actions
        let costs_content = self.format_costs_panel();
        self.draw_panel(
            frame,
            right_panels[0],
            "Cost Breakdown",
            &costs_content,
            self.focus_panel == FocusPanel::CostBreakdown,
        );

        // Render Quick Actions panel with widget
        let quick_actions_panel = QuickActionsPanel::new()
            .focused(self.focus_panel == FocusPanel::MetricsCharts);
        frame.render_widget(quick_actions_panel, right_panels[1]);
    }

    /// Draw wide 2-column layout (120-198 cols).
    fn draw_overview_wide(&mut self, frame: &mut Frame, area: Rect) {
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

        // Use real worker data
        let worker_summary = self.data_manager.worker_data.format_worker_pool_summary();

        self.draw_panel(
            frame,
            top_chunks[0],
            "Worker Pool",
            &worker_summary,
            self.focus_panel == FocusPanel::WorkerPool,
        );

        // Subscriptions panel - show subscription usage and reset timers
        let subscriptions_content = crate::subscription_panel::format_subscription_summary(
            &self.data_manager.subscription_data,
        );

        self.draw_panel(
            frame,
            top_chunks[1],
            "Subscriptions",
            &subscriptions_content,
            self.focus_panel == FocusPanel::Subscriptions,
        );

        // Bottom section: Task Queue and Activity Log
        let bottom_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);

        // Task queue - shows real bead data from monitored workspaces
        let task_queue_content = self.data_manager.bead_manager.format_task_queue_summary();

        self.draw_panel(
            frame,
            bottom_chunks[0],
            "Task Queue",
            &task_queue_content,
            self.focus_panel == FocusPanel::TaskQueue,
        );

        // Activity log from real worker data
        let activity_log = self.data_manager.worker_data.format_activity_log();

        self.draw_panel(
            frame,
            bottom_chunks[1],
            "Activity Log",
            &activity_log,
            self.focus_panel == FocusPanel::ActivityLog,
        );
    }

    /// Draw narrow single-column layout (<120 cols).
    fn draw_overview_narrow(&mut self, frame: &mut Frame, area: Rect) {
        // Stack panels vertically, show fewer in constrained space
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(35),
                Constraint::Percentage(35),
                Constraint::Percentage(30),
            ])
            .split(area);

        // Worker Pool (primary focus)
        let worker_summary = self.data_manager.worker_data.format_worker_pool_summary();
        self.draw_panel(
            frame,
            chunks[0],
            "Worker Pool",
            &worker_summary,
            self.focus_panel == FocusPanel::WorkerPool,
        );

        // Task Queue (secondary focus)
        let task_queue_content = self.data_manager.bead_manager.format_task_queue_summary();
        self.draw_panel(
            frame,
            chunks[1],
            "Task Queue",
            &task_queue_content,
            self.focus_panel == FocusPanel::TaskQueue,
        );

        // Activity Log (compact)
        let activity_log = self.data_manager.worker_data.format_activity_log();
        self.draw_panel(
            frame,
            chunks[2],
            "Activity Log",
            &activity_log,
            self.focus_panel == FocusPanel::ActivityLog,
        );
    }

    /// Format the costs panel for the right column in ultra-wide mode.
    fn format_costs_panel(&self) -> String {
        use crate::cost_panel::{format_usd, format_tokens};

        if !self.data_manager.is_ready() {
            return "Loading...".to_string();
        }

        let data = &self.data_manager.cost_data;

        if data.is_loading {
            return "Loading cost data...".to_string();
        }

        if !data.has_data() {
            return "No cost data available.\n\n\
                    Cost tracking requires:\n\
                    - forge-cost database\n\
                    - Worker logs being parsed"
                .to_string();
        }

        let mut lines = Vec::new();

        // Today's costs
        let daily_alert = data.daily_alert();
        lines.push(format!(
            "{} Today: {}",
            daily_alert.icon(),
            format_usd(data.today_total())
        ));
        lines.push(format!("  Calls: {}", data.today_calls()));
        lines.push(format!("  Tokens: {}", format_tokens(data.today_tokens())));
        lines.push(String::new());

        // Monthly budget
        let monthly_alert = data.monthly_alert();
        lines.push(format!(
            "{} Month: {} / {}",
            monthly_alert.icon(),
            format_usd(data.monthly_total),
            format_usd(data.budget.monthly_limit)
        ));

        // Progress bar
        let pct = data.monthly_usage_pct().min(100.0);
        let bar_width: usize = 15;
        let filled = ((pct / 100.0) * bar_width as f64).round() as usize;
        let empty = bar_width.saturating_sub(filled);
        lines.push(format!(
            "  [{}{}] {:.1}%",
            "█".repeat(filled),
            "░".repeat(empty),
            data.monthly_usage_pct()
        ));

        lines.join("\n")
    }

    /// Draw the Workers view.
    fn draw_workers(&self, frame: &mut Frame, area: Rect) {
        let worker_table = self.data_manager.worker_data.format_worker_table();

        self.draw_panel(
            frame,
            area,
            "Worker Pool Management",
            &worker_table,
            true,
        );
    }

    /// Draw the Tasks view.
    fn draw_tasks(&self, frame: &mut Frame, area: Rect) {
        let content = self.data_manager.bead_manager.format_task_queue_full();

        self.draw_panel(
            frame,
            area,
            "Task Queue & Bead Management",
            &content,
            true,
        );
    }

    /// Draw the Costs view.
    fn draw_costs(&self, frame: &mut Frame, area: Rect) {
        // Use the CostPanel widget for rich analytics display
        let cost_panel = CostPanel::new(&self.data_manager.cost_data)
            .focused(self.focus_panel == FocusPanel::CostBreakdown);
        frame.render_widget(cost_panel, area);
    }

    /// Draw the Metrics view.
    fn draw_metrics(&self, frame: &mut Frame, area: Rect) {
        // Use the MetricsPanel widget for rich performance visualization
        let metrics_panel = MetricsPanel::new(&self.data_manager.metrics_data)
            .focused(self.focus_panel == FocusPanel::MetricsCharts);
        frame.render_widget(metrics_panel, area);
    }

    /// Draw the Logs view.
    fn draw_logs(&self, frame: &mut Frame, area: Rect) {
        // Use real activity log from worker data
        let activity_log = self.data_manager.worker_data.format_activity_log();

        self.draw_panel(
            frame,
            area,
            "Activity Log",
            &activity_log,
            true,
        );
    }

    /// Draw the Chat view.
    fn draw_chat(&self, frame: &mut Frame, area: Rect) {
        let theme = self.theme_manager.current();
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
            Style::default().fg(theme.colors.hotkey)
        } else {
            Style::default().fg(theme.colors.text_dim)
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
                    .border_style(Style::default().fg(theme.colors.header))
                    .title(" Input "),
            );

        frame.render_widget(input, chunks[1]);
    }

    /// Draw a panel with optional highlight.
    fn draw_panel(&self, frame: &mut Frame, area: Rect, title: &str, content: &str, focused: bool) {
        let theme = self.theme_manager.current();
        let border_style = if focused {
            Style::default().fg(theme.colors.header)
        } else {
            Style::default().fg(theme.colors.border_dim)
        };

        let title_style = if focused {
            Style::default().fg(theme.colors.header).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.colors.text)
        };

        let panel = Paragraph::new(content)
            .style(Style::default().fg(theme.colors.text))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(Span::styled(format!(" {} ", title), title_style)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(panel, area);
    }

    /// Poll for data updates. Called from the main loop.
    #[allow(dead_code)]
    pub fn poll_data(&mut self) {
        self.data_manager.poll_updates();
    }

    /// Draw the help overlay.
    fn draw_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let theme = self.theme_manager.current();
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
  Ctrl+U   Update forge (rebuild & restart)
  r        Refresh
  C        Cycle theme

Navigation:
  ↑ k      Move up
  ↓ j      Move down
  PgUp     Page up
  PgDn     Page down

Press any key to close this help.";

        let help = Paragraph::new(help_text)
            .style(Style::default().fg(theme.colors.text))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.colors.header))
                    .title(Span::styled(
                        " Help ",
                        Style::default().fg(theme.colors.header).add_modifier(Modifier::BOLD),
                    ))
                    .style(Style::default().bg(Color::Black)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(help, overlay_area);
    }
}

/// Truncate an error message for status bar display.
fn truncate_status_error(err: &str) -> String {
    if err.len() <= 20 {
        err.to_string()
    } else {
        format!("{}...", &err[..17])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    /// Helper to create a test terminal with specified dimensions
    fn test_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
        let backend = TestBackend::new(width, height);
        Terminal::new(backend).unwrap()
    }

    /// Helper to render app and get the buffer
    fn render_app(app: &mut App, width: u16, height: u16) -> Buffer {
        let mut terminal = test_terminal(width, height);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// Check if a buffer contains a specific string
    fn buffer_contains(buffer: &Buffer, text: &str) -> bool {
        let content = buffer_to_string(buffer);
        content.contains(text)
    }

    /// Convert buffer to string for debugging/searching
    fn buffer_to_string(buffer: &Buffer) -> String {
        let area = buffer.area;
        let mut result = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                result.push(buffer[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            result.push('\n');
        }
        result
    }

    // ============================================================
    // Dashboard Panel Rendering Tests
    // ============================================================

    #[test]
    fn test_overview_renders_worker_pool_panel() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 100, 30);

        // Check Worker Pool panel title appears
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "Overview should render Worker Pool panel"
        );
    }

    #[test]
    fn test_overview_renders_utilization_panel() {
        let mut app = App::new();
        // Use wide layout (120+ cols) to ensure Subscriptions panel is visible
        let buffer = render_app(&mut app, 140, 30);

        assert!(
            buffer_contains(&buffer, "Subscriptions"),
            "Overview should render Subscriptions panel in wide layout"
        );
    }

    #[test]
    fn test_overview_renders_task_queue_panel() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "Task Queue"),
            "Overview should render Task Queue panel"
        );
    }

    #[test]
    fn test_overview_renders_activity_log_panel() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "Activity Log"),
            "Overview should render Activity Log panel"
        );
    }

    #[test]
    fn test_costs_view_renders_cost_analytics_panel() {
        let mut app = App::new();
        app.switch_view(View::Costs);
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "Cost Analytics"),
            "Costs view should render Cost Analytics panel"
        );
    }

    #[test]
    fn test_metrics_view_renders_performance_panel() {
        let mut app = App::new();
        app.switch_view(View::Metrics);
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "Performance Metrics"),
            "Metrics view should render Performance Metrics panel"
        );
    }

    #[test]
    fn test_all_six_panel_types_render() {
        // Test that all 6 FocusPanel types have corresponding views that render
        let mut app = App::new();

        // 1. Worker Pool (Overview - switch to Workers first, then back to Overview)
        // The app starts in Overview, so switch_view(Overview) won't change focus
        app.switch_view(View::Workers);
        app.switch_view(View::Overview);
        assert_eq!(app.focus_panel(), FocusPanel::WorkerPool);
        // Use ultra-wide layout (199+ cols) to ensure all 6 panels are visible
        let buffer = render_app(&mut app, 199, 38);
        assert!(buffer_contains(&buffer, "Worker Pool"));

        // 2. Subscriptions (part of Overview) - visible in ultra-wide layout
        assert!(buffer_contains(&buffer, "Subscriptions"));

        // 3. Task Queue (Tasks view)
        app.switch_view(View::Tasks);
        assert_eq!(app.focus_panel(), FocusPanel::TaskQueue);
        let buffer = render_app(&mut app, 100, 30);
        assert!(buffer_contains(&buffer, "Task Queue"));

        // 4. Activity Log (Logs view)
        app.switch_view(View::Logs);
        assert_eq!(app.focus_panel(), FocusPanel::ActivityLog);
        let buffer = render_app(&mut app, 100, 30);
        assert!(buffer_contains(&buffer, "Activity Log"));

        // 5. Cost Breakdown (Costs view)
        app.switch_view(View::Costs);
        assert_eq!(app.focus_panel(), FocusPanel::CostBreakdown);
        let buffer = render_app(&mut app, 100, 30);
        assert!(buffer_contains(&buffer, "Cost Analytics"));

        // 6. Metrics Charts (Metrics view)
        app.switch_view(View::Metrics);
        assert_eq!(app.focus_panel(), FocusPanel::MetricsCharts);
        let buffer = render_app(&mut app, 100, 30);
        assert!(buffer_contains(&buffer, "Performance Metrics"));
    }

    // ============================================================
    // Border Rendering Tests
    // ============================================================

    #[test]
    fn test_panels_render_with_borders() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 100, 30);

        // Unicode box drawing characters used by ratatui
        // Check for corner characters that indicate borders
        let content = buffer_to_string(&buffer);

        // Should contain horizontal box-drawing characters (─)
        assert!(
            content.contains('─') || content.contains('-'),
            "Panels should render with horizontal border lines"
        );

        // Should contain vertical box-drawing characters (│)
        assert!(
            content.contains('│') || content.contains('|'),
            "Panels should render with vertical border lines"
        );
    }

    #[test]
    fn test_header_renders_with_borders() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "FORGE Dashboard"),
            "Header should contain FORGE Dashboard title"
        );
    }

    #[test]
    fn test_footer_renders_hotkey_hints() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "[o]"),
            "Footer should show Overview hotkey"
        );
        assert!(
            buffer_contains(&buffer, "[w]"),
            "Footer should show Workers hotkey"
        );
        assert!(
            buffer_contains(&buffer, "[q]"),
            "Footer should show Quit hotkey"
        );
    }

    // ============================================================
    // Layout Adaptation Tests
    // ============================================================

    #[test]
    fn test_layout_adapts_to_small_terminal() {
        let mut app = App::new();

        // Very small terminal
        let buffer = render_app(&mut app, 40, 15);

        // Should still render without panic
        assert!(buffer.area.width == 40);
        assert!(buffer.area.height == 15);

        // Should still show some content
        let content = buffer_to_string(&buffer);
        assert!(!content.trim().is_empty(), "Should render content even in small terminal");
    }

    #[test]
    fn test_layout_adapts_to_large_terminal() {
        let mut app = App::new();

        // Large terminal
        let buffer = render_app(&mut app, 200, 60);

        assert!(buffer.area.width == 200);
        assert!(buffer.area.height == 60);

        // All panels should still be visible
        assert!(buffer_contains(&buffer, "Worker Pool"));
        assert!(buffer_contains(&buffer, "Subscriptions"));
    }

    #[test]
    fn test_layout_adapts_to_wide_terminal() {
        let mut app = App::new();

        // Wide but short terminal
        let buffer = render_app(&mut app, 200, 20);

        assert!(buffer.area.width == 200);
        assert!(buffer.area.height == 20);

        // Should render header and some content
        assert!(buffer_contains(&buffer, "FORGE Dashboard"));
    }

    #[test]
    fn test_layout_adapts_to_tall_terminal() {
        let mut app = App::new();

        // Narrow but tall terminal
        let buffer = render_app(&mut app, 60, 50);

        assert!(buffer.area.width == 60);
        assert!(buffer.area.height == 50);

        // Should render content
        assert!(buffer_contains(&buffer, "FORGE Dashboard"));
    }

    #[test]
    fn test_minimum_viable_terminal_size() {
        let mut app = App::new();

        // Minimum size that should still render something
        let buffer = render_app(&mut app, 20, 10);

        // Should not panic and should produce some output
        assert!(buffer.area.width == 20);
        assert!(buffer.area.height == 10);
    }

    // ============================================================
    // Panel Content Tests
    // ============================================================

    #[test]
    fn test_worker_pool_shows_worker_counts() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 100, 30);

        // Worker pool should show worker statistics
        assert!(
            buffer_contains(&buffer, "active") || buffer_contains(&buffer, "idle") || buffer_contains(&buffer, "Total"),
            "Worker Pool should display worker counts"
        );
    }

    #[test]
    fn test_task_queue_shows_priority_indicators() {
        let mut app = App::new();
        app.switch_view(View::Tasks);
        let buffer = render_app(&mut app, 100, 30);

        // Task queue should show priority markers
        assert!(
            buffer_contains(&buffer, "P0") || buffer_contains(&buffer, "P1") || buffer_contains(&buffer, "Ready"),
            "Task Queue should display priority indicators"
        );
    }

    #[test]
    fn test_costs_view_shows_placeholder() {
        let mut app = App::new();
        app.switch_view(View::Costs);
        let buffer = render_app(&mut app, 100, 30);

        // Costs view shows placeholder since cost tracking isn't implemented
        assert!(
            buffer_contains(&buffer, "Cost") || buffer_contains(&buffer, "tracking") || buffer_contains(&buffer, "Loading"),
            "Costs view should display cost-related content"
        );
    }

    #[test]
    fn test_logs_view_shows_activity() {
        let mut app = App::new();
        app.switch_view(View::Logs);
        let buffer = render_app(&mut app, 100, 30);

        // Logs view should show activity log panel title and content
        assert!(
            buffer_contains(&buffer, "Activity Log") || buffer_contains(&buffer, "No recent activity") || buffer_contains(&buffer, "Loading"),
            "Logs view should display activity log"
        );
    }

    // ============================================================
    // View-Specific Rendering Tests
    // ============================================================

    #[test]
    fn test_workers_view_renders_table() {
        let mut app = App::new();
        app.switch_view(View::Workers);
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "Worker Pool Management"),
            "Workers view should show management panel"
        );
        assert!(
            buffer_contains(&buffer, "Worker ID") || buffer_contains(&buffer, "Model") || buffer_contains(&buffer, "Status"),
            "Workers view should show table headers"
        );
    }

    #[test]
    fn test_chat_view_renders_input_field() {
        let mut app = App::new();
        app.switch_view(View::Chat);
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "Chat") || buffer_contains(&buffer, "Input"),
            "Chat view should show chat interface"
        );
    }

    // ============================================================
    // Help Overlay Tests
    // ============================================================

    #[test]
    fn test_help_overlay_renders() {
        let mut app = App::new();
        app.handle_app_event(AppEvent::ShowHelp);
        let buffer = render_app(&mut app, 100, 40);

        assert!(app.show_help(), "Help should be visible");
        assert!(
            buffer_contains(&buffer, "Help") || buffer_contains(&buffer, "Hotkey"),
            "Help overlay should render"
        );
    }

    #[test]
    fn test_help_overlay_shows_navigation_keys() {
        let mut app = App::new();
        app.handle_app_event(AppEvent::ShowHelp);
        let buffer = render_app(&mut app, 100, 40);

        // Help should show view navigation keys
        let content = buffer_to_string(&buffer);
        assert!(
            content.contains("Tab") || content.contains("Esc") || content.contains("Navigation"),
            "Help overlay should show navigation keys"
        );
    }

    // ============================================================
    // Focus Highlighting Tests
    // ============================================================

    #[test]
    fn test_focused_panel_is_highlighted() {
        let mut app = App::new();

        // Initial focus is None (no highlight)
        assert_eq!(app.focus_panel(), FocusPanel::None);
        assert!(!app.focus_panel().is_highlighted());

        // After switching view, focus is set and highlighted
        app.switch_view(View::Workers);
        assert_eq!(app.focus_panel(), FocusPanel::WorkerPool);
        assert!(app.focus_panel().is_highlighted());
    }

    #[test]
    fn test_focus_changes_with_view() {
        let mut app = App::new();

        // Initial state has no focus
        assert_eq!(app.focus_panel(), FocusPanel::None);

        // After switching views, each view sets appropriate focus
        // Note: switch_view only sets focus when view actually changes,
        // so we need to switch to a different view first for Overview
        app.switch_view(View::Workers);
        assert_eq!(app.focus_panel(), FocusPanel::WorkerPool);

        // Now test each view has correct focus when switched to
        let view_focus_pairs = [
            (View::Tasks, FocusPanel::TaskQueue),
            (View::Costs, FocusPanel::CostBreakdown),
            (View::Metrics, FocusPanel::MetricsCharts),
            (View::Logs, FocusPanel::ActivityLog),
            (View::Chat, FocusPanel::ChatInput),
            (View::Overview, FocusPanel::WorkerPool),
            (View::Workers, FocusPanel::WorkerPool),
        ];

        for (view, expected_focus) in view_focus_pairs {
            app.switch_view(view);
            assert_eq!(
                app.focus_panel(),
                expected_focus,
                "View {:?} should have focus {:?}",
                view,
                expected_focus
            );
        }
    }

    // ============================================================
    // Original Tests
    // ============================================================

    #[test]
    fn test_app_creation() {
        let mut app = App::new();
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

    // ============================================================
    // 3-Column Layout Tests (Ultra-Wide Mode)
    // ============================================================

    #[test]
    fn test_ultrawide_layout_renders_all_six_panels() {
        let mut app = App::new();
        // Ultra-wide: 199x38 terminal
        let buffer = render_app(&mut app, 199, 38);

        // All 6 panels should be visible in ultra-wide mode
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "Ultra-wide layout should show Worker Pool panel"
        );
        assert!(
            buffer_contains(&buffer, "Subscriptions"),
            "Ultra-wide layout should show Subscriptions panel"
        );
        assert!(
            buffer_contains(&buffer, "Task Queue"),
            "Ultra-wide layout should show Task Queue panel"
        );
        assert!(
            buffer_contains(&buffer, "Activity Log"),
            "Ultra-wide layout should show Activity Log panel"
        );
        assert!(
            buffer_contains(&buffer, "Cost Breakdown"),
            "Ultra-wide layout should show Cost Breakdown panel"
        );
        assert!(
            buffer_contains(&buffer, "Quick Actions"),
            "Ultra-wide layout should show Quick Actions panel"
        );
    }

    #[test]
    fn test_ultrawide_layout_at_exact_boundary() {
        let mut app = App::new();
        // Exactly 199 cols - should trigger ultra-wide
        let buffer = render_app(&mut app, 199, 38);

        assert!(
            buffer_contains(&buffer, "Cost Breakdown"),
            "At 199 cols, should use ultra-wide layout with Cost Breakdown panel"
        );
        assert!(
            buffer_contains(&buffer, "Quick Actions"),
            "At 199 cols, should use ultra-wide layout with Quick Actions panel"
        );
    }

    #[test]
    fn test_wide_layout_at_boundary_below_ultrawide() {
        let mut app = App::new();
        // 198 cols - should NOT trigger ultra-wide, just wide
        let buffer = render_app(&mut app, 198, 38);

        // Should have 4 panels (wide mode)
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "Wide layout should show Worker Pool"
        );
        assert!(
            buffer_contains(&buffer, "Subscriptions"),
            "Wide layout should show Subscriptions"
        );
        assert!(
            buffer_contains(&buffer, "Task Queue"),
            "Wide layout should show Task Queue"
        );
        assert!(
            buffer_contains(&buffer, "Activity Log"),
            "Wide layout should show Activity Log"
        );

        // Should NOT have the right column panels
        assert!(
            !buffer_contains(&buffer, "Cost Breakdown"),
            "Wide layout should NOT show Cost Breakdown panel"
        );
        assert!(
            !buffer_contains(&buffer, "Quick Actions"),
            "Wide layout should NOT show Quick Actions panel"
        );
    }

    #[test]
    fn test_narrow_layout_below_wide_threshold() {
        let mut app = App::new();
        // 119 cols - should trigger narrow mode
        let buffer = render_app(&mut app, 119, 30);

        // Should still show essential panels
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "Narrow layout should show Worker Pool"
        );
        assert!(
            buffer_contains(&buffer, "Task Queue"),
            "Narrow layout should show Task Queue"
        );
        assert!(
            buffer_contains(&buffer, "Activity Log"),
            "Narrow layout should show Activity Log"
        );

        // Should NOT show secondary panels
        assert!(
            !buffer_contains(&buffer, "Cost Breakdown"),
            "Narrow layout should NOT show Cost Breakdown"
        );
    }

    #[test]
    fn test_wide_layout_at_wide_threshold() {
        let mut app = App::new();
        // 120 cols - exactly at wide threshold
        let buffer = render_app(&mut app, 120, 30);

        // Should have 4 panels (wide mode)
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "Wide layout at threshold should show Worker Pool"
        );
        assert!(
            buffer_contains(&buffer, "Subscriptions"),
            "Wide layout at threshold should show Subscriptions"
        );
    }

    #[test]
    fn test_layout_mode_detection() {
        use crate::view::LayoutMode;

        // Ultra-wide: 199+
        assert_eq!(LayoutMode::from_width(199), LayoutMode::UltraWide);
        assert_eq!(LayoutMode::from_width(250), LayoutMode::UltraWide);

        // Wide: 120-198
        assert_eq!(LayoutMode::from_width(198), LayoutMode::Wide);
        assert_eq!(LayoutMode::from_width(150), LayoutMode::Wide);
        assert_eq!(LayoutMode::from_width(120), LayoutMode::Wide);

        // Narrow: <120
        assert_eq!(LayoutMode::from_width(119), LayoutMode::Narrow);
        assert_eq!(LayoutMode::from_width(80), LayoutMode::Narrow);
        assert_eq!(LayoutMode::from_width(40), LayoutMode::Narrow);
    }

    #[test]
    fn test_layout_min_height_requirements() {
        use crate::view::LayoutMode;

        assert_eq!(LayoutMode::UltraWide.min_height(), 38);
        assert_eq!(LayoutMode::Wide.min_height(), 30);
        assert_eq!(LayoutMode::Narrow.min_height(), 20);
    }

    #[test]
    fn test_ultrawide_renders_without_panic_at_various_heights() {
        let mut app = App::new();

        // Test various heights with ultra-wide width
        for height in [20, 30, 38, 50, 60, 100] {
            let buffer = render_app(&mut app, 199, height);
            assert_eq!(buffer.area.height, height);
            // Should render something without panic
            assert!(buffer_contains(&buffer, "FORGE Dashboard"));
        }
    }

    #[test]
    fn test_ultrawide_shows_action_hints() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 199, 38);

        // Quick Actions panel should show worker action hints
        let content = buffer_to_string(&buffer);
        assert!(
            content.contains("Spawn") || content.contains("[G]") || content.contains("Worker"),
            "Quick Actions panel should show action hints"
        );
    }

    #[test]
    fn test_ultrawide_shows_cost_placeholders() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 199, 38);

        // Cost Breakdown panel should show cost placeholders
        let content = buffer_to_string(&buffer);
        assert!(
            content.contains("Cost") || content.contains("Budget") || content.contains("$"),
            "Cost Breakdown panel should show cost-related content"
        );
    }

    #[test]
    fn test_graceful_degradation_sequence() {
        let mut app = App::new();

        // Test the degradation sequence: ultra-wide -> wide -> narrow
        // Each step down should still render without errors and show appropriate panels

        // Ultra-wide (199): 6 panels
        let buffer_ultrawide = render_app(&mut app, 199, 38);
        assert!(buffer_contains(&buffer_ultrawide, "Cost Breakdown"));
        assert!(buffer_contains(&buffer_ultrawide, "Quick Actions"));

        // Wide (150): 4 panels
        let buffer_wide = render_app(&mut app, 150, 30);
        assert!(buffer_contains(&buffer_wide, "Worker Pool"));
        assert!(buffer_contains(&buffer_wide, "Task Queue"));
        assert!(!buffer_contains(&buffer_wide, "Cost Breakdown"));

        // Narrow (80): 3 panels stacked
        let buffer_narrow = render_app(&mut app, 80, 25);
        assert!(buffer_contains(&buffer_narrow, "Worker Pool"));
        assert!(buffer_contains(&buffer_narrow, "Task Queue"));
        assert!(buffer_contains(&buffer_narrow, "Activity Log"));
    }
}
