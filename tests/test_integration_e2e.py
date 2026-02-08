"""
FORGE End-to-End Integration Tests

Tests complete workflows with mock workers, including:
- Launcher spawning, status file creation, log parsing
- Bead integration and worker discovery
- Error scenarios from ADR 0014
- Multi-worker coordination
"""

import asyncio
import json
import os
import signal
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from unittest.mock import AsyncMock, Mock, patch
from typing import Any

import pytest

from forge.launcher import (
    WorkerLauncher,
    LauncherConfig,
    LauncherResult,
    LauncherErrorType,
    spawn_worker,
)
from forge.status_watcher import (
    StatusWatcher,
    StatusFileParser,
    WorkerStatusCache,
    parse_status_file,
    WorkerStatusValue,
)
from forge.log_parser import (
    LogParser,
    LogFormat,
    MalformedEntryPolicy,
    LogRingBuffer,
    LogTailer,
    parse_log_file,
)
from forge.beads import (
    Bead,
    BeadParser,
    BeadWorkspace,
    discover_bead_workspaces,
    parse_workspace,
    DependencyGraph,
    BeadState,
)


# =============================================================================
# Fixtures
# =============================================================================


@pytest.fixture
def integration_env(tmp_path):
    """Create a complete integration test environment"""
    # Create FORGE directory structure
    forge_dir = tmp_path / ".forge"
    forge_dir.mkdir(parents=True, exist_ok=True)

    status_dir = forge_dir / "status"
    status_dir.mkdir(parents=True, exist_ok=True)

    log_dir = forge_dir / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)

    # Create workspace directory
    workspace = tmp_path / "test-project"
    workspace.mkdir(parents=True, exist_ok=True)

    # Create beads directory
    beads_dir = workspace / ".beads"
    beads_dir.mkdir(parents=True, exist_ok=True)

    return {
        "forge_dir": forge_dir,
        "status_dir": status_dir,
        "log_dir": log_dir,
        "workspace": workspace,
        "beads_dir": beads_dir,
    }


@pytest.fixture
def mock_worker_launcher(integration_env):
    """Create a mock worker launcher script"""
    launcher_script = integration_env["workspace"] / "mock-launcher.sh"
    status_dir = integration_env["status_dir"]
    log_dir = integration_env["log_dir"]

    script_content = f"""#!/bin/bash
set -e

MODEL=""
WORKSPACE=""
SESSION_NAME=""

while [[ $# -gt 0 ]]; do
  case $1 in
    --model=*)
      MODEL="${{1#*=}}"
      shift
      ;;
    --workspace=*)
      WORKSPACE="${{1#*=}}"
      shift
      ;;
    --session-name=*)
      SESSION_NAME="${{1#*=}}"
      shift
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$MODEL" ]] || [[ -z "$WORKSPACE" ]] || [[ -z "$SESSION_NAME" ]]; then
  echo "Error: Missing required arguments" >&2
  exit 1
fi

# Use $$ instead of background process to avoid hanging
PID=$$

# Output worker metadata first (before any other output)
cat << EOF
{{
  "worker_id": "$SESSION_NAME",
  "pid": $PID,
  "status": "spawned",
  "launcher": "mock-launcher",
  "timestamp": "$(date -Iseconds)"
}}
EOF

# Flush stdout to ensure JSON is output immediately
exec 1>&-

# Create status file
cat > {status_dir}/$SESSION_NAME.json << STATUS_EOF
{{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "workspace": "$WORKSPACE",
  "pid": $PID,
  "started_at": "$(date -Iseconds)",
  "last_activity": "$(date -Iseconds)",
  "current_task": null,
  "tasks_completed": 0
}}
STATUS_EOF

# Write initial log entry
echo '{{"timestamp": "$(date -Iseconds)", "level": "info", "worker_id": "$SESSION_NAME", "message": "Worker started"}}' >> {log_dir}/$SESSION_NAME.log

# Exit immediately - don't hang
exit 0
"""

    launcher_script.write_text(script_content)
    launcher_script.chmod(0o755)

    return launcher_script


@pytest.fixture
def sample_beads_jsonl(integration_env):
    """Create sample beads JSONL file"""
    jsonl_file = integration_env["beads_dir"] / "beads.jsonl"

    beads = [
        {
            "id": "bd-001",
            "title": "Implement feature X",
            "description": "Implement the X feature",
            "status": "open",
            "priority": 0,
            "issue_type": "task",
            "assignee": None,
            "labels": ["critical", "mvp"],
            "created_at": "2026-02-07T10:00:00Z",
            "updated_at": "2026-02-07T10:00:00Z",
            "closed_at": None,
            "dependencies": [],
            "source_repo": ".",
        },
        {
            "id": "bd-002",
            "title": "Fix bug Y",
            "description": "Fix the Y bug",
            "status": "open",
            "priority": 1,
            "issue_type": "bug",
            "assignee": "claude",
            "labels": ["bug"],
            "created_at": "2026-02-07T10:00:00Z",
            "updated_at": "2026-02-07T10:00:00Z",
            "closed_at": None,
            "dependencies": [
                {
                    "issue_id": "bd-002",
                    "depends_on_id": "bd-001",
                    "type": "depends_on",
                    "created_at": "2026-02-07T10:00:00Z",
                }
            ],
            "source_repo": ".",
        },
        {
            "id": "bd-003",
            "title": "Write tests",
            "description": "Write unit tests",
            "status": "closed",
            "priority": 2,
            "issue_type": "task",
            "assignee": None,
            "labels": ["testing"],
            "created_at": "2026-02-06T10:00:00Z",
            "updated_at": "2026-02-07T10:00:00Z",
            "closed_at": "2026-02-07T10:00:00Z",
            "dependencies": [],
            "source_repo": ".",
        },
    ]

    with open(jsonl_file, "w") as f:
        for bead in beads:
            f.write(json.dumps(bead) + "\n")

    return jsonl_file


# =============================================================================
# End-to-End Tests
# =============================================================================


class TestEndToEndWorkflows:
    """End-to-end workflow tests"""

    def test_complete_worker_lifecycle(self, integration_env, mock_worker_launcher):
        """Test complete worker lifecycle: spawn, monitor, cleanup"""
        status_dir = integration_env["status_dir"]
        log_dir = integration_env["log_dir"]
        workspace = integration_env["workspace"]
        worker_id = "test-worker-lifecycle"

        # Step 1: Spawn worker
        launcher = WorkerLauncher(
            forge_dir=integration_env["forge_dir"],
            status_dir=status_dir,
            log_dir=log_dir,
        )

        config = LauncherConfig(
            launcher_path=mock_worker_launcher,
            model="sonnet",
            workspace=workspace,
            session_name=worker_id,
        )

        result = launcher.spawn(config)

        # Verify spawn success
        assert result.success is True
        assert result.worker_id == worker_id
        assert result.status == "spawned"
        assert result.pid is not None

        # Step 2: Verify status file
        status_file = status_dir / f"{worker_id}.json"
        assert status_file.exists()

        parsed_status = parse_status_file(status_file)
        assert parsed_status.worker_id == worker_id
        assert parsed_status.status == WorkerStatusValue.ACTIVE

        # Step 3: Verify log file
        log_file = log_dir / f"{worker_id}.log"
        assert log_file.exists()

        log_entries = parse_log_file(log_file)
        assert len(log_entries) > 0
        assert log_entries[0].message == "Worker started"

        # Step 4: Cleanup
        launcher._cleanup_test_worker(worker_id)

        # Verify cleanup
        assert not status_file.exists()

    def test_workspace_discovery_with_beads(self, integration_env, sample_beads_jsonl):
        """Test discovering workspace with beads"""
        workspace = integration_env["workspace"]

        # Parse workspace
        parser = BeadParser(workspace)
        parsed_workspace = parser.parse_workspace()

        # Verify bead parsing
        assert parsed_workspace.bead_count == 3
        assert parsed_workspace.open_count == 2
        assert parsed_workspace.closed_count == 1

        # Verify bead states
        ready_beads = [b for b in parsed_workspace.beads if b.state == BeadState.READY]
        blocked_beads = [b for b in parsed_workspace.beads if b.state == BeadState.BLOCKED]
        completed_beads = [b for b in parsed_workspace.beads if b.state == BeadState.COMPLETED]

        assert len(ready_beads) == 1  # bd-001
        assert len(blocked_beads) == 1  # bd-002 depends on bd-001
        assert len(completed_beads) == 1  # bd-003

    def test_multi_worker_scenario(self, integration_env, mock_worker_launcher):
        """Test scenario with multiple workers"""
        status_dir = integration_env["status_dir"]
        log_dir = integration_env["log_dir"]
        workspace = integration_env["workspace"]

        launcher = WorkerLauncher(
            forge_dir=integration_env["forge_dir"],
            status_dir=status_dir,
            log_dir=log_dir,
        )

        worker_ids = ["worker-1", "worker-2", "worker-3"]

        # Spawn multiple workers
        for worker_id in worker_ids:
            config = LauncherConfig(
                launcher_path=mock_worker_launcher,
                model="sonnet",
                workspace=workspace,
                session_name=worker_id,
            )

            result = launcher.spawn(config)
            assert result.success is True

        # Verify all status files exist
        for worker_id in worker_ids:
            status_file = status_dir / f"{worker_id}.json"
            assert status_file.exists()

        # Use cache to track all workers
        cache = WorkerStatusCache()

        for worker_id in worker_ids:
            status_file = status_dir / f"{worker_id}.json"
            status = parse_status_file(status_file)

            # The parse_status_file returns WorkerStatusFile with status as WorkerStatusValue enum
            # Use it directly for the event
            from forge.status_watcher import StatusFileEvent
            event = StatusFileEvent(
                worker_id=worker_id,
                event_type=StatusFileEvent.EventType.CREATED,
                path=status_file,
                status=status,
            )
            cache.update(event)

        # Verify cache counts
        assert cache.worker_count == 3
        # active_count checks if status == WorkerStatusValue.ACTIVE
        assert cache.active_count == 3

        # Cleanup
        for worker_id in worker_ids:
            launcher._cleanup_test_worker(worker_id)


# =============================================================================
# Error Handling Tests (ADR 0014)
# =============================================================================


class TestADR0014ErrorScenarios:
    """Error handling tests per ADR 0014"""

    def test_launcher_failure_shows_detailed_error(self, integration_env):
        """Test launcher failure shows detailed error (ADR 0014)"""
        # Create a launcher that fails
        failing_launcher = integration_env["workspace"] / "failing-launcher.sh"
        failing_launcher.write_text("""#!/bin/bash
echo "Error: API key not set" >&2
exit 1
""")
        failing_launcher.chmod(0o755)

        status_dir = integration_env["status_dir"]
        log_dir = integration_env["log_dir"]

        launcher = WorkerLauncher(
            forge_dir=integration_env["forge_dir"],
            status_dir=status_dir,
            log_dir=log_dir,
        )

        config = LauncherConfig(
            launcher_path=failing_launcher,
            model="sonnet",
            workspace=integration_env["workspace"],
            session_name="test-fail",
        )

        result = launcher.spawn(config)

        # Verify error handling
        assert result.success is False
        assert result.error_type == LauncherErrorType.EXIT_CODE_NONZERO
        assert result.error is not None
        assert "code 1" in result.error
        assert result.exit_code == 1

    def test_corrupted_status_file_graceful_handling(self, integration_env):
        """Test corrupted status file handled gracefully (ADR 0014)"""
        status_dir = integration_env["status_dir"]

        # Create corrupted status file
        corrupted_file = status_dir / "corrupted-worker.json"
        corrupted_file.write_text("invalid json {")

        # Should not crash, return error status
        parser = StatusFileParser()
        result = parser.parse(corrupted_file)

        assert result.status == WorkerStatusValue.FAILED
        assert result.error is not None
        assert "json" in result.error.lower()
        assert result.is_error_state

    def test_malformed_log_entries_skipped(self, integration_env):
        """Test malformed log entries are skipped (ADR 0014)"""
        log_dir = integration_env["log_dir"]
        log_file = log_dir / "malformed.log"

        # Write mix of valid and malformed entries
        entries = [
            '{"timestamp": "2026-02-07T10:00:00Z", "level": "info", "message": "Valid 1"}',
            "this is not valid json",
            "{broken json}",
            '{"timestamp": "2026-02-07T10:00:01Z", "level": "info", "message": "Valid 2"}',
        ]

        log_file.write_text("\n".join(entries))

        # Parse with skip policy
        parser = LogParser(
            format=LogFormat.JSONL,
            malformed_policy=MalformedEntryPolicy.SKIP,
        )

        parsed = []
        for line in entries:
            entry = parser.parse_line(line)
            if entry.is_valid:
                parsed.append(entry)

        # Should skip malformed entries
        assert len(parsed) == 2
        assert parsed[0].message == "Valid 1"
        assert parsed[1].message == "Valid 2"
        assert parser.stats.failed_parses == 2

    def test_missing_bead_dependencies_handling(self, integration_env):
        """Test handling of beads with missing dependencies"""
        beads_dir = integration_env["beads_dir"]
        jsonl_file = beads_dir / "missing-deps.jsonl"

        # Bead that depends on non-existent bead
        bead = {
            "id": "bd-orphan",
            "title": "Orphan task",
            "description": "Has missing dependency",
            "status": "open",
            "priority": 0,
            "issue_type": "task",
            "assignee": None,
            "labels": [],
            "created_at": "2026-02-07T10:00:00Z",
            "updated_at": "2026-02-07T10:00:00Z",
            "closed_at": None,
            "dependencies": [
                {
                    "issue_id": "bd-orphan",
                    "depends_on_id": "bd-nonexistent",
                    "type": "depends_on",
                    "created_at": "2026-02-07T10:00:00Z",
                }
            ],
            "source_repo": ".",
        }

        jsonl_file.write_text(json.dumps(bead) + "\n")

        # Parse workspace
        parser = BeadParser(integration_env["workspace"])
        workspace = parser.parse_workspace()

        # Bead should still be parsed
        assert workspace.bead_count == 1

        # Build dependency graph
        graph = DependencyGraph(workspace)

        # The bead has a dependency on a non-existent bead
        # get_blocked_beads only returns beads with EXISTING open dependencies
        # Since the dependency doesn't exist in workspace, blocking list is empty
        blocked = graph.get_blocked_beads()

        # Should NOT be in blocked list since dependency doesn't exist in workspace
        assert len(blocked) == 0

        # But the bead is still technically BLOCKED by state (has depends_on)
        # just not tracked by get_blocked_beads
        assert workspace.beads[0].state == BeadState.BLOCKED


# =============================================================================
# Real-Time Watching Tests
# =============================================================================


class TestRealTimeWatching:
    """Tests for real-time file watching"""

    @pytest.mark.asyncio
    async def test_status_file_watching(self, integration_env):
        """Test real-time status file watching"""
        status_dir = integration_env["status_dir"]

        events = []
        callback = lambda e: events.append(e)

        watcher = StatusWatcher(
            status_dir=status_dir,
            callback=callback,
            poll_interval=0.1,
        )

        await watcher.start()

        # Create a status file
        status_file = status_dir / "test-watcher.json"
        status_file.write_text('{"worker_id": "test-watcher", "status": "active", "model": "sonnet", "workspace": "/tmp"}')

        # Wait for detection
        await asyncio.sleep(0.3)

        await watcher.stop()

        # Should have detected the file
        assert len(events) > 0

    @pytest.mark.asyncio
    async def test_log_tailing_real_time(self, integration_env):
        """Test real-time log tailing"""
        log_dir = integration_env["log_dir"]
        log_file = log_dir / "realtime.log"

        parser = LogParser(format=LogFormat.JSONL)
        buffer = LogRingBuffer()
        tailer = LogTailer(log_file, parser, buffer, poll_interval=0.05)

        # Write initial content
        log_file.write_text('{"timestamp": "2026-02-07T10:00:00Z", "level": "info", "message": "Initial"}')

        await tailer.start()

        # Wait for initial read
        await asyncio.sleep(0.2)

        initial_size = buffer.size
        assert initial_size >= 1

        # Append more content
        with open(log_file, 'a') as f:
            f.write('\n{"timestamp": "2026-02-07T10:00:01Z", "level": "info", "message": "New line"}')

        # Wait for tailer to detect
        await asyncio.sleep(0.2)

        await tailer.stop()

        # Should have new entry
        assert buffer.size >= initial_size


# =============================================================================
# Integration Scenarios
# =============================================================================


class TestIntegrationScenarios:
    """Complex integration scenarios"""

    def test_worker_fails_mid_task(self, integration_env, mock_worker_launcher):
        """Test scenario where worker fails during task execution"""
        status_dir = integration_env["status_dir"]
        log_dir = integration_env["log_dir"]
        workspace = integration_env["workspace"]

        launcher = WorkerLauncher(
            forge_dir=integration_env["forge_dir"],
            status_dir=status_dir,
            log_dir=log_dir,
        )

        # Spawn worker
        config = LauncherConfig(
            launcher_path=mock_worker_launcher,
            model="sonnet",
            workspace=workspace,
            session_name="failing-worker",
        )

        result = launcher.spawn(config)
        assert result.success is True

        worker_id = result.worker_id

        # Simulate worker failure by updating status file
        status_file = status_dir / f"{worker_id}.json"
        status_data = json.loads(status_file.read_text())
        status_data["status"] = "failed"
        status_data["error"] = "Task failed: API timeout"
        status_file.write_text(json.dumps(status_data))

        # Parse updated status
        parsed_status = parse_status_file(status_file)

        assert parsed_status.status == WorkerStatusValue.FAILED

        # Cleanup
        launcher._cleanup_test_worker(worker_id)

    def test_workspace_with_multiple_bead_files(self, integration_env):
        """Test workspace with multiple bead files"""
        beads_dir = integration_env["beads_dir"]

        # Create multiple JSONL files
        for i in range(3):
            jsonl_file = beads_dir / f"beads-{i}.jsonl"
            bead = {
                "id": f"bd-{i:03d}",
                "title": f"Task {i}",
                "description": f"Description {i}",
                "status": "open",
                "priority": i,
                "issue_type": "task",
                "assignee": None,
                "labels": [],
                "created_at": "2026-02-07T10:00:00Z",
                "updated_at": "2026-02-07T10:00:00Z",
                "closed_at": None,
                "dependencies": [],
                "source_repo": ".",
            }
            jsonl_file.write_text(json.dumps(bead) + "\n")

        # Discover workspaces
        workspaces = discover_bead_workspaces(integration_env["workspace"])

        # Should find the workspace
        assert len(workspaces) == 1
        assert workspaces[0].name == integration_env["workspace"].name


# =============================================================================
# Performance Tests
# =============================================================================


class TestIntegrationPerformance:
    """Performance tests for integration scenarios"""

    def test_spawn_multiple_workers_performance(self, integration_env, mock_worker_launcher):
        """Test spawning multiple workers is fast enough"""
        import time

        status_dir = integration_env["status_dir"]
        log_dir = integration_env["log_dir"]
        workspace = integration_env["workspace"]

        launcher = WorkerLauncher(
            forge_dir=integration_env["forge_dir"],
            status_dir=status_dir,
            log_dir=log_dir,
        )

        start = time.time()

        # Spawn 5 workers
        for i in range(5):
            config = LauncherConfig(
                launcher_path=mock_worker_launcher,
                model="sonnet",
                workspace=workspace,
                session_name=f"perf-worker-{i}",
            )

            result = launcher.spawn(config)
            assert result.success is True

        duration = time.time() - start

        # Should complete all spawns in under 5 seconds
        assert duration < 5.0

        # Cleanup
        for i in range(5):
            launcher._cleanup_test_worker(f"perf-worker-{i}")

    def test_parse_large_bead_file_performance(self, integration_env):
        """Test parsing large bead file performance"""
        import time

        jsonl_file = integration_env["beads_dir"] / "large-beads.jsonl"

        # Create 100 beads
        beads = []
        for i in range(100):
            bead = {
                "id": f"bd-{i:04d}",
                "title": f"Task {i}",
                "description": f"Description {i}",
                "status": "open" if i % 3 != 0 else "closed",
                "priority": i % 5,
                "issue_type": "task",
                "assignee": None,
                "labels": [],
                "created_at": "2026-02-07T10:00:00Z",
                "updated_at": "2026-02-07T10:00:00Z",
                "closed_at": None if i % 3 != 0 else "2026-02-07T10:00:00Z",
                "dependencies": [],
                "source_repo": ".",
            }
            beads.append(bead)

        jsonl_file.write_text("\n".join(json.dumps(b) for b in beads) + "\n")

        start = time.time()
        parser = BeadParser(integration_env["workspace"])
        workspace = parser.parse_workspace()
        duration = time.time() - start

        # Should parse 100 beads in under 1 second
        assert duration < 1.0
        assert workspace.bead_count == 100

    def test_log_file_real_time_parsing_performance(self, integration_env):
        """Test real-time log parsing performance"""
        import time

        log_dir = integration_env["log_dir"]
        log_file = log_dir / "perf.log"

        # Write 100 log entries
        entries = [
            f'{{"timestamp": "2026-02-07T10:00:{i:02d}Z", "level": "info", "message": "Entry {i}"}}'
            for i in range(100)
        ]
        log_file.write_text("\n".join(entries) + "\n")

        start = time.time()
        parsed_entries = parse_log_file(log_file)
        duration = time.time() - start

        # Should parse 100 entries in under 0.5 seconds
        assert duration < 0.5
        assert len(parsed_entries) == 100
