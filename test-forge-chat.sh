#!/bin/bash
# Automated test script for validating chat UI rendering in forge
# This script tests that chat responses appear in UI after being received via channel.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEST_SESSION_NAME="forge-test-$$"
LOG_FILE="${HOME}/.forge/logs/forge.log.$(date +%Y-%m-%d)"
TIMEOUT_SECONDS=60
TEST_QUERY="What is 2 plus 2?"

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

echo_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

echo_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

cleanup() {
    echo_info "Cleaning up test session..."
    if tmux has-session -t "$TEST_SESSION_NAME" 2>/dev/null; then
        # Try graceful quit first
        tmux send-keys -t "$TEST_SESSION_NAME" 'q' 2>/dev/null || true
        sleep 1
        # Force kill if still running
        tmux kill-session -t "$TEST_SESSION_NAME" 2>/dev/null || true
    fi
}

trap cleanup EXIT

# Check if forge binary exists
FORGE_BIN="$HOME/.cargo/bin/forge"
if [[ ! -x "$FORGE_BIN" ]]; then
    echo_error "forge binary not found at $FORGE_BIN"
    echo_info "Building forge..."
    cd "$SCRIPT_DIR"
    cargo build --release
    cp target/release/forge "$FORGE_BIN"
fi

# Verify chat backend config exists
CONFIG_FILE="$HOME/.forge/config.yaml"
if [[ ! -f "$CONFIG_FILE" ]]; then
    echo_error "Chat config not found at $CONFIG_FILE"
    exit 1
fi

# Check for chat_backend section
if ! grep -q "chat_backend:" "$CONFIG_FILE"; then
    echo_error "chat_backend section missing from config.yaml"
    exit 1
fi

echo_info "Starting forge in tmux session: $TEST_SESSION_NAME"

# Create tmux session with forge
tmux new-session -d -s "$TEST_SESSION_NAME" -x 100 -y 30 "$FORGE_BIN"

# Wait for forge to start up
echo_info "Waiting for forge to initialize..."
sleep 3

# Capture initial state
INITIAL_PANE=$(tmux capture-pane -t "$TEST_SESSION_NAME" -p)
echo_info "Initial state captured"

if echo "$INITIAL_PANE" | grep -q "FORGE"; then
    echo_info "Forge started successfully"
else
    echo_error "Forge did not start correctly"
    echo "$INITIAL_PANE"
    exit 1
fi

# Switch to Chat view
echo_info "Switching to Chat view (pressing ':' key)..."
tmux send-keys -t "$TEST_SESSION_NAME" ':'
sleep 1

# Verify Chat view is active
CHAT_VIEW=$(tmux capture-pane -t "$TEST_SESSION_NAME" -p)
if echo "$CHAT_VIEW" | grep -q "Chat"; then
    echo_info "Chat view activated"
else
    echo_warn "Chat view might not be active - continuing anyway"
fi

# Test 1: Submit a chat query
echo_info "Submitting test query: $TEST_QUERY"
tmux send-keys -t "$TEST_SESSION_NAME" "$TEST_QUERY"
sleep 0.5
tmux send-keys -t "$TEST_SESSION_NAME" Enter
echo_info "Query submitted"

# Wait for response
echo_info "Waiting for chat response (timeout: ${TIMEOUT_SECONDS}s)..."
ELAPSED=0
RESPONSE_RECEIVED=false

while [[ $ELAPSED -lt $TIMEOUT_SECONDS ]]; do
    CURRENT_PANE=$(tmux capture-pane -t "$TEST_SESSION_NAME" -p)

    # Check for response indicators
    if echo "$CURRENT_PANE" | grep -qE "(Assistant:|Response received|4|four|answer)"; then
        RESPONSE_RECEIVED=true
        echo_info "Response detected in pane!"
        break
    fi

    # Check if still processing
    if echo "$CURRENT_PANE" | grep -q "Processing"; then
        echo -n "."
    fi

    sleep 1
    ELAPSED=$((ELAPSED + 1))
done
echo ""

# Final pane capture
FINAL_PANE=$(tmux capture-pane -t "$TEST_SESSION_NAME" -p)

# Display test results
echo ""
echo "=========================================="
echo "TEST RESULTS"
echo "=========================================="

# Check 1: Chat view is shown
if echo "$FINAL_PANE" | grep -q "Chat"; then
    echo_info "CHECK 1: Chat view displayed - PASS"
else
    echo_error "CHECK 1: Chat view displayed - FAIL"
fi

# Check 2: User query appears in history
if echo "$FINAL_PANE" | grep -q "$TEST_QUERY"; then
    echo_info "CHECK 2: User query in history - PASS"
else
    echo_warn "CHECK 2: User query in history - NOT FOUND (may have scrolled)"
fi

# Check 3: Response received
if [[ "$RESPONSE_RECEIVED" == "true" ]]; then
    echo_info "CHECK 3: Response received - PASS"
else
    echo_error "CHECK 3: Response received - FAIL (timeout)"
fi

# Check 4: No error messages
if echo "$FINAL_PANE" | grep -qiE "error|failed|panic"; then
    echo_error "CHECK 4: No errors displayed - FAIL"
    echo "$FINAL_PANE" | grep -iE "error|failed|panic"
else
    echo_info "CHECK 4: No errors displayed - PASS"
fi

# Check 5: Timestamp present
if echo "$FINAL_PANE" | grep -qE "\[[0-9]{2}:[0-9]{2}:[0-9]{2}\]"; then
    echo_info "CHECK 5: Timestamp displayed - PASS"
else
    echo_warn "CHECK 5: Timestamp displayed - NOT FOUND"
fi

# Check diagnostic logs
echo ""
echo "=========================================="
echo "DIAGNOSTIC LOGS"
echo "=========================================="

if [[ -f "$LOG_FILE" ]]; then
    echo_info "Recent chat-related log entries:"
    tail -50 "$LOG_FILE" 2>/dev/null | grep -E "(Success|Response text length|Chat history|Current view|poll_chat|Processing chat)" | tail -20 || echo "No relevant log entries"
else
    echo_warn "Log file not found: $LOG_FILE"
fi

echo ""
echo "=========================================="
echo "PANE CONTENT SNAPSHOT"
echo "=========================================="
echo "$FINAL_PANE"

# Test 2: Multiple exchanges
echo ""
echo "=========================================="
echo "TEST 2: MULTIPLE EXCHANGES"
echo "=========================================="

echo_info "Sending second query..."
TEST_QUERY_2="What color is the sky?"
tmux send-keys -t "$TEST_SESSION_NAME" "$TEST_QUERY_2"
tmux send-keys -t "$TEST_SESSION_NAME" Enter

sleep 10

MULTI_PANE=$(tmux capture-pane -t "$TEST_SESSION_NAME" -p)
if echo "$MULTI_PANE" | grep -q "You:"; then
    echo_info "Multiple exchanges: User prompts visible - PASS"
else
    echo_warn "Multiple exchanges: User prompts not detected"
fi

# Test 3: View persistence
echo ""
echo "=========================================="
echo "TEST 3: VIEW PERSISTENCE"
echo "=========================================="

echo_info "Switching to Workers view..."
tmux send-keys -t "$TEST_SESSION_NAME" 'w'
sleep 1

WORKERS_PANE=$(tmux capture-pane -t "$TEST_SESSION_NAME" -p)
if echo "$WORKERS_PANE" | grep -q "Worker"; then
    echo_info "Workers view activated - PASS"
fi

echo_info "Returning to Chat view..."
tmux send-keys -t "$TEST_SESSION_NAME" ':'
sleep 1

RETURN_PANE=$(tmux capture-pane -t "$TEST_SESSION_NAME" -p)
if echo "$RETURN_PANE" | grep -q "You:"; then
    echo_info "Chat history persisted after view switch - PASS"
else
    echo_warn "Chat history persistence check - UNCERTAIN"
fi

# Summary
echo ""
echo "=========================================="
echo "SUMMARY"
echo "=========================================="
echo "Test session: $TEST_SESSION_NAME"
echo ""
echo "To attach and inspect manually: tmux attach -t $TEST_SESSION_NAME"
echo ""

if [[ "$RESPONSE_RECEIVED" == "true" ]]; then
    echo_info "OVERALL: Chat UI rendering validation PASSED"
    exit 0
else
    echo_error "OVERALL: Chat UI rendering validation FAILED"
    echo_info "The test session is still running for manual inspection."
    echo_info "Press Enter to kill the session and exit..."
    read -r
    exit 1
fi
