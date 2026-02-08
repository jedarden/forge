#!/bin/bash
# =============================================================================
# FORGE Launcher Protocol - FAILING Example: Missing Arguments
# =============================================================================
# This is a FAILING example that demonstrates what happens when required
# argument validation is missing or incorrect.
#
# EXPECTED FAILURE: Test 1 - Argument Parsing
# This launcher will fail because it doesn't validate required arguments.
#
# USE CASE:
# Use this to understand the argument parsing requirements and to test
# the test harness's validation logic.
#
# =============================================================================

set -e

# =============================================================================
# Argument Parsing - BROKEN
# =============================================================================
# This launcher doesn't parse or validate arguments properly
# It will fail Test 1 because it accepts missing arguments without error

MODEL=""
WORKSPACE=""
SESSION_NAME=""

# Intentionally broken: doesn't validate required arguments
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
      # Silently ignore unknown arguments instead of failing
      shift
      ;;
  esac
done

# MISSING: No validation of required arguments
# This should be here:
# if [[ -z "$MODEL" ]] || [[ -z "$WORKSPACE" ]] || [[ -z "$SESSION_NAME" ]]; then
#   echo "Error: Missing required arguments" >&2
#   exit 1
# fi

# =============================================================================
# Directory Setup
# =============================================================================
mkdir -p ~/.forge/logs ~/.forge/status

# =============================================================================
# Worker Spawning
# =============================================================================
# Use defaults if not provided (this is wrong - should fail)
SESSION_NAME="${SESSION_NAME:-default-worker}"

(
  cd "${WORKSPACE:-/tmp}"
  while true; do
    echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker active\"}" \
      >> ~/.forge/logs/$SESSION_NAME.log
    sleep 10
  done
) &

PID=$!

# =============================================================================
# Output Worker Metadata
# =============================================================================
cat << EOF
{
  "worker_id": "$SESSION_NAME",
  "pid": $PID,
  "status": "spawned"
}
EOF

exit 0
