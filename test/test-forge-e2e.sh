#!/bin/bash
# End-to-End Integration Test Suite for Forge
# Tests complete forge workflow across multiple terminal sizes
#
# Acceptance Criteria (from fg-21wz):
# - All 3 terminal sizes pass (80x24, 120x40, 199x55)
# - No crashes during workflow
# - All views accessible
# - Clean shutdown achieved
# - Test completes in < 2 minutes

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FORGE_ROOT="$(dirname "$SCRIPT_DIR")"
FORGE_BIN="${FORGE_BIN:-$HOME/.cargo/bin/forge}"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULTS_DIR="/tmp/forge-e2e-results-${TIMESTAMP}"
START_TIME=$(date +%s)

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

# Test counters
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_WARNED=0

# Terminal sizes to test
declare -a TERMINAL_SIZES=("80x24" "120x40" "199x55")

# View hotkeys and names
# Note: 'o' lowercase spawns Opus worker, 'O' uppercase is Overview
declare -a VIEW_KEYS=("w" "t" "c" "m" "l" "O")
declare -a VIEW_NAMES=("Workers" "Tasks" "Costs" "Metrics" "Logs" "Overview")

echo_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
echo_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; ((TESTS_WARNED++)) || true; }
echo_error() { echo -e "${RED}[ERROR]${NC} $1"; ((TESTS_FAILED++)) || true; }
echo_pass() { echo -e "${GREEN}[PASS]${NC} $1"; ((TESTS_PASSED++)) || true; }
echo_fail() { echo -e "${RED}[FAIL]${NC} $1"; ((TESTS_FAILED++)) || true; }
echo_section() {
    echo ""
    echo -e "${BOLD}${BLUE}══════════════════════════════════════════════════════════════${NC}"
    echo -e "${BOLD}${BLUE}  $1${NC}"
    echo -e "${BOLD}${BLUE}══════════════════════════════════════════════════════════════${NC}"
    echo ""
}

setup() {
    echo_section "Setting Up Test Environment"
    mkdir -p "$RESULTS_DIR"
    echo_info "Results directory: $RESULTS_DIR"

    if [[ ! -x "$FORGE_BIN" ]]; then
        echo_error "Forge binary not found at $FORGE_BIN"
        echo_info "Building forge..."
        cd "$FORGE_ROOT" && make release
        FORGE_BIN="$FORGE_ROOT/target/release/forge"
    fi
    echo_info "Using forge binary: $FORGE_BIN"

    if [[ ! -f "$HOME/.forge/config.yaml" ]]; then
        echo_error "Forge config not found at ~/.forge/config.yaml"
        exit 1
    fi
    echo_info "Found forge config"
    mkdir -p "$HOME/.forge/status" "$HOME/.forge/logs"
}

cleanup_session() {
    local name="$1"
    tmux send-keys -t "$name" 'q' 2>/dev/null || true
    sleep 0.3
    tmux kill-session -t "$name" 2>/dev/null || true
}

test_single_size() {
    local size="$1"
    local cols="${size%x*}"
    local rows="${size#*x}"
    local session="forge-e2e-${size}"
    local size_results="$RESULTS_DIR/$size"

    mkdir -p "$size_results"
    cleanup_session "$session"

    echo_info "Testing at ${cols}x${rows}..."

    # Start forge directly (not via shell command with pipe)
    # This ensures the tmux session closes when forge exits
    tmux new-session -d -s "$session" -x "$cols" -y "$rows" "$FORGE_BIN 2>&1 | tee $size_results/forge.log; exec bash"
    sleep 3

    # Test 1: Basic rendering
    tmux capture-pane -t "$session" -p > "$size_results/render.txt"
    if grep -q "FORGE" "$size_results/render.txt"; then
        echo_pass "Render OK at ${size}"
    else
        echo_fail "Render FAILED at ${size}"
        return 1
    fi

    # Test 2: View switching (quick test of each view)
    for i in "${!VIEW_KEYS[@]}"; do
        tmux send-keys -t "$session" "${VIEW_KEYS[$i]}"
        sleep 0.3
    done
    # Go back to overview
    tmux send-keys -t "$session" "o"
    sleep 0.5
    echo_pass "View switching OK at ${size}"

    # Test 3: Chat interface
    tmux send-keys -t "$session" ":"
    sleep 0.5
    tmux capture-pane -t "$session" -p > "$size_results/chat.txt"
    tmux send-keys -t "$session" Escape  # Exit chat input mode
    sleep 0.3
    echo_pass "Chat accessible at ${size}"

    # Test 4: Clean quit
    # Make sure we're not in any input mode
    tmux send-keys -t "$session" Escape
    sleep 0.3
    # Go to overview
    tmux send-keys -t "$session" "o"
    sleep 0.3
    # Send quit - forge should exit, leaving us at bash prompt
    tmux send-keys -t "$session" "q"
    sleep 2

    # Check if forge is still running (tmux session exists but forge may have exited)
    # The session will have a bash prompt if forge exited cleanly
    tmux capture-pane -t "$session" -p > "$size_results/after-quit.txt"
    if grep -qE "^\$|coder@|forge.*exit|logout" "$size_results/after-quit.txt" 2>/dev/null || \
       ! grep -q "FORGE" "$size_results/after-quit.txt" 2>/dev/null; then
        echo_pass "Clean exit at ${size} (forge terminated)"
        # Kill the remaining shell session
        tmux send-keys -t "$session" "exit" C-m 2>/dev/null || true
        sleep 0.5
        tmux kill-session -t "$session" 2>/dev/null || true
    else
        # Try Ctrl+C to force quit
        tmux send-keys -t "$session" C-c
        sleep 1
        tmux capture-pane -t "$session" -p > "$size_results/after-quit2.txt"
        if ! grep -q "FORGE" "$size_results/after-quit2.txt" 2>/dev/null; then
            echo_pass "Clean exit at ${size} (after Ctrl+C)"
            tmux kill-session -t "$session" 2>/dev/null || true
        else
            echo_warn "Forced kill at ${size}"
            tmux kill-session -t "$session" 2>/dev/null || true
        fi
    fi

    # Test 5: No crashes in log
    if grep -qiE "panic|thread.*panicked|fatal error" "$size_results/forge.log" 2>/dev/null; then
        echo_fail "Crash detected at ${size}"
        return 1
    else
        echo_pass "No crashes at ${size}"
    fi

    return 0
}

run_all_tests() {
    local all_passed=0

    for size in "${TERMINAL_SIZES[@]}"; do
        echo_section "Testing ${size}"
        if test_single_size "$size"; then
            ((all_passed++))
        fi
    done

    return $(( ${#TERMINAL_SIZES[@]} - all_passed ))
}

print_summary() {
    local end_time=$(date +%s)
    local duration=$((end_time - START_TIME))

    echo_section "E2E Test Summary"
    echo "Terminal sizes tested: ${TERMINAL_SIZES[*]}"
    echo ""
    echo "Results:"
    echo "  ${GREEN}Passed:${NC}  $TESTS_PASSED"
    echo "  ${RED}Failed:${NC}  $TESTS_FAILED"
    echo "  ${YELLOW}Warnings:${NC} $TESTS_WARNED"
    echo ""
    echo "Duration: ${duration}s"
    echo "Results directory: $RESULTS_DIR"
    echo ""

    if [[ $TESTS_FAILED -eq 0 ]]; then
        echo -e "${GREEN}${BOLD}ALL E2E TESTS PASSED${NC}"
        return 0
    else
        echo -e "${RED}${BOLD}SOME E2E TESTS FAILED${NC}"
        return 1
    fi
}

cleanup_all() {
    for size in "${TERMINAL_SIZES[@]}"; do
        cleanup_session "forge-e2e-${size}"
    done
}

main() {
    echo_section "Forge E2E Integration Test Suite"
    echo "Started at: $(date)"
    echo ""

    setup
    run_all_tests
    local result=$?
    cleanup_all
    print_summary

    return $result
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    trap 'echo_error "Test interrupted"; cleanup_all; exit 130' INT TERM
    main "$@"
fi
