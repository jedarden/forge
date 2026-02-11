#!/usr/bin/env bash
# FORGE Test Suite Runner
#
# Executes all test scripts and generates a summary report.
#
# Usage:
#   ./run-all-tests.sh           # Run all tests
#   ./run-all-tests.sh --quick   # Run only quick tests (skip chat)
#   ./run-all-tests.sh --verbose # Show detailed output
#
# Exit codes:
#   0 - All tests passed
#   1 - One or more tests failed

set -euo pipefail

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Source helpers for colors
source "$SCRIPT_DIR/lib/test-helpers.sh"

# ==============================================================================
# Configuration
# ==============================================================================

# Test scripts in execution order
TEST_SCRIPTS=(
    "test-forge-views.sh"
    "test-forge-theme.sh"
    "test-forge-workers.sh"
    "test-forge-chat.sh"
)

# Quick tests (skip slow chat test)
QUICK_TESTS=(
    "test-forge-views.sh"
    "test-forge-theme.sh"
    "test-forge-workers.sh"
)

# Results tracking
declare -a TEST_RESULTS
declare -a TEST_TIMES
declare -a TEST_NAMES

# Options
VERBOSE=false
QUICK=false

# ==============================================================================
# Functions
# ==============================================================================

usage() {
    echo "FORGE Test Suite Runner"
    echo ""
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --quick     Run only quick tests (skip chat test)"
    echo "  --verbose   Show detailed test output"
    echo "  --help      Show this help message"
    echo ""
    echo "Tests:"
    for test in "${TEST_SCRIPTS[@]}"; do
        echo "  - $test"
    done
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --quick)
                QUICK=true
                shift
                ;;
            --verbose)
                VERBOSE=true
                shift
                ;;
            --help|-h)
                usage
                exit 0
                ;;
            *)
                echo "Unknown option: $1"
                usage
                exit 1
                ;;
        esac
    done
}

run_single_test() {
    local test_script="$1"
    local test_path="$SCRIPT_DIR/$test_script"

    if [ ! -f "$test_path" ]; then
        log_fail "Test script not found: $test_script"
        return 1
    fi

    if [ ! -x "$test_path" ]; then
        log_warn "Making test executable: $test_script"
        chmod +x "$test_path"
    fi

    local start_time=$SECONDS

    if $VERBOSE; then
        # Run with full output
        if "$test_path"; then
            return 0
        else
            return 1
        fi
    else
        # Run with suppressed output, capture exit code
        local output_file
        output_file=$(mktemp)

        if "$test_path" > "$output_file" 2>&1; then
            rm -f "$output_file"
            return 0
        else
            # Show output on failure
            echo ""
            echo "=== Test output for $test_script ==="
            cat "$output_file"
            echo "=== End test output ==="
            echo ""
            rm -f "$output_file"
            return 1
        fi
    fi
}

cleanup_stale_sessions() {
    log_info "Cleaning up stale test sessions..."

    # Kill any forge-test sessions
    local stale_sessions
    stale_sessions=$(tmux list-sessions 2>/dev/null | grep "forge-test" | cut -d: -f1 || true)

    if [ -n "$stale_sessions" ]; then
        for session in $stale_sessions; do
            log_warn "Killing stale session: $session"
            tmux kill-session -t "$session" 2>/dev/null || true
        done
    fi

    # Kill any stray forge processes
    pkill -f "^forge$" 2>/dev/null || true

    sleep 1
}

print_banner() {
    echo ""
    echo -e "${BLUE}========================================"
    echo "  FORGE Test Suite"
    echo -e "========================================${NC}"
    echo ""
}

print_summary() {
    local total_tests=${#TEST_NAMES[@]}
    local passed=0
    local failed=0
    local total_time=0

    echo ""
    echo -e "${BLUE}========================================"
    echo "  Test Summary"
    echo -e "========================================${NC}"
    echo ""

    for i in "${!TEST_NAMES[@]}"; do
        local name="${TEST_NAMES[$i]}"
        local result="${TEST_RESULTS[$i]}"
        local time="${TEST_TIMES[$i]}"
        total_time=$((total_time + time))

        if [ "$result" -eq 0 ]; then
            echo -e "${GREEN}  PASS${NC} $name (${time}s)"
            ((passed++)) || true
        else
            echo -e "${RED}  FAIL${NC} $name (${time}s)"
            ((failed++)) || true
        fi
    done

    echo ""
    echo "----------------------------------------"
    echo ""

    if [ $failed -eq 0 ]; then
        echo -e "${GREEN}  All tests passed: $passed/$total_tests${NC}"
    else
        echo -e "${RED}  Tests failed: $failed/$total_tests${NC}"
    fi

    echo "  Total time: ${total_time}s"
    echo ""

    return $failed
}

# ==============================================================================
# Main
# ==============================================================================

main() {
    parse_args "$@"

    print_banner

    # Select test set
    local tests
    if $QUICK; then
        log_info "Running quick test suite (skipping chat test)..."
        tests=("${QUICK_TESTS[@]}")
    else
        log_info "Running full test suite..."
        tests=("${TEST_SCRIPTS[@]}")
    fi

    # Cleanup before starting
    cleanup_stale_sessions

    # Adjust for CI
    ci_adjust

    local suite_start=$SECONDS
    local any_failed=false

    # Run each test
    for test_script in "${tests[@]}"; do
        local test_name="${test_script%.sh}"
        TEST_NAMES+=("$test_name")

        echo ""
        log_info "Running: $test_script"
        echo "----------------------------------------"

        local test_start=$SECONDS

        if run_single_test "$test_script"; then
            TEST_RESULTS+=(0)
            local elapsed=$((SECONDS - test_start))
            TEST_TIMES+=("$elapsed")
            echo -e "${GREEN}  PASSED${NC} ($elapsed s)"
        else
            TEST_RESULTS+=(1)
            local elapsed=$((SECONDS - test_start))
            TEST_TIMES+=("$elapsed")
            echo -e "${RED}  FAILED${NC} ($elapsed s)"
            any_failed=true
        fi

        # Cleanup between tests
        cleanup_stale_sessions
    done

    # Print summary
    print_summary
    local summary_result=$?

    # Final cleanup
    cleanup_stale_sessions

    # Verify cleanup
    local remaining_sessions
    remaining_sessions=$(tmux list-sessions 2>/dev/null | grep -c "forge-test" || echo "0")
    if [ "$remaining_sessions" -gt 0 ] 2>/dev/null; then
        log_warn "Warning: $remaining_sessions test sessions still running"
    fi

    # Exit with appropriate code
    if $any_failed; then
        exit 1
    else
        exit 0
    fi
}

main "$@"
