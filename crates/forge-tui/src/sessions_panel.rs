//! Sessions panel for displaying connected users in team collaboration mode.
//!
//! Shows active user sessions, their roles, and current views.

use crate::view::LayoutMode;
use forge_core::{UserSession, UserRole, SessionStatus};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

/// Panel data for sessions view.
pub struct SessionsPanel {
    sessions: Vec<UserSession>,
    selected_index: usize,
}

impl SessionsPanel {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            selected_index: 0,
        }
    }

    pub fn set_sessions(&mut self, sessions: Vec<UserSession>) {
        self.sessions = sessions;
        if self.selected_index >= self.sessions.len() && !self.sessions.is_empty() {
            self.selected_index = 0;
        }
    }

    pub fn sessions(&self) -> &[UserSession] {
        &self.sessions
    }

    pub fn selected_session(&self) -> Option<&UserSession> {
        self.sessions.get(self.selected_index)
    }

    pub fn next(&mut self) {
        if !self.sessions.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.sessions.len();
        }
    }

    pub fn previous(&mut self) {
        if !self.sessions.is_empty() {
            self.selected_index = if self.selected_index == 0 {
                self.sessions.len() - 1
            } else {
                self.selected_index - 1
            };
        }
    }

    /// Add a user to the sessions panel.
    pub fn add_user(&mut self, user_id: String, display_name: String, role: UserRole) {
        // Check if user already exists
        if self.sessions.iter().any(|s| s.user_id == user_id) {
            return;
        }

        let session = UserSession::new(
            &format!("session-{}", user_id),
            &user_id,
            &display_name,
            role,
        );
        self.sessions.push(session);
    }

    /// Remove a user from the sessions panel.
    pub fn remove_user(&mut self, user_id: &str) {
        self.sessions.retain(|s| s.user_id != user_id);
        if self.selected_index >= self.sessions.len() && !self.sessions.is_empty() {
            self.selected_index = self.sessions.len().saturating_sub(1);
        }
    }

    /// Draw the sessions panel.
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, layout_mode: LayoutMode, focused: bool) {
        let border_style = if focused {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };

        // Calculate layout
        let (header_height, _table_area) = if layout_mode == LayoutMode::Narrow {
            (3, area)
        } else {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
                .split(area);
            (3, chunks[1])
        };

        // Header
        let header = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Connected Users", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(" "),
                Span::styled(format!("({})", self.sessions.len()), Style::default().fg(Color::Gray)),
            ]),
        ])
        .block(Block::default())
        .alignment(Alignment::Center);

        frame.render_widget(header, Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: header_height,
        });

        if self.sessions.is_empty() {
            let empty = Paragraph::new("No active sessions. Server mode not enabled.")
                .block(Block::default()
                    .title(" Team Sessions ")
                    .title_style(border_style)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(border_style))
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true });

            frame.render_widget(empty, Rect {
                x: area.x,
                y: area.y + header_height,
                width: area.width,
                height: area.height.saturating_sub(header_height),
            });
            return;
        }

        // Build table rows
        let rows: Vec<Row> = self.sessions.iter().enumerate().map(|(i, session)| {
            let is_selected = i == self.selected_index;

            let role_color = match session.role {
                UserRole::Admin => Color::Red,
                UserRole::Operator => Color::Yellow,
                UserRole::Viewer => Color::Gray,
            };

            let status_indicator = match session.status {
                SessionStatus::Active => "●",
                SessionStatus::Idle => "○",
                SessionStatus::Disconnected => "○",
            };

            let status_color = match session.status {
                SessionStatus::Active => Color::Green,
                SessionStatus::Idle => Color::Yellow,
                SessionStatus::Disconnected => Color::Gray,
            };

            let style = if is_selected {
                Style::default().bg(Color::DarkGray).add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(status_indicator.to_string()).style(Style::default().fg(status_color)),
                Cell::from(session.display_name.clone()).style(style),
                Cell::from(session.user_id.clone()).style(Style::default().fg(Color::Blue)),
                Cell::from(format!("{:?}", session.role)).style(Style::default().fg(role_color)),
                Cell::from(session.current_view.as_deref().unwrap_or("None").to_string()).style(Style::default().fg(Color::Gray)),
            ]).style(style)
        }).collect();

        // Table header
        let header_cells = vec![
            Cell::from(""),
            Cell::from("Name"),
            Cell::from("User"),
            Cell::from("Role"),
            Cell::from("View"),
        ];

        let widths = if layout_mode == LayoutMode::Narrow {
            vec![
                Constraint::Length(1),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(20),
                Constraint::Percentage(30),
            ]
        } else {
            vec![
                Constraint::Length(1),
                Constraint::Min(15),
                Constraint::Min(15),
                Constraint::Min(10),
                Constraint::Min(15),
            ]
        };

        let table = Table::new(rows, widths)
            .header(Row::new(header_cells).style(Style::default().fg(Color::Cyan)))
            .block(Block::default()
                .title(" Team Sessions ")
                .title_style(border_style)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(border_style))
            .column_spacing(1);

        frame.render_widget(table, Rect {
            x: area.x,
            y: area.y + header_height,
            width: area.width,
            height: area.height.saturating_sub(header_height),
        });
    }
}

impl Default for SessionsPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_session(user_id: &str, display_name: &str, role: UserRole) -> UserSession {
        UserSession::new("test-session", user_id, display_name, role)
    }

    #[test]
    fn test_sessions_panel_empty() {
        let panel = SessionsPanel::new();
        assert!(panel.sessions().is_empty());
        assert!(panel.selected_session().is_none());
    }

    #[test]
    fn test_sessions_panel_navigation() {
        let mut panel = SessionsPanel::new();
        panel.set_sessions(vec![
            create_test_session("user1", "User One", UserRole::Viewer),
            create_test_session("user2", "User Two", UserRole::Operator),
            create_test_session("user3", "User Three", UserRole::Admin),
        ]);

        assert_eq!(panel.selected_index, 0);
        panel.next();
        assert_eq!(panel.selected_index, 1);
        panel.next();
        assert_eq!(panel.selected_index, 2);
        panel.next();
        assert_eq!(panel.selected_index, 0); // wrap around

        panel.previous();
        assert_eq!(panel.selected_index, 2); // wrap to end
    }

    #[test]
    fn test_sessions_panel_selection() {
        let mut panel = SessionsPanel::new();
        panel.set_sessions(vec![
            create_test_session("user1", "User One", UserRole::Viewer),
            create_test_session("user2", "User Two", UserRole::Operator),
        ]);

        let selected = panel.selected_session();
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().user_id, "user1");
    }
}
