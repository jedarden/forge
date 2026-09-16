//! Cross-process bead claims via the bead CLI.
//!
//! The scheduler's in-process bead → worker mapping (`BeadScheduler`'s
//! assignment table) is invisible to every other process on the machine:
//! a second forge instance, or an external worker such as NEEDLE sharing
//! the same bead queue, keeps its own mapping and can select the very same
//! bead. This module closes that gap by making the **bead store itself**
//! the shared lock.
//!
//! The protocol, per `docs/BEAD_LAUNCHER_PROTOCOL.md` §4.1:
//!
//! 1. **Read** the bead's current assignment state:
//!    `bead show <bead-id> --json` → `assignee`, `status`, `revision`.
//!    A bead the store already shows as held by someone else is refused
//!    outright.
//! 2. **Claim** with a guarded, conditional write:
//!    `bead update <bead-id> --status in_progress --assignee <worker>
//!    --if-revision <revision>`. The `--if-revision` guard makes the
//!    read-then-write sequence atomic against competing processes: whoever
//!    commits first bumps the revision, and the loser's update fails with
//!    exit code 4 instead of silently clobbering the winner. The claim
//!    doubles as the protocol's step-4 "mark in-progress" transition, so
//!    assignment and status land in one guarded update.
//! 3. **Verify on retry**: a re-launch of an already-claimed bead re-reads
//!    the store and only skips the write when the claim still holds for
//!    the same worker — a claim taken over by another process is reported,
//!    not assumed.
//!
//! Lost races surface as [`forge_core::ForgeError::BeadClaimConflict`] so
//! the scheduler can drop the bead and move on to the next candidate
//! rather than launching two workers onto one bead.

use forge_core::types::BeadId;
use forge_core::{ForgeError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::process::Command;
use tracing::{debug, warn};

/// Default bead CLI binary (the canonical bead-rs CLI).
pub const DEFAULT_BEAD_BINARY: &str = "bead";

/// Exit code the bead CLI uses for conflicts: a stale `--if-revision`
/// revision, an invalid transition, or a refused claim.
pub const BEAD_EXIT_CONFLICT: i32 = 4;

/// What the bead store currently says about a bead's assignment.
///
/// `revision` is the store's logical revision of the bead; it is the
/// precondition token for the guarded claim write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredClaim {
    /// Assignee recorded in the store, if any.
    pub assignee: Option<String>,
    /// Base status (`open`, `in_progress`, `closed`, ...).
    pub status: String,
    /// Logical revision of the bead at read time.
    pub revision: i64,
    /// Claim epoch used as the fencing token for release/close mutations.
    #[serde(default)]
    pub claim_epoch: Option<u64>,
}

impl StoredClaim {
    /// A bead that exists but has never been claimed.
    fn unclaimed() -> Self {
        Self {
            assignee: None,
            status: "open".to_string(),
            revision: 0,
            claim_epoch: None,
        }
    }
}

/// Outcome of a guarded claim attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimOutcome {
    /// The conditional write landed: this worker owns the assignment, and
    /// the bead is marked in-progress.
    Acquired {
        /// The store revision after the claim.
        revision: i64,
    },
    /// The bead changed between the read and the guarded write (or the
    /// CLI refused the update as a conflict). The caller must not launch.
    Lost {
        /// Conflict detail from the CLI, for logs and error messages.
        detail: String,
    },
}

/// Whether a worker's previously-taken claim still holds in the store.
///
/// This is the retry-path check: a scheduler re-attempting an assignment
/// (after a rollback, a crash, or a failed launch) re-reads the store
/// instead of assuming its own claim survived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimVerification {
    /// The store still shows the bead assigned to this worker.
    Held,
    /// The claim is gone: unassigned, or held by someone else.
    Lost {
        /// The current holder, if the bead is assigned at all.
        holder: Option<String>,
    },
}

/// How the scheduler reaches the bead store for cross-process claims.
#[derive(Debug, Clone)]
pub enum BeadClaimBackend {
    /// Shell out to the bead CLI in the bead's workspace — the real
    /// backend, and the default. `bead show` reads the claim state and
    /// `bead update --if-revision` takes it atomically.
    Cli {
        /// Bead CLI binary name.
        binary: String,
    },
    /// An in-memory claim table. Used by tests and dry runs: two
    /// schedulers sharing one [`MemoryClaimStore`] (via its `Clone`) race
    /// exactly as two processes would, without executing any CLI.
    Memory(MemoryClaimStore),
}

impl Default for BeadClaimBackend {
    fn default() -> Self {
        Self::Cli {
            binary: DEFAULT_BEAD_BINARY.to_string(),
        }
    }
}

impl BeadClaimBackend {
    /// Read the bead's current claim state from the store.
    ///
    /// `Ok(None)` means the store's state is unavailable — no bead CLI,
    /// a store the CLI cannot read (legacy flat `issues.jsonl` without a
    /// beads database), or a bead the CLI does not know. Callers degrade
    /// to the in-process lock in that case; cross-process claiming is
    /// simply impossible there.
    pub async fn read_claim(
        &self,
        workspace: &Path,
        bead_id: &BeadId,
    ) -> Result<Option<StoredClaim>> {
        match self {
            Self::Cli { binary } => {
                let output = match self
                    .exec_cli(workspace, binary, &["show", bead_id, "--json"], "bead show")
                    .await
                {
                    Ok(output) => output,
                    // A store the CLI cannot read cannot be claimed
                    // through the CLI either; report unavailability rather
                    // than failing the whole scheduling pass.
                    Err(e) => {
                        warn!(
                            bead_id = %bead_id,
                            workspace = %workspace.display(),
                            error = %e,
                            "bead show failed; cross-process claim state unavailable"
                        );
                        return Ok(None);
                    }
                };

                let stdout = String::from_utf8_lossy(&output.stdout);
                match parse_show_payload(&stdout) {
                    Some(claim) => Ok(Some(claim)),
                    None => {
                        warn!(
                            bead_id = %bead_id,
                            "bead show output was not a recognized claim payload"
                        );
                        Ok(None)
                    }
                }
            }
            Self::Memory(store) => Ok(Some(store.read(bead_id))),
        }
    }

    /// Take the claim with a guarded conditional write.
    ///
    /// `expected` must be the claim state as read by
    /// [`BeadClaimBackend::read_claim`]; if the store's revision has moved
    /// since, another process got there first and [`ClaimOutcome::Lost`]
    /// is returned instead of clobbering it.
    pub async fn acquire(
        &self,
        workspace: &Path,
        bead_id: &BeadId,
        worker_id: &str,
        expected: &StoredClaim,
    ) -> Result<ClaimOutcome> {
        match self {
            Self::Cli { binary } => {
                let output = self
                    .exec_cli(
                        workspace,
                        binary,
                        &[
                            "update",
                            bead_id,
                            "--status",
                            "in_progress",
                            "--assignee",
                            worker_id,
                            "--if-revision",
                            &expected.revision.to_string(),
                        ],
                        "bead update --if-revision",
                    )
                    .await?;

                if output.status.code() == Some(BEAD_EXIT_CONFLICT) {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    debug!(
                        bead_id = %bead_id,
                        worker_id,
                        expected_revision = expected.revision,
                        "Bead claim lost the race (exit 4): {}",
                        stderr
                    );
                    return Ok(ClaimOutcome::Lost {
                        detail: if stderr.is_empty() {
                            format!(
                                "revision moved past {} before the guarded update landed",
                                expected.revision
                            )
                        } else {
                            stderr
                        },
                    });
                }

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(ForgeError::ToolExecution {
                        tool_name: "bead update --if-revision".to_string(),
                        message: stderr.to_string(),
                    });
                }

                Ok(ClaimOutcome::Acquired {
                    revision: expected.revision + 1,
                })
            }
            Self::Memory(store) => Ok(store.acquire(bead_id, worker_id, expected)),
        }
    }

    /// Check whether a previously-taken claim still holds for `worker_id`.
    ///
    /// The retry-path primitive: re-reads the store rather than trusting
    /// local state, so a claim taken over by another process is detected.
    pub async fn verify_claim(
        &self,
        workspace: &Path,
        bead_id: &BeadId,
        worker_id: &str,
    ) -> Result<ClaimVerification> {
        let stored = self.read_claim(workspace, bead_id).await?;
        match stored {
            Some(stored) if stored.assignee.as_deref() == Some(worker_id) => {
                Ok(ClaimVerification::Held)
            }
            Some(stored) => Ok(ClaimVerification::Lost {
                holder: stored.assignee,
            }),
            // Store unavailable: the claim cannot be confirmed.
            None => Ok(ClaimVerification::Lost { holder: None }),
        }
    }

    /// Give a claim back (the rollback side of a failed launch).
    ///
    /// Only releases when the store still shows the bead held by
    /// `worker_id` — a bead that was never claimed, or that another
    /// process has already taken over, is left alone. Returns whether a
    /// release was actually performed.
    pub async fn release_claim(
        &self,
        workspace: &Path,
        bead_id: &BeadId,
        worker_id: &str,
    ) -> Result<bool> {
        match self {
            Self::Cli { binary } => {
                let Some(stored) = self.read_claim(workspace, bead_id).await? else {
                    return Ok(false);
                };
                let ours =
                    stored.assignee.as_deref() == Some(worker_id) && stored.status == "in_progress";
                if !ours {
                    return Ok(false);
                }

                let mut args = vec!["release".to_string(), bead_id.to_string()];
                if let Some(epoch) = stored.claim_epoch {
                    args.extend(["--fencing-token".to_string(), epoch.to_string()]);
                }
                let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
                let output = self
                    .exec_cli(workspace, binary, &arg_refs, "bead release")
                    .await?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(ForgeError::ToolExecution {
                        tool_name: "bead release".to_string(),
                        message: stderr.to_string(),
                    });
                }
                Ok(true)
            }
            Self::Memory(store) => Ok(store.release(bead_id, worker_id)),
        }
    }

    /// Run the bead CLI in `workspace` and return the completed output.
    ///
    /// A non-zero exit is *not* mapped to an error here: conflict exit
    /// codes (notably 4 for a stale `--if-revision`) carry claim-race
    /// semantics the callers must interpret.
    async fn exec_cli(
        &self,
        workspace: &Path,
        binary: &str,
        args: &[&str],
        tool: &str,
    ) -> Result<std::process::Output> {
        Command::new(binary)
            .args(args)
            .current_dir(workspace)
            .output()
            .await
            .map_err(|e| ForgeError::io(format!("running {}", tool).as_str(), workspace, e))
    }
}

/// Parse the one-element `bead show <id> --json` payload into a claim.
///
/// The array shape is NEEDLE v1 compatibility (the bead object is element
/// 0); anything else — an empty array, a non-array, missing required
/// fields — returns `None`.
fn parse_show_payload(stdout: &str) -> Option<StoredClaim> {
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    let bead = value.as_array()?.first()?;
    let status = bead.get("status")?.as_str()?.to_string();
    let revision = bead.get("revision")?.as_i64()?;
    let claim_epoch = bead.get("claim_epoch").and_then(|e| e.as_u64());
    let assignee = bead
        .get("assignee")
        .and_then(|a| a.as_str())
        .map(str::to_string);

    Some(StoredClaim {
        assignee,
        status,
        revision,
        claim_epoch,
    })
}

/// Shared in-memory claim table behind
/// [`BeadClaimBackend::Memory`].
///
/// Cloning shares the same table (the fields are `Arc`s), which is what
/// lets a test pit two schedulers against one bead exactly as two
/// processes would race. Revision rules mirror the real store's: every
/// successful claim or release bumps the revision, and an `acquire` whose
/// expected revision is stale is refused.
#[derive(Debug, Clone, Default)]
pub struct MemoryClaimStore {
    claims: Arc<Mutex<HashMap<BeadId, StoredClaim>>>,
    /// Test hook: the next `acquire` on this bead reports a lost race even
    /// if the revision still matches, simulating a competing writer that
    /// lands between the scheduler's read and its guarded write.
    fail_next_acquire: Arc<Mutex<Vec<BeadId>>>,
}

impl MemoryClaimStore {
    /// Create an empty claim table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed a bead's claim state (e.g. to simulate a bead another process
    /// has already claimed).
    pub fn seed(&self, bead_id: impl Into<BeadId>, claim: StoredClaim) {
        self.claims
            .lock()
            .expect("memory claim store poisoned")
            .insert(bead_id.into(), claim);
    }

    /// Arm the lost-race hook: the next `acquire` on `bead_id` fails with
    /// a conflict even though the visible revision still matches,
    /// simulating a competing writer that lands between the scheduler's
    /// read and its guarded write.
    pub fn simulate_lost_race(&self, bead_id: impl Into<BeadId>) {
        self.fail_next_acquire
            .lock()
            .expect("memory claim store poisoned")
            .push(bead_id.into());
    }

    /// The current claim state for a bead (unclaimed if never touched).
    pub fn read(&self, bead_id: &BeadId) -> StoredClaim {
        self.claims
            .lock()
            .expect("memory claim store poisoned")
            .get(bead_id)
            .cloned()
            .unwrap_or_else(StoredClaim::unclaimed)
    }

    /// Guarded claim, mirroring `bead update --if-revision`.
    fn acquire(&self, bead_id: &BeadId, worker_id: &str, expected: &StoredClaim) -> ClaimOutcome {
        {
            let mut armed = self
                .fail_next_acquire
                .lock()
                .expect("memory claim store poisoned");
            if armed.iter().any(|id| id == bead_id) {
                armed.retain(|id| id != bead_id);
                return ClaimOutcome::Lost {
                    detail: "simulated lost race: revision moved underneath the claim".to_string(),
                };
            }
        }

        let mut claims = self.claims.lock().expect("memory claim store poisoned");
        let current = claims
            .get(bead_id)
            .cloned()
            .unwrap_or_else(StoredClaim::unclaimed);
        if current.revision != expected.revision {
            return ClaimOutcome::Lost {
                detail: format!(
                    "revision moved from {} to {} before the guarded update landed",
                    expected.revision, current.revision
                ),
            };
        }

        let claimed = StoredClaim {
            assignee: Some(worker_id.to_string()),
            status: "in_progress".to_string(),
            revision: current.revision + 1,
            claim_epoch: None,
        };
        claims.insert(bead_id.clone(), claimed);

        ClaimOutcome::Acquired {
            revision: expected.revision + 1,
        }
    }

    /// Release a claim held by `worker_id` (models `bead release`: the
    /// bead returns to open/unassigned and the revision is bumped as a
    /// high-water mark).
    fn release(&self, bead_id: &BeadId, worker_id: &str) -> bool {
        let mut claims = self.claims.lock().expect("memory claim store poisoned");
        match claims.get_mut(bead_id) {
            Some(claim)
                if claim.assignee.as_deref() == Some(worker_id)
                    && claim.status == "in_progress" =>
            {
                claim.assignee = None;
                claim.status = "open".to_string();
                claim.revision += 1;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    const SHOW_PAYLOAD: &str = r#"[
        {
            "id": "fg-1qo",
            "title": "Design bead-aware launcher protocol",
            "description": "",
            "priority": 0,
            "status": "open",
            "assignee": null,
            "claim_epoch": 3,
            "dependencies": [],
            "labels": [],
            "revision": 7
        }
    ]"#;

    #[test]
    fn test_parse_show_payload_reads_claim_state() {
        let claim = parse_show_payload(SHOW_PAYLOAD).expect("payload should parse");
        assert_eq!(claim.assignee, None);
        assert_eq!(claim.status, "open");
        assert_eq!(claim.revision, 7);
        assert_eq!(claim.claim_epoch, Some(3));
    }

    #[test]
    fn test_parse_show_payload_reads_assignee() {
        let payload = SHOW_PAYLOAD.replace("\"assignee\": null", "\"assignee\": \"worker-9\"");
        let claim = parse_show_payload(&payload).expect("payload should parse");
        assert_eq!(claim.assignee.as_deref(), Some("worker-9"));
    }

    #[test]
    fn test_parse_show_payload_rejects_unrecognized_shapes() {
        assert!(parse_show_payload("[]").is_none(), "empty array");
        assert!(parse_show_payload("not json").is_none());
        assert!(
            parse_show_payload("{\"id\": \"x\"}").is_none(),
            "not an array"
        );
        // Missing revision: the guard token is required.
        let no_revision = SHOW_PAYLOAD.replace("\"revision\": 7", "");
        assert!(parse_show_payload(&no_revision).is_none());
    }

    /// A stand-in `bead` binary: `show` cats a canned payload, `update`
    /// exits with a scripted code after logging its arguments. The caller
    /// must keep the returned TempDir alive while the backend is in use.
    fn write_fake_bead(show_payload: &str, update_exit: i32) -> TempDir {
        let bin_dir = TempDir::new().unwrap();
        let payload_file = bin_dir.path().join("show.json");
        fs::write(&payload_file, show_payload).unwrap();
        let log_file = bin_dir.path().join("update.log");
        let script = bin_dir.path().join("fake-bead.sh");
        let mut file = fs::File::create(&script).unwrap();
        writeln!(file, "#!/bin/sh").unwrap();
        writeln!(file, "if [ \"$1\" = \"show\" ]; then").unwrap();
        writeln!(file, "  cat {}", payload_file.display()).unwrap();
        writeln!(file, "  exit 0").unwrap();
        writeln!(file, "fi").unwrap();
        writeln!(file, "if [ \"$1\" = \"update\" ]; then").unwrap();
        writeln!(file, "  printf '%s\\n' \"$@\" >> {}", log_file.display()).unwrap();
        writeln!(file, "  exit {}", update_exit).unwrap();
        writeln!(file, "fi").unwrap();
        writeln!(file, "if [ \"$1\" = \"release\" ]; then").unwrap();
        writeln!(file, "  printf '%s\\n' \"$@\" >> {}", log_file.display()).unwrap();
        writeln!(file, "  exit 0").unwrap();
        writeln!(file, "fi").unwrap();
        writeln!(file, "exit 1").unwrap();
        drop(file);
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        bin_dir
    }

    fn update_log(bin: &TempDir) -> String {
        fs::read_to_string(bin.path().join("update.log")).unwrap_or_default()
    }

    #[tokio::test]
    async fn test_cli_acquire_sends_guarded_update_and_acquires() {
        let workspace = TempDir::new().unwrap();
        let bin = write_fake_bead(SHOW_PAYLOAD, 0);
        let backend = BeadClaimBackend::Cli {
            binary: bin.path().join("fake-bead.sh").display().to_string(),
        };

        let stored = backend
            .read_claim(workspace.path(), &"fg-1qo".to_string())
            .await
            .unwrap()
            .expect("payload should parse");
        assert_eq!(stored.revision, 7);

        let outcome = backend
            .acquire(workspace.path(), &"fg-1qo".to_string(), "worker-a", &stored)
            .await
            .unwrap();
        assert_eq!(
            outcome,
            ClaimOutcome::Acquired { revision: 8 },
            "the CLI bumped the revision past the guard"
        );

        // The guarded update went out with the read revision as the
        // precondition and the worker as assignee.
        let log = update_log(&bin);
        assert!(log.contains("--if-revision"), "log was: {}", log);
        assert!(log.contains("7"), "log was: {}", log);
        assert!(log.contains("--assignee"), "log was: {}", log);
        assert!(log.contains("worker-a"), "log was: {}", log);
        assert!(log.contains("--status"), "log was: {}", log);
        assert!(log.contains("in_progress"), "log was: {}", log);
    }

    #[tokio::test]
    async fn test_cli_acquire_maps_conflict_exit_to_lost() {
        let workspace = TempDir::new().unwrap();
        // Exit 4: the guard failed — another process moved the bead first.
        let bin = write_fake_bead(SHOW_PAYLOAD, BEAD_EXIT_CONFLICT);
        let backend = BeadClaimBackend::Cli {
            binary: bin.path().join("fake-bead.sh").display().to_string(),
        };

        let stored = backend
            .read_claim(workspace.path(), &"fg-1qo".to_string())
            .await
            .unwrap()
            .unwrap();

        let outcome = backend
            .acquire(workspace.path(), &"fg-1qo".to_string(), "worker-a", &stored)
            .await
            .unwrap();
        assert!(
            matches!(outcome, ClaimOutcome::Lost { .. }),
            "exit 4 must map to a lost race, not an error: {:?}",
            outcome
        );
    }

    #[tokio::test]
    async fn test_cli_read_degrades_when_store_unavailable() {
        let workspace = TempDir::new().unwrap();
        // The fake bead fails `show` outright: no readable store.
        let bin = write_fake_bead(SHOW_PAYLOAD, 0);
        fs::remove_file(bin.path().join("show.json")).unwrap();
        let backend = BeadClaimBackend::Cli {
            binary: bin.path().join("fake-bead.sh").display().to_string(),
        };

        let claim = backend
            .read_claim(workspace.path(), &"fg-1qo".to_string())
            .await
            .unwrap();
        assert!(claim.is_none(), "unreadable store must read as unavailable");

        // And the retry-path verification reports the claim as not held
        // rather than erroring.
        let verification = backend
            .verify_claim(workspace.path(), &"fg-1qo".to_string(), "worker-a")
            .await
            .unwrap();
        assert_eq!(verification, ClaimVerification::Lost { holder: None });
    }

    #[tokio::test]
    async fn test_cli_release_claim_only_releases_our_live_claim() {
        let workspace = TempDir::new().unwrap();
        let claimed = SHOW_PAYLOAD
            .replace("\"assignee\": null", "\"assignee\": \"worker-a\"")
            .replace("\"status\": \"open\"", "\"status\": \"in_progress\"");
        let bin = write_fake_bead(&claimed, 0);
        let backend = BeadClaimBackend::Cli {
            binary: bin.path().join("fake-bead.sh").display().to_string(),
        };

        let released = backend
            .release_claim(workspace.path(), &"fg-1qo".to_string(), "worker-a")
            .await
            .unwrap();
        assert!(released, "our live in-progress claim should release");
        let log = update_log(&bin);
        assert!(
            log.contains("--fencing-token\n3"),
            "release must carry the claim epoch fencing token: {log}"
        );

        // A bead held by someone else is left alone.
        let foreign = claimed.replace("worker-a", "worker-b");
        let bin = write_fake_bead(&foreign, 0);
        let backend = BeadClaimBackend::Cli {
            binary: bin.path().join("fake-bead.sh").display().to_string(),
        };
        let released = backend
            .release_claim(workspace.path(), &"fg-1qo".to_string(), "worker-a")
            .await
            .unwrap();
        assert!(!released, "another worker's claim must not be released");
    }

    #[tokio::test]
    async fn test_backend_verify_claim_reads_store_not_local_state() {
        let workspace = TempDir::new().unwrap();
        // The store shows the bead held by worker-a; a re-launch by
        // worker-a must see its claim still held, and worker-b must see
        // it taken.
        let claimed = SHOW_PAYLOAD
            .replace("\"assignee\": null", "\"assignee\": \"worker-a\"")
            .replace("\"status\": \"open\"", "\"status\": \"in_progress\"");
        let bin = write_fake_bead(&claimed, 0);
        let backend = BeadClaimBackend::Cli {
            binary: bin.path().join("fake-bead.sh").display().to_string(),
        };

        assert_eq!(
            backend
                .verify_claim(workspace.path(), &"fg-1qo".to_string(), "worker-a")
                .await
                .unwrap(),
            ClaimVerification::Held
        );
        assert_eq!(
            backend
                .verify_claim(workspace.path(), &"fg-1qo".to_string(), "worker-b")
                .await
                .unwrap(),
            ClaimVerification::Lost {
                holder: Some("worker-a".to_string())
            }
        );

        // And an open, unassigned bead verifies as lost with no holder.
        let bin = write_fake_bead(SHOW_PAYLOAD, 0);
        let backend = BeadClaimBackend::Cli {
            binary: bin.path().join("fake-bead.sh").display().to_string(),
        };
        assert_eq!(
            backend
                .verify_claim(workspace.path(), &"fg-1qo".to_string(), "worker-a")
                .await
                .unwrap(),
            ClaimVerification::Lost { holder: None }
        );
    }

    #[tokio::test]
    async fn test_memory_backend_end_to_end_claim_verify_release() {
        let workspace = TempDir::new().unwrap();
        let backend = BeadClaimBackend::Memory(MemoryClaimStore::new());
        let bead = "fg-1qo".to_string();

        // Verify before any claim: not held.
        assert_eq!(
            backend
                .verify_claim(workspace.path(), &bead, "worker-a")
                .await
                .unwrap(),
            ClaimVerification::Lost { holder: None }
        );

        let stored = backend
            .read_claim(workspace.path(), &bead)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            backend
                .acquire(workspace.path(), &bead, "worker-a", &stored)
                .await
                .unwrap(),
            ClaimOutcome::Acquired { revision: 1 }
        ));

        // Retry path: the claim still holds for us, and only for us.
        assert_eq!(
            backend
                .verify_claim(workspace.path(), &bead, "worker-a")
                .await
                .unwrap(),
            ClaimVerification::Held
        );
        assert_eq!(
            backend
                .verify_claim(workspace.path(), &bead, "worker-b")
                .await
                .unwrap(),
            ClaimVerification::Lost {
                holder: Some("worker-a".to_string())
            }
        );

        // Rollback path: releasing returns the bead to allocatable.
        assert!(
            backend
                .release_claim(workspace.path(), &bead, "worker-a")
                .await
                .unwrap()
        );
        assert!(
            !backend
                .release_claim(workspace.path(), &bead, "worker-a")
                .await
                .unwrap()
        );
        let after = backend
            .read_claim(workspace.path(), &bead)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.assignee, None);
        assert_eq!(after.status, "open");
        assert_eq!(after.revision, 2);
    }

    #[test]
    fn test_memory_store_claim_and_revision_guard() {
        let store = MemoryClaimStore::new();

        // First read of an untouched bead: open, unclaimed, revision 0.
        let initial = store.read(&"fg-1qo".to_string());
        assert_eq!(
            initial,
            StoredClaim {
                assignee: None,
                status: "open".to_string(),
                revision: 0,
                claim_epoch: None,
            }
        );

        let outcome = store.acquire(&"fg-1qo".to_string(), "worker-a", &initial);
        assert_eq!(outcome, ClaimOutcome::Acquired { revision: 1 });

        // A competing claimant that read the same pre-claim state loses:
        // the revision moved underneath it.
        let outcome = store.acquire(&"fg-1qo".to_string(), "worker-b", &initial);
        assert!(matches!(outcome, ClaimOutcome::Lost { .. }));

        let now = store.read(&"fg-1qo".to_string());
        assert_eq!(now.assignee.as_deref(), Some("worker-a"));
        assert_eq!(now.status, "in_progress");
        assert_eq!(now.revision, 1);
    }

    #[test]
    fn test_memory_store_verify_and_release() {
        let store = MemoryClaimStore::new();
        let bead = "fg-1qo".to_string();

        let held = store.acquire(&bead, "worker-a", &store.read(&bead));
        assert!(matches!(held, ClaimOutcome::Acquired { .. }));

        assert_eq!(
            store.acquire_via_verify(&bead, "worker-a"),
            ClaimVerification::Held
        );
        assert_eq!(
            store.acquire_via_verify(&bead, "worker-b"),
            ClaimVerification::Lost {
                holder: Some("worker-a".to_string())
            }
        );

        assert!(store.release(&bead, "worker-a"));
        assert!(!store.release(&bead, "worker-a"), "already released");

        let after = store.read(&bead);
        assert_eq!(after.assignee, None);
        assert_eq!(after.status, "open");
        assert_eq!(after.revision, 2, "release is a revision bump");
    }

    #[test]
    fn test_memory_store_simulated_lost_race() {
        let store = MemoryClaimStore::new();
        let bead = "fg-1qo".to_string();
        store.simulate_lost_race(bead.clone());

        // Even against a matching revision the armed hook reports the
        // race lost, and it disarms after firing once.
        let outcome = store.acquire(&bead, "worker-a", &store.read(&bead));
        assert!(matches!(outcome, ClaimOutcome::Lost { .. }));

        let outcome = store.acquire(&bead, "worker-a", &store.read(&bead));
        assert!(matches!(outcome, ClaimOutcome::Acquired { .. }));
    }

    /// Convenience wrapper so tests can reach `verify` semantics directly.
    impl MemoryClaimStore {
        fn acquire_via_verify(&self, bead_id: &BeadId, worker_id: &str) -> ClaimVerification {
            let stored = self.read(bead_id);
            if stored.assignee.as_deref() == Some(worker_id) {
                ClaimVerification::Held
            } else {
                ClaimVerification::Lost {
                    holder: stored.assignee,
                }
            }
        }
    }
}
