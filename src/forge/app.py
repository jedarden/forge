"""
Main FORGE Textual Application

Implements the 6-panel dashboard layout for 199×55 terminal:
- Workers: Worker pool status and management
- Tasks: Task queue and bead tracking
- Costs: Cost analytics and optimization
- Metrics: Performance metrics and resource usage
- Logs: Activity log stream
- Chat: Conversational command input
"""

from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from typing import Any, Callable
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

# =============================================================================
# Data Models
# =============================================================================


class WorkerStatus(Enum):
    """Worker health status"""
    ACTIVE = "active"
    IDLE = "idle"
    UNHEALTHY = "unhealthy"
    SPAWNING = "spawning"
    TERMINATING = "terminating"


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

    6-panel layout for 199×55 terminal:
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

    # Bindings
    BINDINGS = [
        Binding("q", "quit", "Quit", show=True),
        Binding("c", "focus_chat", "Chat", show=True),
        Binding("w", "focus_panel('workers')", "Workers", show=True),
        Binding("t", "focus_panel('tasks')", "Tasks", show=True),
        Binding("m", "focus_panel('metrics')", "Metrics", show=True),
        Binding("l", "focus_panel('logs')", "Logs", show=True),
        Binding("r", "refresh", "Refresh", show=True),
        Binding(":", "toggle_chat", "Command", show=True),
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

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        # Initialize storage
        self._workers_store = []
        self._tasks_store = []
        self._subscriptions_store = []
        self._costs_store = []
        self._metrics_store = None
        self._logs_store = []
        # Initialize with sample data
        self._initialize_sample_data()

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
        # Set up chat command handler
        if self._chat_panel is not None:
            self._chat_panel.on_command_submit = self._handle_command

        # Initialize panels with data
        self._update_all_panels()

        # Start background refresh task
        self.set_interval(2.0, self._refresh_data)

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

    def _handle_command(self, command: str) -> None:
        """Handle chat command submission"""
        now = datetime.now()
        log_entry = LogEntry(now, "COMMAND", f"User command: {command}", "💬")
        self.logs.append(log_entry)
        self._update_all_panels()

    # -------------------------------------------------------------------------
    # Actions
    # -------------------------------------------------------------------------

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
        panel_map = {
            "workers": self._workers_panel,
            "tasks": self._tasks_panel,
            "metrics": self._metrics_panel,
            "logs": self._logs_panel,
        }

        panel = panel_map.get(panel_name)
        if panel is not None:
            panel.focus()

    def action_refresh(self) -> None:
        """Force refresh all data"""
        self._refresh_data()

        # Add log entry
        now = datetime.now()
        log_entry = LogEntry(now, "INFO", "Dashboard refreshed", "🔄")
        self.logs.append(log_entry)
        self._update_all_panels()

# =============================================================================
# CLI Entry Point
# =============================================================================


def main() -> None:
    """Main entry point for the CLI"""
    app = ForgeApp()
    app.run()
