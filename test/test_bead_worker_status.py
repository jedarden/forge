#!/usr/bin/env python3
"""
Integration tests for bead-worker-launcher.sh

Tests that the launcher correctly writes status files to ~/.forge/status/
and log files to ~/.forge/logs/ according to ADR 0005 specification.

Usage:
    pytest test_bead_worker_status.py
    python test_bead_worker_status.py
"""

import json
import os
import re
import subprocess
import sys
import tempfile
import shutil
from pathlib import Path
from datetime import datetime
import time

class BeadWorkerLauncherTest:
    def __init__(self):
        self.test_workspace = None
        self.original_home = None
        self.test_forge_dir = None
        self.launcher_path = Path(__file__).parent.parent / "scripts" / "launchers" / "bead-worker-launcher.sh"

    def setup(self):
        """Setup test environment"""
        # Create temporary workspace
        self.test_workspace = tempfile.mkdtemp(prefix="forge-test-workspace-")

        # Create test .forge directory in temp location
        self.test_forge_dir = tempfile.mkdtemp(prefix="forge-test-home-")

        # Create required subdirectories
        os.makedirs(os.path.join(self.test_forge_dir, ".forge", "logs"), exist_ok=True)
        os.makedirs(os.path.join(self.test_forge_dir, ".forge", "status"), exist_ok=True)

        print(f"Test workspace: {self.test_workspace}")
        print(f"Test .forge dir: {self.test_forge_dir}")

    def teardown(self):
        """Cleanup test environment"""
        # Kill any tmux sessions created during tests
        subprocess.run(["tmux", "kill-server"], stderr=subprocess.DEVNULL)

        if self.test_workspace and os.path.exists(self.test_workspace):
            shutil.rmtree(self.test_workspace)
        if self.test_forge_dir and os.path.exists(self.test_forge_dir):
            shutil.rmtree(self.test_forge_dir)

    def run_launcher(self, session_name, model="sonnet", bead_ref=None):
        """Run the bead-worker launcher and return result"""
        cmd = [
            str(self.launcher_path),
            f"--model={model}",
            f"--workspace={self.test_workspace}",
            f"--session-name={session_name}"
        ]

        if bead_ref:
            cmd.append(f"--bead-ref={bead_ref}")

        # Set HOME to test directory
        env = os.environ.copy()
        env["HOME"] = self.test_forge_dir

        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            env=env
        )

        return result

    def test_status_file_basic(self):
        """Test 1: Basic status file creation without bead"""
        print("\n" + "="*60)
        print("Test 1: Basic status file creation without bead")
        print("="*60)

        session = "test-worker-basic"
        result = self.run_launcher(session)

        print(f"Launcher exit code: {result.returncode}")
        print(f"Launcher stdout: {result.stdout[:200]}...")
        print(f"Launcher stderr: {result.stderr[:200]}...")

        # Check status file exists
        status_path = Path(self.test_forge_dir) / ".forge" / "status" / f"{session}.json"
        print(f"Status file path: {status_path}")
        print(f"Status file exists: {status_path.exists()}")

        if not status_path.exists():
            print("FAIL: Status file not created")
            return False

        # Read and validate status file
        with open(status_path) as f:
            status = json.load(f)

        print(f"Status content:\n{json.dumps(status, indent=2)}")

        # Validate required fields per ADR 0005
        required_fields = ["worker_id", "status", "model", "workspace", "pid", "started_at", "last_activity", "tasks_completed"]
        missing = [f for f in required_fields if f not in status]

        if missing:
            print(f"FAIL: Missing required fields: {missing}")
            return False

        # Validate field types and values
        tests = [
            (status["worker_id"] == session, f"worker_id mismatch: {status['worker_id']} != {session}"),
            (status["status"] in ["active", "idle", "failed", "stopped"], f"Invalid status: {status['status']}"),
            (status["model"] == "sonnet", f"model mismatch: {status['model']} != sonnet"),
            (status["workspace"] == self.test_workspace, f"workspace mismatch"),
            (isinstance(status["pid"], int), f"pid must be int, got {type(status['pid'])}"),
            (isinstance(status["tasks_completed"], int), f"tasks_completed must be int"),
        ]

        # Check optional ADR 0005 fields
        if "uptime_seconds" in status:
            tests.append((isinstance(status["uptime_seconds"], int), f"uptime_seconds must be int"))
            print("PASS: uptime_seconds field present (ADR 0005)")
        else:
            print("WARNING: uptime_seconds field missing (recommended by ADR 0005)")

        # Check current_task is string (not object) - ADR 0005 fix
        if "current_task" in status:
            if status["current_task"] is None or isinstance(status["current_task"], str):
                print("PASS: current_task is string or null (ADR 0005 compliant)")
            else:
                print(f"FAIL: current_task must be string, got {type(status['current_task'])}")
                return False

        for passed, msg in tests:
            if not passed:
                print(f"FAIL: {msg}")
                return False

        print("PASS: All validations passed")
        return True

    def test_status_file_with_bead(self):
        """Test 2: Status file with bead reference"""
        print("\n" + "="*60)
        print("Test 2: Status file with bead reference")
        print("="*60)

        # Initialize beads workspace first
        print("Initializing beads workspace...")
        init_result = subprocess.run(
            ["br", "init", "--prefix", "fg"],
            cwd=self.test_workspace,
            capture_output=True,
            text=True
        )
        if init_result.returncode != 0:
            print(f"SKIP: Failed to initialize beads workspace: {init_result.stderr}")
            return True  # Skip test rather than fail

        # Create a test bead
        print("Creating test bead...")
        bead_result = subprocess.run(
            ["br", "create", "Test bead for launcher validation",
             "--description", "Testing bead-worker-launcher status file integration",
             "--priority", "1"],
            cwd=self.test_workspace,
            capture_output=True,
            text=True
        )

        # Extract bead ID from output
        # Format: "✓ Created fg-xxx: Title"
        bead_id = None
        for line in bead_result.stdout.splitlines():
            if "Created" in line and "fg-" in line:
                # Extract bead_id using regex or string manipulation
                # The format is "Created fg-xxx:" so we need to extract just the ID
                import re
                match = re.search(r'(fg-[a-z0-9]+)', line)
                if match:
                    bead_id = match.group(1)
                    break

        if not bead_id:
            print(f"SKIP: Could not create test bead: {bead_result.stdout}")
            return True  # Skip test rather than fail

        print(f"Created test bead: {bead_id}")

        session = "test-worker-bead"
        result = self.run_launcher(session, bead_ref=bead_id)

        print(f"Launcher exit code: {result.returncode}")

        status_path = Path(self.test_forge_dir) / ".forge" / "status" / f"{session}.json"

        if not status_path.exists():
            print("FAIL: Status file not created")
            return False

        with open(status_path) as f:
            status = json.load(f)

        print(f"Status content:\n{json.dumps(status, indent=2)}")

        # Check current_task contains bead_id as string (not object)
        if "current_task" not in status:
            print("FAIL: current_task field missing")
            return False

        current_task = status["current_task"]
        if not isinstance(current_task, str):
            print(f"FAIL: current_task must be string, got {type(current_task)}: {current_task}")
            return False

        if current_task != bead_id:
            print(f"FAIL: current_task value mismatch: '{current_task}' != '{bead_id}'")
            print(f"  This usually means br commands failed in the launcher")
            return False

        print(f"PASS: current_task correctly set to bead_id: {bead_id}")
        return True

    def test_log_file_creation(self):
        """Test 3: Log file creation with proper format"""
        print("\n" + "="*60)
        print("Test 3: Log file creation with proper format")
        print("="*60)

        session = "test-worker-logs"
        result = self.run_launcher(session)

        log_path = Path(self.test_forge_dir) / ".forge" / "logs" / f"{session}.log"

        if not log_path.exists():
            print("FAIL: Log file not created")
            return False

        with open(log_path) as f:
            log_content = f.read()

        print(f"Log content:\n{log_content}")

        # Validate JSON log format per ADR 0005
        log_lines = [line.strip() for line in log_content.strip().split('\n') if line.strip()]

        if not log_lines:
            print("FAIL: Log file is empty")
            return False

        for i, line in enumerate(log_lines):
            try:
                entry = json.loads(line)
                print(f"Log entry {i}: {json.dumps(entry, indent=2)}")

                # Check required log fields per ADR 0005
                required = ["timestamp", "level", "worker_id", "message"]
                missing = [f for f in required if f not in entry]

                if missing:
                    print(f"FAIL: Log entry {i} missing fields: {missing}")
                    return False

                # Validate ISO 8601 timestamp
                try:
                    datetime.fromisoformat(entry["timestamp"].replace('Z', '+00:00'))
                except ValueError:
                    print(f"FAIL: Invalid timestamp format: {entry['timestamp']}")
                    return False

            except json.JSONDecodeError as e:
                print(f"FAIL: Log entry {i} is not valid JSON: {e}")
                return False

        print(f"PASS: All {len(log_lines)} log entries are valid")
        return True

    def test_json_output(self):
        """Test 4: Launcher outputs valid JSON on stdout"""
        print("\n" + "="*60)
        print("Test 4: Launcher outputs valid JSON on stdout")
        print("="*60)

        session = "test-worker-json"
        result = self.run_launcher(session)

        print(f"stdout: {result.stdout}")

        try:
            output = json.loads(result.stdout)
            print(f"Parsed output:\n{json.dumps(output, indent=2)}")

            # Check required output fields
            if "worker_id" not in output:
                print("FAIL: worker_id missing from output")
                return False

            if output["worker_id"] != session:
                print(f"FAIL: worker_id mismatch: {output['worker_id']} != {session}")
                return False

            if "status" not in output:
                print("FAIL: status missing from output")
                return False

            if output["status"] != "spawned":
                print(f"FAIL: Unexpected status: {output['status']}")
                return False

            print("PASS: JSON output is valid")
            return True

        except json.JSONDecodeError as e:
            print(f"FAIL: stdout is not valid JSON: {e}")
            return False

    def run_all_tests(self):
        """Run all tests and report results"""
        print("\n" + "="*60)
        print("FORGE Bead-Worker Launcher Integration Tests")
        print("="*60)

        self.setup()

        tests = [
            ("Status file basic creation", self.test_status_file_basic),
            ("Status file with bead reference", self.test_status_file_with_bead),
            ("Log file creation", self.test_log_file_creation),
            ("JSON output format", self.test_json_output),
        ]

        passed = 0
        failed = 0

        for name, test_func in tests:
            try:
                if test_func():
                    passed += 1
                else:
                    failed += 1
            except Exception as e:
                print(f"EXCEPTION in {name}: {e}")
                import traceback
                traceback.print_exc()
                failed += 1

        self.teardown()

        print("\n" + "="*60)
        print(f"Test Results: {passed} passed, {failed} failed")
        print("="*60)

        return failed == 0


def main():
    tester = BeadWorkerLauncherTest()
    success = tester.run_all_tests()
    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
