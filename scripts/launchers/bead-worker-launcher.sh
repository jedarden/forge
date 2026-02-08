#!/bin/bash
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
#   1. Fetch bead data using `br show <bead-id>`
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
# - br CLI (beads_rust) for bead operations
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
BR_CMD="br"

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

  # Check if br is available
  if ! command -v "$BR_CMD" &> /dev/null; then
    echo "Warning: br CLI not found, cannot fetch bead data" >&2
    echo "Launching generic worker instead..." >&2
    BEAD_REF=""
  else
    # Fetch bead data (br show outputs a JSON array)
    BEAD_JSON=$(cd "$WORKSPACE" && "$BR_CMD" show "$BEAD_REF" --format json 2>/dev/null || echo "")

    if [[ -z "$BEAD_JSON" ]]; then
      echo "Warning: Failed to fetch bead $BEAD_REF" >&2
      echo "Launching generic worker instead..." >&2
      BEAD_REF=""
    else
      BEAD_DATA="$BEAD_JSON"

      # Parse bead data using jq if available
      # br show returns an array, so we need to access the first element
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

      # Update bead status to in_progress
      cd "$WORKSPACE" && "$BR_CMD" update "$BEAD_REF" --status in_progress 2>/dev/null || true

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

# Build the worker command
# In a real deployment, this would launch the actual AI tool
# For this reference implementation, we simulate the worker process
WORKER_CMD=$(cat <<'INNER_EOF'
  # Simulate a worker that processes the task
  echo "Worker started, processing task..."
  echo "Task: $BEAD_REF - $BEAD_TITLE"
  # In real implementation: $AI_CMD $AI_MODEL_ARG <bead-prompt>
INNER_EOF
)

# Launch in tmux session
if tmux has-session -t "$SESSION_NAME" 2>/dev/null; then
  echo "Warning: Session $SESSION_NAME already exists, killing it" >&2
  tmux kill-session -t "$SESSION_NAME" 2>/dev/null || true
  sleep 1
fi

# Create tmux session with the worker command
tmux new-session -d -s "$SESSION_NAME" "
  cd '$WORKSPACE'
  echo 'Starting worker for bead: ${BEAD_REF:-<none>}'
  echo 'Model: $MODEL'
  echo ''
  echo 'Task Prompt:'
  echo '$WORKER_PROMPT'
  echo ''
  echo 'Simulating worker process (press Ctrl+C to exit)...'
  # Simulate work
  sleep 300
"

# Get the PID of the tmux server (not the session, but close enough for monitoring)
TMUX_PID=$(pgrep -f "tmux.*$SESSION_NAME" | head -1 || echo "0")
if [[ "$TMUX_PID" == "0" ]]; then
  # Try getting the pane PID instead
  TMUX_PID=$(tmux list-panes -t "$SESSION_NAME" -F '#{pane_pid}' 2>/dev/null || echo "0")
fi

echo "Worker spawned (tmux PID: $TMUX_PID)" >&2

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

# Build status JSON
cat > "$STATUS_FILE" << EOF
{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "workspace": "$WORKSPACE",
  "pid": $TMUX_PID,
  "started_at": "$(date -Iseconds)",
  "last_activity": "$(date -Iseconds)",
  "current_task": {
EOF

if [[ -n "$BEAD_REF" ]]; then
  cat >> "$STATUS_FILE" << EOF
    "bead_id": "$BEAD_REF",
    "bead_title": "$(echo "$BEAD_TITLE" | sed 's/"/\\"/g')",
    "bead_priority": "$BEAD_PRIO"
EOF
else
  cat >> "$STATUS_FILE" << EOF
    "type": "generic",
    "description": "No specific bead assigned"
EOF
fi

cat >> "$STATUS_FILE" << EOF
  },
  "tasks_completed": 0,
  "metadata": {
EOF

if [[ -n "$BEAD_REF" ]]; then
  cat >> "$STATUS_FILE" << EOF
    "type": "bead_worker",
    "bead_id": "$BEAD_REF"
EOF
else
  cat >> "$STATUS_FILE" << EOF
    "type": "generic_worker"
EOF
fi

cat >> "$STATUS_FILE" << EOF
  }
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
