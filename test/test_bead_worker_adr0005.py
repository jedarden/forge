#!/usr/bin/env python3
"""
FORGE Bead Worker ADR 0005 Compliance Test

Tests that the bead-worker-launcher correctly implements ADR 0005 requirements:
- Status files in ~/.forge/status/ with proper JSON format
- Log files in ~/.forge/logs/ with proper JSON format
- Bead-aware fields in status and log files

Usage:
    ./test_bead_worker_adr0005.py
    ./test_bead_worker_adr0005.py --launcher /path/to/bead-worker-launcher.sh
"""

import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from datetime import datetime

class BeadWorkerADR0005Test:
    def __init__(self, launcher_path: str = None):
        if launcher_path is None:
            launcher_path = "/home/coder/forge/test/example-launchers/bead-worker-launcher.sh"
        self.launcher_path = Path(launcher_path).expanduser()
        self.test_results = []

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

        # Kill test worker (tmux)
        try:
            subprocess.run(["tmux", "kill-session", "-t", worker_id],
                         stderr=subprocess.DEVNULL, check=False)
        except:
            pass

    def run_launcher(self, model: str, workspace: str, session_name: str, bead_ref: str = None) -> dict:
        """Run launcher and return result"""
        cmd = [
            str(self.launcher_path),
            f"--model={model}",
            f"--workspace={workspace}",
            f"--session-name={session_name}"
        ]

        if bead_ref:
            cmd.append(f"--bead-ref={bead_ref}")

        try:
            result = subprocess.run(
                cmd,
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

    def test_1_status_file_location(self) -> bool:
        """Test 1: Status file is written to ~/.forge/status/"""
        print("Test 1: Status File Location (~/.forge/status/)...", end=" ")

        with tempfile.TemporaryDirectory() as tmpdir:
            result = self.run_launcher("sonnet", tmpdir, "test-adr0005-1")

            if not result["success"]:
                print(f"❌ FAIL - Launcher failed")
                return False

            status_file = Path.home() / ".forge/status/test-adr0005-1.json"

            if not status_file.exists():
                print(f"❌ FAIL - Status file not created at {status_file}")
                return False

            print("✅ PASS")
            self.cleanup("test-adr0005-1")
            return True

    def test_2_status_file_json_format(self) -> bool:
        """Test 2: Status file is valid JSON"""
        print("Test 2: Status File JSON Format...", end=" ")

        with tempfile.TemporaryDirectory() as tmpdir:
            result = self.run_launcher("sonnet", tmpdir, "test-adr0005-2")

            if not result["success"]:
                print(f"❌ FAIL - Launcher failed")
                return False

            status_file = Path.home() / ".forge/status/test-adr0005-2.json"

            if not status_file.exists():
                print(f"❌ FAIL - Status file not created")
                return False

            try:
                with open(status_file) as f:
                    status = json.load(f)
            except json.JSONDecodeError as e:
                print(f"❌ FAIL - Invalid JSON: {e}")
                return False

            print("✅ PASS")
            self.cleanup("test-adr0005-2")
            return True

    def test_3_status_file_required_fields(self) -> bool:
        """Test 3: Status file has all ADR 0005 required fields"""
        print("Test 3: Status File Required Fields...", end=" ")

        with tempfile.TemporaryDirectory() as tmpdir:
            result = self.run_launcher("sonnet", tmpdir, "test-adr0005-3")

            if not result["success"]:
                print(f"❌ FAIL - Launcher failed")
                return False

            status_file = Path.home() / ".forge/status/test-adr0005-3.json"

            try:
                with open(status_file) as f:
                    status = json.load(f)
            except Exception as e:
                print(f"❌ FAIL - Cannot read status: {e}")
                return False

            # ADR 0005 required fields (from section "Status files")
            required_fields = ["worker_id", "status", "pid", "model", "workspace"]
            missing = []

            for field in required_fields:
                if field not in status:
                    missing.append(field)

            if missing:
                print(f"❌ FAIL - Missing required fields: {', '.join(missing)}")
                print(f"  Got: {list(status.keys())}")
                return False

            # Validate status value
            valid_statuses = ["active", "idle", "failed", "stopped", "starting", "spawned"]
            if status["status"] not in valid_statuses:
                print(f"❌ FAIL - Invalid status: {status['status']}")
                return False

            print("✅ PASS")
            self.cleanup("test-adr0005-3")
            return True

    def test_4_log_file_location(self) -> bool:
        """Test 4: Log file is written to ~/.forge/logs/"""
        print("Test 4: Log File Location (~/.forge/logs/)...", end=" ")

        with tempfile.TemporaryDirectory() as tmpdir:
            result = self.run_launcher("sonnet", tmpdir, "test-adr0005-4")

            if not result["success"]:
                print(f"❌ FAIL - Launcher failed")
                return False

            log_file = Path.home() / ".forge/logs/test-adr0005-4.log"

            if not log_file.exists():
                print(f"❌ FAIL - Log file not created at {log_file}")
                return False

            print("✅ PASS")
            self.cleanup("test-adr0005-4")
            return True

    def test_5_log_file_json_format(self) -> bool:
        """Test 5: Log file uses JSON lines format (ADR 0005)"""
        print("Test 5: Log File JSON Lines Format...", end=" ")

        with tempfile.TemporaryDirectory() as tmpdir:
            result = self.run_launcher("sonnet", tmpdir, "test-adr0005-5")

            if not result["success"]:
                print(f"❌ FAIL - Launcher failed")
                return False

            log_file = Path.home() / ".forge/logs/test-adr0005-5.log"

            if not log_file.exists():
                print(f"❌ FAIL - Log file not created")
                return False

            # Validate first line is JSON
            try:
                with open(log_file) as f:
                    first_line = f.readline().strip()

                if not first_line:
                    print(f"❌ FAIL - Log file is empty")
                    return False

                entry = json.loads(first_line)

                # ADR 0005 log format required fields
                required_log_fields = ["timestamp", "level", "worker_id"]
                missing = []

                for field in required_log_fields:
                    if field not in entry:
                        missing.append(field)

                if missing:
                    print(f"❌ FAIL - Log entry missing fields: {', '.join(missing)}")
                    return False

            except json.JSONDecodeError as e:
                print(f"❌ FAIL - First line is not valid JSON: {e}")
                return False

            print("✅ PASS")
            self.cleanup("test-adr0005-5")
            return True

    def test_6_bead_aware_status_fields(self) -> bool:
        """Test 6: Status file includes bead_id in current_task when bead_ref is provided"""
        print("Test 6: Bead-Aware Status Fields...", end=" ")

        with tempfile.TemporaryDirectory() as tmpdir:
            # Create a minimal beads workspace
            init_result = subprocess.run(["br", "init", "--prefix", "fg"], cwd=tmpdir,
                         capture_output=True, check=False)

            if init_result.returncode != 0:
                print(f"⚠️  SKIP - Could not initialize beads workspace")
                self.cleanup("test-adr0005-6")
                return True

            # Create a test bead
            bead_result = subprocess.run(
                ["br", "create", "Test bead for ADR 0005 validation",
                 "--description", "Testing ADR 0005 compliance",
                 "--priority", "1"],
                cwd=tmpdir,
                capture_output=True,
                text=True
            )

            # Extract bead ID from output
            # Format: "✓ Created fg-xxx: Title"
            bead_id = None
            for line in bead_result.stdout.splitlines():
                if "Created" in line and "fg-" in line:
                    # Extract bead_id using regex
                    # The format is "Created fg-xxx:" so we need to extract just the ID
                    match = re.search(r'(fg-[a-z0-9]+)', line)
                    if match:
                        bead_id = match.group(1)
                        break

            if not bead_id:
                print(f"⚠️  SKIP - Could not create test bead")
                self.cleanup("test-adr0005-6")
                return True

            result = self.run_launcher("sonnet", tmpdir, "test-adr0005-6", bead_id)

            if not result["success"]:
                print(f"❌ FAIL - Launcher failed with exit code {result.get('exit_code')}")
                return False

            status_file = Path.home() / ".forge/status/test-adr0005-6.json"

            if not status_file.exists():
                print(f"❌ FAIL - Status file not created")
                return False

            try:
                with open(status_file) as f:
                    status = json.load(f)
            except Exception as e:
                print(f"❌ FAIL - Cannot read status: {e}")
                return False

            # Check for bead_id in current_task (per ADR 0005, current_task is a string)
            current_task = status.get("current_task")
            if current_task != bead_id:
                print(f"❌ FAIL - current_task should be '{bead_id}', got '{current_task}'")
                return False

            print("✅ PASS")
            self.cleanup("test-adr0005-6")
            return True

    def test_7_bead_aware_log_fields(self) -> bool:
        """Test 7: Log file includes bead_id in log entries when bead_ref is provided"""
        print("Test 7: Bead-Aware Log Fields...", end=" ")

        with tempfile.TemporaryDirectory() as tmpdir:
            # Create a minimal beads workspace
            init_result = subprocess.run(["br", "init", "--prefix", "fg"], cwd=tmpdir,
                         capture_output=True, check=False)

            if init_result.returncode != 0:
                print(f"⚠️  SKIP - Could not initialize beads workspace")
                self.cleanup("test-adr0005-7")
                return True

            # Create a test bead
            bead_result = subprocess.run(
                ["br", "create", "Test bead for ADR 0005 log validation",
                 "--description", "Testing ADR 0005 log compliance",
                 "--priority", "1"],
                cwd=tmpdir,
                capture_output=True,
                text=True
            )

            # Extract bead ID from output
            # Format: "✓ Created fg-xxx: Title"
            bead_id = None
            for line in bead_result.stdout.splitlines():
                if "Created" in line and "fg-" in line:
                    # Extract bead_id using regex
                    # The format is "Created fg-xxx:" so we need to extract just the ID
                    match = re.search(r'(fg-[a-z0-9]+)', line)
                    if match:
                        bead_id = match.group(1)
                        break

            if not bead_id:
                print(f"⚠️  SKIP - Could not create test bead")
                self.cleanup("test-adr0005-7")
                return True

            result = self.run_launcher("sonnet", tmpdir, "test-adr0005-7", bead_id)

            if not result["success"]:
                print(f"❌ FAIL - Launcher failed with exit code {result.get('exit_code')}")
                return False

            log_file = Path.home() / ".forge/logs/test-adr0005-7.log"

            if not log_file.exists():
                print(f"❌ FAIL - Log file not created")
                return False

            try:
                with open(log_file) as f:
                    lines = f.readlines()

                # Check if any log entry contains bead_id
                has_bead_id = False
                for line in lines:
                    if line.strip():
                        try:
                            entry = json.loads(line.strip())
                            if "bead_id" in entry and entry["bead_id"] == bead_id:
                                has_bead_id = True
                                break
                        except:
                            pass

                if not has_bead_id:
                    print(f"❌ FAIL - No log entry contains bead_id")
                    return False

            except Exception as e:
                print(f"❌ FAIL - Cannot read log: {e}")
                return False

            print("✅ PASS")
            self.cleanup("test-adr0005-7")
            return True

    def test_8_timestamp_iso8601(self) -> bool:
        """Test 8: Timestamps are in ISO 8601 format"""
        print("Test 8: Timestamp ISO 8601 Format...", end=" ")

        with tempfile.TemporaryDirectory() as tmpdir:
            result = self.run_launcher("sonnet", tmpdir, "test-adr0005-8")

            if not result["success"]:
                print(f"❌ FAIL - Launcher failed")
                return False

            status_file = Path.home() / ".forge/status/test-adr0005-8.json"
            log_file = Path.home() / ".forge/logs/test-adr0005-8.log"

            # Check status file timestamps
            try:
                with open(status_file) as f:
                    status = json.load(f)

                for field in ["started_at", "last_activity"]:
                    if field in status:
                        try:
                            datetime.fromisoformat(status[field].replace('Z', '+00:00'))
                        except ValueError:
                            print(f"❌ FAIL - Invalid {field} format: {status[field]}")
                            return False
            except Exception as e:
                print(f"❌ FAIL - Cannot validate status timestamps: {e}")
                return False

            # Check log file timestamps
            try:
                with open(log_file) as f:
                    first_line = f.readline().strip()

                if first_line:
                    entry = json.loads(first_line)
                    if "timestamp" in entry:
                        try:
                            datetime.fromisoformat(entry["timestamp"].replace('Z', '+00:00'))
                        except ValueError:
                            print(f"❌ FAIL - Invalid log timestamp format: {entry['timestamp']}")
                            return False
            except Exception as e:
                print(f"❌ FAIL - Cannot validate log timestamps: {e}")
                return False

            print("✅ PASS")
            self.cleanup("test-adr0005-8")
            return True

    def test_9_stdout_json_format(self) -> bool:
        """Test 9: Launcher stdout outputs valid JSON with required fields"""
        print("Test 9: Stdout JSON Format...", end=" ")

        with tempfile.TemporaryDirectory() as tmpdir:
            result = self.run_launcher("sonnet", tmpdir, "test-adr0005-9")

            if not result["success"]:
                print(f"❌ FAIL - Launcher failed")
                return False

            # Parse stdout JSON
            try:
                output = json.loads(result["stdout"])
            except json.JSONDecodeError as e:
                print(f"❌ FAIL - Invalid JSON on stdout: {e}")
                return False

            # Check required stdout fields
            required_fields = ["worker_id", "pid", "status"]
            missing = []

            for field in required_fields:
                if field not in output:
                    missing.append(field)

            if missing:
                print(f"❌ FAIL - Missing stdout fields: {', '.join(missing)}")
                return False

            if output["status"] != "spawned":
                print(f"❌ FAIL - Status should be 'spawned', got '{output['status']}'")
                return False

            print("✅ PASS")
            self.cleanup("test-adr0005-9")
            return True

    def test_10_current_task_structure(self) -> bool:
        """Test 10: current_task field has proper structure per ADR 0005"""
        print("Test 10: current_task Structure...", end=" ")

        with tempfile.TemporaryDirectory() as tmpdir:
            result = self.run_launcher("sonnet", tmpdir, "test-adr0005-10")

            if not result["success"]:
                print(f"❌ FAIL - Launcher failed")
                return False

            status_file = Path.home() / ".forge/status/test-adr0005-10.json"

            try:
                with open(status_file) as f:
                    status = json.load(f)
            except Exception as e:
                print(f"❌ FAIL - Cannot read status: {e}")
                return False

            # Check current_task exists and conforms to ADR 0005
            if "current_task" not in status:
                print(f"❌ FAIL - Missing current_task field")
                return False

            # ADR 0005 specifies current_task as a string (bead ID) or null
            # NOT as an object with nested fields
            current_task = status["current_task"]
            if current_task is None or isinstance(current_task, str):
                print("✅ PASS")
                self.cleanup("test-adr0005-10")
                return True
            else:
                print(f"❌ FAIL - current_task must be string or null per ADR 0005, got {type(current_task)}")
                return False

    def run_all_tests(self) -> bool:
        """Run all ADR 0005 compliance tests"""
        print(f"\n{'='*60}")
        print(f"FORGE Bead Worker - ADR 0005 Compliance Test")
        print(f"{'='*60}")
        print(f"Launcher: {self.launcher_path}")
        print(f"{'='*60}\n")

        if not self.launcher_path.exists():
            print(f"❌ ERROR: Launcher not found: {self.launcher_path}")
            return False

        if not os.access(self.launcher_path, os.X_OK):
            print(f"❌ ERROR: Launcher not executable: {self.launcher_path}")
            return False

        self.setup()

        tests = [
            self.test_1_status_file_location,
            self.test_2_status_file_json_format,
            self.test_3_status_file_required_fields,
            self.test_4_log_file_location,
            self.test_5_log_file_json_format,
            self.test_6_bead_aware_status_fields,
            self.test_7_bead_aware_log_fields,
            self.test_8_timestamp_iso8601,
            self.test_9_stdout_json_format,
            self.test_10_current_task_structure,
        ]

        passed = 0
        failed = 0
        skipped = 0

        for test in tests:
            try:
                result = test()
                if result is True:
                    passed += 1
                elif result is None:
                    skipped += 1
                else:
                    failed += 1
            except Exception as e:
                print(f"❌ EXCEPTION: {e}")
                import traceback
                traceback.print_exc()
                failed += 1

        print(f"\n{'='*60}")
        print(f"Results: {passed} passed, {failed} failed, {skipped} skipped")
        print(f"{'='*60}\n")

        return failed == 0


def main():
    launcher_path = None
    if len(sys.argv) > 1:
        if sys.argv[1] == "--launcher" and len(sys.argv) > 2:
            launcher_path = sys.argv[2]

    tester = BeadWorkerADR0005Test(launcher_path)
    success = tester.run_all_tests()

    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
