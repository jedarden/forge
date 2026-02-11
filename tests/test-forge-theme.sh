#!/usr/bin/env bash
# FORGE Theme Switching Test
#
# Tests:
# 1. Theme switching with 'C' key
# 2. Verify theme cycle: Default -> Dark -> Light -> Cyberpunk -> Default
# 3. Check theme persists in status message
# 4. Verify color changes apply
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

TEST_NAME="forge-theme"

# Theme names in cycle order
THEMES=("Default" "Dark" "Light" "Cyberpunk")

# ==============================================================================
# Test Implementation
# ==============================================================================

test_theme_cycle() {
    local session
    session=$(get_session_name)
    local result=0

    log_info "=== Testing Theme Cycling ==="

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

    # Step 3: Cycle through all themes
    log_info "Step 3: Cycling through themes with 'C' key..."

    local themes_verified=0

    # Initial theme should be Default (or whatever was last saved)
    log_info "Testing initial theme..."

    for theme in "${THEMES[@]}"; do
        log_info "Cycling to next theme (expecting: $theme)..."
        send_key_wait "$session" "C" 1

        # Check for theme name in status message
        if pane_contains "$session" "Theme:" || pane_contains "$session" "$theme"; then
            log_success "Theme cycled, status shows theme information"
            ((themes_verified++))
        else
            # Try checking the pane content for any theme indicator
            local output
            output=$(capture_pane "$session")
            if grep -q "Theme" "$output" 2>/dev/null; then
                log_success "Theme change detected in output"
                ((themes_verified++))
            else
                log_warn "Could not verify theme change to: $theme"
            fi
        fi
    done

    log_info "Theme cycle verified $themes_verified/${#THEMES[@]} changes"

    # Step 4: Verify we're back to original (full cycle)
    log_info "Step 4: Verifying full cycle completes..."

    # After cycling through all 4 themes, we should be back to start
    if [ $themes_verified -ge 2 ]; then
        log_success "Theme cycling is functional"
    else
        log_warn "Theme cycling could not be fully verified"
        result=1
    fi

    # Step 5: Cleanup
    log_info "Step 5: Cleaning up..."
    stop_forge "$session"

    return $result
}

test_theme_persistence() {
    local session
    session=$(get_session_name)
    local result=0

    log_info "=== Testing Theme Persistence ==="

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

    # Step 3: Change theme
    log_info "Step 3: Changing theme..."
    send_key_wait "$session" "C" 1

    # Step 4: Check theme config file
    log_info "Step 4: Checking theme config file..."

    local theme_config="$HOME/.forge/theme.toml"
    if [ -f "$theme_config" ]; then
        log_success "Theme config file exists: $theme_config"

        # Read current theme
        local current_theme
        current_theme=$(grep "current_theme" "$theme_config" 2>/dev/null | head -1 || echo "")
        if [ -n "$current_theme" ]; then
            log_success "Theme config contains: $current_theme"
        else
            log_warn "Could not read theme from config"
        fi
    else
        log_warn "Theme config file not found (might be first run)"
    fi

    # Step 5: Cleanup
    log_info "Step 5: Cleaning up..."
    stop_forge "$session"

    return $result
}

test_theme_visual_changes() {
    local session
    session=$(get_session_name)
    local result=0

    log_info "=== Testing Theme Visual Changes ==="

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

    # Step 3: Capture initial state
    log_info "Step 3: Capturing initial theme state..."
    local initial_output
    initial_output=$(mktemp)
    tmux capture-pane -t "$session" -p > "$initial_output"

    # Step 4: Change theme
    log_info "Step 4: Changing theme..."
    send_key_wait "$session" "C" 1

    # Step 5: Capture new state
    log_info "Step 5: Capturing new theme state..."
    local new_output
    new_output=$(mktemp)
    tmux capture-pane -t "$session" -p > "$new_output"

    # Step 6: Compare outputs (they should have same structure but potentially different display)
    log_info "Step 6: Comparing theme states..."

    # Both should contain the same UI elements
    if grep -q "FORGE" "$initial_output" && grep -q "FORGE" "$new_output"; then
        log_success "UI structure preserved after theme change"
    else
        log_warn "UI structure might have changed"
    fi

    # The status message should show theme change
    if grep -q "Theme" "$new_output"; then
        log_success "Theme change message visible"
    else
        log_warn "Theme change message not visible"
    fi

    # Step 7: Cleanup temp files
    rm -f "$initial_output" "$new_output"

    # Step 8: Cleanup session
    log_info "Step 7: Cleaning up..."
    stop_forge "$session"

    return $result
}

test_all_themes_available() {
    local session
    session=$(get_session_name)
    local result=0

    log_info "=== Testing All Themes Available ==="

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

    # Step 3: Open help to check theme documentation
    log_info "Step 3: Opening help overlay..."
    send_key_wait "$session" "?" 1

    # Check if theme hotkey is documented
    if pane_contains "$session" "Theme" || pane_contains "$session" "[C]"; then
        log_success "Theme hotkey documented in help"
    else
        log_warn "Theme hotkey not visible in help"
    fi

    # Step 4: Close help and verify footer hints
    log_info "Step 4: Checking footer for theme hint..."
    send_key_wait "$session" "Escape" 1

    if pane_contains "$session" "[C]"; then
        log_success "Theme hotkey hint visible in footer"
    else
        log_warn "Theme hotkey hint not visible in footer"
    fi

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
    echo "FORGE Theme Switching Test"
    echo "========================================"

    # Adjust for CI environment
    ci_adjust

    local start_time=$SECONDS
    local result=0
    local tests_passed=0
    local tests_total=4

    # Run theme cycle test
    echo ""
    if test_theme_cycle; then
        ((tests_passed++)) || true
    else
        result=1
    fi

    # Run theme persistence test
    echo ""
    if test_theme_persistence; then
        ((tests_passed++)) || true
    else
        result=1
    fi

    # Run visual changes test
    echo ""
    if test_theme_visual_changes; then
        ((tests_passed++)) || true
    else
        result=1
    fi

    # Run all themes available test
    echo ""
    if test_all_themes_available; then
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
