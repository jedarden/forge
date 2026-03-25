#!/bin/bash
# FORGE Marathon Launcher — GLM-5 via ZAI Proxy
# Runs claude in a loop, each iteration reads the marathon instruction
# and works through the FORGE codebase.
#
# Usage:
#   ./launch-marathon.sh             # starts session "forge-marathon"
#   ./launch-marathon.sh my-session  # starts named session

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(dirname "$SCRIPT_DIR")"
CONFIG_DIR="$SCRIPT_DIR/glm5-config"
INSTRUCTION_FILE="$SCRIPT_DIR/instruction.md"
SESSION_NAME="${1:-forge-marathon}"

if tmux has-session -t "$SESSION_NAME" 2>/dev/null; then
    echo "Session '$SESSION_NAME' already exists."
    echo "Attach with: tmux attach -t $SESSION_NAME"
    exit 0
fi

echo "Launching FORGE marathon session: $SESSION_NAME"
echo "  Repo:        $REPO_DIR"
echo "  Config:      $CONFIG_DIR"
echo "  Instruction: $INSTRUCTION_FILE"
echo "  Model:       glm-5 via ZAI proxy"
echo ""

# Build the loop command:
# - cd to repo
# - set config dir for GLM-5 proxy settings
# - loop: pass instruction to claude --print, sleep briefly, repeat
LOOP_CMD="cd '$REPO_DIR' && export CLAUDE_CONFIG_DIR='$CONFIG_DIR' && while true; do
  echo \"[forge-marathon] Starting iteration at \$(date)\"
  cat '$INSTRUCTION_FILE' | claude --dangerously-skip-permissions --output-format stream-json --verbose --print 2>&1
  EXIT_CODE=\$?
  echo \"[forge-marathon] Iteration complete (exit \$EXIT_CODE) at \$(date)\"
  if [ \$EXIT_CODE -ne 0 ]; then
    echo \"[forge-marathon] Non-zero exit, pausing 30s before retry\"
    sleep 30
  else
    sleep 5
  fi
done"

tmux new-session -d -s "$SESSION_NAME"
tmux send-keys -t "$SESSION_NAME" "$LOOP_CMD" Enter

echo "Marathon running in tmux session: $SESSION_NAME"
echo "  Attach:  tmux attach -t $SESSION_NAME"
echo "  Detach:  Ctrl+B, D"
echo "  Stop:    tmux kill-session -t $SESSION_NAME"
