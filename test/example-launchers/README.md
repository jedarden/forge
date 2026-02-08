# FORGE Launcher Examples

This directory contains example launcher scripts that demonstrate the FORGE launcher protocol. Both passing and failing examples are provided to help understand the requirements and test the launcher-test-harness.py.

## Quick Reference

| Example | Status | Description | Fails Test |
|---------|--------|-------------|------------|
| `claude-code-launcher.sh` | ✅ PASS | Reference implementation for Claude Code | - |
| `aider-launcher.sh` | ✅ PASS | Example for Aider AI pair programming tool | - |
| `continue-launcher.sh` | ✅ PASS | Example for Continue VS Code extension | - |
| `bead-worker-launcher.sh` | ✅ PASS | **Bead-aware launcher** with `--bead-ref` support | - |
| `failing-missing-arguments.sh` | ❌ FAIL | Doesn't validate required arguments | Test 1 |
| `failing-invalid-json.sh` | ❌ FAIL | Outputs invalid JSON | Test 2 |
| `failing-wrong-status.sh` | ❌ FAIL | Uses wrong status value | Test 2 |
| `failing-no-status-file.sh` | ❌ FAIL | Doesn't create status file | Test 3 |
| `failing-no-log-file.sh` | ❌ FAIL | Doesn't create log file | Test 4 |
| `failing-no-process.sh` | ❌ FAIL | Doesn't spawn running process | Test 5 |

## FORGE Launcher Protocol

A valid FORGE launcher script is an executable file that implements the following interface:

### 1. Command-Line Arguments

```bash
launcher \
  --model=<model> \
  --workspace=<path> \
  --session-name=<name> \
  [--config=<path>]
```

**Required Arguments:**
- `--model` - Model identifier (e.g., "sonnet", "opus", "haiku", "gpt4")
- `--workspace` - Path to the workspace directory (must exist)
- `--session-name` - Unique session name for the worker

**Optional Arguments:**
- `--config` - Path to worker configuration file

### Bead-Aware Protocol Extension

Bead-aware launchers additionally support:

```bash
launcher \
  --model=<model> \
  --workspace=<path> \
  --session-name=<name> \
  --bead-ref=<bead-id> \
  [--config=<path>]
```

**Bead-Aware Parameter:**
- `--bead-ref` - Bead ID from br CLI (e.g., "fg-1qo", "bd-abc")
  - When present: launcher fetches bead data and constructs task prompt
  - When absent: launcher operates in standard mode (no task assigned)

**Bead-aware launchers:**
- Fetch bead data using `br show <bead-id>`
- Construct task prompt with bead context (title, description, priority)
- Update bead status to `in_progress` on spawn
- Include `bead_ref` field in output JSON and status file
- Close bead with `br close <bead-id>` when worker completes

See `bead-worker-launcher.sh` for a complete reference implementation.

### 2. stdout Output (JSON ONLY)

The launcher must output valid JSON on stdout with these fields:

```json
{
  "worker_id": "<session-name>",
  "pid": <integer>,
  "status": "spawned",
  "launcher": "<launcher-name>",
  "timestamp": "<ISO-8601-timestamp>"
}
```

**Standard Output (no bead assigned):**

**Required Fields:**
- `worker_id` (string) - Worker identifier
- `pid` (integer) - Process ID of spawned worker
- `status` (string) - Must be exactly `"spawned"`

**Optional Fields:**
- `launcher` (string) - Launcher name for logging
- `timestamp` (string) - ISO-8601 timestamp

**Bead-Aware Output (when --bead-ref is provided):**

```json
{
  "worker_id": "<session-name>",
  "pid": <integer>,
  "status": "spawned",
  "launcher": "<launcher-name>",
  "bead_ref": "<bead-id>",
  "timestamp": "<ISO-8601-timestamp>"
}
```

**Bead-Aware Additional Field:**
- `bead_ref` (string) - Bead ID assigned to this worker

**Important:**
- No extra output before or after the JSON
- No debug messages on stdout (use stderr)
- Must be valid JSON

### 3. Status File

Create `~/.forge/status/<worker-id>.json`:

```json
{
  "worker_id": "<worker-id>",
  "status": "active",
  "model": "<model>",
  "workspace": "<workspace-path>",
  "pid": <integer>,
  "started_at": "<ISO-8601>",
  "last_activity": "<ISO-8601>",
  "current_task": null,
  "tasks_completed": 0
}
```

**Bead-Aware Status File (when --bead-ref is provided):**

```json
{
  "worker_id": "<worker-id>",
  "status": "active",
  "model": "<model>",
  "workspace": "<workspace-path>",
  "pid": <integer>,
  "started_at": "<ISO-8601>",
  "last_activity": "<ISO-8601>",
  "current_task": {
    "bead_id": "<bead-id>",
    "bead_title": "<bead-title>",
    "priority": <0-4>
  },
  "tasks_completed": 0
}
```

**Valid Status Values:** `"active"`, `"idle"`, `"starting"`, `"spawned"`

**Requirements:**
- Must be created within 5 seconds of execution
- Must contain valid JSON
- Must include required fields

### 4. Log File

Write logs to `~/.forge/logs/<worker-id>.log`:

```json
{"timestamp": "2026-02-07T10:30:00Z", "level": "info", "worker_id": "sonnet-alpha", "message": "Worker started"}
```

**Requirements:**
- Must be created within 5 seconds of execution
- Must not be empty (at least one log entry)
- JSON Lines (JSONL) format recommended

### 5. Process Spawning

The launcher must spawn an actual running process:
- Background the worker process using `&` or equivalent
- The PID in the output must correspond to a running process
- The process must still be running after 5 seconds
- Alternative: Create a tmux session (test harness checks for both)

### 6. Exit Behavior

- Exit with code `0` on success
- Exit with code `1` on error
- Print errors to stderr (not stdout)
- Complete within 15 seconds (timeout enforced by test harness)

## Testing Examples

### Test All Passing Examples

```bash
# Test Claude Code launcher
./test/launcher-test-harness.py test/example-launchers/claude-code-launcher.sh

# Test Aider launcher
./test/launcher-test-harness.py test/example-launchers/aider-launcher.sh

# Test Continue launcher
./test/launcher-test-harness.py test/example-launchers/continue-launcher.sh

# Test Bead-Aware launcher (standard mode - no bead)
./test/launcher-test-harness.py test/example-launchers/bead-worker-launcher.sh
```

### Test Bead-Aware Launcher

```bash
# Test with a real bead (requires br CLI and valid bead ID)
./test/example-launchers/bead-worker-launcher.sh \
  --model=sonnet \
  --workspace=/home/coder/forge \
  --session-name=test-bead-launch \
  --bead-ref=fg-1qo

# Verify bead status was updated to in_progress
br show fg-1qo

# Verify status file contains bead_ref
cat ~/.forge/status/test-bead-launch.json | jq '.current_task'

# Test without bead ref (standard mode - should pass test harness)
./test/launcher-test-harness.py test/example-launchers/bead-worker-launcher.sh
```

Expected output:
```
============================================================
Testing launcher: test/example-launchers/claude-code-launcher.sh
============================================================

Test 1: Argument Parsing... ✅ PASS
Test 2: Output Format... ✅ PASS
Test 3: Status File Creation... ✅ PASS
Test 4: Log File Creation... ✅ PASS
Test 5: Process Spawning... ✅ PASS

============================================================
Results: 5 passed, 0 failed
============================================================
```

### Test Failing Examples

```bash
# Test missing arguments validation
./test/launcher-test-harness.py test/example-launchers/failing-missing-arguments.sh

# Test invalid JSON handling
./test/launcher-test-harness.py test/example-launchers/failing-invalid-json.sh

# Test wrong status value
./test/launcher-test-harness.py test/example-launchers/failing-wrong-status.sh

# Test missing status file
./test/launcher-test-harness.py test/example-launchers/failing-no-status-file.sh

# Test missing log file
./test/launcher-test-harness.py test/example-launchers/failing-no-log-file.sh

# Test no process spawned
./test/launcher-test-harness.py test/example-launchers/failing-no-process.sh
```

## Creating Your Own Launcher

### Step 1: Copy a Passing Example

```bash
cp test/example-launchers/claude-code-launcher.sh ~/.forge/launchers/my-launcher
chmod +x ~/.forge/launchers/my-launcher
```

### Step 2: Modify for Your Tool

Edit the worker spawning section to launch your tool:

```bash
# Example: Launch a custom Python worker
(
  cd "$WORKSPACE"
  python my_worker.py --model "$MODEL" 2>&1 | tee ~/.forge/logs/$SESSION_NAME.log
) &
```

### Step 3: Test Your Launcher

```bash
./test/launcher-test-harness.py ~/.forge/launchers/my-launcher
```

### Step 4: Configure FORGE

Add to `~/.forge/config.yaml`:

```yaml
launchers:
  my-launcher:
    executable: "~/.forge/launchers/my-launcher"
    models: ["custom-model"]
```

## Common Pitfalls

| Pitfall | Symptom | Solution |
|---------|---------|----------|
| Extra output on stdout | Test 2 fails - "Invalid JSON" | Ensure ONLY JSON is printed to stdout |
| Wrong status value | Test 2 fails - "Status should be 'spawned'" | Use exactly `"status": "spawned"` |
| Missing status file | Test 3 fails - "Status file not created" | Create `~/.forge/status/<worker-id>.json` |
| Empty log file | Test 4 fails - "Log file is empty" | Write at least one log entry |
| Process exits immediately | Test 5 fails - "Process not running" | Ensure worker process stays alive |
| Missing workspace validation | Test 1 passes but runtime errors | Check workspace exists with `-d` |

## Test Harness Details

The `launcher-test-harness.py` runs 5 tests:

1. **Test 1: Argument Parsing** - Verifies launcher accepts required arguments and fails with missing arguments
2. **Test 2: Output Format** - Validates JSON output with required fields
3. **Test 3: Status File Creation** - Checks status file exists and is valid
4. **Test 4: Log File Creation** - Checks log file exists and is not empty
5. **Test 5: Process Spawning** - Verifies a running process or tmux session exists

### Test Environment

- Test workspace: `/tmp/forge-test`
- Test session names: `test-arg-parse`, `test-output`, `test-status`, `test-log`, `test-process`
- Timeout: 15 seconds

## Additional Resources

- [FORGE Integration Guide](../../docs/INTEGRATION_GUIDE.md) - Complete integration documentation
- [Launcher Testing Guide](../../docs/LAUNCHER_TESTING.md) - Detailed testing documentation
- [Bead-Aware Launcher Protocol](../../docs/BEAD_LAUNCHER_PROTOCOL.md) - Bead allocation protocol specification
- [test/launcher-test-harness.py](../launcher-test-harness.py) - Test harness source code
- [src/forge/launcher.py](../../src/forge/launcher.py) - Python WorkerLauncher implementation
- [Bead-Aware Launcher Reference](bead-worker-launcher.sh) - Complete bead-aware launcher implementation
