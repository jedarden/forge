"""
FORGE Status File Watcher Module

Implements real-time monitoring of worker status files using inotify
with polling fallback. Handles corrupted status files per ADR 0014.

Reference: docs/INTEGRATION_GUIDE.md#status-files
Error Handling: docs/adr/0014-error-handling-strategy.md
Architecture: docs/adr/0008-real-time-update-architecture.md
"""

from __future__ import annotations

import asyncio
import json
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from pathlib import Path
from typing import Any, Callable

# Try to import watchdog for inotify support
try:
    from watchdog.observers import Observer
    from watchdog.events import FileSystemEventHandler, FileCreatedEvent, FileModifiedEvent, FileDeletedEvent
    WATCHDOG_AVAILABLE = True
except ImportError:
    WATCHDOG_AVAILABLE = False
    # Create stub classes for when watchdog is not available
    class FileSystemEventHandler:  # type: ignore
        """Stub class when watchdog is not available"""
        pass

    class FileCreatedEvent:  # type: ignore
        """Stub class when watchdog is not available"""
        pass

    class FileModifiedEvent:  # type: ignore
        """Stub class when watchdog is not available"""
        pass

    class FileDeletedEvent:  # type: ignore
        """Stub class when watchdog is not available"""
        pass


# =============================================================================
# Status File Data Models
# =============================================================================


class WorkerStatusValue(Enum):
    """Valid worker status values from status files"""
    ACTIVE = "active"
    IDLE = "idle"
    FAILED = "failed"
    STOPPED = "stopped"
    STARTING = "starting"
    SPAWNED = "spawned"


@dataclass
class WorkerStatusFile:
    """
    Parsed worker status file data.

    Attributes:
        worker_id: Worker identifier (from filename or field)
        status: Worker status (active, idle, failed, stopped, etc.)
        model: Model name (e.g., "sonnet", "opus", "haiku")
        workspace: Path to workspace directory
        pid: Process ID (optional)
        started_at: ISO 8601 timestamp when worker started (optional)
        last_activity: ISO 8601 timestamp of last activity (optional)
        current_task: Current task ID or description (optional)
        tasks_completed: Number of tasks completed (optional)
        error: Error message if status file is corrupted (optional)
        raw_data: Raw JSON data from file (for debugging)
    """
    worker_id: str
    status: WorkerStatusValue
    model: str
    workspace: str
    pid: int | None = None
    started_at: str | None = None
    last_activity: str | None = None
    current_task: str | None = None
    tasks_completed: int | None = None
    error: str | None = None
    raw_data: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization"""
        return {
            "worker_id": self.worker_id,
            "status": self.status.value if isinstance(self.status, WorkerStatusValue) else self.status,
            "model": self.model,
            "workspace": self.workspace,
            "pid": self.pid,
            "started_at": self.started_at,
            "last_activity": self.last_activity,
            "current_task": self.current_task,
            "tasks_completed": self.tasks_completed,
            "error": self.error,
        }

    @property
    def is_healthy(self) -> bool:
        """Check if worker is in healthy state"""
        return self.status in (WorkerStatusValue.ACTIVE, WorkerStatusValue.IDLE, WorkerStatusValue.SPAWNED) and self.error is None

    @property
    def is_error_state(self) -> bool:
        """Check if worker is in error state"""
        return self.status == WorkerStatusValue.FAILED or self.error is not None


# =============================================================================
# Status File Parser with Error Handling (ADR 0014)
# =============================================================================


class StatusFileParser:
    """
    Parse worker status files with graceful error handling.

    Implements ADR 0014 error handling strategy:
    - Mark worker as unknown, show error
    - No automatic recovery
    - Clear error messages
    """

    REQUIRED_FIELDS = ["worker_id", "status", "model", "workspace"]
    VALID_STATUSES = {s.value for s in WorkerStatusValue}

    def __init__(self):
        """Initialize the status file parser"""
        self.parse_errors = 0
        self.last_error = None

    def parse(self, status_file: Path) -> WorkerStatusFile:
        """
        Parse a status file with error handling per ADR 0014.

        Args:
            status_file: Path to the status JSON file

        Returns:
            WorkerStatusFile with parsed data or error state
        """
        try:
            with open(status_file) as f:
                data = json.load(f)

            # Validate required fields
            missing = [f for f in self.REQUIRED_FIELDS if f not in data]

            if missing:
                self.parse_errors += 1
                self.last_error = f"Missing fields: {', '.join(missing)}"
                return WorkerStatusFile(
                    worker_id=status_file.stem,
                    status=WorkerStatusValue.FAILED,
                    model="unknown",
                    workspace="unknown",
                    error=f"Corrupted status file (missing: {', '.join(missing)})",
                    raw_data=data,
                )

            # Validate status value
            status_str = data.get("status", "").lower()
            if status_str not in self.VALID_STATUSES:
                self.parse_errors += 1
                self.last_error = f"Invalid status: {status_str}"
                return WorkerStatusFile(
                    worker_id=data["worker_id"],
                    status=WorkerStatusValue.FAILED,
                    model=data.get("model", "unknown"),
                    workspace=data.get("workspace", "unknown"),
                    error=f"Invalid status value: {status_str}",
                    raw_data=data,
                )

            # Parse status to enum
            try:
                status = WorkerStatusValue(status_str)
            except ValueError:
                status = WorkerStatusValue.FAILED

            return WorkerStatusFile(
                worker_id=data["worker_id"],
                status=status,
                model=data.get("model", "unknown"),
                workspace=data.get("workspace", "unknown"),
                pid=data.get("pid"),
                started_at=data.get("started_at"),
                last_activity=data.get("last_activity"),
                current_task=data.get("current_task"),
                tasks_completed=data.get("tasks_completed"),
                raw_data=data,
            )

        except json.JSONDecodeError as e:
            self.parse_errors += 1
            self.last_error = f"Invalid JSON: {str(e)[:50]}"
            return WorkerStatusFile(
                worker_id=status_file.stem,
                status=WorkerStatusValue.FAILED,
                model="unknown",
                workspace="unknown",
                error=f"Corrupted status file (invalid JSON: {str(e)[:50]})",
            )

        except FileNotFoundError:
            # Status file deleted - worker stopped
            return WorkerStatusFile(
                worker_id=status_file.stem,
                status=WorkerStatusValue.STOPPED,
                model="unknown",
                workspace="unknown",
            )

        except Exception as e:
            self.parse_errors += 1
            self.last_error = f"Read error: {str(e)[:50]}"
            return WorkerStatusFile(
                worker_id=status_file.stem,
                status=WorkerStatusValue.FAILED,
                model="unknown",
                workspace="unknown",
                error=f"Failed to read status: {str(e)[:50]}",
            )


# =============================================================================
# File Change Event
# =============================================================================


@dataclass
class StatusFileEvent:
    """
    Event representing a status file change.

    Attributes:
        worker_id: Worker identifier (from filename)
        event_type: Type of change (created, modified, deleted)
        path: Path to the status file
        status: Parsed worker status (None for deleted files)
        timestamp: When the event was detected
    """
    class EventType(Enum):
        CREATED = "created"
        MODIFIED = "modified"
        DELETED = "deleted"

    worker_id: str
    event_type: EventType
    path: Path
    status: WorkerStatusFile | None
    timestamp: datetime = field(default_factory=datetime.now)


# =============================================================================
# Inotify-based Watcher (Primary)
# =============================================================================


class _InotifyEventHandler(FileSystemEventHandler):
    """
    Internal watchdog event handler for status file changes.

    Converts watchdog events to StatusFileEvent objects.
    """

    def __init__(
        self,
        callback: Callable[[StatusFileEvent], None],
        parser: StatusFileParser,
    ):
        """
        Initialize the event handler.

        Args:
            callback: Function to call for each status file event
            parser: Status file parser
        """
        self.callback = callback
        self.parser = parser

    def on_created(self, event) -> None:
        """Handle file creation event"""
        if event.is_directory:
            return

            path = Path(event.src_path)
            if not path.suffix == ".json":
                return

        status = self.parser.parse(path)
        self.callback(StatusFileEvent(
            worker_id=status.worker_id,
            event_type=StatusFileEvent.EventType.CREATED,
            path=path,
            status=status,
        ))

    def on_modified(self, event) -> None:
        """Handle file modification event"""
        if event.is_directory:
            return

        path = Path(event.src_path)
        if not path.suffix == ".json":
            return

        status = self.parser.parse(path)
        self.callback(StatusFileEvent(
            worker_id=status.worker_id,
            event_type=StatusFileEvent.EventType.MODIFIED,
            path=path,
            status=status,
        ))

    def on_deleted(self, event) -> None:
        """Handle file deletion event"""
        if event.is_directory:
            return

        path = Path(event.src_path)
        if not path.suffix == ".json":
            return

        worker_id = path.stem
        self.callback(StatusFileEvent(
            worker_id=worker_id,
            event_type=StatusFileEvent.EventType.DELETED,
            path=path,
            status=None,  # Deleted files have no status
        ))


class InotifyStatusWatcher:
    """
    Watch status files using inotify (via watchdog).

    Falls back to polling if watchdog is not available or inotify fails.
    """

    def __init__(
        self,
        status_dir: Path | str,
        callback: Callable[[StatusFileEvent], None],
        parser: StatusFileParser | None = None,
    ):
        """
        Initialize the inotify watcher.

        Args:
            status_dir: Directory containing status files
            callback: Function to call for each status file event
            parser: Status file parser (creates default if None)
        """
        self.status_dir = Path(status_dir).expanduser()
        self.callback = callback
        self.parser = parser if parser is not None else StatusFileParser()

        self._observer: Any | None = None
        self._running = False
        self._available = WATCHDOG_AVAILABLE

    @property
    def is_available(self) -> bool:
        """Check if inotify watching is available"""
        return self._available

    async def start(self) -> bool:
        """
        Start watching status files.

        Returns:
            True if started successfully, False if unavailable
        """
        if not self._available:
            return False

        if self._running:
            return True

        try:
            # Ensure status directory exists
            self.status_dir.mkdir(parents=True, exist_ok=True)

            # Create and start observer
            self._observer = Observer()
            handler = _InotifyEventHandler(self.callback, self.parser)
            self._observer.schedule(handler, str(self.status_dir), recursive=False)
            self._observer.start()
            self._running = True

            return True

        except OSError as e:
            # Check for inotify limit error
            if 'inotify' in str(e).lower() or 'no space left' in str(e).lower() or 'user limit' in str(e).lower():
                # Inotify limit reached - fall back to polling
                self._available = False
                import sys
                print(f"[FORGE] inotify limit reached, falling back to polling: {e}", file=sys.stderr)
                return False
            raise

        except Exception as e:
            # Fall back to polling for other errors
            self._available = False
            import sys
            print(f"[FORGE] Inotify watcher failed, falling back to polling: {e}", file=sys.stderr)
            return False

    async def stop(self) -> None:
        """Stop watching status files"""
        if not self._running:
            return

        if self._observer:
            self._observer.stop()
            self._observer.join(timeout=5.0)
            self._observer = None

        self._running = False

    @property
    def is_running(self) -> bool:
        """Check if watcher is running"""
        return self._running


# =============================================================================
# Polling-based Watcher (Fallback)
# =============================================================================


class PollingStatusWatcher:
    """
    Watch status files using periodic polling.

    Fallback mechanism when inotify is not available or fails.
    Uses 5-second poll interval per ADR 0008.
    """

    def __init__(
        self,
        status_dir: Path | str,
        callback: Callable[[StatusFileEvent], None],
        parser: StatusFileParser | None = None,
        poll_interval: float = 5.0,
    ):
        """
        Initialize the polling watcher.

        Args:
            status_dir: Directory containing status files
            callback: Function to call for each status file event
            parser: Status file parser (creates default if None)
            poll_interval: Seconds between polls (default 5.0 per ADR 0008)
        """
        self.status_dir = Path(status_dir).expanduser()
        self.callback = callback
        self.parser = parser if parser is not None else StatusFileParser()
        self.poll_interval = poll_interval

        self._running = False
        self._task: asyncio.Task[None] | None = None
        self._known_files: dict[str, float] = {}  # worker_id -> mtime
        self._known_hashes: dict[str, int] = {}  # worker_id -> hash (for quick change detection)

    async def start(self) -> bool:
        """
        Start polling status files.

        Returns:
            True if started successfully
        """
        if self._running:
            return True

        # Ensure status directory exists
        self.status_dir.mkdir(parents=True, exist_ok=True)

        # Initial scan
        await self._scan_directory()

        # Start polling loop
        self._running = True
        self._task = asyncio.create_task(self._poll_loop())

        return True

    async def stop(self) -> None:
        """Stop polling status files"""
        self._running = False

        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
            self._task = None

    async def _poll_loop(self) -> None:
        """Main polling loop"""
        while self._running:
            try:
                await self._scan_directory()
                await asyncio.sleep(self.poll_interval)
            except asyncio.CancelledError:
                break
            except Exception as e:
                import sys
                print(f"[FORGE] Polling watcher error: {e}", file=sys.stderr)
                await asyncio.sleep(self.poll_interval)

    async def _scan_directory(self) -> None:
        """Scan directory for changes"""
        if not self.status_dir.exists():
            return

        # Get current status files
        current_files = set(self.status_dir.glob("*.json"))
        current_worker_ids = {f.stem for f in current_files}

        # Detect deletions
        deleted_worker_ids = self._known_files.keys() - current_worker_ids
        for worker_id in deleted_worker_ids:
            del self._known_files[worker_id]
            if worker_id in self._known_hashes:
                del self._known_hashes[worker_id]

            self.callback(StatusFileEvent(
                worker_id=worker_id,
                event_type=StatusFileEvent.EventType.DELETED,
                path=self.status_dir / f"{worker_id}.json",
                status=None,
            ))

        # Detect creations and modifications
        for status_file in current_files:
            worker_id = status_file.stem
            current_mtime = status_file.stat().st_mtime
            current_size = status_file.stat().st_size

            # Create simple hash from mtime + size for change detection
            current_hash = hash((current_mtime, current_size))

            if worker_id not in self._known_files:
                # New file
                self._known_files[worker_id] = current_mtime
                self._known_hashes[worker_id] = current_hash

                status = self.parser.parse(status_file)
                self.callback(StatusFileEvent(
                    worker_id=worker_id,
                    event_type=StatusFileEvent.EventType.CREATED,
                    path=status_file,
                    status=status,
                ))

            elif self._known_hashes[worker_id] != current_hash:
                # Modified file
                self._known_files[worker_id] = current_mtime
                self._known_hashes[worker_id] = current_hash

                status = self.parser.parse(status_file)
                self.callback(StatusFileEvent(
                    worker_id=worker_id,
                    event_type=StatusFileEvent.EventType.MODIFIED,
                    path=status_file,
                    status=status,
                ))

    @property
    def is_running(self) -> bool:
        """Check if watcher is running"""
        return self._running


# =============================================================================
# Unified Status Watcher (Auto-fallback)
# =============================================================================


class StatusWatcher:
    """
    Unified status file watcher with automatic fallback.

    Tries inotify first, falls back to polling if unavailable.
    Uses 5-second polling interval per ADR 0008.
    """

    def __init__(
        self,
        status_dir: Path | str,
        callback: Callable[[StatusFileEvent], None],
        parser: StatusFileParser | None = None,
        poll_interval: float = 5.0,
    ):
        """
        Initialize the status watcher.

        Args:
            status_dir: Directory containing status files (~/.forge/status/)
            callback: Function to call for each status file event
            parser: Status file parser (creates default if None)
            poll_interval: Seconds between polls for fallback (default 5.0 per ADR 0008)
        """
        self.status_dir = Path(status_dir).expanduser()
        self.callback = callback
        self.parser = parser if parser is not None else StatusFileParser()
        self.poll_interval = poll_interval

        self._inotify_watcher = InotifyStatusWatcher(
            status_dir, callback, parser
        )
        self._polling_watcher = PollingStatusWatcher(
            status_dir, callback, parser, poll_interval
        )
        self._active_watcher: InotifyStatusWatcher | PollingStatusWatcher | None = None
        self._watcher_type: str = "none"

    async def start(self) -> str:
        """
        Start watching status files.

        Returns:
            Watcher type being used ("inotify" or "polling")
        """
        # Try inotify first
        if await self._inotify_watcher.start():
            self._active_watcher = self._inotify_watcher
            self._watcher_type = "inotify"
            return "inotify"

        # Fall back to polling
        await self._polling_watcher.start()
        self._active_watcher = self._polling_watcher
        self._watcher_type = "polling"
        return "polling"

    async def stop(self) -> None:
        """Stop watching status files"""
        if self._inotify_watcher.is_running:
            await self._inotify_watcher.stop()
        if self._polling_watcher.is_running:
            await self._polling_watcher.stop()

        self._active_watcher = None
        self._watcher_type = "none"

    @property
    def watcher_type(self) -> str:
        """Get the active watcher type"""
        return self._watcher_type

    @property
    def is_running(self) -> bool:
        """Check if watcher is running"""
        return (
            self._inotify_watcher.is_running or
            self._polling_watcher.is_running
        )

    @property
    def is_using_polling(self) -> bool:
        """Check if using polling fallback"""
        return self._watcher_type == "polling"


# =============================================================================
# Health-Aware Status Watcher (Integrated Health Monitoring)
# =============================================================================


class HealthAwareStatusWatcher(StatusWatcher):
    """
    Status watcher with integrated health monitoring.

    Extends StatusWatcher to run periodic health checks every 10 seconds.
    Marks workers as failed when health checks fail per ADR 0014.

    Health checks performed:
    - PID existence (process liveness)
    - Log activity (recent activity within 5 minutes)
    - Status file validation
    - Tmux session aliveness (if applicable)
    """

    def __init__(
        self,
        status_dir: Path | str,
        callback: Callable[[StatusFileEvent], None],
        parser: StatusFileParser | None = None,
        poll_interval: float = 5.0,
        log_dir: Path | str = "~/.forge/logs",
        health_check_interval: int = 10,
        on_worker_unhealthy: Callable[[str, Any], None] | None = None,
    ):
        """
        Initialize the health-aware status watcher.

        Args:
            status_dir: Directory containing status files
            callback: Function to call for each status file event
            parser: Status file parser (creates default if None)
            poll_interval: Seconds between polls for fallback (default 5.0 per ADR 0008)
            log_dir: Directory containing log files (for health checks)
            health_check_interval: Seconds between health checks (default 10)
            on_worker_unhealthy: Callback(worker_id, health_status) when worker is unhealthy
        """
        super().__init__(status_dir, callback, parser, poll_interval)

        self.log_dir = Path(log_dir).expanduser()
        self.health_check_interval = health_check_interval
        self.on_worker_unhealthy = on_worker_unhealthy

        # Health monitoring loop (created on start)
        self._health_monitoring_loop: Any | None = None

    async def start(self) -> str:
        """
        Start watching status files with health monitoring.

        Returns:
            Watcher type being used ("inotify" or "polling")
        """
        # Start base status watcher
        watcher_type = await super().start()

        # Import health monitor here to avoid circular imports
        from forge.health_monitor import (
            WorkerHealthMonitor,
            HealthMonitoringLoop,
        )

        # Create health monitor
        health_monitor = WorkerHealthMonitor(
            status_dir=self.status_dir,
            log_dir=self.log_dir,
        )

        # Create and start health monitoring loop
        self._health_monitoring_loop = HealthMonitoringLoop(
            health_monitor=health_monitor,
            status_dir=self.status_dir,
            on_worker_unhealthy=self._on_worker_unhealthy,
        )

        await self._health_monitoring_loop.start(self.health_check_interval)

        return watcher_type

    async def stop(self) -> None:
        """Stop watching status files and health monitoring"""
        # Stop health monitoring loop first
        if self._health_monitoring_loop and self._health_monitoring_loop.is_running:
            await self._health_monitoring_loop.stop()
            self._health_monitoring_loop = None

        # Stop base status watcher
        await super().stop()

    def _on_worker_unhealthy(self, worker_id: str, health_status: Any) -> None:
        """
        Internal callback when worker is marked as unhealthy.

        Triggers a status file event to update the UI.

        Args:
            worker_id: Worker identifier
            health_status: WorkerHealthStatus object
        """
        # Re-parse the updated status file
        status_file = self.status_dir / f"{worker_id}.json"
        status = self.parser.parse(status_file)

        # Create status file event to trigger UI update
        event = StatusFileEvent(
            worker_id=worker_id,
            event_type=StatusFileEvent.EventType.MODIFIED,
            path=status_file,
            status=status,
        )

        # Update cache via callback
        self.callback(event)

        # Notify user callback if provided
        if self.on_worker_unhealthy:
            self.on_worker_unhealthy(worker_id, health_status)


# =============================================================================
# Worker Status Cache
# =============================================================================


class WorkerStatusCache:
    """
    Cache of worker statuses updated by status file events.

    Provides quick lookup of current worker status for the UI.
    """

    def __init__(self):
        """Initialize the status cache"""
        self._workers: dict[str, WorkerStatusFile] = {}
        self._last_updated: dict[str, datetime] = {}

    def update(self, event: StatusFileEvent) -> None:
        """
        Update cache based on status file event.

        Args:
            event: Status file event
        """
        if event.event_type == StatusFileEvent.EventType.DELETED:
            # Remove worker from cache
            if event.worker_id in self._workers:
                del self._workers[event.worker_id]
                del self._last_updated[event.worker_id]
        elif event.status is not None:
            # Update worker status
            self._workers[event.worker_id] = event.status
            self._last_updated[event.worker_id] = event.timestamp

    def get(self, worker_id: str) -> WorkerStatusFile | None:
        """
        Get worker status from cache.

        Args:
            worker_id: Worker identifier

        Returns:
            WorkerStatusFile if found, None otherwise
        """
        return self._workers.get(worker_id)

    def get_all(self) -> dict[str, WorkerStatusFile]:
        """Get all cached worker statuses"""
        return self._workers.copy()

    def get_active_workers(self) -> list[WorkerStatusFile]:
        """Get list of active workers"""
        return [
            w for w in self._workers.values()
            if w.status == WorkerStatusValue.ACTIVE
        ]

    def get_idle_workers(self) -> list[WorkerStatusFile]:
        """Get list of idle workers"""
        return [
            w for w in self._workers.values()
            if w.status == WorkerStatusValue.IDLE
        ]

    def get_failed_workers(self) -> list[WorkerStatusFile]:
        """Get list of failed workers"""
        return [
            w for w in self._workers.values()
            if w.status == WorkerStatusValue.FAILED or w.error is not None
        ]

    @property
    def worker_count(self) -> int:
        """Get total number of workers"""
        return len(self._workers)

    @property
    def active_count(self) -> int:
        """Get number of active workers"""
        return len(self.get_active_workers())

    @property
    def idle_count(self) -> int:
        """Get number of idle workers"""
        return len(self.get_idle_workers())

    @property
    def failed_count(self) -> int:
        """Get number of failed workers"""
        return len(self.get_failed_workers())


# =============================================================================
# Convenience Functions
# =============================================================================


def parse_status_file(status_file: Path | str) -> WorkerStatusFile:
    """
    Convenience function to parse a single status file.

    Args:
        status_file: Path to status JSON file

    Returns:
        WorkerStatusFile with parsed data
    """
    parser = StatusFileParser()
    return parser.parse(Path(status_file))


async def watch_status_files(
    status_dir: Path | str,
    callback: Callable[[StatusFileEvent], None],
    poll_interval: float = 5.0,
) -> StatusWatcher:
    """
    Convenience function to start watching status files.

    Args:
        status_dir: Directory containing status files
        callback: Function to call for each event
        poll_interval: Polling interval for fallback (default 5.0 per ADR 0008)

    Returns:
        StatusWatcher instance
    """
    watcher = StatusWatcher(
        status_dir=status_dir,
        callback=callback,
        poll_interval=poll_interval,
    )
    await watcher.start()
    return watcher
