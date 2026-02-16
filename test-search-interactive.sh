#!/usr/bin/env bash
# Interactive test for search functionality

set -e

SESSION="forge-search-interactive"

# Clean up any existing session
tmux kill-session -t "$SESSION" 2>/dev/null || true

echo "Creating tmux session for interactive search test..."
tmux new-session -d -s "$SESSION" -x 120 -y 40

echo "Starting forge (without debug logs)..."
tmux send-keys -t "$SESSION" "cd /home/coder/forge && ./target/release/forge" C-m

echo "Waiting for forge to start and load data..."
sleep 8

echo "Switching to Tasks view..."
tmux send-keys -t "$SESSION" "t"
sleep 2

echo "Activating search mode with /..."
tmux send-keys -t "$SESSION" "/"
sleep 1

echo "Typing search query: 'fg'..."
tmux send-keys -t "$SESSION" "fg"
sleep 2

echo "Capturing pane output..."
tmux capture-pane -t "$SESSION" -p > /tmp/search-interactive.txt

echo ""
echo "========================================="
echo "Search Test Results:"
echo "========================================="
echo ""

# Check for search indicator
if grep -q "Search:" /tmp/search-interactive.txt; then
    echo "✓ Search indicator found in UI"
else
    echo "✗ Search indicator NOT found"
fi

# Check for filtered content
if grep -q "Filtered:" /tmp/search-interactive.txt; then
    echo "✓ Filter indicator found"
else
    echo "Note: No filter indicator (may be expected if no tasks match)"
fi

# Check task view is active
if grep -q "Task Queue" /tmp/search-interactive.txt; then
    echo "✓ Task Queue view is active"
else
    echo "✗ Task Queue view NOT found"
fi

echo ""
echo "Full output saved to /tmp/search-interactive.txt"
echo ""
echo "To manually inspect the session, run:"
echo "  tmux attach -t $SESSION"
echo ""
echo "To kill the session, run:"
echo "  tmux kill-session -t $SESSION"
echo ""
echo "First 30 lines of output:"
echo "----------------------------------------"
head -30 /tmp/search-interactive.txt
