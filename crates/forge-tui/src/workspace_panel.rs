//! Workspace management panel for multi-workspace coordination.
//!
//! This module provides the UI for managing multiple FORGE workspaces,
//! including switching between workspaces and viewing aggregated statistics.

use crate::theme::Theme;
use forge_core::WorkspaceRegistry;
use forge_worker::discovery::CrossWorkspaceDiscoveryResult;
use forge_cost::MultiWorkspaceCostSummary;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
    Frame,
};
use std::collections::HashMap;

/// Data for the workspace panel display.
#[derive(Debug, Default, Clone)]
pub struct WorkspacePanelData {
    /// All configured workspaces
    pub workspaces: Vec<WorkspaceDisplayInfo>,
    /// Currently selected workspace index
    pub selected_index: usize,
    /// Aggregated statistics across all workspaces
    pub aggregate_stats: WorkspaceAggregateDisplay,
    /// Validation issues found
    pub issues: Vec<WorkspaceIssueDisplay>,
    /// Whether workspace switching is in progress
    pub switching: bool,
}

/// Display information for a single workspace.
#[derive(Debug, Clone)]
pub struct WorkspaceDisplayInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub enabled: bool,
    pub is_current: bool,
    pub is_accessible: bool,
    pub has_workers: bool,
    pub worker_count: usize,
    pub has_costs: bool,
    pub total_cost: f64,
    pub has_beads: bool,
    pub bead_count: usize,
    pub last_updated: String,
}

/// Aggregated statistics display.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceAggregateDisplay {
    pub total_workspaces: usize,
    pub enabled_workspaces: usize,
    pub total_workers: usize,
    pub total_cost: f64,
    pub total_beads: usize,
}

/// Display wrapper for workspace issues.
#[derive(Debug, Clone)]
pub struct WorkspaceIssueDisplay {
    pub workspace_id: String,
    pub severity: String,
    pub message: String,
}

impl WorkspacePanelData {
    /// Create empty workspace panel data.
    pub fn new() -> Self {
        Self::default()
    }

    /// Update from a workspace registry.
    pub fn update_from_registry(&mut self, registry: &WorkspaceRegistry, current_id: Option<&str>) {
        self.workspaces.clear();

        for ws in registry.all() {
            let is_current = current_id.map_or(false, |id| id == ws.id);

            self.workspaces.push(WorkspaceDisplayInfo {
                id: ws.id.clone(),
                name: ws.name.clone(),
                path: ws.path.display().to_string(),
                enabled: ws.enabled,
                is_current,
                is_accessible: ws.is_accessible(),
                has_workers: false, // Will be updated by discovery
                worker_count: 0,
                has_costs: ws.has_cost_data(),
                total_cost: 0.0, // Will be updated by cost query
                has_beads: ws.has_beads(),
                bead_count: 0,   // Will be updated by bead query
                last_updated: String::new(),
            });
        }

        // Update aggregate stats
        self.aggregate_stats.total_workspaces = registry.len();
        self.aggregate_stats.enabled_workspaces = registry.enabled().len();
    }

    /// Get the currently selected workspace.
    pub fn selected(&self) -> Option<&WorkspaceDisplayInfo> {
        if self.selected_index < self.workspaces.len() {
            self.workspaces.get(self.selected_index)
        } else {
            self.workspaces.first()
        }
    }

    /// Select the next workspace.
    pub fn select_next(&mut self) {
        if self.workspaces.is_empty() {
            return;
        }
        self.selected_index = (self.selected_index + 1) % self.workspaces.len();
    }

    /// Select the previous workspace.
    pub fn select_prev(&mut self) {
        if self.workspaces.is_empty() {
            return;
        }
        if self.selected_index == 0 {
            self.selected_index = self.workspaces.len() - 1;
        } else {
            self.selected_index -= 1;
        }
    }

    /// Select a specific workspace by index.
    pub fn select_index(&mut self, index: usize) {
        if index < self.workspaces.len() {
            self.selected_index = index;
        }
    }

    /// Update worker count for a workspace.
    pub fn update_worker_count(&mut self, workspace_id: &str, count: usize) {
        if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == workspace_id) {
            ws.worker_count = count;
            ws.has_workers = count > 0;
            self.aggregate_stats.total_workers += count;
        }
    }

    /// Update cost for a workspace.
    pub fn update_cost(&mut self, workspace_id: &str, cost: f64) {
        if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == workspace_id) {
            ws.total_cost = cost;
            self.aggregate_stats.total_cost += cost;
        }
    }

    /// Update bead count for a workspace.
    pub fn update_bead_count(&mut self, workspace_id: &str, count: usize) {
        if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == workspace_id) {
            ws.bead_count = count;
            self.aggregate_stats.total_beads += count;
        }
    }

    /// Update worker counts from cross-workspace discovery result.
    pub fn update_from_discovery(&mut self, discovery: &CrossWorkspaceDiscoveryResult) {
        // Reset worker counts
        self.aggregate_stats.total_workers = 0;

        for (workspace_id, workers) in &discovery.by_workspace {
            self.update_worker_count(workspace_id, workers.len());
        }
    }

    /// Update costs from multi-workspace cost summary.
    pub fn update_from_cost_summary(&mut self, summary: &MultiWorkspaceCostSummary) {
        // Reset total cost
        self.aggregate_stats.total_cost = 0.0;

        for (workspace_id, ws_cost) in &summary.by_workspace {
            if ws_cost.has_cost_data {
                self.update_cost(workspace_id, ws_cost.total_cost);
            }
        }
    }

    /// Update bead counts from a map of workspace_id -> bead_count.
    pub fn update_from_bead_counts(&mut self, bead_counts: &HashMap<String, usize>) {
        // Reset total beads
        self.aggregate_stats.total_beads = 0;

        for (workspace_id, count) in bead_counts {
            self.update_bead_count(workspace_id, *count);
        }
    }
}

/// Draw the workspace management panel.
pub fn draw_workspace_panel(
    f: &mut Frame,
    area: Rect,
    data: &WorkspacePanelData,
    theme: &Theme,
) {
    // Split into header, main content, and footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(0)
        .constraints([
            Constraint::Length(3),  // Header with stats
            Constraint::Min(10),    // Main workspace list
            Constraint::Length(3),  // Footer with hints
        ])
        .split(area);

    draw_workspace_header(f, chunks[0], data, theme);
    draw_workspace_list(f, chunks[1], data, theme);
    draw_workspace_footer(f, chunks[2], theme);
}

/// Draw the workspace header with aggregated statistics.
fn draw_workspace_header(
    f: &mut Frame,
    area: Rect,
    data: &WorkspacePanelData,
    theme: &Theme,
) {
    let title = Span::styled(
        "Multi-Workspace Management",
        Style::default()
            .fg(theme.colors.header)
            .add_modifier(Modifier::BOLD),
    );

    let stats = vec![
        Span::styled(
            format!(" Workspaces: {}/{}", data.aggregate_stats.enabled_workspaces, data.aggregate_stats.total_workspaces),
            Style::default().fg(theme.colors.hotkey),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("Workers: {}", data.aggregate_stats.total_workers),
            Style::default().fg(theme.colors.status_healthy),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("Cost: ${:.2}", data.aggregate_stats.total_cost),
            Style::default().fg(theme.colors.status_warning),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("Beads: {}", data.aggregate_stats.total_beads),
            Style::default().fg(theme.colors.action_view),
        ),
    ];

    let header_text = Text::from(vec![
        Line::from(title),
        Line::from(stats),
    ]);

    let paragraph = Paragraph::new(header_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.colors.border_dim))
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, area);
}

/// Draw the workspace list table.
fn draw_workspace_list(
    f: &mut Frame,
    area: Rect,
    data: &WorkspacePanelData,
    theme: &Theme,
) {
    if data.workspaces.is_empty() {
        let no_workspaces = Paragraph::new(
            Text::styled(
                "No workspaces configured.\n\nAdd workspaces in ~/.forge/config.yaml under workspaces.workspace_paths",
                Style::default().fg(theme.colors.text_dim),
            )
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.colors.border_dim))
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

        f.render_widget(no_workspaces, area);
        return;
    }

    // Define table headers
    let header = vec![
        "Status",
        "ID",
        "Name",
        "Workers",
        "Cost",
        "Beads",
        "Path",
    ];

    // Build table rows
    let rows: Vec<Row> = data.workspaces
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            let is_selected = i == data.selected_index;
            let base_style = if is_selected {
                Style::default()
                    .fg(theme.colors.focus_highlight)
                    .add_modifier(Modifier::BOLD)
            } else if !ws.enabled {
                Style::default().fg(theme.colors.text_dim)
            } else {
                Style::default().fg(theme.colors.text)
            };

            let status = if ws.is_current {
                Span::styled("★", Style::default().fg(theme.colors.status_healthy))
            } else if !ws.enabled {
                Span::styled("○", Style::default().fg(theme.colors.text_dim))
            } else if !ws.is_accessible {
                Span::styled("✕", Style::default().fg(theme.colors.status_error))
            } else {
                Span::styled("●", Style::default().fg(theme.colors.status_healthy))
            };

            let workers = if ws.has_workers {
                format!("{}", ws.worker_count)
            } else {
                "-".to_string()
            };

            let cost = if ws.has_costs {
                format!("${:.2}", ws.total_cost)
            } else {
                "-".to_string()
            };

            let beads = if ws.has_beads {
                format!("{}", ws.bead_count)
            } else {
                "-".to_string()
            };

            let path = if ws.path.len() > 30 {
                format!("...{}", &ws.path[ws.path.len() - 27..])
            } else {
                ws.path.clone()
            };

            let cells = vec![
                Cell::from(status),
                Cell::from(ws.id.as_str()),
                Cell::from(ws.name.as_str()),
                Cell::from(workers),
                Cell::from(cost),
                Cell::from(beads),
                Cell::from(path),
            ];

            Row::new(cells).style(base_style)
        })
        .collect();

    // Calculate column widths
    let widths = vec![
        Constraint::Length(8),  // Status
        Constraint::Length(12), // ID
        Constraint::Length(15), // Name
        Constraint::Length(8),  // Workers
        Constraint::Length(8),  // Cost
        Constraint::Length(8),  // Beads
        Constraint::Min(30),    // Path
    ];

    let table = Table::new(rows, widths)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.colors.border_dim))
        )
        .header(
            Row::new(header.iter().map(|h| Cell::from(*h)))
                .style(
                    Style::default()
                        .fg(theme.colors.header)
                        .add_modifier(Modifier::BOLD)
                )
        )
        .column_spacing(1)
        .row_highlight_style(
            Style::default()
                .bg(theme.colors.focus_highlight)
                .add_modifier(Modifier::BOLD)
        );

    f.render_widget(table, area);
}

/// Draw the workspace footer with key hints.
fn draw_workspace_footer(f: &mut Frame, area: Rect, theme: &Theme) {
    let hints = vec![
        Span::styled("[Enter]", Style::default().fg(theme.colors.hotkey).add_modifier(Modifier::BOLD)),
        Span::styled(" Switch ", Style::default().fg(theme.colors.text)),
        Span::styled("[↑/↓]", Style::default().fg(theme.colors.hotkey).add_modifier(Modifier::BOLD)),
        Span::styled(" Navigate ", Style::default().fg(theme.colors.text)),
        Span::styled("[+]", Style::default().fg(theme.colors.hotkey).add_modifier(Modifier::BOLD)),
        Span::styled(" Add ", Style::default().fg(theme.colors.text)),
        Span::styled("[-]", Style::default().fg(theme.colors.hotkey).add_modifier(Modifier::BOLD)),
        Span::styled(" Remove ", Style::default().fg(theme.colors.text)),
        Span::styled("[Esc]", Style::default().fg(theme.colors.hotkey).add_modifier(Modifier::BOLD)),
        Span::styled(" Back ", Style::default().fg(theme.colors.text)),
    ];

    let paragraph = Paragraph::new(Line::from(hints))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.colors.border_dim))
        )
        .alignment(Alignment::Center);

    f.render_widget(paragraph, area);
}

/// Draw workspace detail overlay.
pub fn draw_workspace_detail_overlay(
    f: &mut Frame,
    data: &WorkspacePanelData,
    theme: &Theme,
) {
    let ws = match data.selected() {
        Some(ws) => ws,
        None => return,
    };

    // Create overlay area centered on screen
    let area = f.area();
    let width = area.width.min(80);
    let height = area.height.min(20);
    let x = (area.width - width) / 2;
    let y = (area.height - height) / 2;
    let overlay_area = Rect::new(x, y, width, height);

    // Clear the area behind the overlay
    f.render_widget(Clear, overlay_area);

    let details = vec![
        Line::from(vec![
            Span::styled("Workspace Details", Style::default().fg(theme.colors.header).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("ID: ", Style::default().fg(theme.colors.action_view)),
            Span::styled(&ws.id, Style::default().fg(theme.colors.text)),
        ]),
        Line::from(vec![
            Span::styled("Name: ", Style::default().fg(theme.colors.action_view)),
            Span::styled(&ws.name, Style::default().fg(theme.colors.text)),
        ]),
        Line::from(vec![
            Span::styled("Path: ", Style::default().fg(theme.colors.action_view)),
            Span::styled(&ws.path, Style::default().fg(theme.colors.text)),
        ]),
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(theme.colors.action_view)),
            Span::styled(
                if ws.is_current { "Current ★" }
                else if ws.enabled { "Enabled" }
                else { "Disabled" },
                Style::default().fg(
                    if ws.is_current { theme.colors.status_healthy }
                    else if ws.enabled { theme.colors.status_warning }
                    else { theme.colors.text_dim }
                ),
            ),
        ]),
        Line::from(vec![
            Span::styled("Accessible: ", Style::default().fg(theme.colors.action_view)),
            Span::styled(
                if ws.is_accessible { "Yes" } else { "No" },
                Style::default().fg(if ws.is_accessible { theme.colors.status_healthy } else { theme.colors.status_error }),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Workers: ", Style::default().fg(theme.colors.action_view)),
            Span::styled(
                format!("{}", ws.worker_count),
                Style::default().fg(theme.colors.text),
            ),
        ]),
        Line::from(vec![
            Span::styled("Total Cost: ", Style::default().fg(theme.colors.action_view)),
            Span::styled(
                format!("${:.2}", ws.total_cost),
                Style::default().fg(theme.colors.text),
            ),
        ]),
        Line::from(vec![
            Span::styled("Beads: ", Style::default().fg(theme.colors.action_view)),
            Span::styled(
                format!("{}", ws.bead_count),
                Style::default().fg(theme.colors.text),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Press [Esc] to close, [Enter] to switch to this workspace", Style::default().fg(theme.colors.text_dim)),
        ]),
    ];

    let paragraph = Paragraph::new(Text::from(details))
        .block(
            Block::default()
                .title(vec![Span::styled(
                    format!("Workspace: {}", ws.name),
                    Style::default().fg(theme.colors.action_view).add_modifier(Modifier::BOLD),
                )])
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.colors.border_dim))
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, overlay_area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_panel_data_new() {
        let data = WorkspacePanelData::new();
        assert!(data.workspaces.is_empty());
        assert_eq!(data.selected_index, 0);
    }

    #[test]
    fn test_workspace_panel_data_navigation() {
        let mut data = WorkspacePanelData::new();
        data.workspaces.push(create_test_workspace("ws1"));
        data.workspaces.push(create_test_workspace("ws2"));

        assert_eq!(data.selected_index, 0);

        data.select_next();
        assert_eq!(data.selected_index, 1);

        data.select_next();
        assert_eq!(data.selected_index, 0); // Wraps around

        data.select_prev();
        assert_eq!(data.selected_index, 1);
    }

    fn create_test_workspace(id: &str) -> WorkspaceDisplayInfo {
        WorkspaceDisplayInfo {
            id: id.to_string(),
            name: id.to_string(),
            path: format!("/tmp/{}", id),
            enabled: true,
            is_current: false,
            is_accessible: true,
            has_workers: false,
            worker_count: 0,
            has_costs: false,
            total_cost: 0.0,
            has_beads: false,
            bead_count: 0,
            last_updated: String::new(),
        }
    }
}
