//! View types and navigation for the FORGE TUI.
//!
//! Views represent the different screens/modes available in the dashboard.

use std::fmt;

/// Available views in the FORGE dashboard.
///
/// Each view represents a distinct screen with its own content and interactions.
/// Views can be switched using hotkeys or the Tab key to cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    /// Overview/dashboard showing summary of all components
    #[default]
    Overview,
    /// Worker pool management and status
    Workers,
    /// Task queue and bead management
    Tasks,
    /// Cost analytics and optimization
    Costs,
    /// Performance metrics and statistics
    Metrics,
    /// Activity log viewer
    Logs,
    /// Conversational chat interface (activated with `:`)
    Chat,
}

impl View {
    /// Returns the hotkey character for this view.
    pub fn hotkey(&self) -> char {
        match self {
            View::Overview => 'o',
            View::Workers => 'w',
            View::Tasks => 't',
            View::Costs => 'c',
            View::Metrics => 'm',
            View::Logs => 'l',
            View::Chat => ':',
        }
    }

    /// Returns the display title for this view.
    pub fn title(&self) -> &'static str {
        match self {
            View::Overview => "Overview",
            View::Workers => "Workers",
            View::Tasks => "Tasks",
            View::Costs => "Costs",
            View::Metrics => "Metrics",
            View::Logs => "Logs",
            View::Chat => "Chat",
        }
    }

    /// Returns the hotkey hint for status bar display.
    pub fn hotkey_hint(&self) -> String {
        format!("[{}] {}", self.hotkey(), self.title())
    }

    /// All views in display order (for Tab cycling).
    pub const ALL: [View; 7] = [
        View::Overview,
        View::Workers,
        View::Tasks,
        View::Costs,
        View::Metrics,
        View::Logs,
        View::Chat,
    ];

    /// Returns the next view in the cycle (for Tab navigation).
    pub fn next(&self) -> View {
        let idx = Self::ALL.iter().position(|v| v == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// Returns the previous view in the cycle (for Shift+Tab navigation).
    pub fn prev(&self) -> View {
        let idx = Self::ALL.iter().position(|v| v == self).unwrap_or(0);
        if idx == 0 {
            Self::ALL[Self::ALL.len() - 1]
        } else {
            Self::ALL[idx - 1]
        }
    }

    /// Try to parse a view from a hotkey character.
    pub fn from_hotkey(key: char) -> Option<View> {
        match key.to_ascii_lowercase() {
            'o' => Some(View::Overview),
            'w' => Some(View::Workers),
            't' => Some(View::Tasks),
            'c' => Some(View::Costs),
            'm' => Some(View::Metrics),
            'l' => Some(View::Logs),
            ':' => Some(View::Chat),
            _ => None,
        }
    }
}

impl fmt::Display for View {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.title())
    }
}

/// Focus state for panels within a view.
///
/// Tracks which panel has keyboard focus for navigation and highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusPanel {
    /// No specific panel focused (view-level focus)
    #[default]
    None,
    /// Worker pool panel
    WorkerPool,
    /// Subscription/usage panel
    Subscriptions,
    /// Task queue panel
    TaskQueue,
    /// Activity log panel
    ActivityLog,
    /// Cost breakdown panel
    CostBreakdown,
    /// Metrics charts panel
    MetricsCharts,
    /// Chat input panel
    ChatInput,
}

impl FocusPanel {
    /// Returns whether this panel should show a highlight border.
    pub fn is_highlighted(&self) -> bool {
        !matches!(self, FocusPanel::None)
    }
}

/// Layout mode based on terminal dimensions.
///
/// The TUI adapts its layout based on available screen real estate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// Ultra-wide layout (199+ cols): 3-column with all 6 panels visible.
    /// Left (66 cols): Workers + Subscriptions
    /// Middle (66 cols): Tasks + Activity
    /// Right (65 cols): Costs + Actions
    UltraWide,
    /// Wide layout (120-198 cols): 2-column with 4 panels visible.
    Wide,
    /// Narrow layout (<120 cols): single-view mode.
    Narrow,
}

impl LayoutMode {
    /// Determine the layout mode based on terminal width.
    pub fn from_width(width: u16) -> Self {
        if width >= 199 {
            LayoutMode::UltraWide
        } else if width >= 120 {
            LayoutMode::Wide
        } else {
            LayoutMode::Narrow
        }
    }

    /// Get the minimum terminal height for this layout mode.
    pub fn min_height(&self) -> u16 {
        match self {
            LayoutMode::UltraWide => 38,
            LayoutMode::Wide => 30,
            LayoutMode::Narrow => 20,
        }
    }

    /// Check if the terminal dimensions meet the layout requirements.
    pub fn meets_requirements(&self, width: u16, height: u16) -> bool {
        let required_mode = Self::from_width(width);
        let height_ok = height >= self.min_height();

        // Mode is valid if it matches width requirement and height is sufficient
        *self == required_mode && height_ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_hotkeys() {
        assert_eq!(View::Overview.hotkey(), 'o');
        assert_eq!(View::Workers.hotkey(), 'w');
        assert_eq!(View::Tasks.hotkey(), 't');
        assert_eq!(View::Costs.hotkey(), 'c');
        assert_eq!(View::Metrics.hotkey(), 'm');
        assert_eq!(View::Logs.hotkey(), 'l');
        assert_eq!(View::Chat.hotkey(), ':');
    }

    #[test]
    fn test_view_from_hotkey() {
        assert_eq!(View::from_hotkey('o'), Some(View::Overview));
        assert_eq!(View::from_hotkey('W'), Some(View::Workers)); // case insensitive
        assert_eq!(View::from_hotkey(':'), Some(View::Chat));
        assert_eq!(View::from_hotkey('x'), None);
    }

    #[test]
    fn test_view_cycling() {
        assert_eq!(View::Overview.next(), View::Workers);
        assert_eq!(View::Chat.next(), View::Overview); // wraps around
        assert_eq!(View::Overview.prev(), View::Chat); // wraps around
        assert_eq!(View::Workers.prev(), View::Overview);
    }

    #[test]
    fn test_view_titles() {
        assert_eq!(View::Overview.title(), "Overview");
        assert_eq!(View::Workers.title(), "Workers");
        assert_eq!(View::Chat.title(), "Chat");
    }

    #[test]
    fn test_hotkey_hint() {
        assert_eq!(View::Overview.hotkey_hint(), "[o] Overview");
        assert_eq!(View::Chat.hotkey_hint(), "[:] Chat");
    }

    #[test]
    fn test_default_view() {
        assert_eq!(View::default(), View::Overview);
    }

    // ============================================================
    // LayoutMode Tests
    // ============================================================

    #[test]
    fn test_layout_mode_from_width_ultrawide() {
        assert_eq!(LayoutMode::from_width(199), LayoutMode::UltraWide);
        assert_eq!(LayoutMode::from_width(200), LayoutMode::UltraWide);
        assert_eq!(LayoutMode::from_width(300), LayoutMode::UltraWide);
    }

    #[test]
    fn test_layout_mode_from_width_wide() {
        assert_eq!(LayoutMode::from_width(120), LayoutMode::Wide);
        assert_eq!(LayoutMode::from_width(150), LayoutMode::Wide);
        assert_eq!(LayoutMode::from_width(198), LayoutMode::Wide);
    }

    #[test]
    fn test_layout_mode_from_width_narrow() {
        assert_eq!(LayoutMode::from_width(119), LayoutMode::Narrow);
        assert_eq!(LayoutMode::from_width(80), LayoutMode::Narrow);
        assert_eq!(LayoutMode::from_width(40), LayoutMode::Narrow);
        assert_eq!(LayoutMode::from_width(0), LayoutMode::Narrow);
    }

    #[test]
    fn test_layout_mode_min_height() {
        assert_eq!(LayoutMode::UltraWide.min_height(), 38);
        assert_eq!(LayoutMode::Wide.min_height(), 30);
        assert_eq!(LayoutMode::Narrow.min_height(), 20);
    }

    #[test]
    fn test_layout_mode_meets_requirements() {
        // Ultra-wide mode requirements
        assert!(LayoutMode::UltraWide.meets_requirements(199, 38));
        assert!(LayoutMode::UltraWide.meets_requirements(250, 50));
        assert!(!LayoutMode::UltraWide.meets_requirements(199, 37)); // Height too short
        assert!(!LayoutMode::UltraWide.meets_requirements(198, 38)); // Width too narrow

        // Wide mode requirements
        assert!(LayoutMode::Wide.meets_requirements(150, 30));
        assert!(LayoutMode::Wide.meets_requirements(120, 35));
        assert!(!LayoutMode::Wide.meets_requirements(150, 29)); // Height too short
        assert!(!LayoutMode::Wide.meets_requirements(199, 30)); // Width triggers UltraWide

        // Narrow mode requirements
        assert!(LayoutMode::Narrow.meets_requirements(80, 20));
        assert!(LayoutMode::Narrow.meets_requirements(100, 25));
        assert!(!LayoutMode::Narrow.meets_requirements(80, 19)); // Height too short
        assert!(!LayoutMode::Narrow.meets_requirements(120, 20)); // Width triggers Wide
    }

    #[test]
    fn test_layout_mode_boundary_conditions() {
        // Test exact boundary values
        assert_eq!(LayoutMode::from_width(198), LayoutMode::Wide);
        assert_eq!(LayoutMode::from_width(199), LayoutMode::UltraWide);

        assert_eq!(LayoutMode::from_width(119), LayoutMode::Narrow);
        assert_eq!(LayoutMode::from_width(120), LayoutMode::Wide);
    }
}
