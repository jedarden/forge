//! Local handling for a small set of common chat queries.
//!
//! Fast-path matching is deliberately conservative: input is normalized for
//! case, surrounding whitespace, and terminal punctuation, then compared with
//! an explicit list of phrases. Anything not in that list falls through to the
//! configured chat provider.

/// The local operation corresponding to a matched query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastPathAction {
    /// Show today's cost analytics (the equivalent of
    /// `get_cost_analytics {"timeframe":"today"}`).
    ShowCosts,
    /// Show the worker status view.
    ShowWorkers,
    /// Show the task queue, optionally filtered to one priority.
    ShowTasks { priority: Option<u8> },
    /// Switch to a named dashboard view.
    SwitchView(ViewTarget),
}

/// Result of trying the local fast path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastPathResult {
    /// The query matched an explicit local action.
    Handled(FastPathAction),
    /// The query is not in the local phrase list and should use the LLM path.
    FallThrough,
}

/// Dashboard view targets understood by the chat fast path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewTarget {
    Overview,
    Workers,
    Tasks,
    Costs,
    Alerts,
    Logs,
    Chat,
}

/// Stateless matcher for the explicit local query list.
#[derive(Debug, Clone, Copy, Default)]
pub struct FastPathMatcher;

const COST_QUERIES: &[&str] = &[
    "what did i spend today",
    "how much did i spend today",
    "how much have i spent today",
    "spending today",
    "cost today",
    "today's cost",
    "today's spending",
    "show costs",
    "cost analytics",
    "my costs",
    "cost summary",
];

const WORKER_QUERIES: &[&str] = &[
    "show workers",
    "worker status",
    "list workers",
    "workers",
    "status of workers",
    "how are my workers",
];

const TASK_QUERIES: &[&str] = &[
    "show tasks",
    "list tasks",
    "my tasks",
    "task queue",
    "ready tasks",
];

const PRIORITY_TASK_QUERIES: &[(&str, u8)] = &[
    ("show p0 tasks", 0),
    ("priority 0 tasks", 0),
    ("show p1 tasks", 1),
    ("priority 1 tasks", 1),
    ("show p2 tasks", 2),
    ("priority 2 tasks", 2),
    ("show p3 tasks", 3),
    ("priority 3 tasks", 3),
    ("show p4 tasks", 4),
    ("priority 4 tasks", 4),
];

impl FastPathMatcher {
    /// Construct a matcher.
    pub const fn new() -> Self {
        Self
    }

    /// Match a query against the explicit local fast-path list.
    pub fn match_query(&self, query: &str) -> FastPathResult {
        let query = normalize_query(query);

        if COST_QUERIES.contains(&query.as_str()) {
            return FastPathResult::Handled(FastPathAction::ShowCosts);
        }

        if WORKER_QUERIES.contains(&query.as_str()) {
            return FastPathResult::Handled(FastPathAction::ShowWorkers);
        }

        if let Some((_, priority)) = PRIORITY_TASK_QUERIES
            .iter()
            .find(|(phrase, _)| *phrase == query)
        {
            return FastPathResult::Handled(FastPathAction::ShowTasks {
                priority: Some(*priority),
            });
        }

        if TASK_QUERIES.contains(&query.as_str()) {
            return FastPathResult::Handled(FastPathAction::ShowTasks { priority: None });
        }

        let view = match query.as_str() {
            "show overview" | "go to overview" | "overview" | "dashboard" => {
                Some(ViewTarget::Overview)
            }
            "go to workers" | "workers view" => Some(ViewTarget::Workers),
            "go to tasks" | "tasks view" => Some(ViewTarget::Tasks),
            "go to costs" | "costs view" => Some(ViewTarget::Costs),
            "show alerts" | "go to alerts" | "alerts view" => Some(ViewTarget::Alerts),
            "show logs" | "go to logs" | "logs view" | "activity log" => Some(ViewTarget::Logs),
            "show chat" | "go to chat" | "chat view" => Some(ViewTarget::Chat),
            _ => None,
        };

        view.map_or(FastPathResult::FallThrough, |target| {
            FastPathResult::Handled(FastPathAction::SwitchView(target))
        })
    }
}

/// Normalize only formatting that should not affect an exact phrase match.
fn normalize_query(query: &str) -> String {
    query
        .trim()
        .trim_end_matches(['?', '!', '.'])
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_common_queries_case_insensitively() {
        let matcher = FastPathMatcher::new();

        assert_eq!(
            matcher.match_query("  WHAT DID I SPEND TODAY? "),
            FastPathResult::Handled(FastPathAction::ShowCosts)
        );
        assert_eq!(
            matcher.match_query("Worker Status"),
            FastPathResult::Handled(FastPathAction::ShowWorkers)
        );
        assert_eq!(
            matcher.match_query("show P0 tasks"),
            FastPathResult::Handled(FastPathAction::ShowTasks { priority: Some(0) })
        );
    }

    #[test]
    fn matches_all_task_priorities() {
        let matcher = FastPathMatcher::new();

        for priority in 0..=4 {
            let query = format!("show p{priority} tasks");
            assert_eq!(
                matcher.match_query(&query),
                FastPathResult::Handled(FastPathAction::ShowTasks {
                    priority: Some(priority),
                })
            );
        }
    }

    #[test]
    fn matches_view_navigation() {
        let matcher = FastPathMatcher::new();

        assert_eq!(
            matcher.match_query("dashboard"),
            FastPathResult::Handled(FastPathAction::SwitchView(ViewTarget::Overview))
        );
        assert_eq!(
            matcher.match_query("go to logs!"),
            FastPathResult::Handled(FastPathAction::SwitchView(ViewTarget::Logs))
        );
    }

    #[test]
    fn unknown_or_non_exact_queries_fall_through() {
        let matcher = FastPathMatcher::new();

        for query in [
            "what did i spend yesterday",
            "show p5 tasks",
            "show all workers please",
            "explain quantum computing",
        ] {
            assert_eq!(matcher.match_query(query), FastPathResult::FallThrough);
        }
    }
}
