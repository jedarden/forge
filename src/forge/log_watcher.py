"""
FORGE Log File Watcher Module

Implements real-time monitoring of worker log files using inotify
with polling fallback. Handles log rotation and graceful error handling.

Reference: docs/adr/0008-real-time-update-architecture.md
Error Handling: docs/adr/0014-error-handling-strategy.md
"""

from __future__ import annotations

import asyncio
import json
from collections import deque
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from pathlib import Path
from typing import Any, Callable

# Try to import watchdog for inotify support
try:
    from watchdog.observers import Observer
    from watchdog.events import FileSystemEventHandler
    WATCHDOG_AVAILABLE = True
except ImportError:
    WATCHDOG_AVAILABLE = False
    # Create stub class for when watchdog is not available
    class FileSystemEventHandler:  # type: ignore
        """Stub class when watchdog is not available"""
        pass


# =============================================================================
# Log Entry Data Models
# =============================================================================


class LogLevel(Enum):
    """Log level enumeration"""
    DEBUG = "debug"
    INFO = "info"
    WARNING = "warning"
    ERROR = "error"
    CRITICAL = "critical"


@dataclass
class LogEntry:
    """
    A parsed log entry from worker logs.

    Attributes:
        raw: The original raw log line
        timestamp: ISO 8601 timestamp (or None if parsing failed)
        level: Log level (info, warning, error, debug, critical)
        worker_id: Worker identifier (or None if not present)
        message: Human-readable message (or None)
        event: Event type (e.g., task_started, worker_stopped)
        extra: Additional fields from the log entry
        parse_error: Error message if parsing failed
        line_number: Line number in the source file
    """
    raw: str
    timestamp: str | None = None
    level: str = "info"
    worker_id: str | None = None
    message: str | None = None
    event: str | None = None
    extra: dict[str, Any] = field(default_factory=dict)
    parse_error: str | None = None
    line_number: int = 0

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization"""
        return {
            "raw": self.raw,
            "timestamp": self.timestamp,
            "level": self.level,
            "worker_id": self.worker_id,
            "message": self.message,
            "event": self.event,
            "extra": self.extra,
            "parse_error": self.parse_error,
            "line_number": self.line_number,
        }

    @property
    def is_valid(self) -> bool:
        """Check if the entry was parsed successfully"""
        return self.parse_error is None

    @property
    def is_error(self) -> bool:
        """Check if this is an error-level log entry"""
        return self.level.lower() in ("error", "critical")


# =============================================================================
# Log Parser with Auto-Format Detection
# =============================================================================


class LogFormat(Enum):
    """Log format types supported by FORGE"""
    JSONL = "jsonl"          # JSON Lines format
    KEYVALUE = "keyvalue"    # Key-value format
    AUTO = "auto"            # Auto-detect format


class LogParser:
    """
    Universal log parser with automatic format detection.

    Supports JSON Lines (JSONL) and key-value formats.
    """

    STANDARD_FIELDS = {"timestamp", "level", "worker_id", "message", "event"}

    def __init__(self, format: LogFormat = LogFormat.AUTO):
        """
        Initialize the log parser.

        Args:
            format: Log format to use (AUTO detects automatically)
        """
        self.format = format
        self._detected_format: LogFormat | None = None

    def detect_format(self, line: str) -> LogFormat:
        """
        Detect log format from a line.

        Args:
            line: Sample log line

        Returns:
            Detected LogFormat
        """
        line = line.strip()

        if not line:
            return LogFormat.JSONL  # Default

        # Try JSON first
        if line.startswith('{'):
            try:
                json.loads(line)
                return LogFormat.JSONL
            except json.JSONDecodeError:
                pass

        # Check for key=value pattern
        if '=' in line and ' ' in line:
            parts = line.split()
            kv_count = sum(1 for p in parts if '=' in p)
            if kv_count >= 2:  # At least 2 key=value pairs
                return LogFormat.KEYVALUE

        return LogFormat.JSONL  # Default to JSONL

    def parse_line(self, line: str, line_number: int = 0) -> LogEntry:
        """
        Parse a single log line with auto-detection.

        Args:
            line: Raw log line
            line_number: Line number for error reporting

        Returns:
            LogEntry with extracted fields
        """
        line = line.strip()

        if not line:
            return LogEntry(
                raw=line,
                line_number=line_number,
            )

        # Determine format to use
        format_to_use = self.format

        if format_to_use == LogFormat.AUTO:
            format_to_use = self.detect_format(line)

            # Lock in detected format after first successful parse
            if format_to_use != LogFormat.JSONL and self._detected_format is None:
                self._detected_format = format_to_use

            # Use previously detected format if available
            if self._detected_format is not None:
                format_to_use = self._detected_format

        # Parse based on format
        if format_to_use == LogFormat.JSONL:
            entry = self._parse_jsonl(line, line_number)
        elif format_to_use == LogFormat.KEYVALUE:
            entry = self._parse_keyvalue(line, line_number)
        else:
            # Unknown format - try JSONL as fallback
            entry = self._parse_jsonl(line, line_number)

        return entry

    def _parse_jsonl(self, line: str, line_number: int) -> LogEntry:
        """Parse JSONL format line"""
        try:
            data = json.loads(line)

            # Extract standard fields
            timestamp = data.get("timestamp")
            level = self._normalize_level(data.get("level", "info"))
            worker_id = data.get("worker_id")
            message = data.get("message")
            event = data.get("event")

            # Extract extra fields (non-standard fields)
            extra = {
                k: v for k, v in data.items()
                if k not in self.STANDARD_FIELDS
            }

            return LogEntry(
                raw=line,
                timestamp=timestamp,
                level=level,
                worker_id=worker_id,
                message=message,
                event=event,
                extra=extra,
                line_number=line_number,
            )

        except json.JSONDecodeError as e:
            return LogEntry(
                raw=line,
                line_number=line_number,
                parse_error=f"Invalid JSON: {str(e)[:100]}"
            )
        except Exception as e:
            return LogEntry(
                raw=line,
                line_number=line_number,
                parse_error=f"Parse error: {str(e)[:100]}"
            )

    def _parse_keyvalue(self, line: str, line_number: int) -> LogEntry:
        """Parse key-value format line"""
        try:
            # Split into parts
            parts = line.split()

            # Extract timestamp (first non-key=value part)
            timestamp = None
            idx = 0

            if parts and '=' not in parts[0]:
                timestamp = parts[0]
                idx = 1

            # Parse key=value pairs
            data: dict[str, str] = {}
            i = 0

            while i < len(parts[idx:]):
                part = parts[idx:][i]

                if '=' in part:
                    key, value = part.split('=', 1)

                    # Handle quoted values
                    if value.startswith('"') and not value.endswith('"'):
                        # Need to find the closing quote across parts
                        full_value = value[1:]  # Remove opening quote
                        j = i + 1
                        while j < len(parts[idx:]):
                            full_value += ' ' + parts[idx:][j]
                            if parts[idx:][j].endswith('"'):
                                full_value = full_value[:-1]  # Remove closing quote
                                break
                            j += 1
                        i = j  # Skip consumed parts
                        value = full_value
                    else:
                        # Strip quotes if present
                        value = value.strip('"')

                    data[key] = value

                i += 1

            # Extract standard fields
            level = self._normalize_level(data.get("level", "info"))
            worker_id = data.get("worker_id")
            message = data.get("message")
            event = data.get("event")

            # Extract extra fields
            extra = {
                k: v for k, v in data.items()
                if k not in {"level", "worker_id", "message", "event"}
            }

            return LogEntry(
                raw=line,
                timestamp=timestamp,
                level=level,
                worker_id=worker_id,
                message=message,
                event=event,
                extra=extra,
                line_number=line_number,
            )

        except Exception as e:
            return LogEntry(
                raw=line,
                line_number=line_number,
                parse_error=f"Parse error: {str(e)[:100]}"
            )

    def _normalize_level(self, level: str | None) -> str:
        """Normalize log level to lowercase"""
        if not level:
            return "info"
        level_map = {
            "warn": "warning",
            "err": "error",
            "crit": "critical",
            "fatal": "critical",
        }
        return level_map.get(level.lower(), level.lower())


# =============================================================================
# File Change Event
# =============================================================================


@dataclass
class LogFileEvent:
    """
    Event representing a log file change.

    Attributes:
        worker_id: Worker identifier (from filename)
        event_type: Type of change (created, modified, deleted)
        path: Path to the log file
        entries: New log entries since last event (None for deleted files)
        timestamp: When the event was detected
    """
    class EventType(Enum):
        CREATED = "created"
        MODIFIED = "modified"
        DELETED = "deleted"

    worker_id: str
    event_type: EventType
    path: Path
    entries: list[LogEntry] | None
    timestamp: datetime = field(default_factory=datetime.now)


# =============================================================================
# Inotify-based Log Watcher (Primary)
# =============================================================================


class _InotifyLogEventHandler(FileSystemEventHandler):
    """
    Internal watchdog event handler for log file changes.

    Converts watchdog events to LogFileEvent objects.
    """

    def __init__(
        self,
        callback: Callable[[LogFileEvent], None],
        parser: LogParser,
        log_dir: Path,
    ):
        """
        Initialize the event handler.

        Args:
            callback: Function to call for each log file event
            parser: Log parser
            log_dir: Directory containing log files
        """
        self.callback = callback
        self.parser = parser
        self.log_dir = log_dir

        # Track file positions to read only new entries
        self._file_positions: dict[Path, int] = {}

    def on_created(self, event) -> None:
        """Handle file creation event"""
        if event.is_directory:
            return

        path = Path(event.src_path)
        if not path.suffix == ".log":
            return

        worker_id = path.stem
        entries = self._read_new_entries(path, is_new_file=True)

        self.callback(LogFileEvent(
            worker_id=worker_id,
            event_type=LogFileEvent.EventType.CREATED,
            path=path,
            entries=entries,
        ))

    def on_modified(self, event) -> None:
        """Handle file modification event"""
        if event.is_directory:
            return

        path = Path(event.src_path)
        if not path.suffix == ".log":
            return

        worker_id = path.stem
        entries = self._read_new_entries(path, is_new_file=False)

        if entries:  # Only trigger if there are new entries
            self.callback(LogFileEvent(
                worker_id=worker_id,
                event_type=LogFileEvent.EventType.MODIFIED,
                path=path,
                entries=entries,
            ))

    def on_deleted(self, event) -> None:
        """Handle file deletion event"""
        if event.is_directory:
            return

        path = Path(event.src_path)
        if not path.suffix == ".log":
            return

        worker_id = path.stem

        # Remove from position tracking
        if path in self._file_positions:
            del self._file_positions[path]

        self.callback(LogFileEvent(
            worker_id=worker_id,
            event_type=LogFileEvent.EventType.DELETED,
            path=path,
            entries=None,
        ))

    def _read_new_entries(self, path: Path, is_new_file: bool) -> list[LogEntry]:
        """
        Read new log entries from file.

        Args:
            path: Path to log file
            is_new_file: Whether this is a new file (read from start)

        Returns:
            List of new LogEntry objects
        """
        entries = []

        try:
            # Get current position
            if is_new_file:
                start_pos = 0
            else:
                start_pos = self._file_positions.get(path, 0)

            with open(path, 'r', encoding='utf-8', errors='replace') as f:
                # Seek to last position
                f.seek(start_pos)

                # Read new lines
                line_number = 0
                for line in f:
                    line_number += 1
                    entry = self.parser.parse_line(line, line_number)
                    if entry.is_valid or entry.parse_error:  # Include all entries
                        entries.append(entry)

                # Update position
                self._file_positions[path] = f.tell()

        except FileNotFoundError:
            # File was deleted
            pass
        except Exception as e:
            # Log error but return empty list
            import sys
            print(f"[FORGE] Error reading log file {path}: {e}", file=sys.stderr)

        return entries


class InotifyLogWatcher:
    """
    Watch log files using inotify (via watchdog).

    Falls back to polling if watchdog is not available or inotify fails.
    """

    def __init__(
        self,
        log_dir: Path | str,
        callback: Callable[[LogFileEvent], None],
        parser: LogParser | None = None,
    ):
        """
        Initialize the inotify watcher.

        Args:
            log_dir: Directory containing log files
            callback: Function to call for each log file event
            parser: Log parser (creates default if None)
        """
        self.log_dir = Path(log_dir).expanduser()
        self.callback = callback
        self.parser = parser if parser is not None else LogParser(format=LogFormat.AUTO)

        self._observer: Any | None = None
        self._running = False
        self._available = WATCHDOG_AVAILABLE

    @property
    def is_available(self) -> bool:
        """Check if inotify watching is available"""
        return self._available

    async def start(self) -> bool:
        """
        Start watching log files.

        Returns:
            True if started successfully, False if unavailable
        """
        if not self._available:
            return False

        if self._running:
            return True

        try:
            # Ensure log directory exists
            self.log_dir.mkdir(parents=True, exist_ok=True)

            # Create and start observer
            self._observer = Observer()
            handler = _InotifyLogEventHandler(self.callback, self.parser, self.log_dir)
            self._observer.schedule(handler, str(self.log_dir), recursive=False)
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
        """Stop watching log files"""
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
# Polling-based Log Watcher (Fallback)
# =============================================================================


class PollingLogWatcher:
    """
    Watch log files using periodic polling.

    Fallback mechanism when inotify is not available or fails.
    Uses 5-second poll interval per ADR 0008.
    """

    def __init__(
        self,
        log_dir: Path | str,
        callback: Callable[[LogFileEvent], None],
        parser: LogParser | None = None,
        poll_interval: float = 5.0,
    ):
        """
        Initialize the polling watcher.

        Args:
            log_dir: Directory containing log files
            callback: Function to call for each log file event
            parser: Log parser (creates default if None)
            poll_interval: Seconds between polls (default 5.0 per ADR 0008)
        """
        self.log_dir = Path(log_dir).expanduser()
        self.callback = callback
        self.parser = parser if parser is not None else LogParser(format=LogFormat.AUTO)
        self.poll_interval = poll_interval

        self._running = False
        self._task: asyncio.Task[None] | None = None

        # Track file state for change detection
        self._known_files: dict[str, dict] = {}  # worker_id -> {mtime, size, position}

    async def start(self) -> bool:
        """
        Start polling log files.

        Returns:
            True if started successfully
        """
        if self._running:
            return True

        # Ensure log directory exists
        self.log_dir.mkdir(parents=True, exist_ok=True)

        # Initial scan
        await self._scan_directory()

        # Start polling loop
        self._running = True
        self._task = asyncio.create_task(self._poll_loop())

        return True

    async def stop(self) -> None:
        """Stop polling log files"""
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
                print(f"[FORGE] Polling log watcher error: {e}", file=sys.stderr)
                await asyncio.sleep(self.poll_interval)

    async def _scan_directory(self) -> None:
        """Scan directory for changes"""
        if not self.log_dir.exists():
            return

        # Get current log files
        current_files = set(self.log_dir.glob("*.log"))
        current_worker_ids = {f.stem for f in current_files}

        # Detect deletions
        deleted_worker_ids = set(self._known_files.keys()) - current_worker_ids
        for worker_id in deleted_worker_ids:
            del self._known_files[worker_id]

            self.callback(LogFileEvent(
                worker_id=worker_id,
                event_type=LogFileEvent.EventType.DELETED,
                path=self.log_dir / f"{worker_id}.log",
                entries=None,
            ))

        # Detect creations and modifications
        for log_file in current_files:
            worker_id = log_file.stem
            current_mtime = log_file.stat().st_mtime
            current_size = log_file.stat().st_size

            file_info = self._known_files.get(worker_id)

            if file_info is None:
                # New file
                entries = self._read_file(log_file, start_pos=0)
                self._known_files[worker_id] = {
                    "mtime": current_mtime,
                    "size": current_size,
                    "position": current_size,
                }

                self.callback(LogFileEvent(
                    worker_id=worker_id,
                    event_type=LogFileEvent.EventType.CREATED,
                    path=log_file,
                    entries=entries,
                ))

            elif file_info["mtime"] != current_mtime or file_info["size"] != current_size:
                # Modified file - read new entries
                start_pos = file_info.get("position", 0)
                entries = self._read_file(log_file, start_pos=start_pos)

                self._known_files[worker_id] = {
                    "mtime": current_mtime,
                    "size": current_size,
                    "position": current_size,
                }

                if entries:  # Only trigger if there are new entries
                    self.callback(LogFileEvent(
                        worker_id=worker_id,
                        event_type=LogFileEvent.EventType.MODIFIED,
                        path=log_file,
                        entries=entries,
                    ))

    def _read_file(self, path: Path, start_pos: int) -> list[LogEntry]:
        """
        Read log entries from file starting at position.

        Args:
            path: Path to log file
            start_pos: Position to start reading from

        Returns:
            List of LogEntry objects
        """
        entries = []

        try:
            with open(path, 'r', encoding='utf-8', errors='replace') as f:
                # Seek to start position
                f.seek(start_pos)

                # Read lines
                line_number = 0
                for line in f:
                    line_number += 1
                    entry = self.parser.parse_line(line, line_number)
                    if entry.is_valid or entry.parse_error:
                        entries.append(entry)

        except FileNotFoundError:
            # File was deleted
            pass
        except Exception as e:
            import sys
            print(f"[FORGE] Error reading log file {path}: {e}", file=sys.stderr)

        return entries

    @property
    def is_running(self) -> bool:
        """Check if watcher is running"""
        return self._running


# =============================================================================
# Unified Log Watcher (Auto-fallback)
# =============================================================================


class LogWatcher:
    """
    Unified log file watcher with automatic fallback.

    Tries inotify first, falls back to polling if unavailable.
    Uses 5-second polling interval per ADR 0008.
    """

    def __init__(
        self,
        log_dir: Path | str,
        callback: Callable[[LogFileEvent], None],
        parser: LogParser | None = None,
        poll_interval: float = 5.0,
    ):
        """
        Initialize the log watcher.

        Args:
            log_dir: Directory containing log files (~/.forge/logs/)
            callback: Function to call for each log file event
            parser: Log parser (creates default if None)
            poll_interval: Seconds between polls for fallback (default 5.0 per ADR 0008)
        """
        self.log_dir = Path(log_dir).expanduser()
        self.callback = callback
        self.parser = parser if parser is not None else LogParser(format=LogFormat.AUTO)
        self.poll_interval = poll_interval

        self._inotify_watcher = InotifyLogWatcher(
            log_dir, callback, parser
        )
        self._polling_watcher = PollingLogWatcher(
            log_dir, callback, parser, poll_interval
        )
        self._active_watcher: InotifyLogWatcher | PollingLogWatcher | None = None
        self._watcher_type: str = "none"

    async def start(self) -> str:
        """
        Start watching log files.

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
        """Stop watching log files"""
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
# Ring Buffer for Log Streaming
# =============================================================================


@dataclass
class RingBufferConfig:
    """Configuration for log ring buffer"""
    capacity: int = 1000  # Maximum number of entries to keep
    enable_streaming: bool = True
    filter_level: str | None = None  # Filter by log level (e.g., "error")
    filter_worker: str | None = None  # Filter by worker ID


class LogRingBuffer:
    """
    Ring buffer for storing recent log entries.

    Maintains a fixed-size circular buffer of log entries for real-time
    display in the TUI logs panel.
    """

    def __init__(self, config: RingBufferConfig | None = None):
        """
        Initialize the ring buffer.

        Args:
            config: Buffer configuration
        """
        self.config = config if config is not None else RingBufferConfig()
        self._buffer: deque[LogEntry] = deque(
            maxlen=self.config.capacity
        )
        self._filtered_count = 0
        self._total_entries = 0

    def append(self, entry: LogEntry) -> None:
        """
        Append a log entry to the buffer.

        Args:
            entry: Log entry to add
        """
        self._total_entries += 1

        # Apply filters if configured
        if not self._should_include(entry):
            self._filtered_count += 1
            return

        self._buffer.append(entry)

    def extend(self, entries: list[LogEntry]) -> None:
        """
        Append multiple log entries to the buffer.

        Args:
            entries: Log entries to add
        """
        for entry in entries:
            self.append(entry)

    def _should_include(self, entry: LogEntry) -> bool:
        """Check if entry should be included based on filters"""
        if not entry.is_valid:
            # Include malformed entries
            return True

        # Level filter
        if self.config.filter_level:
            level_order = ["debug", "info", "warning", "error", "critical"]
            try:
                if level_order.index(entry.level.lower()) < \
                   level_order.index(self.config.filter_level.lower()):
                    return False
            except ValueError:
                pass  # Unknown level, include it

        # Worker filter
        if self.config.filter_worker and entry.worker_id:
            if entry.worker_id != self.config.filter_worker:
                return False

        return True

    def get_entries(self, count: int | None = None) -> list[LogEntry]:
        """
        Get entries from the buffer.

        Args:
            count: Maximum number of entries to return (None for all)

        Returns:
            List of LogEntry objects (most recent last)
        """
        entries = list(self._buffer)
        if count is not None:
            return entries[-count:]
        return entries

    def get_errors_only(self) -> list[LogEntry]:
        """Get only error-level entries"""
        return [e for e in self._buffer if e.is_error]

    def clear(self) -> None:
        """Clear all entries from the buffer"""
        self._buffer.clear()
        self._filtered_count = 0
        self._total_entries = 0

    @property
    def size(self) -> int:
        """Current number of entries in the buffer"""
        return len(self._buffer)

    def __len__(self) -> int:
        """Return the current number of entries in the buffer"""
        return len(self._buffer)

    @property
    def is_empty(self) -> bool:
        """Check if buffer is empty"""
        return len(self._buffer) == 0

    @property
    def is_full(self) -> bool:
        """Check if buffer is at capacity"""
        return len(self._buffer) == self._buffer.maxlen

    @property
    def filter_stats(self) -> dict[str, int]:
        """Get filtering statistics"""
        return {
            "total_received": self._total_entries,
            "filtered_out": self._filtered_count,
            "stored": len(self._buffer),
        }


# =============================================================================
# Convenience Functions
# =============================================================================


async def watch_log_files(
    log_dir: Path | str,
    callback: Callable[[LogFileEvent], None],
    poll_interval: float = 5.0,
) -> LogWatcher:
    """
    Convenience function to start watching log files.

    Args:
        log_dir: Directory containing log files
        callback: Function to call for each event
        poll_interval: Polling interval for fallback (default 5.0 per ADR 0008)

    Returns:
        LogWatcher instance
    """
    watcher = LogWatcher(
        log_dir=log_dir,
        callback=callback,
        poll_interval=poll_interval,
    )
    await watcher.start()
    return watcher


def parse_log_line(line: str) -> LogEntry:
    """
    Convenience function to parse a single log line with auto-detection.

    Args:
        line: Raw log line

    Returns:
        LogEntry with extracted fields
    """
    parser = LogParser(format=LogFormat.AUTO)
    return parser.parse_line(line)
