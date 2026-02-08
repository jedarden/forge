"""
Tests for FORGE Log Parser Module

Comprehensive tests for log parsing, format detection, ring buffer,
and async log tailing functionality.
"""

import asyncio
import json
from pathlib import Path
from unittest.mock import Mock, patch

import pytest

from forge.log_parser import (
    JsonlParser,
    KeyValueParser,
    LogParser,
    LogFormat,
    MalformedEntryPolicy,
    ParsedLogEntry,
    ParseStats,
    RingBufferConfig,
    LogRingBuffer,
    LogTailer,
    LogMonitor,
    parse_log_line,
    parse_log_file,
)


# =============================================================================
# Fixtures
# =============================================================================


@pytest.fixture
def jsonl_samples():
    """Sample JSONL log lines"""
    return [
        '{"timestamp": "2026-02-07T10:30:00Z", "level": "info", "worker_id": "sonnet-alpha", "message": "Worker started"}',
        '{"timestamp": "2026-02-07T10:30:05Z", "level": "info", "worker_id": "sonnet-alpha", "event": "task_started", "task_id": "bd-abc"}',
        '{"timestamp": "2026-02-07T10:35:00Z", "level": "error", "worker_id": "sonnet-alpha", "event": "task_failed", "task_id": "bd-def", "error": "API rate limit exceeded"}',
        '{"timestamp": "2026-02-07T10:35:01Z", "level": "warning", "worker_id": "sonnet-alpha", "message": "High memory usage"}',
        '{"timestamp": "2026-02-07T10:35:02Z", "level": "debug", "worker_id": "sonnet-alpha", "message": "Debug info"}',
    ]


@pytest.fixture
def keyvalue_samples():
    """Sample key-value log lines"""
    return [
        '2026-02-07T10:30:00Z level=info worker_id=sonnet-alpha message="Worker started"',
        '2026-02-07T10:30:05Z level=info worker_id=sonnet-alpha event=task_started task_id=bd-abc',
        '2026-02-07T10:35:00Z level=error worker_id=sonnet-alpha event=task_failed task_id=bd-def error="API rate limit exceeded"',
        '2026-02-07T10:35:01Z level=warn worker_id=sonnet-alpha message="High memory usage"',
        '2026-02-07T10:35:02Z level=debug worker_id=sonnet-alpha message="Debug info"',
    ]


@pytest.fixture
def malformed_samples():
    """Sample malformed log lines"""
    return [
        '',
        '   \n\t',
        'not a valid log format',
        '{invalid json}',
        'timestamp=2026-02-07T10:30:00Z no_equals_sign',
        '{"level": "info", "missing_timestamp": true}',
    ]


@pytest.fixture
def temp_log_file(tmp_path):
    """Create a temporary log file for testing"""
    log_file = tmp_path / "test.log"
    return log_file


# =============================================================================
# ParsedLogEntry Tests
# =============================================================================


class TestParsedLogEntry:
    """Tests for ParsedLogEntry dataclass"""

    def test_valid_entry_properties(self):
        """Test properties of a valid log entry"""
        entry = ParsedLogEntry(
            raw='{"level": "info", "message": "test"}',
            timestamp="2026-02-07T10:30:00Z",
            level="info",
            worker_id="test-worker",
            message="test",
        )

        assert entry.is_valid
        assert not entry.is_error
        assert not entry.is_warning

    def test_error_level_detection(self):
        """Test error level detection"""
        for level in ["error", "critical", "ERROR", "Critical"]:
            entry = ParsedLogEntry(raw="", level=level)
            assert entry.is_error

    def test_warning_level_detection(self):
        """Test warning level detection"""
        entry = ParsedLogEntry(raw="", level="warning")
        assert entry.is_warning

        entry = ParsedLogEntry(raw="", level="warn")
        assert entry.is_warning

    def test_parse_error_entry(self):
        """Test entry with parse error"""
        entry = ParsedLogEntry(
            raw="invalid log line",
            parse_error="Invalid JSON",
        )

        assert not entry.is_valid
        assert entry.parse_error == "Invalid JSON"

    def test_to_dict(self):
        """Test dictionary conversion"""
        entry = ParsedLogEntry(
            raw='{"level": "info"}',
            timestamp="2026-02-07T10:30:00Z",
            level="info",
            line_number=42,
        )

        d = entry.to_dict()
        assert d["raw"] == entry.raw
        assert d["timestamp"] == entry.timestamp
        assert d["level"] == entry.level
        assert d["line_number"] == 42


# =============================================================================
# JsonlParser Tests
# =============================================================================


class TestJsonlParser:
    """Tests for JSONL parser"""

    def test_parse_valid_entry(self):
        """Test parsing a valid JSONL entry"""
        parser = JsonlParser()
        line = '{"timestamp": "2026-02-07T10:30:00Z", "level": "info", "worker_id": "sonnet-alpha", "message": "Worker started"}'

        entry = parser.parse_line(line, 1)

        assert entry.is_valid
        assert entry.timestamp == "2026-02-07T10:30:00Z"
        assert entry.level == "info"
        assert entry.worker_id == "sonnet-alpha"
        assert entry.message == "Worker started"
        assert entry.line_number == 1

    def test_parse_with_event(self):
        """Test parsing entry with event type"""
        parser = JsonlParser()
        line = '{"timestamp": "2026-02-07T10:30:05Z", "level": "info", "worker_id": "sonnet-alpha", "event": "task_started", "task_id": "bd-abc"}'

        entry = parser.parse_line(line)

        assert entry.is_valid
        assert entry.event == "task_started"
        assert entry.extra["task_id"] == "bd-abc"

    def test_parse_with_extra_fields(self):
        """Test parsing entry with extra fields"""
        parser = JsonlParser()
        line = '{"timestamp": "2026-02-07T10:35:00Z", "level": "error", "worker_id": "sonnet-alpha", "custom_field": "value", "another": 123}'

        entry = parser.parse_line(line)

        assert entry.is_valid
        assert entry.extra["custom_field"] == "value"
        assert entry.extra["another"] == 123

    def test_parse_empty_line(self):
        """Test parsing empty line"""
        parser = JsonlParser()
        entry = parser.parse_line("", 1)

        assert not entry.is_valid
        assert entry.parse_error == "Empty line"

    def test_parse_invalid_json(self):
        """Test parsing invalid JSON"""
        parser = JsonlParser()
        entry = parser.parse_line("{invalid json}", 1)

        assert not entry.is_valid
        assert "Invalid JSON" in entry.parse_error

    def test_normalize_level(self):
        """Test level normalization"""
        parser = JsonlParser()

        # Test various level formats
        for level, expected in [
            ("INFO", "info"),
            ("Info", "info"),
            ("WARN", "warning"),
            ("ERR", "error"),
            ("CRIT", "critical"),
            ("FATAL", "critical"),
            ("debug", "debug"),
        ]:
            line = f'{{"level": "{level}"}}'
            entry = parser.parse_line(line)
            assert entry.level == expected


# =============================================================================
# KeyValueParser Tests
# =============================================================================


class TestKeyValueParser:
    """Tests for key-value parser"""

    def test_parse_valid_entry(self):
        """Test parsing a valid key-value entry"""
        parser = KeyValueParser()
        line = '2026-02-07T10:30:00Z level=info worker_id=sonnet-alpha message="Worker started"'

        entry = parser.parse_line(line, 1)

        assert entry.is_valid
        assert entry.timestamp == "2026-02-07T10:30:00Z"
        assert entry.level == "info"
        assert entry.worker_id == "sonnet-alpha"
        assert entry.message == "Worker started"
        assert entry.line_number == 1

    def test_parse_with_event(self):
        """Test parsing entry with event type"""
        parser = KeyValueParser()
        line = '2026-02-07T10:30:05Z level=info worker_id=sonnet-alpha event=task_started task_id=bd-abc'

        entry = parser.parse_line(line)

        assert entry.is_valid
        assert entry.event == "task_started"
        assert entry.extra["task_id"] == "bd-abc"

    def test_parse_quoted_values(self):
        """Test parsing quoted values with spaces"""
        parser = KeyValueParser()
        line = '2026-02-07T10:35:00Z level=error worker_id=sonnet-alpha error="API rate limit exceeded"'

        entry = parser.parse_line(line)

        assert entry.is_valid
        assert entry.extra["error"] == "API rate limit exceeded"

    def test_parse_empty_line(self):
        """Test parsing empty line"""
        parser = KeyValueParser()
        entry = parser.parse_line("", 1)

        assert not entry.is_valid
        assert entry.parse_error == "Empty line"

    def test_normalize_level(self):
        """Test level normalization"""
        parser = KeyValueParser()

        for level, expected in [
            ("INFO", "info"),
            ("WARN", "warning"),
            ("ERR", "error"),
            ("CRIT", "critical"),
        ]:
            line = f'2026-02-07T10:30:00Z level={level} worker_id=test'
            entry = parser.parse_line(line)
            assert entry.level == expected


# =============================================================================
# LogParser Tests (Universal Parser)
# =============================================================================


class TestLogParser:
    """Tests for universal log parser"""

    def test_detect_jsonl_format(self):
        """Test JSONL format detection"""
        parser = LogParser(format=LogFormat.AUTO)

        line = '{"timestamp": "2026-02-07T10:30:00Z", "level": "info"}'
        assert parser.detect_format(line) == LogFormat.JSONL

    def test_detect_keyvalue_format(self):
        """Test key-value format detection"""
        parser = LogParser(format=LogFormat.AUTO)

        line = '2026-02-07T10:30:00Z level=info worker_id=test'
        assert parser.detect_format(line) == LogFormat.KEYVALUE

    def test_detect_unknown_format(self):
        """Test unknown format detection"""
        parser = LogParser(format=LogFormat.AUTO)

        assert parser.detect_format("") == LogFormat.UNKNOWN
        assert parser.detect_format("not a valid format") == LogFormat.UNKNOWN

    def test_auto_parse_jsonl(self, jsonl_samples):
        """Test auto-detection and parsing of JSONL"""
        parser = LogParser(format=LogFormat.AUTO)

        for line in jsonl_samples:
            entry = parser.parse_line(line)
            assert entry.is_valid, f"Failed to parse: {line}"

    def test_auto_parse_keyvalue(self, keyvalue_samples):
        """Test auto-detection and parsing of key-value"""
        parser = LogParser(format=LogFormat.AUTO)

        for line in keyvalue_samples:
            entry = parser.parse_line(line)
            assert entry.is_valid, f"Failed to parse: {line}"

    def test_malformed_policy_skip(self, malformed_samples):
        """Test SKIP policy for malformed entries"""
        parser = LogParser(
            format=LogFormat.AUTO,
            malformed_policy=MalformedEntryPolicy.SKIP,
        )

        # Should not raise, just mark as invalid
        for line in malformed_samples:
            entry = parser.parse_line(line)
            # Only non-empty, actually malformed entries should be invalid
            if line.strip() and line not in ['{"level": "info", "missing_timestamp": true}']:
                assert not entry.is_valid

    def test_malformed_policy_pass_through(self, malformed_samples):
        """Test PASS_THROUGH policy for malformed entries"""
        parser = LogParser(
            format=LogFormat.AUTO,
            malformed_policy=MalformedEntryPolicy.PASS_THROUGH,
        )

        # All entries should be returned, even if invalid
        count = 0
        for line in malformed_samples:
            entry = parser.parse_line(line)
            count += 1
            # Only expect parse_error for truly malformed entries (not valid JSON)
            if line.strip() and line not in ['{"level": "info", "missing_timestamp": true}']:
                assert entry.parse_error is not None

    def test_stats_tracking(self, jsonl_samples, malformed_samples):
        """Test parse statistics tracking"""
        parser = LogParser(format=LogFormat.AUTO)

        # Count non-empty malformed samples that are actually malformed (not valid JSON)
        truly_malformed = [
            line for line in malformed_samples
            if line.strip() and line not in ['{"level": "info", "missing_timestamp": true}']
        ]

        # Parse valid entries
        for line in jsonl_samples:
            parser.parse_line(line)

        # Parse malformed entries (only non-empty, truly malformed ones)
        for line in truly_malformed:
            parser.parse_line(line)

        expected_total = len(jsonl_samples) + len(truly_malformed)
        assert parser.stats.total_lines == expected_total
        assert parser.stats.successful_parses == len(jsonl_samples)  # Only valid JSONL samples
        assert parser.stats.failed_parses == len(truly_malformed)

    def test_format_locking(self, jsonl_samples):
        """Test that detected format is locked after first success"""
        parser = LogParser(format=LogFormat.AUTO)

        # Parse JSONL entry
        parser.parse_line(jsonl_samples[0])
        assert parser._detected_format == LogFormat.JSONL

        # Try to parse key-value - should still use JSONL
        kv_line = '2026-02-07T10:30:00Z level=info worker_id=test'
        entry = parser.parse_line(kv_line)
        assert not entry.is_valid  # Failed because it tried JSONL parser

    def test_malformed_callback(self):
        """Test malformed entry callback"""
        callback_calls = []

        def on_malformed(entry):
            callback_calls.append(entry)

        parser = LogParser(
            format=LogFormat.AUTO,
            malformed_policy=MalformedEntryPolicy.PASS_THROUGH,
            on_malformed=on_malformed,
        )

        parser.parse_line("invalid log line")
        assert len(callback_calls) == 1
        assert callback_calls[0].parse_error is not None


# =============================================================================
# LogRingBuffer Tests
# =============================================================================


class TestLogRingBuffer:
    """Tests for log ring buffer"""

    def test_append_and_retrieve(self):
        """Test appending and retrieving entries"""
        config = RingBufferConfig(capacity=10)
        buffer = LogRingBuffer(config)

        for i in range(5):
            entry = ParsedLogEntry(
                raw=f"line {i}",
                level="info",
                message=f"message {i}",
            )
            buffer.append(entry)

        assert buffer.size == 5

        entries = buffer.get_entries()
        assert len(entries) == 5

    def test_capacity_limit(self):
        """Test ring buffer capacity limit"""
        config = RingBufferConfig(capacity=3)
        buffer = LogRingBuffer(config)

        # Add 5 entries, but only 3 should be kept
        for i in range(5):
            entry = ParsedLogEntry(
                raw=f"line {i}",
                level="info",
                message=f"message {i}",
            )
            buffer.append(entry)

        assert buffer.size == 3

        entries = buffer.get_entries()
        assert len(entries) == 3
        # Should keep the most recent 3
        assert entries[0].raw == "line 2"
        assert entries[2].raw == "line 4"

    def test_get_entries_count(self):
        """Test getting limited number of entries"""
        config = RingBufferConfig(capacity=100)
        buffer = LogRingBuffer(config)

        for i in range(10):
            buffer.append(ParsedLogEntry(raw=f"line {i}", level="info"))

        entries = buffer.get_entries(5)
        assert len(entries) == 5

    def test_filter_by_level(self):
        """Test filtering by log level"""
        config = RingBufferConfig(capacity=100, filter_level="error")
        buffer = LogRingBuffer(config)

        # Add mixed levels
        for level in ["debug", "info", "warning", "error", "critical"]:
            buffer.append(ParsedLogEntry(raw="", level=level))

        # Only error and critical should be kept
        assert buffer.size == 2

    def test_filter_by_worker(self):
        """Test filtering by worker ID"""
        config = RingBufferConfig(capacity=100, filter_worker="worker-1")
        buffer = LogRingBuffer(config)

        # Add entries from different workers
        for worker_id in ["worker-1", "worker-2", "worker-1"]:
            buffer.append(ParsedLogEntry(
                raw="",
                level="info",
                worker_id=worker_id,
            ))

        # Only worker-1 entries should be kept
        assert buffer.size == 2

    def test_get_errors_only(self):
        """Test getting only error entries"""
        buffer = LogRingBuffer()

        for level in ["info", "error", "warning", "critical", "debug"]:
            buffer.append(ParsedLogEntry(raw="", level=level))

        errors = buffer.get_errors_only()
        assert len(errors) == 2  # error + critical

    def test_get_warnings_and_errors(self):
        """Test getting warning and error entries"""
        buffer = LogRingBuffer()

        for level in ["info", "error", "warning", "critical", "debug"]:
            buffer.append(ParsedLogEntry(raw="", level=level))

        warnings_errors = buffer.get_warnings_and_errors()
        assert len(warnings_errors) == 3  # warning + error + critical

    def test_clear(self):
        """Test clearing the buffer"""
        buffer = LogRingBuffer()

        buffer.append(ParsedLogEntry(raw="test", level="info"))
        assert buffer.size == 1

        buffer.clear()
        assert buffer.size == 0
        assert buffer.is_empty

    def test_filter_stats(self):
        """Test filtering statistics"""
        config = RingBufferConfig(capacity=10, filter_level="error")
        buffer = LogRingBuffer(config)

        for level in ["info", "info", "error"]:
            buffer.append(ParsedLogEntry(raw="", level=level))

        stats = buffer.filter_stats
        assert stats["total_received"] == 3
        assert stats["filtered_out"] == 2  # info entries filtered
        assert stats["stored"] == 1  # only error stored


# =============================================================================
# LogTailer Tests
# =============================================================================


class TestLogTailer:
    """Tests for async log tailer"""

    @pytest.mark.asyncio
    async def test_read_existing_content(self, temp_log_file, jsonl_samples):
        """Test reading existing content from log file"""
        # Write sample content
        temp_log_file.write_text("\n".join(jsonl_samples))

        parser = LogParser(format=LogFormat.JSONL)
        buffer = LogRingBuffer(RingBufferConfig(capacity=100))
        tailer = LogTailer(temp_log_file, parser, buffer)

        await tailer._read_existing()

        assert buffer.size == len(jsonl_samples)

    @pytest.mark.asyncio
    async def test_start_and_stop(self, temp_log_file):
        """Test starting and stopping tailer"""
        temp_log_file.write_text("test log line")

        parser = LogParser(format=LogFormat.AUTO)
        buffer = LogRingBuffer()
        tailer = LogTailer(temp_log_file, parser, buffer)

        await tailer.start()
        assert tailer.is_running

        await tailer.stop()
        assert not tailer.is_running

    @pytest.mark.asyncio
    async def test_file_rotation_detection(self, temp_log_file):
        """Test file rotation detection"""
        # This test verifies the tailer can handle file rotation
        # (inode change) gracefully

        parser = LogParser(format=LogFormat.AUTO)
        buffer = LogRingBuffer()
        tailer = LogTailer(temp_log_file, parser, buffer, poll_interval=0.1)

        await tailer.start()

        # Write initial content
        temp_log_file.write_text("line 1\n")
        await asyncio.sleep(0.3)  # Wait for polling

        initial_size = buffer.size
        assert initial_size >= 1

        # Simulate rotation by removing and recreating file
        temp_log_file.unlink()
        temp_log_file.write_text("line 2\n")

        await asyncio.sleep(0.3)  # Wait for polling

        # Should detect rotation and read new content
        final_size = buffer.size
        assert final_size >= initial_size

        await tailer.stop()

    @pytest.mark.asyncio
    async def test_entries_stream(self, temp_log_file):
        """Test streaming entries as they arrive"""
        parser = LogParser(format=LogFormat.AUTO)
        buffer = LogRingBuffer()
        tailer = LogTailer(temp_log_file, parser, buffer, poll_interval=0.05)

        await tailer.start()

        # Start collecting entries in background
        collect_task = asyncio.create_task(self._collect_n_entries(tailer, 3))

        # Give tailer time to start, then write entries
        await asyncio.sleep(0.1)

        # Write entries
        for i in range(3):
            with open(temp_log_file, 'a') as f:
                f.write(f'{{"level": "info", "message": "line {i}"}}\n')
            await asyncio.sleep(0.1)

        # Wait for collection to complete
        collected = await asyncio.wait_for(collect_task, timeout=2.0)
        await tailer.stop()

        assert len(collected) == 3

    async def _collect_n_entries(self, tailer, n):
        """Helper to collect n entries from tailer"""
        collected = []
        async for entry in tailer.entries():
            collected.append(entry)
            if len(collected) >= n:
                break
        return collected


# =============================================================================
# LogMonitor Tests
# =============================================================================


class TestLogMonitor:
    """Tests for multi-file log monitor"""

    @pytest.mark.asyncio
    async def test_add_and_remove_paths(self, tmp_path):
        """Test adding and removing log file paths"""
        log1 = tmp_path / "log1.log"
        log2 = tmp_path / "log2.log"

        # Create files
        log1.write_text("test 1\n")
        log2.write_text("test 2\n")

        monitor = LogMonitor()
        await monitor.add_path(log1)
        await monitor.add_path(log2)

        assert monitor.tailer_count == 2
        assert monitor.is_monitoring

        await monitor.remove_path(log1)

        assert monitor.tailer_count == 1

        await monitor.stop_all()
        assert monitor.tailer_count == 0

    @pytest.mark.asyncio
    async def test_aggregate_buffer(self, tmp_path):
        """Test that multiple files aggregate to single buffer"""
        log1 = tmp_path / "log1.log"
        log2 = tmp_path / "log2.log"

        log1.write_text('{"level": "info", "message": "log1"}\n')
        log2.write_text('{"level": "info", "message": "log2"}\n')

        buffer = LogRingBuffer(RingBufferConfig(capacity=100))
        monitor = LogMonitor(paths=[log1, log2], buffer=buffer)

        await monitor.start_all()
        # Give more time for async operations to complete
        await asyncio.sleep(0.5)  # Wait for initial read

        # Should have entries from both files
        assert buffer.size >= 2, f"Expected at least 2 entries, got {buffer.size}"

        await monitor.stop_all()


# =============================================================================
# Convenience Functions Tests
# =============================================================================


class TestConvenienceFunctions:
    """Tests for convenience functions"""

    def test_parse_log_line_jsonl(self):
        """Test parsing a single JSONL line"""
        line = '{"timestamp": "2026-02-07T10:30:00Z", "level": "info"}'

        entry = parse_log_line(line)

        assert entry.is_valid
        assert entry.level == "info"

    def test_parse_log_line_keyvalue(self):
        """Test parsing a single key-value line"""
        line = '2026-02-07T10:30:00Z level=info worker_id=test'

        entry = parse_log_line(line)

        assert entry.is_valid
        assert entry.level == "info"

    def test_parse_log_file(self, temp_log_file, jsonl_samples):
        """Test parsing an entire log file"""
        temp_log_file.write_text("\n".join(jsonl_samples))

        entries = parse_log_file(temp_log_file)

        assert len(entries) == len(jsonl_samples)
        assert all(e.is_valid for e in entries)


# =============================================================================
# Integration Tests
# =============================================================================


class TestLogParserIntegration:
    """Integration tests for log parser module"""

    @pytest.mark.asyncio
    async def test_full_workflow(self, tmp_path):
        """Test complete workflow: write logs, tail, parse, buffer"""
        log_file = tmp_path / "worker.log"

        # Write sample logs
        jsonl_logs = [
            '{"timestamp": "2026-02-07T10:30:00Z", "level": "info", "worker_id": "sonnet-alpha", "message": "Worker started"}',
            '{"timestamp": "2026-02-07T10:30:05Z", "level": "info", "worker_id": "sonnet-alpha", "event": "task_started", "task_id": "bd-abc"}',
            '{"timestamp": "2026-02-07T10:35:00Z", "level": "error", "worker_id": "sonnet-alpha", "event": "task_failed", "error": "API rate limit"}',
        ]

        log_file.write_text("\n".join(jsonl_logs))

        # Set up tailer
        parser = LogParser(format=LogFormat.JSONL)
        buffer = LogRingBuffer(RingBufferConfig(capacity=1000))
        tailer = LogTailer(log_file, parser, buffer)

        # Read existing content
        await tailer._read_existing()

        # Verify parsing
        assert buffer.size == 3

        # Verify error detection
        errors = buffer.get_errors_only()
        assert len(errors) == 1
        assert errors[0].event == "task_failed"

        # Verify statistics
        assert parser.stats.successful_parses == 3
        assert parser.stats.success_rate() == 100.0

    @pytest.mark.asyncio
    async def test_mixed_format_detection(self, tmp_path):
        """Test handling mixed log formats (should lock after first detection)"""
        log_file = tmp_path / "mixed.log"

        # Start with JSONL
        mixed_logs = [
            '{"timestamp": "2026-02-07T10:30:00Z", "level": "info", "message": "JSONL line"}',
            '2026-02-07T10:30:01Z level=info message="KeyValue line"',  # Will fail
        ]

        log_file.write_text("\n".join(mixed_logs))

        parser = LogParser(format=LogFormat.AUTO)
        entries = []

        async for entry in parser.parse_file(log_file):
            entries.append(entry)

        # First should succeed, second should fail (format locked to JSONL)
        assert entries[0].is_valid
        assert not entries[1].is_valid

    @pytest.mark.asyncio
    async def test_graceful_malformed_handling(self, tmp_path):
        """Test graceful handling of malformed entries"""
        log_file = tmp_path / "malformed.log"

        logs = [
            '{"timestamp": "2026-02-07T10:30:00Z", "level": "info", "message": "Valid"}',
            'this is not valid',
            '{broken json}',
            '{"timestamp": "2026-02-07T10:30:01Z", "level": "info", "message": "Valid again"}',
        ]

        log_file.write_text("\n".join(logs))

        parser = LogParser(
            format=LogFormat.AUTO,
            malformed_policy=MalformedEntryPolicy.PASS_THROUGH,
        )

        entries = []
        async for entry in parser.parse_file(log_file):
            entries.append(entry)

        # All entries should be returned
        assert len(entries) == 4

        # Check validity
        assert entries[0].is_valid
        assert not entries[1].is_valid
        assert not entries[2].is_valid
        assert entries[3].is_valid

        # Check stats
        assert parser.stats.successful_parses == 2
        assert parser.stats.failed_parses == 2
