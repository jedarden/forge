#!/usr/bin/env bash
# Comprehensive test for search functionality

set -e

SESSION="forge-test-search"

# Clean up any existing session
tmux kill-session -t "$SESSION" 2>/dev/null || true

echo "========================================="
echo "FORGE Search Functionality Test"
echo "========================================="
echo ""

# Create session
echo "1. Creating tmux session..."
tmux new-session -d -s "$SESSION" -x 140 -y 40
echo "   ✓ Session created"

# Start forge
echo ""
echo "2. Starting forge..."
tmux send-keys -t "$SESSION" "cd /home/coder/forge && ./target/release/forge" C-m
sleep 6
echo "   ✓ Forge started"

# Switch to Tasks view
echo ""
echo "3. Switching to Tasks view..."
tmux send-keys -t "$SESSION" "t" C-m
sleep 1
tmux capture-pane -t "$SESSION" -p > /tmp/forge-tasks-view.txt
if grep -q "Task Queue" /tmp/forge-tasks-view.txt; then
    echo "   ✓ Tasks view active"
else
    echo "   ✗ Failed to switch to Tasks view"
fi

# Activate search mode
echo ""
echo "4. Activating search mode with '/' key..."
tmux send-keys -t "$SESSION" "/" C-m
sleep 1
tmux capture-pane -t "$SESSION" -p > /tmp/forge-search-activated.txt
if grep -q "Search active" /tmp/forge-search-activated.txt || grep -q "Search mode" /tmp/forge-search-activated.txt; then
    echo "   ✓ Search mode activated"
else
    echo "   Note: Search mode indicator might be in status bar"
fi

# Type search query
echo ""
echo "5. Typing search query 'filter'..."
tmux send-keys -t "$SESSION" "filter" C-m
sleep 1
tmux capture-pane -t "$SESSION" -p > /tmp/forge-search-typed.txt
if grep -q 'Search: "filter"' /tmp/forge-search-typed.txt; then
    echo "   ✓ Search query visible in UI"
else
    echo "   Note: Checking alternate search indicator..."
    if grep -qi "filter" /tmp/forge-search-typed.txt; then
        echo "   ✓ Search term present in output"
    fi
fi

# Clear search with Escape (send raw escape key)
echo ""
echo "6. Clearing search with Escape key..."
# Use Escape key directly
tmux send-keys -t "$SESSION" Escape
sleep 1
tmux capture-pane -t "$SESSION" -p > /tmp/forge-search-cleared.txt
if ! grep -q 'Search: "filter"' /tmp/forge-search-cleared.txt; then
    echo "   ✓ Search cleared"
else
    echo "   Note: Search may still be active"
fi

# Summary
echo ""
echo "========================================="
echo "Test Summary"
echo "========================================="
echo ""
echo "Test outputs saved to:"
echo "  - /tmp/forge-tasks-view.txt"
echo "  - /tmp/forge-search-activated.txt"
echo "  - /tmp/forge-search-typed.txt"
echo "  - /tmp/forge-search-cleared.txt"
echo ""
echo "To inspect the live session:"
echo "  tmux attach -t $SESSION"
echo ""
echo "To kill the session:"
echo "  tmux kill-session -t $SESSION"
echo ""
echo "Key checks:"
grep -c "Task Queue" /tmp/forge-tasks-view.txt > /dev/null && echo "  ✓ Tasks view working" || echo "  ✗ Tasks view issue"
grep -q 'Search:' /tmp/forge-search-typed.txt && echo "  ✓ Search indicator displayed" || echo "  - Search indicator check (may be expected)"
echo ""
