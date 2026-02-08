#!/bin/bash
# =============================================================================
# FORGE Launcher Protocol - FAILING Example: Invalid JSON Output
# =============================================================================
# This is a FAILING example that demonstrates what happens when the
# stdout output is not valid JSON.
#
# EXPECTED FAILURE: Test 2 - Output Format
# This launcher will fail because it outputs invalid JSON on stdout.
#
# USE CASE:
# Use this to understand the JSON output format requirements.
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
# This output is INVALID JSON:
# - Missing closing brace
# - Unquoted values
# - Trailing comma
# - Extra text before JSON

echo "Spawning worker..."  # WRONG: Extra output before JSON

cat << EOF
{
  "worker_id": "$SESSION_NAME",
  "pid": $PID,
  "status": "spawned",    # WRONG: Trailing comma
  "timestamp": "$(date -Iseconds)"
  # WRONG: Missing closing brace, unquoted comment in JSON
EOF

echo "Worker spawned!"  # WRONG: Extra output after JSON

exit 0
