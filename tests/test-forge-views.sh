#!/usr/bin/env bash
# FORGE View Navigation Test
#
# Tests:
# 1. Test all view hotkeys: w, t, c, m, l, o, :
# 2. Verify each view renders correctly
# 3. Test Tab/Shift+Tab navigation
# 4. Validate help panel (? key)
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

TEST_NAME="forge-views"

# View definitions with expected content
# Format: "key:view_name:expected_text"
# NOTE: lowercase 'o' is spawn Opus worker, NOT Overview!
# Overview can only be accessed via 'O' (uppercase)
declare -A VIEW_TESTS=(
    ["O"]="Overview:Worker Pool"
    ["w"]="Workers:Worker Pool"
    ["W"]="Workers:Worker Pool"
    ["t"]="Tasks:Task Queue"
    ["T"]="Tasks:Task Queue"
    ["c"]="Costs:Cost"
    ["m"]="Metrics:Metrics"
    ["l"]="Logs:Activity Log"
    ["a"]="Logs:Activity Log"
)

# ==============================================================================
# Test Implementation
# ==============================================================================

test_view_hotkeys() {
    local session
    session=$(get_session_name)
    local result=0
    local passed=0
    local failed=0

    log_info "=== Testing View Hotkey Navigation ==="

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

    # Step 3: Test each view hotkey
    log_info "Step 3: Testing view hotkeys..."

    for key in "${!VIEW_TESTS[@]}"; do
        local view_info="${VIEW_TESTS[$key]}"
        local view_name="${view_info%%:*}"
        local expected_text="${view_info#*:}"

        log_info "Testing '$key' key for $view_name view..."
        send_key_wait "$session" "$key" 1

        if pane_contains "$session" "$expected_text"; then
            log_success "'$key' -> $view_name: Found '$expected_text'"
            ((passed++))
        else
            log_fail "'$key' -> $view_name: Expected '$expected_text' not found"
            ((failed++))
            result=1
        fi
    done

    # Step 4: Test Chat mode activation
    log_info "Step 4: Testing ':' for Chat mode..."
    send_key_wait "$session" ":" 1

    if pane_contains "$session" "Chat" || pane_contains "$session" "Input"; then
        log_success "':' -> Chat: Found chat interface"
        ((passed++))
    else
        log_fail "':' -> Chat: Chat interface not found"
        ((failed++))
        result=1
    fi

    # Exit chat mode
    send_keys "$session" "Escape"
    sleep 0.5

    log_info "View hotkey tests: $passed passed, $failed failed"

    # Step 5: Cleanup
    log_info "Step 5: Cleaning up..."
    stop_forge "$session"

    return $result
}

test_tab_navigation() {
    local session
    session=$(get_session_name)
    local result=0

    log_info "=== Testing Tab Navigation ==="

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

    # Starting view should be Overview
    log_info "Step 3: Verifying initial Overview view..."
    if ! pane_contains "$session" "FORGE"; then
        log_fail "Initial view should show FORGE header"
        result=1
    fi

    # Step 4: Test Tab to cycle forward
    log_info "Step 4: Testing Tab key to cycle views..."

    # Views cycle: Overview -> Workers -> Tasks -> Costs -> Metrics -> Logs -> Chat -> Overview
    local views=("Workers" "Tasks" "Costs" "Metrics" "Logs" "Chat")
    local tab_passed=0

    for expected in "${views[@]}"; do
        send_keys "$session" "Tab"
        sleep 0.5

        # Each view should have some identifying content
        local found=false
        case "$expected" in
            "Workers") pane_contains "$session" "Worker" && found=true ;;
            "Tasks") pane_contains "$session" "Task" && found=true ;;
            "Costs") pane_contains "$session" "Cost" && found=true ;;
            "Metrics") pane_contains "$session" "Metric" && found=true ;;
            "Logs") pane_contains "$session" "Activity" && found=true ;;
            "Chat") pane_contains "$session" "Chat" && found=true ;;
        esac

        if $found; then
            log_success "Tab -> $expected: View found"
            ((tab_passed++))
        else
            log_warn "Tab -> $expected: View content not verified"
        fi
    done

    log_info "Tab navigation verified $tab_passed/${#views[@]} views"

    # Step 5: Test Shift+Tab to cycle backward
    log_info "Step 5: Testing Shift+Tab key for reverse cycling..."

    # From Chat, Shift+Tab should go to Logs
    send_keys "$session" "BTab"  # BTab is Shift+Tab in tmux
    sleep 0.5

    if pane_contains "$session" "Activity" || pane_contains "$session" "Log"; then
        log_success "Shift+Tab reversed to Logs view"
    else
        log_warn "Could not verify Shift+Tab reverse navigation"
    fi

    # Step 6: Cleanup
    log_info "Step 6: Cleaning up..."
    stop_forge "$session"

    return $result
}

test_help_overlay() {
    local session
    session=$(get_session_name)
    local result=0

    log_info "=== Testing Help Overlay ==="

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

    # Step 3: Open help with '?'
    log_info "Step 3: Opening help overlay with '?' key..."
    send_key_wait "$session" "?" 1

    # Check for help content
    if assert_pane_contains "$session" "Help" "Help overlay should be visible"; then
        log_success "Help overlay opened"
    else
        log_fail "Help overlay not visible"
        result=1
    fi

    # Check for hotkey documentation
    if pane_contains "$session" "Tab" || pane_contains "$session" "Navigation"; then
        log_success "Help shows navigation information"
    else
        log_warn "Navigation info not visible in help"
    fi

    if pane_contains "$session" "Quit" || pane_contains "$session" "Esc"; then
        log_success "Help shows quit/escape information"
    else
        log_warn "Quit/escape info not visible in help"
    fi

    # Step 4: Close help with any key
    log_info "Step 4: Closing help overlay..."
    send_key_wait "$session" "Escape" 1

    # Help should be closed - Overview content should be visible
    if pane_contains "$session" "Worker Pool"; then
        log_success "Help overlay closed, main view visible"
    else
        log_warn "Could not verify help overlay closed"
    fi

    # Step 5: Cleanup
    log_info "Step 5: Cleaning up..."
    stop_forge "$session"

    return $result
}

test_footer_hotkeys() {
    local session
    session=$(get_session_name)
    local result=0

    log_info "=== Testing Footer Hotkey Hints ==="

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

    # Step 3: Check for footer hotkey hints
    log_info "Step 3: Verifying footer hotkey hints..."

    local hints=("[o]" "[w]" "[t]" "[c]" "[m]" "[l]" "[?]" "[q]")
    local hints_found=0

    for hint in "${hints[@]}"; do
        if pane_contains "$session" "$hint"; then
            log_success "Footer shows hint: $hint"
            ((hints_found++))
        else
            log_warn "Footer missing hint: $hint"
        fi
    done

    if [ $hints_found -ge 4 ]; then
        log_success "Footer shows sufficient hotkey hints ($hints_found/${#hints[@]})"
    else
        log_fail "Footer missing too many hotkey hints ($hints_found/${#hints[@]})"
        result=1
    fi

    # Step 4: Cleanup
    log_info "Step 4: Cleaning up..."
    stop_forge "$session"

    return $result
}

# ==============================================================================
# Main
# ==============================================================================

main() {
    echo ""
    echo "========================================"
    echo "FORGE View Navigation Test"
    echo "========================================"

    # Adjust for CI environment
    ci_adjust

    local start_time=$SECONDS
    local result=0
    local tests_passed=0
    local tests_total=4

    # Run view hotkey tests
    echo ""
    if test_view_hotkeys; then
        ((tests_passed++)) || true
    else
        result=1
    fi

    # Run tab navigation tests
    echo ""
    if test_tab_navigation; then
        ((tests_passed++)) || true
    else
        result=1
    fi

    # Run help overlay tests
    echo ""
    if test_help_overlay; then
        ((tests_passed++)) || true
    else
        result=1
    fi

    # Run footer hotkey tests
    echo ""
    if test_footer_hotkeys; then
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
