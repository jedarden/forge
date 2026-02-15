//! Tmux session management for workers.
//!
//! This module provides utilities for creating, managing, and interacting
//! with tmux sessions that host worker processes.

use forge_core::{ForgeError, Result};
use std::path::Path;
use tokio::process::Command;
use tracing::{debug, instrument, warn};

/// Check if a tmux session exists.
#[instrument(level = "debug", skip_all, fields(session = %session_name))]
pub async fn session_exists(session_name: &str) -> Result<bool> {
    let output = Command::new("tmux")
        .args(["has-session", "-t", session_name])
        .output()
        .await
        .map_err(|e| ForgeError::WorkerSpawn {
            worker_id: session_name.into(),
            message: format!("Failed to check tmux session: {}", e),
        })?;

    Ok(output.status.success())
}

/// Kill a tmux session.
#[instrument(level = "debug", skip_all, fields(session = %session_name))]
pub async fn kill_session(session_name: &str) -> Result<()> {
    let output = Command::new("tmux")
        .args(["kill-session", "-t", session_name])
        .output()
        .await
        .map_err(|e| ForgeError::WorkerSpawn {
            worker_id: session_name.into(),
            message: format!("Failed to kill tmux session: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // It's okay if the session doesn't exist
        if !stderr.contains("can't find session") {
            warn!("Failed to kill session {}: {}", session_name, stderr);
        }
    }

    debug!("Killed tmux session: {}", session_name);
    Ok(())
}

/// Create a new detached tmux session.
///
/// # Arguments
/// * `session_name` - Name for the tmux session
/// * `working_dir` - Working directory for the session
/// * `command` - Optional command to run in the session
#[instrument(level = "debug", skip_all, fields(session = %session_name, dir = %working_dir.display()))]
pub async fn create_session(
    session_name: &str,
    working_dir: &Path,
    command: Option<&str>,
) -> Result<()> {
    let mut args = vec![
        "new-session",
        "-d", // Detached
        "-s",
        session_name,
        "-c",
        working_dir.to_str().unwrap_or("."),
    ];

    if let Some(cmd) = command {
        args.push(cmd);
    }

    let output = Command::new("tmux")
        .args(&args)
        .output()
        .await
        .map_err(|e| ForgeError::WorkerSpawn {
            worker_id: session_name.into(),
            message: format!("Failed to create tmux session: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ForgeError::WorkerSpawn {
            worker_id: session_name.into(),
            message: format!("tmux new-session failed: {}", stderr.trim()),
        });
    }

    debug!("Created tmux session: {}", session_name);
    Ok(())
}

/// Send a command to a tmux session.
#[instrument(level = "debug", skip_all, fields(session = %session_name))]
pub async fn send_command(session_name: &str, command: &str) -> Result<()> {
    let output = Command::new("tmux")
        .args(["send-keys", "-t", session_name, command, "Enter"])
        .output()
        .await
        .map_err(|e| ForgeError::WorkerSpawn {
            worker_id: session_name.into(),
            message: format!("Failed to send command to tmux: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ForgeError::WorkerSpawn {
            worker_id: session_name.into(),
            message: format!("tmux send-keys failed: {}", stderr.trim()),
        });
    }

    debug!("Sent command to session {}: {}", session_name, command);
    Ok(())
}

/// Get the PID of the main process in a tmux session.
#[instrument(level = "debug", skip_all, fields(session = %session_name))]
pub async fn get_session_pid(session_name: &str) -> Result<Option<u32>> {
    // Get the pane PID (the shell or process running in the pane)
    let output = Command::new("tmux")
        .args(["display-message", "-t", session_name, "-p", "#{pane_pid}"])
        .output()
        .await
        .map_err(|e| ForgeError::WorkerSpawn {
            worker_id: session_name.into(),
            message: format!("Failed to get tmux pane PID: {}", e),
        })?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let pid_str = stdout.trim();

    if pid_str.is_empty() {
        return Ok(None);
    }

    match pid_str.parse::<u32>() {
        Ok(pid) => {
            debug!("Session {} has pane PID: {}", session_name, pid);
            Ok(Some(pid))
        }
        Err(_) => {
            warn!("Invalid PID from tmux: {}", pid_str);
            Ok(None)
        }
    }
}

/// List all tmux sessions matching a prefix.
#[instrument(level = "debug", skip_all, fields(prefix = %prefix))]
pub async fn list_sessions(prefix: &str) -> Result<Vec<String>> {
    let output = Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output()
        .await
        .map_err(|e| ForgeError::WorkerSpawn {
            worker_id: "tmux".into(),
            message: format!("Failed to list tmux sessions: {}", e),
        })?;

    if !output.status.success() {
        // No sessions is not an error
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let sessions: Vec<String> = stdout
        .lines()
        .filter(|line| line.starts_with(prefix))
        .map(|s| s.to_string())
        .collect();

    debug!("Found {} sessions with prefix '{}'", sessions.len(), prefix);
    Ok(sessions)
}

/// Capture the current output from a tmux pane.
#[instrument(level = "debug", skip_all, fields(session = %session_name))]
pub async fn capture_pane(session_name: &str, lines: Option<u32>) -> Result<String> {
    let lines_arg = lines
        .map(|n| format!("-{}", n))
        .unwrap_or_else(|| "-".to_string());

    let output = Command::new("tmux")
        .args(["capture-pane", "-t", session_name, "-p", "-S", &lines_arg])
        .output()
        .await
        .map_err(|e| ForgeError::WorkerSpawn {
            worker_id: session_name.into(),
            message: format!("Failed to capture tmux pane: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ForgeError::WorkerSpawn {
            worker_id: session_name.into(),
            message: format!("tmux capture-pane failed: {}", stderr.trim()),
        });
    }

    let content = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(content)
}

/// Pause a worker by sending SIGSTOP to its tmux session.
///
/// This sends a pause signal (SIGSTOP) to all processes in the session's
/// process group, which should cause the worker and all its children to
/// suspend. The worker can later be resumed with `resume_session`.
///
/// Note: SIGSTOP is used instead of SIGTSTP because it cannot be
/// caught or ignored by the target process, ensuring reliable pause
/// functionality even for multi-threaded workers.
///
/// # Arguments
/// * `session_name` - The tmux session name (e.g., "claude-code-glm-47-alpha")
#[instrument(level = "debug", skip_all, fields(session = %session_name))]
pub async fn pause_session(session_name: &str) -> Result<()> {
    // Get the pane PID
    let pid = match get_session_pid(session_name).await? {
        Some(p) => p,
        None => {
            return Err(ForgeError::WorkerSpawn {
                worker_id: session_name.into(),
                message: "No PID found for session".to_string(),
            });
        }
    };

    // Send SIGSTOP to the entire process group using negative PID
    // This stops the pane process and all its children
    let output = Command::new("kill")
        .args(["-STOP", &format!("-{}", pid)])
        .output()
        .await
        .map_err(|e| ForgeError::WorkerSpawn {
            worker_id: session_name.into(),
            message: format!("Failed to send pause signal: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ForgeError::WorkerSpawn {
            worker_id: session_name.into(),
            message: format!("Failed to pause session: {}", stderr.trim()),
        });
    }

    debug!("Paused tmux session: {}", session_name);
    Ok(())
}

/// Resume a paused worker by sending SIGCONT to its tmux session.
///
/// This sends a continue signal (SIGCONT) to all processes in the session's
/// process group, which should resume previously suspended worker processes.
///
/// # Arguments
/// * `session_name` - The tmux session name (e.g., "claude-code-glm-47-alpha")
#[instrument(level = "debug", skip_all, fields(session = %session_name))]
pub async fn resume_session(session_name: &str) -> Result<()> {
    // Get the pane PID
    let pid = match get_session_pid(session_name).await? {
        Some(p) => p,
        None => {
            return Err(ForgeError::WorkerSpawn {
                worker_id: session_name.into(),
                message: "No PID found for session".to_string(),
            });
        }
    };

    // Send SIGCONT to the entire process group using negative PID
    // This resumes the pane process and all its children
    let output = Command::new("kill")
        .args(["-CONT", &format!("-{}", pid)])
        .output()
        .await
        .map_err(|e| ForgeError::WorkerSpawn {
            worker_id: session_name.into(),
            message: format!("Failed to send resume signal: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ForgeError::WorkerSpawn {
            worker_id: session_name.into(),
            message: format!("Failed to resume session: {}", stderr.trim()),
        });
    }

    debug!("Resumed tmux session: {}", session_name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require tmux to be installed and may create real sessions.
    // They are marked as ignored by default.

    #[tokio::test]
    #[ignore = "requires tmux installation"]
    async fn test_session_lifecycle() {
        let session_name = "forge-test-session";

        // Clean up any existing session
        let _ = kill_session(session_name).await;

        // Should not exist initially
        assert!(!session_exists(session_name).await.unwrap());

        // Create session
        create_session(session_name, Path::new("/tmp"), None)
            .await
            .unwrap();

        // Should exist now
        assert!(session_exists(session_name).await.unwrap());

        // Get PID
        let pid = get_session_pid(session_name).await.unwrap();
        assert!(pid.is_some());

        // Kill session
        kill_session(session_name).await.unwrap();

        // Should not exist anymore
        assert!(!session_exists(session_name).await.unwrap());
    }

    #[tokio::test]
    #[ignore = "requires tmux installation"]
    async fn test_list_sessions() {
        let sessions = list_sessions("forge-").await.unwrap();
        // Just verify it doesn't error
        assert!(sessions.is_empty() || !sessions.is_empty());
    }
}
