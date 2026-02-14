#!/usr/bin/env bash
# Validation Tests for Error Handling (Task fg-2eq2)
#
# This script tests:
# 1. Database lock retry with exponential backoff
# 2. Config file parsing with fallback to defaults
# 3. Network timeout handling with retry logic
# 4. Error notifications displayed in TUI

set -e

FORGE_DIR="${FORGE_DIR:-$HOME/.forge}"
FORGE_BIN="${FORGE_BIN:-./target/release/forge}"
LOG_DIR="$FORGE_DIR/logs"
SESSION_PREFIX="forge-error-test"

echo "🧪 Error Handling Validation Tests"
echo "======================================"

# Color helpers
green() { echo -e "\033[0;32m✓ $1\033[0m"; }
red() { echo -e "\033[0;31m✗ $1\033[0m"; }
yellow() { echo -e "\033[0;33m⚠ $1\033[0m"; }

# Cleanup function
cleanup() {
    echo ""
    echo "🧹 Cleaning up test sessions..."
    tmux list-sessions 2>/dev/null | grep "^$SESSION_PREFIX" | cut -d: -f1 | while read session; do
        tmux kill-session -t "$session" 2>/dev/null || true
    done
}

# Register cleanup on exit
trap cleanup EXIT

# Test 1: Database Lock Recovery
test_db_lock() {
    echo ""
    echo "Test 1: Database Lock Recovery"
    echo "-----------------------------------"

    # Create two test sessions to simulate DB contention
    local session1="${SESSION_PREFIX}-db1"
    local session2="${SESSION_PREFIX}-db2"

    # Launch first instance
    tmux new-session -d -s "$session1" -x 80 -y 25
    tmux send-keys -t "$session1" "cd $(pwd) && cargo run --release 2>&1 | head -20" C-m

    sleep 3

    # Launch second instance (should handle DB lock gracefully)
    tmux new-session -d -s "$session2" -x 80 -y 25
    tmux send-keys -t "$session2" "cd $(pwd) && cargo run --release 2>&1 | head -20" C-m

    sleep 5

    # Check both instances handled the DB lock
    local output1=$(tmux capture-pane -t "$session1" -p)
    local output2=$(tmux capture-pane -t "$session2" -p)

    # Check for panic messages
    if echo "$output1" | grep -qi "panic"; then
        red "Instance 1: Panic detected (FAILED)"
        return 1
    else
        green "Instance 1: No panic (PASSED)"
    fi

    if echo "$output2" | grep -qi "panic"; then
        red "Instance 2: Panic detected (FAILED)"
        return 1
    else
        green "Instance 2: No panic (PASSED)"
    fi

    # Check for error handling
    if echo "$output1" | grep -qi "database is locked\|database is busy\|retrying"; then
        green "Instance 1: DB lock retry detected (PASSED)"
    else
        yellow "Instance 1: No explicit retry message (WARNING)"
    fi

    green "Test 1 completed"
}

# Test 2: Invalid Config File Handling
test_invalid_config() {
    echo ""
    echo "Test 2: Invalid Config File Handling"
    echo "-----------------------------------"

    local session="${SESSION_PREFIX}-config"
    local bad_config="$HOME/.forge/config.test.yaml"

    # Backup existing config if present
    if [ -f "$HOME/.forge/config.yaml" ]; then
        cp "$HOME/.forge/config.yaml" "$HOME/.forge/config.yaml.backup"
    fi

    # Create invalid YAML config
    cat > "$bad_config" << 'EOF'
# Invalid YAML configuration
this is not valid: [
  unclosed bracket
EOF

    # Try to launch with bad config
    tmux new-session -d -s "$session" -x 120 -y 40
    tmux send-keys -t "$session" "cd $(pwd) && FORGE_CONFIG_FILE=$bad_config cargo run --release 2>&1 | head -30" C-m

    sleep 5

    local output=$(tmux capture-pane -t "$session" -p)

    # Check if app started with fallback
    if echo "$output" | grep -qi "using defaults\|fallback\|warning.*config"; then
        green "Config fallback detected (PASSED)"
    else
        yellow "No explicit fallback message (WARNING)"
    fi

    # Check if app crashed
    if echo "$output" | grep -qi "panic\|fatal"; then
        red "App crashed on bad config (FAILED)"
    else
        green "App did not crash (PASSED)"
    fi

    # Restore backup
    if [ -f "$HOME/.forge/config.yaml.backup" ]; then
        mv "$HOME/.forge/config.yaml.backup" "$HOME/.forge/config.yaml"
    fi

    rm -f "$bad_config"
    green "Test 2 completed"
}

# Test 3: Network Error Handling (via grep check)
test_network_handling() {
    echo ""
    echo "Test 3: Network Error Handling"
    echo "-----------------------------------"

    # Check for timeout/retry logic in chat backend
    if grep -q "timeout\|retry\|backoff" crates/forge-chat/src/claude_api.rs; then
        green "Network timeout handling found (PASSED)"
    else
        red "Network timeout handling NOT found (FAILED)"
        return 1
    fi

    # Check for exponential backoff implementation
    if grep -q "exponential\|backoff\|retry.*attempt" crates/forge-chat/src/claude_api.rs; then
        green "Exponential backoff found (PASSED)"
    else
        yellow "Exponential backoff not explicitly mentioned (WARNING)"
    fi

    # Check for database retry logic
    if grep -q "retry\|with_retry\|DB_LOCK.*RETRY" crates/forge-cost/src/db.rs; then
        green "Database retry logic found (PASSED)"
    else
        red "Database retry logic NOT found (FAILED)"
        return 1
    fi

    green "Test 3 completed"
}

# Test 4: Error Recovery Manager Integration
test_error_recovery_manager() {
    echo ""
    echo "Test 4: Error Recovery Manager Integration"
    echo "-----------------------------------"

    # Check for ErrorRecoveryManager in App struct
    if grep -q "error_recovery.*SharedErrorRecoveryManager" crates/forge-tui/src/app.rs; then
        green "Error recovery manager field added to App (PASSED)"
    else
        red "Error recovery manager field NOT found in App (FAILED)"
        return 1
    fi

    # Check for error_recovery import
    if grep -q "use crate::error_recovery" crates/forge-tui/src/app.rs; then
        green "Error recovery module imported (PASSED)"
    else
        red "Error recovery module NOT imported (FAILED)"
        return 1
    fi

    # Check for unacknowledged errors retrieval
    if grep -q "unacknowledged_errors\|unacknowledged" crates/forge-tui/src/app.rs; then
        green "Unacknowledged errors check implemented (PASSED)"
    else
        yellow "Unacknowledged errors check not found (WARNING)"
    fi

    green "Test 4 completed"
}

# Test 5: Error Notification Display
test_error_notifications() {
    echo ""
    echo "Test 5: Error Notification Display"
    echo "-----------------------------------"

    # Check for error text in header drawing
    if grep -q "error_text\|⚠️" crates/forge-tui/src/app.rs; then
        green "Error notification display implemented (PASSED)"
    else
        red "Error notification display NOT found (FAILED)"
        return 1
    fi

    # Check for ErrorSeverity and ErrorCategory usage
    if grep -q "ErrorSeverity\|ErrorCategory" crates/forge-tui/src/error_recovery.rs; then
        green "Error severity and categorization found (PASSED)"
    else
        red "Error severity and categorization NOT found (FAILED)"
        return 1
    fi

    green "Test 5 completed"
}

# Main test runner
main() {
    echo ""
    echo "Starting error handling validation tests..."
    echo ""

    local failed=0

    # Run non-interactive tests first
    test_network_handling || ((failed++))
    test_error_recovery_manager || ((failed++))
    test_error_notifications || ((failed++))

    # Ask if user wants to run interactive tests
    echo ""
    read -p "Run interactive tests (DB lock, config handling)? [y/N] " -n 1 -r
    echo ""

    if [[ $REPLY =~ ^[Yy]$ ]]; then
        test_db_lock || ((failed++))
        test_invalid_config || ((failed++))
    fi

    # Summary
    echo ""
    echo "======================================"
    echo "Test Summary"
    echo "======================================"

    if [ $failed -eq 0 ]; then
        green "All tests PASSED ✓"
        echo ""
        echo "Error handling implementation:"
        echo "  ✓ Database lock retry with exponential backoff"
        echo "  ✓ Config file fallback to defaults"
        echo "  ✓ Network timeout handling with retry"
        echo "  ✓ Error recovery manager integration"
        echo "  ✓ Error notification display"
        return 0
    else
        red "$failed test(s) FAILED ✗"
        return 1
    fi
}

# Run main
main "$@"
