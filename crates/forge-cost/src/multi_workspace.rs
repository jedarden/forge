//! Cross-workspace cost aggregation.
//!
//! This module provides utilities for querying and aggregating costs across
//! multiple FORGE workspaces, each with its own cost database.

use crate::db::CostDatabase;
use crate::error::{CostError, Result};
use crate::models::DailyCost;
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Cost information for a single workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceCost {
    /// Workspace identifier
    pub workspace_id: String,
    /// Workspace name
    pub workspace_name: String,
    /// Path to workspace
    pub workspace_path: PathBuf,
    /// Whether the workspace has cost data
    pub has_cost_data: bool,
    /// Today's cost for this workspace
    pub today_cost: Option<DailyCost>,
    /// Total cost for this workspace
    pub total_cost: f64,
    /// Worker count for this workspace
    pub worker_count: usize,
}

/// Aggregated cost information across all workspaces.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MultiWorkspaceCostSummary {
    /// Cost information per workspace
    pub by_workspace: HashMap<String, WorkspaceCost>,
    /// Total cost across all workspaces
    pub total_cost: f64,
    /// Today's total cost across all workspaces
    pub today_total: f64,
    /// Total worker count across all workspaces
    pub total_workers: usize,
    /// Workspaces with cost data
    pub workspaces_with_costs: usize,
    /// Workspaces without cost data
    pub workspaces_without_costs: usize,
}

impl MultiWorkspaceCostSummary {
    /// Get the cost for a specific workspace.
    pub fn workspace_cost(&self, workspace_id: &str) -> Option<&WorkspaceCost> {
        self.by_workspace.get(workspace_id)
    }

    /// Get the most expensive workspace.
    pub fn most_expensive_workspace(&self) -> Option<&WorkspaceCost> {
        self.by_workspace
            .values()
            .filter(|ws| ws.has_cost_data)
            .max_by(|a, b| a.total_cost.partial_cmp(&b.total_cost).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Get the workspace with the most workers.
    pub fn most_active_workspace(&self) -> Option<&WorkspaceCost> {
        self.by_workspace
            .values()
            .max_by_key(|ws| ws.worker_count)
    }
}

/// Aggregator for costs across multiple workspaces.
pub struct MultiWorkspaceCostAggregator {
    /// Path to each workspace's cost database
    workspace_dbs: HashMap<String, PathBuf>,
}

impl MultiWorkspaceCostAggregator {
    /// Create a new aggregator from a map of workspace_id -> database_path.
    pub fn new(workspace_dbs: HashMap<String, PathBuf>) -> Self {
        Self { workspace_dbs }
    }

    /// Create an aggregator from workspace paths.
    ///
    /// Each workspace should have a `.forge/costs.db` file.
    pub fn from_workspace_paths(
        workspaces: HashMap<String, (String, PathBuf)>,
    ) -> Result<Self> {
        let mut workspace_dbs = HashMap::new();

        for (id, (_name, path)) in workspaces {
            let cost_db_path = path.join(".forge").join("costs.db");
            if cost_db_path.exists() {
                debug!("Found cost DB for workspace {}: {:?}", id, cost_db_path);
                workspace_dbs.insert(id, cost_db_path);
            } else {
                debug!("No cost DB found for workspace {} at {:?}", id, cost_db_path);
            }
        }

        Ok(Self { workspace_dbs })
    }

    /// Get the number of workspaces with cost databases.
    pub fn len(&self) -> usize {
        self.workspace_dbs.len()
    }

    /// Check if there are any workspaces.
    pub fn is_empty(&self) -> bool {
        self.workspace_dbs.is_empty()
    }

    /// Aggregate today's costs across all workspaces.
    pub fn aggregate_today_costs(&self) -> Result<MultiWorkspaceCostSummary> {
        let today = Utc::now().date_naive();
        self.aggregate_costs_for_date(today)
    }

    /// Aggregate costs for a specific date across all workspaces.
    pub fn aggregate_costs_for_date(&self, date: NaiveDate) -> Result<MultiWorkspaceCostSummary> {
        let mut summary = MultiWorkspaceCostSummary::default();

        for (workspace_id, db_path) in &self.workspace_dbs {
            let workspace_name = workspace_id.clone(); // In real usage, this would come from config

            match self.query_workspace_cost(db_path, date) {
                Ok(Some(daily_cost)) => {
                    let cost = daily_cost.total_cost_usd;
                    let ws_cost = WorkspaceCost {
                        workspace_id: workspace_id.clone(),
                        workspace_name: workspace_name.clone(),
                        workspace_path: db_path
                            .parent()
                            .and_then(|p| p.parent())
                            .unwrap_or(db_path)
                            .to_path_buf(),
                        has_cost_data: true,
                        today_cost: Some(daily_cost),
                        total_cost: cost,
                        worker_count: 0, // Would be populated separately
                    };

                    summary.total_cost += cost;
                    summary.today_total += cost;
                    summary.workspaces_with_costs += 1;
                    summary.by_workspace.insert(workspace_id.clone(), ws_cost);
                }
                Ok(None) => {
                    // Workspace has no cost data for this date
                    summary.workspaces_without_costs += 1;
                    summary.by_workspace.insert(
                        workspace_id.clone(),
                        WorkspaceCost {
                            workspace_id: workspace_id.clone(),
                            workspace_name,
                            workspace_path: db_path
                                .parent()
                                .and_then(|p| p.parent())
                                .unwrap_or(db_path)
                                .to_path_buf(),
                            has_cost_data: false,
                            today_cost: None,
                            total_cost: 0.0,
                            worker_count: 0,
                        },
                    );
                }
                Err(e) => {
                    warn!("Failed to query cost for workspace {}: {}", workspace_id, e);
                    summary.workspaces_without_costs += 1;
                }
            }
        }

        info!(
            "Aggregated costs for {} workspaces: ${:.2} total",
            summary.workspaces_with_costs,
            summary.total_cost
        );

        Ok(summary)
    }

    /// Aggregate costs across all workspaces for a date range.
    pub fn aggregate_costs_range(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<MultiWorkspaceCostSummary> {
        let mut summary = MultiWorkspaceCostSummary::default();

        for (workspace_id, db_path) in &self.workspace_dbs {
            let workspace_name = workspace_id.clone();

            match self.query_workspace_cost_range(db_path, start_date, end_date) {
                Ok(cost) => {
                    let ws_cost = WorkspaceCost {
                        workspace_id: workspace_id.clone(),
                        workspace_name,
                        workspace_path: db_path
                            .parent()
                            .and_then(|p| p.parent())
                            .unwrap_or(db_path)
                            .to_path_buf(),
                        has_cost_data: true,
                        today_cost: None,
                        total_cost: cost,
                        worker_count: 0,
                    };

                    summary.total_cost += cost;
                    summary.workspaces_with_costs += 1;
                    summary.by_workspace.insert(workspace_id.clone(), ws_cost);
                }
                Err(e) => {
                    warn!("Failed to query cost range for workspace {}: {}", workspace_id, e);
                    summary.workspaces_without_costs += 1;
                }
            }
        }

        Ok(summary)
    }

    /// Query cost for a specific workspace on a specific date.
    fn query_workspace_cost(
        &self,
        db_path: &Path,
        date: NaiveDate,
    ) -> Result<Option<DailyCost>> {
        let db = CostDatabase::open(db_path)?;
        let query = crate::query::CostQuery::new(&db);

        match query.get_costs_for_date(date) {
            Ok(daily) => {
                if daily.call_count > 0 {
                    Ok(Some(daily))
                } else {
                    Ok(None)
                }
            }
            Err(e) => {
                // Treat query errors as no data rather than hard failures
                warn!("Cost query failed for {:?}: {}", db_path, e);
                Ok(None)
            }
        }
    }

    /// Query cost for a specific workspace over a date range.
    fn query_workspace_cost_range(
        &self,
        db_path: &Path,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<f64> {
        let db = CostDatabase::open(db_path)?;
        let conn = db.connection();
        let conn = conn.lock().map_err(|e| {
            CostError::Query(format!("failed to acquire lock: {}", e))
        })?;

        let start = start_date.format("%Y-%m-%d").to_string();
        let end = end_date.format("%Y-%m-%d").to_string();

        let total: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(cost_usd), 0)
                 FROM api_calls
                 WHERE DATE(timestamp) BETWEEN ?1 AND ?2",
                rusqlite::params![start, end],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        Ok(total)
    }

    /// Get cost breakdown by model across all workspaces.
    pub fn aggregate_model_costs(
        &self,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
    ) -> Result<HashMap<String, f64>> {
        let mut model_costs: HashMap<String, f64> = HashMap::new();

        for db_path in self.workspace_dbs.values() {
            let db = CostDatabase::open(db_path)?;
            let query = crate::query::CostQuery::new(&db);

            match query.get_model_costs(start_date, end_date) {
                Ok(costs) => {
                    for model_cost in costs {
                        *model_costs.entry(model_cost.model).or_insert(0.0) += model_cost.total_cost_usd;
                    }
                }
                Err(e) => {
                    warn!("Failed to get model costs for {:?}: {}", db_path, e);
                }
            }
        }

        Ok(model_costs)
    }

    /// Get cost breakdown by workspace for today.
    pub fn get_workspace_breakdown_today(&self) -> Result<Vec<(String, f64)>> {
        let mut breakdown = Vec::new();

        for (workspace_id, db_path) in &self.workspace_dbs {
            let db = CostDatabase::open(db_path)?;
            let query = crate::query::CostQuery::new(&db);

            match query.get_today_costs() {
                Ok(daily) => {
                    breakdown.push((workspace_id.clone(), daily.total_cost_usd));
                }
                Err(e) => {
                    warn!("Failed to get today's costs for {}: {}", workspace_id, e);
                }
            }
        }

        breakdown.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        Ok(breakdown)
    }
}

/// Helper function to create a workspace cost database path.
pub fn workspace_cost_db_path(workspace_path: &Path) -> PathBuf {
    workspace_path.join(".forge").join("costs.db")
}

/// Check if a workspace has cost data.
pub fn workspace_has_cost_data(workspace_path: &Path) -> bool {
    workspace_cost_db_path(workspace_path).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_workspace_cost_db_path() {
        let path = Path::new("/home/coding/FORGE");
        let db_path = workspace_cost_db_path(path);
        assert_eq!(db_path, PathBuf::from("/home/coding/FORGE/.forge/costs.db"));
    }

    #[test]
    fn test_workspace_has_cost_data() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_path = temp_dir.path();

        // No cost data initially
        assert!(!workspace_has_cost_data(workspace_path));

        // Create .forge directory
        let forge_dir = workspace_path.join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();

        // Still no cost data
        assert!(!workspace_has_cost_data(workspace_path));

        // Create cost DB file
        let db_path = workspace_cost_db_path(workspace_path);
        std::fs::write(&db_path, b"").unwrap();

        // Now has cost data
        assert!(workspace_has_cost_data(workspace_path));
    }

    #[test]
    fn test_multi_workspace_cost_aggregator_new() {
        let mut dbs = HashMap::new();
        dbs.insert("forge".to_string(), PathBuf::from("/tmp/forge/.forge/costs.db"));
        dbs.insert("other".to_string(), PathBuf::from("/tmp/other/.forge/costs.db"));

        let aggregator = MultiWorkspaceCostAggregator::new(dbs);
        assert_eq!(aggregator.len(), 2);
        assert!(!aggregator.is_empty());
    }

    #[test]
    fn test_multi_workspace_cost_aggregator_empty() {
        let aggregator = MultiWorkspaceCostAggregator::new(HashMap::new());
        assert_eq!(aggregator.len(), 0);
        assert!(aggregator.is_empty());
    }

    #[test]
    fn test_multi_workspace_cost_summary_default() {
        let summary = MultiWorkspaceCostSummary::default();
        assert_eq!(summary.total_cost, 0.0);
        assert_eq!(summary.today_total, 0.0);
        assert_eq!(summary.total_workers, 0);
        assert_eq!(summary.workspaces_with_costs, 0);
        assert!(summary.by_workspace.is_empty());
        assert!(summary.most_expensive_workspace().is_none());
        assert!(summary.most_active_workspace().is_none());
    }

    #[test]
    fn test_workspace_cost_structure() {
        let ws_cost = WorkspaceCost {
            workspace_id: "test".to_string(),
            workspace_name: "Test Workspace".to_string(),
            workspace_path: PathBuf::from("/tmp/test"),
            has_cost_data: true,
            today_cost: None,
            total_cost: 10.50,
            worker_count: 3,
        };

        assert_eq!(ws_cost.workspace_id, "test");
        assert_eq!(ws_cost.total_cost, 10.50);
        assert_eq!(ws_cost.worker_count, 3);
        assert!(ws_cost.has_cost_data);
    }

    #[test]
    fn test_multi_workspace_cost_summary_most_expensive() {
        let mut summary = MultiWorkspaceCostSummary::default();
        summary.total_cost = 30.0;

        let mut by_workspace = HashMap::new();

        by_workspace.insert(
            "cheap".to_string(),
            WorkspaceCost {
                workspace_id: "cheap".to_string(),
                workspace_name: "Cheap".to_string(),
                workspace_path: PathBuf::from("/tmp/cheap"),
                has_cost_data: true,
                today_cost: None,
                total_cost: 5.0,
                worker_count: 1,
            },
        );

        by_workspace.insert(
            "expensive".to_string(),
            WorkspaceCost {
                workspace_id: "expensive".to_string(),
                workspace_name: "Expensive".to_string(),
                workspace_path: PathBuf::from("/tmp/expensive"),
                has_cost_data: true,
                today_cost: None,
                total_cost: 25.0,
                worker_count: 5,
            },
        );

        summary.by_workspace = by_workspace;

        let most_expensive = summary.most_expensive_workspace();
        assert!(most_expensive.is_some());
        assert_eq!(most_expensive.unwrap().workspace_id, "expensive");

        let most_active = summary.most_active_workspace();
        assert!(most_active.is_some());
        assert_eq!(most_active.unwrap().workspace_id, "expensive");
    }
}
