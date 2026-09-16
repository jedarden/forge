#!/usr/bin/env bash
# =============================================================================
# FORGE Bead-Aware Launcher Protocol Reference Implementation
# =============================================================================
# This is a reference implementation of a bead-aware launcher that extends the
# standard FORGE launcher protocol to support allocating workers to specific
# beads/tasks from the `bead` CLI issue tracker (bead-rs).
#
# BEAD-AWARE PROTOCOL EXTENSION:
# ------------------------------
# In addition to standard launcher arguments, this launcher supports:
#   --bead-ref=<bead-id>  - Bead ID from the bead CLI (e.g., "fg-1qo", "bd-abc")
#
# LAUNCHER RESPONSIBILITIES:
# --------------------------
# 1. Fetch bead data (bead show <bead-id> --json)
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
BEAD_CMD="${FORGE_BEAD_CLI:-bead}"

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
  # Verify bead CLI is available
  if ! command -v "$BEAD_CMD" >/dev/null 2>&1; then
    echo "Error: bead CLI not found (required for --bead-ref)" >&2
    exit 1
  fi

  # Fetch bead data from the bead CLI (must run from workspace directory)
  BEAD_DATA=$(cd "$WORKSPACE" && "$BEAD_CMD" show "$BEAD_REF" --json 2>/dev/null) || {
    echo "Error: Failed to fetch bead data for: $BEAD_REF" >&2
    echo "       Bead may not exist or bead CLI may not be configured" >&2
    exit 1
  }

  # Parse bead data using jq (bead show --json returns a one-element array,
  # so access the first element)
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

  # Update bead status to in_progress before spawning. A failed transition
  # must not leave a worker running without a tracked bead assignment.
  if ! (cd "$WORKSPACE" && "$BEAD_CMD" update "$BEAD_REF" --status in_progress --assignee "$SESSION_NAME" >/dev/null 2>&1); then
    echo "Error: Failed to update bead $BEAD_REF to in_progress" >&2
    exit 1
  fi
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

# On a clean worker exit the worker writes its exit code here; the bead
# completion monitor below reads it to decide close vs release.
BEAD_EXIT_MARKER=""

if [[ -n "$TASK_PROMPT" ]]; then
  # Bead-aware mode: Worker receives task via temp file
  TASK_FILE=$(mktemp)
  echo "$TASK_PROMPT" > "$TASK_FILE"

  # Exit-code marker for the completion monitor
  BEAD_EXIT_MARKER="$HOME/.forge/status/${SESSION_NAME}.exit"
  rm -f "$BEAD_EXIT_MARKER"

  # In a real implementation, this would launch the actual AI worker
  # Example for claude-code:
  # WORKER_CMD="claude-code --model=$MODEL < $TASK_FILE 2>&1 | tee ~/.forge/logs/$SESSION_NAME.log"

  # For this example, simulate a worker that processes the task and then
  # finishes (exit code lands in the marker for the completion monitor)
  WORKER_CMD="echo 'Processing task for bead $BEAD_REF'; cat $TASK_FILE; sleep 10; echo 0 > '$BEAD_EXIT_MARKER'; echo 'Task complete for bead $BEAD_REF'"

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
# Bead Completion Monitor (Detached Background)
# =============================================================================
# Protocol §4: a standalone bead-aware launcher applies completion transitions
# on FORGE's behalf, through the bead CLI (sole write authority — ADR 0020):
#   worker exit 0    -> bead close <id> --reason "Completed by <session>"
#   worker failure   -> bead release <id>  (back to open/unassigned)
#   monitor timeout  -> no transition; the bead stays in_progress and
#                       stale-assignment recovery owns it from there
# FORGE itself stays read-only over the store files at all times.
if [[ -n "$BEAD_REF" ]]; then
  BEAD_SESSION="$SESSION_NAME" \
  BEAD_CMD="$BEAD_CMD" \
  BEAD_ID="$BEAD_REF" \
  BEAD_WORKSPACE="$WORKSPACE" \
  BEAD_PID="$PID" \
  BEAD_EXIT_MARKER="$BEAD_EXIT_MARKER" \
  BEAD_LOG_FILE="$HOME/.forge/logs/$SESSION_NAME.log" \
  BEAD_MONITOR_TIMEOUT_SECS="${BEAD_MONITOR_TIMEOUT_SECS:-86400}" \
  nohup setsid bash -c '
    # The monitor is detached from the launcher and must finish its cleanup
    # path even when the parent shell exported errexit.
    set +e
    deadline=$(( $(date +%s) + BEAD_MONITOR_TIMEOUT_SECS ))
    timed_out=0
    while :; do
      if [ -f "$BEAD_EXIT_MARKER" ]; then
        break
      fi
      if ! kill -0 "$BEAD_PID" 2> /dev/null; then
        # Worker process ended; brief grace for a racing marker write
        sleep 2
        break
      fi
      if [ "$(date +%s)" -ge "$deadline" ]; then
        timed_out=1
        break
      fi
      sleep 2
    done

    exit_code=1
    if [ -f "$BEAD_EXIT_MARKER" ]; then
      exit_code="$(cat "$BEAD_EXIT_MARKER" 2> /dev/null || echo 1)"
    fi
    rm -f "$BEAD_EXIT_MARKER"

    log_event() {
      echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$BEAD_SESSION\", \"message\": \"$1\", \"event\": \"$2\", \"bead_id\": \"$BEAD_ID\", \"exit_code\": ${exit_code:-1}}" \
        >> "$BEAD_LOG_FILE" 2> /dev/null || true
    }

    # Transitions must run against the bead workspace store.
    cd "$BEAD_WORKSPACE" 2> /dev/null || exit 0

    if [ "$timed_out" = "1" ]; then
      log_event "Completion monitor timed out; bead left in_progress" "bead_monitor_timeout"
      exit 0
    fi

    if [ "$exit_code" = "0" ]; then
      close_args=(close "$BEAD_ID" --reason "Completed by $BEAD_SESSION")
      claim_epoch="$("$BEAD_CMD" show "$BEAD_ID" --json 2> /dev/null | jq -r ".[0].claim_epoch // empty" 2> /dev/null || true)"
      if [ -n "$claim_epoch" ]; then
        close_args+=(--fencing-token "$claim_epoch")
      fi
      if "$BEAD_CMD" "${close_args[@]}" > /dev/null 2>&1; then
        log_event "Bead closed after worker exit 0" "bead_closed"
      else
        log_event "Failed to close bead via bead CLI" "bead_close_failed"
      fi
    else
      release_args=(release "$BEAD_ID")
      claim_epoch="$("$BEAD_CMD" show "$BEAD_ID" --json 2> /dev/null | jq -r ".[0].claim_epoch // empty" 2> /dev/null || true)"
      if [ -n "$claim_epoch" ]; then
        release_args+=(--fencing-token "$claim_epoch")
      fi
      if "$BEAD_CMD" "${release_args[@]}" > /dev/null 2>&1; then
        log_event "Bead released for reallocation after worker exit" "bead_released"
      else
        log_event "Failed to release bead via bead CLI" "bead_release_failed"
      fi
    fi
  ' </dev/null > /dev/null 2>&1 &
  disown || true

  echo "Bead completion monitor armed for $BEAD_REF" >&2
fi

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
# Clean Exit
# =============================================================================
exit 0

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
# Bead Completion Handling
# =============================================================================
# Completion transitions are applied by the detached bead completion monitor
# armed during worker spawning above: it waits for the worker to end and then
# closes the bead (exit 0) or releases it (failure) through the bead CLI.
# Nothing further is needed here.

# =============================================================================
# Clean Exit
# =============================================================================
# Exit immediately after spawning (don't wait for the worker)
# This is crucial - the launcher must return control to FORGE quickly
exit 0
