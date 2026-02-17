//! Interactive TUI wizard for onboarding.
//!
//! Provides an interactive setup flow showing detected CLI tools and allowing
//! the user to select which one to configure.

use std::io;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use thiserror::Error;

use crate::detection::{CliToolDetection, ToolStatus};

/// Errors that can occur during wizard flow.
#[derive(Debug, Error)]
pub enum WizardError {
    #[error("User cancelled setup")]
    UserCancelled,

    #[error("TUI rendering failed: {0}")]
    RenderFailed(String),

    #[error("No CLI tools available")]
    NoToolsAvailable,

    #[error("Terminal I/O error: {0}")]
    IoError(#[from] io::Error),
}

/// Result type for wizard operations.
pub type Result<T> = std::result::Result<T, WizardError>;

/// Represents the current selection focus in the wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WizardFocus {
    /// Focus is on the tool list
    ToolList,
    /// Focus is on the action buttons
    Buttons,
}

/// Available action buttons in the wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WizardButton {
    Continue,
    ManualSetup,
    Quit,
}

impl WizardButton {
    fn all() -> [WizardButton; 3] {
        [
            WizardButton::Continue,
            WizardButton::ManualSetup,
            WizardButton::Quit,
        ]
    }

    fn label(&self) -> &'static str {
        match self {
            WizardButton::Continue => "Continue",
            WizardButton::ManualSetup => "Manual Setup",
            WizardButton::Quit => "Quit",
        }
    }
}

/// Internal state for the wizard TUI.
struct WizardState {
    /// Available CLI tools detected
    tools: Vec<CliToolDetection>,
    /// Currently selected tool index
    selected_tool: usize,
    /// Current focus area
    focus: WizardFocus,
    /// Currently selected button (when focus is on buttons)
    selected_button: usize,
    /// List state for ratatui
    list_state: ListState,
    /// Whether the wizard should exit
    should_exit: bool,
    /// The result to return (Some = selection made, None = cancelled)
    result: Option<Option<CliToolDetection>>,
}

impl WizardState {
    fn new(tools: Vec<CliToolDetection>) -> Self {
        let mut list_state = ListState::default();
        if !tools.is_empty() {
            list_state.select(Some(0));
        }

        Self {
            tools,
            selected_tool: 0,
            focus: WizardFocus::ToolList,
            selected_button: 0, // Default to "Continue"
            list_state,
            should_exit: false,
            result: None,
        }
    }

    fn handle_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Up | KeyCode::Char('k') => self.move_up(),
            KeyCode::Down | KeyCode::Char('j') => self.move_down(),
            KeyCode::Left | KeyCode::Char('h') => self.move_left(),
            KeyCode::Right | KeyCode::Char('l') => self.move_right(),
            KeyCode::Tab => self.toggle_focus(),
            KeyCode::BackTab => self.toggle_focus_reverse(),
            KeyCode::Enter => self.activate_selection(),
            KeyCode::Esc | KeyCode::Char('q') => self.quit(),
            _ => {}
        }
    }

    fn move_up(&mut self) {
        if self.focus == WizardFocus::ToolList && !self.tools.is_empty() {
            if self.selected_tool > 0 {
                self.selected_tool -= 1;
            } else {
                self.selected_tool = self.tools.len() - 1; // Wrap around
            }
            self.list_state.select(Some(self.selected_tool));
        }
    }

    fn move_down(&mut self) {
        if self.focus == WizardFocus::ToolList && !self.tools.is_empty() {
            if self.selected_tool < self.tools.len() - 1 {
                self.selected_tool += 1;
            } else {
                self.selected_tool = 0; // Wrap around
            }
            self.list_state.select(Some(self.selected_tool));
        }
    }

    fn move_left(&mut self) {
        if self.focus == WizardFocus::Buttons && self.selected_button > 0 {
            self.selected_button -= 1;
        }
    }

    fn move_right(&mut self) {
        if self.focus == WizardFocus::Buttons {
            let buttons = WizardButton::all();
            if self.selected_button < buttons.len() - 1 {
                self.selected_button += 1;
            }
        }
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            WizardFocus::ToolList => WizardFocus::Buttons,
            WizardFocus::Buttons => WizardFocus::ToolList,
        };
    }

    fn toggle_focus_reverse(&mut self) {
        // Same as toggle_focus since we only have two areas
        self.toggle_focus();
    }

    fn activate_selection(&mut self) {
        match self.focus {
            WizardFocus::ToolList => {
                // Select the current tool and continue
                if !self.tools.is_empty() {
                    let selected = self.tools[self.selected_tool].clone();
                    self.result = Some(Some(selected));
                    self.should_exit = true;
                }
            }
            WizardFocus::Buttons => {
                let buttons = WizardButton::all();
                let button = buttons[self.selected_button];
                match button {
                    WizardButton::Continue => {
                        // Use currently selected tool
                        if !self.tools.is_empty() {
                            let selected = self.tools[self.selected_tool].clone();
                            self.result = Some(Some(selected));
                        } else {
                            self.result = Some(None);
                        }
                        self.should_exit = true;
                    }
                    WizardButton::ManualSetup => {
                        // Skip auto-configuration
                        self.result = Some(None);
                        self.should_exit = true;
                    }
                    WizardButton::Quit => {
                        self.quit();
                    }
                }
            }
        }
    }

    fn quit(&mut self) {
        self.should_exit = true;
        self.result = None; // Signal cancellation (will return Err(UserCancelled))
    }
}

/// Run the interactive onboarding wizard.
///
/// Shows detected tools and lets the user select which one to configure.
/// Returns the selected tool or None if user chose "Manual Setup".
///
/// # Arguments
///
/// * `tools` - List of detected CLI tools to display
///
/// # Returns
///
/// * `Ok(Some(tool))` - User selected a tool
/// * `Ok(None)` - User chose "Manual Setup" to skip auto-configuration
/// * `Err(WizardError::UserCancelled)` - User chose "Quit" or pressed Escape
/// * `Err(WizardError::NoToolsAvailable)` - No tools were provided (empty list)
///
/// # Examples
///
/// ```no_run
/// use forge_init::wizard::run_wizard;
/// use forge_init::detection::detect_cli_tools;
///
/// let tools = detect_cli_tools().unwrap();
/// match run_wizard(tools) {
///     Ok(Some(tool)) => println!("Selected: {}", tool.name),
///     Ok(None) => println!("Manual setup selected"),
///     Err(e) => eprintln!("Wizard error: {}", e),
/// }
/// ```
pub fn run_wizard(tools: Vec<CliToolDetection>) -> Result<Option<CliToolDetection>> {
    // Handle empty tools case
    if tools.is_empty() {
        return Err(WizardError::NoToolsAvailable);
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create wizard state
    let mut state = WizardState::new(tools);

    // Main event loop
    let result = run_wizard_loop(&mut terminal, &mut state);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Return the result
    match result {
        Ok(()) => state.result.ok_or(WizardError::UserCancelled),
        Err(e) => Err(e),
    }
}

/// Main event loop for the wizard.
fn run_wizard_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut WizardState,
) -> Result<()> {
    loop {
        // Draw the UI
        terminal
            .draw(|f| draw_wizard(f, state))
            .map_err(|e| WizardError::RenderFailed(e.to_string()))?;

        // Check if we should exit
        if state.should_exit {
            break;
        }

        // Handle events with a short poll timeout
        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            // Only handle key press events (not release)
            if key.kind == KeyEventKind::Press {
                state.handle_key(key.code);
            }
        }
    }

    Ok(())
}

/// Draw the wizard UI.
fn draw_wizard(f: &mut Frame, state: &mut WizardState) {
    let size = f.area();

    // Clear the background
    f.render_widget(Clear, size);

    // Calculate centered box dimensions
    let box_width = size.width.min(60);
    let box_height = size.height.min(20 + state.tools.len() as u16 * 2);

    let centered = centered_rect(box_width, box_height, size);

    // Draw the main border
    let outer_block = Block::default()
        .title(" FORGE First Run Setup ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().fg(Color::Cyan));

    f.render_widget(outer_block.clone(), centered);

    // Inner area for content
    let inner = outer_block.inner(centered);

    // Layout: title, tools list, selection indicator, buttons, help
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2), // Title
            Constraint::Min(4),    // Tools list
            Constraint::Length(2), // Selected tool indicator
            Constraint::Length(3), // Buttons
            Constraint::Length(1), // Help text
        ])
        .split(inner);

    // Title section
    let title = Paragraph::new("Detected CLI Tools:")
        .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD));
    f.render_widget(title, chunks[0]);

    // Tools list
    draw_tools_list(f, state, chunks[1]);

    // Selected tool indicator
    draw_selection_indicator(f, state, chunks[2]);

    // Buttons
    draw_buttons(f, state, chunks[3]);

    // Help text
    draw_help(f, state, chunks[4]);
}

/// Draw the tools list.
fn draw_tools_list(f: &mut Frame, state: &mut WizardState, area: Rect) {
    let items: Vec<ListItem> = state
        .tools
        .iter()
        .enumerate()
        .map(|(i, tool)| {
            let status_icon = match tool.status {
                ToolStatus::Ready => "✅",
                ToolStatus::MissingApiKey => "⚠️",
                ToolStatus::IncompatibleVersion => "❌",
                ToolStatus::NotExecutable => "❌",
            };

            // First line: icon, name, version, path
            let version = tool.version.as_deref().unwrap_or("unknown");
            let path = tool.binary_path.display();
            let line1 = Line::from(vec![
                Span::raw(format!("  {} ", status_icon)),
                Span::styled(
                    format!("{} ", tool.name),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("(v{}) ", version), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("- {}", path), Style::default().fg(Color::DarkGray)),
            ]);

            // Second line: API key status (if relevant)
            let line2 = if tool.api_key_required {
                let (key_icon, key_msg, key_color) = if tool.api_key_detected {
                    (
                        "✅",
                        format!(
                            "Found ({})",
                            tool.api_key_env_var.as_deref().unwrap_or("API_KEY")
                        ),
                        Color::Green,
                    )
                } else {
                    (
                        "❌",
                        format!(
                            "Missing ({})",
                            tool.api_key_env_var.as_deref().unwrap_or("API_KEY")
                        ),
                        Color::Red,
                    )
                };
                Line::from(vec![
                    Span::raw("     API Key: "),
                    Span::styled(format!("{} {}", key_icon, key_msg), Style::default().fg(key_color)),
                ])
            } else {
                Line::from(vec![
                    Span::raw("     "),
                    Span::styled("(CLI handles authentication)", Style::default().fg(Color::DarkGray)),
                ])
            };

            // Highlight selected item
            let is_selected = i == state.selected_tool && state.focus == WizardFocus::ToolList;
            let style = if is_selected {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else {
                Style::default()
            };

            ListItem::new(Text::from(vec![line1, line2])).style(style)
        })
        .collect();

    let list_block = Block::default()
        .borders(Borders::NONE);

    let list = List::new(items).block(list_block);

    f.render_stateful_widget(list, area, &mut state.list_state);
}

/// Draw the selected tool indicator.
fn draw_selection_indicator(f: &mut Frame, state: &WizardState, area: Rect) {
    let selected_name = if !state.tools.is_empty() {
        let tool = &state.tools[state.selected_tool];
        let recommended = if state.selected_tool == 0 && tool.status == ToolStatus::Ready {
            " (Recommended)"
        } else {
            ""
        };
        format!("Select chat backend: [{}{}]", tool.name, recommended)
    } else {
        "No tools available".to_string()
    };

    let indicator = Paragraph::new(selected_name)
        .style(Style::default().fg(Color::Yellow))
        .alignment(Alignment::Center);

    f.render_widget(indicator, area);
}

/// Draw the action buttons.
fn draw_buttons(f: &mut Frame, state: &WizardState, area: Rect) {
    let buttons = WizardButton::all();
    let button_width = 14;
    let total_width = buttons.len() * button_width + (buttons.len() - 1) * 2; // 2 spaces between buttons
    let start_x = area.x + (area.width.saturating_sub(total_width as u16)) / 2;

    for (i, button) in buttons.iter().enumerate() {
        let x = start_x + (i * (button_width + 2)) as u16;
        let button_area = Rect::new(x, area.y, button_width as u16, 3);

        let is_selected = i == state.selected_button && state.focus == WizardFocus::Buttons;
        let style = if is_selected {
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let button_block = Block::default()
            .borders(Borders::ALL)
            .border_type(if is_selected {
                BorderType::Thick
            } else {
                BorderType::Rounded
            })
            .style(style);

        let button_text = Paragraph::new(button.label())
            .alignment(Alignment::Center)
            .style(style)
            .block(button_block);

        f.render_widget(button_text, button_area);
    }
}

/// Draw the help text.
fn draw_help(f: &mut Frame, state: &WizardState, area: Rect) {
    let help_text = match state.focus {
        WizardFocus::ToolList => "↑/↓: Navigate | Tab: Switch to buttons | Enter: Select | q: Quit",
        WizardFocus::Buttons => "←/→: Navigate | Tab: Switch to list | Enter: Activate | q: Quit",
    };

    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);

    f.render_widget(help, area);
}

/// Helper function to create a centered rectangle.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;

    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_tool(name: &str, status: ToolStatus) -> CliToolDetection {
        CliToolDetection::new(name, PathBuf::from(format!("/usr/bin/{}", name)))
            .with_version("1.0.0")
            .with_headless_support(true)
            .with_skip_permissions(true)
            .with_api_key(false, None, true)
            .with_status(status)
    }

    #[test]
    fn test_wizard_state_navigation() {
        let tools = vec![
            create_test_tool("claude-code", ToolStatus::Ready),
            create_test_tool("opencode", ToolStatus::MissingApiKey),
        ];
        let mut state = WizardState::new(tools);

        // Initial state
        assert_eq!(state.selected_tool, 0);
        assert_eq!(state.focus, WizardFocus::ToolList);

        // Move down
        state.handle_key(KeyCode::Down);
        assert_eq!(state.selected_tool, 1);

        // Move down again (wraps to 0)
        state.handle_key(KeyCode::Down);
        assert_eq!(state.selected_tool, 0);

        // Move up (wraps to last)
        state.handle_key(KeyCode::Up);
        assert_eq!(state.selected_tool, 1);

        // Toggle focus
        state.handle_key(KeyCode::Tab);
        assert_eq!(state.focus, WizardFocus::Buttons);

        // Move right in buttons
        state.handle_key(KeyCode::Right);
        assert_eq!(state.selected_button, 1);

        // Move left
        state.handle_key(KeyCode::Left);
        assert_eq!(state.selected_button, 0);
    }

    #[test]
    fn test_wizard_state_continue() {
        let tools = vec![create_test_tool("claude-code", ToolStatus::Ready)];
        let mut state = WizardState::new(tools);

        // Press Enter on Continue (default selected)
        state.focus = WizardFocus::Buttons;
        state.selected_button = 0; // Continue
        state.handle_key(KeyCode::Enter);

        assert!(state.should_exit);
        assert!(state.result.is_some());
        let result = state.result.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "claude-code");
    }

    #[test]
    fn test_wizard_state_manual_setup() {
        let tools = vec![create_test_tool("claude-code", ToolStatus::Ready)];
        let mut state = WizardState::new(tools);

        // Select Manual Setup button and activate
        state.focus = WizardFocus::Buttons;
        state.selected_button = 1; // Manual Setup
        state.handle_key(KeyCode::Enter);

        assert!(state.should_exit);
        assert!(state.result.is_some());
        assert!(state.result.unwrap().is_none()); // None means manual setup
    }

    #[test]
    fn test_wizard_state_quit() {
        let tools = vec![create_test_tool("claude-code", ToolStatus::Ready)];
        let mut state = WizardState::new(tools);

        // Press q to quit
        state.handle_key(KeyCode::Char('q'));

        assert!(state.should_exit);
        assert!(state.result.is_none()); // None result means cancelled
    }

    #[test]
    fn test_wizard_state_escape_quit() {
        let tools = vec![create_test_tool("claude-code", ToolStatus::Ready)];
        let mut state = WizardState::new(tools);

        // Press Escape to quit
        state.handle_key(KeyCode::Esc);

        assert!(state.should_exit);
        assert!(state.result.is_none());
    }

    #[test]
    fn test_wizard_state_select_tool_with_enter() {
        let tools = vec![
            create_test_tool("claude-code", ToolStatus::Ready),
            create_test_tool("opencode", ToolStatus::Ready),
        ];
        let mut state = WizardState::new(tools);

        // Move to second tool
        state.handle_key(KeyCode::Down);
        assert_eq!(state.selected_tool, 1);

        // Press Enter to select
        state.handle_key(KeyCode::Enter);

        assert!(state.should_exit);
        let result = state.result.unwrap().unwrap();
        assert_eq!(result.name, "opencode");
    }

    #[test]
    fn test_wizard_button_labels() {
        assert_eq!(WizardButton::Continue.label(), "Continue");
        assert_eq!(WizardButton::ManualSetup.label(), "Manual Setup");
        assert_eq!(WizardButton::Quit.label(), "Quit");
    }

    #[test]
    fn test_empty_tools_error() {
        let result = run_wizard(vec![]);
        assert!(matches!(result, Err(WizardError::NoToolsAvailable)));
    }

    #[test]
    fn test_vim_keybindings() {
        let tools = vec![
            create_test_tool("claude-code", ToolStatus::Ready),
            create_test_tool("opencode", ToolStatus::Ready),
        ];
        let mut state = WizardState::new(tools);

        // j moves down
        state.handle_key(KeyCode::Char('j'));
        assert_eq!(state.selected_tool, 1);

        // k moves up
        state.handle_key(KeyCode::Char('k'));
        assert_eq!(state.selected_tool, 0);

        // Tab to buttons
        state.handle_key(KeyCode::Tab);
        assert_eq!(state.focus, WizardFocus::Buttons);

        // l moves right
        state.handle_key(KeyCode::Char('l'));
        assert_eq!(state.selected_button, 1);

        // h moves left
        state.handle_key(KeyCode::Char('h'));
        assert_eq!(state.selected_button, 0);
    }
}
