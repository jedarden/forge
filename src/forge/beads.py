"""
FORGE Bead Integration Module

Implements bead/task tracking via .beads/*.jsonl files using orjson for fast parsing.
Provides task value scoring, dependency graph building, and workspace discovery.

Reference: docs/adr/0007-bead-integration-strategy.md
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from datetime import datetime, timezone
from enum import Enum
from pathlib import Path
from typing import Any, Callable

import orjson

# Try to import watchdog for inotify support
try:
    from watchdog.observers import Observer
    from watchdog.events import FileSystemEventHandler
    WATCHDOG_AVAILABLE = True
except ImportError:
    WATCHDOG_AVAILABLE = False
    FileSystemEventHandler = object  # type: ignore


# =============================================================================
# Bead Data Models
# =============================================================================


class BeadStatus(Enum):
    """Valid bead status values from JSONL files"""
    OPEN = "open"
    IN_PROGRESS = "in_progress"
    CLOSED = "closed"


class BeadPriority(Enum):
    """Bead priority levels"""
    P0 = "0"  # Critical
    P1 = "1"  # High
    P2 = "2"  # Medium
    P3 = "3"  # Low
    P4 = "4"  # Backlog


class BeadType(Enum):
    """Bead/issue types"""
    TASK = "task"
    BUG = "bug"
    FEATURE = "feature"
    EPIC = "epic"


class BeadState(Enum):
    """Bead readiness state for task execution"""
    READY = "ready"           # Open and unblocked
    BLOCKED = "blocked"       # Open but has open dependencies
    IN_PROGRESS = "in_progress"  # Currently being worked on
    COMPLETED = "completed"   # Closed


@dataclass
class BeadDependency:
    """
    Represents a dependency relationship between beads.

    Attributes:
        issue_id: The issue that has this dependency
        depends_on_id: The issue this depends on
        type: Dependency type (depends_on, blocks)
    """
    issue_id: str
    depends_on_id: str
    type: str = "depends_on"
    created_at: str | None = None


@dataclass
class Bead:
    """
    Represents a single bead/task from the br CLI system.

    Attributes:
        id: Unique bead identifier (e.g., "bd-abc", "fg-36s")
        title: Short description of the task
        type: Type of issue (task, bug, feature, epic)
        status: Current status (open, in_progress, closed)
        priority: Priority level (P0-P4)
        description: Detailed explanation of the task
        assignee: Worker ID assigned to this task (optional)
        labels: List of tags for categorization
        created_at: ISO 8601 timestamp when bead was created
        updated_at: ISO 8601 timestamp of last update
        closed_at: ISO 8601 timestamp when bead was closed (optional)
        dependencies: List of dependency relationships
        source_repo: Repository where this bead originated
        workspace: Path to workspace containing .beads/ directory
    """
    id: str
    title: str
    type: str
    status: str
    priority: int
    description: str
    assignee: str | None
    labels: list[str]
    created_at: str
    updated_at: str
    closed_at: str | None
    dependencies: list[BeadDependency]
    source_repo: str
    workspace: Path

    @property
    def status_enum(self) -> BeadStatus:
        """Get status as enum"""
        status_map = {
            "open": BeadStatus.OPEN,
            "in_progress": BeadStatus.IN_PROGRESS,
            "closed": BeadStatus.CLOSED,
        }
        return status_map.get(self.status, BeadStatus.OPEN)

    @property
    def priority_enum(self) -> BeadPriority:
        """Get priority as enum"""
        priority_map = {
            0: BeadPriority.P0,
            1: BeadPriority.P1,
            2: BeadPriority.P2,
            3: BeadPriority.P3,
            4: BeadPriority.P4,
        }
        return priority_map.get(self.priority, BeadPriority.P2)

    @property
    def type_enum(self) -> BeadType:
        """Get type as enum"""
        type_map = {
            "task": BeadType.TASK,
            "bug": BeadType.BUG,
            "feature": BeadType.FEATURE,
            "epic": BeadType.EPIC,
        }
        return type_map.get(self.type, BeadType.TASK)

    @property
    def created_at_datetime(self) -> datetime:
        """Get created_at as datetime object"""
        try:
            return datetime.fromisoformat(self.created_at.replace("Z", "+00:00"))
        except (ValueError, AttributeError):
            return datetime.now(timezone.utc)

    @property
    def depends_on(self) -> list[str]:
        """Get list of bead IDs that this bead depends on"""
        return [
            d.depends_on_id for d in self.dependencies
            if d.type == "depends_on"
        ]

    @property
    def blocks(self) -> list[str]:
        """Get list of bead IDs that this bead blocks"""
        return [
            d.depends_on_id for d in self.dependencies
            if d.type == "blocks"
        ]

    @property
    def state(self) -> BeadState:
        """
        Get the readiness state of this bead.

        Returns:
            BeadState indicating if the bead is ready, blocked, in progress, or completed
        """
        if self.status_enum == BeadStatus.CLOSED:
            return BeadState.COMPLETED
        if self.status_enum == BeadStatus.IN_PROGRESS:
            return BeadState.IN_PROGRESS

        # Check if blocked by open dependencies
        if self.depends_on:
            return BeadState.BLOCKED

        return BeadState.READY

    @property
    def value_score(self) -> int:
        """
        Calculate task value score (0-100) for intelligent routing.

        Scoring algorithm from ADR 0007:
        - Priority (40 points): P0=40, P1=30, P2=20, P3=10, P4=5
        - Blockers (30 points): 10 points per blocked task, max 30
        - Age (20 points): older tasks prioritized, max at 7 days
        - Labels (10 points): critical/urgent/blocker/hotfix tags

        Returns:
            Integer score from 0-100
        """
        score = 0

        # Priority contribution (0-40 points)
        priority_scores = {
            0: 40,  # P0
            1: 30,  # P1
            2: 20,  # P2
            3: 10,  # P3
            4: 5,   # P4
        }
        score += priority_scores.get(self.priority, 15)

        # Blocker contribution (0-30 points)
        # More tasks blocked = higher value
        blocked_count = len(self.blocks)
        score += min(blocked_count * 10, 30)

        # Age contribution (0-20 points)
        # Older than 7 days = full points, linear scale before
        age_days = (datetime.now(timezone.utc) - self.created_at_datetime).days
        score += min(age_days * 3, 20)

        # Label contribution (0-10 points)
        urgent_labels = {"critical", "urgent", "blocker", "hotfix", "mvp"}
        if any(label.lower() in urgent_labels for label in self.labels):
            score += 10

        return min(score, 100)

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization"""
        return {
            "id": self.id,
            "title": self.title,
            "type": self.type,
            "status": self.status,
            "priority": self.priority,
            "description": self.description,
            "assignee": self.assignee,
            "labels": self.labels,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
            "closed_at": self.closed_at,
            "dependencies": [
                {
                    "issue_id": d.issue_id,
                    "depends_on_id": d.depends_on_id,
                    "type": d.type,
                    "created_at": d.created_at,
                }
                for d in self.dependencies
            ],
            "source_repo": self.source_repo,
            "workspace": str(self.workspace),
            "state": self.state.value,
            "value_score": self.value_score,
        }


# =============================================================================
# Bead Workspace
# =============================================================================


@dataclass
class BeadWorkspace:
    """
    Represents a workspace containing .beads/ directory.

    Attributes:
        path: Path to the workspace directory
        beads: List of beads in this workspace
        name: Workspace name (derived from path)
    """
    path: Path
    beads: list[Bead] = field(default_factory=list)

    @property
    def name(self) -> str:
        """Get workspace name from path"""
        return self.path.name

    @property
    def bead_count(self) -> int:
        """Get total number of beads"""
        return len(self.beads)

    @property
    def open_count(self) -> int:
        """Get number of open beads"""
        return sum(1 for b in self.beads if b.status_enum == BeadStatus.OPEN)

    @property
    def in_progress_count(self) -> int:
        """Get number of in-progress beads"""
        return sum(1 for b in self.beads if b.status_enum == BeadStatus.IN_PROGRESS)

    @property
    def closed_count(self) -> int:
        """Get number of closed beads"""
        return sum(1 for b in self.beads if b.status_enum == BeadStatus.CLOSED)

    @property
    def ready_beads(self) -> list[Bead]:
        """Get list of beads ready to be worked on"""
        return [b for b in self.beads if b.state == BeadState.READY]

    @property
    def blocked_beads(self) -> list[Bead]:
        """Get list of blocked beads"""
        return [b for b in self.beads if b.state == BeadState.BLOCKED]

    def get_bead_by_id(self, bead_id: str) -> Bead | None:
        """Get bead by ID"""
        for bead in self.beads:
            if bead.id == bead_id:
                return bead
        return None

    def find_dependencies(self, bead: Bead) -> list[Bead]:
        """
        Find all beads that this bead depends on.

        Args:
            bead: The bead to find dependencies for

        Returns:
            List of beads that this bead depends on
        """
        dependencies = []
        for dep_id in bead.depends_on:
            dep_bead = self.get_bead_by_id(dep_id)
            if dep_bead:
                dependencies.append(dep_bead)
        return dependencies

    def find_blocked_by(self, bead: Bead) -> list[Bead]:
        """
        Find all beads that are blocked by this bead.

        Args:
            bead: The bead to find blocked beads for

        Returns:
            List of beads that are blocked by this bead
        """
        blocked = []
        for other_bead in self.beads:
            if bead.id in other_bead.depends_on:
                blocked.append(other_bead)
        return blocked


# =============================================================================
# JSONL Parser with orjson
# =============================================================================


class BeadParser:
    """
    Parse .beads/*.jsonl files using orjson for fast JSON parsing.

    Handles malformed entries gracefully per ADR 0014.
    """

    def __init__(self, workspace: Path):
        """
        Initialize the bead parser.

        Args:
            workspace: Path to workspace directory
        """
        self.workspace = workspace
        self.parse_errors = 0
        self.last_error = None

    def parse_jsonl_file(self, jsonl_file: Path) -> list[Bead]:
        """
        Parse a single JSONL file containing bead data.

        Args:
            jsonl_file: Path to the JSONL file

        Returns:
            List of parsed Bead objects
        """
        beads = []

        try:
            with open(jsonl_file, "rb") as f:
                for line_num, line in enumerate(f, 1):
                    if not line.strip():
                        continue

                    try:
                        data = orjson.loads(line)
                        bead = self._parse_bead_dict(data)
                        if bead:
                            beads.append(bead)
                    except orjson.JSONDecodeError as e:
                        self.parse_errors += 1
                        self.last_error = f"Line {line_num}: Invalid JSON - {str(e)[:50]}"
                        continue
                    except Exception as e:
                        self.parse_errors += 1
                        self.last_error = f"Line {line_num}: Parse error - {str(e)[:50]}"
                        continue

        except FileNotFoundError:
            # JSONL file doesn't exist yet
            return []
        except Exception as e:
            self.parse_errors += 1
            self.last_error = f"File read error: {str(e)[:50]}"
            return []

        return beads

    def _parse_bead_dict(self, data: dict[str, Any]) -> Bead | None:
        """
        Parse a bead dictionary from JSONL into a Bead object.

        Args:
            data: Dictionary from JSONL file

        Returns:
            Bead object or None if required fields are missing
        """
        # Required fields
        if "id" not in data or "title" not in data:
            return None

        # Parse dependencies
        dependencies = []
        if "dependencies" in data and isinstance(data["dependencies"], list):
            for dep_dict in data["dependencies"]:
                if isinstance(dep_dict, dict):
                    dependencies.append(BeadDependency(
                        issue_id=dep_dict.get("issue_id", ""),
                        depends_on_id=dep_dict.get("depends_on_id", ""),
                        type=dep_dict.get("type", "depends_on"),
                        created_at=dep_dict.get("created_at"),
                    ))

        # Parse labels
        labels = data.get("labels", [])
        if isinstance(labels, str):
            labels = [labels]
        elif not isinstance(labels, list):
            labels = []

        return Bead(
            id=data["id"],
            title=data["title"],
            type=data.get("issue_type", data.get("type", "task")),
            status=data.get("status", "open"),
            priority=data.get("priority", 2),
            description=data.get("description", ""),
            assignee=data.get("assignee"),
            labels=labels,
            created_at=data.get("created_at", datetime.now(timezone.utc).isoformat()),
            updated_at=data.get("updated_at", datetime.now(timezone.utc).isoformat()),
            closed_at=data.get("closed_at"),
            dependencies=dependencies,
            source_repo=data.get("source_repo", "."),
            workspace=self.workspace,
        )

    def parse_workspace(self) -> BeadWorkspace:
        """
        Parse all beads in a workspace.

        Returns:
            BeadWorkspace with all parsed beads
        """
        beads_dir = self.workspace / ".beads"

        if not beads_dir.exists():
            return BeadWorkspace(path=self.workspace, beads=[])

        all_beads = []

        # Find all JSONL files
        jsonl_files = list(beads_dir.glob("*.jsonl"))

        for jsonl_file in jsonl_files:
            beads = self.parse_jsonl_file(jsonl_file)
            all_beads.extend(beads)

        return BeadWorkspace(path=self.workspace, beads=all_beads)


# =============================================================================
# Workspace Discovery
# =============================================================================


def discover_bead_workspaces(root: Path) -> list[BeadWorkspace]:
    """
    Discover all bead workspaces in a directory tree.

    Searches for .beads/ directories in:
    1. The root directory
    2. Direct subdirectories of root

    Args:
        root: Root directory to search

    Returns:
        List of discovered BeadWorkspace objects
    """
    workspaces = []
    root = Path(root).expanduser()

    # Check root directory
    if (root / ".beads").exists():
        parser = BeadParser(root)
        workspace = parser.parse_workspace()
        workspaces.append(workspace)

    # Check subdirectories (one level deep)
    if root.is_dir():
        for subdir in root.iterdir():
            if subdir.is_dir() and (subdir / ".beads").exists():
                parser = BeadParser(subdir)
                workspace = parser.parse_workspace()
                workspaces.append(workspace)

    return workspaces


# =============================================================================
# Dependency Graph
# =============================================================================


@dataclass
class DependencyNode:
    """
    Represents a node in the dependency graph.

    Attributes:
        bead: The bead this node represents
        dependents: List of bead IDs that depend on this bead
        dependencies: List of bead IDs this bead depends on
        is_critical_path: Whether this node is on the critical path
    """
    bead: Bead
    dependents: list[str] = field(default_factory=list)
    dependencies: list[str] = field(default_factory=list)
    is_critical_path: bool = False


class DependencyGraph:
    """
    Dependency graph for beads.

    Provides visualization and analysis of bead dependencies.
    """

    def __init__(self, workspace: BeadWorkspace):
        """
        Initialize the dependency graph.

        Args:
            workspace: BeadWorkspace to build graph from
        """
        self.workspace = workspace
        self.nodes: dict[str, DependencyNode] = {}
        self._build_graph()

    def _build_graph(self) -> None:
        """Build the dependency graph from workspace beads"""
        # Create nodes for all beads
        for bead in self.workspace.beads:
            self.nodes[bead.id] = DependencyNode(bead=bead)

        # Link dependencies
        for bead in self.workspace.beads:
            node = self.nodes[bead.id]

            for dep_id in bead.depends_on:
                if dep_id in self.nodes:
                    node.dependencies.append(dep_id)
                    self.nodes[dep_id].dependents.append(bead.id)

    def get_ready_beads(self) -> list[Bead]:
        """
        Get beads that are ready to be worked on (no open dependencies).

        Returns:
            List of ready beads sorted by value score
        """
        ready = []

        for bead_id, node in self.nodes.items():
            if node.bead.state == BeadState.READY:
                ready.append(node.bead)

        # Sort by value score (highest first)
        return sorted(ready, key=lambda b: b.value_score, reverse=True)

    def get_blocked_beads(self) -> list[tuple[Bead, list[str]]]:
        """
        Get beads that are blocked by open dependencies.

        Returns:
            List of (bead, blocking_bead_ids) tuples
        """
        blocked = []

        for bead_id, node in self.nodes.items():
            if node.bead.state == BeadState.BLOCKED:
                # Find which dependencies are still open
                blocking = []
                for dep_id in node.dependencies:
                    dep_bead = self.workspace.get_bead_by_id(dep_id)
                    if dep_bead and dep_bead.status_enum != BeadStatus.CLOSED:
                        blocking.append(dep_id)

                if blocking:
                    blocked.append((node.bead, blocking))

        return blocked

    def get_critical_path(self) -> list[Bead]:
        """
        Find the critical path (longest path) through the dependency graph.

        The critical path represents the sequence of tasks that determine
        the minimum project completion time.

        Returns:
            List of beads on the critical path
        """
        # Find longest path using DFS
        def longest_path_from(node_id: str, visited: set[str]) -> list[str]:
            if node_id in visited:
                return []  # Cycle detected

            visited.add(node_id)
            node = self.nodes.get(node_id)
            if not node:
                return [node_id]

            max_path = [node_id]
            for dep_id in node.dependencies:
                path = longest_path_from(dep_id, visited.copy())
                if len(path) + 1 > len(max_path):
                    max_path = [node_id] + path

            return max_path

        critical_path_ids = []
        for bead_id in self.nodes:
            path = longest_path_from(bead_id, set())
            if len(path) > len(critical_path_ids):
                critical_path_ids = path

        # Convert to Bead objects
        critical_path = []
        for bead_id in critical_path_ids:
            if bead_id in self.nodes:
                critical_path.append(self.nodes[bead_id].bead)

        return critical_path

    def detect_cycles(self) -> list[list[str]]:
        """
        Detect circular dependencies in the graph.

        Returns:
            List of cycles (each cycle is a list of bead IDs)
        """
        cycles = []
        visited: set[str] = set()
        rec_stack: set[str] = set()
        path: list[str] = []

        def dfs(node_id: str) -> None:
            visited.add(node_id)
            rec_stack.add(node_id)
            path.append(node_id)

            node = self.nodes.get(node_id)
            if node:
                for dep_id in node.dependencies:
                    if dep_id not in visited:
                        dfs(dep_id)
                    elif dep_id in rec_stack:
                        # Found a cycle
                        cycle_start = path.index(dep_id)
                        cycle = path[cycle_start:] + [dep_id]
                        if cycle not in cycles:
                            cycles.append(cycle)

            path.pop()
            rec_stack.remove(node_id)

        for bead_id in self.nodes:
            if bead_id not in visited:
                dfs(bead_id)

        return cycles


# =============================================================================
# File Watching (inotify + polling fallback)
# =============================================================================


class _BeadFileHandler(FileSystemEventHandler):
    """
    Internal watchdog event handler for bead JSONL file changes.
    """

    def __init__(
        self,
        callback: Callable[[BeadWorkspace], None],
    ):
        """
        Initialize the event handler.

        Args:
            callback: Function to call when bead files change
        """
        self.callback = callback

    def on_modified(self, event) -> None:
        """Handle file modification event"""
        if event.is_directory:
            return

        path = Path(event.src_path)
        if not path.suffix == ".jsonl":
            return

        # Find workspace from file path
        workspace_path = path.parent.parent
        parser = BeadParser(workspace_path)
        workspace = parser.parse_workspace()
        self.callback(workspace)


class BeadWatcher:
    """
    Watch bead JSONL files for changes using inotify with polling fallback.

    Triggers callback when .beads/*.jsonl files are modified.
    """

    def __init__(
        self,
        workspaces: list[Path],
        callback: Callable[[BeadWorkspace], None],
        poll_interval: float = 1.0,
    ):
        """
        Initialize the bead watcher.

        Args:
            workspaces: List of workspace paths to watch
            callback: Function to call when bead files change
            poll_interval: Seconds between polls for fallback
        """
        self.workspaces = [Path(w).expanduser() for w in workspaces]
        self.callback = callback
        self.poll_interval = poll_interval

        self._observer: Any | None = None
        self._running = False
        self._task: asyncio.Task[None] | None = None
        self._using_polling = False

    async def start(self) -> str:
        """
        Start watching bead files.

        Returns:
            Watcher type being used ("inotify" or "polling")
        """
        if self._running:
            return "polling" if self._using_polling else "inotify"

        # Try inotify first
        if WATCHDOG_AVAILABLE:
            try:
                self._observer = Observer()
                handler = _BeadFileHandler(self.callback)

                # Watch all workspace .beads directories
                for workspace in self.workspaces:
                    beads_dir = workspace / ".beads"
                    if beads_dir.exists():
                        self._observer.schedule(handler, str(beads_dir), recursive=False)

                self._observer.start()
                self._running = True
                return "inotify"
            except Exception:
                # Fall back to polling
                pass

        # Use polling fallback
        self._using_polling = True
        self._running = True
        self._task = asyncio.create_task(self._poll_loop())
        return "polling"

    async def stop(self) -> None:
        """Stop watching bead files"""
        self._running = False

        if self._observer:
            self._observer.stop()
            self._observer.join(timeout=5.0)
            self._observer = None

        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
            self._task = None

    async def _poll_loop(self) -> None:
        """Main polling loop"""
        known_files: dict[Path, float] = {}  # file -> mtime

        while self._running:
            try:
                for workspace in self.workspaces:
                    beads_dir = workspace / ".beads"
                    if not beads_dir.exists():
                        continue

                    for jsonl_file in beads_dir.glob("*.jsonl"):
                        current_mtime = jsonl_file.stat().st_mtime

                        if jsonl_file not in known_files:
                            # New file
                            known_files[jsonl_file] = current_mtime
                            parser = BeadParser(workspace)
                            self.callback(parser.parse_workspace())
                        elif known_files[jsonl_file] != current_mtime:
                            # Modified file
                            known_files[jsonl_file] = current_mtime
                            parser = BeadParser(workspace)
                            self.callback(parser.parse_workspace())

                await asyncio.sleep(self.poll_interval)
            except asyncio.CancelledError:
                break
            except Exception:
                await asyncio.sleep(self.poll_interval)

    @property
    def is_running(self) -> bool:
        """Check if watcher is running"""
        return self._running

    @property
    def is_using_polling(self) -> bool:
        """Check if using polling fallback"""
        return self._using_polling


# =============================================================================
# Convenience Functions
# =============================================================================


def parse_workspace(workspace: Path | str) -> BeadWorkspace:
    """
    Convenience function to parse a single workspace.

    Args:
        workspace: Path to workspace directory

    Returns:
        BeadWorkspace with parsed beads
    """
    parser = BeadParser(Path(workspace).expanduser())
    return parser.parse_workspace()


def calculate_value_score(bead: Bead) -> int:
    """
    Convenience function to calculate value score for a bead.

    Args:
        bead: Bead to score

    Returns:
        Integer score from 0-100
    """
    return bead.value_score


def build_dependency_graph(workspace: BeadWorkspace) -> DependencyGraph:
    """
    Convenience function to build a dependency graph.

    Args:
        workspace: BeadWorkspace to build graph from

    Returns:
        DependencyGraph instance
    """
    return DependencyGraph(workspace)
