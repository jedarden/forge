#!/usr/bin/env bash
# FORGE Test Framework - Shared Helper Functions
#
# This library provides common utilities for automated TUI testing using tmux.
# Source this file in individual test scripts.

set -euo pipefail

# ==============================================================================
# Configuration
# ==============================================================================

# Test session naming
TEST_SESSION_PREFIX="${TEST_SESSION_PREFIX:-forge-test}"

# Timeouts (in seconds)
INIT_TIMEOUT="${INIT_TIMEOUT:-90}"      # Time to wait for forge to initialize
ACTION_TIMEOUT="${ACTION_TIMEOUT:-30}"  # Time to wait for an action to complete
POLL_INTERVAL="${POLL_INTERVAL:-1}"     # Polling interval for checks

# Terminal dimensions
TERM_WIDTH="${TERM_WIDTH:-229}"
TERM_HEIGHT="${TERM_HEIGHT:-55}"

# Output paths
TMP_DIR="${TMP_DIR:-/tmp}"
LOG_BASE="$HOME/.forge/logs/forge.log"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# ==============================================================================
# Core Helper Functions
# ==============================================================================

# Print colored status messages
log_info() {
    echo -e "${BLUE}[INFO]${NC} $*"
}

log_success() {
    echo -e "${GREEN}[PASS]${NC} $*"
}

log_fail() {
    echo -e "${RED}[FAIL]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

# Generate unique session name
get_session_name() {
    echo "${TEST_SESSION_PREFIX}-$$-$(date +%s)"
}

# Get today's log file
get_log_file() {
    echo "${LOG_BASE}.$(date +%Y-%m-%d)"
}

# Check if a tmux session exists
session_exists() {
    local session="$1"
    tmux has-session -t "$session" 2>/dev/null
}

# ==============================================================================
# Forge Lifecycle Management
# ==============================================================================

# Start forge in a new tmux session
# Usage: start_forge SESSION_NAME [WIDTH] [HEIGHT]
start_forge() {
    local session="$1"
    local width="${2:-$TERM_WIDTH}"
    local height="${3:-$TERM_HEIGHT}"

    log_info "Starting forge in tmux session: $session (${width}x${height})"

    # Kill any existing session with same name
    tmux kill-session -t "$session" 2>/dev/null || true

    # Kill any existing forge processes (safety cleanup)
    pkill -f "^forge$" 2>/dev/null || true
    sleep 0.5

    # Create new detached session running forge
    tmux new-session -d -s "$session" -x "$width" -y "$height" "forge"

    return 0
}

# Wait for forge to initialize
# Usage: wait_for_init SESSION_NAME [TIMEOUT]
wait_for_init() {
    local session="$1"
    local timeout="${2:-$INIT_TIMEOUT}"
    local log_file
    log_file=$(get_log_file)

    log_info "Waiting for forge to initialize (timeout: ${timeout}s)..."

    local start_time=$SECONDS
    while (( SECONDS - start_time < timeout )); do
        # Check for initialization success in logs
        if grep -q "Chat backend initialized successfully" <(tail -100 "$log_file" 2>/dev/null || echo ""); then
            local elapsed=$((SECONDS - start_time))
            log_success "Forge initialized after ${elapsed}s"
            # Wait a bit more for UI to fully render
            sleep 2
            return 0
        fi

        # Also check if session died
        if ! session_exists "$session"; then
            log_fail "Session died during initialization"
            return 1
        fi

        sleep "$POLL_INTERVAL"
    done

    log_fail "Timeout: Forge did not initialize within ${timeout}s"
    show_diagnostic_info "$session"
    return 1
}

# Wait for UI to stabilize (helper for after actions)
# Usage: wait_for_ui SESSION_NAME [WAIT_SECS]
wait_for_ui() {
    local session="$1"
    local wait="${2:-0.5}"
    sleep "$wait"
}

# Stop forge and cleanup session
# Usage: stop_forge SESSION_NAME
stop_forge() {
    local session="$1"

    log_info "Stopping forge session: $session"

    # Send quit command
    tmux send-keys -t "$session" "q" 2>/dev/null || true
    sleep 0.5

    # Kill session if still exists
    tmux kill-session -t "$session" 2>/dev/null || true

    return 0
}

# ==============================================================================
# Keystroke Injection
# ==============================================================================

# Send keys to the tmux session
# Usage: send_keys SESSION_NAME KEYS...
send_keys() {
    local session="$1"
    shift

    tmux send-keys -t "$session" "$@"
}

# Send a key and wait for a brief moment
# Usage: send_key_wait SESSION_NAME KEY [WAIT_SECS]
send_key_wait() {
    local session="$1"
    local key="$2"
    local wait="${3:-0.5}"

    tmux send-keys -t "$session" "$key"
    sleep "$wait"
}

# Type text character by character (for input fields)
# Usage: type_text SESSION_NAME TEXT
type_text() {
    local session="$1"
    local text="$2"

    tmux send-keys -t "$session" "$text"
}

# ==============================================================================
# Output Capture & Verification
# ==============================================================================

# Capture current tmux pane content
# Usage: capture_pane SESSION_NAME [OUTPUT_FILE]
capture_pane() {
    local session="$1"
    local output="${2:-$TMP_DIR/forge-test-output-$$.txt}"

    tmux capture-pane -t "$session" -p > "$output"
    echo "$output"
}

# Check if pane content contains text
# Usage: pane_contains SESSION_NAME TEXT
pane_contains() {
    local session="$1"
    local text="$2"

    local content
    content=$(tmux capture-pane -t "$session" -p 2>/dev/null || echo "")
    # Use fixed-string grep for reliable matching
    echo "$content" | grep -qF "$text"
}

# Wait for text to appear in pane
# Usage: wait_for_pane_text SESSION_NAME TEXT [TIMEOUT]
wait_for_pane_text() {
    local session="$1"
    local text="$2"
    local timeout="${3:-$ACTION_TIMEOUT}"

    local start_time=$SECONDS
    while (( SECONDS - start_time < timeout )); do
        if pane_contains "$session" "$text"; then
            return 0
        fi
        sleep "$POLL_INTERVAL"
    done

    return 1
}

# Check log file for pattern
# Usage: log_contains PATTERN [TAIL_LINES]
log_contains() {
    local pattern="$1"
    local lines="${2:-100}"
    local log_file
    log_file=$(get_log_file)

    grep -q "$pattern" <(tail -"$lines" "$log_file" 2>/dev/null || echo "")
}

# Wait for pattern in log file
# Usage: wait_for_log PATTERN [TIMEOUT] [TAIL_LINES]
wait_for_log() {
    local pattern="$1"
    local timeout="${2:-$ACTION_TIMEOUT}"
    local lines="${3:-100}"

    local start_time=$SECONDS
    while (( SECONDS - start_time < timeout )); do
        if log_contains "$pattern" "$lines"; then
            return 0
        fi
        sleep "$POLL_INTERVAL"
    done

    return 1
}

# ==============================================================================
# Diagnostics
# ==============================================================================

# Show diagnostic information for debugging
# Usage: show_diagnostic_info SESSION_NAME
show_diagnostic_info() {
    local session="$1"

    echo ""
    log_warn "=== Diagnostic Information ==="

    echo "Session status:"
    if session_exists "$session"; then
        echo "  Session '$session' is running"
    else
        echo "  Session '$session' is NOT running"
    fi

    echo ""
    echo "Recent logs (last 20 lines):"
    local log_file
    log_file=$(get_log_file)
    tail -20 "$log_file" 2>/dev/null || echo "  No log file found"

    echo ""
    echo "Pane content:"
    if session_exists "$session"; then
        tmux capture-pane -t "$session" -p 2>/dev/null | head -20 || echo "  Could not capture pane"
    else
        echo "  Session not available"
    fi

    echo "=== End Diagnostics ==="
    echo ""
}

# ==============================================================================
# Test Framework
# ==============================================================================

# Initialize test environment
# Usage: test_init TEST_NAME
test_init() {
    local test_name="$1"

    echo ""
    echo "========================================"
    echo "Test: $test_name"
    echo "========================================"
    log_info "Starting test: $test_name"

    TEST_START_TIME=$SECONDS
    TEST_SESSION=$(get_session_name)
    TEST_OUTPUT="$TMP_DIR/forge-test-${test_name}-$$.txt"

    export TEST_SESSION TEST_OUTPUT
}

# Cleanup test environment
# Usage: test_cleanup
test_cleanup() {
    if [ -n "${TEST_SESSION:-}" ]; then
        stop_forge "$TEST_SESSION" 2>/dev/null || true
    fi
}

# Report test result
# Usage: test_result PASS_OR_FAIL [MESSAGE]
test_result() {
    local result="$1"
    local message="${2:-}"

    local elapsed=$((SECONDS - ${TEST_START_TIME:-0}))

    if [ "$result" = "pass" ] || [ "$result" = "0" ]; then
        log_success "Test PASSED (${elapsed}s)${message:+ - $message}"
        return 0
    else
        log_fail "Test FAILED (${elapsed}s)${message:+ - $message}"
        return 1
    fi
}

# Run a test with automatic setup/teardown
# Usage: run_test TEST_NAME TEST_FUNCTION
run_test() {
    local test_name="$1"
    local test_fn="$2"

    test_init "$test_name"

    # Set trap for cleanup
    trap test_cleanup EXIT

    # Run the test function
    local result=0
    if "$test_fn"; then
        test_result "pass"
    else
        test_result "fail"
        result=1
    fi

    test_cleanup
    trap - EXIT

    return $result
}

# ==============================================================================
# Assertions
# ==============================================================================

# Assert that pane contains text
# Usage: assert_pane_contains SESSION_NAME TEXT [MESSAGE]
assert_pane_contains() {
    local session="$1"
    local text="$2"
    local message="${3:-Pane should contain '$text'}"

    if pane_contains "$session" "$text"; then
        log_success "$message"
        return 0
    else
        log_fail "$message"
        return 1
    fi
}

# Assert that pane does NOT contain text
# Usage: assert_pane_not_contains SESSION_NAME TEXT [MESSAGE]
assert_pane_not_contains() {
    local session="$1"
    local text="$2"
    local message="${3:-Pane should not contain '$text'}"

    if ! pane_contains "$session" "$text"; then
        log_success "$message"
        return 0
    else
        log_fail "$message"
        return 1
    fi
}

# Assert log contains pattern
# Usage: assert_log_contains PATTERN [MESSAGE]
assert_log_contains() {
    local pattern="$1"
    local message="${2:-Log should contain '$pattern'}"

    if log_contains "$pattern"; then
        log_success "$message"
        return 0
    else
        log_fail "$message"
        return 1
    fi
}

# Assert session is running
# Usage: assert_session_running SESSION_NAME [MESSAGE]
assert_session_running() {
    local session="$1"
    local message="${2:-Session '$session' should be running}"

    if session_exists "$session"; then
        log_success "$message"
        return 0
    else
        log_fail "$message"
        return 1
    fi
}

# ==============================================================================
# CI/Non-Interactive Support
# ==============================================================================

# Check if running in CI environment
is_ci() {
    [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ] || [ -n "${JENKINS_URL:-}" ]
}

# Adjust settings for CI environment
ci_adjust() {
    if is_ci; then
        log_info "CI environment detected, adjusting settings"
        # Increase timeouts in CI
        INIT_TIMEOUT=$((INIT_TIMEOUT * 2))
        ACTION_TIMEOUT=$((ACTION_TIMEOUT * 2))
        # Use smaller terminal in CI
        TERM_WIDTH="${COLUMNS:-120}"
        TERM_HEIGHT="${LINES:-40}"
    fi
}

# Export functions for use in subshells
export -f log_info log_success log_fail log_warn
export -f get_session_name get_log_file session_exists
export -f start_forge wait_for_init wait_for_ui stop_forge
export -f send_keys send_key_wait type_text
export -f capture_pane pane_contains wait_for_pane_text
export -f log_contains wait_for_log
export -f show_diagnostic_info
export -f test_init test_cleanup test_result run_test
export -f assert_pane_contains assert_pane_not_contains
export -f assert_log_contains assert_session_running
export -f is_ci ci_adjust
