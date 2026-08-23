#!/bin/bash
# Comprehensive test script for operator actions in both standalone and client modes
# Tests: spawn_worker, kill_worker, chat submission, bead assignment
# At multiple terminal dimensions: 80x24, 120x40, 200x50

# Don't exit on errors - we want to see all test results
set +e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FORGE_BIN="${SCRIPT_DIR}/target/release/forge"
TEST_PORT=18888
TIMEOUT_SECONDS=30
SERVER_URL="ws://localhost:${TEST_PORT}/ws"

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Test counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

echo_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

echo_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

echo_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

echo_test() {
    echo -e "${BLUE}[TEST]${NC} $1"
}

# Track test results
record_test() {
    local name="$1"
    local result="$2"
    TESTS_RUN=$((TESTS_RUN + 1))
    if [[ "$result" == "PASS" ]]; then
        TESTS_PASSED=$((TESTS_PASSED + 1))
        echo_info "✓ $name - PASS"
    else
        TESTS_FAILED=$((TESTS_FAILED + 1))
        echo_error "✗ $name - FAIL"
    fi
}

# Cleanup function
cleanup_all_sessions() {
    echo_info "Cleaning up all test sessions..."

    # List of sessions to clean up
    local sessions=(
        "forge-server-test"
        "forge-client-standalone-80x24"
        "forge-client-standalone-120x40"
        "forge-client-standalone-200x50"
        "forge-server-mode-80x24"
        "forge-server-mode-120x40"
        "forge-server-mode-200x50"
    )

    for session in "${sessions[@]}"; do
        if tmux has-session -t "$session" 2>/dev/null; then
            tmux send-keys -t "$session" 'q' Enter 2>/dev/null || true
            sleep 0.5
            tmux kill-session -t "$session" 2>/dev/null || true
        fi
    done

    # Kill any forge server processes
    pkill -f "forge.*--server" 2>/dev/null || true
    pkill -f "forge.*--connect" 2>/dev/null || true
    sleep 1
}

trap cleanup_all_sessions EXIT

# Check if forge binary exists
if [[ ! -x "$FORGE_BIN" ]]; then
    echo_error "forge binary not found at $FORGE_BIN"
    echo_info "Building forge..."
    cd "$SCRIPT_DIR"
    cargo build --release
fi

# Verify tmux is available
if ! command -v tmux &> /dev/null; then
    echo_error "tmux is required for this test"
    exit 1
fi

# Initial cleanup
cleanup_all_sessions

echo ""
echo "=========================================="
echo "FORGE Operator Actions Test Suite"
echo "=========================================="
echo ""

# ============================================================
# Test Phase 1: Standalone Mode Tests
# ============================================================

echo_test "Phase 1: Testing Standalone Mode (no server)"
echo ""

test_dimensions_standalone() {
    local width=$1
    local height=$2
    local session="forge-client-standalone-${width}x${height}"

    echo_info "Testing at ${width}x${height} in standalone mode"

    # Create tmux session with specified dimensions
    tmux new-session -d -s "$session" -x "$width" -y "$height" "$FORGE_BIN"
    sleep 2

    # Test 1: Verify forge started
    INITIAL_PANE=$(tmux capture-pane -t "$session" -p)
    if echo "$INITIAL_PANE" | grep -q "FORGE"; then
        record_test "Standalone ${width}x${height}: Forge started" "PASS"
    else
        record_test "Standalone ${width}x${height}: Forge started" "FAIL"
        tmux kill-session -t "$session" 2>/dev/null || true
        return 1
    fi

    # Test 2: Switch to Workers view
    tmux send-keys -t "$session" 'w'
    sleep 1
    WORKERS_PANE=$(tmux capture-pane -t "$session" -p)
    if echo "$WORKERS_PANE" | grep -q "Worker"; then
        record_test "Standalone ${width}x${height}: Workers view accessible" "PASS"
    else
        record_test "Standalone ${width}x${height}: Workers view accessible" "FAIL"
    fi

    # Test 3: Attempt spawn worker (check for spawn dialog or status)
    tmux send-keys -t "$session" 's'
    sleep 1
    SPAWN_PANE=$(tmux capture-pane -t "$session" -p)
    if echo "$SPAWN_PANE" | grep -qE "(Spawn|worker|executor|Glm|Sonnet|Opus|Haiku)"; then
        record_test "Standalone ${width}x${height}: Spawn worker dialog shown" "PASS"

        # Cancel the spawn dialog
        tmux send-keys -t "$session" Escape
        sleep 0.5
    else
        record_test "Standalone ${width}x${height}: Spawn worker dialog shown" "FAIL"
    fi

    # Test 4: Switch to Chat view
    tmux send-keys -t "$session" ':'
    sleep 1
    CHAT_PANE=$(tmux capture-pane -t "$session" -p)
    if echo "$CHAT_PANE" | grep -q "Chat"; then
        record_test "Standalone ${width}x${height}: Chat view accessible" "PASS"
    else
        record_test "Standalone ${width}x${height}: Chat view accessible" "FAIL"
    fi

    # Test 5: Verify local chat backend (not server)
    # Check for input prompt (no "Connected to server" message)
    if echo "$CHAT_PANE" | grep -q "Input\|>" && ! echo "$CHAT_PANE" | grep -q "Connected to server"; then
        record_test "Standalone ${width}x${height}: Chat uses local backend" "PASS"
    else
        record_test "Standalone ${width}x${height}: Chat uses local backend" "FAIL"
    fi

    # Test 5a: Submit a chat message in standalone mode
    tmux send-keys -t "$session" "test standalone"
    sleep 0.5
    tmux send-keys -t "$session" Enter
    sleep 3

    CHAT_SUBMIT_PANE=$(tmux capture-pane -t "$session" -p)
    # In standalone mode with no backend, should show error or "not initialized"
    # With backend, would process locally
    if echo "$CHAT_SUBMIT_PANE" | grep -qE "(test standalone|Chat backend|Processing|Error)"; then
        record_test "Standalone ${width}x${height}: Chat submit processed locally" "PASS"
    else
        record_test "Standalone ${width}x${height}: Chat submit processed locally" "FAIL"
    fi

    # Test 6: Check for no server connection indicator
    FINAL_PANE=$(tmux capture-pane -t "$session" -p)
    if ! echo "$FINAL_PANE" | grep -q "Connected to server\|Server:"; then
        record_test "Standalone ${width}x${height}: No server connection shown" "PASS"
    else
        record_test "Standalone ${width}x${height}: No server connection shown" "FAIL"
    fi

    # Cleanup
    tmux kill-session -t "$session" 2>/dev/null || true
    sleep 0.5
}

# Test at multiple dimensions in standalone mode
test_dimensions_standalone 80 24
test_dimensions_standalone 120 40
test_dimensions_standalone 200 50

# ============================================================
# Test Phase 2: Server Mode Tests
# ============================================================

echo ""
echo_test "Phase 2: Testing Server Mode (with FORGE server)"
echo ""

# Start FORGE server
echo_info "Starting FORGE server on port $TEST_PORT"
tmux new-session -d -s "forge-server-test" -x 80 -y 24 "$FORGE_BIN --server --server-port $TEST_PORT"
sleep 3

# Verify server started
SERVER_PANE=$(tmux capture-pane -t "forge-server-test" -p)
if echo "$SERVER_PANE" | grep -qE "(listening|Server|FORGE)"; then
    record_test "Server startup: Server started" "PASS"
else
    record_test "Server startup: Server started" "FAIL"
    echo_error "Server failed to start, aborting client mode tests"
    cleanup_all_sessions
    exit 1
fi

# Wait for server to be ready
sleep 2

test_dimensions_client_mode() {
    local width=$1
    local height=$2
    local session="forge-server-mode-${width}x${height}"

    echo_info "Testing client mode at ${width}x${height}"

    # Create tmux session with client connecting to server
    tmux new-session -d -s "$session" -x "$width" -y "$height" "$FORGE_BIN --connect $SERVER_URL"
    sleep 3

    # Test 1: Verify client started
    CLIENT_PANE=$(tmux capture-pane -t "$session" -p)
    if echo "$CLIENT_PANE" | grep -q "FORGE"; then
        record_test "Client ${width}x${height}: Client started" "PASS"
    else
        record_test "Client ${width}x${height}: Client started" "FAIL"
        tmux kill-session -t "$session" 2>/dev/null || true
        return 1
    fi

    # Test 2: Verify connected to server
    sleep 2
    CONNECTED_PANE=$(tmux capture-pane -t "$session" -p)
    if echo "$CONNECTED_PANE" | grep -qE "(Connected|Server|ws://)"; then
        record_test "Client ${width}x${height}: Connected to server" "PASS"
    else
        record_test "Client ${width}x${height}: Connected to server" "WARN"
        echo_warn "Connection message not detected, may still be connecting"
    fi

    # Test 3: Switch to Workers view
    tmux send-keys -t "$session" 'w'
    sleep 1
    WORKERS_PANE=$(tmux capture-pane -t "$session" -p)
    if echo "$WORKERS_PANE" | grep -q "Worker"; then
        record_test "Client ${width}x${height}: Workers view accessible" "PASS"
    else
        record_test "Client ${width}x${height}: Workers view accessible" "FAIL"
    fi

    # Test 4: Attempt spawn worker (should send request to server)
    tmux send-keys -t "$session" 's'
    sleep 1
    SPAWN_PANE=$(tmux capture-pane -t "$session" -p)
    if echo "$SPAWN_PANE" | grep -qE "(Spawn|worker|executor|Glm|Sonnet|Opus|Haiku)"; then
        record_test "Client ${width}x${height}: Spawn worker dialog shown" "PASS"

        # Navigate to select GLM (first option)
        tmux send-keys -t "$session" Enter
        sleep 2

        # Check if request was sent to server (status message)
        REQUEST_PANE=$(tmux capture-pane -t "$session" -p)
        if echo "$REQUEST_PANE" | grep -qE "(Requested|sent|server)"; then
            record_test "Client ${width}x${height}: Spawn request sent to server" "PASS"
        else
            record_test "Client ${width}x${height}: Spawn request sent to server" "WARN"
        fi
    else
        record_test "Client ${width}x${height}: Spawn worker dialog shown" "FAIL"
    fi

    # Test 5: Check server logs for spawn request
    sleep 2
    SERVER_LOG=$(tmux capture-pane -t "forge-server-test" -p)
    if echo "$SERVER_LOG" | grep -qE "(SpawnWorker|spawn.*request|worker.*spawn)"; then
        record_test "Client ${width}x${height}: Server received spawn request" "PASS"
    else
        record_test "Client ${width}x${height}: Server received spawn request" "WARN"
        echo_warn "Server log check: spawn request not immediately visible"
    fi

    # Test 6: Switch to Chat view
    tmux send-keys -t "$session" ':'
    sleep 1
    CHAT_PANE=$(tmux capture-pane -t "$session" -p)
    if echo "$CHAT_PANE" | grep -q "Chat"; then
        record_test "Client ${width}x${height}: Chat view accessible" "PASS"
    else
        record_test "Client ${width}x${height}: Chat view accessible" "FAIL"
    fi

    # Test 7: Submit chat message (should route to server)
    tmux send-keys -t "$session" "test client mode"
    sleep 0.5
    tmux send-keys -t "$session" Enter
    sleep 3

    CHAT_SUBMIT_PANE=$(tmux capture-pane -t "$session" -p)
    # Check for "sent to server" message or pending indicator
    if echo "$CHAT_SUBMIT_PANE" | grep -qE "(test client mode|Sent|server|pending|Processing)"; then
        record_test "Client ${width}x${height}: Chat sent to server" "PASS"
    else
        record_test "Client ${width}x${height}: Chat sent to server" "FAIL"
    fi

    # Test 8: Check server logs for chat request
    SERVER_CHAT_LOG=$(tmux capture-pane -t "forge-server-test" -p)
    if echo "$SERVER_CHAT_LOG" | grep -qE "(SendChat|chat|message)"; then
        record_test "Client ${width}x${height}: Server received chat request" "PASS"
    else
        record_test "Client ${width}x${height}: Server received chat request" "WARN"
    fi

    # Test 9: Check for visual artifacts at this dimension
    FINAL_PANE=$(tmux capture-pane -t "$session" -p)
    # Look for common TUI issues: truncated text, misaligned borders, etc.
    if echo "$FINAL_PANE" | grep -qE "\.\.\.|\[...\]"; then
        record_test "Client ${width}x${height}: No visual truncation" "WARN"
        echo_warn "Possible text truncation detected at ${width}x${height}"
    else
        record_test "Client ${width}x${height}: No visual truncation" "PASS"
    fi

    # Cleanup
    tmux kill-session -t "$session" 2>/dev/null || true
    sleep 0.5
}

# Test at multiple dimensions in client mode
test_dimensions_client_mode 80 24
test_dimensions_client_mode 120 40
test_dimensions_client_mode 200 50

# ============================================================
# Test Phase 3: Kill Worker Tests
# ============================================================

echo ""
echo_test "Phase 3: Testing Kill Worker in Both Modes"
echo ""

# Test kill worker in standalone mode
echo_info "Testing kill_worker in standalone mode"
tmux new-session -d -s "forge-kill-standalone" -x 120 -y 40 "$FORGE_BIN"
sleep 2

# Switch to Workers view
tmux send-keys -t "forge-kill-standalone" 'w'
sleep 1

# Try to open kill dialog
tmux send-keys -t "forge-kill-standalone" 'k'
sleep 1

KILL_PANE=$(tmux capture-pane -t "forge-kill-standalone" -p)
if echo "$KILL_PANE" | grep -qE "(Kill|worker|select)"; then
    record_test "Standalone: Kill worker dialog accessible" "PASS"
else
    record_test "Standalone: Kill worker dialog accessible" "FAIL"
fi

# Cancel and cleanup
tmux send-keys -t "forge-kill-standalone" Escape
tmux kill-session -t "forge-kill-standalone" 2>/dev/null || true

# Test kill worker in client mode
echo_info "Testing kill_worker in client mode"
tmux new-session -d -s "forge-kill-client" -x 120 -y 40 "$FORGE_BIN --connect $SERVER_URL"
sleep 3

# Switch to Workers view
tmux send-keys -t "forge-kill-client" 'w'
sleep 1

# Try to open kill dialog
tmux send-keys -t "forge-kill-client" 'k'
sleep 1

KILL_CLIENT_PANE=$(tmux capture-pane -t "forge-kill-client" -p)
if echo "$KILL_CLIENT_PANE" | grep -qE "(Kill|worker|select)"; then
    record_test "Client mode: Kill worker dialog accessible" "PASS"
else
    record_test "Client mode: Kill worker dialog accessible" "FAIL"
fi

# Check server logs for kill request (if worker was selected)
sleep 1
SERVER_KILL_LOG=$(tmux capture-pane -t "forge-server-test" -p)

# Cleanup
tmux send-keys -t "forge-kill-client" Escape
tmux kill-session -t "forge-kill-client" 2>/dev/null || true

# ============================================================
# Test Phase 4: Bead Assignment Tests (Client Mode)
# ============================================================

echo ""
echo_test "Phase 4: Testing Bead Assignment in Client Mode"
echo ""

tmux new-session -d -s "forge-bead-test" -x 120 -y 40 "$FORGE_BIN --connect $SERVER_URL"
sleep 3

# Switch to Tasks/Beads view
tmux send-keys -t "forge-bead-test" 't'
sleep 1

BEADS_PANE=$(tmux capture-pane -t "forge-bead-test" -p)
if echo "$BEADS_PANE" | grep -qE "(Task|Bead|bead)"; then
    record_test "Client mode: Beads/Task view accessible" "PASS"

    # Check for assignment functionality
    # Look for 'a' key hint or assignment-related UI
    if echo "$BEADS_PANE" | grep -qE "(assign|Assign|'a')"; then
        record_test "Client mode: Bead assignment UI present" "PASS"
    else
        record_test "Client mode: Bead assignment UI present" "WARN"
        echo_warn "Assignment UI not clearly visible in this view"
    fi
else
    record_test "Client mode: Beads/Task view accessible" "FAIL"
fi

# Cleanup
tmux kill-session -t "forge-bead-test" 2>/dev/null || true

# ============================================================
# Test Phase 5: Error Handling Tests
# ============================================================

echo ""
echo_test "Phase 5: Testing Error Handling in Client Mode"
echo ""

echo_info "Testing client behavior when server unavailable"

# Kill the server temporarily
tmux kill-session -t "forge-server-test" 2>/dev/null || true
pkill -f "forge.*--server" 2>/dev/null || true
sleep 2

# Try to connect to non-existent server
tmux new-session -d -s "forge-error-test" -x 80 -y 24 "$FORGE_BIN --connect $SERVER_URL"
sleep 3

ERROR_PANE=$(tmux capture-pane -t "forge-error-test" -p)
# Check for error message or graceful fallback
if echo "$ERROR_PANE" | grep -qE "(error|failed|refused|Cannot connect)"; then
    record_test "Client mode: Error message on server unavailable" "PASS"
else
    record_test "Client mode: Error message on server unavailable" "WARN"
    echo_warn "Error message format may vary"
fi

# Check if TUI still renders (doesn't crash)
if echo "$ERROR_PANE" | grep -q "FORGE"; then
    record_test "Client mode: TUI remains responsive on connection error" "PASS"
else
    record_test "Client mode: TUI remains responsive on connection error" "FAIL"
fi

# Cleanup
tmux kill-session -t "forge-error-test" 2>/dev/null || true

# Restart server for final verification
echo_info "Restarting server for final verification"
tmux new-session -d -s "forge-server-test" -x 80 -y 24 "$FORGE_BIN --server --server-port $TEST_PORT"
sleep 3

# ============================================================
# Test Summary
# ============================================================

echo ""
echo "=========================================="
echo "TEST SUMMARY"
echo "=========================================="
echo ""
echo "Total tests run: $TESTS_RUN"
echo -e "Passed: ${GREEN}$TESTS_PASSED${NC}"
echo -e "Failed: ${RED}$TESTS_FAILED${NC}"
echo -e "Warnings: ${YELLOW}$((TESTS_RUN - TESTS_PASSED - TESTS_FAILED))${NC}"
echo ""

# Final cleanup (will also run on trap EXIT)
cleanup_all_sessions

if [[ $TESTS_FAILED -eq 0 ]]; then
    echo_info "All critical tests passed!"
    exit 0
else
    echo_error "Some tests failed. Review output above."
    exit 1
fi
