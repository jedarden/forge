# FORGE Testing Framework

Automated tests for FORGE launchers and integrations.

---

## Launcher Test Harness

### Quick Start

```bash
# Test a launcher
./launcher-test-harness.py ~/.forge/launchers/my-launcher

# Test the example passing launcher
./launcher-test-harness.py ./example-launcher-passing.sh
```

### Expected Output

```
============================================================
Testing launcher: ~/.forge/launchers/my-launcher
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

---

## Test Coverage

The test harness validates:

1. **Argument Parsing** - Accepts required args, rejects invalid
2. **Output Format** - Returns valid JSON with required fields
3. **Status File Creation** - Creates `~/.forge/status/<worker-id>.json`
4. **Log File Creation** - Creates `~/.forge/logs/<worker-id>.log`
5. **Process Spawning** - Actually spawns a running process/tmux session

---

## Files

- `launcher-test-harness.py` - Main test runner
- `example-launcher-passing.sh` - Reference implementation that passes all tests
- `README.md` - This file

---

## Usage

### Test Your Launcher

```bash
# Make your launcher executable
chmod +x ~/.forge/launchers/my-launcher

# Run tests
./launcher-test-harness.py ~/.forge/launchers/my-launcher

# If tests fail, check:
# 1. Launcher is executable (chmod +x)
# 2. Returns valid JSON on stdout
# 3. Creates status file at ~/.forge/status/<worker-id>.json
# 4. Creates log file at ~/.forge/logs/<worker-id>.log
# 5. Actually spawns a process
```

### CI Integration

Add to `.github/workflows/test.yml`:

```yaml
- name: Test Launchers
  run: |
    for launcher in ~/.forge/launchers/*; do
      ./test/launcher-test-harness.py "$launcher" || exit 1
    done
```

---

## Troubleshooting

### Launcher Not Executable

```bash
chmod +x ~/.forge/launchers/my-launcher
```

### Invalid JSON Output

Check your launcher's stdout - should only contain JSON:

```json
{"worker_id": "test", "pid": 12345, "status": "spawned"}
```

Don't print anything else to stdout (use stderr for errors).

### Status File Not Created

Ensure your launcher creates:

```bash
cat > ~/.forge/status/$SESSION_NAME.json << EOF
{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "workspace": "$WORKSPACE",
  "pid": $PID
}
EOF
```

### Log File Not Created

Ensure your launcher writes at least one log entry:

```bash
echo "{\"timestamp\": \"$(date -Iseconds)\", \"level\": \"info\", \"worker_id\": \"$SESSION_NAME\", \"message\": \"Worker started\"}" \
  >> ~/.forge/logs/$SESSION_NAME.log
```

---

## Example Launchers

See `example-launcher-passing.sh` for a reference implementation.

---

**FORGE** - Federated Orchestration & Resource Generation Engine
