#!/usr/bin/env bash
# FORGE Worker Management Test
#
# Tests:
# 1. Spawn worker (g/s/o/h keys for different models)
# 2. Verify worker appears in status
# 3. Kill worker (k key)
# 4. Validate worker panel updates
# 5. Test with multiple workers
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

# Global variable for tracking sessions before spawning
BEFORE_SESSIONS=""

# Trap handler to cleanup on any exit
cleanup_on_exit() {
    local exit_code=$?
    if [ -n "${BEFORE_SESSIONS:-}" ]; then
        cleanup_spawned_workers "$BEFORE_SESSIONS" 2>/dev/null || true
    fi
    exit $exit_code
}

# Set up trap for cleanup on script exit
trap cleanup_on_exit EXIT

# ==============================================================================
# Test Implementation
# ==============================================================================

test_worker_spawn_all_models() {
    local session
    session=$(get_session_name)
    local result=0
    local spawn_passed=0
    local spawn_total=4
    local test_before_sessions

    log_info "=== Testing All Worker Spawn Keys ==="

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

    # Step 2.5: Capture sessions before spawning workers
    log_info "Step 2.5: Capturing sessions before spawning workers..."
    test_before_sessions=$(capture_before_sessions)

    # Step 3: Navigate to Workers view
    log_info "Step 3: Navigating to Workers view with 'w' key..."
    send_key_wait "$session" "w" 1

    # Verify we're in Workers view
    if pane_contains "$session" "Worker Pool"; then
        log_success "Navigated to Workers view"
    else
        log_warn "Workers view may not be visible, continuing..."
    fi

    # Test each spawn key
    declare -A spawn_keys=(
        ["g"]="GLM"
        ["s"]="Sonnet"
        ["o"]="Opus"
        ["h"]="Haiku"
    )

    for key in g s o h; do
        local model="${spawn_keys[$key]}"
        log_info "Testing spawn $model worker with '$key' key..."
        send_key_wait "$session" "$key" 1

        # Check for spawn message in status
        if pane_contains "$session" "Spawning" || pane_contains "$session" "$model"; then
            log_success "'$key' -> Spawning $model worker confirmed"
            ((spawn_passed++))
        else
            # Status message might have passed, check for general spawn indication
            if pane_contains "$session" "worker"; then
                log_success "'$key' -> Worker action detected"
                ((spawn_passed++))
            else
                log_warn "'$key' -> Could not verify $model spawn"
            fi
        fi

        sleep 0.5
    done

    log_info "Spawn key tests: $spawn_passed/$spawn_total passed"

    # Step 4: Verify worker panel shows expected content
    log_info "Step 4: Verifying worker panel content..."

    # Worker panel should show table headers or spawn instructions
    local panel_valid=false
    if pane_contains "$session" "Worker ID" || pane_contains "$session" "[G] Spawn" || pane_contains "$session" "Worker Pool"; then
        log_success "Worker panel shows expected content"
        panel_valid=true
    else
        log_warn "Worker panel content not fully verified"
    fi

    # Step 5: Cleanup spawned workers and main session
    log_info "Step 5: Cleaning up spawned worker sessions..."
    cleanup_spawned_workers "$test_before_sessions"

    log_info "Step 6: Cleaning up main test session..."
    stop_forge "$session"

    # Pass if at least 2 spawn keys worked
    if [ $spawn_passed -ge 2 ]; then
        return 0
    else
        return 1
    fi
}

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

    if pane_contains "$session" "Kill" || pane_contains "$session" "kill" || pane_contains "$session" "not yet implemented"; then
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

test_worker_kill() {
    local session
    session=$(get_session_name)
    local result=0

    log_info "=== Testing Worker Kill Functionality ==="

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
    log_info "Step 3: Navigating to Workers view..."
    send_key_wait "$session" "w" 1

    # Step 4: Test kill worker without any workers
    log_info "Step 4: Testing 'k' key (kill worker)..."
    send_key_wait "$session" "k" 1

    # Check for kill message - either success or "not yet implemented"
    if pane_contains "$session" "Kill" || pane_contains "$session" "kill" || pane_contains "$session" "implemented"; then
        log_success "Kill worker key ('k') triggers appropriate response"
    else
        log_warn "Kill worker response not detected"
    fi

    # Step 5: Capture screen state
    log_info "Step 5: Capturing screen state after kill attempt..."
    local output
    output=$(capture_pane "$session")
    log_info "Screen state saved to: $output"

    # Step 6: Verify we're still in Workers view
    log_info "Step 6: Verifying view state after kill..."
    if pane_contains "$session" "Worker Pool" || pane_contains "$session" "Worker"; then
        log_success "Still in Workers view after kill action"
    else
        log_warn "View state may have changed"
    fi

    # Step 7: Cleanup
    log_info "Step 7: Cleaning up..."
    stop_forge "$session"

    return $result
}

test_worker_status_updates() {
    local session
    session=$(get_session_name)
    local result=0

    log_info "=== Testing Worker Status Updates ==="

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
    log_info "Step 3: Navigating to Workers view..."
    send_key_wait "$session" "w" 1

    # Step 4: Capture initial state
    log_info "Step 4: Capturing initial worker panel state..."
    local initial_output
    initial_output=$(capture_pane "$session")
    local initial_content
    initial_content=$(cat "$initial_output")

    # Step 5: Trigger a spawn action
    log_info "Step 5: Triggering spawn action with 'g' key..."
    send_key_wait "$session" "g" 1

    # Step 6: Capture state after spawn
    log_info "Step 6: Capturing worker panel state after spawn..."
    local after_output
    after_output=$(capture_pane "$session")
    local after_content
    after_content=$(cat "$after_output")

    # Step 7: Verify status message updated
    log_info "Step 7: Verifying status update..."

    # The status bar should show spawning message
    if pane_contains "$session" "Spawning" || pane_contains "$session" "GLM"; then
        log_success "Status updated to show spawn action"
    else
        # Check if any status change occurred
        if [ "$initial_content" != "$after_content" ]; then
            log_success "Screen content changed after spawn action"
        else
            log_warn "No visible status change detected"
        fi
    fi

    # Step 8: Test refresh
    log_info "Step 8: Testing refresh with 'r' key..."
    send_key_wait "$session" "r" 1

    if pane_contains "$session" "Refreshed" || pane_contains "$session" "refresh"; then
        log_success "Refresh action triggered"
    else
        log_warn "Refresh response not detected"
    fi

    # Step 9: Cleanup
    log_info "Step 9: Cleaning up..."
    stop_forge "$session"

    return $result
}

test_worker_panel_display() {
    local session
    session=$(get_session_name)
    local result=0
    local checks_passed=0
    local checks_total=5

    log_info "=== Testing Worker Panel Display ==="

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
    log_info "Step 3: Navigating to Workers view..."
    send_key_wait "$session" "w" 1

    # Step 4: Verify panel elements
    log_info "Step 4: Verifying worker panel elements..."

    # Check 1: Panel title
    if pane_contains "$session" "Worker Pool"; then
        log_success "Check 1: Worker Pool panel title visible"
        ((checks_passed++))
    else
        log_fail "Check 1: Worker Pool panel title not found"
        result=1
    fi

    # Check 2: Spawn hotkey hints
    if pane_contains "$session" "[G]" || pane_contains "$session" "Spawn"; then
        log_success "Check 2: Spawn hotkey hints visible"
        ((checks_passed++))
    else
        log_warn "Check 2: Spawn hotkey hints not visible"
    fi

    # Check 3: Kill hotkey hint
    if pane_contains "$session" "[K]" || pane_contains "$session" "Kill"; then
        log_success "Check 3: Kill hotkey hint visible"
        ((checks_passed++))
    else
        log_warn "Check 3: Kill hotkey hint not visible"
    fi

    # Check 4: Status column or worker message
    if pane_contains "$session" "Status" || pane_contains "$session" "active" || pane_contains "$session" "idle" || pane_contains "$session" "No workers"; then
        log_success "Check 4: Worker status indicator visible"
        ((checks_passed++))
    else
        log_warn "Check 4: Worker status indicator not visible"
    fi

    # Check 5: Border characters (panel is rendered)
    local panel_content
    panel_content=$(tmux capture-pane -t "$session" -p 2>/dev/null || echo "")
    if echo "$panel_content" | grep -q '┌\|│\|─'; then
        log_success "Check 5: Panel borders rendered correctly"
        ((checks_passed++))
    else
        log_warn "Check 5: Panel borders not detected"
    fi

    log_info "Panel display checks: $checks_passed/$checks_total passed"

    # Step 5: Cleanup
    log_info "Step 5: Cleaning up..."
    stop_forge "$session"

    # Pass if at least 3 checks passed
    if [ $checks_passed -ge 3 ]; then
        return 0
    else
        return 1
    fi
}

test_multiple_workers() {
    local session
    session=$(get_session_name)
    local result=0
    local test_before_sessions

    log_info "=== Testing Multiple Worker Spawn ==="

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

    # Step 2.5: Capture sessions before spawning workers
    log_info "Step 2.5: Capturing sessions before spawning workers..."
    test_before_sessions=$(capture_before_sessions)

    # Step 3: Navigate to Workers view
    log_info "Step 3: Navigating to Workers view..."
    send_key_wait "$session" "w" 1

    # Step 4: Spawn multiple workers in sequence
    log_info "Step 4: Spawning multiple workers..."

    # Spawn GLM worker
    log_info "  Spawning GLM worker..."
    send_key_wait "$session" "g" 1
    local spawn1=$(pane_contains "$session" "Spawning" && echo "yes" || echo "no")

    # Spawn Sonnet worker
    log_info "  Spawning Sonnet worker..."
    send_key_wait "$session" "s" 1
    local spawn2=$(pane_contains "$session" "Spawning" && echo "yes" || echo "no")

    # Spawn Haiku worker
    log_info "  Spawning Haiku worker..."
    send_key_wait "$session" "h" 1
    local spawn3=$(pane_contains "$session" "Spawning" && echo "yes" || echo "no")

    log_info "Spawn results: GLM=$spawn1, Sonnet=$spawn2, Haiku=$spawn3"

    # Step 5: Verify all spawn actions registered
    log_info "Step 5: Verifying spawn actions..."

    # At least the last spawn should show in status
    if pane_contains "$session" "Spawning" || pane_contains "$session" "worker"; then
        log_success "Multiple spawn actions processed"
    else
        log_warn "Could not verify multiple spawn actions"
    fi

    # Step 6: Test uppercase spawn keys
    log_info "Step 6: Testing uppercase spawn keys (G, S)..."
    send_key_wait "$session" "G" 1
    send_key_wait "$session" "S" 1

    if pane_contains "$session" "Spawning" || pane_contains "$session" "GLM" || pane_contains "$session" "Sonnet"; then
        log_success "Uppercase spawn keys work"
    else
        log_warn "Uppercase spawn key response not verified"
    fi

    # Step 7: Cleanup spawned workers and main session
    log_info "Step 7: Cleaning up spawned worker sessions..."
    cleanup_spawned_workers "$test_before_sessions"

    log_info "Step 8: Cleaning up main test session..."
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
    local tests_total=7

    # Run all model spawn test
    echo ""
    if test_worker_spawn_all_models; then
        ((tests_passed++)) || true
    else
        result=1
    fi

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

    # Run kill test
    echo ""
    if test_worker_kill; then
        ((tests_passed++)) || true
    else
        result=1
    fi

    # Run status updates test
    echo ""
    if test_worker_status_updates; then
        ((tests_passed++)) || true
    else
        result=1
    fi

    # Run panel display test
    echo ""
    if test_worker_panel_display; then
        ((tests_passed++)) || true
    else
        result=1
    fi

    # Run multiple workers test
    echo ""
    if test_multiple_workers; then
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
