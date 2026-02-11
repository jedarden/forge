# FORGE TUI Test Suite

Automated testing framework for the FORGE TUI application using tmux for headless terminal testing.

## Overview

This test suite provides comprehensive automated testing of the FORGE TUI by:
- Launching forge in controlled tmux sessions
- Injecting keystrokes programmatically
- Capturing pane output for assertions
- Verifying log file entries
- Testing across different terminal dimensions

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

## Test Scripts

### test-forge-chat.sh
Tests chat functionality:
- Launch forge and enter chat mode (`:`)
- Send a query ("What is forge?")
- Verify response is received
- Check logs for success indicators

### test-forge-workers.sh
Tests worker management:
- Navigate to Workers view (`w`)
- Test spawn worker keys (`g`, `s`, `o`, `h`)
- Test kill worker (`k`)
- Verify worker panel updates

### test-forge-views.sh
Tests view navigation:
- All view hotkeys: `o`, `w`, `t`, `c`, `m`, `l`, `:`, `a`
- Tab/Shift+Tab cycling
- Help overlay (`?`)
- Footer hotkey hints

### test-forge-theme.sh
Tests theme switching:
- Theme cycle with `C` key
- Theme persistence to config file
- Visual changes between themes
- Help documentation for theme hotkey

### run-all-tests.sh
Test runner that:
- Executes all test scripts in order
- Collects results and timing
- Generates summary report
- Cleans up stale sessions
- Exits 0 if all pass, 1 if any fail

## Test Framework

### lib/test-helpers.sh
Shared helper library providing:

#### Lifecycle Functions
```bash
start_forge SESSION_NAME [WIDTH] [HEIGHT]  # Start forge in tmux
wait_for_init SESSION_NAME [TIMEOUT]       # Wait for initialization
stop_forge SESSION_NAME                    # Stop and cleanup
```

#### Keystroke Injection
```bash
send_keys SESSION_NAME KEYS...             # Send raw keys
send_key_wait SESSION_NAME KEY [WAIT]      # Send key and wait
type_text SESSION_NAME TEXT                # Type text into input
```

#### Output Capture
```bash
capture_pane SESSION_NAME [OUTPUT_FILE]    # Capture pane content
pane_contains SESSION_NAME TEXT            # Check for text
wait_for_pane_text SESSION TEXT [TIMEOUT]  # Wait for text
```

#### Log Verification
```bash
log_contains PATTERN [TAIL_LINES]          # Check log for pattern
wait_for_log PATTERN [TIMEOUT]             # Wait for log entry
```

#### Assertions
```bash
assert_pane_contains SESSION TEXT [MSG]    # Assert text present
assert_pane_not_contains SESSION TEXT      # Assert text absent
assert_log_contains PATTERN [MSG]          # Assert log pattern
assert_session_running SESSION [MSG]       # Assert session alive
```

## Configuration

Environment variables for customization:

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

## CI Integration

Tests automatically detect CI environment and adjust:
- Increased timeouts
- Smaller terminal dimensions
- Non-interactive mode

```bash
# Force CI mode
CI=true ./tests/run-all-tests.sh

# Custom terminal size
COLUMNS=80 LINES=24 ./tests/run-all-tests.sh
```

## Expected Output

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

## Troubleshooting

### Tests hang indefinitely
```bash
# Check for stale sessions
tmux ls | grep forge-test

# Kill all test sessions
tmux kill-server  # Nuclear option
```

### Forge won't initialize
```bash
# Check forge binary exists
which forge

# Check logs
tail -50 ~/.forge/logs/forge.log.$(date +%Y-%m-%d)
```

### Pane assertions fail
```bash
# Capture manual output for debugging
tmux new-session -d -s debug forge
tmux capture-pane -t debug -p > debug-output.txt
tmux kill-session -t debug
cat debug-output.txt
```

### CI tests fail but local passes
```bash
# Run with CI settings
CI=true COLUMNS=80 LINES=24 ./tests/run-all-tests.sh
```

## Writing New Tests

1. Create test script in `tests/` directory
2. Source the helpers: `source "$SCRIPT_DIR/lib/test-helpers.sh"`
3. Implement test functions
4. Add to `TEST_SCRIPTS` array in `run-all-tests.sh`

Example:
```bash
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/test-helpers.sh"

test_my_feature() {
    local session
    session=$(get_session_name)

    start_forge "$session"
    wait_for_init "$session"

    # Your test logic here
    send_key_wait "$session" "w" 1
    assert_pane_contains "$session" "Workers"

    stop_forge "$session"
    return 0
}

main() {
    ci_adjust

    if test_my_feature; then
        echo -e "${GREEN}TEST PASSED${NC}"
        exit 0
    else
        echo -e "${RED}TEST FAILED${NC}"
        exit 1
    fi
}

[[ "${BASH_SOURCE[0]}" == "${0}" ]] && main "$@"
```

## Architecture

```
tests/
├── README.md              # This documentation
├── run-all-tests.sh       # Test suite runner
├── test-forge-chat.sh     # Chat functionality tests
├── test-forge-views.sh    # View navigation tests
├── test-forge-workers.sh  # Worker management tests
├── test-forge-theme.sh    # Theme switching tests
└── lib/
    └── test-helpers.sh    # Shared helper functions
```

## Key Bindings Reference

| Key | Action |
|-----|--------|
| `o`/`O` | Overview view |
| `w`/`W` | Workers view |
| `t`/`T` | Tasks view |
| `c` | Costs view |
| `m` | Metrics view |
| `l`/`a` | Logs/Activity view |
| `:` | Chat mode |
| `?` | Help overlay |
| `C` | Cycle theme |
| `g`/`G` | Spawn GLM worker |
| `s`/`S` | Spawn Sonnet worker |
| `o` | Spawn Opus worker |
| `h` | Spawn Haiku worker |
| `k` | Kill worker |
| `q`/`Q` | Quit |
| `Tab` | Next view |
| `Shift+Tab` | Previous view |
| `Esc` | Cancel/close |

## License

MIT - Part of the FORGE project.
