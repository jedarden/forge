#!/bin/bash
# =============================================================================
# FORGE Bead-Aware Launcher Protocol Reference Implementation
# =============================================================================
# This is a reference implementation of a bead-aware launcher that extends the
# standard FORGE launcher protocol to support allocating workers to specific
# beads/tasks from the br CLI issue tracker.
#
# BEAD-AWARE PROTOCOL EXTENSION:
# ------------------------------
# In addition to standard launcher arguments, this launcher supports:
#   --bead-ref=<bead-id>  - Bead ID from br CLI (e.g., "fg-1qo", "bd-abc")
#
# LAUNCHER RESPONSIBILITIES:
# --------------------------
# 1. Fetch bead data (br show <bead-id>)
# 2. Construct prompt with bead context
# 3. Launch worker with injected task
# 4. Update bead status on completion
# 5. Include bead_ref in status file and output
#
# COMPATIBILITY:
# --------------
# This launcher remains compatible with the standard protocol:
# - When --bead-ref is absent: operates in standard mode (no task assigned)
# - Workers remain bead-agnostic (receive task via prompt/stdin)
#
# TESTING:
# --------
# Test with bead:
#   ./bead-worker-launcher.sh --model=sonnet --workspace=/home/coder/forge \
#     --session-name=test --bead-ref=fg-1qo
#
# Test without bead (standard mode):
#   ./bead-worker-launcher.sh --model=sonnet --workspace=/home/coder/forge \
#     --session-name=test
#
# =============================================================================

set -e  # Exit on error

# =============================================================================
# Argument Parsing
# =============================================================================
# Parse command-line arguments using a while loop
# This pattern handles all required and optional arguments including --bead-ref
MODEL=""
WORKSPACE=""
SESSION_NAME=""
CONFIG=""
BEAD_REF=""

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
    --bead-ref=*)
      BEAD_REF="${1#*=}"
      shift
      ;;
    *)
      echo "Error: Unknown argument: $1" >&2
      echo "Usage: $0 --model=<model> --workspace=<path> --session-name=<name> [--bead-ref=<bead-id>] [--config=<path>]" >&2
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
# Bead-Aware Mode Setup
# =============================================================================
# If --bead-ref is provided, fetch bead data and construct prompt
TASK_PROMPT=""
BEAD_TITLE=""
BEAD_PRIORITY=""
BEAD_STATUS=""

if [[ -n "$BEAD_REF" ]]; then
  # Verify br CLI is available
  if ! command -v br >/dev/null 2>&1; then
    echo "Error: br CLI not found (required for --bead-ref)" >&2
    exit 1
  fi

  # Fetch bead data from br CLI (must run from workspace directory)
  BEAD_DATA=$(cd "$WORKSPACE" && br show "$BEAD_REF" --format json 2>/dev/null) || {
    echo "Error: Failed to fetch bead data for: $BEAD_REF" >&2
    echo "       Bead may not exist or br CLI may not be configured" >&2
    exit 1
  }

  # Parse bead data using jq (br show returns an array, get first element)
  BEAD_TITLE=$(echo "$BEAD_DATA" | jq -r '.[0].title // empty')
  BEAD_STATUS=$(echo "$BEAD_DATA" | jq -r '.[0].status // empty')
  BEAD_PRIORITY=$(echo "$BEAD_DATA" | jq -r '.[0].priority // 2')
  BEAD_TYPE=$(echo "$BEAD_DATA" | jq -r '.[0].issue_type // "task"')
  BEAD_DESCRIPTION=$(echo "$BEAD_DATA" | jq -r '.[0].description // ""')
  BEAD_LABELS=$(echo "$BEAD_DATA" | jq -r '.[0].labels // [] | join(", ")')

  # Validate bead status
  if [[ "$BEAD_STATUS" == "closed" ]]; then
    echo "Error: Bead $BEAD_REF is already closed" >&2
    exit 1
  fi

  # Map priority to label
  case "$BEAD_PRIORITY" in
    0) PRIORITY_LABEL="Critical" ;;
    1) PRIORITY_LABEL="High" ;;
    2) PRIORITY_LABEL="Normal" ;;
    3) PRIORITY_LABEL="Low" ;;
    4) PRIORITY_LABEL="Backlog" ;;
    *) PRIORITY_LABEL="Unknown" ;;
  esac

  # Construct task prompt with bead context
  TASK_PROMPT=$(cat <<EOF
You are working on bead $BEAD_REF: $BEAD_TITLE

Priority: P$BEAD_PRIORITY ($PRIORITY_LABEL)
Type: $BEAD_TYPE
Labels: $BEAD_LABELS

Description:
$BEAD_DESCRIPTION

Workspace: $WORKSPACE

Please work on this task. When complete, commit your changes to git with an appropriate commit message.
EOF
)

  # Update bead status to in_progress
  cd "$WORKSPACE" && br update "$BEAD_REF" --status in_progress --assignee "$SESSION_NAME" >/dev/null 2>&1 || {
    echo "Warning: Failed to update bead status to in_progress" >&2
  }
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
# In a real launcher, this would be: tmux, docker run, claude-code, etc.
# For this example, we simulate a worker with a sleep loop
#
# NOTE: Using setsid to create new session and avoid the subprocess
# capture_output issue when running under test harness.
WORKER_CMD=""

if [[ -n "$TASK_PROMPT" ]]; then
  # Bead-aware mode: Worker receives task via temp file
  TASK_FILE=$(mktemp)
  echo "$TASK_PROMPT" > "$TASK_FILE"

  # In a real implementation, this would launch the actual AI worker
  # Example for claude-code:
  # WORKER_CMD="claude-code --model=$MODEL < $TASK_FILE 2>&1 | tee ~/.forge/logs/$SESSION_NAME.log"

  # For this example, simulate a worker that processes the task
  WORKER_CMD="echo 'Processing task for bead $BEAD_REF'; cat $TASK_FILE; while true; do echo '{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker active on bead $BEAD_REF\"}' >> ~/.forge/logs/$SESSION_NAME.log; sleep 10; done"

  # Clean up temp file in background
  (sleep 1; rm -f "$TASK_FILE") &
else
  # Standard mode: No specific task assigned
  WORKER_CMD="while true; do echo '{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker active\"}' >> ~/.forge/logs/$SESSION_NAME.log; sleep 10; done"
fi

# Launch the worker in a subshell with setsid
(
  cd "$WORKSPACE"
  eval "$WORKER_CMD"
) >/dev/null 2>&1 &

# Capture the PID of the backgrounded process
PID=$!

# =============================================================================
# Output Worker Metadata (stdout - JSON ONLY)
# =============================================================================
# This is the CRITICAL output that FORGE parses
# Must be valid JSON with required fields: worker_id, pid, status
# NOTE: No extra output before or after this JSON block
#
# Bead-aware extension: Includes bead_ref field when applicable

if [[ -n "$BEAD_REF" ]]; then
  cat << EOF
{
  "worker_id": "$SESSION_NAME",
  "pid": $PID,
  "status": "spawned",
  "launcher": "bead-worker-launcher",
  "bead_ref": "$BEAD_REF",
  "timestamp": "$(date -Iseconds)"
}
EOF
else
  cat << EOF
{
  "worker_id": "$SESSION_NAME",
  "pid": $PID,
  "status": "spawned",
  "launcher": "bead-worker-launcher",
  "timestamp": "$(date -Iseconds)"
}
EOF
fi

# =============================================================================
# Status File Creation
# =============================================================================
# Create the status file that FORGE monitors for worker state
# This file MUST exist within 5 seconds of launcher execution
#
# Bead-aware extension: Includes current_task with bead information

if [[ -n "$BEAD_REF" ]]; then
  cat > ~/.forge/status/$SESSION_NAME.json << EOF
{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "workspace": "$WORKSPACE",
  "pid": $PID,
  "started_at": "$(date -Iseconds)",
  "last_activity": "$(date -Iseconds)",
  "current_task": "$BEAD_REF",
  "tasks_completed": 0
}
EOF
else
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
fi

# =============================================================================
# Log File Creation
# =============================================================================
# Write initial log entry to show the worker started
# Use JSON Lines (JSONL) format for structured logging

if [[ -n "$BEAD_REF" ]]; then
  echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker started\", \"event\": \"worker_started\", \"model\": \"$MODEL\", \"bead_id\": \"$BEAD_REF\", \"bead_title\": \"$BEAD_TITLE\"}" \
    >> ~/.forge/logs/$SESSION_NAME.log
else
  echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker started\", \"event\": \"worker_started\", \"model\": \"$MODEL\"}" \
    >> ~/.forge/logs/$SESSION_NAME.log
fi

# =============================================================================
# Bead Completion Handler (Background)
# =============================================================================
# In a production launcher, this would monitor the worker and update bead
# status when the worker completes. For this example, we just log the intent.

if [[ -n "$BEAD_REF" ]]; then
  # In a real implementation, you would:
  # 1. Monitor the worker process
  # 2. Wait for completion or detect failure
  # 3. Close the bead with appropriate reason
  #
  # Example:
  #   wait $PID
  #   if [[ $? -eq 0 ]]; then
  #     br close "$BEAD_REF" --reason "Completed by $SESSION_NAME"
  #   else
  #     br update "$BEAD_REF" --status open --assignee ""  # Release assignment
  #   fi

  # For this example, just log that we would handle completion
  echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"debug\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Bead completion handler registered for $BEAD_REF\", \"event\": \"bead_handler_registered\"}" \
    >> ~/.forge/logs/$SESSION_NAME.log
fi

# =============================================================================
# Clean Exit
# =============================================================================
# Exit immediately after spawning (don't wait for the worker)
# This is crucial - the launcher must return control to FORGE quickly
exit 0
