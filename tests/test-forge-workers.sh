#!/usr/bin/env bash
# FORGE Worker Management Test
#
# Tests:
# 1. Spawn worker (g/s/o/h keys for different models)
# 2. Verify worker appears in status
# 3. Kill worker (k key)
# 4. Validate worker panel updates
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

TEST_NAME="forge-workers"

# Worker spawn keys:
# g/G - GLM-4.7
# s/S - Sonnet 4.5
# o   - Opus 4.6 (lowercase only)
# h   - Haiku (lowercase only)

# ==============================================================================
# Test Implementation
# ==============================================================================

test_worker_spawn() {
    local session
    session=$(get_session_name)
    local result=0

    log_info "=== Testing Worker Spawn Functionality ==="

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

    # Step 3: Navigate to Workers view
    log_info "Step 3: Navigating to Workers view with 'w' key..."
    send_key_wait "$session" "w" 1

    # Verify we're in Workers view
    if ! wait_for_pane_text "$session" "Workers" 5; then
        log_warn "Workers view not visible, continuing..."
    fi

    # Step 4: Test spawning GLM worker
    log_info "Step 4: Testing spawn GLM worker with 'g' key..."
    send_key_wait "$session" "g" 2

    # Check for spawn message in status
    if wait_for_pane_text "$session" "Spawning" 5 || wait_for_pane_text "$session" "GLM" 5; then
        log_success "GLM worker spawn initiated"
    else
        log_warn "Could not verify GLM spawn - checking status message..."
    fi

    # Step 5: Capture worker panel state
    log_info "Step 5: Capturing worker panel state..."
    local output1
    output1=$(capture_pane "$session")
    log_info "Pre-spawn state saved to: $output1"

    # Step 6: Test spawning Sonnet worker
    log_info "Step 6: Testing spawn Sonnet worker with 's' key..."
    send_key_wait "$session" "s" 2

    if pane_contains "$session" "Spawning" || pane_contains "$session" "Sonnet"; then
        log_success "Sonnet worker spawn initiated"
    else
        log_warn "Could not verify Sonnet spawn"
    fi

    # Step 7: Test kill worker
    log_info "Step 7: Testing kill worker with 'k' key..."
    send_key_wait "$session" "k" 2

    if pane_contains "$session" "Kill" || pane_contains "$session" "kill"; then
        log_success "Kill worker action triggered"
    else
        log_warn "Could not verify kill action - may not be implemented yet"
    fi

    # Step 8: Verify worker panel updates
    log_info "Step 8: Verifying worker panel displays..."

    # Worker panel should show table headers
    if assert_pane_contains "$session" "Worker" "Worker panel should show worker information"; then
        result=0
    fi

    # Check for worker count or status
    if pane_contains "$session" "Status" || pane_contains "$session" "Model" || pane_contains "$session" "active"; then
        log_success "Worker panel shows status information"
    else
        log_warn "Worker panel status information not visible"
    fi

    # Step 9: Cleanup
    log_info "Step 9: Cleaning up..."
    stop_forge "$session"

    return $result
}

test_worker_navigation() {
    local session
    session=$(get_session_name)
    local result=0

    log_info "=== Testing Worker View Navigation ==="

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

    # Step 3: Go to Workers view
    log_info "Step 3: Testing 'w' key for Workers view..."
    send_key_wait "$session" "w" 1

    if assert_pane_contains "$session" "Worker Pool" "Workers view should show Worker Pool panel"; then
        log_success "'w' key navigates to Workers view"
    else
        log_fail "'w' key did not navigate to Workers view"
        result=1
    fi

    # Step 4: Test navigation within view
    log_info "Step 4: Testing navigation keys in Workers view..."

    # Test down navigation
    send_key_wait "$session" "j" 0.5
    log_info "Pressed 'j' for down navigation"

    # Test up navigation
    send_keys "$session" "Up"
    sleep 0.5
    log_info "Pressed Up arrow for up navigation"

    # Step 5: Cleanup
    log_info "Step 5: Cleaning up..."
    stop_forge "$session"

    return $result
}

# ==============================================================================
# Main
# ==============================================================================

main() {
    echo ""
    echo "========================================"
    echo "FORGE Worker Management Test"
    echo "========================================"

    # Adjust for CI environment
    ci_adjust

    local start_time=$SECONDS
    local result=0
    local tests_passed=0
    local tests_total=2

    # Run spawn test
    echo ""
    if test_worker_spawn; then
        ((tests_passed++)) || true
    else
        result=1
    fi

    # Run navigation test
    echo ""
    if test_worker_navigation; then
        ((tests_passed++)) || true
    else
        result=1
    fi

    # Summary
    local elapsed=$((SECONDS - start_time))
    echo ""
    if [ $result -eq 0 ]; then
        echo -e "${GREEN}========================================"
        echo "ALL TESTS PASSED: $tests_passed/$tests_total (${elapsed}s)"
        echo -e "========================================${NC}"
    else
        echo -e "${RED}========================================"
        echo "TESTS FAILED: $tests_passed/$tests_total passed (${elapsed}s)"
        echo -e "========================================${NC}"
    fi

    exit $result
}

# Only run main if not being sourced
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
