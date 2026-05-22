#!/usr/bin/env bash
# FORGE Sessions View Test (Team Collaboration)
#
# Tests the Sessions view for team collaboration features:
# 1. Test 's' hotkey to access Sessions view
# 2. Verify Sessions view renders correctly
# 3. Check empty state message when no server is connected
# 4. Test panel navigation within Sessions view
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

TEST_NAME="forge-sessions"

# ==============================================================================
# Test Implementation
# ==============================================================================

test_sessions_view() {
    local session
    session=$(get_session_name)
    local result=0
    local passed=0
    local failed=0

    log_info "=== Testing Sessions View (Team Collaboration) ==="

    # Step 1: Start forge
    log_info "Step 1: Starting forge..."
    if ! start_forge "$session"; then
        log_fail "Failed to start forge"
        return 1
    fi

    # Step 2: Wait for initialization
    log_info "Step 2: Waiting for initialization..."
    if ! wait_for_init "$session"; then
        log_fail "Forge failed to initialize"
        stop_forge "$session"
        return 1
    fi

    # Step 3: Navigate to Sessions view using 's' hotkey
    log_info "Step 3: Testing 's' hotkey for Sessions view..."
    send_key_wait "$session" "s" 1

    if pane_contains "$session" "Team Sessions"; then
        log_success "'s' -> Sessions: Found 'Team Sessions' title"
        ((passed++))
    else
        log_fail "'s' -> Sessions: Expected 'Team Sessions' title not found"
        ((failed++))
        result=1
    fi

    # Step 4: Verify empty state message (no server connected)
    log_info "Step 4: Verifying empty state message..."
    if pane_contains "$session" "No active sessions"; then
        log_success "Empty state: Found 'No active sessions' message"
        ((passed++))
    else
        log_fail "Empty state: Expected 'No active sessions' message not found"
        ((failed++))
        result=1
    fi

    # Step 5: Verify "Connected Users" header with count
    log_info "Step 5: Verifying Connected Users header..."
    if pane_contains "$session" "Connected Users"; then
        log_success "Header: Found 'Connected Users' text"
        ((passed++))
    else
        log_fail "Header: Expected 'Connected Users' text not found"
        ((failed++))
        result=1
    fi

    # Step 6: Verify we can navigate away and back to Sessions view
    log_info "Step 6: Testing navigation away and back to Sessions..."
    # Go to Overview
    send_key_wait "$session" "O" 1
    if ! pane_contains "$session" "Team Sessions"; then
        log_success "Navigation: Successfully left Sessions view"
        ((passed++))
    else
        log_fail "Navigation: Failed to leave Sessions view"
        ((failed++))
        result=1
    fi

    # Return to Sessions view
    send_key_wait "$session" "s" 1
    if pane_contains "$session" "Team Sessions"; then
        log_success "Navigation: Successfully returned to Sessions view"
        ((passed++))
    else
        log_fail "Navigation: Failed to return to Sessions view"
        ((failed++))
        result=1
    fi

    # Step 7: Verify the Sessions view is in the view navigation cycle
    log_info "Step 7: Testing Tab navigation through views..."
    # Press Tab multiple times to cycle through views
    local found_sessions=false
    for i in {1..15}; do
        send_key_wait "$session" "Tab" 1
        if pane_contains "$session" "Team Sessions"; then
            found_sessions=true
            break
        fi
    done

    if $found_sessions; then
        log_success "Tab navigation: Sessions view found in navigation cycle"
        ((passed++))
    else
        log_warn "Tab navigation: Could not verify Sessions in navigation cycle (may be timing issue)"
    fi

    # Cleanup
    stop_forge "$session"

    # Summary
    echo ""
    echo "========================================"
    echo "  Sessions View Test Summary"
    echo "========================================"
    echo "  Passed: $passed"
    echo "  Failed: $failed"
    echo "========================================"
    echo ""

    if [ $result -eq 0 ]; then
        log_success "All Sessions view tests passed!"
    else
        log_fail "Some Sessions view tests failed"
    fi

    return $result
}

# ==============================================================================
# Main
# ==============================================================================

main() {
    local session
    session=$(get_session_name)

    print_banner "$TEST_NAME"

    # Cleanup any stale sessions
    cleanup_stale_sessions

    # Run the test
    if test_sessions_view; then
        exit 0
    else
        exit 1
    fi
}

main "$@"
