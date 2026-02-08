"""
Main FORGE Textual Application

Implements the 6-panel dashboard layout for 199×55 terminal:
- Workers: Worker pool status and management
- Tasks: Task queue and bead tracking
- Costs: Cost analytics and optimization
- Metrics: Performance metrics and resource usage
- Logs: Activity log stream
- Chat: Conversational command input
- Views: Full-screen and split-screen views
"""

import asyncio
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from pathlib import Path
from typing import Any, Callable, cast
from rich.text import Text
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import (
    Container,
    Horizontal,
    HorizontalGroup,
    Vertical,
    VerticalScroll,
)
from textual.reactive import reactive
from textual.widgets import (
    DataTable,
    Footer,
    Header,
    Input,
    Label,
    ListItem,
    Log,
    Markdown,
    ProgressBar,
    Static,
)

# Import status watcher module
from forge.status_watcher import (
    StatusWatcher,
    StatusFileEvent,
    WorkerStatusCache,
    WorkerStatusFile,
    WorkerStatusValue,
    parse_status_file,
)

# Import bead watcher module
from forge.beads import (
    BeadWatcher,
    BeadWorkspace,
    parse_workspace,
)

# Import tools module
from forge.tools import (
    ToolExecutor,
    ToolCallResult,
    ToolDefinition,
    create_success_result,
    create_error_result,
    initialize_tools,
    get_default_tools_path,
)

# Import cost tracker module (lazy import to avoid circular dependency)
def _get_cost_tracker():
    """Lazy import of cost tracker"""
    from forge.cost_tracker import get_cost_tracker
    return get_cost_tracker()


# Import log watcher for real-time log file monitoring (lazy import)
def _get_log_watcher():
    """Lazy import of log watcher"""
    from forge.log_watcher import LogWatcher, LogFileEvent
    return LogWatcher, LogFileEvent


# Import error display patterns per ADR 0014
from forge.error_display import (
    ErrorDisplayManager,
    ErrorSeverity,
    ErrorAction,
    NotificationOverlay,
    ComponentErrorWidget,
    FatalErrorScreen,
    ErrorDialog,
)

# Import workspace manager for multi-workspace support
# Lazy import to avoid circular dependency
def _get_workspace_manager():
    """Lazy import of workspace manager"""
    from forge.workspace_manager import get_workspace_manager, WorkspaceStatus
    return get_workspace_manager(), WorkspaceStatus


# =============================================================================
# Data Models
# =============================================================================


class ViewMode(Enum):
    """View mode for the application"""
    OVERVIEW = "overview"  # Default 6-panel dashboard
    WORKERS = "workers"    # Full-screen workers view
    TASKS = "tasks"        # Full-screen tasks view
    COSTS = "costs"        # Full-screen costs view
    METRICS = "metrics"    # Full-screen metrics view
    LOGS = "logs"          # Full-screen logs view
    SPLIT = "split"        # Split-screen view


class WorkerStatus(Enum):
    """Worker health status"""
    ACTIVE = "active"
    IDLE = "idle"
    UNHEALTHY = "unhealthy"
    SPAWNING = "spawning"
    TERMINATING = "terminating"
    FAILED = "failed"
    STOPPED = "stopped"


class TaskPriority(Enum):
    """Task priority levels"""
    P0 = "0"  # Critical
    P1 = "1"  # High
    P2 = "2"  # Medium
    P3 = "3"  # Low
    P4 = "4"  # Backlog


class TaskStatus(Enum):
    """Task execution status"""
    READY = "ready"
    IN_PROGRESS = "in_progress"
    BLOCKED = "blocked"
    COMPLETED = "completed"


@dataclass
class Worker:
    """Represents an AI coding agent worker"""
    session_id: str
    model: str
    workspace: str
    status: WorkerStatus = WorkerStatus.IDLE
    current_task: str | None = None
    uptime_seconds: int = 0
    tokens_used: int = 0
    cost: float = 0.0
    last_heartbeat: datetime | None = None
    error: str | None = None  # Error message if status file is corrupted
    health_error: str | None = None  # Health check error (from health_monitor)
    health_guidance: list[str] = None  # User guidance per ADR 0014
    health_score: float = 1.0  # Health score 0.0-1.0

    def __post_init__(self):
        """Initialize default values for mutable fields"""
        if self.health_guidance is None:
            self.health_guidance = []

    @classmethod
    def from_status_file(cls, status_file: WorkerStatusFile) -> "Worker":
        """Create Worker from WorkerStatusFile"""
        # Map WorkerStatusValue to WorkerStatus
        status_map = {
            WorkerStatusValue.ACTIVE: WorkerStatus.ACTIVE,
            WorkerStatusValue.IDLE: WorkerStatus.IDLE,
            WorkerStatusValue.FAILED: WorkerStatus.UNHEALTHY,
            WorkerStatusValue.STOPPED: WorkerStatus.TERMINATING,
            WorkerStatusValue.STARTING: WorkerStatus.SPAWNING,
            WorkerStatusValue.SPAWNED: WorkerStatus.SPAWNING,
        }

        status = status_map.get(
            status_file.status,
            WorkerStatus.UNHEALTHY if status_file.error else WorkerStatus.IDLE
        )

        # Extract health error and guidance from raw_data
        health_error = status_file.raw_data.get("health_error") if status_file.raw_data else None
        health_guidance = status_file.raw_data.get("health_guidance", []) if status_file.raw_data else []
        health_score = status_file.raw_data.get("health_score", 1.0) if status_file.raw_data else 1.0

        return cls(
            session_id=status_file.worker_id,
            model=status_file.model,
            workspace=status_file.workspace,
            status=status,
            current_task=status_file.current_task,
            uptime_seconds=0,  # Calculate from started_at if needed
            error=status_file.error,
            health_error=health_error,
            health_guidance=health_guidance,
            health_score=health_score,
        )


@dataclass
class Task:
    """Represents a task/bead in the queue"""
    id: str
    title: str
    priority: TaskPriority
    status: TaskStatus
    model: str | None = None
    workspace: str = ""
    assigned_worker: str | None = None
    estimated_tokens: int = 0
    created_at: datetime | None = None


@dataclass
class Subscription:
    """Represents an AI service subscription"""
    name: str
    model: str
    used: int
    limit: int
    resets_at: datetime
    monthly_cost: float


@dataclass
class CostEntry:
    """Cost tracking entry"""
    model: str
    requests: int
    tokens: int
    cost: float


@dataclass
class MetricData:
    """Performance metrics"""
    throughput_per_hour: float
    avg_time_per_task: float
    queue_velocity: float
    cpu_percent: float
    memory_gb: float
    memory_total_gb: float
    disk_gb: float
    disk_total_gb: float
    network_down_mbps: float
    network_up_mbps: float
    success_rate: float
    completion_count: int
    in_progress_count: int
    failed_count: int


@dataclass
class LogEntry:
    """Activity log entry"""
    timestamp: datetime
    level: str
    message: str
    icon: str

# =============================================================================
# Panel Widgets
# =============================================================================


class WorkersPanel(Static):
    """Worker pool status panel with health error display per ADR 0014"""

    DEFAULT_CSS = """
    WorkersPanel {
        height: 1fr;
        width: 1fr;
        border: thick $primary;
    }

    WorkersPanel > Label {
        text-style: bold;
        padding: 0 1;
    }

    WorkersPanel > Static.error {
        color: $error;
        text-style: bold;
        padding: 0 1;
    }

    WorkersPanel > Static.guidance {
        color: $warning;
        padding: 0 2;
    }
    """

    worker_list: reactive[list[Worker]] = reactive([])
    active_count: reactive[int] = reactive(0)
    idle_count: reactive[int] = reactive(0)

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._table: DataTable[Worker] | None = None
        self._status_cache = WorkerStatusCache()
        self._error_display_workers: set[str] = set()  # Track workers with displayed errors

    def compose(self: ComposeResult) -> ComposeResult:
        yield Label("👷 WORKER POOL")

    def on_mount(self) -> None:
        """Initialize the worker table on mount"""
        # Initial setup
        self._update_counts(self.worker_list)
        self._update_display(self.worker_list)

    def watch_worker_list(self, old_workers: list[Worker], new_workers: list[Worker]) -> None:
        """React to worker list changes"""
        self._update_counts(new_workers)
        self._update_display(new_workers)

    def on_status_file_event(self, event: StatusFileEvent) -> None:
        """
        Handle status file change events.

        Args:
            event: Status file event from the watcher
        """
        # Update cache
        self._status_cache.update(event)

        # Rebuild workers list from cache
        self._rebuild_workers_from_cache()

    def _rebuild_workers_from_cache(self) -> None:
        """Rebuild workers list from status cache"""
        cached_statuses = self._status_cache.get_all()
        new_workers = []

        for status_file in cached_statuses.values():
            worker = Worker.from_status_file(status_file)
            new_workers.append(worker)

        # Update reactive worker list
        self.worker_list = new_workers

    def _update_counts(self, workers: list[Worker]) -> None:
        """Update worker counts"""
        self.active_count = sum(1 for w in workers if w.status == WorkerStatus.ACTIVE)
        self.idle_count = sum(1 for w in workers if w.status == WorkerStatus.IDLE)

    def _update_display(self, workers: list[Worker]) -> None:
        """Update the display with worker data including health errors per ADR 0014"""
        # Build display text
        active = sum(1 for w in workers if w.status == WorkerStatus.ACTIVE)
        idle = sum(1 for w in workers if w.status == WorkerStatus.IDLE)
        unhealthy = sum(1 for w in workers if w.status == WorkerStatus.UNHEALTHY)

        title = Text()
        title.append("👷 WORKER POOL (", style="bold")
        title.append(f"{active}", style="bold green")
        title.append(" Active, ", style="bold")
        title.append(f"{idle}", style="bold yellow")
        title.append(" Idle", style="bold")

        if unhealthy > 0:
            title.append(f", {unhealthy} Unhealthy", style="bold red")

        title.append(")", style="bold")
        self.update(title)

        # If table exists, update it
        if self._table is not None and self._table.is_mounted:
            self._table.clear()
            for worker in workers[:15]:  # Show first 15 workers
                status_symbol = self._get_status_symbol(worker.status)
                self._table.add_row(
                    worker.session_id,
                    worker.model,
                    worker.workspace[:20] + "..." if len(worker.workspace) > 20 else worker.workspace,
                    status_symbol,
                    f"{worker.uptime_seconds // 60}m",
                )

        # Show health errors per ADR 0014
        self._show_health_errors(workers)

    def _show_health_errors(self, workers: list[Worker]) -> None:
        """
        Display health errors with actionable guidance per ADR 0014.

        Shows error indicators for unhealthy workers with:
        - Primary error message
        - Actionable guidance steps
        - No automatic recovery (user decides)
        """
        # Get unhealthy workers with health errors
        unhealthy_workers = [
            w for w in workers
            if w.status == WorkerStatus.UNHEALTHY and (w.error or w.health_error)
        ]

        # Clear error display for recovered workers
        current_unhealthy_ids = {w.session_id for w in unhealthy_workers}
        recovered = self._error_display_workers - current_unhealthy_ids
        if recovered:
            self._error_display_workers = current_unhealthy_ids
            # Re-mount to clear old error displays
            self._refresh_display()
            return

        # Show new errors
        for worker in unhealthy_workers:
            if worker.session_id in self._error_display_workers:
                continue  # Already displayed

            self._error_display_workers.add(worker.session_id)

            # Use health_error if available (from health monitor), otherwise use error
            error_message = worker.health_error or worker.error

            # Build error display with guidance
            error_widget = self._build_error_widget(worker, error_message)
            if error_widget:
                # Display error in the panel
                self.mount(error_widget, before=self._table if self._table else None)

    def _build_error_widget(self, worker: Worker, error_message: str) -> Static | None:
        """
        Build error display widget per ADR 0014.

        Args:
            worker: Worker with error
            error_message: Error message to display

        Returns:
            Static widget with error and guidance, or None if no error
        """
        if not error_message:
            return None

        # Build error text with guidance
        error_lines = [
            f"⚠️  {worker.session_id}: {error_message}",
        ]

        # Add guidance if available
        if worker.health_guidance:
            error_lines.append("")
            error_lines.append("Suggested actions:")
            for guidance in worker.health_guidance[:3]:  # Show max 3 guidance items
                error_lines.append(f"  • {guidance}")

        return Static(
            "\n".join(error_lines),
            classes="error" if not worker.health_guidance else None
        )

    def _refresh_display(self) -> None:
        """Refresh the display to clear old error messages"""
        # Remove all children and rebuild
        for child in self.children:
            if isinstance(child, Static) and "error" in getattr(child, 'classes', []):
                child.remove()

    def _get_status_symbol(self, status: WorkerStatus) -> str:
        """Get status symbol for display"""
        symbols = {
            WorkerStatus.ACTIVE: "●EXEC",
            WorkerStatus.IDLE: "○IDLE",
            WorkerStatus.UNHEALTHY: "⚠️ ERR",
            WorkerStatus.SPAWNING: "⟳SPAWN",
            WorkerStatus.TERMINATING: "⦻STOP",
            WorkerStatus.FAILED: "✗FAIL",
            WorkerStatus.STOPPED: "⦻STOP",
        }
        return symbols.get(status, "?UNKNOWN")


class TasksPanel(Static):
    """Task queue panel"""

    DEFAULT_CSS = """
    TasksPanel {
        height: 1fr;
        width: 1fr;
        border: thick $secondary;
    }

    TasksPanel > Label {
        text-style: bold;
        padding: 0 1;
    }
    """

    tasks: reactive[list[Task]] = reactive([])
    ready_count: reactive[int] = reactive(0)

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)

    def compose(self: ComposeResult) -> ComposeResult:
        yield Label("📋 TASK QUEUE")

    def watch_tasks(self, old_tasks: list[Task], new_tasks: list[Task]) -> None:
        """React to task list changes"""
        ready = sum(1 for t in new_tasks if t.status == TaskStatus.READY)
        self.ready_count = ready
        self._update_display(new_tasks)

    def _update_display(self, tasks: list[Task]) -> None:
        """Update the display with task data and value scores"""
        ready_tasks = [t for t in tasks if t.status == TaskStatus.READY]

        # Calculate value scores for ready tasks
        ready_tasks_with_scores = []
        for task in ready_tasks:
            score = self._calculate_task_value_score(task)
            ready_tasks_with_scores.append((task, score))

        # Sort by value score (descending)
        ready_tasks_with_scores.sort(key=lambda x: x[1], reverse=True)

        # Build display text
        title = Text()
        title.append("📋 TASK QUEUE (", style="bold")
        title.append(f"{len(ready_tasks)}", style="bold cyan")
        title.append(" Ready)", style="bold")

        # Add top tasks with scores
        if ready_tasks_with_scores:
            title.append("\n\n", style="")
            title.append("Top Tasks by Value Score:\n", style="bold yellow")

            for i, (task, score) in enumerate(ready_tasks_with_scores[:5], 1):
                # Color code by score
                if score >= 70:
                    score_style = "bold green"
                elif score >= 40:
                    score_style = "bold yellow"
                else:
                    score_style = "bold white"

                title.append(f"{i}. ", style="dim")
                title.append(f"{task.id[:10]}...", style="cyan")
                title.append(f" {task.title[:30]}", style="white")
                title.append(f" [P{task.priority}]", style=self._priority_style(task.priority))
                title.append(f" ★{score}", style=score_style)
                title.append("\n", style="")

            if len(ready_tasks_with_scores) > 5:
                title.append(f"... and {len(ready_tasks_with_scores) - 5} more\n", style="dim")

        self.update(title)

    def _calculate_task_value_score(self, task: Task) -> int:
        """Calculate task value score using the scoring algorithm"""
        # Priority (40 points)
        priority_score = self._get_priority_score(task.priority)

        # Blockers (30 points) - count tasks blocked by this task
        # For Task objects, we need to check if there's a blocks attribute
        blocker_count = len(getattr(task, 'blocks', []))
        blocker_score = min(blocker_count * 10, 30)

        # Age (20 points)
        from datetime import datetime, timezone
        created_at = getattr(task, 'created_at', datetime.now(timezone.utc))
        if isinstance(created_at, str):
            created_at = datetime.fromisoformat(created_at.replace('Z', '+00:00'))
        age_days = (datetime.now(timezone.utc) - created_at).days
        age_score = min(age_days * 3, 20)

        # Labels (10 points)
        labels = getattr(task, 'labels', [])
        urgent_labels = {"critical", "urgent", "blocker", "hotfix", "mvp"}
        label_score = 10 if any(l.lower() in urgent_labels for l in labels) else 0

        return min(priority_score + blocker_score + age_score + label_score, 100)

    def _priority_style(self, priority: str) -> str:
        """Get color style for priority level"""
        if priority == "0" or priority == TaskPriority.P0:
            return "bold red"
        elif priority == "1" or priority == TaskPriority.P1:
            return "bold orange"
        elif priority == "2" or priority == TaskPriority.P2:
            return "bold yellow"
        elif priority == "3" or priority == TaskPriority.P3:
            return "bold white"
        else:
            return "dim"


class CostsPanel(Static):
    """Cost analytics panel"""

    DEFAULT_CSS = """
    CostsPanel {
        height: 1fr;
        width: 1fr;
        border: thick $success;
    }

    CostsPanel > Label {
        text-style: bold;
        padding: 0 1;
    }
    """

    subscriptions: reactive[list[Subscription]] = reactive([])
    costs: reactive[list[CostEntry]] = reactive([])
    total_cost_today: reactive[float] = reactive(0.0)

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)

    def compose(self: ComposeResult) -> ComposeResult:
        yield Label("💰 COST ANALYTICS")

    def watch_costs(self, old_costs: list[CostEntry], new_costs: list[CostEntry]) -> None:
        """React to cost changes"""
        self.total_cost_today = sum(c.cost for c in new_costs)
        self._update_display(new_costs)

    def _update_display(self, costs: list[CostEntry]) -> None:
        """Update the display with cost data"""
        total = sum(c.cost for c in costs)
        title = Text()
        title.append("💰 COST ANALYTICS (Today: $", style="bold")
        title.append(f"{total:.2f}", style="bold green")
        title.append(")", style="bold")
        self.update(title)


class MetricsPanel(Static):
    """Performance metrics panel"""

    DEFAULT_CSS = """
    MetricsPanel {
        height: 1fr;
        width: 1fr;
        border: thick $warning;
    }

    MetricsPanel > Label {
        text-style: bold;
        padding: 0 1;
    }
    """

    metrics: reactive[MetricData | None] = reactive(None)

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)

    def compose(self: ComposeResult) -> ComposeResult:
        yield Label("📊 PERFORMANCE METRICS")

    def watch_metrics(
        self, old_metrics: MetricData | None, new_metrics: MetricData | None
    ) -> None:
        """React to metric changes"""
        if new_metrics is not None:
            self._update_display(new_metrics)

    def _update_display(self, metrics: MetricData) -> None:
        """Update the display with metrics"""
        title = Text()
        title.append("📊 METRICS (", style="bold")
        title.append(f"{metrics.throughput_per_hour:.1f}", style="bold cyan")
        title.append(" beads/hr)", style="bold")
        self.update(title)


class LogsPanel(Static):
    """Activity log panel"""

    DEFAULT_CSS = """
    LogsPanel {
        height: 1fr;
        width: 2fr;
        border: thick $accent;
    }

    LogsPanel > Label {
        text-style: bold;
        padding: 0 1;
    }

    LogsPanel Log {
        height: 1fr;
    }
    """

    logs: reactive[list[LogEntry]] = reactive([])

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._log_widget: Log | None = None

    def compose(self: ComposeResult) -> ComposeResult:
        yield Label("📜 ACTIVITY LOG")
        yield Log()

    def on_mount(self) -> None:
        """Get reference to log widget on mount"""
        self._log_widget = self.query_one(Log)

    def watch_logs(self, old_logs: list[LogEntry], new_logs: list[LogEntry]) -> None:
        """React to log changes"""
        if self._log_widget is not None:
            # Only add new logs
            for log_entry in new_logs[len(old_logs) :]:
                ts = log_entry.timestamp.strftime("%H:%M:%S")
                self._log_widget.write_line(f"{ts} {log_entry.icon} {log_entry.message}")


class ChatPanel(Static):
    """Conversational command input panel"""

    DEFAULT_CSS = """
    ChatPanel {
        height: 3;
        width: 1fr;
        border: thick $primary;
    }

    ChatPanel > Label {
        text-style: bold;
        padding: 0 1;
    }

    ChatPanel Input {
        width: 1fr;
    }
    """

    input_text: reactive[str] = reactive("")
    on_command_submit: Callable[[str], None] | None = None

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._input: Input | None = None

    def compose(self: ComposeResult) -> ComposeResult:
        yield Label("💬 COMMAND (Press : to activate)")
        yield Input(placeholder="Enter command or natural language...", id="command_input")

    def on_mount(self) -> None:
        """Get reference to input widget on mount"""
        self._input = self.query_one(Input)

    def on_input_submitted(self, event: Input.Submitted) -> None:
        """Handle command submission"""
        if self.on_command_submit is not None and event.value.strip():
            self.on_command_submit(event.value.strip())
            self._input.clear()

    def focus_input(self) -> None:
        """Focus the command input"""
        if self._input is not None:
            self._input.focus()

# =============================================================================
# Main Application
# =============================================================================


class ForgeApp(App):
    """
    Main FORGE Control Panel Application

    Responsive 6-panel layout with support for multiple terminal sizes:
    - 199×38: Compact layout (reduced panel heights)
    - 199×55: Standard layout (default)
    - 199×70+: Large layout (expanded panel heights)
    - Other sizes: Responsive fallback using flex units

    Layout:
    - Top-left: Workers panel
    - Top-center: Tasks panel
    - Top-right: Costs panel
    - Middle-left: Metrics panel
    - Bottom: Logs panel (spans full width)
    - Footer: Chat input
    """

    TITLE = "FORGE Control Panel"
    SUB_TITLE = "Federated Orchestration & Resource Generation Engine"
    CSS_PATH = "styles.css"

    # View state
    _current_view: ViewMode = ViewMode.OVERVIEW
    _split_left: str | None = None
    _split_right: str | None = None
    _view_history: list[ViewMode] = []
    _tool_executor: ToolExecutor | None = None

    # Bindings
    BINDINGS = [
        # Global
        Binding("q", "quit", "Quit", show=True),
        Binding(":", "toggle_chat", "Command", show=True),
        Binding("r", "refresh", "Refresh", show=True),
        Binding("?", "show_help", "Help", show=True),
        # View Navigation (uppercase for switching views)
        Binding("W", "switch_view('workers')", "Workers View", show=True),
        Binding("T", "switch_view('tasks')", "Tasks View", show=True),
        Binding("C", "switch_view('costs')", "Costs View", show=True),
        Binding("M", "switch_view('metrics')", "Metrics View", show=True),
        Binding("L", "switch_view('logs')", "Logs View", show=True),
        Binding("O", "switch_view('overview')", "Overview", show=True),
        Binding("s", "toggle_split", "Split View", show=True),
        # Panel Focus (lowercase for focusing panels)
        Binding("c", "focus_chat", "Chat", show=True),
        Binding("ctrl+w", "focus_panel('workers')", "Focus Workers", show=True),
        Binding("ctrl+t", "focus_panel('tasks')", "Focus Tasks", show=True),
        Binding("ctrl+m", "focus_panel('metrics')", "Focus Metrics", show=True),
        Binding("ctrl+l", "focus_panel('logs')", "Focus Logs", show=True),
        # Navigation
        Binding("escape", "go_back", "Back", show=True),
        Binding("tab", "cycle_view", "Next View", show=True),
        Binding("shift+tab", "cycle_view_reverse", "Prev View", show=True),
    ]

    # Data storage (private to avoid conflicts with Textual internals)
    _workers_store: list[Worker]
    _tasks_store: list[Task]
    _subscriptions_store: list[Subscription]
    _costs_store: list[CostEntry]
    _metrics_store: MetricData | None
    _logs_store: list[LogEntry]

    # Panel references
    _workers_panel: WorkersPanel | None = None
    _tasks_panel: TasksPanel | None = None
    _costs_panel: CostsPanel | None = None
    _metrics_panel: MetricsPanel | None = None
    _logs_panel: LogsPanel | None = None
    _chat_panel: ChatPanel | None = None

    # Status watcher
    _status_watcher: StatusWatcher | None = None
    _status_dir: Path = Path.home() / ".forge" / "status"

    # Bead watcher
    _bead_watcher: BeadWatcher | None = None
    _bead_workspaces: list[Path] = field(default_factory=list)

    # Workspace manager for multi-workspace support
    _workspace_manager: Any | None = None  # WorkspaceManager

    # Cost tracker
    _cost_tracker: "CostTracker | None" = None
    _cost_refresh_interval: float = 30.0  # Refresh cost data every 30 seconds

    # Log monitor for cost tracking
    _log_monitor: Any | None = None
    _log_dir: Path = Path.home() / ".forge" / "logs"

    # Error display manager per ADR 0014
    _error_display: ErrorDisplayManager | None = None

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        # Initialize storage
        self._workers_store = []
        self._tasks_store = []
        self._subscriptions_store = []
        self._costs_store = []
        self._metrics_store = None
        self._logs_store = []

        # Initialize cost tracker
        self._initialize_cost_tracker()

        # Initialize log monitor (lazy, starts in on_mount)
        self._log_monitor = None

        # Initialize bead watcher workspaces from environment or current directory
        self._initialize_bead_workspaces()

        # Initialize workspace manager for multi-workspace support
        self._initialize_workspace_manager()

        # Initialize view state
        self._current_view = ViewMode.OVERVIEW
        self._split_left = None
        self._split_right = None
        self._view_history = []
        # Initialize tool executor with all tools
        self._tool_executor = ToolExecutor(register_all=False)  # We'll register manually with callbacks
        self._register_all_tools()
        # Initialize tool definitions for chat backend
        self._tools_path = get_default_tools_path()
        self._tools_payload = initialize_tools(
            output_path=self._tools_path,
            format="openai",
            force=False
        )
        # Initialize with sample data
        self._initialize_sample_data()

        # Initialize error display manager per ADR 0014
        self._error_display = ErrorDisplayManager(self)

    def _register_tool_safe(self, tool_name: str, callback) -> None:
        """Register a tool callback, handling missing tools gracefully"""
        from forge.tool_definitions import get_tool

        tool_def = get_tool(tool_name)
        if tool_def is not None and self._tool_executor is not None:
            self._tool_executor.register_tool(tool_def, callback=callback)

    def _register_all_tools(self) -> None:
        """Register all tool callbacks with the tool executor"""
        if self._tool_executor is None:
            return

        # Register view control tools (these have app-level callbacks)
        self._register_tool_safe("switch_view", lambda **kwargs: self._tool_switch_view(**kwargs))
        self._register_tool_safe("split_view", lambda **kwargs: self._tool_split_view(**kwargs))
        self._register_tool_safe("focus_panel", lambda **kwargs: self._tool_focus_panel(**kwargs))

        # Register worker management tools
        self._register_tool_safe("spawn_worker", lambda **kwargs: self._tool_spawn_worker(**kwargs))
        self._register_tool_safe("kill_worker", lambda **kwargs: self._tool_kill_worker(**kwargs))
        self._register_tool_safe("list_workers", lambda **kwargs: self._tool_list_workers(**kwargs))
        self._register_tool_safe("restart_worker", lambda **kwargs: self._tool_restart_worker(**kwargs))

        # Register task management tools
        self._register_tool_safe("filter_tasks", lambda **kwargs: self._tool_filter_tasks(**kwargs))
        self._register_tool_safe("create_task", lambda **kwargs: self._tool_create_task(**kwargs))
        self._register_tool_safe("assign_task", lambda **kwargs: self._tool_assign_task(**kwargs))
        self._register_tool_safe("suggest_assignments", lambda **kwargs: self._tool_suggest_assignments(**kwargs))

        # Register cost analytics tools
        self._register_tool_safe("show_costs", lambda **kwargs: self._tool_show_costs(**kwargs))
        self._register_tool_safe("optimize_routing", lambda **kwargs: self._tool_optimize_routing(**kwargs))
        self._register_tool_safe("forecast_costs", lambda **kwargs: self._tool_forecast_costs(**kwargs))
        self._register_tool_safe("show_metrics", lambda **kwargs: self._tool_show_metrics(**kwargs))

        # Register data export tools
        self._register_tool_safe("export_logs", lambda **kwargs: self._tool_export_logs(**kwargs))
        self._register_tool_safe("export_metrics", lambda **kwargs: self._tool_export_metrics(**kwargs))
        self._register_tool_safe("screenshot", lambda **kwargs: self._tool_screenshot(**kwargs))

        # Register configuration tools
        self._register_tool_safe("set_config", lambda **kwargs: self._tool_set_config(**kwargs))
        self._register_tool_safe("get_config", lambda **kwargs: self._tool_get_config(**kwargs))
        self._register_tool_safe("save_layout", lambda **kwargs: self._tool_save_layout(**kwargs))
        self._register_tool_safe("load_layout", lambda **kwargs: self._tool_load_layout(**kwargs))

        # Register help & discovery tools
        self._register_tool_safe("help", lambda **kwargs: self._tool_help(**kwargs))
        self._register_tool_safe("search_docs", lambda **kwargs: self._tool_search_docs(**kwargs))
        self._register_tool_safe("list_capabilities", lambda **kwargs: self._tool_list_capabilities(**kwargs))

        # Register notification tools (from tools.py, not tool_definitions.py)
        self._register_tool_safe("show_notification", lambda **kwargs: self._tool_show_notification(**kwargs))
        self._register_tool_safe("show_warning", lambda **kwargs: self._tool_show_warning(**kwargs))
        self._register_tool_safe("ask_user", lambda **kwargs: self._tool_ask_user(**kwargs))
        self._register_tool_safe("highlight_beads", lambda **kwargs: self._tool_highlight_beads(**kwargs))

        # Register system tools (from tools.py)
        self._register_tool_safe("get_status", lambda **kwargs: self._tool_get_status(**kwargs))
        self._register_tool_safe("refresh", lambda **kwargs: self._tool_refresh(**kwargs))
        self._register_tool_safe("ping_worker", lambda **kwargs: self._tool_ping_worker(**kwargs))
        self._register_tool_safe("get_worker_info", lambda **kwargs: self._tool_get_worker_info(**kwargs))
        self._register_tool_safe("pause_worker", lambda **kwargs: self._tool_pause_worker(**kwargs))
        self._register_tool_safe("resume_worker", lambda **kwargs: self._tool_resume_worker(**kwargs))

        # Register workspace tools (from tools.py)
        self._register_tool_safe("switch_workspace", lambda **kwargs: self._tool_switch_workspace(**kwargs))
        self._register_tool_safe("list_workspaces", lambda **kwargs: self._tool_list_workspaces(**kwargs))
        self._register_tool_safe("create_workspace", lambda **kwargs: self._tool_create_workspace(**kwargs))
        self._register_tool_safe("get_workspace_info", lambda **kwargs: self._tool_get_workspace_info(**kwargs))

        # Register analytics tools (from tools.py)
        self._register_tool_safe("show_throughput", lambda **kwargs: self._tool_show_throughput(**kwargs))
        self._register_tool_safe("show_latency", lambda **kwargs: self._tool_show_latency(**kwargs))
        self._register_tool_safe("show_success_rate", lambda **kwargs: self._tool_show_success_rate(**kwargs))
        self._register_tool_safe("show_worker_efficiency", lambda **kwargs: self._tool_show_worker_efficiency(**kwargs))
        self._register_tool_safe("show_task_distribution", lambda **kwargs: self._tool_show_task_distribution(**kwargs))
        self._register_tool_safe("show_trends", lambda **kwargs: self._tool_show_trends(**kwargs))
        self._register_tool_safe("analyze_bottlenecks", lambda **kwargs: self._tool_analyze_bottlenecks(**kwargs))

    # =============================================================================
    # View Control Tool Callbacks
    # =============================================================================

    def _tool_switch_view(self, view: str) -> ToolCallResult:
        """Tool callback for switch_view"""
        try:
            self.action_switch_view(view)
            return create_success_result(
                "switch_view",
                f"Switched to {view} view",
                {"view": view}
            )
        except Exception as e:
            return create_error_result("switch_view", f"Failed to switch view: {e}")

    def _tool_split_view(self, left: str, right: str) -> ToolCallResult:
        """Tool callback for split_view"""
        try:
            self.action_split_view(left, right)
            return create_success_result(
                "split_view",
                f"Created split view: {left} | {right}",
                {"left": left, "right": right}
            )
        except Exception as e:
            return create_error_result("split_view", f"Failed to create split view: {e}")

    def _tool_focus_panel(self, panel: str) -> ToolCallResult:
        """Tool callback for focus_panel"""
        try:
            self.action_focus_panel(panel)
            return create_success_result(
                "focus_panel",
                f"Focused on {panel} panel",
                {"panel": panel}
            )
        except Exception as e:
            return create_error_result("focus_panel", f"Failed to focus panel: {e}")

    # =============================================================================
    # Worker Management Tool Callbacks
    # =============================================================================

    def _tool_spawn_worker(self, model: str, count: int, workspace: str | None = None) -> ToolCallResult:
        """Tool callback for spawn_worker - spawns new AI coding workers"""
        try:
            # Import launcher for worker spawning
            from forge.launcher import spawn_workers

            # Determine workspace path
            if workspace:
                workspace_path = workspace
            else:
                # Use active workspace from workspace manager
                if self._workspace_manager is not None:
                    active_ws = self._workspace_manager.get_active_workspace()
                    workspace_path = str(active_ws.path) if active_ws else str(Path.cwd())
                else:
                    workspace_path = str(Path.cwd())

            worker_ids = spawn_workers(
                model=model,
                count=count,
                workspace=workspace_path
            )

            # Assign workers to workspace in workspace manager
            if self._workspace_manager is not None:
                for worker_id in worker_ids:
                    self._workspace_manager.assign_worker(worker_id, workspace_path)

            return create_success_result(
                "spawn_worker",
                f"Spawned {count} {model} worker(s)",
                {
                    "worker_ids": worker_ids,
                    "model": model,
                    "count": count,
                    "workspace": workspace_path
                }
            )
        except Exception as e:
            return create_error_result("spawn_worker", f"Failed to spawn workers: {e}")

    def _tool_kill_worker(self, worker_id: str, filter: str | None = None) -> ToolCallResult:
        """Tool callback for kill_worker - terminates a worker"""
        try:
            from forge.launcher import kill_worker_by_id, kill_workers_by_filter

            if worker_id.lower() == "all":
                # Kill all workers with optional filter
                killed_ids = kill_workers_by_filter(filter or "all")
            else:
                # Kill specific worker
                killed_ids = [worker_id]
                kill_worker_by_id(worker_id)

            return create_success_result(
                "kill_worker",
                f"Terminated {len(killed_ids)} worker(s)",
                {"worker_ids": killed_ids}
            )
        except Exception as e:
            return create_error_result("kill_worker", f"Failed to kill worker: {e}")

    def _tool_list_workers(self, filter: str = "all", workspace: str | None = None) -> ToolCallResult:
        """Tool callback for list_workers - lists workers with optional filtering"""
        try:
            workers = self._workers_store

            # Apply workspace filter first
            if workspace and self._workspace_manager is not None:
                workspace_workers = self._workspace_manager.get_workspace_workers(workspace)
                workers = [w for w in workers if w.session_id in workspace_workers]

            # Apply status filter
            if filter != "all":
                if filter == "idle":
                    filtered = [w for w in workers if w.status == WorkerStatus.IDLE]
                elif filter == "active":
                    filtered = [w for w in workers if w.status == WorkerStatus.ACTIVE]
                elif filter == "failed":
                    filtered = [w for w in workers if w.status == WorkerStatus.FAILED]
                elif filter == "stuck":
                    filtered = [w for w in workers if w.status == WorkerStatus.UNHEALTHY]
                elif filter == "healthy":
                    filtered = [w for w in workers if w.status in [WorkerStatus.IDLE, WorkerStatus.ACTIVE]]
                else:
                    filtered = workers
            else:
                filtered = workers

            worker_data = [
                {
                    "id": w.session_id,
                    "model": w.model,
                    "status": w.status.value,
                    "workspace": w.workspace,
                    "task": w.current_task,
                }
                for w in filtered
            ]

            return create_success_result(
                "list_workers",
                f"Found {len(filtered)} worker(s)",
                {"workers": worker_data, "filter": filter, "workspace": workspace}
            )
        except Exception as e:
            return create_error_result("list_workers", f"Failed to list workers: {e}")

    def _tool_restart_worker(self, worker_id: str) -> ToolCallResult:
        """Tool callback for restart_worker - restarts a worker"""
        try:
            from forge.launcher import restart_worker

            new_worker_id = restart_worker(worker_id)

            return create_success_result(
                "restart_worker",
                f"Restarted worker {worker_id} -> {new_worker_id}",
                {"old_id": worker_id, "new_id": new_worker_id}
            )
        except Exception as e:
            return create_error_result("restart_worker", f"Failed to restart worker: {e}")

    # =============================================================================
    # Task Management Tool Callbacks
    # =============================================================================

    def _tool_filter_tasks(self, priority: str | None = None, status: str | None = None, labels: list[str] | None = None) -> ToolCallResult:
        """Tool callback for filter_tasks - filters task queue"""
        try:
            tasks = self._tasks_store

            # Apply filters
            if priority:
                tasks = [t for t in tasks if t.priority == priority]
            if status:
                tasks = [t for t in tasks if t.status == status]
            if labels:
                for label in labels:
                    tasks = [t for t in tasks if label in t.labels]

            task_data = [
                {
                    "id": t.id,
                    "title": t.title,
                    "priority": t.priority,
                    "status": t.status,
                    "labels": t.labels,
                }
                for t in tasks
            ]

            return create_success_result(
                "filter_tasks",
                f"Filtered to {len(tasks)} task(s)",
                {
                    "tasks": task_data,
                    "filters": {"priority": priority, "status": status, "labels": labels}
                }
            )
        except Exception as e:
            return create_error_result("filter_tasks", f"Failed to filter tasks: {e}")

    def _tool_create_task(self, title: str, priority: str, description: str | None = None) -> ToolCallResult:
        """Tool callback for create_task - creates a new task"""
        try:
            # Generate task ID
            import uuid
            task_id = f"bd-{uuid.uuid4().hex[:6]}"

            # Create task
            new_task = Task(
                id=task_id,
                title=title,
                priority=priority,
                status="open",
                description=description or "",
                labels=[]
            )

            self._tasks_store.append(new_task)
            if self._tasks_panel:
                self._tasks_panel.tasks = self._tasks_store

            return create_success_result(
                "create_task",
                f"Created task {task_id}: {title}",
                {
                    "task_id": task_id,
                    "title": title,
                    "priority": priority
                }
            )
        except Exception as e:
            return create_error_result("create_task", f"Failed to create task: {e}")

    def _tool_assign_task(self, task_id: str, worker_id: str = "auto") -> ToolCallResult:
        """Tool callback for assign_task - assigns a task to a worker"""
        try:
            # Find task
            task = next((t for t in self._tasks_store if t.id == task_id), None)
            if not task:
                return create_error_result("assign_task", f"Task not found: {task_id}")

            # Auto-assign or assign to specific worker
            if worker_id == "auto":
                # Find first available idle worker
                worker = next((w for w in self._workers_store if w.status == "idle"), None)
                if not worker:
                    return create_error_result("assign_task", "No available workers")
                worker_id = worker.id

            # Update task
            task.status = "in_progress"
            task.assignee = worker_id

            # Update worker
            for worker in self._workers_store:
                if worker.id == worker_id:
                    worker.current_task = task_id
                    worker.status = "active"
                    break

            return create_success_result(
                "assign_task",
                f"Assigned {task_id} to {worker_id}",
                {"task_id": task_id, "worker_id": worker_id}
            )
        except Exception as e:
            return create_error_result("assign_task", f"Failed to assign task: {e}")

    def _tool_suggest_assignments(self, count: int = 5, worker_filter: str = "all") -> ToolCallResult:
        """Tool callback for suggest_assignments - suggests optimal task-worker assignments"""
        try:
            from forge.config import get_config
            from forge.beads import calculate_value_score, Bead

            config = get_config()
            priority_tiers = config.routing.priority_tiers

            # Get worker tier mapping
            tier_mapping = {
                "P0": priority_tiers.P0,
                "P1": priority_tiers.P1,
                "P2": priority_tiers.P2,
                "P3": priority_tiers.P3,
                "P4": priority_tiers.P4,
            }

            # Filter workers based on worker_filter
            available_workers = []
            for worker in self._workers_store:
                if worker.status != "idle":
                    continue

                # Determine worker tier
                worker_tier = self._get_worker_tier(worker.model)
                worker_model = worker.model.lower()

                # Apply filter
                if worker_filter == "all":
                    available_workers.append(worker)
                elif worker_filter == "premium" and worker_tier == "premium":
                    available_workers.append(worker)
                elif worker_filter == "standard" and worker_tier == "standard":
                    available_workers.append(worker)
                elif worker_filter == "budget" and worker_tier == "budget":
                    available_workers.append(worker)
                elif worker_filter in ["sonnet", "opus", "haiku"] and worker_filter in worker_model:
                    available_workers.append(worker)

            # Get ready tasks (not completed or in progress)
            ready_tasks = [
                t for t in self._tasks_store
                if t.status not in ("completed", "in_progress")
            ]

            # Calculate value scores for each task
            task_scores = []
            for task in ready_tasks:
                # Calculate value score using the algorithm
                # Priority (40pts) + Blockers (30pts) + Age (20pts) + Labels (10pts)
                priority_score = self._get_priority_score(task.priority)
                blocker_score = min(len(getattr(task, 'blocks', [])) * 10, 30)

                # Age score (older tasks get more points)
                from datetime import datetime, timezone
                created_at = getattr(task, 'created_at', datetime.now(timezone.utc))
                if isinstance(created_at, str):
                    created_at = datetime.fromisoformat(created_at.replace('Z', '+00:00'))
                age_days = (datetime.now(timezone.utc) - created_at).days
                age_score = min(age_days * 3, 20)

                # Label score (urgent labels get bonus)
                labels = getattr(task, 'labels', [])
                urgent_labels = {"critical", "urgent", "blocker", "hotfix", "mvp"}
                label_score = 10 if any(l.lower() in urgent_labels for l in labels) else 0

                total_score = priority_score + blocker_score + age_score + label_score

                task_scores.append({
                    "task": task,
                    "value_score": total_score,
                    "priority_score": priority_score,
                    "blocker_score": blocker_score,
                    "age_score": age_score,
                    "label_score": label_score,
                })

            # Sort by value score (descending)
            task_scores.sort(key=lambda x: x["value_score"], reverse=True)

            # Generate suggestions
            suggestions = []
            for i, task_data in enumerate(task_scores[:count]):
                task = task_data["task"]
                score = task_data["value_score"]

                # Determine required tier from task priority
                required_tier = tier_mapping.get(task.priority, "standard")

                # Find matching workers
                matching_workers = [
                    w for w in available_workers
                    if self._get_worker_tier(w.model) == required_tier
                ]

                # If no matching workers, use any available worker
                if not matching_workers:
                    matching_workers = available_workers

                worker_suggestions = [
                    {
                        "worker_id": w.id,
                        "model": w.model,
                        "tier": self._get_worker_tier(w.model),
                    }
                    for w in matching_workers[:3]  # Top 3 workers per task
                ]

                suggestions.append({
                    "rank": i + 1,
                    "task_id": task.id,
                    "task_title": task.title,
                    "priority": task.priority,
                    "value_score": score,
                    "score_breakdown": {
                        "priority": task_data["priority_score"],
                        "blockers": task_data["blocker_score"],
                        "age": task_data["age_score"],
                        "labels": task_data["label_score"],
                    },
                    "required_tier": required_tier,
                    "suggested_workers": worker_suggestions,
                    "labels": getattr(task, 'labels', []),
                    "age_days": task_data["age_score"] // 3 if task_data["age_score"] > 0 else 0,
                })

            return create_success_result(
                "suggest_assignments",
                f"Generated {len(suggestions)} task assignment suggestions",
                {
                    "suggestions": suggestions,
                    "total_tasks": len(ready_tasks),
                    "available_workers": len(available_workers),
                    "worker_filter": worker_filter,
                }
            )
        except Exception as e:
            return create_error_result("suggest_assignments", f"Failed to suggest assignments: {e}")

    def _get_priority_score(self, priority: str | TaskPriority) -> int:
        """Get priority score for value calculation (0-40 points)"""
        if isinstance(priority, TaskPriority):
            priority = priority.value

        priority_scores = {
            "0": 40,  # P0 - Critical
            "1": 30,  # P1 - High
            "2": 20,  # P2 - Medium
            "3": 10,  # P3 - Low
            "4": 5,   # P4 - Backlog
        }
        return priority_scores.get(str(priority), 15)

    def _get_worker_tier(self, model: str) -> str:
        """Determine worker tier based on model"""
        model_lower = model.lower()

        # Premium models
        if any(m in model_lower for m in ["opus", "gpt-4", "claude-opus"]):
            return "premium"
        if any(m in model_lower for m in ["sonnet", "claude-sonnet", "gpt-4o"]):
            return "premium"

        # Standard models
        if any(m in model_lower for m in ["qwen", "llama", "mistral"]):
            return "standard"

        # Budget models
        if any(m in model_lower for m in ["haiku", "claude-haiku"]):
            return "budget"

        # Default to standard
        return "standard"

    # =============================================================================
    # Cost & Analytics Tool Callbacks
    # =============================================================================

    def _tool_show_costs(self, period: str = "today", breakdown: str | None = None) -> ToolCallResult:
        """Tool callback for show_costs - displays cost analysis"""
        try:
            # Switch to costs view
            self.action_switch_view("costs")

            # Get cost data from cost tracker if available
            if self._cost_tracker is not None:
                # Determine time period
                if period in ("today", "24h", "last_24h"):
                    summary = self._cost_tracker.get_costs_last_24h()
                elif period == "week":
                    from datetime import timedelta
                    start = datetime.now() - timedelta(days=7)
                    summary = self._cost_tracker.get_costs_period(start, datetime.now())
                elif period == "month":
                    from datetime import timedelta
                    start = datetime.now() - timedelta(days=30)
                    summary = self._cost_tracker.get_costs_period(start, datetime.now())
                else:
                    summary = self._cost_tracker.get_costs_today()

                # Build cost data based on breakdown
                if breakdown == "by_model":
                    cost_data = [
                        {
                            "model": model,
                            "cost": data["cost"],
                            "tokens": data["tokens"],
                            "requests": data["requests"],
                            "avg_cost_per_request": data["avg_cost_per_request"],
                        }
                        for model, data in summary.by_model.items()
                    ]
                elif breakdown == "by_worker":
                    cost_data = [
                        {
                            "worker_id": worker_id,
                            "cost": data["cost"],
                            "tokens": data["tokens"],
                            "requests": data["requests"],
                            "models": data["models"],
                        }
                        for worker_id, data in summary.by_worker.items()
                    ]
                else:
                    cost_data = [
                        {
                            "period": f"{summary.period_start.strftime('%Y-%m-%d %H:%M')} - {summary.period_end.strftime('%Y-%m-%d %H:%M')}",
                            "total_cost": summary.total_cost,
                            "total_requests": summary.total_requests,
                            "total_tokens": summary.total_tokens,
                        }
                    ]

                return create_success_result(
                    "show_costs",
                    f"Showing costs for {period}" + (f" by {breakdown}" if breakdown else ""),
                    {
                        "period": period,
                        "breakdown": breakdown,
                        "costs": cost_data,
                        "total_cost": summary.total_cost,
                        "total_requests": summary.total_requests,
                    }
                )
            else:
                # Fallback to stored cost data
                cost_data = [
                    {
                        "model": c.model,
                        "cost": c.cost,
                        "tokens": c.tokens,
                        "requests": c.requests,
                    }
                    for c in self._costs_store
                ]

                return create_success_result(
                    "show_costs",
                    f"Showing costs for {period}",
                    {
                        "period": period,
                        "breakdown": breakdown,
                        "costs": cost_data
                    }
                )
        except Exception as e:
            return create_error_result("show_costs", f"Failed to show costs: {e}")

    def _tool_optimize_routing(self) -> ToolCallResult:
        """Tool callback for optimize_routing - runs cost optimization"""
        try:
            # Placeholder for optimization logic
            # In real implementation, this would analyze routing and update config
            return create_success_result(
                "optimize_routing",
                "Cost optimization analysis complete. No changes recommended.",
                {
                    "recommendations": [],
                    "potential_savings": 0.0
                }
            )
        except Exception as e:
            return create_error_result("optimize_routing", f"Failed to optimize routing: {e}")

    def _tool_forecast_costs(self, days: int = 30) -> ToolCallResult:
        """Tool callback for forecast_costs - forecasts future costs"""
        try:
            # Get historical data from cost tracker
            if self._cost_tracker is not None:
                from datetime import timedelta
                start = datetime.now() - timedelta(days=7)  # Use last 7 days for forecast
                summary = self._cost_tracker.get_costs_period(start, datetime.now())

                # Calculate daily average from historical data
                days_in_period = (summary.period_end - summary.period_start).days or 1
                daily_avg = summary.total_cost / days_in_period
                forecast = daily_avg * days

                return create_success_result(
                    "forecast_costs",
                    f"Forecasted cost for {days} days: ${forecast:.2f}",
                    {
                        "days": days,
                        "forecast": forecast,
                        "daily_average": daily_avg,
                        "historical_days": days_in_period,
                        "historical_total": summary.total_cost,
                    }
                )
            else:
                # Fallback to stored cost data
                daily_avg = sum(c.cost for c in self._costs_store) / max(len(self._costs_store), 1)
                forecast = daily_avg * days

                return create_success_result(
                    "forecast_costs",
                    f"Forecasted cost for {days} days: ${forecast:.2f}",
                    {
                        "days": days,
                        "forecast": forecast,
                        "daily_average": daily_avg
                    }
                )
        except Exception as e:
            return create_error_result("forecast_costs", f"Failed to forecast costs: {e}")

    def _tool_show_metrics(self, metric_type: str = "all", period: str = "today") -> ToolCallResult:
        """Tool callback for show_metrics - displays performance metrics"""
        try:
            # Switch to metrics view
            self.action_switch_view("metrics")

            metrics_data = {}
            if self._metrics_store:
                metrics_data = {
                    "throughput": self._metrics_store.throughput,
                    "latency": self._metrics_store.latency,
                    "success_rate": self._metrics_store.success_rate,
                }

            return create_success_result(
                "show_metrics",
                f"Showing {metric_type} metrics for {period}",
                {
                    "metric_type": metric_type,
                    "period": period,
                    "metrics": metrics_data
                }
            )
        except Exception as e:
            return create_error_result("show_metrics", f"Failed to show metrics: {e}")

    # =============================================================================
    # Data Export Tool Callbacks
    # =============================================================================

    def _tool_export_logs(self, format: str = "json", period: str = "today") -> ToolCallResult:
        """Tool callback for export_logs - exports activity logs"""
        try:
            import tempfile
            from datetime import datetime

            # Filter logs by period
            logs = self._logs_store

            # Generate output file
            timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
            with tempfile.NamedTemporaryFile(
                mode="w",
                suffix=f".{format}",
                prefix=f"forge_logs_{timestamp}_",
                delete=False
            ) as f:
                if format == "json":
                    import json
                    log_dicts = [
                        {
                            "timestamp": l.timestamp.isoformat(),
                            "level": l.level,
                            "message": l.message,
                            "icon": l.icon,
                        }
                        for l in logs
                    ]
                    json.dump(log_dicts, f, indent=2)
                elif format == "csv":
                    import csv
                    if logs:
                        fieldnames = ["timestamp", "level", "message", "icon"]
                        writer = csv.DictWriter(f, fieldnames=fieldnames)
                        writer.writeheader()
                        for log in logs:
                            writer.writerow({
                                "timestamp": log.timestamp.isoformat(),
                                "level": log.level,
                                "message": log.message,
                                "icon": log.icon,
                            })
                else:  # txt
                    for log in logs:
                        f.write(f"{log.timestamp} [{log.level}] {log.icon} {log.message}\n")
                output_path = f.name

            return create_success_result(
                "export_logs",
                f"Exported {len(logs)} log entries to {output_path}",
                {
                    "file": output_path,
                    "format": format,
                    "period": period,
                    "count": len(logs)
                }
            )
        except Exception as e:
            return create_error_result("export_logs", f"Failed to export logs: {e}")

    def _tool_export_metrics(self, metric_type: str = "all", format: str = "json") -> ToolCallResult:
        """Tool callback for export_metrics - exports metrics data"""
        try:
            import tempfile
            from datetime import datetime

            # Gather metrics data
            metrics_data = {
                "workers": [
                    {
                        "id": w.id,
                        "model": w.model,
                        "status": w.status,
                    }
                    for w in self._workers_store
                ],
                "tasks": [
                    {
                        "id": t.id,
                        "title": t.title,
                        "priority": t.priority,
                        "status": t.status,
                    }
                    for t in self._tasks_store
                ],
            }

            if self._metrics_store:
                metrics_data["performance"] = {
                    "throughput": self._metrics_store.throughput,
                    "latency": self._metrics_store.latency,
                    "success_rate": self._metrics_store.success_rate,
                }

            # Generate output file
            timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
            with tempfile.NamedTemporaryFile(
                mode="w",
                suffix=f".{format}",
                prefix=f"forge_metrics_{timestamp}_",
                delete=False
            ) as f:
                if format == "json":
                    import json
                    json.dump(metrics_data, f, indent=2)
                elif format == "csv":
                    import csv
                    # Flatten for CSV export
                    flat_data = []
                    for category, items in metrics_data.items():
                        if isinstance(items, list):
                            for item in items:
                                item["category"] = category
                                flat_data.append(item)
                    if flat_data:
                        writer = csv.DictWriter(f, fieldnames=flat_data[0].keys())
                        writer.writeheader()
                        writer.writerows(flat_data)
                output_path = f.name

            return create_success_result(
                "export_metrics",
                f"Exported metrics to {output_path}",
                {
                    "file": output_path,
                    "format": format,
                    "metric_type": metric_type
                }
            )
        except Exception as e:
            return create_error_result("export_metrics", f"Failed to export metrics: {e}")

    def _tool_screenshot(self, panel: str = "all") -> ToolCallResult:
        """Tool callback for screenshot - takes a screenshot"""
        try:
            import tempfile
            from datetime import datetime

            # Textual doesn't have built-in screenshot, so we export state
            timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
            output_path = f"/tmp/forge_screenshot_{timestamp}.txt"

            with open(output_path, "w") as f:
                f.write(f"FORGE Screenshot - {timestamp}\n")
                f.write(f"Current View: {self._current_view.value}\n")
                f.write(f"Workers: {len(self._workers_store)}\n")
                f.write(f"Tasks: {len(self._tasks_store)}\n")
                f.write("\n=== Workers ===\n")
                for worker in self._workers_store:
                    f.write(f"  {worker.id}: {worker.status} - {worker.current_task or 'No task'}\n")
                f.write("\n=== Tasks ===\n")
                for task in self._tasks_store[:10]:  # First 10 tasks
                    f.write(f"  {task.id}: [{task.priority}] {task.title} - {task.status}\n")

            return create_success_result(
                "screenshot",
                f"Screenshot saved to {output_path}",
                {
                    "file": output_path,
                    "panel": panel
                }
            )
        except Exception as e:
            return create_error_result("screenshot", f"Failed to take screenshot: {e}")

    # =============================================================================
    # Configuration Tool Callbacks
    # =============================================================================

    def _tool_set_config(self, key: str, value: str) -> ToolCallResult:
        """Tool callback for set_config - updates configuration"""
        try:
            # In real implementation, this would update config file
            # For now, just acknowledge
            return create_success_result(
                "set_config",
                f"Set {key} = {value}",
                {
                    "key": key,
                    "value": value
                }
            )
        except Exception as e:
            return create_error_result("set_config", f"Failed to set config: {e}")

    def _tool_get_config(self, key: str | None = None) -> ToolCallResult:
        """Tool callback for get_config - views configuration"""
        try:
            # In real implementation, this would read from config file
            # For now, return sample config
            config = {
                "default_model": "sonnet",
                "max_workers": 10,
                "workspace": str(Path.cwd()),
            }

            if key:
                value = config.get(key)
                return create_success_result(
                    "get_config",
                    f"{key} = {value}",
                    {"key": key, "value": value}
                )
            else:
                return create_success_result(
                    "get_config",
                    "Current configuration",
                    {"config": config}
                )
        except Exception as e:
            return create_error_result("get_config", f"Failed to get config: {e}")

    def _tool_save_layout(self, name: str) -> ToolCallResult:
        """Tool callback for save_layout - saves dashboard layout"""
        try:
            import json
            from pathlib import Path

            layout = {
                "view": self._current_view.value,
                "split_left": self._split_left,
                "split_right": self._split_right,
            }

            layout_dir = Path.home() / ".forge" / "layouts"
            layout_dir.mkdir(parents=True, exist_ok=True)
            layout_file = layout_dir / f"{name}.json"

            layout_file.write_text(json.dumps(layout, indent=2))

            return create_success_result(
                "save_layout",
                f"Saved layout '{name}'",
                {
                    "name": name,
                    "file": str(layout_file)
                }
            )
        except Exception as e:
            return create_error_result("save_layout", f"Failed to save layout: {e}")

    def _tool_load_layout(self, name: str) -> ToolCallResult:
        """Tool callback for load_layout - loads dashboard layout"""
        try:
            import json
            from pathlib import Path

            layout_file = Path.home() / ".forge" / "layouts" / f"{name}.json"

            if not layout_file.exists():
                return create_error_result("load_layout", f"Layout not found: {name}")

            layout = json.loads(layout_file.read_text())

            # Apply layout
            if "view" in layout:
                self.action_switch_view(layout["view"])
            if "split_left" in layout and "split_right" in layout:
                self.action_split_view(layout["split_left"], layout["split_right"])

            return create_success_result(
                "load_layout",
                f"Loaded layout '{name}'",
                {
                    "name": name,
                    "layout": layout
                }
            )
        except Exception as e:
            return create_error_result("load_layout", f"Failed to load layout: {e}")

    # =============================================================================
    # Help & Discovery Tool Callbacks
    # =============================================================================

    def _tool_help(self, topic: str | None = None) -> ToolCallResult:
        """Tool callback for help - displays help information"""
        try:
            help_text = f"FORGE Help - {topic or 'All Topics'}\n\n"

            if topic == "spawning" or topic is None:
                help_text += "Spawning Workers:\n"
                help_text += "  - Use 'spawn_worker' tool to spawn new AI workers\n"
                help_text += "  - Specify model (sonnet, opus, haiku, gpt4, qwen)\n"
                help_text += "  - Specify count (1-10 workers)\n\n"

            if topic == "costs" or topic is None:
                help_text += "Cost Management:\n"
                help_text += "  - Use 'show_costs' to view spending\n"
                help_text += "  - Use 'optimize_routing' to reduce costs\n"
                help_text += "  - Use 'forecast_costs' to predict future spending\n\n"

            if topic == "tasks" or topic is None:
                help_text += "Task Management:\n"
                help_text += "  - Use 'filter_tasks' to find specific tasks\n"
                help_text += "  - Use 'create_task' to add new tasks\n"
                help_text += "  - Use 'assign_task' to assign tasks to workers\n\n"

            if topic == "tools" or topic is None:
                help_text += f"Available Tools: Multiple tools across various categories\n"
                help_text += "  - Use 'list_capabilities' to see all tools\n"
                help_text += "  - Use 'search_docs' to find specific information\n\n"

            return create_success_result(
                "help",
                f"Help for {topic or 'all topics'}",
                {
                    "topic": topic,
                    "help_text": help_text.strip()
                }
            )
        except Exception as e:
            return create_error_result("help", f"Failed to show help: {e}")

    def _tool_search_docs(self, query: str) -> ToolCallResult:
        """Tool callback for search_docs - searches documentation"""
        try:
            from pathlib import Path

            # Search for query in docs directory
            docs_dir = Path(__file__).parent.parent.parent / "docs"
            results = []

            if docs_dir.exists():
                for doc_file in docs_dir.rglob("*.md"):
                    content = doc_file.read_text().lower()
                    if query.lower() in content:
                        results.append({
                            "file": str(doc_file.relative_to(docs_dir)),
                            "matches": content.count(query.lower())
                        })

            results.sort(key=lambda r: r["matches"], reverse=True)

            return create_success_result(
                "search_docs",
                f"Found {len(results)} result(s) for '{query}'",
                {
                    "query": query,
                    "results": results[:10]  # Top 10 results
                }
            )
        except Exception as e:
            return create_error_result("search_docs", f"Failed to search docs: {e}")

    def _tool_list_capabilities(self) -> ToolCallResult:
        """Tool callback for list_capabilities - lists all available tools"""
        try:
            from collections import Counter

            # Get tools by category
            category_counts = Counter()
            tool_list = []

            from forge.tool_definitions import get_all_tools
            all_tools = get_all_tools()

            for tool in all_tools:
                category_counts[tool.category.value] += 1
                tool_list.append({
                    "name": tool.name,
                    "description": tool.description,
                    "category": tool.category.value,
                    "requires_confirmation": tool.requires_confirmation,
                })

            return create_success_result(
                "list_capabilities",
                f"FORGE has {len(all_tools)} tools across {len(category_counts)} categories",
                {
                    "total_tools": len(all_tools),
                    "categories": dict(category_counts),
                    "tools": tool_list
                }
            )
        except Exception as e:
            return create_error_result("list_capabilities", f"Failed to list capabilities: {e}")

    # =============================================================================
    # Notification Tool Callbacks
    # =============================================================================

    def _tool_show_notification(self, message: str, level: str = "info") -> ToolCallResult:
        """Tool callback for show_notification - displays a notification"""
        try:
            # In Textual, we could use a notification overlay
            # For now, log to the logs store
            from datetime import datetime

            log_entry = LogEntry(
                timestamp=datetime.now(),
                level=level.upper(),
                source="notification",
                message=message
            )
            self._logs_store.append(log_entry)

            return create_success_result(
                "show_notification",
                f"Notification: {message}",
                {
                    "message": message,
                    "level": level
                }
            )
        except Exception as e:
            return create_error_result("show_notification", f"Failed to show notification: {e}")

    def _tool_show_warning(self, message: str, details: str | None = None) -> ToolCallResult:
        """Tool callback for show_warning - displays a warning"""
        try:
            from datetime import datetime

            log_entry = LogEntry(
                timestamp=datetime.now(),
                level="WARNING",
                source="warning",
                message=f"{message} - {details}" if details else message
            )
            self._logs_store.append(log_entry)

            return create_success_result(
                "show_warning",
                f"Warning: {message}",
                {
                    "message": message,
                    "details": details
                }
            )
        except Exception as e:
            return create_error_result("show_warning", f"Failed to show warning: {e}")

    def _tool_ask_user(self, question: str, options: list[str] | None = None) -> ToolCallResult:
        """Tool callback for ask_user - prompts the user for input"""
        try:
            # This would require implementing a modal dialog in Textual
            # For now, return that user needs to be prompted
            return create_success_result(
                "ask_user",
                f"Question for user: {question}",
                {
                    "question": question,
                    "options": options or ["Yes", "No"],
                    "requires_input": True
                }
            )
        except Exception as e:
            return create_error_result("ask_user", f"Failed to ask user: {e}")

    def _tool_highlight_beads(self, bead_ids: list[str], reason: str | None = None) -> ToolCallResult:
        """Tool callback for highlight_beads - highlights specific beads"""
        try:
            # In real implementation, this would update the tasks panel
            # to highlight the specified beads
            return create_success_result(
                "highlight_beads",
                f"Highlighted {len(bead_ids)} bead(s)",
                {
                    "bead_ids": bead_ids,
                    "reason": reason
                }
            )
        except Exception as e:
            return create_error_result("highlight_beads", f"Failed to highlight beads: {e}")

    # =============================================================================
    # System Tool Callbacks
    # =============================================================================

    def _tool_get_status(self, component: str = "all") -> ToolCallResult:
        """Tool callback for get_status - gets system status"""
        try:
            status = {}

            if component in ["all", "workers"]:
                status["workers"] = {
                    "total": len(self._workers_store),
                    "active": sum(1 for w in self._workers_store if w.status == "active"),
                    "idle": sum(1 for w in self._workers_store if w.status == "idle"),
                    "failed": sum(1 for w in self._workers_store if w.status == "failed"),
                }

            if component in ["all", "tasks"]:
                status["tasks"] = {
                    "total": len(self._tasks_store),
                    "open": sum(1 for t in self._tasks_store if t.status == "open"),
                    "in_progress": sum(1 for t in self._tasks_store if t.status == "in_progress"),
                    "blocked": sum(1 for t in self._tasks_store if t.status == "blocked"),
                    "completed": sum(1 for t in self._tasks_store if t.status == "completed"),
                }

            if component in ["all", "system"]:
                status["system"] = {
                    "workspace": str(Path.cwd()),
                    "current_view": self._current_view.value,
                }

            return create_success_result(
                "get_status",
                f"Status for {component}",
                {
                    "component": component,
                    "status": status
                }
            )
        except Exception as e:
            return create_error_result("get_status", f"Failed to get status: {e}")

    def _tool_refresh(self, scope: str = "current") -> ToolCallResult:
        """Tool callback for refresh - refreshes data"""
        try:
            self.action_refresh()

            return create_success_result(
                "refresh",
                f"Refreshed {scope}",
                {"scope": scope}
            )
        except Exception as e:
            return create_error_result("refresh", f"Failed to refresh: {e}")

    def _tool_ping_worker(self, worker_id: str) -> ToolCallResult:
        """Tool callback for ping_worker - checks if worker is responsive"""
        try:
            worker = next((w for w in self._workers_store if w.id == worker_id), None)
            if not worker:
                return create_error_result("ping_worker", f"Worker not found: {worker_id}")

            # In real implementation, this would actually ping the worker
            is_responsive = worker.status in ["idle", "active"]

            return create_success_result(
                "ping_worker",
                f"Worker {worker_id} is {'responsive' if is_responsive else 'not responsive'}",
                {
                    "worker_id": worker_id,
                    "responsive": is_responsive,
                    "status": worker.status
                }
            )
        except Exception as e:
            return create_error_result("ping_worker", f"Failed to ping worker: {e}")

    def _tool_get_worker_info(self, worker_id: str) -> ToolCallResult:
        """Tool callback for get_worker_info - gets detailed worker information"""
        try:
            worker = next((w for w in self._workers_store if w.id == worker_id), None)
            if not worker:
                return create_error_result("get_worker_info", f"Worker not found: {worker_id}")

            worker_info = {
                "id": worker.id,
                "model": worker.model,
                "status": worker.status,
                "workspace": worker.workspace,
                "current_task": worker.current_task,
                "created_at": worker.created_at.isoformat() if hasattr(worker, "created_at") else None,
            }

            return create_success_result(
                "get_worker_info",
                f"Worker info for {worker_id}",
                {
                    "worker_id": worker_id,
                    "info": worker_info
                }
            )
        except Exception as e:
            return create_error_result("get_worker_info", f"Failed to get worker info: {e}")

    def _tool_pause_worker(self, worker_id: str) -> ToolCallResult:
        """Tool callback for pause_worker - pauses a worker"""
        try:
            worker = next((w for w in self._workers_store if w.id == worker_id), None)
            if not worker:
                return create_error_result("pause_worker", f"Worker not found: {worker_id}")

            worker.status = "paused"

            return create_success_result(
                "pause_worker",
                f"Paused worker {worker_id}",
                {"worker_id": worker_id}
            )
        except Exception as e:
            return create_error_result("pause_worker", f"Failed to pause worker: {e}")

    def _tool_resume_worker(self, worker_id: str) -> ToolCallResult:
        """Tool callback for resume_worker - resumes a paused worker"""
        try:
            worker = next((w for w in self._workers_store if w.id == worker_id), None)
            if not worker:
                return create_error_result("resume_worker", f"Worker not found: {worker_id}")

            worker.status = "idle"  # Resume to idle state

            return create_success_result(
                "resume_worker",
                f"Resumed worker {worker_id}",
                {"worker_id": worker_id}
            )
        except Exception as e:
            return create_error_result("resume_worker", f"Failed to resume worker: {e}")

    # =============================================================================
    # Workspace Tool Callbacks
    # =============================================================================

    def _tool_switch_workspace(self, path: str) -> ToolCallResult:
        """Tool callback for switch_workspace - switches to a different workspace"""
        try:
            workspace_path = Path(path).expanduser()

            if self._workspace_manager is None:
                return create_error_result("switch_workspace", "Workspace manager not initialized")

            # Switch workspace using manager
            import asyncio
            metadata = asyncio.run(self._workspace_manager.switch_workspace(workspace_path))

            if metadata is None:
                # Try to add the workspace
                metadata = asyncio.run(self._workspace_manager.add_workspace(workspace_path))
                if metadata is None:
                    return create_error_result("switch_workspace", f"Workspace not found: {path}")

                # Now switch to it
                metadata = asyncio.run(self._workspace_manager.switch_workspace(workspace_path))

            # Refresh workspace-dependent data
            self._refresh_workspace_data()

            return create_success_result(
                "switch_workspace",
                f"Switched to workspace: {metadata.name}",
                {"workspace": str(workspace_path), "name": metadata.name}
            )
        except Exception as e:
            return create_error_result("switch_workspace", f"Failed to switch workspace: {e}")

    def _refresh_workspace_data(self) -> None:
        """Refresh data that depends on the current workspace"""
        if self._workspace_manager is None:
            return

        # Get active workspace
        active_ws = self._workspace_manager.get_active_workspace()
        if active_ws is None:
            return

        # Update bead workspaces for the watcher
        self._bead_workspaces = [active_ws.path]

        # Restart bead watcher with new workspace
        # (This would be done in a more sophisticated way in production)
        pass

    def _tool_list_workspaces(self, filter: str = "all") -> ToolCallResult:
        """Tool callback for list_workspaces - lists available workspaces"""
        try:
            if self._workspace_manager is None:
                return create_error_result("list_workspaces", "Workspace manager not initialized")

            workspaces = self._workspace_manager.get_all_workspaces()

            # Convert to dict format
            workspace_list = []
            for ws in workspaces:
                # Apply filter
                if filter == "active":
                    if ws.status != WorkspaceStatus.ACTIVE:
                        continue
                elif filter == "inactive":
                    if ws.status == WorkspaceStatus.ACTIVE:
                        continue

                workspace_list.append({
                    "path": str(ws.path),
                    "name": ws.name,
                    "status": ws.status.value,
                    "active": ws.status == WorkspaceStatus.ACTIVE,
                    "bead_count": ws.bead_count,
                    "worker_count": ws.worker_count,
                    "total_cost": ws.total_cost,
                })

            return create_success_result(
                "list_workspaces",
                f"Found {len(workspace_list)} workspace(s)",
                {
                    "workspaces": workspace_list,
                    "filter": filter
                }
            )
        except Exception as e:
            return create_error_result("list_workspaces", f"Failed to list workspaces: {e}")

    def _tool_create_workspace(self, path: str, template: str = "empty") -> ToolCallResult:
        """Tool callback for create_workspace - creates a new workspace"""
        try:
            if self._workspace_manager is None:
                return create_error_result("create_workspace", "Workspace manager not initialized")

            workspace_path = Path(path).expanduser()

            # Create directory
            workspace_path.mkdir(parents=True, exist_ok=True)

            # Initialize .beads directory
            beads_dir = workspace_path / ".beads"
            beads_dir.mkdir(exist_ok=True)

            # Add to workspace manager
            import asyncio
            metadata = asyncio.run(self._workspace_manager.add_workspace(workspace_path))

            if metadata is None:
                return create_error_result("create_workspace", f"Failed to create workspace: {path}")

            return create_success_result(
                "create_workspace",
                f"Created workspace: {metadata.name}",
                {
                    "workspace": str(workspace_path),
                    "name": metadata.name,
                    "template": template
                }
            )
        except Exception as e:
            return create_error_result("create_workspace", f"Failed to create workspace: {e}")

    def _tool_get_workspace_info(self) -> ToolCallResult:
        """Tool callback for get_workspace_info - gets current workspace info"""
        try:
            if self._workspace_manager is None:
                return create_error_result("get_workspace_info", "Workspace manager not initialized")

            active_ws = self._workspace_manager.get_active_workspace()

            if active_ws is None:
                # Try to use current directory
                workspace_path = Path.cwd()
                active_ws = self._workspace_manager.get_workspace(workspace_path)

                if active_ws is None:
                    # Return basic info even if not tracked
                    info = {
                        "path": str(workspace_path),
                        "name": workspace_path.name,
                        "tracked": False,
                        "has_beads": (workspace_path / ".beads").exists(),
                    }
                    return create_success_result(
                        "get_workspace_info",
                        f"Workspace info (untracked): {workspace_path.name}",
                        {"info": info}
                    )

            info = {
                "path": str(active_ws.path),
                "name": active_ws.name,
                "status": active_ws.status.value,
                "tracked": True,
                "has_beads": active_ws.bead_count > 0,
                "bead_count": active_ws.bead_count,
                "open_beads": active_ws.open_beads,
                "in_progress_beads": active_ws.in_progress_beads,
                "closed_beads": active_ws.closed_beads,
                "worker_count": active_ws.worker_count,
                "total_cost": active_ws.total_cost,
                "completion_rate": active_ws.completion_rate,
            }

            return create_success_result(
                "get_workspace_info",
                f"Workspace: {active_ws.name}",
                {"info": info}
            )
        except Exception as e:
            return create_error_result("get_workspace_info", f"Failed to get workspace info: {e}")

    # =============================================================================
    # Analytics Tool Callbacks
    # =============================================================================

    def _tool_show_throughput(self, period: str = "today") -> ToolCallResult:
        """Tool callback for show_throughput - displays throughput metrics"""
        try:
            self.action_switch_view("metrics")

            # Calculate throughput
            completed_tasks = sum(1 for t in self._tasks_store if t.status == "completed")
            throughput = completed_tasks  # Tasks per period

            return create_success_result(
                "show_throughput",
                f"Throughput: {throughput} tasks/{period}",
                {
                    "period": period,
                    "throughput": throughput,
                    "completed_tasks": completed_tasks
                }
            )
        except Exception as e:
            return create_error_result("show_throughput", f"Failed to show throughput: {e}")

    def _tool_show_latency(self, period: str = "today") -> ToolCallResult:
        """Tool callback for show_latency - displays latency metrics"""
        try:
            self.action_switch_view("metrics")

            # Calculate average latency (placeholder)
            avg_latency = 2.5  # seconds

            return create_success_result(
                "show_latency",
                f"Average latency: {avg_latency}s",
                {
                    "period": period,
                    "avg_latency": avg_latency
                }
            )
        except Exception as e:
            return create_error_result("show_latency", f"Failed to show latency: {e}")

    def _tool_show_success_rate(self, period: str = "today") -> ToolCallResult:
        """Tool callback for show_success_rate - displays success rate metrics"""
        try:
            self.action_switch_view("metrics")

            # Calculate success rate
            total = len(self._tasks_store)
            completed = sum(1 for t in self._tasks_store if t.status == "completed")
            success_rate = (completed / total * 100) if total > 0 else 0

            return create_success_result(
                "show_success_rate",
                f"Success rate: {success_rate:.1f}%",
                {
                    "period": period,
                    "success_rate": success_rate,
                    "completed": completed,
                    "total": total
                }
            )
        except Exception as e:
            return create_error_result("show_success_rate", f"Failed to show success rate: {e}")

    def _tool_show_worker_efficiency(self, by_model: bool = True) -> ToolCallResult:
        """Tool callback for show_worker_efficiency - displays worker efficiency"""
        try:
            efficiency = []

            if by_model:
                # Group by model
                from collections import defaultdict
                model_stats = defaultdict(lambda: {"completed": 0, "total": 0})

                for worker in self._workers_store:
                    model_stats[worker.model]["total"] += 1
                    if worker.status in ["idle", "active"]:
                        model_stats[worker.model]["completed"] += 1

                for model, stats in model_stats.items():
                    efficiency.append({
                        "model": model,
                        "efficiency": stats["completed"] / stats["total"] * 100 if stats["total"] > 0 else 0,
                        "completed": stats["completed"],
                        "total": stats["total"],
                    })

            return create_success_result(
                "show_worker_efficiency",
                f"Worker efficiency by model",
                {
                    "by_model": by_model,
                    "efficiency": efficiency
                }
            )
        except Exception as e:
            return create_error_result("show_worker_efficiency", f"Failed to show worker efficiency: {e}")

    def _tool_show_task_distribution(self) -> ToolCallResult:
        """Tool callback for show_task_distribution - displays task distribution"""
        try:
            from collections import Counter

            priority_dist = Counter(t.priority for t in self._tasks_store)
            status_dist = Counter(t.status for t in self._tasks_store)

            return create_success_result(
                "show_task_distribution",
                "Task distribution across priorities and statuses",
                {
                    "by_priority": dict(priority_dist),
                    "by_status": dict(status_dist),
                }
            )
        except Exception as e:
            return create_error_result("show_task_distribution", f"Failed to show task distribution: {e}")

    def _tool_show_trends(self, metric: str, period: str = "this_week") -> ToolCallResult:
        """Tool callback for show_trends - displays metric trends over time"""
        try:
            self.action_switch_view("metrics")

            # Placeholder trend data
            trend_data = [
                {"date": "2026-02-01", "value": 10},
                {"date": "2026-02-02", "value": 15},
                {"date": "2026-02-03", "value": 12},
                {"date": "2026-02-04", "value": 20},
                {"date": "2026-02-05", "value": 18},
            ]

            return create_success_result(
                "show_trends",
                f"Trends for {metric} over {period}",
                {
                    "metric": metric,
                    "period": period,
                    "trend": trend_data
                }
            )
        except Exception as e:
            return create_error_result("show_trends", f"Failed to show trends: {e}")

    def _tool_analyze_bottlenecks(self) -> ToolCallResult:
        """Tool callback for analyze_bottlenecks - analyzes workflow bottlenecks"""
        try:
            bottlenecks = []

            # Check for idle workers with pending tasks
            idle_workers = sum(1 for w in self._workers_store if w.status == "idle")
            pending_tasks = sum(1 for t in self._tasks_store if t.status in ["open", "in_progress"])

            if idle_workers > 0 and pending_tasks > 0:
                bottlenecks.append({
                    "type": "underutilization",
                    "severity": "low",
                    "description": f"{idle_workers} idle workers with {pending_tasks} pending tasks",
                })

            # Check for long-running tasks
            for task in self._tasks_store:
                if task.status == "in_progress":
                    # Check if task has been running too long
                    bottlenecks.append({
                        "type": "long_running_task",
                        "severity": "medium",
                        "description": f"Task {task.id} has been in progress",
                    })

            return create_success_result(
                "analyze_bottlenecks",
                f"Found {len(bottlenecks)} potential bottleneck(s)",
                {
                    "bottlenecks": bottlenecks
                }
            )
        except Exception as e:
            return create_error_result("analyze_bottlenecks", f"Failed to analyze bottlenecks: {e}")

    @property
    def forge_workers(self) -> list[Worker]:
        """Get workers list"""
        return self._workers_store

    @forge_workers.setter
    def forge_workers(self, value: list[Worker]) -> None:
        """Set workers list and trigger update"""
        self._workers_store = value
        if self._workers_panel:
            self._workers_panel.worker_list = value

    @property
    def tasks(self) -> list[Task]:
        """Get tasks list"""
        return self._tasks_store

    @tasks.setter
    def tasks(self, value: list[Task]) -> None:
        """Set tasks list and trigger update"""
        self._tasks_store = value
        if self._tasks_panel:
            self._tasks_panel.tasks = value

    @property
    def subscriptions(self) -> list[Subscription]:
        """Get subscriptions list"""
        return self._subscriptions_store

    @subscriptions.setter
    def subscriptions(self, value: list[Subscription]) -> None:
        """Set subscriptions list and trigger update"""
        self._subscriptions_store = value
        if self._costs_panel:
            self._costs_panel.subscriptions = value

    @property
    def costs(self) -> list[CostEntry]:
        """Get costs list"""
        return self._costs_store

    @costs.setter
    def costs(self, value: list[CostEntry]) -> None:
        """Set costs list and trigger update"""
        self._costs_store = value
        if self._costs_panel:
            self._costs_panel.costs = value

    @property
    def metrics(self) -> MetricData | None:
        """Get metrics"""
        return self._metrics_store

    @metrics.setter
    def metrics(self, value: MetricData | None) -> None:
        """Set metrics and trigger update"""
        self._metrics_store = value
        if self._metrics_panel:
            self._metrics_panel.metrics = value

    @property
    def logs(self) -> list[LogEntry]:
        """Get logs list"""
        return self._logs_store

    @logs.setter
    def logs(self, value: list[LogEntry]) -> None:
        """Set logs list and trigger update"""
        self._logs_store = value
        if self._logs_panel:
            self._logs_panel.logs = value

    def _initialize_cost_tracker(self) -> None:
        """Initialize the cost tracker for API call cost tracking"""
        try:
            self._cost_tracker = _get_cost_tracker()
        except Exception as e:
            # Cost tracker initialization failed - continue without it
            print(f"Warning: Could not initialize cost tracker: {e}")
            self._cost_tracker = None

    def _load_cost_data(self) -> None:
        """Load cost data from the cost tracker"""
        if self._cost_tracker is None:
            # Use sample data if cost tracker is not available
            now = datetime.now()
            self._costs_store = [
                CostEntry("Sonnet 4.5", 24, 347000, 4.17),
                CostEntry("GLM-4.7", 89, 124000, 0.00),
            ]
            return

        try:
            # Get today's cost summary
            summary = self._cost_tracker.get_costs_today()

            # Convert to CostEntry format
            self._costs_store = []
            for model, data in summary.by_model.items():
                self._costs_store.append(
                    CostEntry(
                        model=model,
                        requests=data["requests"],
                        tokens=data["tokens"],
                        cost=data["cost"],
                    )
                )

            # If no data, show placeholder
            if not self._costs_store:
                self._costs_store = [
                    CostEntry("No data today", 0, 0, 0.0),
                ]
        except Exception as e:
            # Fall back to sample data on error
            now = datetime.now()
            self._costs_store = [
                CostEntry("Error loading", 0, 0, 0.0),
            ]

    def _initialize_sample_data(self) -> None:
        """Initialize with sample data for testing"""
        now = datetime.now()

        # Sample workers
        self._workers_store = [
            Worker("glm-alpha", "GLM-4.7", "/home/coder/forge", WorkerStatus.ACTIVE, "fg-1zy", 750, 15000, 0.03, now),
            Worker("glm-bravo", "GLM-4.7", "/home/coder/claude-config", WorkerStatus.IDLE, None, 480, 8500, 0.02, now),
            Worker("sonnet-01", "Sonnet 4.5", "/home/coder/forge", WorkerStatus.ACTIVE, "fg-1ab", 1200, 45000, 0.15, now),
        ]

        # Sample tasks
        self._tasks_store = [
            Task("fg-1zy", "Implement Textual app skeleton", TaskPriority.P0, TaskStatus.IN_PROGRESS, "GLM-4.7", "/home/coder/forge", "glm-alpha", 50000, now),
            Task("fg-1ab", "Design dashboard layout", TaskPriority.P0, TaskStatus.READY, "Sonnet 4.5", "/home/coder/forge", None, 30000, now),
            Task("fg-2cd", "Add reactive data binding", TaskPriority.P1, TaskStatus.READY, "GLM-4.7", "/home/coder/forge", None, 25000, now),
        ]

        # Sample subscriptions
        self.subscriptions = [
            Subscription("Claude Pro", "Sonnet 4.5", 72, 100, now, 20.0),
            Subscription("GLM-4.7 Pro", "GLM-4.7", 430, 1000, now, 50.0),
        ]

        # Load cost data from cost tracker
        self._load_cost_data()

        # Sample metrics
        self.metrics = MetricData(
            throughput_per_hour=12.4,
            avg_time_per_task=290.0,  # 4m 50s
            queue_velocity=9.2,
            cpu_percent=45.0,
            memory_gb=2.1,
            memory_total_gb=16.0,
            disk_gb=45.0,
            disk_total_gb=500.0,
            network_down_mbps=1.2,
            network_up_mbps=0.8,
            success_rate=92.0,
            completion_count=24,
            in_progress_count=2,
            failed_count=0,
        )

        # Sample logs
        self.logs = [
            LogEntry(now, "INFO", "glm-alpha started fg-1zy", "●EXEC"),
            LogEntry(now, "INFO", "Control panel initialized", "ℹ"),
            LogEntry(now, "WARN", "Check subscription usage", "⚠"),
        ]

    def _initialize_bead_workspaces(self) -> None:
        """Initialize bead workspaces from environment or current directory"""
        # Check FORGE_WORKSPACES environment variable
        import os
        workspaces_str = os.environ.get("FORGE_WORKSPACES", "")

        if workspaces_str:
            # Parse colon-separated list of workspaces
            self._bead_workspaces = [Path(w).expanduser() for w in workspaces_str.split(":") if w]
        else:
            # Default to current directory if it has .beads subdirectory
            cwd = Path.cwd()
            if (cwd / ".beads").exists():
                self._bead_workspaces = [cwd]
            else:
                # No bead workspaces configured
                self._bead_workspaces = []

    def _initialize_workspace_manager(self) -> None:
        """Initialize the workspace manager for multi-workspace support"""
        try:
            from forge.workspace_manager import (
                WorkspaceManager,
                DiscoveryConfig,
                get_workspace_manager,
            )

            # Create discovery config with bead workspace paths
            config = DiscoveryConfig(
                search_paths=self._bead_workspaces or [Path.cwd()],
                max_depth=3,
            )

            # Get or create workspace manager
            self._workspace_manager = get_workspace_manager()

            # Note: The workspace manager will be started asynchronously in on_mount
        except ImportError:
            self._workspace_manager = None

    def _on_bead_workspace_change(self, workspace: BeadWorkspace) -> None:
        """
        Handle bead workspace change events from the watcher.

        Args:
            workspace: Updated bead workspace
        """
        # Convert Bead objects to Task objects for the UI
        new_tasks = []
        for bead in workspace.beads:
            # Map bead status to task status
            status_map = {
                "open": TaskStatus.READY,
                "in_progress": TaskStatus.IN_PROGRESS,
                "blocked": TaskStatus.BLOCKED,
                "closed": TaskStatus.COMPLETED,
            }
            task_status = status_map.get(bead.status, TaskStatus.READY)

            # Map bead priority to task priority
            priority_map = {
                "0": TaskPriority.P0,
                "1": TaskPriority.P1,
                "2": TaskPriority.P2,
                "3": TaskPriority.P3,
                "4": TaskPriority.P4,
            }
            task_priority = priority_map.get(bead.priority, TaskPriority.P2)

            task = Task(
                id=bead.id,
                title=bead.title,
                priority=task_priority,
                status=task_status,
                model="",
                workspace=str(workspace.path),
                assignee=bead.assignee,
                tokens_used=0,
                created_at=datetime.now(),
            )
            new_tasks.append(task)

        # Update tasks store (triggers TUI update via reactive variable)
        self._tasks_store = new_tasks
        if self._tasks_panel is not None:
            self._tasks_panel.tasks = self._tasks_store

        # Log significant changes
        now = datetime.now()
        log_entry = LogEntry(
            now,
            "INFO",
            f"Tasks updated: {len(new_tasks)} bead(s) from {workspace.path.name}",
            "📋"
        )
        self.logs.append(log_entry)

    def compose(self: ComposeResult) -> ComposeResult:
        """Compose the UI"""
        yield Header()

        # Main dashboard container
        with Container(id="dashboard"):
            # Top row: Workers, Tasks, Costs
            with Horizontal(id="top_row"):
                with Vertical(id="left_col"):
                    self._workers_panel = WorkersPanel()
                    yield self._workers_panel

                with Vertical(id="center_col"):
                    self._tasks_panel = TasksPanel()
                    yield self._tasks_panel

                with Vertical(id="right_col"):
                    self._costs_panel = CostsPanel()
                    yield self._costs_panel

            # Middle row: Metrics
            with Horizontal(id="middle_row"):
                with Vertical(id="metrics_col"):
                    self._metrics_panel = MetricsPanel()
                    yield self._metrics_panel

                # Add space for future panels
                with Vertical(id="spacer"):
                    yield Static()

            # Bottom row: Logs (spans full width)
            with Horizontal(id="bottom_row"):
                self._logs_panel = LogsPanel()
                yield self._logs_panel

        # Chat panel
        with Container(id="chat_container"):
            self._chat_panel = ChatPanel()
            yield self._chat_panel

        yield Footer()

    def on_mount(self) -> None:
        """Initialize after mounting"""
        # Apply responsive layout based on terminal size
        self._apply_responsive_layout()

        # Set up chat command handler
        if self._chat_panel is not None:
            self._chat_panel.on_command_submit = self._handle_command

        # Initialize panels with data
        self._update_all_panels()

        # Start background refresh task
        self.set_interval(2.0, self._refresh_data)

        # Watch for terminal resize events
        self.watch("size", self._on_terminal_resize)

        # Start status file watcher
        self._start_status_watcher()

        # Start log monitor for cost tracking
        self._start_log_monitor()

        # Start bead watcher for task updates
        self._start_bead_watcher()

        # Start cost data refresh
        self.set_interval(self._cost_refresh_interval, self._refresh_cost_data)

    def _refresh_cost_data(self) -> None:
        """Periodically refresh cost data from the cost tracker"""
        self._load_cost_data()
        if self._costs_panel is not None:
            self._costs_panel.costs = self._costs_store

    def _update_all_panels(self) -> None:
        """Update all panels with current data"""
        if self._workers_panel is not None:
            self._workers_panel.worker_list = self._workers_store
        if self._tasks_panel is not None:
            self._tasks_panel.tasks = self._tasks_store
        if self._costs_panel is not None:
            self._costs_panel.costs = self._costs_store
        if self._metrics_panel is not None:
            self._metrics_panel.metrics = self._metrics_store
        if self._logs_panel is not None:
            self._logs_panel.logs = self._logs_store

    def _refresh_data(self) -> None:
        """Periodic data refresh"""
        # This would fetch real data in production
        # For now, just trigger panel updates
        self._update_all_panels()

    def _start_status_watcher(self) -> None:
        """Start the status file watcher for real-time worker updates"""
        async def start_watcher() -> None:
            """Async task to start the status watcher"""
            self._status_watcher = StatusWatcher(
                status_dir=self._status_dir,
                callback=self._on_status_file_event,
                poll_interval=5.0,  # 5-second polling fallback per ADR 0008
            )
            watcher_type = await self._status_watcher.start()

            # Log which watcher type is being used
            now = datetime.now()
            if watcher_type == "inotify":
                log_entry = LogEntry(now, "INFO", "Status watcher started (inotify mode)", "🔍")
            else:
                log_entry = LogEntry(now, "INFO", "Status watcher started (polling mode)", "🔄")
            self.logs.append(log_entry)

        # Start watcher in background
        asyncio.create_task(start_watcher())

    def _on_status_file_event(self, event: StatusFileEvent) -> None:
        """
        Handle status file change events from the watcher.

        Args:
            event: Status file event
        """
        # Forward event to workers panel
        if self._workers_panel is not None:
            self._workers_panel.on_status_file_event(event)

        # Log significant events
        now = datetime.now()
        if event.event_type == StatusFileEvent.EventType.DELETED:
            log_entry = LogEntry(
                now,
                "INFO",
                f"Worker stopped: {event.worker_id}",
                "⦻"
            )
            self.logs.append(log_entry)
        elif event.status and event.status.error:
            # Log corrupted status file
            log_entry = LogEntry(
                now,
                "ERROR",
                f"Worker {event.worker_id}: {event.status.error}",
                "⚠"
            )
            self.logs.append(log_entry)
        elif event.status and event.status.status == WorkerStatusValue.FAILED:
            log_entry = LogEntry(
                now,
                "WARN",
                f"Worker failed: {event.worker_id}",
                "✗"
            )
            self.logs.append(log_entry)

    def _start_log_monitor(self) -> None:
        """Start the log watcher for parsing api_call_completed events with inotify + polling fallback"""
        async def start_watcher() -> None:
            """Async task to start the log watcher"""
            try:
                LogWatcher, _ = _get_log_watcher()

                # Create callback for handling log file events
                def on_log_event(event: Any) -> None:
                    """Handle log file events (created, modified, deleted)"""
                    # Process new log entries
                    if event.entries:
                        for entry in event.entries:
                            # Check if this is an api_call_completed event
                            if entry.event == "api_call_completed":
                                # Send to cost tracker
                                if self._cost_tracker is not None:
                                    # Convert LogEntry to dict
                                    log_dict = entry.to_dict()

                                    # Add event type explicitly
                                    log_dict["event"] = "api_call_completed"

                                    self._cost_tracker.add_event_from_log(log_dict)

                    # Log file creation/deletion events
                    now = datetime.now()
                    if event.event_type.value == "created":
                        log_entry = LogEntry(now, "INFO", f"Log file created: {event.worker_id}", "📄")
                        self.logs.append(log_entry)
                    elif event.event_type.value == "deleted":
                        log_entry = LogEntry(now, "INFO", f"Log file deleted: {event.worker_id}", "🗑")
                        self.logs.append(log_entry)

                # Ensure log directory exists
                self._log_dir.mkdir(parents=True, exist_ok=True)

                # Create and start log watcher with 5s polling fallback per ADR 0008
                self._log_monitor = LogWatcher(
                    log_dir=self._log_dir,
                    callback=on_log_event,
                    poll_interval=5.0,  # 5-second polling fallback per ADR 0008
                )

                watcher_type = await self._log_monitor.start()

                # Log successful start with watcher type
                now = datetime.now()
                mode_label = "⚡" if watcher_type == "inotify" else "🔄"
                log_entry = LogEntry(
                    now,
                    "INFO",
                    f"Log watcher started ({watcher_type}): {self._log_dir}",
                    mode_label
                )
                self.logs.append(log_entry)

            except Exception as e:
                # Log error but continue without log watcher
                now = datetime.now()
                log_entry = LogEntry(now, "WARN", f"Log watcher failed to start: {e}", "⚠")
                self.logs.append(log_entry)

        # Start watcher in background
        asyncio.create_task(start_watcher())

    def _start_bead_watcher(self) -> None:
        """Start the bead watcher for real-time task updates with inotify + polling fallback"""
        async def start_watcher() -> None:
            """Async task to start the bead watcher"""
            try:
                # Only start if we have workspaces configured
                if not self._bead_workspaces:
                    now = datetime.now()
                    log_entry = LogEntry(now, "INFO", "No bead workspaces configured, skipping bead watcher", "📋")
                    self.logs.append(log_entry)
                    return

                # Create and start bead watcher with 5s polling fallback per ADR 0008
                self._bead_watcher = BeadWatcher(
                    workspaces=self._bead_workspaces,
                    callback=self._on_bead_workspace_change,
                    poll_interval=5.0,  # 5-second polling fallback per ADR 0008
                )

                watcher_type = await self._bead_watcher.start()

                # Log successful start with watcher type
                now = datetime.now()
                mode_label = "⚡" if watcher_type == "inotify" else "🔄"
                log_entry = LogEntry(
                    now,
                    "INFO",
                    f"Bead watcher started ({watcher_type}): {len(self._bead_workspaces)} workspace(s)",
                    mode_label
                )
                self.logs.append(log_entry)

                # Initial load of beads
                for workspace_path in self._bead_workspaces:
                    try:
                        workspace = parse_workspace(workspace_path)
                        self._on_bead_workspace_change(workspace)
                    except Exception as e:
                        now = datetime.now()
                        log_entry = LogEntry(now, "WARN", f"Failed to load workspace {workspace_path}: {e}", "⚠")
                        self.logs.append(log_entry)

            except Exception as e:
                # Log error but continue without bead watcher
                now = datetime.now()
                log_entry = LogEntry(now, "WARN", f"Bead watcher failed to start: {e}", "⚠")
                self.logs.append(log_entry)

        # Start watcher in background
        asyncio.create_task(start_watcher())

    def _handle_command(self, command: str) -> None:
        """Handle chat command submission"""
        now = datetime.now()
        log_entry = LogEntry(now, "COMMAND", f"User command: {command}", "💬")
        self.logs.append(log_entry)

        # Process natural language commands
        command_lower = command.lower()

        # View switching commands
        if "show" in command_lower or "go to" in command_lower or "switch" in command_lower:
            if "worker" in command_lower:
                self.action_switch_view("workers")
            elif "task" in command_lower:
                self.action_switch_view("tasks")
            elif "cost" in command_lower:
                self.action_switch_view("costs")
            elif "metric" in command_lower:
                self.action_switch_view("metrics")
            elif "log" in command_lower:
                self.action_switch_view("logs")
            elif "overview" in command_lower or "dashboard" in command_lower:
                self.action_switch_view("overview")

        # Split view commands
        elif "split" in command_lower:
            # Simple split command - default to workers|tasks
            self.action_split_view("workers", "tasks")

        # Focus commands
        elif "focus" in command_lower:
            if "worker" in command_lower:
                self.action_focus_panel("workers")
            elif "task" in command_lower:
                self.action_focus_panel("tasks")
            elif "cost" in command_lower:
                self.action_focus_panel("costs")
            elif "metric" in command_lower:
                self.action_focus_panel("metrics")
            elif "log" in command_lower:
                self.action_focus_panel("logs")
            elif "chat" in command_lower:
                self.action_focus_chat()

        # Help command
        elif command_lower in ["help", "?", "h"]:
            self.action_show_help()

        # Refresh command
        elif "refresh" in command_lower or "reload" in command_lower:
            self.action_refresh()

        else:
            # Unknown command - log it
            info_entry = LogEntry(now, "INFO", f"Command received: {command}", "ℹ")
            self.logs.append(info_entry)

        self._update_all_panels()

    # -------------------------------------------------------------------------
    # Responsive Layout
    # -------------------------------------------------------------------------

    def _apply_responsive_layout(self) -> None:
        """Apply appropriate CSS classes based on terminal size"""
        dashboard = self.query_one("#dashboard", Container)
        terminal_height = self.size.height

        # Remove existing layout classes
        dashboard.remove_class("-compact", "-large", "-responsive", "-standard")

        # Apply layout class based on terminal height
        if terminal_height < 45:
            # Compact layout for 199×38 and similar
            dashboard.add_class("-compact")
            self._log_info(f"Applied compact layout for {self.size.width}×{terminal_height} terminal")
        elif terminal_height >= 65:
            # Large layout for tall terminals
            dashboard.add_class("-large")
            self._log_info(f"Applied large layout for {self.size.width}×{terminal_height} terminal")
        elif 53 <= terminal_height <= 57:
            # Standard 199×55 layout
            dashboard.add_class("-standard")
            self._log_info(f"Applied standard layout for {self.size.width}×{terminal_height} terminal")
        else:
            # Responsive fallback for non-standard sizes
            dashboard.add_class("-responsive")
            self._log_info(f"Applied responsive layout for {self.size.width}×{terminal_height} terminal")

    def _on_terminal_resize(self) -> None:
        """Handle terminal resize events"""
        self._apply_responsive_layout()

    # -------------------------------------------------------------------------
    # Actions
    # -------------------------------------------------------------------------

    def action_switch_view(self, view: str) -> None:
        """Switch to a different view"""
        # Map view string to ViewMode enum
        view_map = {
            "overview": ViewMode.OVERVIEW,
            "workers": ViewMode.WORKERS,
            "tasks": ViewMode.TASKS,
            "costs": ViewMode.COSTS,
            "metrics": ViewMode.METRICS,
            "logs": ViewMode.LOGS,
        }

        new_view = view_map.get(view)
        if new_view is None:
            self._log_error(f"Unknown view: {view}")
            return

        # Save current view to history
        if self._current_view != new_view:
            self._view_history.append(self._current_view)

        self._current_view = new_view
        self._update_view_layout()

        # Log the view change
        self._log_info(f"Switched to {view} view")

    def action_split_view(self, left: str, right: str) -> None:
        """Create a split-screen view"""
        self._split_left = left
        self._split_right = right
        self._current_view = ViewMode.SPLIT
        self._view_history.append(ViewMode.OVERVIEW)
        self._update_view_layout()

        # Log the split view creation
        self._log_info(f"Created split view: {left} | {right}")

    def action_toggle_split(self) -> None:
        """Toggle split view mode"""
        if self._current_view == ViewMode.SPLIT:
            # Go back to overview
            self.action_switch_view("overview")
        else:
            # Create default split view (workers | tasks)
            self.action_split_view("workers", "tasks")

    def action_go_back(self) -> None:
        """Go back to the previous view"""
        if self._view_history:
            previous_view = self._view_history.pop()
            self._current_view = previous_view
            self._update_view_layout()
            self._log_info(f"Returned to {previous_view.value} view")
        else:
            self._log_info("No previous view in history")

    def action_cycle_view(self) -> None:
        """Cycle to the next view"""
        views = [ViewMode.OVERVIEW, ViewMode.WORKERS, ViewMode.TASKS,
                 ViewMode.COSTS, ViewMode.METRICS, ViewMode.LOGS]
        current_index = views.index(self._current_view)
        next_index = (current_index + 1) % len(views)
        self.action_switch_view(views[next_index].value)

    def action_cycle_view_reverse(self) -> None:
        """Cycle to the previous view"""
        views = [ViewMode.OVERVIEW, ViewMode.WORKERS, ViewMode.TASKS,
                 ViewMode.COSTS, ViewMode.METRICS, ViewMode.LOGS]
        current_index = views.index(self._current_view)
        prev_index = (current_index - 1) % len(views)
        self.action_switch_view(views[prev_index].value)

    def action_show_help(self) -> None:
        """Show help overlay"""
        # For now, just log the help message
        help_text = """
FORGE Control Panel Help
========================

View Navigation:
  W/T/C/M/L/O - Switch to Workers/Tasks/Costs/Metrics/Logs/Overview
  Tab/Shift+Tab - Cycle through views
  Esc - Go back to previous view
  S - Toggle split view

Panel Focus:
  Ctrl+W/T/M/L - Focus Workers/Tasks/Metrics/Logs panel
  C - Focus chat input

Other:
  R - Refresh all data
  Q - Quit
  : - Activate command input

Type :help for more information
        """
        self._log_info(help_text.strip())

    def _update_view_layout(self) -> None:
        """Update the view layout based on current view mode"""
        # This is a placeholder - the actual layout updates would happen
        # by showing/hiding containers or remounting the compose tree
        # For now, we'll update the sub_title to show the current view
        view_titles = {
            ViewMode.OVERVIEW: "Dashboard Overview",
            ViewMode.WORKERS: "Worker Pool Status",
            ViewMode.TASKS: "Task Queue",
            ViewMode.COSTS: "Cost Analytics",
            ViewMode.METRICS: "Performance Metrics",
            ViewMode.LOGS: "Activity Log",
            ViewMode.SPLIT: f"Split: {self._split_left} | {self._split_right}",
        }
        self.sub_title = view_titles.get(self._current_view, "FORGE")

    def _log_info(self, message: str) -> None:
        """Log an info message"""
        now = datetime.now()
        log_entry = LogEntry(now, "INFO", message, "ℹ")
        self.logs.append(log_entry)
        self._update_all_panels()

    def _log_error(self, message: str) -> None:
        """Log an error message"""
        now = datetime.now()
        log_entry = LogEntry(now, "ERROR", message, "✗")
        self.logs.append(log_entry)
        self._update_all_panels()

    def action_focus_chat(self) -> None:
        """Focus the chat input"""
        if self._chat_panel is not None:
            self._chat_panel.focus_input()

    def action_toggle_chat(self) -> None:
        """Toggle chat input focus"""
        if self._chat_panel is not None:
            self._chat_panel.focus_input()

    def action_focus_panel(self, panel_name: str) -> None:
        """Focus a specific panel"""
        # Normalize panel name
        panel_name = panel_name.lower().replace(" ", "_")

        panel_map = {
            "workers": self._workers_panel,
            "tasks": self._tasks_panel,
            "costs": self._costs_panel,
            "metrics": self._metrics_panel,
            "logs": self._logs_panel,
            "chat": self._chat_panel,
            "command": self._chat_panel,
        }

        panel = panel_map.get(panel_name)
        if panel is not None:
            panel.focus()
            self._log_info(f"Focused on {panel_name} panel")
        else:
            self._log_error(f"Unknown panel: {panel_name}")

    def action_refresh(self) -> None:
        """Force refresh all data"""
        self._refresh_data()

        # Add log entry
        now = datetime.now()
        log_entry = LogEntry(now, "INFO", "Dashboard refreshed", "🔄")
        self.logs.append(log_entry)
        self._update_all_panels()

    # =============================================================================
    # Error Display Methods (per ADR 0014)
    # =============================================================================

    def show_transient_error(
        self,
        message: str,
        severity: str = "info",
        timeout: float | None = None
    ) -> None:
        """
        Show transient notification (non-blocking error).

        Per ADR 0014: Shows notification, doesn't interrupt workflow.

        Args:
            message: Notification message
            severity: info, warning, error, or success
            timeout: Auto-dismiss timeout in seconds (None = manual dismiss)

        Example:
            self.show_transient_error("Cost update delayed", severity="warning", timeout=5)
        """
        if self._error_display:
            self._error_display.transient(message, severity, timeout)

    def show_component_error(
        self,
        component: str,
        error: str,
        fallback: str = "",
        guidance: list[str] | None = None
    ) -> None:
        """
        Show component error in panel (degrades component, app keeps running).

        Per ADR 0014: Show error in component panel, degrade gracefully.

        Args:
            component: Component name (e.g., "chat", "workers", "logs")
            error: Primary error message (clear, non-technical)
            fallback: Fallback mode description
            guidance: List of actionable guidance steps (3-5 items)

        Example:
            self.show_component_error(
                "chat",
                "Backend unavailable",
                fallback="Using hotkey-only mode",
                guidance=["Restart backend: :restart-backend", "Check logs: :logs backend"]
            )
        """
        if self._error_display:
            # Find panel widget for this component
            panel_map = {
                "workers": self._workers_panel,
                "tasks": self._tasks_panel,
                "costs": self._costs_panel,
                "metrics": self._metrics_panel,
                "logs": self._logs_panel,
                "chat": self._chat_panel,
            }
            panel = panel_map.get(component)
            self._error_display.component(component, error, fallback, guidance, panel)

    def clear_component_error(self, component: str) -> None:
        """
        Clear component error (component recovered).

        Args:
            component: Component name to clear error for
        """
        if self._error_display:
            self._error_display.clear_component(component)

    def show_fatal_error(
        self,
        title: str,
        errors: list[str],
        guidance: list[str],
        exit_on_dismiss: bool = True
    ) -> None:
        """
        Show fatal error screen (blocks app).

        Per ADR 0014: Show full-screen error, exit app.

        Args:
            title: Error title
            errors: List of error messages
            guidance: List of fix suggestions (3-5 items)
            exit_on_dismiss: Whether to exit app on dismiss

        Example:
            self.show_fatal_error(
                "Cannot Start FORGE",
                ["Cannot write to ~/.forge (permission denied)"],
                [
                    "Check file permissions: ls -la ~/.forge",
                    "Verify disk space: df -h",
                    "Check directory ownership: ls -ld ~/.forge"
                ]
            )
        """
        if self._error_display:
            self._error_display.fatal(title, errors, guidance, exit_on_dismiss)

    def show_error_dialog(
        self,
        title: str,
        message: str,
        details: dict[str, Any] | None = None,
        actions: list[tuple[str, Callable[[], None] | None]] | None = None
    ) -> None:
        """
        Show error dialog with actionable buttons.

        Per ADR 0014: Show dialog with actions, no automatic retry.

        Args:
            title: Error title
            message: Error message
            details: Additional context (e.g., launcher, model, workspace)
            actions: List of (label, callback) tuples

        Example:
            self.show_error_dialog(
                "Worker Spawn Failed",
                "Launcher exited with code 1",
                details={"Launcher": "~/.forge/launchers/claude-code", "Stderr": "..."},
                actions=[
                    ("View Logs", lambda: self.view_logs()),
                    ("Edit Config", lambda: self.edit_config()),
                    ("Retry", lambda: self.retry_spawn()),
                    ("Dismiss", None),
                ]
            )
        """
        if self._error_display:
            # Convert actions to ErrorAction objects
            error_actions = []
            if actions:
                for label, callback in actions:
                    error_actions.append(ErrorAction(label=label, callback=callback))
            self._error_display.dialog(title, message, details, error_actions)

    async def on_unmount(self) -> None:
        """Cleanup when app is unmounted"""
        # Stop status watcher
        if self._status_watcher is not None:
            await self._status_watcher.stop()
            self._status_watcher = None

# =============================================================================
# CLI Entry Point
# =============================================================================


def main() -> None:
    """Main entry point for the CLI"""
    app = ForgeApp()
    app.run()
