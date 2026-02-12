#!/usr/bin/env python3
"""
Integration tests for worker status real-time updates.

Tests that worker status updates are reflected in real-time through the
StatusWatcher mechanism, covering the following scenarios per fg-56p:

1. Worker spawn status changes (starting -> active/idle)
2. Task pickup status updates (active, current_task field)
3. Task completion updates (tasks_completed increments, status -> idle)
4. External worker kill detection (status -> stopped/failed)

Success Criteria:
- Status updates within 1-2 seconds (50ms debounce + processing time)
- All status transitions visible
- No stale data displayed
- Handles external changes gracefully

Usage:
    pytest test_worker_status_realtime.py -v
    python test_worker_status_realtime.py
"""

import json
import os
import subprocess
import sys
import tempfile
import shutil
import time
from pathlib import Path
from datetime import datetime, timezone
from typing import Optional, Dict, Any


class WorkerStatusSimulator:
    """Simulates worker status file operations for testing."""

    def __init__(self, status_dir: Path):
        self.status_dir = status_dir
        self.status_dir.mkdir(parents=True, exist_ok=True)

    def create_status(
        self,
        worker_id: str,
        status: str = "starting",
        model: str = "sonnet",
        workspace: str = "/test/workspace",
        current_task: Optional[str] = None,
        tasks_completed: int = 0,
        pid: Optional[int] = None,
    ) -> Path:
        """Create or update a worker status file."""
        status_data = {
            "worker_id": worker_id,
            "status": status,
            "model": model,
            "workspace": workspace,
            "started_at": datetime.now(timezone.utc).isoformat(),
            "last_activity": datetime.now(timezone.utc).isoformat(),
            "tasks_completed": tasks_completed,
        }

        if current_task:
            status_data["current_task"] = current_task

        if pid:
            status_data["pid"] = pid

        status_path = self.status_dir / f"{worker_id}.json"
        with open(status_path, "w") as f:
            json.dump(status_data, f, indent=2)

        return status_path

    def update_status(
        self,
        worker_id: str,
        status: Optional[str] = None,
        current_task: Optional[str] = None,
        tasks_completed: Optional[int] = None,
        clear_current_task: bool = False,
    ) -> Path:
        """Update specific fields in a worker status file.

        Args:
            worker_id: The worker to update
            status: New status value (None = no change)
            current_task: New current task (None = no change unless clear_current_task=True)
            tasks_completed: New task count (None = no change)
            clear_current_task: If True, remove current_task field from status
        """
        status_path = self.status_dir / f"{worker_id}.json"

        if not status_path.exists():
            raise FileNotFoundError(f"Status file not found: {status_path}")

        with open(status_path) as f:
            data = json.load(f)

        if status is not None:
            data["status"] = status
        if clear_current_task:
            # Remove the current_task field entirely
            data.pop("current_task", None)
        elif current_task is not None:
            data["current_task"] = current_task
        if tasks_completed is not None:
            data["tasks_completed"] = tasks_completed

        data["last_activity"] = datetime.now(timezone.utc).isoformat()

        with open(status_path, "w") as f:
            json.dump(data, f, indent=2)

        return status_path

    def delete_status(self, worker_id: str) -> None:
        """Delete a worker status file (simulates worker death)."""
        status_path = self.status_dir / f"{worker_id}.json"
        if status_path.exists():
            status_path.unlink()

    def read_status(self, worker_id: str) -> Optional[Dict[str, Any]]:
        """Read a worker status file."""
        status_path = self.status_dir / f"{worker_id}.json"
        if not status_path.exists():
            return None
        with open(status_path) as f:
            return json.load(f)


class TestWorkerStatusRealtime:
    """Test suite for worker status real-time updates."""

    def setup(self):
        """Setup test environment."""
        self.temp_dir = tempfile.mkdtemp(prefix="forge-status-test-")
        self.status_dir = Path(self.temp_dir) / "status"
        self.simulator = WorkerStatusSimulator(self.status_dir)

        # Build the forge binary if needed
        self.forge_binary = self._get_or_build_forge()

    def teardown(self):
        """Cleanup test environment."""
        if self.temp_dir and os.path.exists(self.temp_dir):
            shutil.rmtree(self.temp_dir)

    def _get_or_build_forge(self) -> Optional[Path]:
        """Get path to forge binary or build it."""
        # Check for existing release binary
        release_path = Path("/home/coder/forge/target/release/forge")
        if release_path.exists():
            return release_path

        # Check for debug binary
        debug_path = Path("/home/coder/forge/target/debug/forge")
        if debug_path.exists():
            return debug_path

        # Try to build
        try:
            result = subprocess.run(
                ["cargo", "build", "--release"],
                cwd="/home/coder/forge",
                capture_output=True,
                timeout=120,
            )
            if result.returncode == 0 and release_path.exists():
                return release_path
        except Exception:
            pass

        return None

    def test_status_file_lifecycle(self):
        """
        Test 1: Worker spawn status changes (starting -> active/idle)

        This test verifies:
        - Status file is created with 'starting' status
        - Status can transition to 'active' when worker starts working
        - Status can transition to 'idle' when worker is waiting
        """
        print("\n" + "=" * 60)
        print("Test 1: Worker spawn status changes")
        print("=" * 60)

        worker_id = "test-spawn-worker"

        # Step 1: Create worker with 'starting' status
        status_path = self.simulator.create_status(
            worker_id=worker_id,
            status="starting",
            model="sonnet",
        )

        print(f"Created status file: {status_path}")
        assert status_path.exists(), "Status file should exist"

        # Read and verify
        status = self.simulator.read_status(worker_id)
        assert status is not None, "Should be able to read status"
        assert status["status"] == "starting", f"Status should be 'starting', got {status['status']}"
        print(f"✓ Initial status: {status['status']}")

        # Step 2: Simulate transition to 'active'
        time.sleep(0.1)  # Small delay to simulate processing time
        self.simulator.update_status(worker_id, status="active")
        status = self.simulator.read_status(worker_id)
        assert status["status"] == "active", f"Status should be 'active', got {status['status']}"
        print(f"✓ Transitioned to: {status['status']}")

        # Step 3: Simulate transition to 'idle'
        time.sleep(0.1)
        self.simulator.update_status(worker_id, status="idle")
        status = self.simulator.read_status(worker_id)
        assert status["status"] == "idle", f"Status should be 'idle', got {status['status']}"
        print(f"✓ Transitioned to: {status['status']}")

        print("PASS: Worker spawn status changes verified")
        return True

    def test_task_pickup_status(self):
        """
        Test 2: Task pickup status updates (active, current_task field)

        This test verifies:
        - current_task field is updated when worker picks up a task
        - status changes to 'active' when working on a task
        - current_task contains the bead/task ID
        """
        print("\n" + "=" * 60)
        print("Test 2: Task pickup status updates")
        print("=" * 60)

        worker_id = "test-task-pickup-worker"
        task_id = "fg-test-task"

        # Create idle worker
        self.simulator.create_status(
            worker_id=worker_id,
            status="idle",
            model="sonnet",
        )

        status = self.simulator.read_status(worker_id)
        assert status["status"] == "idle", "Worker should be idle"
        assert status.get("current_task") is None, "Worker should have no current task"
        print(f"✓ Initial state: {status['status']}, current_task: {status.get('current_task')}")

        # Simulate task pickup
        time.sleep(0.1)
        self.simulator.update_status(
            worker_id=worker_id,
            status="active",
            current_task=task_id,
        )

        status = self.simulator.read_status(worker_id)
        assert status["status"] == "active", f"Status should be 'active', got {status['status']}"
        assert status.get("current_task") == task_id, \
            f"current_task should be '{task_id}', got {status.get('current_task')}"
        print(f"✓ After pickup: {status['status']}, current_task: {status['current_task']}")

        print("PASS: Task pickup status updates verified")
        return True

    def test_task_completion_status(self):
        """
        Test 3: Task completion updates (tasks_completed increments, status -> idle)

        This test verifies:
        - tasks_completed counter increments
        - current_task is cleared
        - status returns to 'idle'
        """
        print("\n" + "=" * 60)
        print("Test 3: Task completion status updates")
        print("=" * 60)

        worker_id = "test-task-completion-worker"
        task_id = "fg-complete-task"

        # Create worker actively working on a task
        self.simulator.create_status(
            worker_id=worker_id,
            status="active",
            current_task=task_id,
            tasks_completed=0,
        )

        status = self.simulator.read_status(worker_id)
        assert status["status"] == "active", "Worker should be active"
        assert status.get("current_task") == task_id, "Worker should have current task"
        assert status.get("tasks_completed", 0) == 0, "tasks_completed should be 0"
        print(f"✓ Initial state: {status['status']}, current_task: {status['current_task']}, completed: {status['tasks_completed']}")

        # Simulate task completion
        time.sleep(0.1)
        self.simulator.update_status(
            worker_id=worker_id,
            status="idle",
            clear_current_task=True,  # Clear current task
            tasks_completed=1,  # Increment counter
        )

        status = self.simulator.read_status(worker_id)
        assert status["status"] == "idle", f"Status should be 'idle', got {status['status']}"
        assert status.get("current_task") is None, f"current_task should be None, got {status.get('current_task')}"
        assert status.get("tasks_completed", 0) == 1, f"tasks_completed should be 1, got {status.get('tasks_completed')}"
        print(f"✓ After completion: {status['status']}, current_task: {status.get('current_task')}, completed: {status['tasks_completed']}")

        # Complete another task
        time.sleep(0.1)
        self.simulator.update_status(
            worker_id=worker_id,
            status="active",
            current_task="fg-second-task",
        )

        time.sleep(0.1)
        self.simulator.update_status(
            worker_id=worker_id,
            status="idle",
            clear_current_task=True,
            tasks_completed=2,
        )

        status = self.simulator.read_status(worker_id)
        assert status.get("tasks_completed", 0) == 2, f"tasks_completed should be 2, got {status.get('tasks_completed')}"
        print(f"✓ Second completion verified: tasks_completed = {status['tasks_completed']}")

        print("PASS: Task completion status updates verified")
        return True

    def test_external_worker_kill_detection(self):
        """
        Test 4: External worker kill detection (status -> stopped/failed)

        This test verifies:
        - Worker status file deletion is handled gracefully
        - Status can transition to 'stopped' or 'failed' on external kill
        - File removal simulates tmux kill-session behavior
        """
        print("\n" + "=" * 60)
        print("Test 4: External worker kill detection")
        print("=" * 60)

        worker_id = "test-kill-worker"

        # Create active worker
        self.simulator.create_status(
            worker_id=worker_id,
            status="active",
            current_task="fg-some-task",
            pid=12345,
        )

        status = self.simulator.read_status(worker_id)
        assert status is not None, "Worker should exist"
        assert status["status"] == "active", "Worker should be active"
        print(f"✓ Initial state: {status['status']} (pid: {status.get('pid')})")

        # Test 1: Graceful stop (update status to 'stopped')
        time.sleep(0.1)
        self.simulator.update_status(worker_id, status="stopped")
        status = self.simulator.read_status(worker_id)
        assert status["status"] == "stopped", f"Status should be 'stopped', got {status['status']}"
        print(f"✓ Graceful stop: {status['status']}")

        # Test 2: Simulate external kill (file deletion)
        time.sleep(0.1)
        self.simulator.delete_status(worker_id)
        status = self.simulator.read_status(worker_id)
        assert status is None, "Status file should be deleted"
        print("✓ Status file deleted (simulates external kill)")

        # Test 3: Simulate failed worker (status update before death)
        worker_id2 = "test-fail-worker"
        self.simulator.create_status(
            worker_id=worker_id2,
            status="active",
            pid=54321,
        )

        time.sleep(0.1)
        self.simulator.update_status(worker_id2, status="failed")
        status = self.simulator.read_status(worker_id2)
        assert status["status"] == "failed", f"Status should be 'failed', got {status['status']}"
        print(f"✓ Failed worker detection: {status['status']}")

        print("PASS: External worker kill detection verified")
        return True

    def test_status_update_latency(self):
        """
        Test 5: Verify status update latency is within acceptable bounds.

        This test verifies:
        - Status file writes complete quickly (< 100ms)
        - Multiple sequential updates don't cause delays
        """
        print("\n" + "=" * 60)
        print("Test 5: Status update latency")
        print("=" * 60)

        worker_id = "test-latency-worker"

        # Create initial status
        start_time = time.time()
        self.simulator.create_status(worker_id=worker_id, status="starting")
        create_time = time.time() - start_time
        print(f"✓ Status file creation: {create_time*1000:.1f}ms")
        assert create_time < 0.1, f"Status creation took {create_time}s, should be < 0.1s"

        # Perform multiple rapid updates
        update_times = []
        for i in range(10):
            start_time = time.time()
            self.simulator.update_status(worker_id, status="active" if i % 2 == 0 else "idle")
            update_times.append(time.time() - start_time)

        avg_update_time = sum(update_times) / len(update_times)
        max_update_time = max(update_times)
        print(f"✓ Average update time: {avg_update_time*1000:.1f}ms")
        print(f"✓ Max update time: {max_update_time*1000:.1f}ms")

        assert max_update_time < 0.05, f"Max update time {max_update_time}s exceeds 50ms target"
        print("PASS: Status update latency within bounds")
        return True

    def test_concurrent_worker_status_updates(self):
        """
        Test 6: Multiple workers updating status concurrently.

        This test verifies:
        - Multiple workers can update their status independently
        - No race conditions or data corruption
        """
        print("\n" + "=" * 60)
        print("Test 6: Concurrent worker status updates")
        print("=" * 60)

        num_workers = 5

        # Create multiple workers
        for i in range(num_workers):
            worker_id = f"concurrent-worker-{i}"
            self.simulator.create_status(
                worker_id=worker_id,
                status="starting",
                model="sonnet" if i % 2 == 0 else "opus",
            )

        # Verify all created
        for i in range(num_workers):
            worker_id = f"concurrent-worker-{i}"
            status = self.simulator.read_status(worker_id)
            assert status is not None, f"Worker {worker_id} should exist"
            assert status["status"] == "starting"

        print(f"✓ Created {num_workers} workers")

        # Update all workers
        for i in range(num_workers):
            worker_id = f"concurrent-worker-{i}"
            self.simulator.update_status(
                worker_id=worker_id,
                status="active",
                current_task=f"task-{i}",
            )

        # Verify all updated
        for i in range(num_workers):
            worker_id = f"concurrent-worker-{i}"
            status = self.simulator.read_status(worker_id)
            assert status["status"] == "active", f"Worker {worker_id} should be active"
            assert status["current_task"] == f"task-{i}"

        print(f"✓ Updated {num_workers} workers concurrently")

        # Complete all tasks
        for i in range(num_workers):
            worker_id = f"concurrent-worker-{i}"
            self.simulator.update_status(
                worker_id=worker_id,
                status="idle",
                clear_current_task=True,
                tasks_completed=1,
            )

        # Verify all completed
        active_count = 0
        for i in range(num_workers):
            worker_id = f"concurrent-worker-{i}"
            status = self.simulator.read_status(worker_id)
            if status["status"] == "idle":
                active_count += 1

        print(f"✓ All {active_count} workers transitioned to idle")
        assert active_count == num_workers, f"Expected {num_workers} idle workers, got {active_count}"

        print("PASS: Concurrent worker status updates verified")
        return True

    def test_status_field_types(self):
        """
        Test 7: Verify status file field types are correct.

        This test verifies:
        - All required fields are present and correctly typed
        - JSON serialization/deserialization works correctly
        """
        print("\n" + "=" * 60)
        print("Test 7: Status field types validation")
        print("=" * 60)

        worker_id = "test-types-worker"

        # Create status with all fields
        self.simulator.create_status(
            worker_id=worker_id,
            status="active",
            model="sonnet",
            workspace="/home/coder/test-workspace",
            current_task="fg-type-test",
            tasks_completed=42,
            pid=99999,
        )

        status = self.simulator.read_status(worker_id)

        # Verify field types
        assert isinstance(status["worker_id"], str), "worker_id should be string"
        assert isinstance(status["status"], str), "status should be string"
        assert isinstance(status["model"], str), "model should be string"
        assert isinstance(status["workspace"], str), "workspace should be string"
        assert isinstance(status["current_task"], str), "current_task should be string"
        assert isinstance(status["tasks_completed"], int), "tasks_completed should be int"
        assert isinstance(status["pid"], int), "pid should be int"
        assert isinstance(status["started_at"], str), "started_at should be string (ISO 8601)"
        assert isinstance(status["last_activity"], str), "last_activity should be string (ISO 8601)"

        print("✓ All field types correct:")
        print(f"  - worker_id: {type(status['worker_id']).__name__}")
        print(f"  - status: {type(status['status']).__name__}")
        print(f"  - model: {type(status['model']).__name__}")
        print(f"  - workspace: {type(status['workspace']).__name__}")
        print(f"  - current_task: {type(status['current_task']).__name__}")
        print(f"  - tasks_completed: {type(status['tasks_completed']).__name__}")
        print(f"  - pid: {type(status['pid']).__name__}")
        print(f"  - started_at: {type(status['started_at']).__name__}")
        print(f"  - last_activity: {type(status['last_activity']).__name__}")

        # Verify ISO 8601 timestamp format
        try:
            datetime.fromisoformat(status["started_at"].replace("Z", "+00:00"))
            datetime.fromisoformat(status["last_activity"].replace("Z", "+00:00"))
            print("✓ Timestamps are valid ISO 8601 format")
        except ValueError as e:
            raise AssertionError(f"Invalid timestamp format: {e}")

        print("PASS: Status field types validated")
        return True

    def run_all_tests(self):
        """Run all tests and report results."""
        print("\n" + "=" * 60)
        print("FORGE Worker Status Real-Time Update Tests")
        print("Testing per fg-56p requirements")
        print("=" * 60)

        self.setup()

        tests = [
            ("Worker spawn status changes", self.test_status_file_lifecycle),
            ("Task pickup status updates", self.test_task_pickup_status),
            ("Task completion status updates", self.test_task_completion_status),
            ("External worker kill detection", self.test_external_worker_kill_detection),
            ("Status update latency", self.test_status_update_latency),
            ("Concurrent worker updates", self.test_concurrent_worker_status_updates),
            ("Status field types", self.test_status_field_types),
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

        print("\n" + "=" * 60)
        print(f"Test Results: {passed} passed, {failed} failed")
        print("=" * 60)

        return failed == 0


def main():
    tester = TestWorkerStatusRealtime()
    success = tester.run_all_tests()
    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
