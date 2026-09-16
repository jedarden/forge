//! Reading bead stores in both supported on-disk formats.
//!
//! Two formats exist in the wild:
//!
//! - **bead-rs** (current): `.beads/config.json` plus a `.beads/checkpoint/`
//!   directory. `checkpoint/current.json` names the active snapshot root under
//!   `checkpoint/objects/`, and `checkpoint/forensic.jsonl` carries a full
//!   copy of the same records. Every issue record is wrapped as
//!   `{"record_type": "issue", "issue": {...}}` and names its status
//!   `base_status`.
//! - **legacy bead-forge** (deprecated): a flat `.beads/issues.jsonl` with one
//!   bare issue object per line and a plain `status` field.
//!
//! All FORGE reads go through this module. FORGE never opens the SQLite live
//! database and never shells out to read bead state; the checkpoint is the
//! committed, durable copy of the store, so reads also work on a fresh clone
//! where `beads.db` has not been restored yet. Mutations belong to the `bead`
//! CLI, which stays the sole write authority over the store (ADR 0007, as
//! clarified by ADR 0020).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{ForgeError, Result};

/// Which on-disk bead store format a workspace uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeadStoreFormat {
    /// bead-rs: `.beads/config.json` + `.beads/checkpoint/`.
    BeadRs,
    /// Legacy bead-forge: flat `.beads/issues.jsonl`.
    LegacyFlatJsonl,
}

/// Detect which bead store format a workspace uses.
///
/// Returns `None` when the workspace has no bead store at all.
pub fn detect_format(workspace: &Path) -> Option<BeadStoreFormat> {
    let beads = workspace.join(".beads");
    let checkpoint = beads.join("checkpoint");

    // A checkout may contain the portable checkpoint without the live
    // `beads.db` (or, in a minimal fixture, without config.json). The
    // checkpoint is sufficient to identify bead-rs and is the only input the
    // reader needs, so do not require the SQLite database or config file.
    if beads.join("config.json").exists()
        || checkpoint.join("current.json").exists()
        || checkpoint.join("forensic.jsonl").exists()
        || checkpoint.join("objects").is_dir()
    {
        Some(BeadStoreFormat::BeadRs)
    } else if beads.join("issues.jsonl").exists() {
        Some(BeadStoreFormat::LegacyFlatJsonl)
    } else {
        None
    }
}

/// A blocking edge from another bead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreDependency {
    /// ID of the bead that must complete first.
    pub blocker: String,
    /// Relationship kind; only `blocks` edges gate readiness.
    #[serde(default = "default_blocks")]
    pub kind: String,
}

fn default_blocks() -> String {
    "blocks".to_string()
}

/// A normalized bead issue, identical across both store formats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreBead {
    /// Unique bead identifier.
    pub id: String,
    /// Title of the bead.
    pub title: String,
    /// Detailed description.
    #[serde(default)]
    pub description: String,
    /// Base status: open, in_progress, closed, deferred.
    pub status: String,
    /// Priority (0-4, where 0 is critical).
    pub priority: u8,
    /// Issue type (task, bug, feature, genesis, ...).
    #[serde(default)]
    pub issue_type: String,
    /// Labels.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Assignee, if any.
    #[serde(default)]
    pub assignee: Option<String>,
    /// Claim epoch used by bead-rs as the fencing token for mutations.
    #[serde(default)]
    pub claim_epoch: Option<u64>,
    /// True when the bead was explicitly blocked via `bead update --status blocked`.
    #[serde(default)]
    pub manual_blocked: bool,
    /// Declared dependencies.
    #[serde(default)]
    pub dependencies: Vec<StoreDependency>,
    /// Creation timestamp (ISO 8601).
    #[serde(default)]
    pub created_at: Option<String>,
    /// Last update timestamp (ISO 8601).
    #[serde(default)]
    pub updated_at: Option<String>,
}

impl StoreBead {
    /// IDs of dependencies whose kind gates readiness (`blocks`).
    pub fn blocking_dependency_ids(&self) -> Vec<&str> {
        self.dependencies
            .iter()
            .filter(|d| d.kind.eq_ignore_ascii_case("blocks"))
            .map(|d| d.blocker.as_str())
            .collect()
    }

    /// True when the bead is an unfinished `blocks` edge away from another
    /// bead in the index (a blocker that is not `closed`).
    pub fn has_unfinished_blocker(&self, index: &HashMap<&str, &StoreBead>) -> bool {
        self.blocking_dependency_ids()
            .into_iter()
            .any(|blocker| index.get(blocker).is_some_and(|dep| dep.status != "closed"))
    }
}

/// Build an id -> bead index over a bead set.
pub fn build_index(beads: &[StoreBead]) -> HashMap<&str, &StoreBead> {
    beads.iter().map(|b| (b.id.as_str(), b)).collect()
}

/// Whether a bead sits on the ready frontier: open, unassigned, not manually
/// blocked, and with no unfinished `blocks` edge. This mirrors the documented
/// semantics of `bead list --ready`.
pub fn is_ready(bead: &StoreBead, index: &HashMap<&str, &StoreBead>) -> bool {
    bead.status == "open"
        && bead.assignee.is_none()
        && !bead.manual_blocked
        && !bead.has_unfinished_blocker(index)
}

/// Number of beads that declare a `blocks` edge against `bead_id`.
pub fn count_dependents(beads: &[StoreBead], bead_id: &str) -> usize {
    beads
        .iter()
        .filter(|b| {
            b.dependencies
                .iter()
                .any(|d| d.kind.eq_ignore_ascii_case("blocks") && d.blocker == bead_id)
        })
        .count()
}

/// Read every issue in a workspace's bead store, whichever format it uses.
///
/// Returns an empty set when the workspace has no bead store; errors only
/// when a store exists but cannot be read.
pub fn read_all_beads(workspace: &Path) -> Result<Vec<StoreBead>> {
    match detect_format(workspace) {
        Some(BeadStoreFormat::BeadRs) => read_bead_rs(workspace),
        Some(BeadStoreFormat::LegacyFlatJsonl) => {
            read_legacy_flat(&workspace.join(".beads/issues.jsonl"))
        }
        None => Ok(Vec::new()),
    }
}

/// Read the fencing token for a bead from the durable checkpoint.
pub fn claim_epoch(workspace: &Path, bead_id: &str) -> Result<Option<u64>> {
    Ok(read_all_beads(workspace)?
        .into_iter()
        .find(|bead| bead.id == bead_id)
        .and_then(|bead| bead.claim_epoch))
}

/// Read a bead-rs store from its checkpoint.
///
/// Prefers the snapshot named by `current.json`'s `active_root`; falls back to
/// `forensic.jsonl` (a full copy of the same records) when the active root is
/// missing or unreadable.
fn read_bead_rs(workspace: &Path) -> Result<Vec<StoreBead>> {
    let checkpoint = workspace.join(".beads/checkpoint");
    let active_root = fs::read_to_string(checkpoint.join("current.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|current| {
            current["active_root"]["path"]
                .as_str()
                .map(|rel| checkpoint.join(rel))
        });

    if let Some(path) = active_root {
        match read_snapshot_file(&path) {
            Ok(beads) if !beads.is_empty() => return Ok(beads),
            Ok(_) => warn!(
                path = %path.display(),
                "bead-rs active root parsed to no issues; falling back to forensic.jsonl"
            ),
            Err(e) => warn!(
                path = %path.display(),
                error = %e,
                "failed to read bead-rs active root; falling back to forensic.jsonl"
            ),
        }
    }

    read_snapshot_file(&checkpoint.join("forensic.jsonl"))
}

/// Parse a bead-rs snapshot file (`record_type`-wrapped JSONL records).
fn read_snapshot_file(path: &Path) -> Result<Vec<StoreBead>> {
    let content = fs::read_to_string(path)
        .map_err(|e| ForgeError::io("reading bead-rs checkpoint snapshot", path, e))?;

    let mut beads = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        // Parse JSONL entry
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(value) if value["record_type"] == "issue" => {
                if let Some(bead) = parse_issue(&value["issue"]) {
                    beads.push(bead);
                }
            }
            Ok(_) => {} // event and other record types carry no issue state
            Err(e) => {
                warn!(path = %path.display(), error = %e, "skipping malformed checkpoint record")
            }
        }
    }

    Ok(beads)
}

/// Parse a legacy bead-forge flat `issues.jsonl` file.
fn read_legacy_flat(path: &Path) -> Result<Vec<StoreBead>> {
    let content =
        fs::read_to_string(path).map_err(|e| ForgeError::io("reading beads file", path, e))?;

    let mut beads = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(value) => {
                if let Some(bead) = parse_issue(&value) {
                    beads.push(bead);
                }
            }
            Err(e) => warn!(path = %path.display(), error = %e, "skipping malformed bead entry"),
        }
    }

    Ok(beads)
}

/// Parse one issue object (bare for legacy stores, `issue`-wrapped for
/// bead-rs snapshots) into a [`StoreBead`].
///
/// Returns `None` for entries without an `id`, which cannot be tracked.
fn parse_issue(value: &serde_json::Value) -> Option<StoreBead> {
    let id = value.get("id")?.as_str()?.to_string();
    if id.is_empty() {
        return None;
    }

    let title = value
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let description = value
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    // bead-rs snapshots name the field `base_status`; the CLI and legacy
    // stores use `status`.
    let status = value
        .get("base_status")
        .or_else(|| value.get("status"))
        .and_then(|v| v.as_str())
        .unwrap_or("open")
        .to_string();

    let priority = parse_priority(value.get("priority"));

    let issue_type = value
        .get("issue_type")
        .or_else(|| value.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("task")
        .to_string();

    let labels = value
        .get("labels")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let assignee = value
        .get("assignee")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty() && *s != "none")
        .map(String::from);

    let claim_epoch = value.get("claim_epoch").and_then(|v| v.as_u64());

    let manual_blocked = value
        .get("manual_blocked")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Dependencies: legacy stores list blocker IDs as bare strings, bead-rs
    // uses `{"blocker": ..., "kind": ...}` objects.
    let dependencies = value
        .get("dependencies")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|d| match d {
                    serde_json::Value::String(blocker) => Some(StoreDependency {
                        blocker: blocker.clone(),
                        kind: default_blocks(),
                    }),
                    serde_json::Value::Object(_) => {
                        serde_json::from_value::<StoreDependency>(d.clone()).ok()
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    let created_at = value
        .get("created_at")
        .and_then(|v| v.as_str())
        .map(String::from);
    let updated_at = value
        .get("updated_at")
        .and_then(|v| v.as_str())
        .map(String::from);

    Some(StoreBead {
        id,
        title,
        description,
        status,
        priority,
        issue_type,
        labels,
        assignee,
        claim_epoch,
        manual_blocked,
        dependencies,
        created_at,
        updated_at,
    })
}

/// Parse a priority from either a number or a string (`2`, `"2"`, `"P2"`).
fn parse_priority(value: Option<&serde_json::Value>) -> u8 {
    match value {
        Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(2) as u8,
        Some(serde_json::Value::String(s)) => s
            .chars()
            .find(|c| c.is_ascii_digit())
            .and_then(|c| c.to_digit(10))
            .map(|d| d as u8)
            .unwrap_or(2),
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    /// Create a legacy bead-forge workspace with a flat issues.jsonl.
    fn create_legacy_workspace() -> TempDir {
        let dir = TempDir::new().unwrap();
        let beads_dir = dir.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();

        let mut file = fs::File::create(beads_dir.join("issues.jsonl")).unwrap();
        writeln!(file, r#"{{"id":"test-1","title":"Ready bead","description":"A test","status":"open","priority":0,"issue_type":"task","labels":[],"dependencies":[]}}"#).unwrap();
        writeln!(file, r#"{{"id":"test-2","title":"Blocked bead","description":"Blocked","status":"open","priority":1,"issue_type":"task","labels":[],"dependencies":["test-1"]}}"#).unwrap();
        writeln!(file, r#"{{"id":"test-3","title":"Assigned bead","status":"open","priority":1,"issue_type":"task","assignee":"worker-1","dependencies":[]}}"#).unwrap();

        dir
    }

    /// Create a bead-rs workspace with a checkpoint (config.json +
    /// checkpoint/current.json -> objects/<sha>.jsonl).
    fn create_bead_rs_workspace() -> TempDir {
        let dir = TempDir::new().unwrap();
        let beads_dir = dir.path().join(".beads");
        let checkpoint = beads_dir.join("checkpoint");
        let objects = checkpoint.join("objects");
        fs::create_dir_all(&objects).unwrap();

        fs::write(
            beads_dir.join("config.json"),
            r#"{"prefix":"forge","uuid":"6d33e860"}"#,
        )
        .unwrap();

        let root_sha = "073fc7bbf1d714799316ad50cd08bf1be4418b04dc8934f183cf2ab88908614b";
        let mut root = fs::File::create(objects.join(format!("{root_sha}.jsonl"))).unwrap();
        writeln!(
            root,
            r#"{{"record_type":"issue","issue":{{"id":"forge-aaa","title":"Ready","description":"","base_status":"open","priority":0,"issue_type":"task","labels":["urgent"],"assignee":null,"manual_blocked":false,"dependencies":[],"created_at":"2026-09-01T00:00:00Z","updated_at":"2026-09-01T00:00:00Z"}}}}"#
        )
        .unwrap();
        writeln!(
            root,
            r#"{{"record_type":"issue","issue":{{"id":"forge-bbb","title":"Blocked","description":"","base_status":"open","priority":1,"issue_type":"task","labels":[],"assignee":null,"manual_blocked":false,"dependencies":[{{"blocker":"forge-aaa","kind":"blocks"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            root,
            r#"{{"record_type":"issue","issue":{{"id":"forge-ccc","title":"Claimed","description":"","base_status":"in_progress","priority":1,"issue_type":"task","labels":[],"assignee":"worker-9","manual_blocked":false,"dependencies":[]}}}}"#
        )
        .unwrap();
        writeln!(
            root,
            r#"{{"record_type":"issue","issue":{{"id":"forge-ddd","title":"Manually blocked","description":"","base_status":"open","priority":2,"issue_type":"task","labels":[],"assignee":null,"manual_blocked":true,"dependencies":[]}}}}"#
        )
        .unwrap();
        writeln!(
            root,
            r#"{{"record_type":"event","event":{{"id":"evt-1","kind":"created"}}}}"#
        )
        .unwrap();

        fs::write(
            checkpoint.join("current.json"),
            format!(
                r#"{{"active_root":{{"path":"objects/{root_sha}.jsonl","sha256":"{root_sha}"}},"generation_id":"gen-1","issue_count":4,"mode":"monolithic","schema_version":1}}"#
            ),
        )
        .unwrap();

        dir
    }

    #[test]
    fn test_detect_format() {
        let legacy = create_legacy_workspace();
        assert_eq!(
            detect_format(legacy.path()),
            Some(BeadStoreFormat::LegacyFlatJsonl)
        );

        let bead_rs = create_bead_rs_workspace();
        assert_eq!(detect_format(bead_rs.path()), Some(BeadStoreFormat::BeadRs));

        // A clone may carry only the checkpoint. The reader must not require
        // the live database or config file to recognize bead-rs.
        fs::remove_file(bead_rs.path().join(".beads/config.json")).unwrap();
        assert_eq!(detect_format(bead_rs.path()), Some(BeadStoreFormat::BeadRs));

        let empty = TempDir::new().unwrap();
        assert_eq!(detect_format(empty.path()), None);
    }

    #[test]
    fn test_read_legacy_flat() {
        let dir = create_legacy_workspace();
        let beads = read_all_beads(dir.path()).unwrap();

        assert_eq!(beads.len(), 3);
        assert_eq!(beads[0].id, "test-1");
        assert_eq!(beads[0].status, "open");
        assert_eq!(beads[1].blocking_dependency_ids(), vec!["test-1"]);
        assert_eq!(beads[2].assignee.as_deref(), Some("worker-1"));
    }

    #[test]
    fn test_read_bead_rs_checkpoint() {
        let dir = create_bead_rs_workspace();
        let beads = read_all_beads(dir.path()).unwrap();

        assert_eq!(beads.len(), 4);

        let by_id = |id: &str| beads.iter().find(|b| b.id == id).unwrap();
        let ready = by_id("forge-aaa");
        assert_eq!(ready.status, "open");
        assert_eq!(ready.priority, 0);
        assert_eq!(ready.labels, vec!["urgent".to_string()]);

        let blocked = by_id("forge-bbb");
        assert_eq!(blocked.blocking_dependency_ids(), vec!["forge-aaa"]);

        let claimed = by_id("forge-ccc");
        assert_eq!(claimed.status, "in_progress");
        assert_eq!(claimed.assignee.as_deref(), Some("worker-9"));

        // Event records must not leak in as issues.
        assert!(beads.iter().all(|b| b.id != "evt-1"));
    }

    #[test]
    fn test_read_bead_rs_falls_back_to_forensic() {
        let dir = create_bead_rs_workspace();
        let checkpoint = dir.path().join(".beads/checkpoint");

        // Drop the active root, leaving current.json pointing at it; the
        // reader must fall back to forensic.jsonl.
        let root_sha = "073fc7bbf1d714799316ad50cd08bf1be4418b04dc8934f183cf2ab88908614b";
        fs::remove_file(checkpoint.join(format!("objects/{root_sha}.jsonl"))).unwrap();

        let mut forensic = fs::File::create(checkpoint.join("forensic.jsonl")).unwrap();
        writeln!(
            forensic,
            r#"{{"record_type":"issue","issue":{{"id":"forge-aaa","title":"Ready","description":"","base_status":"open","priority":0,"issue_type":"task","labels":[],"assignee":null,"manual_blocked":false,"dependencies":[]}}}}"#
        )
        .unwrap();

        let beads = read_all_beads(dir.path()).unwrap();
        assert_eq!(beads.len(), 1);
        assert_eq!(beads[0].id, "forge-aaa");
    }

    #[test]
    fn test_no_store_reads_empty() {
        let dir = TempDir::new().unwrap();
        let beads = read_all_beads(dir.path()).unwrap();
        assert!(beads.is_empty());
    }

    #[test]
    fn test_ready_semantics() {
        let dir = create_bead_rs_workspace();
        let beads = read_all_beads(dir.path()).unwrap();
        let index = build_index(&beads);

        let is = |id: &str| is_ready(beads.iter().find(|b| b.id == id).unwrap(), &index);

        // Open, unassigned, unblocked.
        assert!(is("forge-aaa"));
        // Blocked by an unfinished blocks edge.
        assert!(!is("forge-bbb"));
        // In progress and assigned.
        assert!(!is("forge-ccc"));
        // Manually blocked.
        assert!(!is("forge-ddd"));
    }

    #[test]
    fn test_count_dependents() {
        let dir = create_bead_rs_workspace();
        let beads = read_all_beads(dir.path()).unwrap();

        assert_eq!(count_dependents(&beads, "forge-aaa"), 1);
        assert_eq!(count_dependents(&beads, "forge-bbb"), 0);
    }

    #[test]
    fn test_parse_priority_variants() {
        assert_eq!(parse_priority(Some(&serde_json::json!(0))), 0);
        assert_eq!(parse_priority(Some(&serde_json::json!("P3"))), 3);
        assert_eq!(parse_priority(Some(&serde_json::json!("1"))), 1);
        assert_eq!(parse_priority(None), 2);
        assert_eq!(parse_priority(Some(&serde_json::json!(null))), 2);
    }

    #[test]
    fn test_malformed_lines_are_skipped() {
        let dir = TempDir::new().unwrap();
        let beads_dir = dir.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();

        let mut file = fs::File::create(beads_dir.join("issues.jsonl")).unwrap();
        writeln!(file, "{{not json").unwrap();
        writeln!(file, r#"{{"title":"no id"}}"#).unwrap();
        writeln!(
            file,
            r#"{{"id":"ok-1","title":"Fine","status":"open","priority":1}}"#
        )
        .unwrap();

        let beads = read_all_beads(dir.path()).unwrap();
        assert_eq!(beads.len(), 1);
        assert_eq!(beads[0].id, "ok-1");
    }
}
