#!/bin/bash
# =============================================================================
# FORGE Launcher Protocol - Continue Example
# =============================================================================
# This is a PASSING example launcher for Continue (VS Code extension).
# It demonstrates the protocol using Python-style subprocess spawning.
#
# LAUNCHER PROTOCOL SUMMARY:
# --------------------------
# See claude-code-launcher.sh for detailed protocol documentation.
#
# This launcher demonstrates:
# - Minimal argument parsing with positional args fallback
# - Simpler validation pattern
# - Demonstrates different log message formats
#
# TESTING:
# --------
# Test this launcher with:
#   ./test/launcher-test-harness.py test/example-launchers/continue-launcher.sh
#
# =============================================================================

set -e

# =============================================================================
# Argument Parsing
# =============================================================================
# Continue launcher supports both --flag=value and positional arguments
# This demonstrates flexibility in argument handling
MODEL=""
WORKSPACE=""
SESSION_NAME=""

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
    *)
      # Fallback: try positional arguments
      if [[ -z "$MODEL" ]]; then
        MODEL="$1"
      elif [[ -z "$WORKSPACE" ]]; then
        WORKSPACE="$1"
      elif [[ -z "$SESSION_NAME" ]]; then
        SESSION_NAME="$1"
      else
        echo "Error: Too many arguments: $1" >&2
        exit 1
      fi
      shift
      ;;
  esac
done

# =============================================================================
# Validation
# =============================================================================
# Simple validation with clear error messages
if [[ -z "$MODEL" ]]; then
  echo "Error: --model argument is required" >&2
  exit 1
fi

if [[ -z "$WORKSPACE" ]]; then
  echo "Error: --workspace argument is required" >&2
  exit 1
fi

if [[ -z "$SESSION_NAME" ]]; then
  echo "Error: --session-name argument is required" >&2
  exit 1
fi

if [[ ! -d "$WORKSPACE" ]]; then
  echo "Error: Workspace does not exist: $WORKSPACE" >&2
  exit 1
fi

# =============================================================================
# Directory Setup
# =============================================================================
mkdir -p ~/.forge/logs ~/.forge/status

# =============================================================================
# Worker Spawning
# =============================================================================
# Simulate Continue extension worker
# In production, Continue would run as a VS Code extension or headless mode
#
# NOTE: Using output redirection to avoid subprocess capture issue
(
  cd "$WORKSPACE"
  # Simulate Continue's autocomplete and code generation loop
  while true; do
    echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"debug\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Continue context update\", \"tool\": \"continue\"}" \
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
  "launcher": "continue-launcher",
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
echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Continue worker started\", \"event\": \"worker_started\", \"model\": \"$MODEL\", \"tool\": \"continue\"}" \
  >> ~/.forge/logs/$SESSION_NAME.log

exit 0
