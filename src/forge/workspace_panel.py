"""
FORGE Workspace Picker Panel

Implements a Textual TUI panel for workspace selection and management.
Provides:
- List of discovered workspaces
- Visual indication of active workspace
- Workspace metadata display (beads, workers, costs)
- Workspace switching functionality
- Cost aggregation view across workspaces
"""

from dataclasses import dataclass
from pathlib import Path
from typing import Any

from textual.widgets import (
    DataTable,
    ListItem,
    ListView,
)
from rich.text import Text

from forge.workspace_manager import (
    WorkspaceManager,
    WorkspaceMetadata,
    WorkspaceStatus,
    get_workspace_manager,
)


# =============================================================================
# Workspace Picker Panel
# =============================================================================


class WorkspacePicker(ListView):
    """
    Workspace picker widget for selecting and managing workspaces.

    Displays a list of discovered workspaces with metadata and
    allows switching between them.
    """

    DEFAULT_CSS = """
    WorkspacePicker {
        height: 1fr;
        width: 1fr;
        border: thick $accent;
    }

    WorkspacePicker ListItem {
        padding: 0 1;
    }

    WorkspacePicker ListItem.-active {
        background: $accent;
    }

    WorkspacePicker ListItem.-active:hover {
        background: $accent-darken-1;
    }
    """

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._workspace_manager: WorkspaceManager | None = None
        self._workspaces: list[WorkspaceMetadata] = []
        self._active_workspace: str | None = None

    def on_mount(self) -> None:
        """Initialize workspace manager and load workspaces"""
        self._workspace_manager = get_workspace_manager()
        self._refresh_workspaces()

    def _refresh_workspaces(self) -> None:
        """Refresh the workspace list from the manager"""
        if self._workspace_manager is None:
            return

        self._workspaces = self._workspace_manager.get_all_workspaces()

        # Sort: active first, then by name
        active = [ws for ws in self._workspaces if ws.status == WorkspaceStatus.ACTIVE]
        inactive = [ws for ws in self._workspaces if ws.status != WorkspaceStatus.ACTIVE]
        inactive.sort(key=lambda ws: ws.name)

        self._workspaces = active + inactive

        # Update active workspace
        active_ws = self._workspace_manager.get_active_workspace()
        self._active_workspace = str(active_ws.path) if active_ws else None

        # Rebuild list items
        self.clear()
        for workspace in self._workspaces:
            self._add_workspace_item(workspace)

    def _add_workspace_item(self, workspace: WorkspaceMetadata) -> None:
        """Add a workspace item to the list"""
        # Build display text
        text = Text()

        # Status indicator
        if workspace.status == WorkspaceStatus.ACTIVE:
            text.append("● ", style="bold green")
        elif workspace.status == WorkspaceStatus.ERROR:
            text.append("⚠ ", style="bold red")
        else:
            text.append("○ ", style="dim")

        # Workspace name
        text.append(workspace.name, style="bold")

        # Bead count
        if workspace.bead_count > 0:
            text.append(f" [{workspace.open_beads}/{workspace.bead_count} beads]", style="cyan")

        # Worker count
        if workspace.worker_count > 0:
            text.append(f" [{workspace.worker_count} workers]", style="yellow")

        # Cost
        if workspace.total_cost > 0:
            text.append(f" [${workspace.total_cost:.2f}]", style="green")

        # Create list item
        item = ListItem(text, id=str(workspace.path))
        self.append(item)

    async def on_list_view_selected(self, event: ListView.Selected) -> None:
        """Handle workspace selection"""
        if self._workspace_manager is None:
            return

        workspace_path = event.item.id
        if not workspace_path:
            return

        # Switch to selected workspace
        await self._workspace_manager.switch_workspace(workspace_path)

        # Refresh display
        self._refresh_workspaces()

    def refresh(self) -> None:
        """Public method to refresh the workspace list"""
        self._refresh_workspaces()


# =============================================================================
# Workspace Summary Panel
# =============================================================================


class WorkspaceSummary(WorkspacePicker):
    """
    Enhanced workspace picker with summary statistics.

    Shows:
    - Workspace list with metadata
    - Total cost across all workspaces
    - Active workspace details
    """

    def _add_workspace_item(self, workspace: WorkspaceMetadata) -> None:
        """Add a workspace item with enhanced information"""
        text = Text()

        # Status indicator
        if workspace.status == WorkspaceStatus.ACTIVE:
            text.append("● ", style="bold green")
        elif workspace.status == WorkspaceStatus.ERROR:
            text.append("⚠ ", style="bold red")
        else:
            text.append("○ ", style="dim")

        # Workspace name
        text.append(workspace.name, style="bold")

        # Completion rate
        if workspace.bead_count > 0:
            completion = workspace.completion_rate * 100
            text.append(f" [{completion:.0f}% complete]", style="cyan")

        # Worker count
        if workspace.worker_count > 0:
            text.append(f" [{workspace.worker_count} workers]", style="yellow")

        # Cost
        if workspace.total_cost > 0:
            text.append(f" [${workspace.total_cost:.2f}]", style="green")

        # Create list item
        item = ListItem(text, id=str(workspace.path))
        self.append(item)


# =============================================================================
# Workspace Cost Breakdown Table
# =============================================================================


class WorkspaceCostTable(DataTable[str]):
    """
    Data table showing cost breakdown by workspace.

    Columns:
    - Workspace name
    - Total cost
    - API requests
    - Tokens used
    - Active workers
    - Bead completion rate
    """

    DEFAULT_CSS = """
    WorkspaceCostTable {
        height: 1fr;
        width: 1fr;
        border: thick $success;
    }

    WorkspaceCostTable DataTable {
        background: $panel;
    }
    """

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._workspace_manager: WorkspaceManager | None = None

        # Add columns
        self.add_column("Workspace", key="name")
        self.add_column("Cost", key="cost")
        self.add_column("Requests", key="requests")
        self.add_column("Tokens", key="tokens")
        self.add_column("Workers", key="workers")
        self.add_column("Complete", key="completion")

        self.zebra_stripes = True

    def on_mount(self) -> None:
        """Initialize workspace manager and load data"""
        self._workspace_manager = get_workspace_manager()
        self._refresh_data()

    def _refresh_data(self) -> None:
        """Refresh the cost data from the manager"""
        if self._workspace_manager is None:
            return

        self.clear()

        workspaces = self._workspace_manager.get_all_workspaces()

        # Sort by cost descending
        workspaces.sort(key=lambda ws: ws.total_cost, reverse=True)

        for workspace in workspaces:
            completion = f"{workspace.completion_rate * 100:.0f}%"

            # Format cost
            cost_str = f"${workspace.total_cost:.2f}"

            # Format tokens (K/M)
            tokens = workspace.total_cost * 1_000_000  # Rough estimate
            if tokens >= 1_000_000:
                tokens_str = f"{tokens / 1_000_000:.1f}M"
            elif tokens >= 1_000:
                tokens_str = f"{tokens / 1_000:.1f}K"
            else:
                tokens_str = str(int(tokens))

            # Add row
            self.add_row(
                workspace.name,
                cost_str,
                str(0),  # TODO: Get actual request count from cost tracker
                tokens_str,
                str(workspace.worker_count),
                completion,
                key=str(workspace.path),
            )

            # Highlight active workspace
            if workspace.status == WorkspaceStatus.ACTIVE:
                row = self.get_row(str(workspace.path))
                if row:
                    row.style = "bold green"

    def refresh(self) -> None:
        """Public method to refresh the cost data"""
        self._refresh_data()
