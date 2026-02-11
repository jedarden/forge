#!/usr/bin/env bash
# FORGE Chat Functionality Test
#
# Tests:
# 1. Launch forge in tmux
# 2. Enter chat mode (:)
# 3. Send query
# 4. Verify response displays
# 5. Check logs for success
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

TEST_NAME="forge-chat"
CHAT_QUERY="What is forge?"
RESPONSE_TIMEOUT=45

# ==============================================================================
# Test Implementation
# ==============================================================================

test_chat_functionality() {
    local session
    session=$(get_session_name)
    local result=0

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

    # Verify session is still running
    if ! assert_session_running "$session" "Forge session should be running after init"; then
        stop_forge "$session"
        return 1
    fi

    # Step 3: Enter chat mode
    log_info "Step 3: Entering chat mode with ':' key..."
    send_key_wait "$session" ":" 1

    # Verify we're in chat view
    if ! wait_for_pane_text "$session" "Chat" 5; then
        log_warn "Chat view title not visible, continuing anyway..."
    fi

    # Step 4: Type chat query
    log_info "Step 4: Typing chat query: '$CHAT_QUERY'"
    type_text "$session" "$CHAT_QUERY"
    sleep 1

    # Step 5: Submit query
    log_info "Step 5: Submitting query with Enter..."
    send_keys "$session" "Enter"

    # Step 6: Wait for response
    log_info "Step 6: Waiting for response (timeout: ${RESPONSE_TIMEOUT}s)..."

    local response_received=false
    local start_time=$SECONDS

    while (( SECONDS - start_time < RESPONSE_TIMEOUT )); do
        # Check for response in logs
        if log_contains "Got response from channel!" 100; then
            response_received=true
            local elapsed=$((SECONDS - start_time))
            log_success "Response received after ${elapsed}s"
            break
        fi

        # Also check for processing indicator
        if log_contains "Chat thread started" 100 && ! log_contains "Got response" 100; then
            echo -n "."
        fi

        sleep 1
    done
    echo ""

    if ! $response_received; then
        log_fail "No response received within ${RESPONSE_TIMEOUT}s"
        result=1
    fi

    # Step 7: Capture final state
    log_info "Step 7: Capturing test output..."
    local output_file
    output_file=$(capture_pane "$session")
    log_info "Output saved to: $output_file"

    # Step 8: Verify response content
    log_info "Step 8: Verifying response..."

    # Check logs for success indicators
    if log_contains "Response text length" 100; then
        log_success "Response text was received"
    else
        log_warn "Could not verify response text in logs"
    fi

    # Check chat history was updated
    if log_contains "Chat history now has" 100; then
        log_success "Chat history was updated"
    else
        log_warn "Could not verify chat history update"
    fi

    # Step 9: Cleanup
    log_info "Step 9: Cleaning up..."
    stop_forge "$session"

    # Verify session is gone
    if session_exists "$session"; then
        log_warn "Session still exists after cleanup"
        tmux kill-session -t "$session" 2>/dev/null || true
    fi

    return $result
}

# ==============================================================================
# Main
# ==============================================================================

main() {
    echo ""
    echo "========================================"
    echo "FORGE Chat Test"
    echo "========================================"

    # Adjust for CI environment
    ci_adjust

    local start_time=$SECONDS
    local result=0

    if test_chat_functionality; then
        local elapsed=$((SECONDS - start_time))
        echo ""
        echo -e "${GREEN}========================================"
        echo "TEST PASSED (${elapsed}s)"
        echo -e "========================================${NC}"
    else
        local elapsed=$((SECONDS - start_time))
        echo ""
        echo -e "${RED}========================================"
        echo "TEST FAILED (${elapsed}s)"
        echo -e "========================================${NC}"
        result=1
    fi

    exit $result
}

# Only run main if not being sourced
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
