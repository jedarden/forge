"""
FORGE Launcher Script Templates

Provides templates for worker launcher scripts that spawn AI coding agents
in tmux sessions with proper logging and status file management.
"""

from pathlib import Path


# =============================================================================
# Launcher Script Templates
# =============================================================================


CLAUDE_CODE_LAUNCHER = """#!/bin/bash
# Claude Code Worker Launcher
# Usage: claude-code-launcher <model> <workspace> <session-name>

set -euo pipefail

MODEL="${1:-sonnet}"
WORKSPACE="${2:-$PWD}"
SESSION_NAME="${3:-claude-code-worker}"

# Create directories
mkdir -p ~/.forge/logs ~/.forge/status

# Map model names to Claude Code model IDs
case "$MODEL" in
    "sonnet") MODEL_ID="claude-sonnet-4.5" ;;
    "opus") MODEL_ID="claude-opus-4.6" ;;
    "haiku") MODEL_ID="claude-haiku-4.5" ;;
    *) MODEL_ID="$MODEL" ;;
esac

# Check if session already exists
if tmux has-session -t "$SESSION_NAME" 2>/dev/null; then
    echo "Error: Session '$SESSION_NAME' already exists"
    exit 1
fi

# Launch Claude Code in tmux with logging
tmux new-session -d -s "$SESSION_NAME" \\
    "cd \\"$WORKSPACE\\" && \\
     claude --model=$MODEL_ID \\
           --dangerously-skip-permissions \\
           --output-format stream-json \\
           2>&1 | tee ~/.forge/logs/$SESSION_NAME.log"

# Get PID
sleep 1
PID=$(tmux list-panes -t "$SESSION_NAME" -F '#{pane_pid}' 2>/dev/null || echo "unknown")

# Output metadata (JSON)
cat << EOF
{
  "worker_id": "$SESSION_NAME",
  "pid": $PID,
  "status": "spawned",
  "model": "$MODEL",
  "workspace": "$WORKSPACE"
}
EOF

# Create status file
cat > ~/.forge/status/$SESSION_NAME.json << EOF
{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "model_id": "$MODEL_ID",
  "workspace": "$WORKSPACE",
  "pid": $PID,
  "started_at": "$(date -Iseconds)",
  "last_activity": "$(date -Iseconds)"
}
EOF

echo "Worker launched in tmux session: $SESSION_NAME"
echo "  Attach: tmux attach -t $SESSION_NAME"
echo "  Logs: tail -f ~/.forge/logs/$SESSION_NAME.log"
"""


OPENCODE_LAUNCHER = """#!/bin/bash
# OpenCode Worker Launcher
# Usage: opencode-launcher <model> <workspace> <session-name>

set -euo pipefail

MODEL="${1:-default}"
WORKSPACE="${2:-$PWD}"
SESSION_NAME="${3:-opencode-worker}"

# Create directories
mkdir -p ~/.forge/logs ~/.forge/status

# Check if session already exists
if tmux has-session -t "$SESSION_NAME" 2>/dev/null; then
    echo "Error: Session '$SESSION_NAME' already exists"
    exit 1
fi

# Launch OpenCode in tmux with logging
tmux new-session -d -s "$SESSION_NAME" \\
    "cd \\"$WORKSPACE\\" && \\
     opencode --headless \\
           2>&1 | tee ~/.forge/logs/$SESSION_NAME.log"

# Get PID
sleep 1
PID=$(tmux list-panes -t "$SESSION_NAME" -F '#{pane_pid}' 2>/dev/null || echo "unknown")

# Output metadata (JSON)
cat << EOF
{
  "worker_id": "$SESSION_NAME",
  "pid": $PID,
  "status": "spawned",
  "model": "$MODEL",
  "workspace": "$WORKSPACE"
}
EOF

# Create status file
cat > ~/.forge/status/$SESSION_NAME.json << EOF
{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "workspace": "$WORKSPACE",
  "pid": $PID,
  "started_at": "$(date -Iseconds)",
  "last_activity": "$(date -Iseconds)"
}
EOF

echo "Worker launched in tmux session: $SESSION_NAME"
echo "  Attach: tmux attach -t $SESSION_NAME"
echo "  Logs: tail -f ~/.forge/logs/$SESSION_NAME.log"
"""


AIDER_LAUNCHER = """#!/bin/bash
# Aider Worker Launcher
# Usage: aider-launcher <model> <workspace> <session-name>

set -euo pipefail

MODEL="${1:-sonnet}"
WORKSPACE="${2:-$PWD}"
SESSION_NAME="${3:-aider-worker}"

# Create directories
mkdir -p ~/.forge/logs ~/.forge/status

# Map model names to Aider model IDs
case "$MODEL" in
    "sonnet") MODEL_ID="claude-sonnet-4.5" ;;
    "opus") MODEL_ID="claude-opus-4.6" ;;
    "gpt4") MODEL_ID="gpt-4" ;;
    "gpt-4-turbo") MODEL_ID="gpt-4-turbo" ;;
    *) MODEL_ID="$MODEL" ;;
esac

# Check if session already exists
if tmux has-session -t "$SESSION_NAME" 2>/dev/null; then
    echo "Error: Session '$SESSION_NAME' already exists"
    exit 1
fi

# Launch Aider in tmux with logging
tmux new-session -d -s "$SESSION_NAME" \\
    "cd \\"$WORKSPACE\\" && \\
     aider --model=$MODEL_ID \\
           --yes \\
           --no-pretty \\
           2>&1 | tee ~/.forge/logs/$SESSION_NAME.log"

# Get PID
sleep 1
PID=$(tmux list-panes -t "$SESSION_NAME" -F '#{pane_pid}' 2>/dev/null || echo "unknown")

# Output metadata (JSON)
cat << EOF
{
  "worker_id": "$SESSION_NAME",
  "pid": $PID,
  "status": "spawned",
  "model": "$MODEL",
  "workspace": "$WORKSPACE"
}
EOF

# Create status file
cat > ~/.forge/status/$SESSION_NAME.json << EOF
{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "model_id": "$MODEL_ID",
  "workspace": "$WORKSPACE",
  "pid": $PID,
  "started_at": "$(date -Iseconds)",
  "last_activity": "$(date -Iseconds)"
}
EOF

echo "Worker launched in tmux session: $SESSION_NAME"
echo "  Attach: tmux attach -t $SESSION_NAME"
echo "  Logs: tail -f ~/.forge/logs/$SESSION_NAME.log"
"""


# =============================================================================
# Template Installation
# =============================================================================


def get_launcher_template(cli_name: str) -> str:
    """Get launcher script template for a CLI tool.

    Args:
        cli_name: Name of CLI tool (claude-code, opencode, aider)

    Returns:
        Launcher script content as string
    """
    templates = {
        "claude-code": CLAUDE_CODE_LAUNCHER,
        "opencode": OPENCODE_LAUNCHER,
        "aider": AIDER_LAUNCHER,
    }

    return templates.get(cli_name, "")


def install_launcher_script(cli_name: str, launchers_dir: Path) -> Path:
    """Install launcher script for a CLI tool.

    Args:
        cli_name: Name of CLI tool
        launchers_dir: Directory to install launchers (e.g., ~/.forge/launchers)

    Returns:
        Path to installed launcher script

    Raises:
        ValueError: If CLI name is unknown
    """
    template = get_launcher_template(cli_name)

    if not template:
        raise ValueError(f"No launcher template for CLI: {cli_name}")

    # Create launchers directory
    launchers_dir.mkdir(parents=True, exist_ok=True)

    # Write launcher script
    launcher_path = launchers_dir / f"{cli_name}-launcher"
    launcher_path.write_text(template)

    # Make executable
    launcher_path.chmod(0o755)

    return launcher_path


def install_all_launchers(cli_names: list[str], launchers_dir: Path) -> list[Path]:
    """Install launcher scripts for multiple CLI tools.

    Args:
        cli_names: List of CLI tool names
        launchers_dir: Directory to install launchers

    Returns:
        List of paths to installed launcher scripts
    """
    installed = []

    for cli_name in cli_names:
        try:
            launcher_path = install_launcher_script(cli_name, launchers_dir)
            installed.append(launcher_path)
        except ValueError:
            # Skip unknown CLI tools
            pass

    return installed
