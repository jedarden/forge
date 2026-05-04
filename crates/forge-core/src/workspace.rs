//! Multi-workspace coordination for FORGE.
//!
//! This module provides the workspace registry and management for coordinating
//! multiple FORGE workspaces from a single dashboard.

use crate::{ForgeError, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use std::fmt;
use tracing::{debug, info, warn};

/// Configuration for a monitored workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// Unique identifier for this workspace
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Absolute path to the workspace root
    pub path: PathBuf,
    /// Whether this workspace is actively monitored
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Priority for display ordering (lower = higher priority)
    #[serde(default = "default_priority")]
    pub priority: u32,
    /// Optional description
    #[serde(default)]
    pub description: Option<String>,
}

fn default_enabled() -> bool { true }
fn default_priority() -> u32 { 100 }

impl WorkspaceConfig {
    /// Create a new workspace configuration.
    pub fn new(id: impl Into<String>, name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            path: path.into(),
            enabled: true,
            priority: 100,
            description: None,
        }
    }

    /// Check if the workspace path exists and is accessible.
    pub fn is_accessible(&self) -> bool {
        self.path.exists() && self.path.is_dir()
    }

    /// Get the path to the status directory for this workspace.
    pub fn status_dir(&self) -> PathBuf {
        self.path.join(".forge").join("status")
    }

    /// Get the path to the logs directory for this workspace.
    pub fn logs_dir(&self) -> PathBuf {
        self.path.join(".forge").join("logs")
    }

    /// Get the path to the cost database for this workspace.
    pub fn cost_db_path(&self) -> PathBuf {
        self.path.join(".forge").join("costs.db")
    }

    /// Get the path to the beads directory for this workspace.
    pub fn beads_dir(&self) -> PathBuf {
        self.path.join(".beads")
    }

    /// Check if this workspace has a cost database.
    pub fn has_cost_data(&self) -> bool {
        self.cost_db_path().exists()
    }

    /// Check if this workspace has beads.
    pub fn has_beads(&self) -> bool {
        self.beads_dir().exists()
    }
}

/// Registry of monitored workspaces.
#[derive(Debug, Clone)]
pub struct WorkspaceRegistry {
    /// All configured workspaces
    workspaces: Vec<WorkspaceConfig>,
    /// Index by workspace ID for fast lookup
    index: HashMap<String, usize>,
}

impl Default for WorkspaceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Custom serialization: only serialize workspaces, skip index (it's rebuilt)
impl Serialize for WorkspaceRegistry {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.workspaces.serialize(serializer)
    }
}

// Custom deserialization: deserialize workspaces and rebuild index
impl<'de> Deserialize<'de> for WorkspaceRegistry {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct WorkspaceRegistryVisitor;

        impl<'de> serde::de::Visitor<'de> for WorkspaceRegistryVisitor {
            type Value = WorkspaceRegistry;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a sequence of workspace configurations")
            }

            fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut registry = WorkspaceRegistry::new();

                while let Some(workspace) = seq.next_element::<WorkspaceConfig>()? {
                    let id = workspace.id.clone();
                    registry.workspaces.push(workspace);
                    registry.index.insert(id, registry.workspaces.len() - 1);
                }

                Ok(registry)
            }
        }

        deserializer.deserialize_seq(WorkspaceRegistryVisitor)
    }
}

impl WorkspaceRegistry {
    /// Create an empty workspace registry.
    pub fn new() -> Self {
        Self {
            workspaces: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Add a workspace to the registry.
    pub fn add(&mut self, workspace: WorkspaceConfig) {
        let id = workspace.id.clone();
        self.workspaces.push(workspace);
        self.index.insert(id, self.workspaces.len() - 1);
    }

    /// Remove a workspace by ID.
    pub fn remove(&mut self, id: &str) -> Option<WorkspaceConfig> {
        if let Some(&idx) = self.index.get(id) {
            self.index.remove(id);
            // Remove from vector first (this shifts subsequent elements)
            let workspace = self.workspaces.remove(idx);
            // Then rebuild index with correct indices
            self.index.clear();
            for (i, ws) in self.workspaces.iter().enumerate() {
                self.index.insert(ws.id.clone(), i);
            }
            Some(workspace)
        } else {
            None
        }
    }

    /// Get a workspace by ID.
    pub fn get(&self, id: &str) -> Option<&WorkspaceConfig> {
        self.index.get(id).and_then(|&idx| self.workspaces.get(idx))
    }

    /// Get a mutable reference to a workspace by ID.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut WorkspaceConfig> {
        if let Some(&idx) = self.index.get(id) {
            self.workspaces.get_mut(idx)
        } else {
            None
        }
    }

    /// Get all workspaces sorted by priority.
    pub fn all(&self) -> Vec<&WorkspaceConfig> {
        let mut workspaces: Vec<_> = self.workspaces.iter().collect();
        workspaces.sort_by_key(|ws| ws.priority);
        workspaces
    }

    /// Get only enabled workspaces.
    pub fn enabled(&self) -> Vec<&WorkspaceConfig> {
        self.all().into_iter().filter(|ws| ws.enabled).collect()
    }

    /// Get the current/active workspace (first enabled by priority).
    pub fn current(&self) -> Option<&WorkspaceConfig> {
        self.enabled().first().copied()
    }

    /// Set the current workspace by moving it to highest priority.
    pub fn set_current(&mut self, id: &str) -> Result<()> {
        if !self.index.contains_key(id) {
            return Err(ForgeError::WorkerNotFound {
                worker_id: format!("workspace {}", id),
            });
        }

        // Reorder by setting the current workspace to priority 0
        // and shifting others down
        if let Some(ws) = self.get_mut(id) {
            let old_priority = ws.priority;
            ws.priority = 0;

            // Shift all other workspaces down
            for other_ws in &mut self.workspaces {
                if other_ws.id != id && other_ws.priority < old_priority {
                    other_ws.priority += 1;
                }
            }
        }

        Ok(())
    }

    /// Get workspace count.
    pub fn len(&self) -> usize {
        self.workspaces.len()
    }

    /// Check if registry is empty.
    pub fn is_empty(&self) -> bool {
        self.workspaces.is_empty()
    }

    /// Discover workspaces from a config file or directory.
    pub fn discover_from_path(path: &Path) -> Result<Self> {
        let mut registry = Self::new();

        // If path is a workspace directory, add it
        if path.exists() && path.is_dir() {
            let forge_dir = path.join(".forge");
            if forge_dir.exists() {
                let id = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("default")
                    .to_string();

                let ws = WorkspaceConfig::new(
                    id.clone(),
                    id.clone(),
                    path
                );
                registry.add(ws);
                info!("Discovered workspace: {} at {:?}", id, path);
            }
        }

        Ok(registry)
    }

    /// Load workspace registry from a YAML configuration file.
    pub fn load_from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            warn!("Workspace config file not found: {:?}", path);
            return Ok(Self::new());
        }

        let content = fs::read_to_string(path).map_err(|e| ForgeError::Io {
            operation: "read".to_string(),
            path: path.to_path_buf(),
            source: e,
        })?;

        let mut registry: Self = serde_yaml::from_str(&content).map_err(|e| {
            ForgeError::YamlParse {
                context: format!("workspace config at {:?}", path),
                message: e.to_string(),
            }
        })?;

        // Rebuild index
        registry.rebuild_index();

        info!("Loaded {} workspaces from {:?}", registry.len(), path);
        Ok(registry)
    }

    /// Save workspace registry to a YAML configuration file.
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| ForgeError::Io {
                operation: "create directory".to_string(),
                path: parent.to_path_buf(),
                source: e,
            })?;
        }

        let yaml = serde_yaml::to_string(self).map_err(|e| ForgeError::YamlParse {
            context: "workspace registry serialization".to_string(),
            message: e.to_string(),
        })?;

        fs::write(path, yaml).map_err(|e| ForgeError::Io {
            operation: "write".to_string(),
            path: path.to_path_buf(),
            source: e,
        })?;

        debug!("Saved {} workspaces to {:?}", self.len(), path);
        Ok(())
    }

    /// Rebuild the index after deserialization or manual modification.
    fn rebuild_index(&mut self) {
        self.index.clear();
        for (i, ws) in self.workspaces.iter().enumerate() {
            self.index.insert(ws.id.clone(), i);
        }
    }

    /// Validate all workspaces and report issues.
    pub fn validate(&self) -> Vec<WorkspaceIssue> {
        let mut issues = Vec::new();

        for ws in &self.workspaces {
            if !ws.enabled {
                continue;
            }

            if !ws.is_accessible() {
                issues.push(WorkspaceIssue {
                    workspace_id: ws.id.clone(),
                    severity: IssueSeverity::Error,
                    message: format!("Workspace path does not exist: {:?}", ws.path),
                });
                continue;
            }

            // Check for required directories
            if !ws.status_dir().exists() {
                issues.push(WorkspaceIssue {
                    workspace_id: ws.id.clone(),
                    severity: IssueSeverity::Warning,
                    message: format!("Status directory not found: {:?}", ws.status_dir()),
                });
            }

            if !ws.logs_dir().exists() {
                issues.push(WorkspaceIssue {
                    workspace_id: ws.id.clone(),
                    severity: IssueSeverity::Warning,
                    message: format!("Logs directory not found: {:?}", ws.logs_dir()),
                });
            }
        }

        issues
    }

    /// Get aggregated statistics across all workspaces.
    pub fn aggregate_stats(&self) -> WorkspaceAggregateStats {
        let mut stats = WorkspaceAggregateStats::default();

        for ws in &self.workspaces {
            if !ws.enabled || !ws.is_accessible() {
                continue;
            }

            stats.total_workspaces += 1;

            if ws.has_cost_data() {
                stats.workspaces_with_costs += 1;
            }

            if ws.has_beads() {
                stats.workspaces_with_beads += 1;
            }
        }

        stats
    }
}

/// Issues found during workspace validation.
#[derive(Debug, Clone)]
pub struct WorkspaceIssue {
    pub workspace_id: String,
    pub severity: IssueSeverity,
    pub message: String,
}

/// Severity of workspace issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    Warning,
    Error,
}

/// Aggregated statistics across workspaces.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceAggregateStats {
    pub total_workspaces: usize,
    pub workspaces_with_costs: usize,
    pub workspaces_with_beads: usize,
}

// ============================================================
// Cross-Workspace Bead Visibility
// ============================================================

/// Bead information from a workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceBead {
    /// Bead ID
    pub id: String,
    /// Workspace this bead belongs to
    pub workspace_id: String,
    /// Workspace name
    pub workspace_name: String,
    /// Bead title (extracted from JSONL)
    pub title: Option<String>,
    /// Bead status (open/closed)
    pub status: String,
    /// Assignee
    pub assignee: Option<String>,
    /// Priority (if available)
    pub priority: Option<String>,
}

/// Result of cross-workspace bead query.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CrossWorkspaceBeadResult {
    /// All beads across workspaces
    pub beads: Vec<WorkspaceBead>,
    /// Beads grouped by workspace
    pub by_workspace: HashMap<String, Vec<WorkspaceBead>>,
    /// Total open beads
    pub total_open: usize,
    /// Total closed beads
    pub total_closed: usize,
    /// Unassigned beads
    pub unassigned: usize,
}

impl CrossWorkspaceBeadResult {
    /// Get beads for a specific workspace.
    pub fn beads_for_workspace(&self, workspace_id: &str) -> Vec<&WorkspaceBead> {
        self.by_workspace
            .get(workspace_id)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Get open beads across all workspaces.
    pub fn open_beads(&self) -> Vec<&WorkspaceBead> {
        self.beads
            .iter()
            .filter(|b| b.status == "open" || b.status == "in-progress")
            .collect()
    }

    /// Get unassigned beads.
    pub fn unassigned_beads(&self) -> Vec<&WorkspaceBead> {
        self.beads
            .iter()
            .filter(|b| b.assignee.is_none() || b.assignee.as_deref() == Some("none"))
            .collect()
    }
}

/// Query beads across multiple workspaces.
///
/// This function reads beads JSONL files from each workspace's .beads directory
/// and aggregates them into a unified view.
///
/// # Example
///
/// ```no_run
/// use forge_core::{WorkspaceRegistry, query_beads_cross_workspace};
///
/// fn main() -> forge_core::Result<()> {
///     let registry = WorkspaceRegistry::new();
///     let result = query_beads_cross_workspace(&registry)?;
///
///     println!("Found {} beads across {} workspaces",
///         result.beads.len(),
///         result.by_workspace.len()
///     );
///
///     Ok(())
/// }
/// ```
pub fn query_beads_cross_workspace(
    registry: &WorkspaceRegistry,
) -> Result<CrossWorkspaceBeadResult> {
    let mut result = CrossWorkspaceBeadResult::default();

    for ws in registry.all() {
        let beads_dir = ws.beads_dir();

        if !beads_dir.exists() {
            continue;
        }

        let issues_path = beads_dir.join("issues.jsonl");

        if !issues_path.exists() {
            continue;
        }

        // Read and parse beads JSONL
        match read_beads_from_jsonl(&issues_path, &ws.id, &ws.name) {
            Ok(mut beads) => {
                result.total_open += beads.iter().filter(|b| b.status == "open" || b.status == "in-progress").count();
                result.total_closed += beads.iter().filter(|b| b.status == "closed").count();
                result.unassigned += beads.iter().filter(|b| b.assignee.is_none() || b.assignee.as_deref() == Some("none")).count();

                result.by_workspace.insert(ws.id.clone(), beads.clone());
                result.beads.append(&mut beads);
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to read beads from workspace {}: {}",
                    ws.id,
                    e
                );
            }
        }
    }

    tracing::debug!(
        "Queried {} beads across {} workspaces ({} open, {} closed)",
        result.beads.len(),
        result.by_workspace.len(),
        result.total_open,
        result.total_closed
    );

    Ok(result)
}

/// Read beads from a JSONL file.
fn read_beads_from_jsonl(
    path: &Path,
    workspace_id: &str,
    workspace_name: &str,
) -> Result<Vec<WorkspaceBead>> {
    let content = fs::read_to_string(path).map_err(|e| ForgeError::Io {
        operation: "read".to_string(),
        path: path.to_path_buf(),
        source: e,
    })?;

    let mut beads = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        // Parse JSONL line
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            let id = value.get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let title = value.get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let status = value.get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("open")
                .to_string();

            let assignee = value.get("assignee")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty() && *s != "none")
                .map(|s| s.to_string());

            let priority = value.get("priority")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            beads.push(WorkspaceBead {
                id,
                workspace_id: workspace_id.to_string(),
                workspace_name: workspace_name.to_string(),
                title,
                status,
                assignee,
                priority,
            });
        }
    }

    Ok(beads)
}

/// Get bead count for a specific workspace.
pub fn get_workspace_bead_count(workspace_path: &Path) -> Result<usize> {
    let beads_dir = workspace_path.join(".beads");
    let issues_path = beads_dir.join("issues.jsonl");

    if !issues_path.exists() {
        return Ok(0);
    }

    let content = fs::read_to_string(&issues_path).map_err(|e| ForgeError::Io {
        operation: "read".to_string(),
        path: issues_path.clone(),
        source: e,
    })?;

    let count = content.lines().filter(|l| !l.trim().is_empty()).count();

    Ok(count)
}

/// Get bead counts for all workspaces in a registry.
pub fn get_workspace_bead_counts(
    registry: &WorkspaceRegistry,
) -> Result<HashMap<String, usize>> {
    let mut counts = HashMap::new();

    for ws in registry.all() {
        if !ws.is_accessible() {
            continue;
        }

        match get_workspace_bead_count(&ws.path) {
            Ok(count) => {
                counts.insert(ws.id.clone(), count);
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to get bead count for workspace {}: {}",
                    ws.id,
                    e
                );
            }
        }
    }

    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_workspace_config_new() {
        let ws = WorkspaceConfig::new("test", "Test Workspace", "/tmp/test");
        assert_eq!(ws.id, "test");
        assert_eq!(ws.name, "Test Workspace");
        assert_eq!(ws.path, PathBuf::from("/tmp/test"));
        assert!(ws.enabled);
        assert_eq!(ws.priority, 100);
    }

    #[test]
    fn test_workspace_registry_add_remove() {
        let mut registry = WorkspaceRegistry::new();

        let ws1 = WorkspaceConfig::new("ws1", "Workspace 1", "/tmp/ws1");
        let ws2 = WorkspaceConfig::new("ws2", "Workspace 2", "/tmp/ws2");

        registry.add(ws1.clone());
        registry.add(ws2.clone());

        assert_eq!(registry.len(), 2);
        assert!(registry.get("ws1").is_some());
        assert!(registry.get("ws2").is_some());

        let removed = registry.remove("ws1");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().id, "ws1");
        assert_eq!(registry.len(), 1);
        assert!(registry.get("ws1").is_none());
    }

    #[test]
    fn test_workspace_registry_priority_sorting() {
        let mut registry = WorkspaceRegistry::new();

        let mut ws1 = WorkspaceConfig::new("ws1", "Workspace 1", "/tmp/ws1");
        ws1.priority = 10;

        let mut ws2 = WorkspaceConfig::new("ws2", "Workspace 2", "/tmp/ws2");
        ws2.priority = 5;

        let mut ws3 = WorkspaceConfig::new("ws3", "Workspace 3", "/tmp/ws3");
        ws3.priority = 15;

        registry.add(ws1);
        registry.add(ws2);
        registry.add(ws3);

        let all = registry.all();
        assert_eq!(all[0].id, "ws2"); // Priority 5
        assert_eq!(all[1].id, "ws1"); // Priority 10
        assert_eq!(all[2].id, "ws3"); // Priority 15
    }

    #[test]
    fn test_workspace_registry_set_current() {
        let mut registry = WorkspaceRegistry::new();

        let mut ws1 = WorkspaceConfig::new("ws1", "Workspace 1", "/tmp/ws1");
        ws1.priority = 0;

        let mut ws2 = WorkspaceConfig::new("ws2", "Workspace 2", "/tmp/ws2");
        ws2.priority = 1;

        registry.add(ws1);
        registry.add(ws2);

        // Initially ws1 is current (priority 0)
        assert_eq!(registry.current().map(|w| w.id.as_str()), Some("ws1"));

        // Set ws2 as current
        registry.set_current("ws2").unwrap();
        assert_eq!(registry.current().map(|w| w.id.as_str()), Some("ws2"));
    }

    #[test]
    fn test_workspace_registry_enabled_filter() {
        let mut registry = WorkspaceRegistry::new();

        let mut ws1 = WorkspaceConfig::new("ws1", "Workspace 1", "/tmp/ws1");
        ws1.enabled = true;

        let mut ws2 = WorkspaceConfig::new("ws2", "Workspace 2", "/tmp/ws2");
        ws2.enabled = false;

        registry.add(ws1);
        registry.add(ws2);

        assert_eq!(registry.enabled().len(), 1);
        assert_eq!(registry.enabled()[0].id, "ws1");
    }

    #[test]
    fn test_workspace_registry_serialize() {
        let mut registry = WorkspaceRegistry::new();

        let ws = WorkspaceConfig::new("test", "Test", "/tmp/test");
        registry.add(ws);

        let yaml = serde_yaml::to_string(&registry).unwrap();
        let loaded: WorkspaceRegistry = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(loaded.len(), 1);
        assert!(loaded.get("test").is_some());
    }

    #[test]
    fn test_workspace_discovery() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_path = temp_dir.path();

        // Create .forge directory to make it a valid workspace
        let forge_dir = workspace_path.join(".forge");
        fs::create_dir_all(&forge_dir).unwrap();

        let registry = WorkspaceRegistry::discover_from_path(workspace_path).unwrap();
        assert_eq!(registry.len(), 1);
        assert!(registry.get(
            workspace_path.file_name().and_then(|n| n.to_str()).unwrap()
        ).is_some());
    }

    #[test]
    fn test_workspace_aggregate_stats() {
        let temp_dir = TempDir::new().unwrap();
        let ws_path = temp_dir.path();

        let mut registry = WorkspaceRegistry::new();

        // Create a workspace with cost data
        let ws = WorkspaceConfig::new("test", "Test", ws_path);
        registry.add(ws);

        let stats = registry.aggregate_stats();
        // Workspace exists but may not be accessible in test
        assert_eq!(stats.total_workspaces, 1);
    }
}
