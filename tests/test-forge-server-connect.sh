#!/usr/bin/env bash
# Comprehensive test runner for FORGE server and client modes
# Runs both server and connection tests sequentially

set -euo pipefail

# Source test helpers
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/test-helpers.sh"

# Test configuration
export TEST_PORT="${TEST_PORT:-19989}"
export TEST_BIND_ADDRESS="${TEST_BIND_ADDRESS:-127.0.0.1}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# ==============================================================================
# Print header
# ==============================================================================
print_header() {
    local title="$1"
    echo ""
    echo "========================================"
    echo "$title"
    echo "========================================"
    echo ""
}

# ==============================================================================
# Run test suite
# ==============================================================================
run_test_suite() {
    local test_script="$1"
    local suite_name="$2"

    print_header "$suite_name"

    if [ ! -f "$test_script" ]; then
        log_fail "Test script not found: $test_script"
        return 1
    fi

    if [ ! -x "$test_script" ]; then
        log_warn "Test script not executable, making executable..."
        chmod +x "$test_script"
    fi

    # Run the test script
    if "$test_script"; then
        log_success "$suite_name PASSED"
        return 0
    else
        log_fail "$suite_name FAILED"
        return 1
    fi
}

# ==============================================================================
# Main test runner
# ==============================================================================
main() {
    print_header "FORGE Server & Client Mode Integration Tests"

    log_info "Test Configuration:"
    log_info "  Test Port: $TEST_PORT"
    log_info "  Bind Address: $TEST_BIND_ADDRESS"
    log_info "  Working Directory: $(pwd)"
    echo ""

    # Check if forge binary exists
    local forge_bin="./target/release/forge"
    if [ ! -f "$forge_bin" ]; then
        log_fail "Forge binary not found at $forge_bin"
        log_info "Run: cargo build --release"
        return 1
    fi

    log_success "Forge binary found: $forge_bin"
    echo ""

    # Check if tmux is available
    if ! command -v tmux &> /dev/null; then
        log_fail "tmux is required for integration tests"
        return 1
    fi

    log_success "tmux is available"
    echo ""

    # Track results
    local total_suites=0
    local passed_suites=0
    local failed_suites=0

    # Define test suites
    declare -a test_suites=(
        "$SCRIPT_DIR/test-forge-server.sh:Server Mode Tests"
        "$SCRIPT_DIR/test-forge-connect.sh:Client Mode Tests"
    )

    # Run each test suite
    for suite_info in "${test_suites[@]}"; do
        IFS=: read -r script name <<< "$suite_info"

        ((total_suites++)) || true

        echo ""
        if run_test_suite "$script" "$name"; then
            ((passed_suites++)) || true
        else
            ((failed_suites++)) || true
        fi
    done

    # Final summary
    print_header "Final Summary"

    log_info "Total Test Suites: $total_suites"
    log_success "Passed: $passed_suites"

    if [ "$failed_suites" -gt 0 ]; then
        log_fail "Failed: $failed_suites"
        echo ""
        log_fail "Some test suites failed"
        return 1
    else
        echo ""
        log_success "All test suites passed!"
        return 0
    fi
}

# Run main function
main "$@"
