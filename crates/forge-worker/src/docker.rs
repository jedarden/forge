//! Docker container backend for workers.
//!
//! This module provides the container lifecycle behind the Docker worker
//! backend: spawning FORGE-managed containers, inspecting their state for
//! health checks, removing them on kill, capturing their logs, and listing
//! them for discovery.
//!
//! ## Container contract
//!
//! A FORGE worker container is:
//!
//! - **Named** after its session (same `<prefix><session-name>` convention as
//!   tmux sessions, so discovery and status checks can find it again).
//! - **Labeled** with `forge.worker=true` (plus the worker id and model), so
//!   discovery can list exactly the containers FORGE owns and never touches
//!   unrelated containers on the host.
//! - **Bind-mounted** at the worker's workspace: `-v <workspace>:/workspace`
//!   with `/workspace` as the working directory, mirroring how tmux workers
//!   are started inside their workspace.
//! - **Pinned**: the image reference must carry an explicit version tag or
//!   digest. `:latest` and untagged images are rejected at spawn time — a
//!   floating tag makes worker fleets irreproducible and turns an upstream
//!   push into a silent redeploy.
//! - **Cleaned up on kill**: stopping a worker runs `docker rm -f`, which
//!   stops the container *and* removes it, leaving no exited shells behind.
//!
//! ## Relationship to launcher scripts
//!
//! The tmux backend delegates to launcher scripts that emit the JSON
//! protocol. The Docker backend needs no script: [`WorkerLauncher`] builds
//! the container spec directly from the [`LaunchConfig`]
//! (`backend: WorkerBackend::Docker` + `with_docker_image(...)`).

use forge_core::{ForgeError, Result};
use std::path::Path;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, instrument, warn};

/// Label marking containers managed by FORGE (used for discovery filtering).
pub const WORKER_LABEL: &str = "forge.worker";

/// Mount point inside the container where the workspace is bind-mounted.
pub const WORKSPACE_MOUNT: &str = "/workspace";

/// Command used when the launch config does not specify one. It keeps the
/// container alive so the orchestrator can manage it; the actual agent
/// command is a deployment concern supplied via
/// [`LaunchConfig::with_container_command`](crate::types::LaunchConfig::with_container_command).
pub const DEFAULT_CONTAINER_COMMAND: &str = "sleep infinity";

/// Default docker CLI binary name.
pub const DEFAULT_DOCKER_BIN: &str = "docker";

/// Name prefix shared by all FORGE worker containers, matching the tmux
/// session prefix so discovery parses container names with the same
/// `<prefix><executor>-<suffix>` convention.
pub const CONTAINER_NAME_PREFIX: &str = "forge-";

/// Timeout for discovery/inspection calls, so a hung daemon cannot stall the
/// orchestrator.
const INSPECT_TIMEOUT_SECS: u64 = 10;

/// Validate that an image reference is pinned to an explicit version.
///
/// Accepts `repo/image:1.2.3`, `registry:5000/team/image:1.2.3`, and digest
/// pins (`repo/image@sha256:...`). Rejects empty references, untagged images
/// (including registry-host forms like `registry:5000/team/image`, where the
/// colon belongs to the port), and the floating `latest` tag in any case.
pub fn validate_image_ref(image: &str) -> Result<()> {
    if image.trim().is_empty() {
        return Err(ForgeError::WorkerSpawn {
            worker_id: "docker".into(),
            message: "Docker image reference is empty".into(),
        });
    }

    // Split off a digest pin first: `repo/image@sha256:...`. A digest is the
    // strongest possible pin and always acceptable.
    let (name, digest) = match image.split_once('@') {
        Some((name, digest)) if !digest.is_empty() => (name, Some(digest)),
        Some(_) => {
            return Err(ForgeError::WorkerSpawn {
                worker_id: "docker".into(),
                message: format!("Docker image `{image}` has an empty digest"),
            });
        }
        None => (image, None),
    };

    // A tag only exists if the last ':' comes after the last '/'; otherwise
    // the colon belongs to a registry port (`registry:5000/team/image`).
    let last_segment = name.rsplit('/').next().unwrap_or_default();
    let tag = last_segment.rsplit_once(':').map(|(_, tag)| tag);

    match (tag, digest) {
        (Some(tag), _) if tag.eq_ignore_ascii_case("latest") => Err(ForgeError::WorkerSpawn {
            worker_id: "docker".into(),
            message: format!(
                "Docker image `{image}` uses the floating `latest` tag; pin an explicit version"
            ),
        }),
        (Some(""), _) => Err(ForgeError::WorkerSpawn {
            worker_id: "docker".into(),
            message: format!("Docker image `{image}` has an empty tag"),
        }),
        (Some(_), _) => Ok(()),
        (None, Some(_)) => Ok(()),
        (None, None) => Err(ForgeError::WorkerSpawn {
            worker_id: "docker".into(),
            message: format!(
                "Docker image `{image}` has no version tag; pin an explicit version (e.g. `{image}:1.2.3`)"
            ),
        }),
    }
}

/// Build the `docker run` argument list for a worker container.
///
/// Pure (no I/O) so tests can assert the exact invocation: pinned image,
/// workspace bind-mount, FORGE labels, and environment propagation.
pub fn run_args(
    config: &crate::types::LaunchConfig,
    container_name: &str,
    worker_id: &str,
) -> Vec<String> {
    let image = config.image.as_deref().unwrap_or_default();
    let mut args: Vec<String> = vec![
        "run".into(),
        "-d".into(),
        "--name".into(),
        container_name.into(),
        "--label".into(),
        format!("{WORKER_LABEL}=true"),
        "--label".into(),
        format!("forge.worker_id={worker_id}"),
        "--label".into(),
        format!("forge.model={}", config.model),
        "--env".into(),
        format!("FORGE_WORKER_ID={worker_id}"),
        "--env".into(),
        format!("FORGE_SESSION={container_name}"),
        "--env".into(),
        format!("FORGE_MODEL={}", config.model),
        "--env".into(),
        format!("FORGE_WORKSPACE={}", config.workspace.display()),
        "--env".into(),
        "FORGE_BACKEND=docker".into(),
        "--volume".into(),
        format!("{}:{WORKSPACE_MOUNT}", config.workspace.display()),
        "--workdir".into(),
        WORKSPACE_MOUNT.into(),
    ];

    for (key, value) in &config.env {
        args.push("--env".into());
        args.push(format!("{key}={value}"));
    }

    args.push(image.into());

    let command = config
        .container_command
        .as_deref()
        .unwrap_or(DEFAULT_CONTAINER_COMMAND);
    args.extend(command.split_whitespace().map(str::to_string));

    args
}

/// Running state of a worker container, as reported by `docker inspect`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerState {
    /// Docker state string: `created`, `running`, `paused`, `restarting`,
    /// `exited`, or `dead`.
    pub status: String,
    /// PID of the container's init process as seen from the host, when the
    /// container is running.
    pub pid: Option<u32>,
}

/// Parse `docker inspect --format '{{json .State}}'` output.
pub fn parse_state_json(output: &str) -> Option<ContainerState> {
    let value: serde_json::Value = serde_json::from_str(output.trim()).ok()?;
    let status = value.get("Status")?.as_str()?.to_string();
    Some(ContainerState {
        status,
        pid: value.get("Pid").and_then(|p| p.as_u64()).map(|p| p as u32),
    })
}

/// Map a container state to the orchestrator's worker status.
pub fn state_to_worker_status(state: &ContainerState) -> forge_core::types::WorkerStatus {
    use forge_core::types::WorkerStatus;
    match state.status.as_str() {
        "running" => WorkerStatus::Active,
        "created" | "restarting" => WorkerStatus::Starting,
        "paused" => WorkerStatus::Paused,
        "exited" | "removing" => WorkerStatus::Stopped,
        "dead" => WorkerStatus::Failed,
        _ => WorkerStatus::Error,
    }
}

/// Run a worker container and return its container id.
#[instrument(level = "debug", skip_all, fields(container = %container_name, image = %config.image.as_deref().unwrap_or_default()))]
pub async fn run_container(
    docker_bin: &Path,
    config: &crate::types::LaunchConfig,
    container_name: &str,
    worker_id: &str,
) -> Result<String> {
    let mut cmd = Command::new(docker_bin);
    cmd.args(run_args(config, container_name, worker_id));

    let result = timeout(
        std::time::Duration::from_secs(config.timeout_secs),
        cmd.output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if id.is_empty() {
                return Err(ForgeError::WorkerSpawn {
                    worker_id: worker_id.into(),
                    message: "docker run produced no container id".into(),
                });
            }
            debug!("Started worker container {} ({})", container_name, id);
            Ok(id)
        }
        Ok(Ok(output)) => Err(ForgeError::WorkerSpawn {
            worker_id: worker_id.into(),
            message: format!(
                "docker run failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }),
        Ok(Err(e)) => Err(ForgeError::WorkerSpawn {
            worker_id: worker_id.into(),
            message: format!("Failed to execute {}: {}", docker_bin.display(), e),
        }),
        Err(_) => Err(ForgeError::WorkerSpawn {
            worker_id: worker_id.into(),
            message: format!("docker run timed out after {}s", config.timeout_secs),
        }),
    }
}

/// Inspect a container's state; `Ok(None)` when the container does not exist.
#[instrument(level = "debug", skip_all, fields(container = %container_name))]
pub async fn inspect_state(
    docker_bin: &Path,
    container_name: &str,
) -> Result<Option<ContainerState>> {
    let output = run_inspect(docker_bin, container_name).await;
    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(parse_state_json(&stdout))
        }
        Ok(output) => {
            // "No such object" and friends mean the container is gone.
            debug!(
                "Container {} not inspectable: {}",
                container_name,
                String::from_utf8_lossy(&output.stderr).trim()
            );
            Ok(None)
        }
        Err(e) => Err(ForgeError::WorkerSpawn {
            worker_id: container_name.into(),
            message: format!("Failed to execute docker inspect: {}", e),
        }),
    }
}

async fn run_inspect(
    docker_bin: &Path,
    container_name: &str,
) -> std::io::Result<std::process::Output> {
    timeout(
        std::time::Duration::from_secs(INSPECT_TIMEOUT_SECS),
        Command::new(docker_bin)
            .args(["inspect", "--format", "{{json .State}}", container_name])
            .output(),
    )
    .await
    .unwrap_or_else(|_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "docker inspect timed out",
        ))
    })
}

/// Check whether a container with this name exists (any state).
pub async fn container_exists(docker_bin: &Path, container_name: &str) -> Result<bool> {
    Ok(inspect_state(docker_bin, container_name).await?.is_some())
}

/// Stop and remove a worker container (`docker rm -f`).
///
/// This is the kill path: `-f` sends SIGKILL to the container's init process
/// and removes the container, so killed workers leave nothing behind.
/// Removing a non-existent container is tolerated, mirroring how the tmux
/// backend tolerates killing an already-dead session.
#[instrument(level = "debug", skip_all, fields(container = %container_name))]
pub async fn remove_container(docker_bin: &Path, container_name: &str) -> Result<()> {
    let output = Command::new(docker_bin)
        .args(["rm", "-f", container_name])
        .output()
        .await;

    match output {
        Ok(output) if output.status.success() => {
            debug!("Removed worker container {}", container_name);
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("No such container") {
                warn!(
                    "Failed to remove container {}: {}",
                    container_name,
                    stderr.trim()
                );
            }
        }
        Err(e) => {
            return Err(ForgeError::WorkerSpawn {
                worker_id: container_name.into(),
                message: format!("Failed to execute docker rm: {}", e),
            });
        }
    }

    Ok(())
}

/// Capture output from a worker container's logs.
///
/// The Docker analogue of [`crate::tmux::capture_pane`]: with `tail`, only
/// the last N lines are returned; with `None`, everything the container has
/// printed so far. `docker logs` merges stdout and stderr, so the result is
/// the container's full console output.
#[instrument(level = "debug", skip_all, fields(container = %container_name))]
pub async fn container_logs(
    docker_bin: &Path,
    container_name: &str,
    tail: Option<u32>,
) -> Result<String> {
    let mut cmd = Command::new(docker_bin);
    cmd.arg("logs");
    if let Some(n) = tail {
        cmd.args(["--tail", &n.to_string()]);
    }
    cmd.arg(container_name);

    let result = timeout(
        std::time::Duration::from_secs(INSPECT_TIMEOUT_SECS),
        cmd.output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let logs = String::from_utf8_lossy(&output.stdout).to_string();
            debug!(
                "Captured {} bytes of logs from container {}",
                logs.len(),
                container_name
            );
            Ok(logs)
        }
        Ok(Ok(output)) => Err(ForgeError::WorkerSpawn {
            worker_id: container_name.into(),
            message: format!(
                "docker logs failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }),
        Ok(Err(e)) => Err(ForgeError::WorkerSpawn {
            worker_id: container_name.into(),
            message: format!("Failed to execute docker logs: {}", e),
        }),
        Err(_) => Err(ForgeError::WorkerSpawn {
            worker_id: container_name.into(),
            message: "docker logs timed out".into(),
        }),
    }
}

/// Summary of a FORGE worker container, as listed by discovery.
#[derive(Debug, Clone)]
pub struct ContainerSummary {
    /// Container name (`<prefix><session-name>`).
    pub name: String,
    /// Pinned image reference the container runs.
    pub image: String,
    /// When the container was created (UTC).
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Parse one line of `docker ps --format '{{.Names}}\t{{.Image}}\t{{.CreatedAt}}'`.
pub fn parse_container_line(line: &str) -> Option<ContainerSummary> {
    let mut parts = line.split('\t');
    let name = parts.next()?.trim();
    let image = parts.next()?.trim();
    let created = parts.next()?.trim();

    if name.is_empty() || image.is_empty() {
        return None;
    }

    // Docker's CreatedAt is `2026-09-16 07:30:12 +0000 UTC`; parse the
    // timestamp portion and treat it as UTC (docker normalizes to the
    // daemon's local zone but prints the offset too — accept the common UTC
    // form and fall back to now for exotic locales).
    let created_at = chrono::NaiveDateTime::parse_from_str(
        &created[..created.len().min(19)],
        "%Y-%m-%d %H:%M:%S",
    )
    .map(|naive| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc))
    .unwrap_or_else(|_| chrono::Utc::now());

    Some(ContainerSummary {
        name: name.to_string(),
        image: image.to_string(),
        created_at,
    })
}

/// List all running containers carrying the FORGE worker label.
pub async fn list_worker_containers(docker_bin: &Path) -> Result<Vec<ContainerSummary>> {
    let output = timeout(
        std::time::Duration::from_secs(INSPECT_TIMEOUT_SECS),
        Command::new(docker_bin)
            .args([
                "ps",
                "--filter",
                &format!("label={WORKER_LABEL}=true"),
                "--format",
                "{{.Names}}\t{{.Image}}\t{{.CreatedAt}}",
            ])
            .output(),
    )
    .await;

    let output = match output {
        Ok(Ok(output)) if output.status.success() => output,
        Ok(Ok(output)) => {
            return Err(ForgeError::WorkerSpawn {
                worker_id: "docker".into(),
                message: format!(
                    "docker ps failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
        Ok(Err(e)) => {
            return Err(ForgeError::WorkerSpawn {
                worker_id: "docker".into(),
                message: format!("Failed to execute docker ps: {}", e),
            });
        }
        Err(_) => {
            return Err(ForgeError::WorkerSpawn {
                worker_id: "docker".into(),
                message: "docker ps timed out".into(),
            });
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let containers: Vec<ContainerSummary> =
        stdout.lines().filter_map(parse_container_line).collect();

    debug!("Found {} FORGE worker containers", containers.len());
    Ok(containers)
}

#[cfg(test)]
pub(crate) mod fake {
    //! A fake `docker` CLI for lifecycle tests: container state lives in a
    //! state file whose path is baked into the generated script, so tests
    //! never touch the process environment and no real daemon is needed.

    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    /// Write an executable fake docker binary into `dir`.
    ///
    /// The emulated container starts in the given `initial_state` (`None` =
    /// no container). The state can be changed afterwards with
    /// [`set_state`] / [`clear_state`].
    pub fn install(dir: &Path, initial_state: Option<&str>) -> PathBuf {
        let state_file = dir.join("container-state");
        match initial_state {
            Some(state) => std::fs::write(&state_file, state).unwrap(),
            None => {
                let _ = std::fs::remove_file(&state_file);
            }
        }

        let script = dir.join("docker");
        let body = format!(
            r#"#!/usr/bin/env bash
# Fake docker CLI for FORGE tests. State file holds the container status;
# absence of the file means no container exists.
STATE="{}"
cmd="$1"; shift
case "$cmd" in
  run)
    name=""
    prev=""
    for a in "$@"; do
      if [ "$prev" = "--name" ]; then name="$a"; fi
      prev="$a"
    done
    printf '%s' "running" > "$STATE"
    printf 'fakeworkercontainer0000000000000000000000000000000000000000000000000000'
    ;;
  inspect)
    name="${{@: -1}}"
    if [ ! -f "$STATE" ]; then
      echo "Error: No such object: $name" >&2
      exit 1
    fi
    for a in "$@"; do
      if [[ "$a" == *"json"* ]]; then
        printf '{{"Status":"%s","Pid":4242,"Running":true}}' "$(cat "$STATE")"
        exit 0
      fi
    done
    cat "$STATE"
    ;;
  rm)
    rm -f "$STATE"
    ;;
  logs)
    name="${{@: -1}}"
    if [ ! -f "$STATE" ]; then
      echo "Error: No such container: $name" >&2
      exit 1
    fi
    body='forge-worker starting
model ready
bead claimed
working
done'
    tailn=""
    prev=""
    for a in "$@"; do
      if [ "$prev" = "--tail" ]; then tailn="$a"; fi
      prev="$a"
    done
    if [ -n "$tailn" ]; then
      printf '%s\n' "$body" | tail -n "$tailn"
    else
      printf '%s\n' "$body"
    fi
    ;;
  ps)
    if [ -f "$STATE" ]; then
      printf 'forge-claude-code-sonnet-alpha\texample/agent:1.2.3\t2026-09-16 07:30:12 +0000 UTC\n'
    fi
    ;;
  *)
    ;;
esac
"#,
            state_file.display()
        );
        std::fs::write(&script, body).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    /// Set the emulated container's status (`running`, `exited`, ...).
    pub fn set_state(fake_dir: &Path, state: &str) {
        std::fs::write(fake_dir.join("container-state"), state).unwrap();
    }

    /// Remove the emulated container entirely.
    pub fn clear_state(fake_dir: &Path) {
        let _ = std::fs::remove_file(fake_dir.join("container-state"));
    }

    /// Whether the emulated container currently exists.
    pub fn container_exists(fake_dir: &Path) -> bool {
        fake_dir.join("container-state").exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LaunchConfig;
    use forge_core::types::WorkerStatus;
    use tempfile::TempDir;

    fn docker_config(image: Option<&str>) -> LaunchConfig {
        let mut config = LaunchConfig::new(
            "/unused/launcher.sh",
            "claude-code-sonnet-alpha",
            "/home/dev/project",
            "sonnet",
        );
        config.backend = crate::types::WorkerBackend::Docker;
        if let Some(image) = image {
            config = config.with_docker_image(image);
        }
        config
    }

    #[test]
    fn test_validate_image_ref_accepts_pinned_tags() {
        for image in [
            "ronaldraygun/forge-worker:1.2.3",
            "ronaldraygun/forge-worker:v0.3.1",
            "registry.ardenone.com:5000/team/agent:2.0",
            "example/agent@sha256:0123abcdef0123abcdef0123abcdef0123abcdef0123abcdef0123abcdef0123ab",
        ] {
            assert!(validate_image_ref(image).is_ok(), "should accept {image}");
        }
    }

    #[test]
    fn test_validate_image_ref_rejects_unpinned() {
        for image in [
            "",
            "   ",
            "ronaldraygun/forge-worker",             // no tag
            "registry.ardenone.com:5000/team/agent", // colon is a port, not a tag
            "ronaldraygun/forge-worker:latest",      // floating tag
            "ronaldraygun/forge-worker:LATEST",      // case-insensitive
            "ronaldraygun/forge-worker@",            // empty digest
        ] {
            assert!(
                validate_image_ref(image).is_err(),
                "should reject `{image}`"
            );
        }
    }

    #[test]
    fn test_run_args_pin_labels_bind_mount_and_command() {
        let config = docker_config(Some("example/agent:1.2.3"))
            .with_env("ANTHROPIC_BASE_URL", "https://proxy.example");

        let args = run_args(&config, "forge-claude-code-sonnet-alpha", "worker-1");

        assert_eq!(args[0], "run");
        assert!(args.contains(&"-d".to_string()));
        assert!(args.contains(&"example/agent:1.2.3".to_string()));

        // Workspace bind-mounted read-write at the fixed mount point, and
        // made the working directory.
        let vol_at = args.iter().position(|a| a == "--volume").unwrap();
        assert_eq!(args[vol_at + 1], "/home/dev/project:/workspace");
        let wd_at = args.iter().position(|a| a == "--workdir").unwrap();
        assert_eq!(args[wd_at + 1], "/workspace");

        // FORGE labels for discovery, including the manager label itself.
        let label_at = args.iter().position(|a| a == "--label").unwrap();
        assert_eq!(args[label_at + 1], "forge.worker=true");
        assert!(args.contains(&"forge.worker_id=worker-1".to_string()));
        assert!(args.contains(&"forge.model=sonnet".to_string()));

        // Protocol environment variables.
        assert!(args.contains(&"FORGE_WORKER_ID=worker-1".to_string()));
        assert!(args.contains(&"FORGE_SESSION=forge-claude-code-sonnet-alpha".to_string()));
        assert!(args.contains(&"FORGE_WORKSPACE=/home/dev/project".to_string()));
        assert!(args.contains(&"ANTHROPIC_BASE_URL=https://proxy.example".to_string()));

        // Default keeps the container alive; the image comes before it.
        let image_at = args
            .iter()
            .position(|a| a == "example/agent:1.2.3")
            .unwrap();
        assert_eq!(
            args[image_at + 1..],
            vec!["sleep".to_string(), "infinity".to_string()]
        );
    }

    #[test]
    fn test_run_args_custom_command() {
        let config = LaunchConfig::new("/unused.sh", "s", "/ws", "m")
            .with_docker_image("example/agent:1.0")
            .with_container_command("claude --dangerously-skip-permissions");

        let args = run_args(&config, "forge-s", "worker-1");
        let image_at = args.iter().position(|a| a == "example/agent:1.0").unwrap();
        assert_eq!(
            args[image_at + 1..],
            vec![
                "claude".to_string(),
                "--dangerously-skip-permissions".to_string()
            ]
        );
    }

    #[test]
    fn test_parse_state_json() {
        let state = parse_state_json(r#"{"Status":"running","Pid":4242,"Running":true}"#).unwrap();
        assert_eq!(state.status, "running");
        assert_eq!(state.pid, Some(4242));

        let state = parse_state_json(r#"{"Status":"exited","Pid":0,"Running":false}"#).unwrap();
        assert_eq!(state.status, "exited");
        assert_eq!(state.pid, Some(0));

        assert!(parse_state_json("not json").is_none());
        assert!(parse_state_json(r#"{"NoStatus":1}"#).is_none());
    }

    #[test]
    fn test_state_to_worker_status_mapping() {
        for (status, expected) in [
            ("running", WorkerStatus::Active),
            ("created", WorkerStatus::Starting),
            ("restarting", WorkerStatus::Starting),
            ("paused", WorkerStatus::Paused),
            ("exited", WorkerStatus::Stopped),
            ("removing", WorkerStatus::Stopped),
            ("dead", WorkerStatus::Failed),
            ("future-state", WorkerStatus::Error),
        ] {
            let state = ContainerState {
                status: status.into(),
                pid: None,
            };
            assert_eq!(state_to_worker_status(&state), expected, "{status}");
        }
    }

    #[test]
    fn test_parse_container_line() {
        let line =
            "forge-claude-code-sonnet-alpha\texample/agent:1.2.3\t2026-09-16 07:30:12 +0000 UTC";
        let summary = parse_container_line(line).unwrap();
        assert_eq!(summary.name, "forge-claude-code-sonnet-alpha");
        assert_eq!(summary.image, "example/agent:1.2.3");
        assert_eq!(
            summary.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-09-16 07:30:12"
        );

        assert!(parse_container_line("only-one-column").is_none());
        assert!(parse_container_line("\timage:1\t2026-09-16 07:30:12 +0000 UTC").is_none());
    }

    // ------------------------------------------------------------
    // Lifecycle against the fake docker CLI (spawn / inspect / kill)
    // ------------------------------------------------------------

    #[tokio::test]
    async fn test_container_lifecycle_spawn_inspect_remove() {
        let tmp = TempDir::new().unwrap();
        let fake = fake::install(tmp.path(), None);
        let config = docker_config(Some("example/agent:1.2.3")).with_timeout(10);

        // Nothing there yet.
        assert!(!container_exists(&fake, "forge-alpha").await.unwrap());
        assert!(inspect_state(&fake, "forge-alpha").await.unwrap().is_none());

        // Spawn.
        let id = run_container(&fake, &config, "forge-alpha", "worker-1")
            .await
            .unwrap();
        assert!(id.starts_with("fakeworkercontainer"));
        assert!(fake::container_exists(tmp.path()));

        // Inspect reports the running state with the fake PID.
        let state = inspect_state(&fake, "forge-alpha").await.unwrap().unwrap();
        assert_eq!(state.status, "running");
        assert_eq!(state.pid, Some(4242));
        assert_eq!(state_to_worker_status(&state), WorkerStatus::Active);

        // Kill cleans up the container entirely.
        remove_container(&fake, "forge-alpha").await.unwrap();
        assert!(!fake::container_exists(tmp.path()));
        assert!(inspect_state(&fake, "forge-alpha").await.unwrap().is_none());

        // Killing again is tolerated (idempotent cleanup).
        remove_container(&fake, "forge-alpha").await.unwrap();
    }

    #[tokio::test]
    async fn test_list_worker_containers_uses_label_filter() {
        let tmp = TempDir::new().unwrap();
        let fake = fake::install(tmp.path(), Some("running"));

        let containers = list_worker_containers(&fake).await.unwrap();
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].name, "forge-claude-code-sonnet-alpha");
        assert_eq!(containers[0].image, "example/agent:1.2.3");

        // No container → empty list, not an error.
        fake::clear_state(tmp.path());
        let containers = list_worker_containers(&fake).await.unwrap();
        assert!(containers.is_empty());
    }

    #[tokio::test]
    async fn test_missing_docker_binary_errors_on_spawn() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("no-such-docker");
        let config = docker_config(Some("example/agent:1.2.3")).with_timeout(5);

        let result = run_container(&missing, &config, "forge-alpha", "worker-1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_container_logs_full_and_tail() {
        let tmp = TempDir::new().unwrap();
        let fake = fake::install(tmp.path(), Some("running"));

        // Full capture returns every line the container printed.
        let logs = container_logs(&fake, "forge-alpha", None).await.unwrap();
        let lines: Vec<&str> = logs.lines().collect();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "forge-worker starting");
        assert_eq!(lines[4], "done");

        // Tail caps the capture to the last N lines.
        let logs = container_logs(&fake, "forge-alpha", Some(2)).await.unwrap();
        assert_eq!(logs.lines().collect::<Vec<_>>(), vec!["working", "done"]);

        // Logs of a missing container are an error, mirroring the CLI.
        fake::clear_state(tmp.path());
        let err = container_logs(&fake, "forge-alpha", None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("No such container"), "{err}");
    }
}
