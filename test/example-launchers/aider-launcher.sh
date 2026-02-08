#!/bin/bash
# =============================================================================
# FORGE Launcher Protocol - Aider Example
# =============================================================================
# This is a PASSING example launcher for Aider (AI pair programming tool).
# It demonstrates the same protocol using a different spawn method.
#
# LAUNCHER PROTOCOL SUMMARY:
# --------------------------
# See claude-code-launcher.sh for detailed protocol documentation.
#
# This launcher demonstrates an alternative spawn pattern:
# - Uses direct subprocess (not tmux)
# - Shows config file handling (optional)
# - Demonstrates environment variable passing
#
# TESTING:
# --------
# Test this launcher with:
#   ./test/launcher-test-harness.py test/example-launchers/aider-launcher.sh
#
# =============================================================================

set -e

# =============================================================================
# Argument Parsing
# =============================================================================
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
if [[ -z "$MODEL" ]] || [[ -z "$WORKSPACE" ]] || [[ -z "$SESSION_NAME" ]]; then
  echo "Error: Missing required arguments" >&2
  echo "Usage: $0 --model=<model> --workspace=<path> --session-name=<name>" >&2
  exit 1
fi

if [[ ! -d "$WORKSPACE" ]]; then
  echo "Error: Workspace directory does not exist: $WORKSPACE" >&2
  exit 1
fi

# =============================================================================
# Directory Setup
# =============================================================================
mkdir -p ~/.forge/logs ~/.forge/status

# If config file is provided, source it (optional)
if [[ -n "$CONFIG" ]] && [[ -f "$CONFIG" ]]; then
  # In a real launcher, you might load additional settings from config
  # For this example, we just note that config was provided
  echo "Note: Using config file: $CONFIG" >&2
fi

# =============================================================================
# Worker Spawning
# =============================================================================
# Simulate Aider worker process
# In production, this would be: aider --model "$MODEL" --msg "$WORKSPACE"
#
# NOTE: Using output redirection to avoid subprocess capture issue
(
  cd "$WORKSPACE"
  # Simulate Aider's coding assistant loop
  while true; do
    echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Aider worker processing\", \"tool\": \"aider\"}" \
      >> ~/.forge/logs/$SESSION_NAME.log
    sleep 10
  done
) >/dev/null 2>&1 &

PID=$!

# =============================================================================
# Output Worker Metadata
# =============================================================================
cat << EOF
{
  "worker_id": "$SESSION_NAME",
  "pid": $PID,
  "status": "spawned",
  "launcher": "aider-launcher",
  "timestamp": "$(date -Iseconds)"
}
EOF

# =============================================================================
# Status File Creation
# =============================================================================
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
echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Aider worker started\", \"event\": \"worker_started\", \"model\": \"$MODEL\", \"tool\": \"aider\"}" \
  >> ~/.forge/logs/$SESSION_NAME.log

exit 0
