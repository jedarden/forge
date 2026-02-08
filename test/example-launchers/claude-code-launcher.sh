#!/bin/bash
# =============================================================================
# FORGE Launcher Protocol - Claude Code Example
# =============================================================================
# This is a PASSING example launcher that demonstrates the FORGE launcher protocol.
# It implements a minimal Claude Code worker launcher using tmux.
#
# LAUNCHER PROTOCOL SUMMARY:
# --------------------------
# A valid FORGE launcher script MUST:
#
# 1. Accept these command-line arguments:
#    --model=<model>         - Model identifier (e.g., "sonnet", "opus", "haiku")
#    --workspace=<path>      - Path to the workspace directory (must exist)
#    --session-name=<name>   - Unique session name for the worker
#    --config=<path>         - Optional: Path to worker configuration
#
# 2. Output JSON on stdout with EXACTLY these fields:
#    {
#      "worker_id": "<session-name>",    # Required: Worker identifier
#      "pid": <integer>,                  # Required: Process ID of spawned worker
#      "status": "spawned",               # Required: Must be "spawned" exactly
#      "launcher": "<name>",              # Optional: Launcher name for logging
#      "timestamp": "<ISO-8601>"          # Optional: When worker was spawned
#    }
#
# 3. Create status file at ~/.forge/status/<worker-id>.json:
#    {
#      "worker_id": "<worker-id>",
#      "status": "active",                # "active", "idle", "starting", or "spawned"
#      "model": "<model>",
#      "workspace": "<workspace-path>",
#      "pid": <integer>,
#      "started_at": "<ISO-8601>",
#      "last_activity": "<ISO-8601>",
#      "current_task": null,
#      "tasks_completed": 0
#    }
#
# 4. Create log file at ~/.forge/logs/<worker-id>.log:
#    Write JSON Lines (JSONL) format log entries:
#    {"timestamp": "<ISO-8601>", "level": "info", "worker_id": "<id>", "message": "..."}
#
# 5. Exit with code 0 on success, 1 on error
# 6. Print errors to stderr (not stdout)
# 7. Complete within 15 seconds (timeout enforced by test harness)
# 8. Spawn an actual running process (backgrounded)
#
# TESTING:
# --------
# Test this launcher with:
#   ./test/launcher-test-harness.py test/example-launchers/claude-code-launcher.sh
#
# =============================================================================

set -e  # Exit on error

# =============================================================================
# Argument Parsing
# =============================================================================
# Parse command-line arguments using a while loop
# This pattern handles all required and optional arguments
MODEL=""
WORKSPACE=""
SESSION_NAME=""
CONFIG=""

while [[ $# -gt 0 ]]; do
  case $1 in
    --model=*)
      MODEL="${1#*=}"
      shift
      ;;
    --workspace=*)
      WORKSPACE="${1#*=}"
      shift
      ;;
    --session-name=*)
      SESSION_NAME="${1#*=}"
      shift
      ;;
    --config=*)
      CONFIG="${1#*=}"
      shift
      ;;
    *)
      echo "Error: Unknown argument: $1" >&2
      echo "Usage: $0 --model=<model> --workspace=<path> --session-name=<name> [--config=<path>]" >&2
      exit 1
      ;;
  esac
done

# =============================================================================
# Validation
# =============================================================================
# Validate that all required arguments were provided
if [[ -z "$MODEL" ]]; then
  echo "Error: Missing required argument: --model" >&2
  exit 1
fi

if [[ -z "$WORKSPACE" ]]; then
  echo "Error: Missing required argument: --workspace" >&2
  exit 1
fi

if [[ -z "$SESSION_NAME" ]]; then
  echo "Error: Missing required argument: --session-name" >&2
  exit 1
fi

# Validate that the workspace directory exists
if [[ ! -d "$WORKSPACE" ]]; then
  echo "Error: Workspace directory does not exist: $WORKSPACE" >&2
  exit 1
fi

# =============================================================================
# Directory Setup
# =============================================================================
# Create required directories for FORGE integration
mkdir -p ~/.forge/logs ~/.forge/status

# =============================================================================
# Worker Spawning
# =============================================================================
# Spawn the worker process in the background using a subshell
# In a real launcher, this would be: tmux, docker run, etc.
# For this example, we simulate a worker with a sleep loop
#
# NOTE: Using setsid to create new session and avoid the subprocess
# capture_output issue when running under test harness.
(
  cd "$WORKSPACE"
  # Simulate a worker process that stays alive
  while true; do
    echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker active\"}" \
      >> ~/.forge/logs/$SESSION_NAME.log
    sleep 10
  done
) >/dev/null 2>&1 &

# Capture the PID of the backgrounded process
PID=$!

# =============================================================================
# Output Worker Metadata (stdout - JSON ONLY)
# =============================================================================
# This is the CRITICAL output that FORGE parses
# Must be valid JSON with required fields: worker_id, pid, status
# NOTE: No extra output before or after this JSON block
cat << EOF
{
  "worker_id": "$SESSION_NAME",
  "pid": $PID,
  "status": "spawned",
  "launcher": "claude-code-launcher",
  "timestamp": "$(date -Iseconds)"
}
EOF

# =============================================================================
# Status File Creation
# =============================================================================
# Create the status file that FORGE monitors for worker state
# This file MUST exist within 5 seconds of launcher execution
cat > ~/.forge/status/$SESSION_NAME.json << EOF
{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "workspace": "$WORKSPACE",
  "pid": $PID,
  "started_at": "$(date -Iseconds)",
  "last_activity": "$(date -Iseconds)",
  "current_task": null,
  "tasks_completed": 0
}
EOF

# =============================================================================
# Log File Creation
# =============================================================================
# Write initial log entry to show the worker started
# Use JSON Lines (JSONL) format for structured logging
echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker started\", \"event\": \"worker_started\", \"model\": \"$MODEL\"}" \
  >> ~/.forge/logs/$SESSION_NAME.log

# =============================================================================
# Clean Exit
# =============================================================================
# Exit immediately after spawning (don't wait for the worker)
# This is crucial - the launcher must return control to FORGE quickly
exit 0
