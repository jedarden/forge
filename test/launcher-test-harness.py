#!/usr/bin/env python3
"""
FORGE Launcher Test Harness

Tests launcher scripts for protocol compliance.

Usage:
    ./launcher-test-harness.py <launcher-path>
    ./launcher-test-harness.py ~/.forge/launchers/claude-code-launcher
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

        # Kill by PID if we can
        try:
            # Try to read PID from output or status file
            if status_file.exists():
                with open(status_file) as f:
                    status = json.load(f)
                    pid = status.get("pid")
                    if pid:
                        os.kill(pid, 15)  # SIGTERM
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
            print(f"❌ FAIL - Failed with valid args")
            if result.get("stderr"):
                print(f"  Error: {result['stderr'][:100]}")
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
            print(f"❌ FAIL - Launcher failed")
            if result.get("stderr"):
                print(f"  Error: {result['stderr'][:100]}")
            return False

        # Parse JSON
        try:
            output = json.loads(result["stdout"])
        except json.JSONDecodeError as e:
            print(f"❌ FAIL - Invalid JSON: {e}")
            print(f"  Output: {result['stdout'][:100]}")
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

            if status["status"] not in ["active", "idle", "starting", "spawned"]:
                print(f"❌ FAIL - Invalid status: {status['status']}")
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

        # Try to validate log format (if JSON)
        try:
            with open(log_file) as f:
                first_line = f.readline()
                if first_line.strip().startswith('{'):
                    json.loads(first_line)  # Validate JSON
        except:
            pass  # Non-JSON format is okay

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

        if not self.launcher_path.exists():
            print(f"❌ ERROR: Launcher not found: {self.launcher_path}")
            return False

        if not os.access(self.launcher_path, os.X_OK):
            print(f"❌ ERROR: Launcher not executable: {self.launcher_path}")
            print(f"  Run: chmod +x {self.launcher_path}")
            return False

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
            try:
                if test():
                    passed += 1
                else:
                    failed += 1
            except Exception as e:
                print(f"❌ EXCEPTION: {e}")
                failed += 1

        print(f"\n{'='*60}")
        print(f"Results: {passed} passed, {failed} failed")
        print(f"{'='*60}\n")

        return failed == 0


def main():
    if len(sys.argv) < 2:
        print("Usage: launcher-test-harness.py <launcher-path>")
        print()
        print("Examples:")
        print("  ./launcher-test-harness.py ~/.forge/launchers/claude-code-launcher")
        print("  ./launcher-test-harness.py /path/to/my-launcher")
        sys.exit(1)

    launcher_path = sys.argv[1]

    tester = LauncherTest(launcher_path)
    success = tester.run_all_tests()

    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
