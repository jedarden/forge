#!/bin/bash
# =============================================================================
# FORGE Launcher Protocol - FAILING Example: Missing Log File
# =============================================================================
# This is a FAILING example that demonstrates what happens when the
# log file is not created or is empty.
#
# EXPECTED FAILURE: Test 4 - Log File Creation
# This launcher will fail because it doesn't create the log file.
#
# The protocol requires ~/.forge/logs/<worker-id>.log to be created
# within 5 seconds and contain at least one log entry.
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
# INTENTIONALLY BROKEN: No logging to file
(
  cd "$WORKSPACE"
  while true; do
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
# Log File Creation - MISSING
# =============================================================================
# INTENTIONALLY BROKEN: Log file is not created
# This should be here:
# echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker started\"}" \
#   >> ~/.forge/logs/$SESSION_NAME.log

exit 0
