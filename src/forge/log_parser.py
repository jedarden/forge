"""
FORGE Log Parser Module

Implements log parsing and streaming for worker logs.
Supports JSON Lines (JSONL) and key-value formats as specified in INTEGRATION_GUIDE.md.
Provides async log tailing with ring buffer and graceful error handling.

Reference: docs/INTEGRATION_GUIDE.md#log-collection
Error Handling: docs/adr/0014-error-handling-strategy.md
"""

from __future__ import annotations

import asyncio
import json
import re
from collections import deque
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from pathlib import Path
from typing import Any, AsyncIterator, Callable


# =============================================================================
# Log Format Detection
# =============================================================================


class LogFormat(Enum):
    """Log format types supported by FORGE"""
    JSONL = "jsonl"          # JSON Lines format: {"timestamp": "...", "level": "info", ...}
    KEYVALUE = "keyvalue"    # Key-value format: timestamp=... level=info ...
    AUTO = "auto"            # Auto-detect format
    UNKNOWN = "unknown"


# =============================================================================
# Parsed Log Entry
# =============================================================================


@dataclass
class ParsedLogEntry:
    """
    A parsed log entry with normalized fields.

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

    @property
    def is_warning(self) -> bool:
        """Check if this is a warning-level log entry"""
        return self.level.lower() in ("warning", "warn")


# =============================================================================
# Malformed Entry Handler
# =============================================================================


class MalformedEntryPolicy(Enum):
    """Policy for handling malformed log entries"""
    SKIP = "skip"              # Silently skip malformed entries
    PASS_THROUGH = "pass"      # Include as-is with parse_error field
    RAISE = "raise"            # Raise an exception
    LOG = "log"                # Log to stderr and continue


@dataclass
class ParseStats:
    """Statistics for log parsing operations"""
    total_lines: int = 0
    successful_parses: int = 0
    failed_parses: int = 0
    empty_lines: int = 0
    format_counts: dict[str, int] = field(default_factory=dict)

    def success_rate(self) -> float:
        """Calculate success rate as percentage"""
        if self.total_lines == 0:
            return 100.0
        return (self.successful_parses / self.total_lines) * 100


# =============================================================================
# JSON Lines Parser
# =============================================================================


class JsonlParser:
    """
    Parser for JSON Lines (JSONL) format.

    Each line is a complete JSON object:
    {"timestamp": "2026-02-07T10:30:00Z", "level": "info", "worker_id": "sonnet-alpha", "message": "..."}
    """

    # Standard fields expected in FORGE logs
    STANDARD_FIELDS = {"timestamp", "level", "worker_id", "message", "event"}

    def parse_line(self, line: str, line_number: int = 0) -> ParsedLogEntry:
        """
        Parse a single JSONL line.

        Args:
            line: Raw log line
            line_number: Line number for error reporting

        Returns:
            ParsedLogEntry with extracted fields
        """
        line = line.strip()

        if not line:
            return ParsedLogEntry(
                raw=line,
                line_number=line_number,
                parse_error="Empty line"
            )

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

            return ParsedLogEntry(
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
            return ParsedLogEntry(
                raw=line,
                line_number=line_number,
                parse_error=f"Invalid JSON: {str(e)[:100]}"
            )
        except Exception as e:
            return ParsedLogEntry(
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
# Key-Value Parser
# =============================================================================


class KeyValueParser:
    """
    Parser for key-value log format.

    Format:
    2026-02-07T10:30:00Z level=info worker_id=sonnet-alpha message="Worker started"
    2026-02-07T10:30:05Z level=info worker_id=sonnet-alpha event=task_started task_id=bd-abc
    """

    # Pattern for key=value pairs (supports quoted and unquoted values)
    KEY_VALUE_PATTERN = re.compile(r'(\w+)=(?:"([^"]*)"|(\S*))')

    # Pattern for quoted values with spaces
    QUOTED_VALUE_PATTERN = re.compile(r'(\w+)="([^"]*)"')

    def parse_line(self, line: str, line_number: int = 0) -> ParsedLogEntry:
        """
        Parse a single key-value line.

        Args:
            line: Raw log line
            line_number: Line number for error reporting

        Returns:
            ParsedLogEntry with extracted fields
        """
        line = line.strip()

        if not line:
            return ParsedLogEntry(
                raw=line,
                line_number=line_number,
                parse_error="Empty line"
            )

        try:
            # Split into parts (first part is usually timestamp)
            parts = line.split()

            # Extract timestamp (first non-key=value part)
            timestamp = None
            idx = 0

            if parts and '=' not in parts[0]:
                timestamp = parts[0]
                idx = 1

            # Parse key=value pairs using regex to handle quoted values
            data: dict[str, str] = {}

            # Use the pattern to find all key=value pairs (including quoted)
            remaining_line = ' '.join(parts[idx:])
            i = 0

            while i < len(parts[idx:]):
                part = parts[idx:][i]

                if '=' in part:
                    key, value = part.split('=', 1)

                    # Check if value is quoted (starts with quote but no closing quote on same part)
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
                        # Regular value, strip quotes if present
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

            return ParsedLogEntry(
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
            return ParsedLogEntry(
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
# Universal Log Parser (Format Detection)
# =============================================================================


class LogParser:
    """
    Universal log parser with automatic format detection.

    Supports:
    - JSON Lines (JSONL) format
    - Key-value format
    - Auto-detection

    Usage:
        parser = LogParser(format=LogFormat.AUTO)
        entry = parser.parse_line('{"timestamp": "...", "level": "info"}')
    """

    def __init__(
        self,
        format: LogFormat = LogFormat.AUTO,
        malformed_policy: MalformedEntryPolicy = MalformedEntryPolicy.PASS_THROUGH,
        on_malformed: Callable[[ParsedLogEntry], None] | None = None,
    ):
        """
        Initialize the log parser.

        Args:
            format: Log format to use (AUTO detects automatically)
            malformed_policy: How to handle malformed entries
            on_malformed: Optional callback for malformed entries
        """
        self.format = format
        self.malformed_policy = malformed_policy
        self.on_malformed = on_malformed
        self._detected_format: LogFormat | None = None

        # Initialize format-specific parsers
        self._jsonl_parser = JsonlParser()
        self._keyvalue_parser = KeyValueParser()

        # Statistics
        self.stats = ParseStats()

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
            return LogFormat.UNKNOWN

        # Try JSON first
        if line.startswith('{'):
            try:
                json.loads(line)
                return LogFormat.JSONL
            except json.JSONDecodeError:
                pass

        # Check for key=value pattern
        if '=' in line and ' ' in line:
            # Look for key=value pairs
            parts = line.split()
            kv_count = sum(1 for p in parts if '=' in p)
            if kv_count >= 2:  # At least 2 key=value pairs
                return LogFormat.KEYVALUE

        return LogFormat.UNKNOWN

    def parse_line(self, line: str, line_number: int = 0) -> ParsedLogEntry:
        """
        Parse a single log line with auto-detection.

        Args:
            line: Raw log line
            line_number: Line number for error reporting

        Returns:
            ParsedLogEntry with extracted fields
        """
        self.stats.total_lines += 1

        # Handle empty lines
        if not line.strip():
            self.stats.empty_lines += 1
            return ParsedLogEntry(
                raw=line,
                line_number=line_number,
            )

        # Determine format to use
        format_to_use = self.format

        if format_to_use == LogFormat.AUTO:
            format_to_use = self.detect_format(line)

            # Lock in detected format after first successful parse
            if format_to_use != LogFormat.UNKNOWN and self._detected_format is None:
                self._detected_format = format_to_use

            # Use previously detected format if available
            if self._detected_format is not None:
                format_to_use = self._detected_format

        # Parse based on format
        if format_to_use == LogFormat.JSONL:
            entry = self._jsonl_parser.parse_line(line, line_number)
        elif format_to_use == LogFormat.KEYVALUE:
            entry = self._keyvalue_parser.parse_line(line, line_number)
        else:
            # Unknown format - treat as parse error
            entry = ParsedLogEntry(
                raw=line,
                line_number=line_number,
                parse_error="Unknown log format"
            )

        # Update format statistics
        if format_to_use != LogFormat.UNKNOWN:
            self.stats.format_counts[format_to_use.value] = \
                self.stats.format_counts.get(format_to_use.value, 0) + 1

        # Handle malformed entries
        if not entry.is_valid:
            self.stats.failed_parses += 1
            self._handle_malformed(entry)
        else:
            self.stats.successful_parses += 1

        return entry

    def _handle_malformed(self, entry: ParsedLogEntry) -> None:
        """Handle malformed entry based on policy"""
        if self.on_malformed:
            self.on_malformed(entry)

        if self.malformed_policy == MalformedEntryPolicy.SKIP:
            return
        elif self.malformed_policy == MalformedEntryPolicy.LOG:
            import sys
            print(f"[FORGE] Malformed log entry at line {entry.line_number}: "
                  f"{entry.parse_error}", file=sys.stderr)
        elif self.malformed_policy == MalformedEntryPolicy.RAISE:
            raise ValueError(f"Malformed log entry: {entry.parse_error}")

    async def parse_file(self, path: Path | str) -> AsyncIterator[ParsedLogEntry]:
        """
        Parse a log file asynchronously.

        Args:
            path: Path to log file

        Yields:
            ParsedLogEntry objects
        """
        path = Path(path).expanduser()

        if not path.exists():
            raise FileNotFoundError(f"Log file not found: {path}")

        # Run file reading in thread pool to avoid blocking
        loop = asyncio.get_event_loop()

        def _read_file():
            entries = []
            with open(path, 'r', encoding='utf-8', errors='replace') as f:
                for line_number, line in enumerate(f, 1):
                    entry = self.parse_line(line, line_number)

                    # Skip if policy says so
                    if not entry.is_valid and self.malformed_policy == MalformedEntryPolicy.SKIP:
                        continue

                    entries.append(entry)
            return entries

        entries = await loop.run_in_executor(None, _read_file)

        for entry in entries:
            yield entry


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
        self._buffer: deque[ParsedLogEntry] = deque(
            maxlen=self.config.capacity
        )
        self._filtered_count = 0
        self._total_entries = 0

    def append(self, entry: ParsedLogEntry) -> None:
        """
        Append a log entry to the buffer.

        Args:
            entry: Parsed log entry to add
        """
        self._total_entries += 1

        # Apply filters if configured
        if not self._should_include(entry):
            self._filtered_count += 1
            return

        self._buffer.append(entry)

    def _should_include(self, entry: ParsedLogEntry) -> bool:
        """Check if entry should be included based on filters"""
        if not entry.is_valid:
            # Include malformed entries if not skipping
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

    def get_entries(self, count: int | None = None) -> list[ParsedLogEntry]:
        """
        Get entries from the buffer.

        Args:
            count: Maximum number of entries to return (None for all)

        Returns:
            List of ParsedLogEntry objects (most recent last)
        """
        entries = list(self._buffer)
        if count is not None:
            return entries[-count:]
        return entries

    def get_errors_only(self) -> list[ParsedLogEntry]:
        """Get only error-level entries"""
        return [e for e in self._buffer if e.is_error]

    def get_warnings_and_errors(self) -> list[ParsedLogEntry]:
        """Get warning and error entries"""
        return [e for e in self._buffer if e.is_error or e.is_warning]

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
# Async Log Tailing
# =============================================================================


class LogTailer:
    """
    Async log file tailer with streaming support.

    Monitors log files for new entries and streams them to a ring buffer.
    Handles file rotation and graceful error handling.
    """

    def __init__(
        self,
        path: Path | str,
        parser: LogParser | None = None,
        buffer: LogRingBuffer | None = None,
        poll_interval: float = 0.5,
    ):
        """
        Initialize the log tailer.

        Args:
            path: Path to log file to monitor
            parser: Log parser to use (creates default if None)
            buffer: Ring buffer to store entries (creates default if None)
            poll_interval: Seconds between polling for new lines
        """
        self.path = Path(path).expanduser()
        self.parser = parser if parser is not None else LogParser(format=LogFormat.AUTO)
        self.buffer = buffer if buffer is not None else LogRingBuffer()
        self.poll_interval = poll_interval

        # State
        self._running = False
        self._task: asyncio.Task[None] | None = None
        self._file_handle: Any | None = None
        self._file_inode: int | None = None
        self._last_position = 0

    async def start(self) -> None:
        """Start tailing the log file"""
        if self._running:
            return

        self._running = True
        self._task = asyncio.create_task(self._tail_loop())

        # Read existing content first
        await self._read_existing()

    async def stop(self) -> None:
        """Stop tailing the log file"""
        self._running = False

        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
            self._task = None

        if self._file_handle:
            self._file_handle.close()
            self._file_handle = None

    async def _read_existing(self) -> None:
        """Read existing content from the log file"""
        if not self.path.exists():
            return

        async for entry in self.parser.parse_file(self.path):
            self.buffer.append(entry)

        # Update last position
        self._last_position = self.path.stat().st_size

    async def _tail_loop(self) -> None:
        """Main tailing loop"""
        while self._running:
            try:
                await self._check_for_new_lines()
                await asyncio.sleep(self.poll_interval)
            except asyncio.CancelledError:
                break
            except Exception as e:
                # Log error but continue running
                import sys
                print(f"[FORGE] Log tailer error for {self.path}: {e}", file=sys.stderr)
                await asyncio.sleep(self.poll_interval)

    async def _check_for_new_lines(self) -> None:
        """Check for new lines in the log file"""
        if not self.path.exists():
            return

        # Check for file rotation (inode change)
        current_stat = self.path.stat()
        current_inode = current_stat.st_ino

        if self._file_inode is not None and current_inode != self._file_inode:
            # File was rotated
            if self._file_handle:
                self._file_handle.close()
            self._file_handle = None
            self._last_position = 0

        self._file_inode = current_inode

        # Open file if not already open
        if self._file_handle is None:
            self._file_handle = open(self.path, 'r', encoding='utf-8', errors='replace')

        # Seek to last position
        self._file_handle.seek(self._last_position)

        # Read new lines
        line_number = 0
        for line in self._file_handle:
            line_number += 1
            entry = self.parser.parse_line(line, line_number)
            self.buffer.append(entry)

        # Update last position
        self._last_position = self._file_handle.tell()

    async def entries(self) -> AsyncIterator[ParsedLogEntry]:
        """
        Stream entries as they arrive.

        Yields:
            ParsedLogEntry objects
        """
        last_size = len(self.buffer)

        while self._running:
            # Check for new entries
            current_size = len(self.buffer)

            if current_size > last_size:
                new_entries = self.buffer.get_entries()[last_size:]
                for entry in new_entries:
                    yield entry
                last_size = current_size

            await asyncio.sleep(self.poll_interval)

    @property
    def is_running(self) -> bool:
        """Check if tailer is running"""
        return self._running


# =============================================================================
# Multi-File Log Monitor
# =============================================================================


class LogMonitor:
    """
    Monitor multiple log files simultaneously.

    Aggregates entries from multiple tailers into a single ring buffer.
    """

    def __init__(
        self,
        paths: list[Path | str] | None = None,
        parser: LogParser | None = None,
        buffer: LogRingBuffer | None = None,
    ):
        """
        Initialize the log monitor.

        Args:
            paths: List of log file paths to monitor
            parser: Log parser to use (creates default if None)
            buffer: Ring buffer to store entries (creates default if None)
        """
        self.paths = [Path(p).expanduser() for p in paths] if paths else []
        self.parser = parser if parser is not None else LogParser(format=LogFormat.AUTO)
        self.buffer = buffer if buffer is not None else LogRingBuffer()

        self._tailers: dict[Path, LogTailer] = {}

    async def add_path(self, path: Path | str) -> None:
        """
        Add a log file to monitor.

        Args:
            path: Path to log file
        """
        path = Path(path).expanduser()

        if path in self._tailers:
            return  # Already monitoring

        tailer = LogTailer(path, self.parser, self.buffer)
        self._tailers[path] = tailer
        await tailer.start()

    async def remove_path(self, path: Path | str) -> None:
        """
        Stop monitoring a log file.

        Args:
            path: Path to log file
        """
        path = Path(path).expanduser()

        if path in self._tailers:
            await self._tailers[path].stop()
            del self._tailers[path]

    async def start_all(self) -> None:
        """Start monitoring all configured paths"""
        for path in self.paths:
            await self.add_path(path)

    async def stop_all(self) -> None:
        """Stop monitoring all log files"""
        for tailer in self._tailers.values():
            await tailer.stop()
        self._tailers.clear()

    @property
    def tailer_count(self) -> int:
        """Number of active tailers"""
        return len(self._tailers)

    @property
    def is_monitoring(self) -> bool:
        """Check if any files are being monitored"""
        return len(self._tailers) > 0


# =============================================================================
# Convenience Functions
# =============================================================================


async def tail_logs(
    paths: list[Path | str],
    buffer_capacity: int = 1000,
) -> LogRingBuffer:
    """
    Convenience function to tail multiple log files.

    Args:
        paths: List of log file paths to monitor
        buffer_capacity: Maximum entries in ring buffer

    Returns:
        LogRingBuffer with aggregated entries
    """
    buffer = LogRingBuffer(RingBufferConfig(capacity=buffer_capacity))
    monitor = LogMonitor(paths=paths, buffer=buffer)

    await monitor.start_all()

    return buffer


def parse_log_line(line: str) -> ParsedLogEntry:
    """
    Convenience function to parse a single log line with auto-detection.

    Args:
        line: Raw log line

    Returns:
        ParsedLogEntry with extracted fields
    """
    parser = LogParser(format=LogFormat.AUTO)
    return parser.parse_line(line)


def parse_log_file(path: Path | str) -> list[ParsedLogEntry]:
    """
    Convenience function to parse an entire log file.

    Args:
        path: Path to log file

    Returns:
        List of ParsedLogEntry objects
    """
    parser = LogParser(format=LogFormat.AUTO)

    # Synchronous wrapper for async generator
    async def _parse() -> list[ParsedLogEntry]:
        entries = []
        async for entry in parser.parse_file(path):
            entries.append(entry)
        return entries

    return asyncio.run(_parse())
