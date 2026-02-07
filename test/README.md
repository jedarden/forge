# FORGE Testing Framework

Automated tests for ALL FORGE integration surfaces.

---

## Test Harnesses

### 1. Launcher Test Harness

Tests worker launcher scripts for protocol compliance.

```bash
# Test a launcher
./launcher-test-harness.py ~/.forge/launchers/my-launcher

# Test the example passing launcher
./launcher-test-harness.py ./example-launcher-passing.sh
```

### 2. Backend Test Harness

Tests headless CLI backend for chat interface.

```bash
# Test a backend
./backend-test-harness.py claude-code chat --headless

# Test custom backend
./backend-test-harness.py python my-backend.py
```

### 3. Worker Config Validator

Validates worker configuration YAML files.

```bash
# Validate a worker config
./worker-config-validator.py ~/.forge/workers/claude-code-sonnet.yaml

# Validate all configs
for config in ~/.forge/workers/*.yaml; do
  ./worker-config-validator.py "$config"
done
```

### 4. Log Format Validator

Validates worker log files for correct format.

```bash
# Validate log format
./log-format-validator.py ~/.forge/logs/sonnet-alpha.log

# Show sample entries
./log-format-validator.py ~/.forge/logs/sonnet-alpha.log --show-samples
```

### 5. Status File Validator

Validates worker status JSON files.

```bash
# Validate status file
./status-file-validator.py ~/.forge/status/sonnet-alpha.json

# Show contents
./status-file-validator.py ~/.forge/status/sonnet-alpha.json --show
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

## Test Coverage by Integration Surface

### Launcher Testing
1. **Argument Parsing** - Accepts required args, rejects invalid
2. **Output Format** - Returns valid JSON with required fields
3. **Status File Creation** - Creates `~/.forge/status/<worker-id>.json`
4. **Log File Creation** - Creates `~/.forge/logs/<worker-id>.log`
5. **Process Spawning** - Actually spawns a running process/tmux session

### Backend Testing
1. **Input Handling** - Accepts JSON on stdin
2. **Output Format** - Returns valid JSON with tool_calls
3. **Tool Call Generation** - Generates appropriate tool calls
4. **Context Awareness** - Uses provided context
5. **Error Handling** - Handles malformed input gracefully
6. **Performance** - Responds within timeout (<30s)

### Worker Config Validation
1. **YAML Syntax** - Valid YAML format
2. **Required Fields** - Has name, launcher, model, tier
3. **Tier Values** - Valid tier (premium/standard/budget/free)
4. **Cost Information** - Correct cost_per_million_tokens format
5. **Subscription Data** - Valid subscription configuration
6. **Environment Vars** - No hardcoded secrets
7. **File Paths** - Contains ${worker_id} placeholders

### Log Format Validation
1. **Format Detection** - Detects JSON lines or key-value
2. **Required Fields** - Has timestamp, level, worker_id
3. **Timestamp Format** - Valid ISO 8601 timestamps
4. **Log Levels** - Valid levels (debug/info/warning/error)
5. **Message/Event** - Has message or event field

### Status File Validation
1. **JSON Syntax** - Valid JSON format
2. **Required Fields** - Has worker_id, status, model, workspace
3. **Status Values** - Valid status (active/idle/failed/stopped)
4. **Timestamps** - Valid ISO 8601 format
5. **Field Types** - Correct types (strings, integers)
6. **Consistency** - Logically consistent (e.g., stopped has stopped_at)

---

## Files

- `launcher-test-harness.py` - Launcher protocol testing
- `backend-test-harness.py` - Chat backend testing
- `worker-config-validator.py` - Worker config validation
- `log-format-validator.py` - Log format validation
- `status-file-validator.py` - Status file validation
- `example-launcher-passing.sh` - Reference launcher implementation
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
