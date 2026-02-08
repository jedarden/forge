#!/bin/bash
# =============================================================================
# FORGE Launcher Protocol - FAILING Example: Missing Status File
# =============================================================================
# This is a FAILING example that demonstrates what happens when the
# status file is not created.
#
# EXPECTED FAILURE: Test 3 - Status File Creation
# This launcher will fail because it doesn't create the status file.
#
# The protocol requires ~/.forge/status/<worker-id>.json to be created
# within 5 seconds of launcher execution.
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
# Status File Creation - MISSING
# =============================================================================
# INTENTIONALLY BROKEN: Status file is not created
# This should be here:
# cat > ~/.forge/status/$SESSION_NAME.json << EOF
# {
#   "worker_id": "$SESSION_NAME",
#   "status": "active",
#   ...
# }
# EOF

# =============================================================================
# Log File Creation
# =============================================================================
echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker started\", \"event\": \"worker_started\", \"model\": \"$MODEL\"}" \
  >> ~/.forge/logs/$SESSION_NAME.log

exit 0
