# FORGE TUI Test Suite

Comprehensive testing framework for the FORGE TUI application using tmux for headless terminal testing.

## Table of Contents

- [Overview](#overview)
- [Quick Start](#quick-start)
- [Test Framework Architecture](#test-framework-architecture)
- [Running Tests](#running-tests)
- [Writing New Tests](#writing-new-tests)
- [Tmux Patterns](#tmux-patterns)
- [Log Parsing](#log-parsing)
- [Troubleshooting Failed Tests](#troubleshooting-failed-tests)
- [CI Integration](#ci-integration)
- [Key Bindings Reference](#key-bindings-reference)

## Overview

This test suite provides comprehensive automated testing of the FORGE TUI by:
- Launching forge in controlled tmux sessions
- Injecting keystrokes programmatically
- Capturing pane output for assertions
- Verifying log file entries
- Testing across different terminal dimensions

### Test Scripts

| Script | Description | Time |
|--------|-------------|------|
| `test-forge-views.sh` | View navigation, Tab cycling, help overlay | ~12s |
| `test-forge-theme.sh` | Theme cycling, persistence, visual changes | ~8s |
| `test-forge-workers.sh` | Worker spawn/kill, panel display, status updates | ~15s |
| `test-forge-chat.sh` | Chat mode, query submission, response verification | ~45s |

## Quick Start

```bash
# Run all tests
./tests/run-all-tests.sh

# Run quick tests (skip slow chat test)
./tests/run-all-tests.sh --quick

# Run with verbose output
./tests/run-all-tests.sh --verbose

# Run individual tests
./tests/test-forge-views.sh
./tests/test-forge-workers.sh
./tests/test-forge-theme.sh
./tests/test-forge-chat.sh
```

## Test Framework Architecture

```
tests/
├── README.md                 # This documentation
├── run-all-tests.sh          # Test suite runner with reporting
├── test-forge-chat.sh        # Chat functionality tests
├── test-forge-views.sh       # View navigation tests
├── test-forge-workers.sh     # Worker management tests
├── test-forge-theme.sh       # Theme switching tests
└── lib/
    └── test-helpers.sh       # Shared helper functions library
```

### Core Components

#### 1. Test Runner (`run-all-tests.sh`)
- Executes all test scripts in order
- Collects results and timing
- Generates summary report
- Cleans up stale sessions
- Exits 0 if all pass, 1 if any fail

#### 2. Helper Library (`lib/test-helpers.sh`)
Shared functions for all tests:

**Lifecycle Functions:**
```bash
start_forge SESSION_NAME [WIDTH] [HEIGHT]  # Start forge in tmux
wait_for_init SESSION_NAME [TIMEOUT]       # Wait for initialization
stop_forge SESSION_NAME                    # Stop and cleanup
```

**Keystroke Injection:**
```bash
send_keys SESSION_NAME KEYS...             # Send raw keys
send_key_wait SESSION_NAME KEY [WAIT]      # Send key and wait
type_text SESSION_NAME TEXT                # Type text into input
```

**Output Capture:**
```bash
capture_pane SESSION_NAME [OUTPUT_FILE]    # Capture pane content
pane_contains SESSION_NAME TEXT            # Check for text
wait_for_pane_text SESSION TEXT [TIMEOUT]  # Wait for text
```

**Log Verification:**
```bash
log_contains PATTERN [TAIL_LINES]          # Check log for pattern
wait_for_log PATTERN [TIMEOUT]             # Wait for log entry
```

**Assertions:**
```bash
assert_pane_contains SESSION TEXT [MSG]    # Assert text present
assert_pane_not_contains SESSION TEXT      # Assert text absent
assert_log_contains PATTERN [MSG]          # Assert log pattern
assert_session_running SESSION [MSG]       # Assert session alive
```

## Running Tests

### Environment Variables

```bash
# Terminal dimensions
TERM_WIDTH=229      # Default terminal width
TERM_HEIGHT=55      # Default terminal height

# Timeouts (seconds)
INIT_TIMEOUT=90     # Forge initialization timeout
ACTION_TIMEOUT=30   # Action completion timeout
POLL_INTERVAL=1     # Polling interval

# Output paths
TMP_DIR=/tmp        # Temp file location
```

### Run Modes

```bash
# Full test suite
./tests/run-all-tests.sh

# Quick tests (no chat)
./tests/run-all-tests.sh --quick

# Verbose mode (show all output)
./tests/run-all-tests.sh --verbose

# CI mode
CI=true ./tests/run-all-tests.sh

# Custom terminal size
COLUMNS=80 LINES=24 ./tests/run-all-tests.sh
```

### Expected Output

```
========================================
  FORGE Test Suite
========================================

[INFO] Running full test suite...
[INFO] Cleaning up stale test sessions...

[INFO] Running: test-forge-views.sh
----------------------------------------
  PASSED (12 s)

[INFO] Running: test-forge-theme.sh
----------------------------------------
  PASSED (8 s)

[INFO] Running: test-forge-workers.sh
----------------------------------------
  PASSED (15 s)

[INFO] Running: test-forge-chat.sh
----------------------------------------
  PASSED (45 s)

========================================
  Test Summary
========================================

  PASS test-forge-views (12s)
  PASS test-forge-theme (8s)
  PASS test-forge-workers (15s)
  PASS test-forge-chat (45s)

----------------------------------------

  All tests passed: 4/4
  Total time: 80s
```

## Writing New Tests

### Step 1: Create Test Script

Create a new file in `tests/` directory:

```bash
#!/usr/bin/env bash
# FORGE [Feature] Test
#
# Tests:
# 1. List what you're testing
# 2. Be specific about expected behavior
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

TEST_NAME="forge-myfeature"

# ==============================================================================
# Test Implementation
# ==============================================================================

test_my_feature() {
    local session
    session=$(get_session_name)
    local result=0

    log_info "=== Testing My Feature ==="

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

    # Step 3: Your test logic
    log_info "Step 3: Testing feature..."
    send_key_wait "$session" "w" 1

    if assert_pane_contains "$session" "Workers" "Should show Workers view"; then
        log_success "Feature works correctly"
    else
        log_fail "Feature did not work"
        result=1
    fi

    # Always cleanup
    log_info "Cleanup: Stopping forge..."
    stop_forge "$session"

    return $result
}

# ==============================================================================
# Main
# ==============================================================================

main() {
    echo ""
    echo "========================================"
    echo "FORGE [Feature] Test"
    echo "========================================"

    ci_adjust

    local start_time=$SECONDS
    local result=0

    if test_my_feature; then
        local elapsed=$((SECONDS - start_time))
        echo -e "${GREEN}TEST PASSED (${elapsed}s)${NC}"
    else
        local elapsed=$((SECONDS - start_time))
        echo -e "${RED}TEST FAILED (${elapsed}s)${NC}"
        result=1
    fi

    exit $result
}

[[ "${BASH_SOURCE[0]}" == "${0}" ]] && main "$@"
```

### Step 2: Make Executable

```bash
chmod +x tests/test-forge-myfeature.sh
```

### Step 3: Register in Test Runner

Edit `run-all-tests.sh` and add to the `TEST_SCRIPTS` array:

```bash
TEST_SCRIPTS=(
    "test-forge-views.sh"
    "test-forge-theme.sh"
    "test-forge-workers.sh"
    "test-forge-myfeature.sh"   # Add your test
    "test-forge-chat.sh"
)
```

### Test Structure Best Practices

#### 1. Use Clear Step Numbers
```bash
log_info "Step 1: Starting forge..."
log_info "Step 2: Waiting for initialization..."
log_info "Step 3: Performing action..."
log_info "Step 4: Verifying result..."
log_info "Step 5: Cleaning up..."
```

#### 2. Always Handle Cleanup
```bash
# Ensure cleanup happens even on failure
if ! wait_for_init "$session"; then
    log_fail "Initialization failed"
    stop_forge "$session"  # Always cleanup
    return 1
fi
```

#### 3. Use Descriptive Assertions
```bash
# Good - describes what we expect
assert_pane_contains "$session" "Worker Pool" "Should display worker panel"

# Bad - no context
pane_contains "$session" "Worker Pool"
```

#### 4. Multiple Sub-Tests Pattern
```bash
main() {
    local tests_passed=0
    local tests_total=3

    if test_feature_a; then ((tests_passed++)) || true; fi
    if test_feature_b; then ((tests_passed++)) || true; fi
    if test_feature_c; then ((tests_passed++)) || true; fi

    if [ $tests_passed -eq $tests_total ]; then
        echo "ALL TESTS PASSED: $tests_passed/$tests_total"
        exit 0
    else
        echo "TESTS FAILED: $tests_passed/$tests_total passed"
        exit 1
    fi
}
```

#### 5. Timeout Handling
```bash
# Wait for async operations with timeout
if ! wait_for_pane_text "$session" "Expected Text" 10; then
    log_fail "Timeout waiting for text"
    return 1
fi
```

## Tmux Patterns

### Session Management

```bash
# Create a new detached session running forge
tmux new-session -d -s "test-session" -x 229 -y 55 "forge"

# Check if session exists
tmux has-session -t "test-session" 2>/dev/null

# Kill a session
tmux kill-session -t "test-session"

# List all sessions (for debugging)
tmux list-sessions
```

### Keystroke Injection

```bash
# Send single key
tmux send-keys -t "session-name" "w"

# Send special keys
tmux send-keys -t "session-name" "Enter"
tmux send-keys -t "session-name" "Escape"
tmux send-keys -t "session-name" "Tab"
tmux send-keys -t "session-name" "BTab"      # Shift+Tab
tmux send-keys -t "session-name" "Up"
tmux send-keys -t "session-name" "Down"
tmux send-keys -t "session-name" "C-c"       # Ctrl+C

# Send multiple keys
tmux send-keys -t "session-name" "hello" "Enter"

# Type literal text
tmux send-keys -t "session-name" "What is forge?"
```

### Pane Capture

```bash
# Capture current pane content to stdout
tmux capture-pane -t "session-name" -p

# Capture to file
tmux capture-pane -t "session-name" -p > output.txt

# Capture with escape sequences (colors)
tmux capture-pane -t "session-name" -p -e

# Capture scrollback buffer
tmux capture-pane -t "session-name" -p -S -1000
```

### Common Tmux Patterns in Tests

```bash
# Pattern 1: Send key and verify
send_key_wait "$session" "w" 1
if pane_contains "$session" "Workers"; then
    log_success "Navigation successful"
fi

# Pattern 2: Type text in input field
send_key_wait "$session" ":" 1           # Enter chat mode
type_text "$session" "Hello world"        # Type message
send_keys "$session" "Enter"              # Submit

# Pattern 3: Toggle overlay
send_key_wait "$session" "?" 1            # Open help
assert_pane_contains "$session" "Help"
send_keys "$session" "Escape"             # Close help

# Pattern 4: Cycle through options
for i in {1..4}; do
    send_key_wait "$session" "C" 0.5      # Cycle theme
done

# Pattern 5: Navigate list
send_keys "$session" "j"                  # Down
send_keys "$session" "k"                  # Up
send_keys "$session" "Enter"              # Select
```

### Timing Considerations

```bash
# Wait for UI to render after action
send_keys "$session" "w"
sleep 0.5                                 # Allow UI to render

# Use longer waits for async operations
send_keys "$session" "Enter"              # Submit chat
sleep 2                                   # Wait for API response

# Poll for expected state
local timeout=30
local start=$SECONDS
while (( SECONDS - start < timeout )); do
    if pane_contains "$session" "Response"; then
        break
    fi
    sleep 1
done
```

## Log Parsing

### Log File Location

```bash
# Daily log file
LOG_FILE="$HOME/.forge/logs/forge.log.$(date +%Y-%m-%d)"

# View recent logs
tail -50 ~/.forge/logs/forge.log.$(date +%Y-%m-%d)
```

### Common Log Patterns

```bash
# Check for specific log entry
if log_contains "Got response from channel!" 100; then
    echo "Response received"
fi

# Wait for log entry
if wait_for_log "Chat thread started" 30; then
    echo "Chat processing started"
fi

# Search for error patterns
grep -E "ERROR|WARN|panic" ~/.forge/logs/forge.log.$(date +%Y-%m-%d)

# Count specific events
grep -c "Spawning worker" ~/.forge/logs/forge.log.$(date +%Y-%m-%d)
```

### Log Entry Types

| Pattern | Meaning |
|---------|---------|
| `Chat thread started` | Chat query initiated |
| `Got response from channel!` | API response received |
| `Response text length` | Response content logged |
| `Chat history now has` | Chat history updated |
| `Spawning worker` | Worker spawn initiated |
| `Theme changed to` | Theme switch logged |
| `View changed to` | Navigation logged |

### Parsing Structured Logs

```bash
# Extract JSON fields (if log is JSON-structured)
jq '.message' < log_line

# Parse timestamp
grep "2024-01-15" ~/.forge/logs/forge.log.$(date +%Y-%m-%d)

# Filter by level
grep '"level":"ERROR"' ~/.forge/logs/forge.log.$(date +%Y-%m-%d)
```

## Troubleshooting Failed Tests

### Quick Diagnosis

```bash
# 1. Check for stale sessions
tmux list-sessions | grep forge-test

# 2. Kill stale sessions
tmux kill-server  # Nuclear option - kills all tmux

# 3. Check forge binary
which forge
forge --version

# 4. Check recent logs
tail -100 ~/.forge/logs/forge.log.$(date +%Y-%m-%d)
```

### Common Failures and Solutions

#### 1. Tests Hang Indefinitely

**Symptoms:** Test never completes, stuck at initialization

**Causes:**
- Stale tmux sessions blocking resources
- Forge process crashed silently
- Infinite loop in application

**Solutions:**
```bash
# Kill all test sessions
tmux list-sessions | grep forge-test | cut -d: -f1 | xargs -I{} tmux kill-session -t {}

# Kill forge processes
pkill -f "^forge$"

# Run with verbose mode
./tests/run-all-tests.sh --verbose
```

#### 2. Forge Won't Initialize

**Symptoms:** `Timeout: Forge did not initialize`

**Causes:**
- Missing forge binary
- Config file errors
- Database lock
- Resource exhaustion

**Solutions:**
```bash
# Verify binary exists
which forge
cargo build --release

# Check config
cat ~/.forge/config.toml

# Remove lock files
rm -f ~/.forge/*.lock

# Check system resources
free -h
df -h
```

#### 3. Pane Assertions Fail

**Symptoms:** `Expected 'Workers' not found`

**Causes:**
- UI didn't render in time
- Wrong view active
- Terminal size too small
- Output truncated

**Solutions:**
```bash
# Increase wait times
send_key_wait "$session" "w" 2  # Wait 2s instead of 0.5s

# Debug pane content
tmux new-session -d -s debug forge
sleep 5
tmux capture-pane -t debug -p > debug-output.txt
tmux kill-session -t debug
cat debug-output.txt

# Check terminal dimensions
echo "Width: $COLUMNS, Height: $LINES"
```

#### 4. CI Tests Fail but Local Passes

**Symptoms:** Tests pass locally, fail in CI

**Causes:**
- Different terminal size
- Missing environment variables
- Slower CI machines
- Network latency

**Solutions:**
```bash
# Simulate CI environment locally
CI=true COLUMNS=80 LINES=24 ./tests/run-all-tests.sh

# Increase timeouts
export INIT_TIMEOUT=180
export ACTION_TIMEOUT=60

# Check CI logs for specific error
```

#### 5. Log Assertions Fail

**Symptoms:** `Log should contain 'pattern'`

**Causes:**
- Log file rotated
- Pattern changed
- Logging level too high
- Async timing issue

**Solutions:**
```bash
# Verify log file exists
ls -la ~/.forge/logs/forge.log.$(date +%Y-%m-%d)

# Search logs manually
grep -r "pattern" ~/.forge/logs/

# Increase log tail lines
log_contains "pattern" 500  # Check more lines
```

### Debug Mode

Add debugging output to tests:

```bash
# Enable bash debug mode
set -x

# Add diagnostic dumps
show_diagnostic_info "$session"

# Capture screen states
local output
output=$(capture_pane "$session")
echo "=== Pane Content ==="
cat "$output"
echo "===================="

# Print log tail
tail -50 ~/.forge/logs/forge.log.$(date +%Y-%m-%d)
```

### Manual Testing Workflow

```bash
# Step 1: Start forge in a new tmux session
tmux new-session -d -s manual-test -x 229 -y 55 "forge"

# Step 2: Attach to session
tmux attach -t manual-test

# Step 3: Manually perform test steps
# (Press keys, observe behavior)

# Step 4: In another terminal, capture state
tmux capture-pane -t manual-test -p > manual-output.txt

# Step 5: Cleanup
tmux kill-session -t manual-test
```

## CI Integration

### GitHub Actions Example

```yaml
name: TUI Tests

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-action@stable

      - name: Install tmux
        run: sudo apt-get install -y tmux

      - name: Build
        run: cargo build --release

      - name: Run TUI Tests
        run: |
          export PATH="$PATH:$PWD/target/release"
          CI=true ./tests/run-all-tests.sh --quick
        timeout-minutes: 10
```

### CI Environment Detection

Tests automatically detect CI and adjust:
- Increased timeouts (2x)
- Smaller terminal dimensions
- Non-interactive mode

```bash
# Force CI mode
CI=true ./tests/run-all-tests.sh

# Or set GitHub Actions variable
GITHUB_ACTIONS=true ./tests/run-all-tests.sh
```

## Key Bindings Reference

| Key | Action | View Context |
|-----|--------|--------------|
| `o`/`O` | Overview view | Global |
| `w`/`W` | Workers view | Global |
| `t`/`T` | Tasks view | Global |
| `c` | Costs view | Global |
| `m` | Metrics view | Global |
| `l`/`a` | Logs/Activity view | Global |
| `:` | Chat mode | Global |
| `?` | Help overlay | Global |
| `C` | Cycle theme | Global |
| `q`/`Q` | Quit | Global |
| `Tab` | Next view | Global |
| `Shift+Tab` | Previous view | Global |
| `Esc` | Cancel/close | Global |
| `g`/`G` | Spawn GLM worker | Workers |
| `s`/`S` | Spawn Sonnet worker | Workers |
| `o` | Spawn Opus worker | Workers |
| `h` | Spawn Haiku worker | Workers |
| `k` | Kill worker | Workers |
| `j` | Move down | Lists |
| `k` | Move up | Lists |
| `Enter` | Select/Submit | Lists/Chat |

## Contributing

1. Follow the existing test structure
2. Add descriptive comments
3. Handle all error cases with cleanup
4. Update this README when adding features
5. Run full test suite before submitting PR

## License

MIT - Part of the FORGE project.
