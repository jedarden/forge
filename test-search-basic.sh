#!/usr/bin/env bash
# Test basic search functionality

set -e

SESSION="forge-search-test"

# Clean up any existing session
tmux kill-session -t "$SESSION" 2>/dev/null || true

echo "Creating tmux session for search test..."
tmux new-session -d -s "$SESSION" -x 120 -y 40

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

echo "Typing search query: 'chat'..."
tmux send-keys -t "$SESSION" "chat" C-m
sleep 1

echo "Capturing pane output..."
tmux capture-pane -t "$SESSION" -p > /tmp/search-basic.txt

echo "Checking for search indicator in output..."
if grep -i "Search" /tmp/search-basic.txt; then
    echo "✓ Search mode indicator found"
else
    echo "✗ Search mode indicator NOT found"
fi

echo "Checking if search term appears in title..."
if grep -i "chat" /tmp/search-basic.txt; then
    echo "✓ Search term appears in output"
else
    echo "✗ Search term NOT found in output"
fi

echo "Cleaning up..."
tmux kill-session -t "$SESSION" 2>/dev/null || true

echo ""
echo "Test output saved to /tmp/search-basic.txt"
echo "Review it with: cat /tmp/search-basic.txt"
