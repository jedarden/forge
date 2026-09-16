#!/usr/bin/env bash
# =============================================================================
# FORGE Bead-Aware Worker Launcher
# =============================================================================
# Reference implementation for launching workers with bead context.
#
# LAUNCHER PROTOCOL (Bead-Aware):
# --------------------------------
# This launcher implements the bead-aware launcher protocol from ADR 0015.
#
# Standard Arguments:
#   --model=<model>         - Model identifier (e.g., "sonnet", "opus", "haiku")
#   --workspace=<path>      - Path to the workspace directory (must exist)
#   --session-name=<name>   - Unique session name for the worker
#   --config=<path>         - Optional: Path to worker configuration
#
# Bead-Aware Argument (Optional):
#   --bead-ref=<bead-id>    - Bead ID to work on (e.g., "fg-1qo")
#
# BEAD-AWARE BEHAVIOR:
# -------------------
# When --bead-ref is provided:
#   1. Fetch bead data using `bead show <bead-id> --json`
#   2. Construct prompt with bead context
#   3. Update bead status to "in_progress"
#   4. Launch worker with bead-specific prompt
#   5. Update bead status on completion (0=success, 1+=failure)
#
# When --bead-ref is NOT provided:
#   Launches a generic worker without bead context.
#
# OUTPUT:
# -------
# JSON on stdout with worker metadata including bead_id if applicable.
#
# DEPENDENCIES:
# -------------
# - bead CLI (bead-rs) for bead operations
# - tmux for session management
# - claude-code or other AI tool (configurable)
#
# =============================================================================

set -euo pipefail

# =============================================================================
# Configuration
# =============================================================================
DEFAULT_MODEL="sonnet"
DEFAULT_AI_TOOL="claude-code"
BEAD_CMD="${FORGE_BEAD_CLI:-bead}"

# =============================================================================
# Argument Parsing
# =============================================================================
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
      echo "Usage: $0 --model=<model> --workspace=<path> --session-name=<name> [--config=<path>] [--bead-ref=<bead-id>]" >&2
      exit 1
      ;;
  esac
done

# =============================================================================
# Validation
# =============================================================================
if [[ -z "$MODEL" ]]; then
  MODEL="$DEFAULT_MODEL"
fi

if [[ -z "$WORKSPACE" ]]; then
  echo "Error: Missing required argument: --workspace" >&2
  exit 1
fi

if [[ -z "$SESSION_NAME" ]]; then
  echo "Error: Missing required argument: --session-name" >&2
  exit 1
fi

if [[ ! -d "$WORKSPACE" ]]; then
  echo "Error: Workspace directory does not exist: $WORKSPACE" >&2
  exit 1
fi

# Create required directories
mkdir -p ~/.forge/logs ~/.forge/status

# =============================================================================
# Bead Fetching (if --bead-ref provided)
# =============================================================================
BEAD_DATA=""
BEAD_TITLE=""
BEAD_DESC=""
BEAD_PRIO=""
BEAD_TYPE=""
BEAD_LABELS=""

if [[ -n "$BEAD_REF" ]]; then
  echo "Fetching bead data for $BEAD_REF..." >&2

  # Check if bead is available
  if ! command -v "$BEAD_CMD" &> /dev/null; then
    echo "Error: bead CLI not found (required for --bead-ref)" >&2
    exit 1
  else
    # Fetch bead data (bead show --json outputs a one-element JSON array)
    if ! BEAD_JSON=$(cd "$WORKSPACE" && "$BEAD_CMD" show "$BEAD_REF" --json 2>/dev/null); then
      echo "Error: Failed to fetch bead $BEAD_REF" >&2
      echo "       Bead may not exist or bead CLI may not be configured" >&2
      exit 1
    fi

    if [[ -z "$BEAD_JSON" ]]; then
      echo "Error: bead show returned no data for $BEAD_REF" >&2
      exit 1
    else
      BEAD_DATA="$BEAD_JSON"

      # Parse bead data using jq if available
      # bead show --json returns a one-element array, so we access the first element
      if command -v jq &> /dev/null; then
        BEAD_TITLE=$(echo "$BEAD_DATA" | jq -r '.[0].title // empty')
        BEAD_DESC=$(echo "$BEAD_DATA" | jq -r '.[0].description // empty')
        BEAD_PRIO=$(echo "$BEAD_DATA" | jq -r '.[0].priority // 2')
        BEAD_TYPE=$(echo "$BEAD_DATA" | jq -r '.[0].issue_type // "task"')
        BEAD_LABELS=$(echo "$BEAD_DATA" | jq -r '.[0].labels // []' | tr -d '[],"')
      else
        # Fallback parsing without jq - handle array format
        BEAD_TITLE=$(echo "$BEAD_DATA" | grep -o '"title":"[^"]*"' | head -1 | cut -d'"' -f4 || echo "")
        BEAD_DESC=$(echo "$BEAD_DATA" | grep -o '"description":"[^"]*"' | head -1 | cut -d'"' -f4 | sed 's/\\n/\n/g' || echo "")
        BEAD_PRIO=$(echo "$BEAD_DATA" | grep -o '"priority":[0-9]*' | head -1 | cut -d':' -f2 || echo "2")
        BEAD_TYPE=$(echo "$BEAD_DATA" | grep -o '"issue_type":"[^"]*"' | head -1 | cut -d'"' -f4 || echo "task")
        BEAD_LABELS=$(echo "$BEAD_DATA" | grep -o '"labels":\[[^]]*\]' | head -1 | sed 's/"//g' | tr -d '[],' || echo "")
      fi

      # Update bead status to in_progress (protocol §4: assignee is part of
      # the transition — it is what duplicate-claim prevention and stale
      # assignment recovery key on). Stdout is dropped: the bead CLI prints
      # the bead id on success and stdout must stay JSON-only for FORGE.
      if ! (cd "$WORKSPACE" && "$BEAD_CMD" update "$BEAD_REF" --status in_progress --assignee "$SESSION_NAME" >/dev/null 2>&1); then
        echo "Error: Failed to update bead $BEAD_REF to in_progress" >&2
        exit 1
      fi

      echo "Bead loaded: $BEAD_TITLE" >&2
    fi
  fi
fi

# =============================================================================
# Prompt Construction
# =============================================================================
WORKER_PROMPT=""

if [[ -n "$BEAD_REF" ]]; then
  # Construct bead-specific prompt
  WORKER_PROMPT=$(cat <<EOF
# Task: $BEAD_REF: $BEAD_TITLE

## Description
$BEAD_DESC

## Context
- Priority: P$BEAD_PRIO
- Type: $BEAD_TYPE
- Workspace: $WORKSPACE
- Labels: $BEAD_LABELS

## Instructions
You are working on bead $BEAD_REF. Follow the task description above.

When you have completed the task:
1. Ensure all requirements are met
2. Commit your changes with clear commit messages
3. Run any applicable tests
4. Exit with code 0 to mark the bead as complete

If you encounter a blocker:
1. Create a new bead for the blocker
2. Add a dependency from current bead to blocker
3. Exit with code 1 to indicate incomplete status

If you need human input:
1. Create a human bead with detailed context
2. Add a dependency from current bead to human bead
3. Exit with code 1 to indicate waiting for human

Current bead ID: $BEAD_REF
EOF
)
else
  # Generic worker prompt
  WORKER_PROMPT="You are a generic AI coding worker. Assist with tasks as requested."
fi

# =============================================================================
# Worker Spawning
# =============================================================================
echo "Spawning worker in tmux session: $SESSION_NAME" >&2

# Determine AI tool command based on model
case "$MODEL" in
  sonnet|opus|haiku)
    AI_CMD="claude-code"
    AI_MODEL_ARG="--model=$MODEL"
    ;;
  gpt-4|gpt-3.5)
    AI_CMD="aider"
    AI_MODEL_ARG="--model=$MODEL"
    ;;
  *)
    AI_CMD="$DEFAULT_AI_TOOL"
    AI_MODEL_ARG="--model=$MODEL"
    ;;
esac

# Materialize the task prompt as a temp file (protocol §3: "For tmux
# sessions: write to a temp file and pass as argument"). Interpolating the
# prompt into the pane command breaks on any description containing a quote;
# a file path is injection-proof.
PROMPT_FILE=$(mktemp "/tmp/forge-prompt-${SESSION_NAME}.XXXXXX")
printf '%s\n' "$WORKER_PROMPT" > "$PROMPT_FILE"

# On a clean worker exit the pane writes its exit code here; the completion
# monitor below reads it to decide close vs release (empty in standard mode).
BEAD_EXIT_MARKER=""
WORKER_FINALIZE=""

if [[ -n "$BEAD_REF" ]]; then
  BEAD_EXIT_MARKER="$HOME/.forge/status/${SESSION_NAME}.exit"
  rm -f "$BEAD_EXIT_MARKER"
  WORKER_FINALIZE="echo 0 > '$BEAD_EXIT_MARKER'"
fi

# Launch in tmux session
if tmux has-session -t "$SESSION_NAME" 2>/dev/null; then
  echo "Warning: Session $SESSION_NAME already exists, killing it" >&2
  tmux kill-session -t "$SESSION_NAME" 2>/dev/null || true
  sleep 1
fi

# Create tmux session with the worker command.
# In a real deployment, the pane would run the actual AI tool fed by the
# prompt file; this reference implementation simulates the worker process.
tmux new-session -d -s "$SESSION_NAME" "
  cd '$WORKSPACE'
  echo 'Starting worker for bead: ${BEAD_REF:-<none>}'
  echo 'Model: $MODEL'
  echo ''
  echo 'Task Prompt:'
  cat '$PROMPT_FILE'
  rm -f '$PROMPT_FILE'
  echo ''
  echo 'Simulating worker process (press Ctrl+C to exit)...'
  # Simulate work
  sleep 300
  $WORKER_FINALIZE
"

# Get the PID of the tmux server (not the session, but close enough for monitoring)
TMUX_PID=$(pgrep -f "tmux.*$SESSION_NAME" | head -1 || echo "0")
if [[ "$TMUX_PID" == "0" ]]; then
  # Try getting the pane PID instead
  TMUX_PID=$(tmux list-panes -t "$SESSION_NAME" -F '#{pane_pid}' 2>/dev/null || echo "0")
fi

echo "Worker spawned (tmux PID: $TMUX_PID)" >&2

# =============================================================================
# Bead Completion Monitor (Detached Background)
# =============================================================================
# Protocol §4: a standalone bead-aware launcher applies completion transitions
# on FORGE's behalf, through the bead CLI (sole write authority — ADR 0020):
#   worker exit 0    -> bead close <id> --reason "Completed by <session>"
#   worker failure   -> bead release <id>  (back to open/unassigned)
#   monitor timeout  -> no transition; the bead stays in_progress and
#                       stale-assignment recovery owns it from there
# The monitor survives this launcher (setsid + disown) and never touches
# .beads/ directly — every transition is a CLI invocation from the workspace.
if [[ -n "$BEAD_REF" ]]; then
  BEAD_SESSION="$SESSION_NAME" \
  BEAD_CMD="$BEAD_CMD" \
  BEAD_ID="$BEAD_REF" \
  BEAD_WORKSPACE="$WORKSPACE" \
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
      if ! tmux has-session -t "$BEAD_SESSION" > /dev/null 2>&1; then
        # Session ended; brief grace for a racing marker write
        sleep 2
        break
      fi
      if [ "$(date +%s)" -ge "$deadline" ]; then
        timed_out=1
        break
      fi
      sleep 5
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

    # Transitions must run against the bead workspace store
    cd "$BEAD_WORKSPACE" 2> /dev/null || exit 0

    if [ "$timed_out" = "1" ]; then
      log_event "Completion monitor timed out; bead left in_progress" "bead_monitor_timeout"
      exit 0
    fi

    if [ "$exit_code" = "0" ]; then
      # A claimed bead (our in_progress transition) requires the claim-epoch
      # fencing token on close; read it fresh to minimize the conflict window.
      # An empty epoch means the bead was never claimed — close without one.
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

  echo "Bead completion monitor armed for $BEAD_REF (timeout: ${BEAD_MONITOR_TIMEOUT_SECS:-86400}s)" >&2
fi

# =============================================================================
# Output Worker Metadata (stdout - JSON ONLY)
# =============================================================================

# Add bead fields if we have a bead
if [[ -n "$BEAD_REF" ]]; then
  cat << EOF
{
  "worker_id": "$SESSION_NAME",
  "pid": $TMUX_PID,
  "status": "spawned",
  "model": "$MODEL",
  "session": "$SESSION_NAME",
  "timestamp": "$(date -Iseconds)",
  "bead_id": "$BEAD_REF",
  "bead_title": "$(echo "$BEAD_TITLE" | sed 's/"/\\"/g')"
}
EOF
else
  cat << EOF
{
  "worker_id": "$SESSION_NAME",
  "pid": $TMUX_PID,
  "status": "spawned",
  "model": "$MODEL",
  "session": "$SESSION_NAME",
  "timestamp": "$(date -Iseconds)"
}
EOF
fi

# =============================================================================
# Status File Creation
# =============================================================================
STATUS_FILE=~/.forge/status/$SESSION_NAME.json

# Determine current_task value (string format per ADR 0005)
CURRENT_TASK_VALUE=""
if [[ -n "$BEAD_REF" ]]; then
  CURRENT_TASK_VALUE="$BEAD_REF"
else
  CURRENT_TASK_VALUE=""
fi

# Build status JSON (per ADR 0005 specification)
cat > "$STATUS_FILE" << EOF
{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "workspace": "$WORKSPACE",
  "pid": $TMUX_PID,
  "started_at": "$(date -Iseconds)",
  "last_activity": "$(date -Iseconds)",
  "uptime_seconds": 0,
  "current_task": "${CURRENT_TASK_VALUE}",
  "tasks_completed": 0
}
EOF

# =============================================================================
# Log File Creation
# =============================================================================
LOG_FILE=~/.forge/logs/$SESSION_NAME.log

# Write initial log entry
echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker spawned\", \"event\": \"worker_started\", \"model\": \"$MODEL\", \"bead_id\": \"$BEAD_REF\"}" >> "$LOG_FILE"

if [[ -n "$BEAD_REF" ]]; then
  echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Assigned to bead $BEAD_REF: $BEAD_TITLE\", \"event\": \"bead_assigned\", \"bead_id\": \"$BEAD_REF\", \"bead_title\": \"$BEAD_TITLE\"}" >> "$LOG_FILE"
fi

# =============================================================================
# Clean Exit
# =============================================================================
exit 0
