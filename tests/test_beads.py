"""
Tests for FORGE Bead Integration Module

Comprehensive tests for bead JSONL parsing, workspace discovery,
task value scoring, dependency graph, and file watching.
"""

import asyncio
import json
from datetime import datetime, timezone, timedelta
from pathlib import Path
from unittest.mock import AsyncMock, Mock, patch
from typing import Any

import pytest

from forge.beads import (
    Bead,
    BeadDependency,
    BeadParser,
    BeadPriority,
    BeadState,
    BeadStatus,
    BeadType,
    BeadWatcher,
    BeadWorkspace,
    DependencyGraph,
    discover_bead_workspaces,
    parse_workspace,
    calculate_value_score,
    build_dependency_graph,
)


# =============================================================================
# Fixtures
# =============================================================================


@pytest.fixture
def temp_workspace(tmp_path):
    """Create a temporary workspace with .beads directory"""
    workspace = tmp_path / "test-project"
    workspace.mkdir(parents=True, exist_ok=True)
    beads_dir = workspace / ".beads"
    beads_dir.mkdir(parents=True, exist_ok=True)
    return workspace


@pytest.fixture
def sample_bead_data():
    """Sample bead data from JSONL"""
    return {
        "id": "bd-abc",
        "title": "Implement feature X",
        "description": "Detailed description of feature X",
        "status": "open",
        "priority": 0,
        "issue_type": "task",
        "assignee": None,
        "labels": ["critical", "mvp"],
        "created_at": "2026-02-07T10:00:00Z",
        "updated_at": "2026-02-07T10:00:00Z",
        "closed_at": None,
        "dependencies": [
            {
                "issue_id": "bd-abc",
                "depends_on_id": "bd-def",
                "type": "depends_on",
                "created_at": "2026-02-07T10:00:00Z",
            }
        ],
        "source_repo": ".",
    }


@pytest.fixture
def valid_jsonl_file(temp_workspace, sample_bead_data):
    """Create a valid JSONL file with beads"""
    jsonl_file = temp_workspace / ".beads" / "beads.jsonl"
    with open(jsonl_file, "w") as f:
        f.write(json.dumps(sample_bead_data) + "\n")
        # Add another bead
        bead2 = sample_bead_data.copy()
        bead2["id"] = "bd-def"
        bead2["title"] = "Implement feature Y"
        bead2["dependencies"] = []
        f.write(json.dumps(bead2) + "\n")
    return jsonl_file


@pytest.fixture
def corrupted_jsonl_file(temp_workspace):
    """Create a corrupted JSONL file"""
    jsonl_file = temp_workspace / ".beads" / "corrupted.jsonl"
    jsonl_file.write_text("invalid json {")
    return jsonl_file


@pytest.fixture
def multi_workspace_setup(tmp_path):
    """Create multiple workspaces for discovery testing"""
    root = tmp_path / "projects"
    root.mkdir(parents=True, exist_ok=True)

    # Root workspace
    root_beads = root / ".beads"
    root_beads.mkdir(parents=True, exist_ok=True)
    root_jsonl = root_beads / "beads.jsonl"
    root_jsonl.write_text('{"id": "root-1", "title": "Root task", "status": "open", "priority": 0}\n')

    # Subdirectory workspace
    subdir = root / "subproject"
    subdir.mkdir(parents=True, exist_ok=True)
    sub_beads = subdir / ".beads"
    sub_beads.mkdir(parents=True, exist_ok=True)
    sub_jsonl = sub_beads / "beads.jsonl"
    sub_jsonl.write_text('{"id": "sub-1", "title": "Sub task", "status": "open", "priority": 1}\n')

    # Directory without beads
    no_beads = root / "no-beads"
    no_beads.mkdir(parents=True, exist_ok=True)

    return root


# =============================================================================
# Bead Data Model Tests
# =============================================================================


class TestBead:
    """Tests for Bead data model"""

    def test_bead_properties(self, sample_bead_data):
        """Test bead property accessors"""
        bead = Bead(
            id=sample_bead_data["id"],
            title=sample_bead_data["title"],
            type=sample_bead_data["issue_type"],
            status=sample_bead_data["status"],
            priority=sample_bead_data["priority"],
            description=sample_bead_data["description"],
            assignee=sample_bead_data["assignee"],
            labels=sample_bead_data["labels"],
            created_at=sample_bead_data["created_at"],
            updated_at=sample_bead_data["updated_at"],
            closed_at=sample_bead_data["closed_at"],
            dependencies=[
                BeadDependency(
                    issue_id=d["issue_id"],
                    depends_on_id=d["depends_on_id"],
                    type=d["type"],
                )
                for d in sample_bead_data["dependencies"]
            ],
            source_repo=sample_bead_data["source_repo"],
            workspace=Path("/test"),
        )

        assert bead.id == "bd-abc"
        assert bead.status_enum == BeadStatus.OPEN
        assert bead.priority_enum == BeadPriority.P0
        assert bead.type_enum == BeadType.TASK
        assert bead.depends_on == ["bd-def"]
        assert bead.blocks == []

    def test_bead_state_ready(self):
        """Test bead state when ready"""
        bead = Bead(
            id="bd-ready",
            title="Ready task",
            type="task",
            status="open",
            priority=0,
            description="No dependencies",
            assignee=None,
            labels=[],
            created_at=datetime.now(timezone.utc).isoformat(),
            updated_at=datetime.now(timezone.utc).isoformat(),
            closed_at=None,
            dependencies=[],
            source_repo=".",
            workspace=Path("/test"),
        )

        assert bead.state == BeadState.READY

    def test_bead_state_blocked(self):
        """Test bead state when blocked"""
        bead = Bead(
            id="bd-blocked",
            title="Blocked task",
            type="task",
            status="open",
            priority=0,
            description="Has dependencies",
            assignee=None,
            labels=[],
            created_at=datetime.now(timezone.utc).isoformat(),
            updated_at=datetime.now(timezone.utc).isoformat(),
            closed_at=None,
            dependencies=[
                BeadDependency(
                    issue_id="bd-blocked",
                    depends_on_id="bd-other",
                    type="depends_on",
                )
            ],
            source_repo=".",
            workspace=Path("/test"),
        )

        assert bead.state == BeadState.BLOCKED

    def test_bead_state_completed(self):
        """Test bead state when completed"""
        bead = Bead(
            id="bd-done",
            title="Done task",
            type="task",
            status="closed",
            priority=0,
            description="Closed",
            assignee=None,
            labels=[],
            created_at=datetime.now(timezone.utc).isoformat(),
            updated_at=datetime.now(timezone.utc).isoformat(),
            closed_at=datetime.now(timezone.utc).isoformat(),
            dependencies=[],
            source_repo=".",
            workspace=Path("/test"),
        )

        assert bead.state == BeadState.COMPLETED

    def test_value_score_p0_critical(self):
        """Test value score for P0 critical task"""
        bead = Bead(
            id="bd-p0",
            title="Critical task",
            type="task",
            status="open",
            priority=0,  # P0 = 40 points
            description="Critical",
            assignee=None,
            labels=["critical"],
            created_at=(datetime.now(timezone.utc) - timedelta(days=5)).isoformat(),
            updated_at=datetime.now(timezone.utc).isoformat(),
            closed_at=None,
            dependencies=[
                BeadDependency(
                    issue_id="bd-p0",
                    depends_on_id="bd-blocked-1",
                    type="blocks",
                ),
                BeadDependency(
                    issue_id="bd-p0",
                    depends_on_id="bd-blocked-2",
                    type="blocks",
                ),
                BeadDependency(
                    issue_id="bd-p0",
                    depends_on_id="bd-blocked-3",
                    type="blocks",
                ),
            ],
            source_repo=".",
            workspace=Path("/test"),
        )

        # P0(40) + blocks 3(30) + 5 days old(15) + critical label(10) = 95
        score = bead.value_score
        assert 90 <= score <= 100

    def test_value_score_p2_normal(self):
        """Test value score for P2 normal task"""
        bead = Bead(
            id="bd-p2",
            title="Normal task",
            type="task",
            status="open",
            priority=2,  # P2 = 20 points
            description="Normal",
            assignee=None,
            labels=[],
            created_at=datetime.now(timezone.utc).isoformat(),
            updated_at=datetime.now(timezone.utc).isoformat(),
            closed_at=None,
            dependencies=[],
            source_repo=".",
            workspace=Path("/test"),
        )

        # P2(20) + no blocks(0) + new(0) + no labels(0) = 20
        assert bead.value_score == 20

    def test_bead_to_dict(self, sample_bead_data):
        """Test converting bead to dictionary"""
        bead = Bead(
            id=sample_bead_data["id"],
            title=sample_bead_data["title"],
            type=sample_bead_data["issue_type"],
            status=sample_bead_data["status"],
            priority=sample_bead_data["priority"],
            description=sample_bead_data["description"],
            assignee=sample_bead_data["assignee"],
            labels=sample_bead_data["labels"],
            created_at=sample_bead_data["created_at"],
            updated_at=sample_bead_data["updated_at"],
            closed_at=sample_bead_data["closed_at"],
            dependencies=[
                BeadDependency(
                    issue_id=d["issue_id"],
                    depends_on_id=d["depends_on_id"],
                    type=d["type"],
                )
                for d in sample_bead_data["dependencies"]
            ],
            source_repo=sample_bead_data["source_repo"],
            workspace=Path("/test"),
        )

        result = bead.to_dict()
        assert result["id"] == "bd-abc"
        assert result["title"] == "Implement feature X"
        assert "state" in result
        assert "value_score" in result


# =============================================================================
# BeadParser Tests
# =============================================================================


class TestBeadParser:
    """Tests for BeadParser"""

    def test_parse_valid_jsonl(self, valid_jsonl_file, temp_workspace):
        """Test parsing a valid JSONL file"""
        parser = BeadParser(temp_workspace)
        beads = parser.parse_jsonl_file(valid_jsonl_file)

        assert len(beads) == 2
        assert beads[0].id == "bd-abc"
        assert beads[1].id == "bd-def"
        assert parser.parse_errors == 0

    def test_parse_corrupted_jsonl(self, corrupted_jsonl_file, temp_workspace):
        """Test parsing a corrupted JSONL file"""
        parser = BeadParser(temp_workspace)
        beads = parser.parse_jsonl_file(corrupted_jsonl_file)

        assert len(beads) == 0
        assert parser.parse_errors > 0
        assert parser.last_error is not None

    def test_parse_nonexistent_file(self, temp_workspace):
        """Test parsing a non-existent file"""
        parser = BeadParser(temp_workspace)
        beads = parser.parse_jsonl_file(temp_workspace / ".beads" / "nonexistent.jsonl")

        assert len(beads) == 0

    def test_parse_workspace(self, valid_jsonl_file, temp_workspace):
        """Test parsing entire workspace"""
        parser = BeadParser(temp_workspace)
        workspace = parser.parse_workspace()

        assert workspace.path == temp_workspace
        assert workspace.bead_count == 2

    def test_parse_empty_workspace(self, temp_workspace):
        """Test parsing workspace with no beads"""
        parser = BeadParser(temp_workspace)
        workspace = parser.parse_workspace()

        assert workspace.bead_count == 0


# =============================================================================
# BeadWorkspace Tests
# =============================================================================


class TestBeadWorkspace:
    """Tests for BeadWorkspace"""

    def test_workspace_properties(self, valid_jsonl_file, temp_workspace):
        """Test workspace property calculations"""
        parser = BeadParser(temp_workspace)
        workspace = parser.parse_workspace()

        assert workspace.bead_count == 2
        assert workspace.open_count == 2
        assert workspace.in_progress_count == 0
        assert workspace.closed_count == 0

    def test_workspace_name(self, temp_workspace):
        """Test workspace name property"""
        parser = BeadParser(temp_workspace)
        workspace = parser.parse_workspace()

        assert workspace.name == temp_workspace.name

    def test_get_bead_by_id(self, valid_jsonl_file, temp_workspace):
        """Test finding bead by ID"""
        parser = BeadParser(temp_workspace)
        workspace = parser.parse_workspace()

        bead = workspace.get_bead_by_id("bd-abc")
        assert bead is not None
        assert bead.id == "bd-abc"

        not_found = workspace.get_bead_by_id("bd-xyz")
        assert not_found is None


# =============================================================================
# Workspace Discovery Tests
# =============================================================================


class TestWorkspaceDiscovery:
    """Tests for workspace discovery"""

    def test_discover_workspaces(self, multi_workspace_setup):
        """Test discovering multiple workspaces"""
        workspaces = discover_bead_workspaces(multi_workspace_setup)

        # Should find root workspace and subdirectory workspace
        assert len(workspaces) == 2

        workspace_names = {w.name for w in workspaces}
        assert "projects" in workspace_names  # Root
        assert "subproject" in workspace_names  # Subdirectory

    def test_discover_empty_directory(self, tmp_path):
        """Test discovering workspaces in empty directory"""
        empty_dir = tmp_path / "empty"
        empty_dir.mkdir(parents=True, exist_ok=True)

        workspaces = discover_bead_workspaces(empty_dir)
        assert len(workspaces) == 0


# =============================================================================
# DependencyGraph Tests
# =============================================================================


class TestDependencyGraph:
    """Tests for DependencyGraph"""

    @pytest.fixture
    def complex_workspace(self, temp_workspace):
        """Create a workspace with complex dependencies"""
        jsonl_file = temp_workspace / ".beads" / "beads.jsonl"

        beads = [
            {"id": "bd-1", "title": "Task 1", "status": "open", "priority": 0, "dependencies": []},
            {"id": "bd-2", "title": "Task 2", "status": "open", "priority": 1,
             "dependencies": [{"issue_id": "bd-2", "depends_on_id": "bd-1", "type": "depends_on"}]},
            {"id": "bd-3", "title": "Task 3", "status": "open", "priority": 2,
             "dependencies": [{"issue_id": "bd-3", "depends_on_id": "bd-2", "type": "depends_on"}]},
            {"id": "bd-4", "title": "Task 4", "status": "closed", "priority": 0, "dependencies": []},
        ]

        with open(jsonl_file, "w") as f:
            for bead in beads:
                f.write(json.dumps(bead) + "\n")

        parser = BeadParser(temp_workspace)
        return parser.parse_workspace()

    def test_get_ready_beads(self, complex_workspace):
        """Test getting ready beads"""
        graph = DependencyGraph(complex_workspace)
        ready = graph.get_ready_beads()

        # Only bd-1 should be ready (no dependencies, open)
        assert len(ready) == 1
        assert ready[0].id == "bd-1"

    def test_get_blocked_beads(self, complex_workspace):
        """Test getting blocked beads"""
        graph = DependencyGraph(complex_workspace)
        blocked = graph.get_blocked_beads()

        # bd-2 is blocked by bd-1, bd-3 is blocked by bd-2
        assert len(blocked) == 2

    def test_detect_cycles(self, temp_workspace):
        """Test cycle detection"""
        jsonl_file = temp_workspace / ".beads" / "beads.jsonl"

        # Create circular dependency: bd-1 -> bd-2 -> bd-3 -> bd-1
        beads = [
            {"id": "bd-1", "title": "Task 1", "status": "open", "priority": 0,
             "dependencies": [{"issue_id": "bd-1", "depends_on_id": "bd-3", "type": "depends_on"}]},
            {"id": "bd-2", "title": "Task 2", "status": "open", "priority": 1,
             "dependencies": [{"issue_id": "bd-2", "depends_on_id": "bd-1", "type": "depends_on"}]},
            {"id": "bd-3", "title": "Task 3", "status": "open", "priority": 2,
             "dependencies": [{"issue_id": "bd-3", "depends_on_id": "bd-2", "type": "depends_on"}]},
        ]

        with open(jsonl_file, "w") as f:
            for bead in beads:
                f.write(json.dumps(bead) + "\n")

        parser = BeadParser(temp_workspace)
        workspace = parser.parse_workspace()
        graph = DependencyGraph(workspace)

        cycles = graph.detect_cycles()
        assert len(cycles) > 0
        assert len(cycles[0]) == 4  # bd-1 -> bd-2 -> bd-3 -> bd-1


# =============================================================================
# BeadWatcher Tests
# =============================================================================


class TestBeadWatcher:
    """Tests for BeadWatcher"""

    @pytest.mark.asyncio
    async def test_watcher_starts_and_stops(self, temp_workspace):
        """Test watcher lifecycle"""
        callback = Mock()
        watcher = BeadWatcher(
            workspaces=[temp_workspace],
            callback=callback,
        )

        watcher_type = await watcher.start()
        assert watcher_type in ["inotify", "polling"]
        assert watcher.is_running

        await watcher.stop()
        assert not watcher.is_running

    @pytest.mark.asyncio
    async def test_watcher_polling_mode(self, temp_workspace, monkeypatch):
        """Test watcher falls back to polling"""
        # Force polling by disabling watchdog
        import forge.beads
        monkeypatch.setattr(forge.beads, "WATCHDOG_AVAILABLE", False)

        callback = Mock()
        watcher = BeadWatcher(
            workspaces=[temp_workspace],
            callback=callback,
            poll_interval=0.1,
        )

        watcher_type = await watcher.start()
        assert watcher_type == "polling"
        assert watcher.is_using_polling

        await watcher.stop()


# =============================================================================
# Convenience Function Tests
# =============================================================================


class TestConvenienceFunctions:
    """Tests for convenience functions"""

    def test_parse_workspace_function(self, valid_jsonl_file, temp_workspace):
        """Test parse_workspace convenience function"""
        workspace = parse_workspace(temp_workspace)

        assert workspace.path == temp_workspace
        assert workspace.bead_count == 2

    def test_calculate_value_score_function(self, sample_bead_data):
        """Test calculate_value_score convenience function"""
        bead = Bead(
            id=sample_bead_data["id"],
            title=sample_bead_data["title"],
            type=sample_bead_data["issue_type"],
            status=sample_bead_data["status"],
            priority=sample_bead_data["priority"],
            description=sample_bead_data["description"],
            assignee=sample_bead_data["assignee"],
            labels=sample_bead_data["labels"],
            created_at=sample_bead_data["created_at"],
            updated_at=sample_bead_data["updated_at"],
            closed_at=sample_bead_data["closed_at"],
            dependencies=[],
            source_repo=sample_bead_data["source_repo"],
            workspace=Path("/test"),
        )

        score = calculate_value_score(bead)
        assert isinstance(score, int)
        assert 0 <= score <= 100

    def test_build_dependency_graph_function(self, valid_jsonl_file, temp_workspace):
        """Test build_dependency_graph convenience function"""
        parser = BeadParser(temp_workspace)
        workspace = parser.parse_workspace()

        graph = build_dependency_graph(workspace)
        assert isinstance(graph, DependencyGraph)
        assert len(graph.nodes) == 2
