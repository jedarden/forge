"""
Tests for FORGE Status Watcher Module

Comprehensive tests for status file watching, parsing, and caching.
Tests inotify watching, polling fallback, error handling per ADR 0014.
"""

import asyncio
import json
from datetime import datetime
from pathlib import Path
from unittest.mock import AsyncMock, Mock, patch
from typing import Any

import pytest

from forge.status_watcher import (
    StatusWatcher,
    StatusFileEvent,
    StatusFileParser,
    WorkerStatusCache,
    WorkerStatusFile,
    WorkerStatusValue,
    InotifyStatusWatcher,
    PollingStatusWatcher,
    parse_status_file,
)


# =============================================================================
# Fixtures
# =============================================================================


@pytest.fixture
def sample_status_data():
    """Sample worker status data"""
    return {
        "worker_id": "test-worker-1",
        "status": "active",
        "model": "sonnet",
        "workspace": "/home/coder/test-project",
        "pid": 12345,
        "started_at": "2026-02-07T10:30:00Z",
        "last_activity": "2026-02-07T10:35:00Z",
        "current_task": "bd-abc",
        "tasks_completed": 5,
    }


@pytest.fixture
def temp_status_dir(tmp_path):
    """Create a temporary status directory"""
    status_dir = tmp_path / "status"
    status_dir.mkdir(parents=True, exist_ok=True)
    return status_dir


@pytest.fixture
def valid_status_file(temp_status_dir, sample_status_data):
    """Create a valid status file"""
    status_file = temp_status_dir / "test-worker-1.json"
    with open(status_file, "w") as f:
        json.dump(sample_status_data, f)
    return status_file


@pytest.fixture
def corrupted_status_file(temp_status_dir):
    """Create a corrupted status file (invalid JSON)"""
    status_file = temp_status_dir / "corrupted-worker.json"
    status_file.write_text("invalid json {")
    return status_file


@pytest.fixture
def missing_fields_status_file(temp_status_dir):
    """Create a status file with missing required fields"""
    status_file = temp_status_dir / "missing-fields.json"
    status_file.write_text('{"worker_id": "test", "status": "active"}')
    return status_file


@pytest.fixture
def invalid_status_file(temp_status_dir):
    """Create a status file with invalid status value"""
    status_file = temp_status_dir / "invalid-status.json"
    status_file.write_text('{"worker_id": "test", "status": "unknown", "model": "sonnet", "workspace": "/tmp"}')
    return status_file


# =============================================================================
# StatusFileParser Tests
# =============================================================================


class TestStatusFileParser:
    """Tests for StatusFileParser"""

    def test_parse_valid_status_file(self, valid_status_file):
        """Test parsing a valid status file"""
        parser = StatusFileParser()
        result = parser.parse(valid_status_file)

        assert result.worker_id == "test-worker-1"
        assert result.status == WorkerStatusValue.ACTIVE
        assert result.model == "sonnet"
        assert result.workspace == "/home/coder/test-project"
        assert result.pid == 12345
        assert result.started_at == "2026-02-07T10:30:00Z"
        assert result.current_task == "bd-abc"
        assert result.tasks_completed == 5
        assert result.error is None
        assert result.is_healthy

    def test_parse_corrupted_json(self, corrupted_status_file):
        """Test parsing a corrupted JSON file (ADR 0014)"""
        parser = StatusFileParser()
        result = parser.parse(corrupted_status_file)

        assert result.worker_id == "corrupted-worker"
        assert result.status == WorkerStatusValue.FAILED
        assert result.model == "unknown"
        assert result.workspace == "unknown"
        assert result.error is not None
        assert "json" in result.error.lower()
        assert result.is_error_state
        assert parser.parse_errors == 1

    def test_parse_missing_fields(self, missing_fields_status_file):
        """Test parsing a file with missing required fields (ADR 0014)"""
        parser = StatusFileParser()
        result = parser.parse(missing_fields_status_file)

        assert result.worker_id == "missing-fields"
        assert result.status == WorkerStatusValue.FAILED
        assert result.error is not None
        assert "missing" in result.error.lower()
        assert parser.parse_errors == 1

    def test_parse_invalid_status(self, invalid_status_file):
        """Test parsing a file with invalid status value (ADR 0014)"""
        parser = StatusFileParser()
        result = parser.parse(invalid_status_file)

        assert result.worker_id == "test"
        assert result.status == WorkerStatusValue.FAILED
        assert result.error is not None
        assert "invalid status" in result.error.lower()
        assert parser.parse_errors == 1

    def test_parse_nonexistent_file(self, temp_status_dir):
        """Test parsing a non-existent file (worker stopped)"""
        parser = StatusFileParser()
        result = parser.parse(temp_status_dir / "nonexistent.json")

        assert result.worker_id == "nonexistent"
        assert result.status == WorkerStatusValue.STOPPED
        assert result.model == "unknown"
        assert result.workspace == "unknown"
        assert result.error is None

    def test_parse_idle_status(self, temp_status_dir):
        """Test parsing an idle worker"""
        status_file = temp_status_dir / "idle-worker.json"
        status_file.write_text('{"worker_id": "idle-worker", "status": "idle", "model": "haiku", "workspace": "/tmp"}')

        parser = StatusFileParser()
        result = parser.parse(status_file)

        assert result.worker_id == "idle-worker"
        assert result.status == WorkerStatusValue.IDLE
        assert result.is_healthy

    def test_parse_failed_status(self, temp_status_dir):
        """Test parsing a failed worker"""
        status_file = temp_status_dir / "failed-worker.json"
        status_file.write_text('{"worker_id": "failed-worker", "status": "failed", "model": "opus", "workspace": "/tmp"}')

        parser = StatusFileParser()
        result = parser.parse(status_file)

        assert result.worker_id == "failed-worker"
        assert result.status == WorkerStatusValue.FAILED


# =============================================================================
# WorkerStatusCache Tests
# =============================================================================


class TestWorkerStatusCache:
    """Tests for WorkerStatusCache"""

    def test_empty_cache(self):
        """Test empty cache"""
        cache = WorkerStatusCache()
        assert cache.worker_count == 0
        assert cache.active_count == 0
        assert cache.idle_count == 0
        assert cache.failed_count == 0

    def test_cache_update_created(self, sample_status_data):
        """Test cache update on worker creation"""
        cache = WorkerStatusCache()
        # Create WorkerStatusFile with proper enum status
        data = sample_status_data.copy()
        data["status"] = WorkerStatusValue.ACTIVE
        status_file = WorkerStatusFile(**data)

        event = StatusFileEvent(
            worker_id="test-worker-1",
            event_type=StatusFileEvent.EventType.CREATED,
            path=Path("/tmp/test.json"),
            status=status_file,
        )

        cache.update(event)
        assert cache.worker_count == 1
        assert cache.active_count == 1

        retrieved = cache.get("test-worker-1")
        assert retrieved is not None
        assert retrieved.worker_id == "test-worker-1"

    def test_cache_update_modified(self, sample_status_data):
        """Test cache update on worker modification"""
        cache = WorkerStatusCache()

        # Create initial active worker
        active_data = sample_status_data.copy()
        active_data["status"] = WorkerStatusValue.ACTIVE
        status_file = WorkerStatusFile(**active_data)

        event = StatusFileEvent(
            worker_id="test-worker-1",
            event_type=StatusFileEvent.EventType.CREATED,
            path=Path("/tmp/test.json"),
            status=status_file,
        )
        cache.update(event)
        assert cache.active_count == 1

        # Update to idle
        idle_data = sample_status_data.copy()
        idle_data["status"] = WorkerStatusValue.IDLE
        idle_status = WorkerStatusFile(**idle_data)

        event = StatusFileEvent(
            worker_id="test-worker-1",
            event_type=StatusFileEvent.EventType.MODIFIED,
            path=Path("/tmp/test.json"),
            status=idle_status,
        )
        cache.update(event)

        assert cache.worker_count == 1
        assert cache.active_count == 0
        assert cache.idle_count == 1

    def test_cache_update_deleted(self, sample_status_data):
        """Test cache update on worker deletion"""
        cache = WorkerStatusCache()
        status_file = WorkerStatusFile(**sample_status_data)

        # Add worker
        event = StatusFileEvent(
            worker_id="test-worker-1",
            event_type=StatusFileEvent.EventType.CREATED,
            path=Path("/tmp/test.json"),
            status=status_file,
        )
        cache.update(event)
        assert cache.worker_count == 1

        # Delete worker
        event = StatusFileEvent(
            worker_id="test-worker-1",
            event_type=StatusFileEvent.EventType.DELETED,
            path=Path("/tmp/test.json"),
            status=None,
        )
        cache.update(event)
        assert cache.worker_count == 0

    def test_get_all_workers(self, sample_status_data):
        """Test getting all workers from cache"""
        cache = WorkerStatusCache()

        # Add multiple workers
        for i in range(3):
            data = sample_status_data.copy()
            data["worker_id"] = f"worker-{i}"
            data["status"] = WorkerStatusValue.ACTIVE if i < 2 else WorkerStatusValue.IDLE
            status_file = WorkerStatusFile(**data)

            event = StatusFileEvent(
                worker_id=f"worker-{i}",
                event_type=StatusFileEvent.EventType.CREATED,
                path=Path(f"/tmp/worker-{i}.json"),
                status=status_file,
            )
            cache.update(event)

        all_workers = cache.get_all()
        assert len(all_workers) == 3  # 2 active, 1 idle = 3 total
        assert cache.active_count == 2
        assert cache.idle_count == 1

    def test_get_failed_workers(self, sample_status_data):
        """Test getting failed workers from cache"""
        cache = WorkerStatusCache()

        # Add failed worker
        failed_data = sample_status_data.copy()
        failed_data["status"] = WorkerStatusValue.FAILED
        failed_data["worker_id"] = "failed-worker"  # Update worker_id in data too
        status_file = WorkerStatusFile(**failed_data)

        event = StatusFileEvent(
            worker_id="failed-worker",
            event_type=StatusFileEvent.EventType.CREATED,
            path=Path("/tmp/failed.json"),
            status=status_file,
        )
        cache.update(event)

        assert cache.failed_count == 1
        failed_workers = cache.get_failed_workers()
        assert len(failed_workers) == 1
        assert failed_workers[0].worker_id == "failed-worker"


# =============================================================================
# PollingStatusWatcher Tests
# =============================================================================


class TestPollingStatusWatcher:
    """Tests for PollingStatusWatcher"""

    @pytest.mark.asyncio
    async def test_polling_watcher_detects_new_file(self, temp_status_dir):
        """Test that polling watcher detects new status files"""
        events = []
        callback = lambda e: events.append(e)

        watcher = PollingStatusWatcher(
            status_dir=temp_status_dir,
            callback=callback,
            poll_interval=0.1,
        )

        await watcher.start()
        await asyncio.sleep(0.2)  # Wait for initial scan

        # Create a new status file
        status_file = temp_status_dir / "new-worker.json"
        status_file.write_text('{"worker_id": "new-worker", "status": "active", "model": "sonnet", "workspace": "/tmp"}')

        await asyncio.sleep(0.3)  # Wait for poll to detect change

        await watcher.stop()

        assert len(events) > 0
        created_events = [e for e in events if e.event_type == StatusFileEvent.EventType.CREATED]
        assert len(created_events) > 0
        assert created_events[0].worker_id == "new-worker"

    @pytest.mark.asyncio
    async def test_polling_watcher_detects_modification(self, temp_status_dir):
        """Test that polling watcher detects file modifications"""
        events = []
        callback = lambda e: events.append(e)

        # Create initial file
        status_file = temp_status_dir / "modifying-worker.json"
        status_file.write_text('{"worker_id": "modifying-worker", "status": "idle", "model": "haiku", "workspace": "/tmp"}')

        watcher = PollingStatusWatcher(
            status_dir=temp_status_dir,
            callback=callback,
            poll_interval=0.1,
        )

        await watcher.start()
        await asyncio.sleep(0.2)  # Wait for initial scan

        # Modify the file
        status_file.write_text('{"worker_id": "modifying-worker", "status": "active", "model": "haiku", "workspace": "/tmp"}')

        await asyncio.sleep(0.3)  # Wait for poll to detect change

        await watcher.stop()

        # Should have created event and modified event
        modified_events = [e for e in events if e.event_type == StatusFileEvent.EventType.MODIFIED]
        assert len(modified_events) > 0

    @pytest.mark.asyncio
    async def test_polling_watcher_detects_deletion(self, temp_status_dir):
        """Test that polling watcher detects file deletions"""
        events = []
        callback = lambda e: events.append(e)

        # Create initial file
        status_file = temp_status_dir / "deleting-worker.json"
        status_file.write_text('{"worker_id": "deleting-worker", "status": "active", "model": "sonnet", "workspace": "/tmp"}')

        watcher = PollingStatusWatcher(
            status_dir=temp_status_dir,
            callback=callback,
            poll_interval=0.1,
        )

        await watcher.start()
        await asyncio.sleep(0.2)  # Wait for initial scan

        # Delete the file
        status_file.unlink()

        await asyncio.sleep(0.3)  # Wait for poll to detect change

        await watcher.stop()

        deleted_events = [e for e in events if e.event_type == StatusFileEvent.EventType.DELETED]
        assert len(deleted_events) > 0
        assert deleted_events[0].worker_id == "deleting-worker"


# =============================================================================
# StatusWatcher Tests (Unified)
# =============================================================================


class TestStatusWatcher:
    """Tests for unified StatusWatcher with auto-fallback"""

    @pytest.mark.asyncio
    async def test_watcher_starts_with_polling_fallback(self, temp_status_dir):
        """Test that watcher falls back to polling when inotify unavailable"""
        events = []
        callback = lambda e: events.append(e)

        watcher = StatusWatcher(
            status_dir=temp_status_dir,
            callback=callback,
            poll_interval=0.1,
        )

        # Mock watchdog as unavailable
        with patch('forge.status_watcher.WATCHDOG_AVAILABLE', False):
            watcher_type = await watcher.start()

        assert watcher_type == "polling"
        assert watcher.is_using_polling
        assert watcher.is_running

        await watcher.stop()

    @pytest.mark.asyncio
    async def test_watcher_stops_cleanly(self, temp_status_dir):
        """Test that watcher stops cleanly"""
        callback = lambda e: None

        watcher = StatusWatcher(
            status_dir=temp_status_dir,
            callback=callback,
        )

        # Mock watchdog as unavailable to use polling
        with patch('forge.status_watcher.WATCHDOG_AVAILABLE', False):
            await watcher.start()
            assert watcher.is_running

            await watcher.stop()
            assert not watcher.is_running


# =============================================================================
# WorkerStatusFile Tests
# =============================================================================


class TestWorkerStatusFile:
    """Tests for WorkerStatusFile dataclass"""

    def test_to_dict(self, sample_status_data):
        """Test converting WorkerStatusFile to dict"""
        status = WorkerStatusFile(**sample_status_data)
        result = status.to_dict()

        assert result["worker_id"] == "test-worker-1"
        assert result["status"] == "active"
        assert result["model"] == "sonnet"
        assert result["workspace"] == "/home/coder/test-project"
        assert result["pid"] == 12345

    def test_is_healthy_active(self):
        """Test is_healthy for active worker"""
        status = WorkerStatusFile(
            worker_id="test",
            status=WorkerStatusValue.ACTIVE,
            model="sonnet",
            workspace="/tmp",
        )
        assert status.is_healthy
        assert not status.is_error_state

    def test_is_healthy_idle(self):
        """Test is_healthy for idle worker"""
        status = WorkerStatusFile(
            worker_id="test",
            status=WorkerStatusValue.IDLE,
            model="sonnet",
            workspace="/tmp",
        )
        assert status.is_healthy
        assert not status.is_error_state

    def test_is_error_state_failed(self):
        """Test is_error_state for failed worker"""
        status = WorkerStatusFile(
            worker_id="test",
            status=WorkerStatusValue.FAILED,
            model="sonnet",
            workspace="/tmp",
        )
        assert not status.is_healthy
        assert status.is_error_state

    def test_is_error_state_with_error(self):
        """Test is_error_state when error field is set"""
        status = WorkerStatusFile(
            worker_id="test",
            status=WorkerStatusValue.ACTIVE,
            model="sonnet",
            workspace="/tmp",
            error="Corrupted status file",
        )
        assert not status.is_healthy
        assert status.is_error_state


# =============================================================================
# Convenience Functions Tests
# =============================================================================


class TestConvenienceFunctions:
    """Tests for convenience functions"""

    def test_parse_status_file(self, valid_status_file):
        """Test parse_status_file convenience function"""
        result = parse_status_file(valid_status_file)

        assert result.worker_id == "test-worker-1"
        assert result.status == WorkerStatusValue.ACTIVE
        assert result.model == "sonnet"
