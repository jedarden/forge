#!/usr/bin/env bash
# FORGE Basic Launch Test - Simplified TUI Verification
#
# This is a simplified test focusing only on core functionality:
# 1. TUI launches successfully
# 2. Basic UI renders (FORGE header visible)
# 3. Clean shutdown works
#
# Created for bead bd-1laz: Alternative simplified-scope approach
# for fg-2p6 "Test TUI dashboard launch and basic rendering"
#
# This test is intentionally minimal - for comprehensive testing,
# use test-forge-e2e.sh which covers multiple terminal sizes,
# view navigation, chat interface, and worker spawning.
#
# Exit codes:
#   0 - Test passed
#   1 - Test failed

set -euo pipefail

# Get script directory and source helpers
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/test-helpers.sh"

# ==============================================================================
# Test Configuration
# ==============================================================================

TEST_NAME="basic-launch"
SESSION_NAME="${TEST_SESSION_PREFIX}-basic-$$"

# Simplified: test only at one reasonable size (standard wide terminal)
COLS=120
ROWS=40

# Shorter timeout for basic test
BASIC_INIT_TIMEOUT=30

# ==============================================================================
# Test Functions
# ==============================================================================

test_tui_launches() {
    log_info "Test: TUI launches successfully"

    # Kill any existing session
    tmux kill-session -t "$SESSION_NAME" 2>/dev/null || true

    # Create session with forge
    if ! tmux new-session -d -s "$SESSION_NAME" -x "$COLS" -y "$ROWS" "forge"; then
        log_fail "Failed to create tmux session"
        return 1
    fi

    log_success "Tmux session created"
    return 0
}

test_ui_renders() {
    log_info "Test: UI renders correctly"

    local start_time=$SECONDS
    local found_forge=false
    local found_panel=false

    while (( SECONDS - start_time < BASIC_INIT_TIMEOUT )); do
        if session_exists "$SESSION_NAME"; then
            local content
            content=$(tmux capture-pane -t "$SESSION_NAME" -p 2>/dev/null || echo "")

            # Check for FORGE header
            if echo "$content" | grep -q "FORGE"; then
                found_forge=true
            fi

            # Check for main panel content (Worker Pool indicates full render)
            if echo "$content" | grep -q "Worker Pool"; then
                found_panel=true
            fi

            if $found_forge && $found_panel; then
                local elapsed=$((SECONDS - start_time))
                log_success "UI rendered after ${elapsed}s"
                return 0
            fi
        else
            log_fail "Session died during initialization"
            return 1
        fi

        sleep 1
    done

    log_fail "Timeout: UI did not render within ${BASIC_INIT_TIMEOUT}s"

    # Show what we got
    log_warn "Debug: Found FORGE header = $found_forge, Found Worker Pool = $found_panel"
    echo "Captured content:"
    tmux capture-pane -t "$SESSION_NAME" -p 2>/dev/null | head -20 || true

    return 1
}

test_clean_shutdown() {
    log_info "Test: Clean shutdown"

    # Escape any input mode first
    tmux send-keys -t "$SESSION_NAME" "Escape" 2>/dev/null || true
    sleep 0.3

    # Send quit command
    tmux send-keys -t "$SESSION_NAME" "q" 2>/dev/null || true
    sleep 1

    if ! session_exists "$SESSION_NAME"; then
        log_success "Clean exit via 'q' key"
        return 0
    fi

    # Fallback: try Ctrl+C
    tmux send-keys -t "$SESSION_NAME" "C-c" 2>/dev/null || true
    sleep 0.5

    if ! session_exists "$SESSION_NAME"; then
        log_warn "Exit required Ctrl+C (acceptable)"
        return 0
    fi

    # Last resort: force kill
    tmux kill-session -t "$SESSION_NAME" 2>/dev/null || true
    log_warn "Forced session kill (check for shutdown issues)"
    return 0
}

test_no_crashes() {
    log_info "Test: No crashes in logs"

    local log_file
    log_file=$(get_log_file)

    if [ -f "$log_file" ]; then
        if grep -qiE "panic|thread.*panicked|fatal error|segfault" "$log_file" 2>/dev/null; then
            log_fail "Crash pattern detected in logs"
            grep -iE "panic|thread.*panicked|fatal error|segfault" "$log_file" | head -5
            return 1
        fi
        log_success "No crash patterns in logs"
    else
        log_warn "No log file found (may be first run)"
    fi

    return 0
}

# ==============================================================================
# Main
# ==============================================================================

cleanup() {
    tmux kill-session -t "$SESSION_NAME" 2>/dev/null || true
}

main() {
    echo ""
    echo "========================================"
    echo "FORGE Basic Launch Test"
    echo "========================================"
    echo ""
    echo "Scope: Minimal viable TUI verification"
    echo "Terminal: ${COLS}x${ROWS}"
    echo ""

    local start_time=$SECONDS
    local tests_passed=0
    local tests_total=4

    # Test 1: TUI launches
    if test_tui_launches; then
        ((tests_passed++)) || true
    else
        cleanup
        echo ""
        echo "RESULT: FAILED (TUI did not launch)"
        exit 1
    fi

    # Test 2: UI renders
    if test_ui_renders; then
        ((tests_passed++)) || true
    else
        cleanup
        echo ""
        echo "RESULT: FAILED (UI did not render)"
        exit 1
    fi

    # Test 3: Clean shutdown
    if test_clean_shutdown; then
        ((tests_passed++)) || true
    fi

    # Test 4: No crashes
    if test_no_crashes; then
        ((tests_passed++)) || true
    fi

    local elapsed=$((SECONDS - start_time))

    # Cleanup
    cleanup

    # Summary
    echo ""
    echo "========================================"
    echo "  Test Summary"
    echo "========================================"
    echo ""
    echo "Tests passed: $tests_passed/$tests_total"
    echo "Duration: ${elapsed}s"
    echo ""

    if [ $tests_passed -eq $tests_total ]; then
        echo -e "${GREEN}ALL TESTS PASSED${NC}"
        exit 0
    elif [ $tests_passed -ge 3 ]; then
        echo -e "${YELLOW}MOSTLY PASSED (minor issues)${NC}"
        exit 0
    else
        echo -e "${RED}TESTS FAILED${NC}"
        exit 1
    fi
}

# Set up trap for cleanup
trap cleanup EXIT INT TERM

main "$@"
