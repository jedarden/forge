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
    /// No action needed
    None,
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

            // Help
            KeyCode::Char('?') | KeyCode::Char('h') | KeyCode::Char('H') => AppEvent::ShowHelp,

            // View navigation hotkeys
            KeyCode::Char('o') | KeyCode::Char('O') => AppEvent::SwitchView(View::Overview),
            KeyCode::Char('w') | KeyCode::Char('W') => AppEvent::SwitchView(View::Workers),
            KeyCode::Char('t') | KeyCode::Char('T') => AppEvent::SwitchView(View::Tasks),
            KeyCode::Char('c') | KeyCode::Char('C') => AppEvent::SwitchView(View::Costs),
            KeyCode::Char('m') | KeyCode::Char('M') => AppEvent::SwitchView(View::Metrics),
            KeyCode::Char('l') | KeyCode::Char('L') => AppEvent::SwitchView(View::Logs),

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

            // List navigation
            KeyCode::Up | KeyCode::Char('k') => AppEvent::NavigateUp,
            KeyCode::Down | KeyCode::Char('j') => AppEvent::NavigateDown,
            KeyCode::PageUp => AppEvent::PageUp,
            KeyCode::PageDown => AppEvent::PageDown,
            KeyCode::Home | KeyCode::Char('g') => AppEvent::GoToTop,
            KeyCode::End | KeyCode::Char('G') => AppEvent::GoToBottom,

            // Selection
            KeyCode::Enter => AppEvent::Select,
            KeyCode::Char(' ') => AppEvent::Toggle,

            // Refresh
            KeyCode::Char('r') | KeyCode::Char('R') => AppEvent::Refresh,

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

        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('o'))),
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
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('k'))),
            AppEvent::NavigateUp
        );
    }

    #[test]
    fn test_help_and_quit() {
        let mut handler = InputHandler::new();

        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('?'))),
            AppEvent::ShowHelp
        );
        assert_eq!(
            handler.handle_key(key_event(KeyCode::Char('h'))),
            AppEvent::ShowHelp
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
}
