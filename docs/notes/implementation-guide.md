# Control Panel Dashboard - Implementation Guide

**Framework**: Textual
**Target**: Pool optimizer real-time monitoring dashboard
**Updated**: 2026-02-07

---

## Quick Start

### Installation

```bash
# Install Textual and development tools
pip install textual textual-dev

# Verify installation
python -m textual --version
```

### Basic Dashboard Scaffold

```python
# dashboard.py
from textual.app import App, ComposeResult
from textual.containers import Container, Horizontal, Vertical
from textual.widgets import Header, Footer, Static

class PoolOptimizerDashboard(App):
    """Pool optimizer monitoring dashboard."""

    CSS = """
    Screen {
        background: $surface;
    }

    #workers {
        width: 1fr;
        height: 40%;
        border: solid $primary;
    }

    #subscriptions {
        width: 1fr;
        height: 40%;
        border: solid $primary;
    }
    """

    BINDINGS = [
        ("q", "quit", "Quit"),
        ("r", "refresh", "Refresh"),
        ("w", "show_workers", "Workers"),
    ]

    def compose(self) -> ComposeResult:
        """Create child widgets for the app."""
        yield Header()

        with Horizontal():
            with Container(id="workers"):
                yield Static("Worker Pool", classes="panel-title")

            with Container(id="subscriptions"):
                yield Static("Subscriptions", classes="panel-title")

        yield Footer()

    def action_refresh(self) -> None:
        """Force refresh all data."""
        self.notify("Refreshing data...")

    def action_show_workers(self) -> None:
        """Switch to worker detail view."""
        self.notify("Worker view")


if __name__ == "__main__":
    app = PoolOptimizerDashboard()
    app.run()
```

### Run Development Server

```bash
# Run with hot reload (auto-refresh on code changes)
textual run --dev dashboard.py

# Run with console for debugging
textual console

# In another terminal
textual run --dev dashboard.py
```

---

## Core Components

### 1. Worker Status Table

```python
from textual.widgets import DataTable
from textual.reactive import reactive

class WorkerTable(DataTable):
    """Live-updating worker status table."""

    def on_mount(self) -> None:
        """Setup table columns and start monitoring."""
        self.cursor_type = "row"
        self.zebra_stripes = True

        # Add columns
        self.add_columns("Worker ID", "Model", "Status", "Task", "Uptime")

        # Start background update task
        self.run_worker(self.update_workers())

    async def update_workers(self) -> None:
        """Background task to fetch and update worker data."""
        while True:
            # Fetch worker data from pool manager
            workers = await get_worker_status()

            # Clear and repopulate table
            self.clear()
            for worker in workers:
                self.add_row(
                    worker.id,
                    worker.model,
                    self.format_status(worker.status),
                    worker.task or "-",
                    worker.uptime,
                    key=worker.id,
                )

            await asyncio.sleep(2)  # Update every 2 seconds

    def format_status(self, status: str) -> str:
        """Format status with color."""
        status_map = {
            "active": "[green]● Active[/green]",
            "idle": "[yellow]▲ Idle[/yellow]",
            "unhealthy": "[red]✗ Unhealthy[/red]",
        }
        return status_map.get(status, status)
```

### 2. Subscription Progress Panel

```python
from textual.widgets import Static, ProgressBar
from textual.containers import Vertical
from textual.reactive import reactive

class SubscriptionPanel(Vertical):
    """Display subscription usage with progress bars."""

    usage_percentage = reactive(0.0)
    remaining_time = reactive("")

    def compose(self) -> ComposeResult:
        yield Static("Claude Pro", classes="subscription-title")
        yield ProgressBar(total=100, id="claude-progress")
        yield Static(id="claude-stats")

    def watch_usage_percentage(self, percentage: float) -> None:
        """Update progress bar when usage changes."""
        progress = self.query_one("#claude-progress", ProgressBar)
        progress.update(progress=percentage)

        # Update color based on usage
        if percentage >= 90:
            progress.add_class("critical")
        elif percentage >= 70:
            progress.add_class("warning")

        # Update stats text
        stats = self.query_one("#claude-stats", Static)
        stats.update(
            f"Usage: {percentage:.0f}% | Resets in: {self.remaining_time}"
        )

    async def on_mount(self) -> None:
        """Start monitoring subscription usage."""
        self.run_worker(self.monitor_subscription())

    async def monitor_subscription(self) -> None:
        """Background task to update subscription data."""
        while True:
            data = await get_subscription_usage("claude-pro")
            self.usage_percentage = (data.used / data.limit) * 100
            self.remaining_time = data.reset_time
            await asyncio.sleep(5)
```

### 3. Activity Log with Auto-scroll

```python
from textual.widgets import RichLog

class ActivityLog(RichLog):
    """Auto-scrolling activity log."""

    def on_mount(self) -> None:
        """Setup log and start listening for events."""
        self.max_lines = 1000
        self.wrap = True
        self.run_worker(self.listen_events())

    async def listen_events(self) -> None:
        """Listen for activity events and log them."""
        async for event in pool_manager.event_stream():
            timestamp = event.timestamp.strftime("%H:%M:%S")

            # Format based on event type
            if event.type == "task_complete":
                self.write(
                    f"[dim]{timestamp}[/dim]  "
                    f"[green]✓[/green] {event.message}"
                )
            elif event.type == "worker_spawn":
                self.write(
                    f"[dim]{timestamp}[/dim]  "
                    f"[blue]⟳[/blue] {event.message}"
                )
            elif event.type == "warning":
                self.write(
                    f"[dim]{timestamp}[/dim]  "
                    f"[yellow]⚠[/yellow] {event.message}"
                )
            elif event.type == "error":
                self.write(
                    f"[dim]{timestamp}[/dim]  "
                    f"[red]✗[/red] {event.message}"
                )
            else:
                self.write(f"[dim]{timestamp}[/dim]  ℹ {event.message}")

    def log_info(self, message: str) -> None:
        """Log info message."""
        timestamp = datetime.now().strftime("%H:%M:%S")
        self.write(f"[dim]{timestamp}[/dim]  ℹ {message}")

    def log_error(self, message: str) -> None:
        """Log error message."""
        timestamp = datetime.now().strftime("%H:%M:%S")
        self.write(f"[dim]{timestamp}[/dim]  [red]✗[/red] {message}")
```

### 4. Task Queue Display

```python
from textual.widgets import DataTable, Static
from textual.containers import Vertical

class TaskQueuePanel(Vertical):
    """Display task queue grouped by workspace."""

    def compose(self) -> ComposeResult:
        yield Static("Task Queue", classes="panel-title")
        yield DataTable(id="task-table")

    def on_mount(self) -> None:
        """Setup table and start monitoring."""
        table = self.query_one("#task-table", DataTable)
        table.cursor_type = "row"
        table.add_columns("Priority", "ID", "Title", "Status")

        self.run_worker(self.update_tasks())

    async def update_tasks(self) -> None:
        """Update task queue display."""
        while True:
            tasks = await get_ready_beads()

            table = self.query_one("#task-table", DataTable)
            table.clear()

            # Group by workspace
            by_workspace = {}
            for task in tasks:
                ws = task.workspace
                by_workspace.setdefault(ws, []).append(task)

            # Add workspace sections
            for workspace, ws_tasks in by_workspace.items():
                # Add workspace header row
                table.add_row(
                    "",
                    "",
                    f"[bold]{workspace}[/bold]",
                    f"{len(ws_tasks)} tasks",
                )

                # Add tasks
                for task in ws_tasks:
                    table.add_row(
                        self.format_priority(task.priority),
                        task.id,
                        task.title[:40],
                        task.status,
                    )

            await asyncio.sleep(3)

    def format_priority(self, priority: str) -> str:
        """Format priority with color."""
        priority_map = {
            "P0": "[red]●●●●[/red]",
            "P1": "[orange]●●●[/orange]",
            "P2": "[yellow]●●[/yellow]",
            "P3": "[white]●[/white]",
        }
        return priority_map.get(priority, priority)
```

---

## Styling with TCSS (Textual CSS)

### Global Styles

```css
/* styles.tcss */

/* Base theme */
Screen {
    background: $surface;
    color: $text;
}

/* Panel styling */
.panel-title {
    background: $primary;
    color: $text;
    padding: 1;
    text-align: center;
    text-style: bold;
}

/* Worker status colors */
.status-active {
    color: $success;
}

.status-idle {
    color: $warning;
}

.status-unhealthy {
    color: $error;
}

/* Progress bar states */
ProgressBar {
    height: 1;
}

ProgressBar.warning {
    bar-color: $warning;
}

ProgressBar.critical {
    bar-color: $error;
}

/* Layout containers */
#workers {
    width: 1fr;
    height: 40%;
    border: solid $primary;
    padding: 1;
}

#subscriptions {
    width: 1fr;
    height: 40%;
    border: solid $primary;
    padding: 1;
}

#tasks {
    width: 100%;
    height: 30%;
    border: solid $primary;
    padding: 1;
}

#activity {
    width: 100%;
    height: 20%;
    border: solid $primary;
    padding: 1;
}

/* DataTable styling */
DataTable {
    height: 1fr;
}

DataTable > .datatable--header {
    background: $primary-darken-2;
    text-style: bold;
}

DataTable > .datatable--cursor {
    background: $primary;
}

/* RichLog styling */
RichLog {
    height: 1fr;
    border: none;
    scrollbar-gutter: stable;
}
```

### Loading Styles

```python
class PoolOptimizerDashboard(App):
    # Load external CSS file
    CSS_PATH = "styles.tcss"

    # Or inline CSS
    CSS = """
    Screen {
        background: #1a1a2e;
    }
    """
```

---

## Event Handling

### Keyboard Shortcuts

```python
class PoolOptimizerDashboard(App):
    BINDINGS = [
        ("q", "quit", "Quit"),
        ("r", "refresh", "Refresh"),
        ("w", "show_workers", "Workers"),
        ("s", "show_subscriptions", "Subscriptions"),
        ("t", "show_tasks", "Tasks"),
        ("c", "show_costs", "Costs"),
        ("escape", "back", "Back"),
        ("question_mark", "help", "Help"),
    ]

    def action_show_workers(self) -> None:
        """Switch to worker detail view."""
        self.push_screen(WorkerDetailScreen())

    def action_show_subscriptions(self) -> None:
        """Switch to subscription view."""
        self.push_screen(SubscriptionScreen())

    def action_back(self) -> None:
        """Go back to previous screen."""
        self.pop_screen()

    def action_help(self) -> None:
        """Show help modal."""
        self.push_screen(HelpScreen())
```

### Interactive Controls

```python
from textual.widgets import Button

class WorkerControls(Horizontal):
    """Interactive worker spawn/kill controls."""

    def compose(self) -> ComposeResult:
        yield Button("Spawn GLM", id="spawn-glm", variant="success")
        yield Button("Spawn Sonnet", id="spawn-sonnet", variant="primary")
        yield Button("Kill Selected", id="kill-worker", variant="error")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        """Handle button clicks."""
        button_id = event.button.id

        if button_id == "spawn-glm":
            self.spawn_worker("glm-4.7")
        elif button_id == "spawn-sonnet":
            self.spawn_worker("sonnet-4.5")
        elif button_id == "kill-worker":
            self.kill_selected_worker()

    async def spawn_worker(self, model: str) -> None:
        """Spawn a worker of specified model."""
        self.app.notify(f"Spawning {model} worker...")

        try:
            worker_id = await pool_manager.spawn_worker(model)
            self.app.notify(f"✓ Spawned {worker_id}", severity="success")
        except Exception as e:
            self.app.notify(f"✗ Failed: {e}", severity="error")

    async def kill_selected_worker(self) -> None:
        """Kill the currently selected worker."""
        # Get selected worker from table
        table = self.app.query_one("#worker-table", DataTable)
        worker_id = table.get_row_key_at_cursor()

        if worker_id:
            # Show confirmation modal
            if await self.app.push_screen_wait(
                ConfirmModal(f"Kill worker {worker_id}?")
            ):
                await pool_manager.kill_worker(worker_id)
                self.app.notify(f"✓ Killed {worker_id}", severity="success")
```

---

## Multi-Screen Navigation

### Screen Management

```python
from textual.screen import Screen

class MainDashboard(Screen):
    """Main dashboard screen."""

    BINDINGS = [
        ("w", "workers", "Workers"),
        ("s", "subscriptions", "Subscriptions"),
    ]

    def compose(self) -> ComposeResult:
        yield Header()
        # ... main dashboard widgets ...
        yield Footer()


class WorkerDetailScreen(Screen):
    """Detailed worker management screen."""

    BINDINGS = [
        ("escape", "back", "Back"),
        ("k", "kill", "Kill Worker"),
        ("g", "spawn_glm", "Spawn GLM"),
    ]

    def compose(self) -> ComposeResult:
        yield Header()
        yield WorkerTable()
        yield WorkerControls()
        yield Footer()

    def action_back(self) -> None:
        """Return to main dashboard."""
        self.app.pop_screen()


class PoolOptimizerApp(App):
    """Main application with screen management."""

    def on_mount(self) -> None:
        """Show main dashboard on startup."""
        self.push_screen(MainDashboard())
```

---

## Data Integration

### Pool Manager Interface

```python
# pool_manager.py
import asyncio
from dataclasses import dataclass
from typing import AsyncIterator

@dataclass
class WorkerStatus:
    id: str
    model: str
    status: str  # active, idle, unhealthy
    task: str | None
    uptime: str
    cost: float


class PoolManager:
    """Interface to control panel backend."""

    async def get_worker_status(self) -> list[WorkerStatus]:
        """Fetch current worker status."""
        # Query worker pool
        workers = await self._query_workers()
        return [
            WorkerStatus(
                id=w["id"],
                model=w["model"],
                status=w["status"],
                task=w.get("task"),
                uptime=self._format_uptime(w["started_at"]),
                cost=w["cost"],
            )
            for w in workers
        ]

    async def spawn_worker(self, model: str, workspace: str = None) -> str:
        """Spawn a new worker."""
        # Launch worker via beads-worker or tmux
        worker_id = await self._launch_worker(model, workspace)
        return worker_id

    async def kill_worker(self, worker_id: str) -> None:
        """Kill a worker."""
        await self._terminate_worker(worker_id)

    async def event_stream(self) -> AsyncIterator[dict]:
        """Stream activity events in real-time."""
        # Listen to log files or event queue
        while True:
            event = await self._get_next_event()
            yield event


# Global instance
pool_manager = PoolManager()
```

### Subscription Tracker Interface

```python
# subscription_tracker.py
from dataclasses import dataclass
from datetime import datetime, timedelta

@dataclass
class SubscriptionUsage:
    name: str
    used: int
    limit: int
    reset_time: str
    reset_datetime: datetime

    @property
    def percentage(self) -> float:
        return (self.used / self.limit) * 100 if self.limit > 0 else 0

    @property
    def remaining(self) -> int:
        return max(0, self.limit - self.used)


class SubscriptionTracker:
    """Track subscription usage across models."""

    async def get_usage(self, subscription_name: str) -> SubscriptionUsage:
        """Get current usage for a subscription."""
        # Query usage from tracker backend
        data = await self._query_usage(subscription_name)

        return SubscriptionUsage(
            name=subscription_name,
            used=data["used"],
            limit=data["limit"],
            reset_time=self._format_reset_time(data["reset_at"]),
            reset_datetime=data["reset_at"],
        )

    def _format_reset_time(self, reset_at: datetime) -> str:
        """Format time until reset."""
        delta = reset_at - datetime.now()
        hours, remainder = divmod(delta.seconds, 3600)
        minutes, _ = divmod(remainder, 60)
        return f"{hours}h {minutes}m"


# Global instance
subscription_tracker = SubscriptionTracker()
```

---

## Testing

### Snapshot Testing

```python
# test_dashboard.py
import pytest
from textual.pilot import Pilot
from dashboard import PoolOptimizerDashboard

@pytest.mark.asyncio
async def test_main_dashboard(snap_compare):
    """Test main dashboard layout."""
    app = PoolOptimizerDashboard()

    async with app.run_test() as pilot:
        # Wait for initial render
        await pilot.pause()

        # Compare screenshot
        assert await snap_compare(app)


@pytest.mark.asyncio
async def test_worker_spawn(snap_compare):
    """Test worker spawn interaction."""
    app = PoolOptimizerDashboard()

    async with app.run_test() as pilot:
        # Navigate to worker view
        await pilot.press("w")

        # Press spawn button
        await pilot.click("#spawn-glm")

        # Wait for worker to appear
        await pilot.pause(2)

        # Verify worker in table
        table = app.query_one("#worker-table")
        assert len(table.rows) > 0
```

### Unit Testing Components

```python
@pytest.mark.asyncio
async def test_subscription_panel_updates():
    """Test subscription panel reactive updates."""
    panel = SubscriptionPanel()

    # Mount in test app
    async with panel.app.run_test():
        # Update usage
        panel.usage_percentage = 75.0

        # Verify progress bar updated
        progress = panel.query_one("#claude-progress", ProgressBar)
        assert progress.progress == 75.0

        # Verify warning class applied
        assert progress.has_class("warning")
```

---

## Performance Optimization

### Efficient Updates

```python
# Only update changed rows, not entire table
class OptimizedWorkerTable(DataTable):
    _last_workers: dict[str, WorkerStatus] = {}

    async def update_workers(self) -> None:
        """Efficiently update only changed workers."""
        workers = await get_worker_status()

        for worker in workers:
            # Check if worker changed
            if (
                worker.id not in self._last_workers
                or self._last_workers[worker.id] != worker
            ):
                # Update or add row
                self.update_row(
                    worker.id,
                    worker.id,
                    worker.model,
                    worker.status,
                    worker.task or "-",
                    worker.uptime,
                )

            # Cache for next comparison
            self._last_workers[worker.id] = worker

        # Remove deleted workers
        current_ids = {w.id for w in workers}
        deleted_ids = set(self._last_workers.keys()) - current_ids
        for worker_id in deleted_ids:
            self.remove_row(worker_id)
            del self._last_workers[worker_id]
```

### Rate Limiting Updates

```python
from asyncio import Event, sleep

class ThrottledUpdate:
    """Throttle update frequency to avoid overwhelming UI."""

    def __init__(self, min_interval: float = 0.5):
        self.min_interval = min_interval
        self._last_update = 0.0
        self._pending_update = Event()

    async def request_update(self) -> None:
        """Request an update (throttled)."""
        self._pending_update.set()

    async def run(self, update_func):
        """Run update loop with throttling."""
        while True:
            await self._pending_update.wait()

            # Check if enough time passed
            now = time.time()
            elapsed = now - self._last_update

            if elapsed < self.min_interval:
                await sleep(self.min_interval - elapsed)

            # Perform update
            await update_func()
            self._last_update = time.time()
            self._pending_update.clear()
```

---

## Deployment

### Running as Daemon

```python
# run_dashboard.py
import asyncio
from dashboard import PoolOptimizerDashboard

def main():
    """Run dashboard."""
    app = PoolOptimizerDashboard()

    try:
        app.run()
    except KeyboardInterrupt:
        pass  # Graceful shutdown


if __name__ == "__main__":
    main()
```

### Systemd Service

```ini
# /etc/systemd/system/pool-dashboard.service
[Unit]
Description=Control Panel Dashboard
After=network.target

[Service]
Type=simple
User=coder
WorkingDirectory=/home/coder/control-panel
Environment="TERM=xterm-256color"
ExecStart=/home/coder/.venv/bin/python run_dashboard.py
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

### Tmux Session

```bash
# Launch dashboard in tmux
tmux new-session -d -s pool-dashboard \
  "cd /home/coder/control-panel && python run_dashboard.py"

# Attach to view
tmux attach -t pool-dashboard

# Detach: Ctrl+B, D
```

---

## Debugging

### Enable Console Output

```bash
# Terminal 1: Start console
textual console

# Terminal 2: Run app with dev mode
textual run --dev dashboard.py
```

### Add Debug Logging

```python
from textual import log

class WorkerTable(DataTable):
    async def update_workers(self) -> None:
        workers = await get_worker_status()
        log(f"Fetched {len(workers)} workers")

        for worker in workers:
            log(f"Worker {worker.id}: {worker.status}")
```

### Inspector Tool

```python
# Press Ctrl+\ to open inspector while running
# Shows widget tree, CSS, and reactive variables
```

---

## Next Steps

1. **Implement Phase 1** - Core dashboard with worker table and activity log
2. **Integrate with pool manager** - Connect to real worker data
3. **Add subscription tracking** - Implement progress bars and recommendations
4. **Test with real workload** - Spawn 20+ workers and verify performance
5. **Iterate on UX** - Gather feedback and refine layouts
6. **Add cost analytics** - Implement charts and insights view
7. **Deploy production** - Set up as service with monitoring

---

## Resources

- **Textual Documentation**: https://textual.textualize.io/
- **Widget Gallery**: https://textual.textualize.io/widget_gallery/
- **Examples**: https://github.com/Textualize/textual/tree/main/examples
- **Discord Community**: https://discord.gg/Enf6Z3qhVr

---

## Troubleshooting

### Common Issues

**Issue**: Widget not updating
**Solution**: Ensure using `reactive` variables and `watch_*` methods

**Issue**: Slow rendering
**Solution**: Use dirty region tracking, avoid full redraws

**Issue**: Async functions not running
**Solution**: Use `self.run_worker()` to schedule async tasks

**Issue**: Keyboard shortcuts not working
**Solution**: Check `BINDINGS` list and action method names match

**Issue**: Terminal colors look wrong
**Solution**: Set `TERM=xterm-256color` or use `--dev` mode

---

This implementation guide provides everything needed to build a production-quality control panel dashboard with Textual. Start with the basic scaffold and incrementally add features following the component examples.
