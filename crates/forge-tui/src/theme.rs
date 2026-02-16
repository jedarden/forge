//! Theme system for FORGE TUI.
//!
//! Provides configurable color themes with runtime switching and persistence.

use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Theme name identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeName {
    /// Default theme (current color scheme)
    #[default]
    Default,
    /// Dark theme (enhanced contrast)
    Dark,
    /// Light theme (for bright environments)
    Light,
    /// Cyberpunk theme (neon colors)
    Cyberpunk,
}

impl ThemeName {
    /// All available themes in cycle order.
    pub fn all() -> &'static [ThemeName] {
        &[
            ThemeName::Default,
            ThemeName::Dark,
            ThemeName::Light,
            ThemeName::Cyberpunk,
        ]
    }

    /// Get the next theme in the cycle.
    pub fn next(&self) -> ThemeName {
        let themes = Self::all();
        let current_idx = themes.iter().position(|t| t == self).unwrap_or(0);
        themes[(current_idx + 1) % themes.len()]
    }

    /// Get the display name for this theme.
    pub fn display_name(&self) -> &'static str {
        match self {
            ThemeName::Default => "Default",
            ThemeName::Dark => "Dark",
            ThemeName::Light => "Light",
            ThemeName::Cyberpunk => "Cyberpunk",
        }
    }

    /// Parse a theme name from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "default" => Some(ThemeName::Default),
            "dark" => Some(ThemeName::Dark),
            "light" => Some(ThemeName::Light),
            "cyberpunk" => Some(ThemeName::Cyberpunk),
            _ => None,
        }
    }
}

/// Color palette for a theme.
#[derive(Debug, Clone)]
pub struct ThemeColors {
    /// Primary headers and focused borders
    pub header: Color,
    /// Hotkey hints
    pub hotkey: Color,
    /// Normal text
    pub text: Color,
    /// Secondary text (timestamps, dim info)
    pub text_dim: Color,
    /// Unfocused borders
    pub border_dim: Color,
    /// Bright focus indicator for panel borders/titles
    pub focus_highlight: Color,
    /// Dimmed text for unfocused panels
    pub unfocused_text: Color,
    /// Status: healthy/success
    pub status_healthy: Color,
    /// Status: warning
    pub status_warning: Color,
    /// Status: error/critical
    pub status_error: Color,
    /// Action: spawn worker
    pub action_spawn: Color,
    /// Action: kill worker
    pub action_kill: Color,
    /// Action: refresh
    pub action_refresh: Color,
    /// Action: view navigation
    pub action_view: Color,
    /// Action: configure
    pub action_config: Color,
}

/// Complete theme definition.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Theme name
    pub name: ThemeName,
    /// Color palette
    pub colors: ThemeColors,
}

impl Theme {
    /// Create the Default theme (matches current FORGE colors).
    pub fn default_theme() -> Self {
        Self {
            name: ThemeName::Default,
            colors: ThemeColors {
                header: Color::Cyan,
                hotkey: Color::Yellow,
                text: Color::White,
                text_dim: Color::Gray,
                border_dim: Color::DarkGray,
                // Enhanced: Bright cyan for maximum visibility
                focus_highlight: Color::Cyan,
                // Enhanced: More dimmed unfocused text (RGB for precise control)
                unfocused_text: Color::Rgb(80, 80, 80),
                status_healthy: Color::Green,
                status_warning: Color::Yellow,
                status_error: Color::Red,
                action_spawn: Color::Green,
                action_kill: Color::Red,
                action_refresh: Color::Cyan,
                action_view: Color::Blue,
                action_config: Color::Yellow,
            },
        }
    }

    /// Create the Dark theme (enhanced contrast).
    pub fn dark_theme() -> Self {
        Self {
            name: ThemeName::Dark,
            colors: ThemeColors {
                header: Color::LightBlue,
                hotkey: Color::LightYellow,
                text: Color::White,
                text_dim: Color::DarkGray,
                border_dim: Color::Black,
                // Enhanced: Bright yellow for strong contrast on dark
                focus_highlight: Color::LightYellow,
                // Enhanced: Very dim unfocused text
                unfocused_text: Color::Rgb(60, 60, 60),
                status_healthy: Color::LightGreen,
                status_warning: Color::LightYellow,
                status_error: Color::LightRed,
                action_spawn: Color::LightGreen,
                action_kill: Color::LightRed,
                action_refresh: Color::LightBlue,
                action_view: Color::LightCyan,
                action_config: Color::LightYellow,
            },
        }
    }

    /// Create the Light theme (for bright environments).
    pub fn light_theme() -> Self {
        Self {
            name: ThemeName::Light,
            colors: ThemeColors {
                header: Color::Blue,
                hotkey: Color::DarkGray,
                text: Color::Black,
                text_dim: Color::DarkGray,
                border_dim: Color::Gray,
                // Enhanced: Deep blue for visibility on light background
                focus_highlight: Color::Rgb(0, 100, 255),
                // Enhanced: Light gray for subdued unfocused
                unfocused_text: Color::Rgb(160, 160, 160),
                status_healthy: Color::Green,
                status_warning: Color::Yellow,
                status_error: Color::Red,
                action_spawn: Color::Green,
                action_kill: Color::Red,
                action_refresh: Color::Blue,
                action_view: Color::Cyan,
                action_config: Color::DarkGray,
            },
        }
    }

    /// Create the Cyberpunk theme (neon colors).
    pub fn cyberpunk_theme() -> Self {
        Self {
            name: ThemeName::Cyberpunk,
            colors: ThemeColors {
                header: Color::Magenta,
                hotkey: Color::Cyan,
                text: Color::White,
                text_dim: Color::LightMagenta,
                border_dim: Color::DarkGray,
                // Enhanced: Neon cyan for cyberpunk aesthetic
                focus_highlight: Color::Rgb(0, 255, 255),
                // Enhanced: Very dim for strong neon contrast
                unfocused_text: Color::Rgb(70, 70, 80),
                status_healthy: Color::Green,
                status_warning: Color::Yellow,
                status_error: Color::Red,
                action_spawn: Color::Green,
                action_kill: Color::Red,
                action_refresh: Color::Cyan,
                action_view: Color::Magenta,
                action_config: Color::LightCyan,
            },
        }
    }

    /// Get a theme by name.
    pub fn by_name(name: ThemeName) -> Self {
        match name {
            ThemeName::Default => Self::default_theme(),
            ThemeName::Dark => Self::dark_theme(),
            ThemeName::Light => Self::light_theme(),
            ThemeName::Cyberpunk => Self::cyberpunk_theme(),
        }
    }

    /// Get budget alert color based on percentage.
    pub fn budget_alert_color(&self, pct: f64) -> Color {
        if pct >= 100.0 {
            self.colors.status_error
        } else if pct >= 90.0 {
            Color::LightRed
        } else if pct >= 70.0 {
            self.colors.status_warning
        } else {
            self.colors.status_healthy
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::default_theme()
    }
}

/// Theme configuration file format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    /// Currently selected theme
    pub current_theme: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            current_theme: "default".to_string(),
        }
    }
}

/// Theme manager handles loading, saving, and switching themes.
pub struct ThemeManager {
    /// Current theme
    current: Theme,
    /// Configuration file path
    config_path: PathBuf,
}

impl ThemeManager {
    /// Create a new theme manager with default theme.
    pub fn new() -> Self {
        Self::with_theme(Theme::default_theme())
    }

    /// Create a theme manager with a specific theme.
    pub fn with_theme(theme: Theme) -> Self {
        let config_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".forge");

        // Create config directory if it doesn't exist
        if let Err(e) = fs::create_dir_all(&config_dir) {
            tracing::debug!("Failed to create .forge config directory: {}", e);
        }

        let config_path = config_dir.join("theme.toml");

        Self {
            current: theme,
            config_path,
        }
    }

    /// Load theme from configuration file.
    pub fn load_config() -> Self {
        let config_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".forge");
        let config_path = config_dir.join("theme.toml");

        let theme_name = if config_path.exists() {
            if let Ok(content) = fs::read_to_string(&config_path) {
                if let Ok(config) = Self::parse_toml(&content) {
                    if let Some(name) = ThemeName::from_str(&config.current_theme) {
                        tracing::info!("Loaded theme: {}", name.display_name());
                        return Self::with_theme(Theme::by_name(name));
                    }
                }
            }
            ThemeName::Default
        } else {
            ThemeName::Default
        };

        Self::with_theme(Theme::by_name(theme_name))
    }

    /// Parse TOML configuration content.
    fn parse_toml(content: &str) -> Result<ThemeConfig, Box<dyn std::error::Error>> {
        // Simple manual TOML parsing since we don't want to add toml dependency
        // Format: current_theme = "value"
        let theme_name = content
            .lines()
            .find_map(|line| {
                let line = line.trim();
                if line.starts_with("current_theme") {
                    line.split('=')
                        .nth(1)
                        .map(|s| s.trim().trim_matches('"').to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "default".to_string());

        Ok(ThemeConfig {
            current_theme: theme_name,
        })
    }

    /// Save current theme to configuration file.
    pub fn save_config(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config = ThemeConfig {
            current_theme: self.current.name.display_name().to_lowercase(),
        };

        let content = format!(
            "# FORGE Theme Configuration\n\
             # Generated by forge - do not edit manually\n\
             # Available themes: default, dark, light, cyberpunk\n\
             current_theme = \"{}\"\n",
            config.current_theme
        );

        // Ensure directory exists
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&self.config_path, content)?;
        tracing::debug!("Saved theme config to: {:?}", self.config_path);
        Ok(())
    }

    /// Get the current theme.
    pub fn current(&self) -> &Theme {
        &self.current
    }

    /// Switch to the next theme in the cycle.
    pub fn cycle_theme(&mut self) -> ThemeName {
        let next_name = self.current.name.next();
        self.current = Theme::by_name(next_name);

        // Save to config
        if let Err(e) = self.save_config() {
            tracing::warn!("Failed to save theme config: {}", e);
        }

        next_name
    }

    /// Set a specific theme.
    pub fn set_theme(&mut self, name: ThemeName) {
        self.current = Theme::by_name(name);

        // Save to config
        if let Err(e) = self.save_config() {
            tracing::warn!("Failed to save theme config: {}", e);
        }
    }

    /// Get the current theme name.
    pub fn theme_name(&self) -> ThemeName {
        self.current.name
    }
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self::new()
    }
}

// Simple dirs crate stub for home directory
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| std::env::var("USERPROFILE").ok().map(PathBuf::from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_name_cycle() {
        assert_eq!(ThemeName::Default.next(), ThemeName::Dark);
        assert_eq!(ThemeName::Dark.next(), ThemeName::Light);
        assert_eq!(ThemeName::Light.next(), ThemeName::Cyberpunk);
        assert_eq!(ThemeName::Cyberpunk.next(), ThemeName::Default);
    }

    #[test]
    fn test_theme_name_from_str() {
        assert_eq!(ThemeName::from_str("default"), Some(ThemeName::Default));
        assert_eq!(ThemeName::from_str("dark"), Some(ThemeName::Dark));
        assert_eq!(ThemeName::from_str("light"), Some(ThemeName::Light));
        assert_eq!(ThemeName::from_str("cyberpunk"), Some(ThemeName::Cyberpunk));
        assert_eq!(ThemeName::from_str("invalid"), None);
    }

    #[test]
    fn test_theme_colors() {
        let theme = Theme::default_theme();
        assert_eq!(theme.colors.header, Color::Cyan);
        assert_eq!(theme.colors.hotkey, Color::Yellow);
        assert_eq!(theme.colors.status_healthy, Color::Green);
    }

    #[test]
    fn test_budget_alert_colors() {
        let theme = Theme::default_theme();

        // Safe zone
        assert_eq!(theme.budget_alert_color(50.0), Color::Green);

        // Warning zone
        assert_eq!(theme.budget_alert_color(80.0), Color::Yellow);

        // Critical zone
        assert_eq!(theme.budget_alert_color(95.0), Color::LightRed);

        // Exceeded
        assert_eq!(theme.budget_alert_color(105.0), Color::Red);
    }

    #[test]
    fn test_theme_manager_cycle() {
        let mut manager = ThemeManager::new();
        assert_eq!(manager.theme_name(), ThemeName::Default);

        let next = manager.cycle_theme();
        assert_eq!(next, ThemeName::Dark);
        assert_eq!(manager.theme_name(), ThemeName::Dark);
    }

    #[test]
    fn test_theme_manager_set() {
        let mut manager = ThemeManager::new();
        manager.set_theme(ThemeName::Cyberpunk);
        assert_eq!(manager.theme_name(), ThemeName::Cyberpunk);
        assert_eq!(manager.current().colors.header, Color::Magenta);
    }

    #[test]
    fn test_all_themes_have_different_colors() {
        let themes = [
            Theme::default_theme(),
            Theme::dark_theme(),
            Theme::light_theme(),
            Theme::cyberpunk_theme(),
        ];

        // Each theme should have at least one different color
        for (i, theme_a) in themes.iter().enumerate() {
            for theme_b in themes.iter().skip(i + 1) {
                // At least one color should differ
                let colors_match = theme_a.colors.header == theme_b.colors.header
                    && theme_a.colors.hotkey == theme_b.colors.hotkey
                    && theme_a.colors.text == theme_b.colors.text;

                assert!(!colors_match, "Themes should have different colors");
            }
        }
    }
}
