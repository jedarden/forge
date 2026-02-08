#!/bin/bash
# =============================================================================
# FORGE Launcher Protocol - FAILING Example: No Process Spawned
# =============================================================================
# This is a FAILING example that demonstrates what happens when no
# actual process is spawned (the PID doesn't correspond to a running process).
#
# EXPECTED FAILURE: Test 5 - Process Spawning
# This launcher will fail because the PID in the output doesn't correspond
# to a running process.
#
# The protocol requires a real running process or tmux session.
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
# Worker Spawning - BROKEN
# =============================================================================
# INTENTIONALLY BROKEN: Process exits immediately
# The subshell terminates, so the PID will be invalid
# Fix: Add output redirection to avoid subprocess timeout
(
  cd "$WORKSPACE"
  # This process exits immediately (no while loop)
  echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Starting\"}" \
    >> ~/.forge/logs/$SESSION_NAME.log
  # Process exits here
) >/dev/null 2>&1 &

PID=$!

# Wait a moment for the process to exit
sleep 0.1

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
# Log File Creation
# =============================================================================
echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker started\", \"event\": \"worker_started\", \"model\": \"$MODEL\"}" \
  >> ~/.forge/logs/$SESSION_NAME.log

exit 0
