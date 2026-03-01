//! Configuration menu system for FORGE TUI.
//!
//! Provides modal dialogs for editing configuration settings including:
//! - General settings (refresh rate, theme, layout)
//! - Budget settings (limits, thresholds)
//! - Worker settings (defaults, timeouts)

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::config_watcher::ForgeConfig;

/// Type of configuration menu to display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigMenuType {
    /// General settings (refresh rate, theme, layout)
    Settings,
    /// Budget and cost tracking settings
    Budget,
    /// Worker default settings
    Worker,
}

impl ConfigMenuType {
    /// Returns the title for this menu type.
    pub fn title(&self) -> &'static str {
        match self {
            ConfigMenuType::Settings => "Settings",
            ConfigMenuType::Budget => "Budget",
            ConfigMenuType::Worker => "Workers",
        }
    }

    /// Returns the hotkey hint for this menu.
    pub fn hotkey(&self) -> &'static str {
        match self {
            ConfigMenuType::Settings => "M",
            ConfigMenuType::Budget => "B",
            ConfigMenuType::Worker => "U",
        }
    }
}

/// A single configurable menu item.
#[derive(Debug, Clone)]
pub struct ConfigMenuItem {
    /// Display label for the setting
    pub label: &'static str,
    /// Current value (as string for display)
    pub value: String,
    /// Description of what this setting does
    pub description: &'static str,
    /// Whether this item is editable
    pub editable: bool,
    /// Input type for validation
    pub input_type: ConfigInputType,
}

/// Input type for config value validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigInputType {
    /// Unsigned integer
    Integer,
    /// Floating point number
    Float,
    /// Text string
    Text,
    /// Selection from predefined options
    Select { options: &'static [&'static str] },
}

impl ConfigInputType {
    /// Validate and normalize input for this type.
    pub fn validate(&self, input: &str) -> Result<String, String> {
        match self {
            ConfigInputType::Integer => {
                input.parse::<u64>()
                    .map(|v| v.to_string())
                    .map_err(|_| "Must be a positive integer".to_string())
            }
            ConfigInputType::Float => {
                input.parse::<f64>()
                    .map(|v| v.to_string())
                    .map_err(|_| "Must be a number".to_string())
            }
            ConfigInputType::Text => {
                Ok(input.to_string())
            }
            ConfigInputType::Select { options } => {
                // Check if input matches an option (case-insensitive)
                let input_lower = input.to_lowercase();
                for &opt in *options {
                    if opt.to_lowercase() == input_lower {
                        return Ok(opt.to_string());
                    }
                }
                Err(format!("Must be one of: {}", options.join(", ")))
            }
        }
    }
}

/// Build menu items for the Settings menu.
pub fn build_settings_items(config: &ForgeConfig) -> Vec<ConfigMenuItem> {
    vec![
        ConfigMenuItem {
            label: "Refresh Interval",
            value: format!("{} ms", config.dashboard.refresh_interval_ms),
            description: "How often to refresh data (100-10000 ms)",
            editable: true,
            input_type: ConfigInputType::Integer,
        },
        ConfigMenuItem {
            label: "Max FPS",
            value: config.dashboard.max_fps.to_string(),
            description: "Maximum frames per second (1-120)",
            editable: true,
            input_type: ConfigInputType::Integer,
        },
        ConfigMenuItem {
            label: "Default Layout",
            value: config.dashboard.default_layout.clone(),
            description: "Starting layout mode",
            editable: true,
            input_type: ConfigInputType::Select {
                options: &["overview", "workers", "tasks", "costs"],
            },
        },
        ConfigMenuItem {
            label: "Theme",
            value: config.theme.name.clone().unwrap_or_else(|| "default".to_string()),
            description: "Color theme",
            editable: true,
            input_type: ConfigInputType::Select {
                options: &["default", "dark", "light", "cyberpunk"],
            },
        },
        ConfigMenuItem {
            label: "Log Retention",
            value: "7 days".to_string(), // TODO: Add to config
            description: "How long to keep log files",
            editable: true,
            input_type: ConfigInputType::Select {
                options: &["1 day", "7 days", "30 days", "forever"],
            },
        },
    ]
}

/// Build menu items for the Budget menu.
pub fn build_budget_items(config: &ForgeConfig) -> Vec<ConfigMenuItem> {
    vec![
        ConfigMenuItem {
            label: "Cost Tracking",
            value: if config.cost_tracking.enabled { "enabled".to_string() } else { "disabled".to_string() },
            description: "Enable/disable cost tracking",
            editable: true,
            input_type: ConfigInputType::Select {
                options: &["enabled", "disabled"],
            },
        },
        ConfigMenuItem {
            label: "Monthly Budget",
            value: config.cost_tracking.monthly_budget_usd
                .map(|v| format!("${:.2}", v))
                .unwrap_or_else(|| "Not set".to_string()),
            description: "Monthly budget limit in USD",
            editable: true,
            input_type: ConfigInputType::Float,
        },
        ConfigMenuItem {
            label: "Warning Threshold",
            value: format!("{}%", config.cost_tracking.budget_warning_threshold),
            description: "Alert when budget usage reaches this %",
            editable: true,
            input_type: ConfigInputType::Integer,
        },
        ConfigMenuItem {
            label: "Critical Threshold",
            value: format!("{}%", config.cost_tracking.budget_critical_threshold),
            description: "Alert when budget usage reaches this %",
            editable: true,
            input_type: ConfigInputType::Integer,
        },
        ConfigMenuItem {
            label: "Sonnet Cost/Input",
            value: "$0.003/1K".to_string(), // TODO: Make configurable
            description: "Cost per 1K input tokens (Sonnet)",
            editable: true,
            input_type: ConfigInputType::Float,
        },
        ConfigMenuItem {
            label: "Sonnet Cost/Output",
            value: "$0.015/1K".to_string(), // TODO: Make configurable
            description: "Cost per 1K output tokens (Sonnet)",
            editable: true,
            input_type: ConfigInputType::Float,
        },
    ]
}

/// Build menu items for the Worker menu.
pub fn build_worker_items(config: &ForgeConfig) -> Vec<ConfigMenuItem> {
    vec![
        ConfigMenuItem {
            label: "Max Workers",
            value: "10".to_string(), // TODO: Add to config
            description: "Maximum concurrent workers",
            editable: true,
            input_type: ConfigInputType::Integer,
        },
        ConfigMenuItem {
            label: "Default Model",
            value: "sonnet".to_string(), // TODO: Add to config
            description: "Default model for new workers",
            editable: true,
            input_type: ConfigInputType::Select {
                options: &["sonnet", "opus", "haiku", "glm"],
            },
        },
        ConfigMenuItem {
            label: "Worker Timeout",
            value: format!("{} min", config.auto_recovery.stuck_task_timeout_mins),
            description: "Minutes before worker is considered stuck",
            editable: true,
            input_type: ConfigInputType::Integer,
        },
        ConfigMenuItem {
            label: "Auto-Recovery",
            value: if config.auto_recovery.enabled { "enabled".to_string() } else { "disabled".to_string() },
            description: "Enable automatic recovery actions",
            editable: true,
            input_type: ConfigInputType::Select {
                options: &["enabled", "disabled"],
            },
        },
        ConfigMenuItem {
            label: "Dead Worker Policy",
            value: config.auto_recovery.dead_worker_policy.clone(),
            description: "What to do when a worker dies",
            editable: true,
            input_type: ConfigInputType::Select {
                options: &["disabled", "notify", "auto"],
            },
        },
        ConfigMenuItem {
            label: "Max Restart Attempts",
            value: config.auto_recovery.max_restart_attempts.to_string(),
            description: "Max restart attempts before giving up",
            editable: true,
            input_type: ConfigInputType::Integer,
        },
    ]
}

/// Draw a configuration menu overlay.
pub fn draw_config_menu(
    frame: &mut Frame,
    area: Rect,
    menu_type: ConfigMenuType,
    items: &[ConfigMenuItem],
    selected: usize,
    editing: bool,
    edit_buffer: &str,
    theme: &crate::theme::Theme,
) {
    // Calculate overlay dimensions
    let overlay_width = 70.min(area.width.saturating_sub(4));
    let content_height = items.len() as u16 + 6; // header + items + footer
    let overlay_height = content_height.max(12).min(area.height.saturating_sub(4));
    let overlay_x = (area.width - overlay_width) / 2;
    let overlay_y = (area.height - overlay_height) / 2;

    let overlay_area = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

    // Clear background
    frame.render_widget(ratatui::widgets::Clear, overlay_area);

    // Build menu content
    let mut lines: Vec<Line> = Vec::new();

    // Title with hotkey hint
    lines.push(Line::from(Span::styled(
        format!("{} Configuration [{}]", menu_type.title(), menu_type.hotkey()),
        Style::default()
            .fg(theme.colors.header)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::raw("")); // Empty line

    // Menu items
    for (i, item) in items.iter().enumerate() {
        let is_selected = i == selected;
        let is_editing = is_selected && editing;

        // Build the line
        let prefix = if is_selected { "> " } else { "  " };

        if is_editing {
            // Editing mode: show input field
            let label_span = Span::styled(
                format!("{}{}:", prefix, item.label),
                Style::default()
                    .fg(theme.colors.hotkey)
                    .add_modifier(Modifier::BOLD),
            );
            let input_span = Span::styled(
                edit_buffer,
                Style::default()
                    .fg(Color::Black)
                    .bg(theme.colors.hotkey),
            );
            let cursor_span = Span::styled(
                "_",
                Style::default()
                    .fg(Color::Black)
                    .bg(theme.colors.hotkey)
                    .add_modifier(Modifier::SLOW_BLINK),
            );
            lines.push(Line::from(vec![label_span, Span::raw(" "), input_span, cursor_span]));
        } else {
            // Normal display mode
            let label_style = if is_selected {
                Style::default()
                    .fg(theme.colors.hotkey)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.colors.text)
            };
            let value_style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(theme.colors.hotkey)
            } else {
                Style::default().fg(theme.colors.text_dim)
            };

            let label_span = Span::styled(format!("{}{}", prefix, item.label), label_style);
            let sep = Span::raw(": ");
            let value_span = Span::styled(&item.value, value_style);

            lines.push(Line::from(vec![label_span, sep, value_span]));
        }
    }

    lines.push(Line::raw("")); // Empty line

    // Footer instructions
    let footer = if editing {
        "Enter: Save | Esc: Cancel"
    } else {
        "Enter/Edit: Change | Esc: Close | Up/Down: Navigate"
    };
    lines.push(Line::from(Span::styled(
        footer,
        Style::default().fg(theme.colors.text_dim),
    )));

    // Create the menu widget
    let menu = Paragraph::new(lines)
        .style(Style::default().fg(theme.colors.text))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.colors.header))
                .title(Span::styled(
                    format!(" {} ", menu_type.title()),
                    Style::default()
                        .fg(theme.colors.header)
                        .add_modifier(Modifier::BOLD),
                ))
                .style(Style::default().bg(Color::Black)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(menu, overlay_area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_menu_type_titles() {
        assert_eq!(ConfigMenuType::Settings.title(), "Settings");
        assert_eq!(ConfigMenuType::Budget.title(), "Budget");
        assert_eq!(ConfigMenuType::Worker.title(), "Workers");
    }

    #[test]
    fn test_config_menu_type_hotkeys() {
        assert_eq!(ConfigMenuType::Settings.hotkey(), "M");
        assert_eq!(ConfigMenuType::Budget.hotkey(), "B");
        assert_eq!(ConfigMenuType::Worker.hotkey(), "U");
    }

    #[test]
    fn test_input_type_integer_validation() {
        let input_type = ConfigInputType::Integer;

        assert!(input_type.validate("100").is_ok());
        assert!(input_type.validate("0").is_ok());
        assert!(input_type.validate("999999").is_ok());

        assert!(input_type.validate("-1").is_err());
        assert!(input_type.validate("1.5").is_err());
        assert!(input_type.validate("abc").is_err());
    }

    #[test]
    fn test_input_type_float_validation() {
        let input_type = ConfigInputType::Float;

        assert!(input_type.validate("100.5").is_ok());
        assert!(input_type.validate("0").is_ok());
        assert!(input_type.validate("-5.0").is_ok());

        assert!(input_type.validate("abc").is_err());
    }

    #[test]
    fn test_input_type_select_validation() {
        let input_type = ConfigInputType::Select {
            options: &["default", "dark", "light", "cyberpunk"],
        };

        assert_eq!(input_type.validate("default").unwrap(), "default");
        assert_eq!(input_type.validate("DEFAULT").unwrap(), "default"); // case insensitive
        assert_eq!(input_type.validate("Dark").unwrap(), "dark"); // preserves case of option

        assert!(input_type.validate("invalid").is_err());
    }

    #[test]
    fn test_build_settings_items() {
        let config = ForgeConfig::default();
        let items = build_settings_items(&config);

        assert!(!items.is_empty());
        assert!(items.iter().any(|i| i.label == "Refresh Interval"));
        assert!(items.iter().any(|i| i.label == "Theme"));
    }

    #[test]
    fn test_build_budget_items() {
        let config = ForgeConfig::default();
        let items = build_budget_items(&config);

        assert!(!items.is_empty());
        assert!(items.iter().any(|i| i.label == "Monthly Budget"));
        assert!(items.iter().any(|i| i.label == "Warning Threshold"));
    }

    #[test]
    fn test_build_worker_items() {
        let config = ForgeConfig::default();
        let items = build_worker_items(&config);

        assert!(!items.is_empty());
        assert!(items.iter().any(|i| i.label == "Max Workers"));
        assert!(items.iter().any(|i| i.label == "Auto-Recovery"));
    }
}
