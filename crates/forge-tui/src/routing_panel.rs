//! Routing analytics panel for the FORGE TUI.
//!
//! This module provides visualization of intelligent model routing:
//! - Task complexity scoring distribution
//! - Model tier routing decisions
//! - Cost savings from intelligent routing
//! - Per-model assignment statistics

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Row, Table, Widget},
};

use forge_core::types::WorkerTier;
use forge_worker::router::{RoutingDecision, RoutingReason, RouterStats};

/// Routing data for display in the TUI.
#[derive(Debug, Clone, Default)]
pub struct RoutingData {
    /// Total routing decisions made
    pub total_decisions: usize,
    /// Decisions by tier (premium, standard, budget)
    pub by_tier: std::collections::HashMap<WorkerTier, usize>,
    /// Decisions by model
    pub by_model: std::collections::HashMap<String, usize>,
    /// Decisions by reason
    pub by_reason: std::collections::HashMap<RoutingReason, usize>,
    /// Recent routing decisions (last 20)
    pub recent_decisions: Vec<RoutingDecision>,
    /// Average complexity score of routed tasks
    pub avg_complexity: f64,
    /// Complexity distribution (simple, moderate, complex counts)
    pub complexity_distribution: (usize, usize, usize),
    /// Estimated cost savings from routing (vs always using premium)
    pub estimated_savings: f64,
    /// Whether data is loading
    pub is_loading: bool,
    /// Last update timestamp
    pub last_update: Option<std::time::Instant>,
}

impl RoutingData {
    /// Create new empty routing data.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create loading state.
    pub fn loading() -> Self {
        Self {
            is_loading: true,
            ..Default::default()
        }
    }

    /// Update from router stats.
    pub fn update_from_stats(&mut self, stats: &RouterStats, recent: Vec<RoutingDecision>) {
        self.total_decisions = stats.total_decisions;
        self.by_tier = stats.by_tier.clone();
        self.by_model = stats.by_model.clone();
        self.by_reason = stats.by_reason.clone();
        self.recent_decisions = recent;
        self.last_update = Some(std::time::Instant::now());
        self.is_loading = false;
    }

    /// Calculate estimated savings from routing decisions.
    ///
    /// This estimates savings by comparing actual routing tier vs
    /// what would have been used with naive (always premium) routing.
    pub fn calculate_savings(&mut self) {
        // Premium model cost multiplier (relative)
        const PREMIUM_COST: f64 = 1.0;
        const STANDARD_COST: f64 = 0.2;  // ~20% of premium
        const BUDGET_COST: f64 = 0.05;   // ~5% of premium

        let premium_count = self.by_tier.get(&WorkerTier::Premium).copied().unwrap_or(0) as f64;
        let standard_count = self.by_tier.get(&WorkerTier::Standard).copied().unwrap_or(0) as f64;
        let budget_count = self.by_tier.get(&WorkerTier::Budget).copied().unwrap_or(0) as f64;

        // Cost if all went to premium
        let naive_cost = (premium_count + standard_count + budget_count) * PREMIUM_COST;

        // Actual cost with routing
        let actual_cost = (premium_count * PREMIUM_COST)
            + (standard_count * STANDARD_COST)
            + (budget_count * BUDGET_COST);

        // Savings (simplified - assumes average $0.10 per task)
        let avg_task_cost = 0.10;
        self.estimated_savings = (naive_cost - actual_cost) * avg_task_cost;
    }

    /// Get percentage of tasks routed to budget tier.
    pub fn budget_percentage(&self) -> f64 {
        if self.total_decisions == 0 {
            return 0.0;
        }
        let budget_count = self.by_tier.get(&WorkerTier::Budget).copied().unwrap_or(0);
        (budget_count as f64 / self.total_decisions as f64) * 100.0
    }

    /// Get percentage of tasks routed to standard tier.
    pub fn standard_percentage(&self) -> f64 {
        if self.total_decisions == 0 {
            return 0.0;
        }
        let standard_count = self.by_tier.get(&WorkerTier::Standard).copied().unwrap_or(0);
        (standard_count as f64 / self.total_decisions as f64) * 100.0
    }

    /// Get percentage of tasks routed to premium tier.
    pub fn premium_percentage(&self) -> f64 {
        if self.total_decisions == 0 {
            return 0.0;
        }
        let premium_count = self.by_tier.get(&WorkerTier::Premium).copied().unwrap_or(0);
        (premium_count as f64 / self.total_decisions as f64) * 100.0
    }
}

/// Widget for displaying routing analytics.
pub struct RoutingPanel<'a> {
    /// Routing data to display
    data: &'a RoutingData,
}

impl<'a> RoutingPanel<'a> {
    /// Create a new routing panel.
    pub fn new(data: &'a RoutingData) -> Self {
        Self { data }
    }
}

impl Widget for RoutingPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Create block with border
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Line::from(vec![
                Span::styled("▸ ", Style::default().fg(Color::Cyan)),
                Span::styled("Model Routing", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]))
            .title_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        block.render(area, buf);

        if self.data.is_loading {
            let loading = Paragraph::new("Loading routing data...")
                .style(Style::default().fg(Color::DarkGray));
            loading.render(inner, buf);
            return;
        }

        if self.data.total_decisions == 0 {
            let empty = Paragraph::new(
                "No routing decisions yet.\n\n\
                 Routing decisions appear when workers\n\
                 are assigned tasks based on complexity."
            )
            .style(Style::default().fg(Color::DarkGray));
            empty.render(inner, buf);
            return;
        }

        // Split into sections
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),   // Summary stats
                Constraint::Length(8),   // Tier distribution
                Constraint::Min(10),     // Recent decisions table
            ])
            .split(inner);

        // Render summary
        self.render_summary(chunks[0], buf);

        // Render tier distribution
        self.render_tier_distribution(chunks[1], buf);

        // Render recent decisions
        self.render_recent_decisions(chunks[2], buf);
    }
}

impl RoutingPanel<'_> {
    fn render_summary(&self, area: Rect, buf: &mut Buffer) {
        let lines = vec![
            Line::from(vec![
                Span::styled("Total Decisions: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}", self.data.total_decisions),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::raw("   "),
                Span::styled("Est. Savings: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("${:.2}", self.data.estimated_savings),
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Avg Complexity: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:.0}", self.data.avg_complexity),
                    complexity_color(self.data.avg_complexity as u32),
                ),
                Span::raw("   "),
                Span::styled("Routing Efficiency: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:.0}%", self.routing_efficiency()),
                    Style::default().fg(Color::Green),
                ),
            ]),
            Line::from(vec![
                Span::styled("Complexity Distribution: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{} simple", self.data.complexity_distribution.0),
                    Style::default().fg(Color::Green),
                ),
                Span::raw(" | "),
                Span::styled(
                    format!("{} moderate", self.data.complexity_distribution.1),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(" | "),
                Span::styled(
                    format!("{} complex", self.data.complexity_distribution.2),
                    Style::default().fg(Color::Red),
                ),
            ]),
        ];

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }

    fn render_tier_distribution(&self, area: Rect, buf: &mut Buffer) {
        let premium = self.data.by_tier.get(&WorkerTier::Premium).copied().unwrap_or(0);
        let standard = self.data.by_tier.get(&WorkerTier::Standard).copied().unwrap_or(0);
        let budget = self.data.by_tier.get(&WorkerTier::Budget).copied().unwrap_or(0);
        let total = self.data.total_decisions.max(1) as f64;

        let rows = vec![
            Row::new(vec![
                Span::styled("Premium", Style::default().fg(Color::Magenta)),
                Span::raw(format!("{}", premium)),
                Span::raw(format!("{:.1}%", (premium as f64 / total) * 100.0)),
                render_bar((premium as f64 / total) * 100.0, 20),
            ]),
            Row::new(vec![
                Span::styled("Standard", Style::default().fg(Color::Cyan)),
                Span::raw(format!("{}", standard)),
                Span::raw(format!("{:.1}%", (standard as f64 / total) * 100.0)),
                render_bar((standard as f64 / total) * 100.0, 20),
            ]),
            Row::new(vec![
                Span::styled("Budget", Style::default().fg(Color::Green)),
                Span::raw(format!("{}", budget)),
                Span::raw(format!("{:.1}%", (budget as f64 / total) * 100.0)),
                render_bar((budget as f64 / total) * 100.0, 20),
            ]),
        ];

        let table = Table::new(
            rows,
            [
                Constraint::Length(10),
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Min(20),
            ],
        )
        .header(
            Row::new(vec![
                Span::styled("Tier", Style::default().fg(Color::DarkGray)),
                Span::styled("Count", Style::default().fg(Color::DarkGray)),
                Span::styled("Pct", Style::default().fg(Color::DarkGray)),
                Span::styled("Distribution", Style::default().fg(Color::DarkGray)),
            ])
            .style(Style::default().add_modifier(Modifier::BOLD)),
        );

        table.render(area, buf);
    }

    fn render_recent_decisions(&self, area: Rect, buf: &mut Buffer) {
        let title = Line::from(vec![
            Span::styled("▸ ", Style::default().fg(Color::Cyan)),
            Span::styled("Recent Routing Decisions", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]);

        let title_block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(title);

        let inner = title_block.inner(area);
        title_block.render(area, buf);

        // Take last 10 decisions
        let decisions: Vec<_> = self.data.recent_decisions.iter().rev().take(10).collect();

        if decisions.is_empty() {
            let empty = Paragraph::new("No recent decisions")
                .style(Style::default().fg(Color::DarkGray));
            empty.render(inner, buf);
            return;
        }

        let rows: Vec<Row> = decisions
            .iter()
            .map(|d| {
                let tier_color = tier_color(d.tier);
                Row::new(vec![
                    Span::styled(&d.task_id, Style::default().fg(Color::DarkGray)),
                    Span::styled(&d.model_id, tier_color),
                    Span::styled(tier_label(d.tier), tier_color),
                    Span::styled(reason_label(d.reason), Style::default().fg(Color::DarkGray)),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Length(20),
                Constraint::Length(10),
                Constraint::Min(20),
            ],
        )
        .header(
            Row::new(vec![
                Span::styled("Task", Style::default().fg(Color::DarkGray)),
                Span::styled("Model", Style::default().fg(Color::DarkGray)),
                Span::styled("Tier", Style::default().fg(Color::DarkGray)),
                Span::styled("Reason", Style::default().fg(Color::DarkGray)),
            ])
            .style(Style::default().add_modifier(Modifier::BOLD)),
        );

        table.render(inner, buf);
    }

    fn routing_efficiency(&self) -> f64 {
        // Efficiency = percentage of tasks NOT routed to premium when not needed
        // Higher budget/standard usage = higher efficiency (assuming good routing)
        if self.data.total_decisions == 0 {
            return 0.0;
        }
        // Budget is 100% efficient, Standard is 50%, Premium is 0% (for efficiency metric)
        let budget_pct = self.data.budget_percentage();
        let standard_pct = self.data.standard_percentage();
        budget_pct + (standard_pct * 0.5)
    }
}

/// Get color for complexity score.
fn complexity_color(score: u32) -> Color {
    match score {
        0..=30 => Color::Green,
        31..=60 => Color::Yellow,
        _ => Color::Red,
    }
}

/// Get color for tier.
fn tier_color(tier: WorkerTier) -> Color {
    match tier {
        WorkerTier::Premium => Color::Magenta,
        WorkerTier::Standard => Color::Cyan,
        WorkerTier::Budget => Color::Green,
    }
}

/// Get label for tier.
fn tier_label(tier: WorkerTier) -> &'static str {
    match tier {
        WorkerTier::Premium => "Premium",
        WorkerTier::Standard => "Standard",
        WorkerTier::Budget => "Budget",
    }
}

/// Get label for routing reason.
fn reason_label(reason: RoutingReason) -> &'static str {
    match reason {
        RoutingReason::PriorityBased => "Priority",
        RoutingReason::ComplexityBased => "Complexity",
        RoutingReason::SubscriptionPreference => "Subscription",
        RoutingReason::LoadBalancing => "Load Balance",
        RoutingReason::Fallback => "Fallback",
        RoutingReason::LabelBased => "Label",
        RoutingReason::Default => "Default",
    }
}

/// Render a horizontal bar.
fn render_bar(percentage: f64, width: usize) -> Span<'static> {
    let filled = ((percentage / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width - filled;

    let bar = format!("{}{}",
        "█".repeat(filled),
        "░".repeat(empty),
    );

    Span::styled(bar, Style::default().fg(Color::Cyan))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routing_data_default() {
        let data = RoutingData::new();
        assert_eq!(data.total_decisions, 0);
        assert!(data.by_tier.is_empty());
        // New data is not loading - it's just empty
        assert!(!data.is_loading);
    }

    #[test]
    fn test_routing_data_percentages() {
        let mut data = RoutingData::new();
        data.total_decisions = 100;
        data.by_tier.insert(WorkerTier::Premium, 30);
        data.by_tier.insert(WorkerTier::Standard, 40);
        data.by_tier.insert(WorkerTier::Budget, 30);
        data.is_loading = false;

        assert_eq!(data.premium_percentage(), 30.0);
        assert_eq!(data.standard_percentage(), 40.0);
        assert_eq!(data.budget_percentage(), 30.0);
    }

    #[test]
    fn test_routing_data_calculate_savings() {
        let mut data = RoutingData::new();
        data.total_decisions = 100;
        data.by_tier.insert(WorkerTier::Premium, 30);
        data.by_tier.insert(WorkerTier::Standard, 40);
        data.by_tier.insert(WorkerTier::Budget, 30);

        data.calculate_savings();

        // Should have some savings
        assert!(data.estimated_savings > 0.0);
    }

    #[test]
    fn test_routing_panel_new() {
        let data = RoutingData::new();
        let panel = RoutingPanel::new(&data);
        assert!(std::ptr::eq(panel.data, &data));
    }
}
