"""
FORGE Workspace Manager Module

Implements workspace discovery, switching, and state management.
Supports multiple workspaces with bead tracking, worker assignment,
and cost aggregation.

Features:
- Discover workspaces with .beads/ directories
- Manage active/selected workspace state
- Per-workspace worker filtering
- Cross-workspace cost aggregation
- Workspace metadata caching
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from pathlib import Path
from typing import Any

from forge.beads import BeadWorkspace, discover_bead_workspaces


# =============================================================================
# Workspace Data Models
# =============================================================================


class WorkspaceStatus(Enum):
    """Workspace status indicators"""
    ACTIVE = "active"           # Currently selected workspace
    INACTIVE = "inactive"       # Not selected but available
    ERROR = "error"             # Workspace has errors
    LOADING = "loading"         # Workspace is being loaded


@dataclass
class WorkspaceMetadata:
    """
    Metadata for a workspace.

    Attributes:
        path: Path to the workspace directory
        name: Workspace name (derived from path or config)
        status: Current workspace status
        bead_count: Number of beads in workspace
        worker_count: Number of active workers
        last_activity: Timestamp of last activity
        total_cost: Total cost accumulated in this workspace
        open_beads: Number of open beads
        in_progress_beads: Number of in-progress beads
    """
    path: Path
    name: str
    status: WorkspaceStatus = WorkspaceStatus.INACTIVE
    bead_count: int = 0
    worker_count: int = 0
    last_activity: datetime | None = None
    total_cost: float = 0.0
    open_beads: int = 0
    in_progress_beads: int = 0
    closed_beads: int = 0

    @property
    def display_name(self) -> str:
        """Get display name for the workspace"""
        if self.status == WorkspaceStatus.ACTIVE:
            return f"{self.name} *"
        return self.name

    @property
    def completion_rate(self) -> float:
        """Get bead completion rate (0.0 to 1.0)"""
        total = self.open_beads + self.in_progress_beads + self.closed_beads
        if total == 0:
            return 0.0
        return self.closed_beads / total


# =============================================================================
# Workspace Discovery Configuration
# =============================================================================


@dataclass
class DiscoveryConfig:
    """Configuration for workspace discovery"""
    search_paths: list[Path] = field(default_factory=lambda: [Path.home()])
    max_depth: int = 3  # How deep to search for .beads/ directories
    exclude_patterns: list[str] = field(default_factory=lambda: [
        "node_modules", ".git", "venv", ".venv", "env", "build", "dist"
    ])
    include_hidden: bool = False  # Include hidden directories


# =============================================================================
# Workspace Manager
# =============================================================================


class WorkspaceManager:
    """
    Manages workspace discovery, state, and operations.

    Features:
    - Discover workspaces from multiple search paths
    - Track active workspace state
    - Cache workspace metadata for performance
    - Aggregate costs across workspaces
    - Filter workers by workspace
    """

    def __init__(
        self,
        config: DiscoveryConfig | None = None,
        refresh_interval: float = 30.0,  # Refresh metadata every 30 seconds
    ):
        """
        Initialize the workspace manager.

        Args:
            config: Discovery configuration (defaults to standard config)
            refresh_interval: Seconds between metadata refreshes
        """
        self._config = config or DiscoveryConfig()
        self._refresh_interval = refresh_interval

        # Workspace storage
        self._workspaces: dict[str, WorkspaceMetadata] = {}  # path -> metadata
        self._active_workspace: str | None = None  # Path to active workspace

        # Bead workspace cache (parsed bead data)
        self._bead_workspaces: dict[str, BeadWorkspace] = {}  # path -> BeadWorkspace

        # Background refresh task
        self._refresh_task: asyncio.Task[None] | None = None
        self._running = False
        self._stop_event = asyncio.Event()

        # Worker tracking (workspace -> worker IDs)
        self._workspace_workers: dict[str, set[str]] = {}

    async def start(self) -> None:
        """Start the workspace manager background refresh task"""
        if self._running:
            return

        self._running = True
        self._stop_event.clear()

        # Initial discovery
        await self._discover_workspaces()

        # Start background refresh
        self._refresh_task = asyncio.create_task(self._refresh_loop())

    async def stop(self) -> None:
        """Stop the workspace manager"""
        if not self._running:
            return

        self._running = False
        self._stop_event.set()

        if self._refresh_task:
            self._refresh_task.cancel()
            try:
                await self._refresh_task
            except asyncio.CancelledError:
                pass
            self._refresh_task = None

    async def _refresh_loop(self) -> None:
        """Background task to periodically refresh workspace metadata"""
        while not self._stop_event.is_set():
            try:
                await asyncio.wait_for(
                    self._stop_event.wait(),
                    timeout=self._refresh_interval,
                )
            except asyncio.TimeoutError:
                # Normal timeout, refresh metadata
                await self._refresh_metadata()

    async def _discover_workspaces(self) -> None:
        """Discover all workspaces from configured search paths"""
        discovered = {}

        for search_path in self._config.search_paths:
            if not search_path.exists():
                continue

            # Use existing discover_bead_workspaces function
            bead_workspaces = discover_bead_workspaces(search_path)

            for bead_ws in bead_workspaces:
                path_str = str(bead_ws.path)

                # Check if already exists
                if path_str in self._workspaces:
                    # Update existing metadata
                    metadata = self._workspaces[path_str]
                    metadata.bead_count = bead_ws.bead_count
                    metadata.open_beads = bead_ws.open_count
                    metadata.in_progress_beads = bead_ws.in_progress_count
                    metadata.closed_beads = bead_ws.closed_count
                else:
                    # Create new metadata
                    metadata = WorkspaceMetadata(
                        path=bead_ws.path,
                        name=bead_ws.name,
                        status=WorkspaceStatus.INACTIVE,
                        bead_count=bead_ws.bead_count,
                        open_beads=bead_ws.open_count,
                        in_progress_beads=bead_ws.in_progress_count,
                        closed_beads=bead_ws.closed_count,
                        last_activity=datetime.now(),
                    )

                discovered[path_str] = metadata
                self._bead_workspaces[path_str] = bead_ws

        self._workspaces = discovered

    async def _refresh_metadata(self) -> None:
        """Refresh metadata for all known workspaces"""
        for path_str, metadata in self._workspaces.items():
            try:
                # Re-parse the workspace to get fresh data
                from forge.beads import parse_workspace
                bead_ws = parse_workspace(metadata.path)
                self._bead_workspaces[path_str] = bead_ws

                # Update metadata
                metadata.bead_count = bead_ws.bead_count
                metadata.open_beads = bead_ws.open_count
                metadata.in_progress_beads = bead_ws.in_progress_count
                metadata.closed_beads = bead_ws.closed_count
                metadata.last_activity = datetime.now()

            except Exception:
                # Mark workspace as having errors
                metadata.status = WorkspaceStatus.ERROR

    # =========================================================================
    # Workspace Query Methods
    # =========================================================================

    def get_all_workspaces(self) -> list[WorkspaceMetadata]:
        """Get all discovered workspaces"""
        return list(self._workspaces.values())

    def get_workspace(self, path: str | Path) -> WorkspaceMetadata | None:
        """Get metadata for a specific workspace"""
        path_str = str(path)
        return self._workspaces.get(path_str)

    def get_active_workspace(self) -> WorkspaceMetadata | None:
        """Get the currently active workspace"""
        if self._active_workspace is None:
            return None
        return self._workspaces.get(self._active_workspace)

    def get_bead_workspace(self, path: str | Path) -> BeadWorkspace | None:
        """Get the parsed bead workspace for a path"""
        path_str = str(path)
        return self._bead_workspaces.get(path_str)

    def get_workspace_workers(self, path: str | Path) -> set[str]:
        """Get worker IDs assigned to a workspace"""
        path_str = str(path)
        return self._workspace_workers.get(path_str, set()).copy()

    # =========================================================================
    # Workspace Operations
    # =========================================================================

    async def switch_workspace(self, path: str | Path) -> WorkspaceMetadata | None:
        """
        Switch to a different workspace.

        Args:
            path: Path to the workspace to switch to

        Returns:
            The new active workspace metadata, or None if not found
        """
        path_obj = Path(path).expanduser()
        path_str = str(path_obj)

        # Check if workspace exists
        if path_str not in self._workspaces:
            # Try to discover it
            await self._discover_workspaces()
            if path_str not in self._workspaces:
                return None

        # Deactivate current workspace
        if self._active_workspace and self._active_workspace in self._workspaces:
            self._workspaces[self._active_workspace].status = WorkspaceStatus.INACTIVE

        # Activate new workspace
        self._active_workspace = path_str
        self._workspaces[path_str].status = WorkspaceStatus.ACTIVE
        self._workspaces[path_str].last_activity = datetime.now()

        return self._workspaces[path_str]

    async def add_workspace(self, path: str | Path) -> WorkspaceMetadata | None:
        """
        Manually add a workspace to the manager.

        Args:
            path: Path to the workspace directory

        Returns:
            The workspace metadata if added successfully, None otherwise
        """
        path_obj = Path(path).expanduser()

        if not path_obj.exists():
            return None

        if not (path_obj / ".beads").exists():
            return None

        path_str = str(path_obj)

        # Parse workspace
        from forge.beads import parse_workspace
        bead_ws = parse_workspace(path_obj)

        # Create metadata
        metadata = WorkspaceMetadata(
            path=path_obj,
            name=bead_ws.name,
            status=WorkspaceStatus.INACTIVE,
            bead_count=bead_ws.bead_count,
            open_beads=bead_ws.open_count,
            in_progress_beads=bead_ws.in_progress_count,
            closed_beads=bead_ws.closed_count,
            last_activity=datetime.now(),
        )

        self._workspaces[path_str] = metadata
        self._bead_workspaces[path_str] = bead_ws

        return metadata

    def remove_workspace(self, path: str | Path) -> bool:
        """
        Remove a workspace from the manager.

        Args:
            path: Path to the workspace to remove

        Returns:
            True if removed, False if not found
        """
        path_str = str(path)

        if path_str in self._workspaces:
            # Can't remove active workspace
            if self._active_workspace == path_str:
                return False

            del self._workspaces[path_str]
            if path_str in self._bead_workspaces:
                del self._bead_workspaces[path_str]
            if path_str in self._workspace_workers:
                del self._workspace_workers[path_str]

            return True

        return False

    # =========================================================================
    # Worker Assignment
    # =========================================================================

    def assign_worker(self, worker_id: str, workspace: str | Path) -> bool:
        """
        Assign a worker to a workspace.

        Args:
            worker_id: Worker session ID
            workspace: Workspace path

        Returns:
            True if assigned successfully
        """
        path_str = str(workspace)

        if path_str not in self._workspaces:
            return False

        if path_str not in self._workspace_workers:
            self._workspace_workers[path_str] = set()

        self._workspace_workers[path_str].add(worker_id)

        # Update worker count
        self._workspaces[path_str].worker_count = len(self._workspace_workers[path_str])

        return True

    def unassign_worker(self, worker_id: str, workspace: str | Path | None = None) -> bool:
        """
        Unassign a worker from a workspace.

        Args:
            worker_id: Worker session ID
            workspace: Workspace path (if None, removes from all workspaces)

        Returns:
            True if unassigned successfully
        """
        if workspace is None:
            # Remove from all workspaces
            removed = False
            for ws_path, workers in self._workspace_workers.items():
                if worker_id in workers:
                    workers.discard(worker_id)
                    # Update worker count
                    if ws_path in self._workspaces:
                        self._workspaces[ws_path].worker_count = len(workers)
                    removed = True
            return removed

        path_str = str(workspace)

        if path_str in self._workspace_workers and worker_id in self._workspace_workers[path_str]:
            self._workspace_workers[path_str].discard(worker_id)
            # Update worker count
            if path_str in self._workspaces:
                self._workspaces[path_str].worker_count = len(self._workspace_workers[path_str])
            return True

        return False

    # =========================================================================
    # Cost Aggregation
    # =========================================================================

    def get_workspace_cost(self, path: str | Path) -> float:
        """Get total cost for a specific workspace"""
        path_str = str(path)
        if path_str in self._workspaces:
            return self._workspaces[path_str].total_cost
        return 0.0

    def set_workspace_cost(self, path: str | Path, cost: float) -> None:
        """Set the total cost for a workspace"""
        path_str = str(path)
        if path_str in self._workspaces:
            self._workspaces[path_str].total_cost = cost

    def add_workspace_cost(self, path: str | Path, cost: float) -> None:
        """Add cost to a workspace"""
        path_str = str(path)
        if path_str in self._workspaces:
            self._workspaces[path_str].total_cost += cost

    def get_total_cost(self) -> float:
        """Get total cost across all workspaces"""
        return sum(ws.total_cost for ws in self._workspaces.values())

    def get_cost_breakdown(self) -> dict[str, float]:
        """Get cost breakdown by workspace"""
        return {
            metadata.name: metadata.total_cost
            for metadata in self._workspaces.values()
        }


# =============================================================================
# Global Instance
# =============================================================================

_workspace_manager: WorkspaceManager | None = None


def get_workspace_manager() -> WorkspaceManager:
    """Get the global workspace manager instance"""
    global _workspace_manager
    if _workspace_manager is None:
        # Create with default config
        _workspace_manager = WorkspaceManager()
    return _workspace_manager
