#!/usr/bin/env bash
# Test search clear functionality

set -e

SESSION="forge-search-clear-test"

# Clean up any existing session
tmux kill-session -t "$SESSION" 2>/dev/null || true

echo "Creating tmux session for search clear test..."
tmux new-session -d -s "$SESSION" -x 80 -y 30

echo "Starting forge..."
tmux send-keys -t "$SESSION" "cd /home/coder/forge && ./target/release/forge --debug" C-m

echo "Waiting for forge to start..."
sleep 5

echo "Switching to Tasks view..."
tmux send-keys -t "$SESSION" "t" C-m
sleep 1

echo "Activating search mode with /..."
tmux send-keys -t "$SESSION" "/" C-m
sleep 1

echo "Typing search query: 'test'..."
tmux send-keys -t "$SESSION" "test" C-m
sleep 1

echo "Capturing pane with search active..."
tmux capture-pane -t "$SESSION" -p > /tmp/search-before-clear.txt

echo "Pressing Escape to clear search..."
tmux send-keys -t "$SESSION" "Escape" C-m
sleep 1

echo "Capturing pane after clearing search..."
tmux capture-pane -t "$SESSION" -p > /tmp/search-after-clear.txt

echo "Checking if search was cleared..."
if grep -i "Search" /tmp/search-before-clear.txt && ! grep -i "Search: \"test\"" /tmp/search-after-clear.txt; then
    echo "✓ Search cleared successfully"
else
    echo "✗ Search might not be cleared properly"
fi

echo "Comparing line counts..."
before_lines=$(wc -l < /tmp/search-before-clear.txt)
after_lines=$(wc -l < /tmp/search-after-clear.txt)
echo "Before clear: $before_lines lines"
echo "After clear: $after_lines lines"

echo "Cleaning up..."
tmux kill-session -t "$SESSION" 2>/dev/null || true

echo ""
echo "Test outputs saved to:"
echo "  - /tmp/search-before-clear.txt"
echo "  - /tmp/search-after-clear.txt"
