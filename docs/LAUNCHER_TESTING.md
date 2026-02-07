# Launcher Testing Framework

**Comprehensive testing harness for FORGE worker launchers**

---

## Quick Start

```bash
# Test a launcher
forge test-launcher ~/.forge/launchers/my-launcher

# Run full test suite
forge test-launcher ~/.forge/launchers/my-launcher --full

# Test with specific configuration
forge test-launcher my-launcher --model=sonnet --workspace=/tmp/test

# Dry run (show what would be tested)
forge test-launcher my-launcher --dry-run
```

---

## Test Categories

The launcher harness validates:

1. **Protocol Compliance** - Follows input/output specification
2. **File Creation** - Creates required status/log files
3. **Process Management** - Successfully spawns and manages worker
4. **Error Handling** - Gracefully handles failures
5. **Cleanup** - Properly terminates and cleans up
6. **Performance** - Meets timing requirements
7. **Idempotency** - Can be called multiple times safely

---

## Test Specification

### Test 1: Argument Parsing

**Validates**: Launcher accepts required arguments

```bash
# Test with minimal args
~/.forge/launchers/test-launcher \
  --model=sonnet \
  --workspace=/tmp/test \
  --session-name=test-worker

# Expected: Exit code 0, valid JSON output

# Test with missing args
~/.forge/launchers/test-launcher --model=sonnet

# Expected: Exit code 1, error message on stderr
```

**Pass criteria**:
- ✅ Accepts `--model=<string>`
- ✅ Accepts `--workspace=<path>`
- ✅ Accepts `--session-name=<string>`
- ✅ Returns exit code 1 if required args missing
- ✅ Prints error to stderr (not stdout) on failure

---

### Test 2: Output Format

**Validates**: Launcher returns valid JSON on stdout

```bash
OUTPUT=$(~/.forge/launchers/test-launcher \
  --model=sonnet \
  --workspace=/tmp/test \
  --session-name=test-worker)

# Parse JSON
echo "$OUTPUT" | jq .
```

**Pass criteria**:
- ✅ Output is valid JSON
- ✅ Contains `worker_id` field (string)
- ✅ Contains `pid` field (integer)
- ✅ Contains `status` field (must be "spawned")
- ✅ Optional: `launcher`, `timestamp` fields
- ✅ No extra output on stdout (only JSON)

**Example valid output**:
```json
{
  "worker_id": "test-worker",
  "pid": 12345,
  "status": "spawned",
  "launcher": "test-launcher",
  "timestamp": "2026-02-07T10:30:00Z"
}
```

---

### Test 3: Status File Creation

**Validates**: Creates status file at correct location

```bash
# Launch worker
~/.forge/launchers/test-launcher \
  --model=sonnet \
  --workspace=/tmp/test \
  --session-name=test-worker

# Check status file exists
test -f ~/.forge/status/test-worker.json
```

**Pass criteria**:
- ✅ File exists at `~/.forge/status/<worker-id>.json`
- ✅ File contains valid JSON
- ✅ Contains required fields: `worker_id`, `status`, `model`, `workspace`
- ✅ `status` is one of: `active`, `idle`, `starting`
- ✅ File is readable (chmod 644)
- ✅ Created within 5 seconds of launch

**Example valid status file**:
```json
{
  "worker_id": "test-worker",
  "status": "active",
  "model": "sonnet",
  "workspace": "/tmp/test",
  "pid": 12345,
  "started_at": "2026-02-07T10:30:00Z",
  "last_activity": "2026-02-07T10:30:00Z",
  "current_task": null,
  "tasks_completed": 0
}
```

---

### Test 4: Log File Creation

**Validates**: Creates log file and writes initial entry

```bash
# Launch worker
~/.forge/launchers/test-launcher \
  --model=sonnet \
  --workspace=/tmp/test \
  --session-name=test-worker

# Check log file exists
test -f ~/.forge/logs/test-worker.log

# Check log has content
test -s ~/.forge/logs/test-worker.log
```

**Pass criteria**:
- ✅ File exists at `~/.forge/logs/<worker-id>.log`
- ✅ File is not empty
- ✅ Contains at least one log entry
- ✅ Log entry is valid JSON (if using JSON format)
- ✅ First entry has `message` like "Worker started"
- ✅ File is appendable (chmod 644)
- ✅ Created within 5 seconds of launch

**Example valid log entry**:
```json
{"timestamp": "2026-02-07T10:30:00Z", "level": "info", "worker_id": "test-worker", "message": "Worker started"}
```

---

### Test 5: Process Spawning

**Validates**: Actually spawns a running process

```bash
# Launch worker
OUTPUT=$(~/.forge/launchers/test-launcher \
  --model=sonnet \
  --workspace=/tmp/test \
  --session-name=test-worker)

# Extract PID
PID=$(echo "$OUTPUT" | jq -r .pid)

# Check process exists
ps -p $PID > /dev/null
```

**Pass criteria**:
- ✅ PID in output corresponds to running process
- ✅ Process is still running after 5 seconds
- ✅ Process has expected command line (if verifiable)
- ✅ Process is in expected directory (workspace)

**For tmux-based launchers**:
```bash
# Check tmux session exists
tmux has-session -t test-worker
```

**For Docker-based launchers**:
```bash
# Check container exists
docker ps | grep test-worker
```

---

### Test 6: Error Handling

**Validates**: Gracefully handles various error conditions

#### Test 6a: Invalid Model
```bash
~/.forge/launchers/test-launcher \
  --model=invalid-model-xyz \
  --workspace=/tmp/test \
  --session-name=test-worker

# Expected: Exit code != 0, error on stderr
```

**Pass criteria**:
- ✅ Returns non-zero exit code
- ✅ Prints error message to stderr
- ✅ Does not create status file
- ✅ Does not create log file
- ✅ Does not spawn process

#### Test 6b: Invalid Workspace
```bash
~/.forge/launchers/test-launcher \
  --model=sonnet \
  --workspace=/nonexistent/path \
  --session-name=test-worker

# Expected: Exit code != 0, error on stderr
```

#### Test 6c: Duplicate Session Name
```bash
# Launch first worker
~/.forge/launchers/test-launcher \
  --model=sonnet \
  --workspace=/tmp/test \
  --session-name=test-worker

# Launch duplicate
~/.forge/launchers/test-launcher \
  --model=sonnet \
  --workspace=/tmp/test \
  --session-name=test-worker

# Expected: Exit code != 0 OR overwrites gracefully
```

---

### Test 7: Cleanup

**Validates**: Properly terminates worker and cleans up

```bash
# Launch worker
OUTPUT=$(~/.forge/launchers/test-launcher \
  --model=sonnet \
  --workspace=/tmp/test \
  --session-name=test-worker)

SESSION_NAME=$(echo "$OUTPUT" | jq -r .worker_id)

# Kill worker (tmux example)
tmux kill-session -t "$SESSION_NAME"

# OR send SIGTERM to PID
PID=$(echo "$OUTPUT" | jq -r .pid)
kill $PID

# Wait for cleanup
sleep 2

# Verify process stopped
! ps -p $PID > /dev/null
```

**Pass criteria**:
- ✅ Process terminates within 5 seconds of SIGTERM
- ✅ Tmux session terminates (if applicable)
- ✅ Docker container stops (if applicable)
- ✅ Status file updated to `status: "stopped"`
- ✅ Final log entry written
- ✅ No zombie processes left

---

### Test 8: Idempotency

**Validates**: Can be called multiple times without issues

```bash
# Launch worker 1
~/.forge/launchers/test-launcher \
  --model=sonnet \
  --workspace=/tmp/test \
  --session-name=test-worker-1

# Launch worker 2 (different session)
~/.forge/launchers/test-launcher \
  --model=sonnet \
  --workspace=/tmp/test \
  --session-name=test-worker-2

# Launch worker 3 (yet another)
~/.forge/launchers/test-launcher \
  --model=opus \
  --workspace=/tmp/test \
  --session-name=test-worker-3
```

**Pass criteria**:
- ✅ All three workers spawn successfully
- ✅ Each has separate status file
- ✅ Each has separate log file
- ✅ No conflicts between workers
- ✅ All can run simultaneously

---

### Test 9: Performance

**Validates**: Meets timing requirements

```bash
START=$(date +%s)
~/.forge/launchers/test-launcher \
  --model=sonnet \
  --workspace=/tmp/test \
  --session-name=test-worker
END=$(date +%s)

DURATION=$((END - START))
```

**Pass criteria**:
- ✅ Launcher returns within 10 seconds
- ✅ Status file created within 5 seconds
- ✅ Log file created within 5 seconds
- ✅ Process spawned within 10 seconds

---

### Test 10: Workspace Handling

**Validates**: Correctly handles workspace directory

```bash
# Create test workspace
mkdir -p /tmp/test-workspace
echo "test file" > /tmp/test-workspace/test.txt

# Launch worker
~/.forge/launchers/test-launcher \
  --model=sonnet \
  --workspace=/tmp/test-workspace \
  --session-name=test-worker

# Verify worker is in correct directory
# (Check process cwd or tmux session cwd)
```

**Pass criteria**:
- ✅ Worker process runs in specified workspace
- ✅ Relative paths resolve from workspace
- ✅ Can access files in workspace

---

## Automated Test Harness

### Test Runner Script

`~/.forge/test/launcher-test-harness.py`:

```python
#!/usr/bin/env python3
"""
FORGE Launcher Test Harness

Tests launcher scripts for protocol compliance.
"""

import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Dict, List, Optional

class LauncherTest:
    def __init__(self, launcher_path: str):
        self.launcher_path = Path(launcher_path).expanduser()
        self.test_results = []
        self.test_dir = Path("/tmp/forge-test")
        self.test_dir.mkdir(parents=True, exist_ok=True)

    def setup(self):
        """Setup test environment"""
        # Create test directories
        (Path.home() / ".forge/logs").mkdir(parents=True, exist_ok=True)
        (Path.home() / ".forge/status").mkdir(parents=True, exist_ok=True)

    def cleanup(self, worker_id: str):
        """Cleanup test artifacts"""
        # Remove test files
        log_file = Path.home() / f".forge/logs/{worker_id}.log"
        status_file = Path.home() / f".forge/status/{worker_id}.json"

        if log_file.exists():
            log_file.unlink()
        if status_file.exists():
            status_file.unlink()

        # Kill test worker (tmux example)
        try:
            subprocess.run(["tmux", "kill-session", "-t", worker_id],
                         stderr=subprocess.DEVNULL, check=False)
        except:
            pass

    def run_launcher(self, model: str, workspace: str, session_name: str) -> Dict:
        """Run launcher and return result"""
        try:
            result = subprocess.run(
                [
                    str(self.launcher_path),
                    f"--model={model}",
                    f"--workspace={workspace}",
                    f"--session-name={session_name}"
                ],
                capture_output=True,
                text=True,
                timeout=15
            )

            return {
                "success": result.returncode == 0,
                "exit_code": result.returncode,
                "stdout": result.stdout,
                "stderr": result.stderr
            }
        except subprocess.TimeoutExpired:
            return {
                "success": False,
                "exit_code": -1,
                "error": "Timeout (>15s)"
            }
        except Exception as e:
            return {
                "success": False,
                "error": str(e)
            }

    def test_1_argument_parsing(self) -> bool:
        """Test 1: Argument parsing"""
        print("Test 1: Argument Parsing...", end=" ")

        # Test with valid args
        result = self.run_launcher("sonnet", str(self.test_dir), "test-arg-parse")

        if not result["success"]:
            print(f"❌ FAIL - Failed with valid args: {result.get('stderr')}")
            return False

        # Test with missing args
        try:
            result = subprocess.run(
                [str(self.launcher_path), "--model=sonnet"],
                capture_output=True,
                timeout=5
            )
            if result.returncode == 0:
                print("❌ FAIL - Should fail with missing args")
                return False
        except:
            pass

        print("✅ PASS")
        self.cleanup("test-arg-parse")
        return True

    def test_2_output_format(self) -> bool:
        """Test 2: Output format"""
        print("Test 2: Output Format...", end=" ")

        result = self.run_launcher("sonnet", str(self.test_dir), "test-output")

        if not result["success"]:
            print(f"❌ FAIL - Launcher failed: {result.get('stderr')}")
            return False

        # Parse JSON
        try:
            output = json.loads(result["stdout"])
        except json.JSONDecodeError as e:
            print(f"❌ FAIL - Invalid JSON: {e}")
            return False

        # Check required fields
        required_fields = ["worker_id", "pid", "status"]
        for field in required_fields:
            if field not in output:
                print(f"❌ FAIL - Missing field: {field}")
                return False

        if output["status"] != "spawned":
            print(f"❌ FAIL - Status should be 'spawned', got '{output['status']}'")
            return False

        print("✅ PASS")
        self.cleanup("test-output")
        return True

    def test_3_status_file_creation(self) -> bool:
        """Test 3: Status file creation"""
        print("Test 3: Status File Creation...", end=" ")

        result = self.run_launcher("sonnet", str(self.test_dir), "test-status")

        if not result["success"]:
            print(f"❌ FAIL - Launcher failed")
            return False

        # Check status file exists
        status_file = Path.home() / ".forge/status/test-status.json"

        # Wait up to 5 seconds for file
        for _ in range(10):
            if status_file.exists():
                break
            time.sleep(0.5)

        if not status_file.exists():
            print(f"❌ FAIL - Status file not created: {status_file}")
            return False

        # Validate status file content
        try:
            with open(status_file) as f:
                status = json.load(f)

            required_fields = ["worker_id", "status", "model", "workspace"]
            for field in required_fields:
                if field not in status:
                    print(f"❌ FAIL - Status file missing field: {field}")
                    return False

        except Exception as e:
            print(f"❌ FAIL - Invalid status file: {e}")
            return False

        print("✅ PASS")
        self.cleanup("test-status")
        return True

    def test_4_log_file_creation(self) -> bool:
        """Test 4: Log file creation"""
        print("Test 4: Log File Creation...", end=" ")

        result = self.run_launcher("sonnet", str(self.test_dir), "test-log")

        if not result["success"]:
            print(f"❌ FAIL - Launcher failed")
            return False

        # Check log file exists
        log_file = Path.home() / ".forge/logs/test-log.log"

        # Wait up to 5 seconds for file
        for _ in range(10):
            if log_file.exists() and log_file.stat().st_size > 0:
                break
            time.sleep(0.5)

        if not log_file.exists():
            print(f"❌ FAIL - Log file not created")
            return False

        if log_file.stat().st_size == 0:
            print(f"❌ FAIL - Log file is empty")
            return False

        print("✅ PASS")
        self.cleanup("test-log")
        return True

    def test_5_process_spawning(self) -> bool:
        """Test 5: Process spawning"""
        print("Test 5: Process Spawning...", end=" ")

        result = self.run_launcher("sonnet", str(self.test_dir), "test-process")

        if not result["success"]:
            print(f"❌ FAIL - Launcher failed")
            return False

        # Extract PID
        try:
            output = json.loads(result["stdout"])
            pid = output["pid"]
        except:
            print(f"❌ FAIL - Could not extract PID")
            return False

        # Check process exists (or tmux session)
        time.sleep(2)

        # Try tmux first
        tmux_check = subprocess.run(
            ["tmux", "has-session", "-t", "test-process"],
            capture_output=True
        )

        if tmux_check.returncode == 0:
            print("✅ PASS (tmux session)")
            self.cleanup("test-process")
            return True

        # Try PID check
        try:
            os.kill(pid, 0)  # Check if process exists
            print("✅ PASS (process)")
            self.cleanup("test-process")
            return True
        except OSError:
            print(f"❌ FAIL - Process {pid} not running")
            return False

    def run_all_tests(self) -> bool:
        """Run all tests"""
        print(f"\n{'='*60}")
        print(f"Testing launcher: {self.launcher_path}")
        print(f"{'='*60}\n")

        self.setup()

        tests = [
            self.test_1_argument_parsing,
            self.test_2_output_format,
            self.test_3_status_file_creation,
            self.test_4_log_file_creation,
            self.test_5_process_spawning,
        ]

        passed = 0
        failed = 0

        for test in tests:
            if test():
                passed += 1
            else:
                failed += 1

        print(f"\n{'='*60}")
        print(f"Results: {passed} passed, {failed} failed")
        print(f"{'='*60}\n")

        return failed == 0


def main():
    if len(sys.argv) < 2:
        print("Usage: launcher-test-harness.py <launcher-path>")
        sys.exit(1)

    launcher_path = sys.argv[1]

    if not Path(launcher_path).exists():
        print(f"Error: Launcher not found: {launcher_path}")
        sys.exit(1)

    tester = LauncherTest(launcher_path)
    success = tester.run_all_tests()

    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
```

---

## Usage

### Basic Testing

```bash
# Test a launcher
python ~/.forge/test/launcher-test-harness.py ~/.forge/launchers/my-launcher

# Expected output:
# ============================================================
# Testing launcher: ~/.forge/launchers/my-launcher
# ============================================================
#
# Test 1: Argument Parsing... ✅ PASS
# Test 2: Output Format... ✅ PASS
# Test 3: Status File Creation... ✅ PASS
# Test 4: Log File Creation... ✅ PASS
# Test 5: Process Spawning... ✅ PASS
#
# ============================================================
# Results: 5 passed, 0 failed
# ============================================================
```

### Integration with FORGE CLI

```bash
# Add to FORGE CLI
forge test-launcher <launcher-name-or-path>

# Run full test suite
forge test-launcher my-launcher --full

# Run specific test
forge test-launcher my-launcher --test=output-format

# Test all launchers
forge test-all-launchers

# CI mode (exit code 1 on any failure)
forge test-launcher my-launcher --ci
```

---

## CI/CD Integration

### GitHub Actions

`.github/workflows/test-launchers.yml`:

```yaml
name: Test Launchers

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3

      - name: Setup Python
        uses: actions/setup-python@v4
        with:
          python-version: '3.10'

      - name: Install dependencies
        run: |
          pip install pytest jq
          sudo apt-get install -y tmux

      - name: Test all launchers
        run: |
          for launcher in ~/.forge/launchers/*; do
            if [ -x "$launcher" ]; then
              echo "Testing $launcher"
              python ~/.forge/test/launcher-test-harness.py "$launcher" || exit 1
            fi
          done
```

---

## Next Steps

1. **Install test harness**: Copy script to `~/.forge/test/`
2. **Test your launchers**: Run harness on existing launchers
3. **Fix issues**: Address any failing tests
4. **Integrate with CI**: Add to your CI/CD pipeline
5. **Contribute**: Share test results with community

---

**FORGE** - Federated Orchestration & Resource Generation Engine
