//! Integration tests: the bead queue must read a bead-rs checkpoint non-empty.
//!
//! The regression these tests guard against is the task queue silently
//! reading as empty after a store migration. The fixtures under
//! `tests/fixtures/` model the two on-disk layouts FORGE supports (see
//! `crates/forge-core/src/bead_store.rs` for the format contract):
//!
//! - `bead-rs-store/` — a realistic committed checkpoint: `.beads/config.json`
//!   plus `.beads/checkpoint/{current.json,forensic.jsonl,objects/<sha256>.jsonl}`,
//!   content-addressed exactly like the real thing, mixing `issue`, `event`,
//!   and `attempt_outcome` records with `base_status` fields and
//!   `{"blocker": ..., "kind": ...}` dependency objects.
//! - `legacy-flat-store/` — a pre-migration bead-forge workspace: flat
//!   `.beads/issues.jsonl` of bare issue objects with plain `status` fields
//!   and bare-string dependencies.
//!
//! The fixtures are static and checked in; the tests resolve them relative to
//! `CARGO_MANIFEST_DIR`, so they exercise the reader end to end against the
//! same bytes every run.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use forge_core::bead_store::{
    BeadStoreFormat, build_index, count_dependents, detect_format, is_ready,
};
use forge_core::{StoreBead, claim_epoch, read_all_beads};
use tempfile::TempDir;

/// Absolute path to a named fixture workspace under `tests/fixtures/`.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Find a bead by id, panicking with a message that names what went missing.
fn find<'a>(beads: &'a [StoreBead], id: &str) -> &'a StoreBead {
    beads
        .iter()
        .find(|b| b.id == id)
        .unwrap_or_else(|| panic!("bead `{id}` missing from the recovered queue"))
}

/// Recursively copy a fixture into a temp dir for tests that need to mutate it.
fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn copy_fixture(name: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    copy_dir(&fixture(name), dir.path());
    dir
}

/// The core regression guard: a bead-rs checkpoint must yield a non-empty
/// queue with every bead and dependency edge recovered.
#[test]
fn test_bead_rs_checkpoint_reads_non_empty_with_dependencies() {
    let workspace = fixture("bead-rs-store");

    assert_eq!(
        detect_format(&workspace),
        Some(BeadStoreFormat::BeadRs),
        "fixture models a bead-rs store; detection disagreed"
    );

    let beads = read_all_beads(&workspace).expect("bead-rs checkpoint fixture must parse");

    // The regression this suite exists for: a migration-era reader bug made
    // this exact call return an empty queue. Non-empty is asserted before
    // anything more specific so a failure names the regression, not a symptom.
    assert!(
        !beads.is_empty(),
        "regression: bead queue read as empty from a bead-rs checkpoint"
    );
    assert_eq!(beads.len(), 7, "every issue record must be recovered");
    // Event and attempt_outcome records share the snapshot; the recovered
    // set must be exactly the seven fixture issues and nothing else.
    let mut ids: Vec<&str> = beads.iter().map(|b| b.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        vec![
            "fixture-aaa1",
            "fixture-bbb2",
            "fixture-ccc3",
            "fixture-ddd4",
            "fixture-eee5",
            "fixture-fff6",
            "fixture-zzz9",
        ]
    );

    // Field mapping survived the bead-rs wrapping: `base_status` -> status,
    // labels, assignee, claim epoch, manual_blocked.
    let bootstrap = find(&beads, "fixture-zzz9");
    assert_eq!(bootstrap.status, "closed");
    let migrating = find(&beads, "fixture-ddd4");
    assert_eq!(migrating.status, "in_progress");
    assert_eq!(migrating.assignee.as_deref(), Some("worker-fixture"));
    let wired = find(&beads, "fixture-aaa1");
    assert_eq!(wired.status, "open");
    assert_eq!(wired.priority, 1);
    assert_eq!(wired.labels, vec!["dashboard", "queue"]);
    assert!(find(&beads, "fixture-eee5").manual_blocked);

    // Dependency links are intact, including the edge kind.
    assert_eq!(
        find(&beads, "fixture-bbb2").blocking_dependency_ids(),
        vec!["fixture-aaa1"]
    );
    assert_eq!(count_dependents(&beads, "fixture-aaa1"), 1);
    let related = find(&beads, "fixture-fff6");
    assert_eq!(
        related.dependencies,
        vec![forge_core::StoreDependency {
            blocker: "fixture-aaa1".to_string(),
            kind: "relates-to".to_string(),
        }]
    );
    // A non-blocks edge gates nothing.
    assert!(related.blocking_dependency_ids().is_empty());

    // Readiness semantics over the recovered index: open+unassigned is ready;
    // an unfinished blocker, an assignee, in_progress, and manual_blocked
    // are not; a closed blocker stops gating.
    let index: HashMap<&str, &StoreBead> = build_index(&beads);
    let ready = |id: &str| is_ready(find(&beads, id), &index);
    assert!(ready("fixture-aaa1"));
    assert!(!ready("fixture-bbb2"));
    assert!(ready("fixture-ccc3"));
    assert!(!ready("fixture-ddd4"));
    assert!(!ready("fixture-eee5"));
    assert!(ready("fixture-fff6"));
    assert!(!ready("fixture-zzz9"));

    // The fencing token travels with the claimed bead and only that bead.
    assert_eq!(claim_epoch(&workspace, "fixture-ddd4").unwrap(), Some(3));
    assert_eq!(claim_epoch(&workspace, "fixture-aaa1").unwrap(), None);
}

/// Losing the active root must fall back to `forensic.jsonl` — a full copy of
/// the same records — so a half-synced checkout cannot silently lose the queue.
#[test]
fn test_bead_rs_checkpoint_forensic_fallback_recovers_beads() {
    let dir = copy_fixture("bead-rs-store");
    let checkpoint = dir.path().join(".beads/checkpoint");

    // Remove the object tree while current.json still points at it.
    fs::remove_dir_all(checkpoint.join("objects")).unwrap();

    // Still a bead-rs store (forensic.jsonl remains), never "no store".
    assert_eq!(detect_format(dir.path()), Some(BeadStoreFormat::BeadRs));

    let beads = read_all_beads(dir.path())
        .expect("forensic.jsonl fallback must parse after losing the active root");
    assert_eq!(
        beads.len(),
        7,
        "forensic.jsonl carries a full copy; the queue must not go empty"
    );
    assert_eq!(
        find(&beads, "fixture-bbb2").blocking_dependency_ids(),
        vec!["fixture-aaa1"],
        "dependency links must survive the fallback path too"
    );
}

/// A legacy flat store must be detected and reported as legacy — a distinct
/// outcome from both bead-rs and "no store" — so a workspace that never
/// finished migrating is visible rather than silently rendering an empty queue.
#[test]
fn test_legacy_flat_store_detected_not_silently_empty() {
    let legacy = fixture("legacy-flat-store");

    // Three-way distinction: legacy is neither bead-rs nor absent.
    assert_eq!(
        detect_format(&legacy),
        Some(BeadStoreFormat::LegacyFlatJsonl),
        "migration gap must be detectable: a legacy store is not 'no store'"
    );
    assert_eq!(
        detect_format(&fixture("bead-rs-store")),
        Some(BeadStoreFormat::BeadRs)
    );
    let empty = TempDir::new().unwrap();
    assert_eq!(detect_format(empty.path()), None);

    // The legacy reader still recovers the queue, dependencies included:
    // bare-string blockers normalize to `blocks` edges.
    let beads = read_all_beads(&legacy).expect("legacy flat store fixture must parse");
    assert!(
        !beads.is_empty(),
        "regression: legacy store read as empty; the migration gap would be invisible"
    );
    assert_eq!(beads.len(), 3);
    assert_eq!(find(&beads, "legacy-aaa1").status, "open");
    assert_eq!(find(&beads, "legacy-ccc3").status, "in_progress");
    assert_eq!(
        find(&beads, "legacy-bbb2").blocking_dependency_ids(),
        vec!["legacy-aaa1"]
    );
}
