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

        return cls(
            session_id=status_file.worker_id,
            model=status_file.model,
            workspace=status_file.workspace,
            status=status,
            current_task=status_file.current_task,
            uptime_seconds=0,  # Calculate from started_at if needed
            error=status_file.error,
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
    """Worker pool status panel"""

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
    """

    workers: reactive[list[Worker]] = reactive([])
    active_count: reactive[int] = reactive(0)
    idle_count: reactive[int] = reactive(0)

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._table: DataTable[Worker] | None = None
        self._status_cache = WorkerStatusCache()

    def compose(self: ComposeResult) -> ComposeResult:
        yield Label("👷 WORKER POOL")

    def on_mount(self) -> None:
        """Initialize the worker table on mount"""
        # Initial setup
        self.update_workers(self.workers)

    def watch_workers(self, old_workers: list[Worker], new_workers: list[Worker]) -> None:
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

        # Update reactive workers list
        self.workers = new_workers

    def _update_counts(self, workers: list[Worker]) -> None:
        """Update worker counts"""
        self.active_count = sum(1 for w in workers if w.status == WorkerStatus.ACTIVE)
        self.idle_count = sum(1 for w in workers if w.status == WorkerStatus.IDLE)

    def _update_display(self, workers: list[Worker]) -> None:
        """Update the display with worker data"""
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

    def _get_status_symbol(self, status: WorkerStatus) -> str:
        """Get status symbol for display"""
        symbols = {
            WorkerStatus.ACTIVE: "●EXEC",
            WorkerStatus.IDLE: "○IDLE",
            WorkerStatus.UNHEALTHY: "✗DEAD",
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
        """Update the display with task data"""
        ready_tasks = [t for t in tasks if t.status == TaskStatus.READY]

        title = Text()
        title.append("📋 TASK QUEUE (", style="bold")
        title.append(f"{len(ready_tasks)}", style="bold cyan")
        title.append(" Ready)", style="bold")
        self.update(title)


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

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        # Initialize storage
        self._workers_store = []
        self._tasks_store = []
        self._subscriptions_store = []
        self._costs_store = []
        self._metrics_store = None
        self._logs_store = []
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

            workspace_path = workspace or str(Path.cwd())
            worker_ids = spawn_workers(
                model=model,
                count=count,
                workspace=workspace_path
            )

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

    def _tool_list_workers(self, filter: str = "all") -> ToolCallResult:
        """Tool callback for list_workers - lists workers with optional filtering"""
        try:
            workers = self._workers_store

            # Apply filter
            if filter != "all":
                if filter == "idle":
                    filtered = [w for w in workers if w.status == "idle"]
                elif filter == "active":
                    filtered = [w for w in workers if w.status == "active"]
                elif filter == "failed":
                    filtered = [w for w in workers if w.status == "failed"]
                elif filter == "stuck":
                    filtered = [w for w in workers if w.status == "stuck"]
                elif filter == "healthy":
                    filtered = [w for w in workers if w.status in ["idle", "active"]]
                else:
                    filtered = workers
            else:
                filtered = workers

            worker_data = [
                {
                    "id": w.id,
                    "model": w.model,
                    "status": w.status,
                    "workspace": w.workspace,
                    "task": w.current_task,
                }
                for w in filtered
            ]

            return create_success_result(
                "list_workers",
                f"Found {len(filtered)} worker(s)",
                {"workers": worker_data, "filter": filter}
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

    # =============================================================================
    # Cost & Analytics Tool Callbacks
    # =============================================================================

    def _tool_show_costs(self, period: str = "today", breakdown: str | None = None) -> ToolCallResult:
        """Tool callback for show_costs - displays cost analysis"""
        try:
            # Switch to costs view
            self.action_switch_view("costs")

            # Apply period filter (would update costs panel)
            cost_data = [
                {
                    "date": c.date,
                    "model": c.model,
                    "cost": c.cost,
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
            # Calculate forecast based on current usage
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
                    json.dump([l.to_dict() for l in logs], f, indent=2)
                elif format == "csv":
                    import csv
                    if logs:
                        writer = csv.DictWriter(f, fieldnames=logs[0].to_dict().keys())
                        writer.writeheader()
                        for log in logs:
                            writer.writerow(log.to_dict())
                else:  # txt
                    for log in logs:
                        f.write(f"{log.timestamp} [{log.level}] {log.message}\n")
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

            if not workspace_path.exists():
                return create_error_result("switch_workspace", f"Workspace not found: {path}")

            # Update workspace (would trigger reload in real implementation)
            return create_success_result(
                "switch_workspace",
                f"Switched to workspace: {path}",
                {"workspace": str(workspace_path)}
            )
        except Exception as e:
            return create_error_result("switch_workspace", f"Failed to switch workspace: {e}")

    def _tool_list_workspaces(self, filter: str = "all") -> ToolCallResult:
        """Tool callback for list_workspaces - lists available workspaces"""
        try:
            from pathlib import Path

            # Find workspaces (directories with .beads subdirectory)
            workspaces = []
            base_path = Path.home()

            for item in base_path.rglob(".beads"):
                workspace = item.parent
                workspaces.append({
                    "path": str(workspace),
                    "name": workspace.name,
                })

            return create_success_result(
                "list_workspaces",
                f"Found {len(workspaces)} workspace(s)",
                {
                    "workspaces": workspaces,
                    "filter": filter
                }
            )
        except Exception as e:
            return create_error_result("list_workspaces", f"Failed to list workspaces: {e}")

    def _tool_create_workspace(self, path: str, template: str = "empty") -> ToolCallResult:
        """Tool callback for create_workspace - creates a new workspace"""
        try:
            workspace_path = Path(path).expanduser()
            workspace_path.mkdir(parents=True, exist_ok=True)

            # Initialize workspace based on template
            if template != "empty":
                # Would add template-specific initialization
                pass

            return create_success_result(
                "create_workspace",
                f"Created workspace: {path}",
                {
                    "workspace": str(workspace_path),
                    "template": template
                }
            )
        except Exception as e:
            return create_error_result("create_workspace", f"Failed to create workspace: {e}")

    def _tool_get_workspace_info(self) -> ToolCallResult:
        """Tool callback for get_workspace_info - gets current workspace info"""
        try:
            workspace_path = Path.cwd()

            info = {
                "path": str(workspace_path),
                "name": workspace_path.name,
                "has_beads": (workspace_path / ".beads").exists(),
            }

            return create_success_result(
                "get_workspace_info",
                f"Current workspace: {workspace_path.name}",
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
    def workers(self) -> list[Worker]:
        """Get workers list"""
        return self._workers_store

    @workers.setter
    def workers(self, value: list[Worker]) -> None:
        """Set workers list and trigger update"""
        self._workers_store = value
        if self._workers_panel:
            self._workers_panel.workers = value

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

        # Sample costs
        self.costs = [
            CostEntry("Sonnet 4.5", 24, 347000, 4.17),
            CostEntry("GLM-4.7", 89, 124000, 0.00),
        ]

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

    def _update_all_panels(self) -> None:
        """Update all panels with current data"""
        if self._workers_panel is not None:
            self._workers_panel.workers = self._workers_store
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
                poll_interval=1.0,
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
