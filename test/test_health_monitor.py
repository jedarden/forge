#!/usr/bin/env python3
"""
FORGE Worker Health Monitor Tests

Tests the worker health monitoring implementation:
- PID existence check
- Log activity check (recent activity within 5 minutes)
- Status file validation
- Tmux session aliveness check
- Health status aggregation
- Worker marking as failed

Per ADR 0014: No automatic recovery, user decides.
"""

import json
import os
import tempfile
import time
from datetime import datetime, timedelta
from pathlib import Path
from unittest import mock

import pytest

# Add src to path for imports
import sys
sys.path.insert(0, str(Path(__file__).parent.parent / "src"))

from forge.health_monitor import (
    WorkerHealthMonitor,
    HealthCheckResult,
    HealthCheckType,
    ErrorType,
    WorkerHealthStatus,
    HealthMonitoringLoop,
    check_worker_health,
)


# =============================================================================
# Test Fixtures
# =============================================================================


@pytest.fixture
def temp_dirs():
    """Create temporary directories for status and log files"""
    with tempfile.TemporaryDirectory() as tmpdir:
        status_dir = Path(tmpdir) / "status"
        log_dir = Path(tmpdir) / "logs"
        status_dir.mkdir()
        log_dir.mkdir()
        yield status_dir, log_dir


@pytest.fixture
def sample_status_file(temp_dirs):
    """Create a sample worker status file"""
    status_dir, log_dir = temp_dirs
    worker_id = "test-worker"
    status_file = status_dir / f"{worker_id}.json"

    status_data = {
        "worker_id": worker_id,
        "status": "active",
        "model": "sonnet",
        "workspace": "/test/workspace",
        "pid": 12345,
        "started_at": datetime.now().isoformat(),
        "last_activity": datetime.now().isoformat(),
        "current_task": None,
        "tasks_completed": 0,
    }

    status_file.write_text(json.dumps(status_data))

    return worker_id, status_file, status_data


@pytest.fixture
def sample_log_file(temp_dirs):
    """Create a sample worker log file"""
    status_dir, log_dir = temp_dirs
    worker_id = "test-worker"
    log_file = log_dir / f"{worker_id}.log"

    # Write a log entry
    log_entry = {
        "timestamp": datetime.now().isoformat(),
        "level": "info",
        "worker_id": worker_id,
        "message": "Test log entry",
    }
    log_file.write_text(json.dumps(log_entry) + "\n")

    return worker_id, log_file


# =============================================================================
# PID Check Tests
# =============================================================================


class TestPIDCheck:
    """Tests for PID existence health check"""

    def test_pid_exists(self, sample_status_file, temp_dirs):
        """Test that existing PID is detected"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        # Use actual current process PID to ensure it exists
        status_data["pid"] = os.getpid()
        status_file.write_text(json.dumps(status_data))

        monitor = WorkerHealthMonitor(status_dir, log_dir)
        result = monitor._check_pid_exists(
            monitor.parser.parse(status_file)
        )

        assert result.check_type == HealthCheckType.PID_EXISTS
        assert result.passed is True
        assert result.error_type is None
        assert result.error_message is None

    def test_pid_not_exists(self, sample_status_file, temp_dirs):
        """Test that non-existing PID is detected"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        # Use a non-existent PID
        status_data["pid"] = 999999999
        status_file.write_text(json.dumps(status_data))

        monitor = WorkerHealthMonitor(status_dir, log_dir)
        result = monitor._check_pid_exists(
            monitor.parser.parse(status_file)
        )

        assert result.check_type == HealthCheckType.PID_EXISTS
        assert result.passed is False
        assert result.error_type == ErrorType.DEAD_PROCESS
        assert "died" in result.error_message.lower()
        assert "999999999" in result.error_message

    def test_pid_none(self, sample_status_file, temp_dirs):
        """Test that missing PID is handled"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        # Remove PID
        del status_data["pid"]
        status_file.write_text(json.dumps(status_data))

        monitor = WorkerHealthMonitor(status_dir, log_dir)
        result = monitor._check_pid_exists(
            monitor.parser.parse(status_file)
        )

        assert result.check_type == HealthCheckType.PID_EXISTS
        assert result.passed is False
        assert result.error_type == ErrorType.UNKNOWN
        assert "no pid" in result.error_message.lower()


# =============================================================================
# Log Activity Check Tests
# =============================================================================


class TestLogActivityCheck:
    """Tests for log activity health check"""

    def test_log_activity_recent(self, sample_status_file, sample_log_file, temp_dirs):
        """Test that recent log activity is detected"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        monitor = WorkerHealthMonitor(status_dir, log_dir)
        result = monitor._check_log_activity(worker_id, monitor.parser.parse(status_file))

        assert result.check_type == HealthCheckType.LOG_ACTIVITY
        assert result.passed is True
        assert result.error_type is None

    def test_log_activity_stale(self, sample_status_file, temp_dirs):
        """Test that stale log activity is detected"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        # Create an old log file
        log_file = log_dir / f"{worker_id}.log"
        log_file.write_text("old log entry\n")

        # Set modification time to 10 minutes ago
        old_time = time.time() - (10 * 60)
        os.utime(log_file, (old_time, old_time))

        monitor = WorkerHealthMonitor(status_dir, log_dir)
        result = monitor._check_log_activity(worker_id, monitor.parser.parse(status_file))

        assert result.check_type == HealthCheckType.LOG_ACTIVITY
        assert result.passed is False
        assert result.error_type == ErrorType.STALE_LOG
        assert "10 minutes" in result.error_message or "10" in result.error_message

    def test_log_file_missing(self, sample_status_file, temp_dirs):
        """Test that missing log file is detected"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        # Don't create log file

        monitor = WorkerHealthMonitor(status_dir, log_dir)
        result = monitor._check_log_activity(worker_id, monitor.parser.parse(status_file))

        assert result.check_type == HealthCheckType.LOG_ACTIVITY
        assert result.passed is False
        assert result.error_type == ErrorType.UNKNOWN
        assert "not found" in result.error_message.lower()


# =============================================================================
# Status File Validation Tests
# =============================================================================


class TestStatusFileCheck:
    """Tests for status file validation health check"""

    def test_status_file_valid(self, sample_status_file, temp_dirs):
        """Test that valid status file passes"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        monitor = WorkerHealthMonitor(status_dir, log_dir)
        result = monitor._check_status_file(monitor.parser.parse(status_file))

        assert result.check_type == HealthCheckType.STATUS_FILE
        assert result.passed is True
        assert result.error_type is None

    def test_status_file_corrupted(self, temp_dirs):
        """Test that corrupted status file is detected"""
        status_dir, log_dir = temp_dirs
        worker_id = "test-worker"
        status_file = status_dir / f"{worker_id}.json"

        # Write invalid JSON
        status_file.write_text("{invalid json}")

        monitor = WorkerHealthMonitor(status_dir, log_dir)
        result = monitor._check_status_file(monitor.parser.parse(status_file))

        assert result.check_type == HealthCheckType.STATUS_FILE
        assert result.passed is False
        assert result.error_type == ErrorType.CORRUPTED_STATUS

    def test_status_file_stale_activity(self, sample_status_file, temp_dirs):
        """Test that stale last_activity for active worker is detected"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        # Set last_activity to 20 minutes ago
        old_time = datetime.now() - timedelta(minutes=20)
        status_data["last_activity"] = old_time.isoformat()
        status_file.write_text(json.dumps(status_data))

        monitor = WorkerHealthMonitor(status_dir, log_dir)
        result = monitor._check_status_file(monitor.parser.parse(status_file))

        assert result.check_type == HealthCheckType.STATUS_FILE
        assert result.passed is False
        assert result.error_type == ErrorType.STALE_LOG
        assert "20" in result.error_message


# =============================================================================
# Tmux Session Check Tests
# =============================================================================


class TestTmuxSessionCheck:
    """Tests for tmux session health check"""

    def test_tmux_not_installed(self, sample_status_file, temp_dirs):
        """Test that missing tmux is handled gracefully"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        monitor = WorkerHealthMonitor(status_dir, log_dir)

        # Mock subprocess.run to raise FileNotFoundError (tmux not installed)
        with mock.patch('subprocess.run', side_effect=FileNotFoundError):
            result = monitor._check_tmux_session(worker_id, monitor.parser.parse(status_file))

        # Should return None when tmux is not available
        assert result is None

    def test_tmux_session_not_found(self, sample_status_file, temp_dirs):
        """Test that missing tmux session is detected"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        monitor = WorkerHealthMonitor(status_dir, log_dir)

        # Mock subprocess.run to return non-zero (session not found)
        completed_process = mock.Mock()
        completed_process.returncode = 1
        with mock.patch('subprocess.run', return_value=completed_process):
            result = monitor._check_tmux_session(worker_id, monitor.parser.parse(status_file))

        assert result is not None
        assert result.check_type == HealthCheckType.TMUX_SESSION
        assert result.passed is False
        assert result.error_type == ErrorType.MISSING_SESSION


# =============================================================================
# Overall Health Check Tests
# =============================================================================


class TestWorkerHealthCheck:
    """Tests for overall worker health check"""

    def test_healthy_worker(self, sample_status_file, sample_log_file, temp_dirs):
        """Test that healthy worker passes core checks (PID, log, status)"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        # Use current process PID to ensure it exists
        status_data["pid"] = os.getpid()
        status_file.write_text(json.dumps(status_data))

        health_status = check_worker_health(worker_id, status_dir, log_dir)

        assert health_status.worker_id == worker_id
        # Core checks should pass: PID, log activity, status file
        # Tmux may fail if session doesn't exist, which is expected
        assert health_status.health_score >= 0.75  # At least 3/4 checks pass
        assert HealthCheckType.PID_EXISTS not in health_status.failed_checks
        assert HealthCheckType.LOG_ACTIVITY not in health_status.failed_checks
        assert HealthCheckType.STATUS_FILE not in health_status.failed_checks

    def test_unhealthy_worker_dead_process(self, sample_status_file, sample_log_file, temp_dirs):
        """Test that dead process is detected"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        # Use non-existent PID
        status_data["pid"] = 999999999
        status_file.write_text(json.dumps(status_data))

        health_status = check_worker_health(worker_id, status_dir, log_dir)

        assert health_status.worker_id == worker_id
        assert health_status.is_healthy is False
        assert health_status.health_score < 1.0
        assert HealthCheckType.PID_EXISTS in health_status.failed_checks
        assert "died" in health_status.primary_error.lower()
        assert len(health_status.guidance) > 0

    def test_unhealthy_worker_stale_log(self, sample_status_file, temp_dirs):
        """Test that stale log is detected"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        # Use current process PID
        status_data["pid"] = os.getpid()
        status_file.write_text(json.dumps(status_data))

        # Create stale log file
        log_file = log_dir / f"{worker_id}.log"
        log_file.write_text("old log\n")
        old_time = time.time() - (10 * 60)
        os.utime(log_file, (old_time, old_time))

        health_status = check_worker_health(worker_id, status_dir, log_dir)

        assert health_status.worker_id == worker_id
        assert health_status.is_healthy is False
        assert HealthCheckType.LOG_ACTIVITY in health_status.failed_checks
        assert "stuck" in health_status.primary_error.lower()

    def test_health_guidance_generation(self, sample_status_file, sample_log_file, temp_dirs):
        """Test that actionable guidance is generated per ADR 0014"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        # Use non-existent PID
        status_data["pid"] = 12345
        status_file.write_text(json.dumps(status_data))

        health_status = check_worker_health(worker_id, status_dir, log_dir)

        # Should have guidance for user action
        assert len(health_status.guidance) > 0
        # Guidance should be actionable
        assert any("ps" in g.lower() or "view logs" in g.lower() or "restart" in g.lower()
                   for g in health_status.guidance)


# =============================================================================
# Health Monitoring Loop Tests
# =============================================================================


class TestHealthMonitoringLoop:
    """Tests for health monitoring loop"""

    @pytest.mark.asyncio
    async def test_loop_starts_and_stops(self, temp_dirs):
        """Test that health monitoring loop can start and stop"""
        status_dir, log_dir = temp_dirs

        monitor = WorkerHealthMonitor(status_dir, log_dir)
        unhealthy_workers = []

        def on_unhealthy(worker_id, health_status):
            unhealthy_workers.append((worker_id, health_status))

        loop = HealthMonitoringLoop(monitor, status_dir, on_unhealthy)

        # Start loop
        await loop.start(interval_seconds=1)
        assert loop.is_running is True

        # Stop loop
        await loop.stop()
        assert loop.is_running is False

    @pytest.mark.asyncio
    async def test_loop_detects_unhealthy_worker(self, temp_dirs):
        """Test that loop detects and marks unhealthy workers"""
        status_dir, log_dir = temp_dirs

        # Create a worker with non-existent PID
        worker_id = "unhealthy-worker"
        status_file = status_dir / f"{worker_id}.json"
        status_data = {
            "worker_id": worker_id,
            "status": "active",
            "model": "sonnet",
            "workspace": "/test/workspace",
            "pid": 999999999,  # Non-existent
            "started_at": datetime.now().isoformat(),
            "last_activity": datetime.now().isoformat(),
        }
        status_file.write_text(json.dumps(status_data))

        # Create log file
        log_file = log_dir / f"{worker_id}.log"
        log_file.write_text("test log\n")

        monitor = WorkerHealthMonitor(status_dir, log_dir)
        unhealthy_workers = []

        def on_unhealthy(worker_id, health_status):
            unhealthy_workers.append((worker_id, health_status))

        loop = HealthMonitoringLoop(monitor, status_dir, on_unhealthy)

        # Run health checks
        await loop._run_health_checks()

        # Should have detected unhealthy worker
        assert len(unhealthy_workers) > 0
        detected_id, health_status = unhealthy_workers[0]
        assert detected_id == worker_id
        assert health_status.is_healthy is False

        # Status file should be updated to failed
        updated_status = monitor.parser.parse(status_file)
        assert updated_status.status.value == "failed"
        assert "health_error" in updated_status.raw_data


# =============================================================================
# Convenience Function Tests
# =============================================================================


class TestConvenienceFunctions:
    """Tests for convenience functions"""

    def test_check_worker_health_convenience(self, sample_status_file, sample_log_file, temp_dirs):
        """Test convenience function for checking worker health"""
        worker_id, status_file, status_data = sample_status_file
        status_dir, log_dir = temp_dirs

        # Use current process PID
        status_data["pid"] = os.getpid()
        status_file.write_text(json.dumps(status_data))

        health_status = check_worker_health(worker_id, status_dir, log_dir)

        assert health_status.worker_id == worker_id
        assert isinstance(health_status, WorkerHealthStatus)


# =============================================================================
# Run Tests
# =============================================================================


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
