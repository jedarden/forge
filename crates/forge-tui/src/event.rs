//! Event handling for the FORGE TUI.
//!
//! Provides keyboard input handling and event routing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::view::View;

/// Application-level events that can trigger state changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    /// Switch to a specific view
    SwitchView(View),
    /// Cycle to the next view
    NextView,
    /// Cycle to the previous view
    PrevView,
    /// Show help overlay
    ShowHelp,
    /// Hide help overlay
    HideHelp,
    /// Request application quit
    Quit,
    /// Force quit (Ctrl+C)
    ForceQuit,
    /// Refresh current view
    Refresh,
    /// Cancel current operation
    Cancel,
    /// Navigate up in a list
    NavigateUp,
    /// Navigate down in a list
    NavigateDown,
    /// Page up
    PageUp,
    /// Page down
    PageDown,
    /// Go to top
    GoToTop,
    /// Go to bottom
    GoToBottom,
    /// Select/expand current item
    Select,
    /// Toggle item selection
    Toggle,
    /// Focus next panel
    FocusNext,
    /// Focus previous panel
    FocusPrev,
    /// Text input character
    TextInput(char),
    /// Backspace in text input
    Backspace,
    /// Submit text input
    Submit,
    /// Spawn a new worker (variant specifies executor type)
    SpawnWorker(WorkerExecutor),
    /// Kill selected worker
    KillWorker,
    /// Open configuration menu
    OpenConfig,
    /// Open budget configuration
    OpenBudgetConfig,
    /// Open worker configuration
    OpenWorkerConfig,
    /// Cycle to the next theme
    CycleTheme,
    /// No action needed
    None,
}

/// Worker executor types for spawn actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerExecutor {
    /// GLM-4.7 model
    Glm,
    /// Claude Sonnet 4.5
    Sonnet,
    /// Claude Opus 4.6
    Opus,
    /// Haiku model
    Haiku,
}

impl WorkerExecutor {
    /// Returns the display name for this executor.
    pub fn name(&self) -> &'static str {
        match self {
            WorkerExecutor::Glm => "GLM-4.7",
            WorkerExecutor::Sonnet => "Sonnet 4.5",
            WorkerExecutor::Opus => "Opus 4.6",
            WorkerExecutor::Haiku => "Haiku",
        }
    }

    /// Returns the hotkey character for this executor.
    pub fn hotkey(&self) -> char {
        match self {
            WorkerExecutor::Glm => 'G',
            WorkerExecutor::Sonnet => 'S',
            WorkerExecutor::Opus => 'O',
            WorkerExecutor::Haiku => 'H',
        }
    }
}

/// Input handler for converting key events to app events.
#[derive(Debug, Default)]
pub struct InputHandler {
    /// Whether we're currently in chat/text input mode
    chat_mode: bool,
}

impl InputHandler {
    /// Create a new input handler.
    pub fn new() -> Self {
        Self { chat_mode: false }
    }

    /// Set whether chat/text input mode is active.
    pub fn set_chat_mode(&mut self, active: bool) {
        self.chat_mode = active;
    }

    /// Returns whether chat mode is active.
    pub fn is_chat_mode(&self) -> bool {
        self.chat_mode
    }

    /// Handle a key event and return the corresponding app event.
    pub fn handle_key(&mut self, key: KeyEvent) -> AppEvent {
        // Ctrl+C always force quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return AppEvent::ForceQuit;
        }

        // Ctrl+L refreshes
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
            return AppEvent::Refresh;
        }

        // Escape cancels current operation or exits chat mode
        if key.code == KeyCode::Esc {
            if self.chat_mode {
                self.chat_mode = false;
                return AppEvent::Cancel;
            }
            return AppEvent::Cancel;
        }

        // In chat mode, handle text input
        if self.chat_mode {
            return self.handle_chat_input(key);
        }

        // Normal mode key handling
        self.handle_normal_mode(key)
    }

    /// Handle input when in chat/text mode.
    fn handle_chat_input(&self, key: KeyEvent) -> AppEvent {
        match key.code {
            KeyCode::Enter => AppEvent::Submit,
            KeyCode::Backspace => AppEvent::Backspace,
            KeyCode::Char(c) => AppEvent::TextInput(c),
            KeyCode::Up => AppEvent::NavigateUp,
            KeyCode::Down => AppEvent::NavigateDown,
            _ => AppEvent::None,
        }
    }

    /// Handle input when in normal navigation mode.
    fn handle_normal_mode(&mut self, key: KeyEvent) -> AppEvent {
        match key.code {
            // Quit
            KeyCode::Char('q') | KeyCode::Char('Q') => AppEvent::Quit,

            // Help (when not used for spawn Haiku)
            KeyCode::Char('?') => AppEvent::ShowHelp,
            // Note: 'h' is now spawn Haiku, help only via '?'

            // Quick Actions - Spawn workers
            KeyCode::Char('g') | KeyCode::Char('G') => AppEvent::SpawnWorker(WorkerExecutor::Glm),
            KeyCode::Char('s') | KeyCode::Char('S') => AppEvent::SpawnWorker(WorkerExecutor::Sonnet),
            KeyCode::Char('o') => AppEvent::SpawnWorker(WorkerExecutor::Opus),  // lowercase only
            KeyCode::Char('h') => AppEvent::SpawnWorker(WorkerExecutor::Haiku),

            // Quick Actions - Kill worker
            KeyCode::Char('k') => AppEvent::KillWorker,

            // Quick Actions - Refresh
            KeyCode::Char('r') => AppEvent::Refresh,

            // View navigation hotkeys (also quick actions - view shortcuts)
            KeyCode::Char('w') | KeyCode::Char('W') => AppEvent::SwitchView(View::Workers),
            KeyCode::Char('t') | KeyCode::Char('T') => AppEvent::SwitchView(View::Tasks),
            // 'a' for Activity/Logs view
            KeyCode::Char('a') | KeyCode::Char('A') => AppEvent::SwitchView(View::Logs),
            // 'l' for Logs view (alternative to 'a')
            KeyCode::Char('l') => AppEvent::SwitchView(View::Logs),

            // Quick Actions - Configure
            KeyCode::Char('M') => AppEvent::OpenConfig,
            KeyCode::Char('b') | KeyCode::Char('B') => AppEvent::OpenBudgetConfig,
            // Note: 'c' lowercase is Costs view, uppercase is now CycleTheme
            // Worker config moved to 'U' (uppercase U)
            KeyCode::Char('U') => AppEvent::OpenWorkerConfig,
            // Cycle theme (uppercase C)
            KeyCode::Char('C') => AppEvent::CycleTheme,

            // Costs view (only lowercase)
            KeyCode::Char('c') => AppEvent::SwitchView(View::Costs),

            // Metrics view (lowercase, uppercase is OpenConfig)
            KeyCode::Char('m') => AppEvent::SwitchView(View::Metrics),

            // Overview view
            KeyCode::Char('O') => AppEvent::SwitchView(View::Overview),

            // Chat mode activation
            KeyCode::Char(':') => {
                self.chat_mode = true;
                AppEvent::SwitchView(View::Chat)
            }

            // Tab cycling
            KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    AppEvent::PrevView
                } else {
                    AppEvent::NextView
                }
            }
            KeyCode::BackTab => AppEvent::PrevView,

            // List navigation (use 'j' for down since 'k' is unused now)
            KeyCode::Up => AppEvent::NavigateUp,
            KeyCode::Down | KeyCode::Char('j') => AppEvent::NavigateDown,
            KeyCode::PageUp => AppEvent::PageUp,
            KeyCode::PageDown => AppEvent::PageDown,
            KeyCode::Home => AppEvent::GoToTop,
            KeyCode::End => AppEvent::GoToBottom,

            // Selection
            KeyCode::Enter => AppEvent::Select,
            KeyCode::Char(' ') => AppEvent::Toggle,

            _ => AppEvent::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_event(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn key_event_with_mods(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn test_view_hotkeys() {
        let mut handler = InputHandler::new();

        // 'O' uppercase for Overview (lowercase 'o' is spawn Opus)
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('O'))),
            AppEvent::SwitchView(View::Overview)
        );
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('w'))),
            AppEvent::SwitchView(View::Workers)
        );
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('t'))),
            AppEvent::SwitchView(View::Tasks)
        );
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('c'))),
            AppEvent::SwitchView(View::Costs)
        );
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('m'))),
            AppEvent::SwitchView(View::Metrics)
        );
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('l'))),
            AppEvent::SwitchView(View::Logs)
        );
        // 'a' for Activity/Logs view
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('a'))),
            AppEvent::SwitchView(View::Logs)
        );
    }

    #[test]
    fn test_chat_mode_activation() {
        let mut handler = InputHandler::new();
        assert!(!handler.is_chat_mode());

        let event = handler.handle_key(key_event(KeyCode::Char(':')));
        assert_eq!(event, AppEvent::SwitchView(View::Chat));
        assert!(handler.is_chat_mode());
    }

    #[test]
    fn test_chat_mode_escape() {
        let mut handler = InputHandler::new();
        handler.set_chat_mode(true);
        assert!(handler.is_chat_mode());

        let event = handler.handle_key(key_event(KeyCode::Esc));
        assert_eq!(event, AppEvent::Cancel);
        assert!(!handler.is_chat_mode());
    }

    #[test]
    fn test_chat_mode_input() {
        let mut handler = InputHandler::new();
        handler.set_chat_mode(true);

        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('a'))),
            AppEvent::TextInput('a')
        );
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Backspace)),
            AppEvent::Backspace
        );
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Enter)),
            AppEvent::Submit
        );
    }

    #[test]
    fn test_ctrl_c_force_quit() {
        let mut handler = InputHandler::new();

        // Works in normal mode
        assert_eq!(
            handler.handle_key(key_event_with_mods(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            AppEvent::ForceQuit
        );

        // Also works in chat mode
        handler.set_chat_mode(true);
        assert_eq!(
            handler.handle_key(key_event_with_mods(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            AppEvent::ForceQuit
        );
    }

    #[test]
    fn test_tab_cycling() {
        let mut handler = InputHandler::new();

        assert_eq!(
            handler.handle_key(key_event(KeyCode::Tab)),
            AppEvent::NextView
        );
        assert_eq!(
            handler.handle_key(key_event_with_mods(KeyCode::Tab, KeyModifiers::SHIFT)),
            AppEvent::PrevView
        );
        assert_eq!(
            handler.handle_key(key_event(KeyCode::BackTab)),
            AppEvent::PrevView
        );
    }

    #[test]
    fn test_navigation_keys() {
        let mut handler = InputHandler::new();

        assert_eq!(
            handler.handle_key(key_event(KeyCode::Up)),
            AppEvent::NavigateUp
        );
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Down)),
            AppEvent::NavigateDown
        );
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('j'))),
            AppEvent::NavigateDown
        );
        // 'k' is now KillWorker, not NavigateUp
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('k'))),
            AppEvent::KillWorker
        );
    }

    #[test]
    fn test_help_and_quit() {
        let mut handler = InputHandler::new();

        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('?'))),
            AppEvent::ShowHelp
        );
        // 'h' is now spawn Haiku, help only via '?'
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('h'))),
            AppEvent::SpawnWorker(WorkerExecutor::Haiku)
        );
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('q'))),
            AppEvent::Quit
        );
    }

    #[test]
    fn test_case_insensitive_hotkeys() {
        let mut handler = InputHandler::new();

        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('W'))),
            AppEvent::SwitchView(View::Workers)
        );
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('Q'))),
            AppEvent::Quit
        );
    }

    #[test]
    fn test_theme_cycle_hotkey() {
        let mut handler = InputHandler::new();

        // Uppercase 'C' cycles theme
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('C'))),
            AppEvent::CycleTheme
        );

        // Lowercase 'c' is still Costs view
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('c'))),
            AppEvent::SwitchView(View::Costs)
        );

        // 'U' is now OpenWorkerConfig (was 'C')
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('U'))),
            AppEvent::OpenWorkerConfig
        );
    }
}
