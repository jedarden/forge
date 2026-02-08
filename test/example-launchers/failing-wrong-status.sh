#!/bin/bash
# =============================================================================
# FORGE Launcher Protocol - FAILING Example: Wrong Status Value
# =============================================================================
# This is a FAILING example that demonstrates what happens when the
# status field has an incorrect value.
#
# EXPECTED FAILURE: Test 2 - Output Format
# This launcher will fail because the status field is not "spawned".
#
# The protocol requires status to be EXACTLY "spawned" in the stdout output.
# Common mistakes: "running", "started", "active", "ready"
#
# =============================================================================

set -e

# =============================================================================
# Argument Parsing
# =============================================================================
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
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$MODEL" ]] || [[ -z "$WORKSPACE" ]] || [[ -z "$SESSION_NAME" ]]; then
  echo "Error: Missing required arguments" >&2
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

# =============================================================================
# Worker Spawning
# =============================================================================
# Fix: Add output redirection to avoid subprocess timeout
(
  cd "$WORKSPACE"
  while true; do
    echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker active\"}" \
      >> ~/.forge/logs/$SESSION_NAME.log
    sleep 10
  done
) >/dev/null 2>&1 &

PID=$!

# =============================================================================
# Output Worker Metadata - BROKEN
# =============================================================================
# WRONG: status is "running" instead of "spawned"
# The protocol REQUIRES status to be exactly "spawned"
cat << EOF
{
  "worker_id": "$SESSION_NAME",
  "pid": $PID,
  "status": "running",
  "timestamp": "$(date -Iseconds)"
}
EOF

# =============================================================================
# Status File Creation
# =============================================================================
# Note: The status file CAN use "active", "idle", "starting", or "spawned"
# But the stdout output MUST be "spawned"
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
echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker started\", \"event\": \"worker_started\", \"model\": \"$MODEL\"}" \
  >> ~/.forge/logs/$SESSION_NAME.log

exit 0
