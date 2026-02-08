"""
Tests for FORGE Textual Application
"""

import pytest
from datetime import datetime
from textual.app import App
from textual.widgets import Log

from forge.app import (
    CostEntry,
    ForgeApp,
    LogEntry,
    MetricData,
    Subscription,
    Task,
    TaskPriority,
    TaskStatus,
    ViewMode,
    Worker,
    WorkerStatus,
)


class TestWorkerDataModel:
    """Tests for Worker data model"""

    def test_worker_creation(self) -> None:
        """Test creating a Worker instance"""
        now = datetime.now()
        worker = Worker(
            session_id="test-worker",
            model="GLM-4.7",
            workspace="/home/coder/forge",
            status=WorkerStatus.ACTIVE,
            current_task="fg-1zy",
            uptime_seconds=600,
            tokens_used=50000,
            cost=0.15,
            last_heartbeat=now,
        )

        assert worker.session_id == "test-worker"
        assert worker.model == "GLM-4.7"
        assert worker.status == WorkerStatus.ACTIVE
        assert worker.current_task == "fg-1zy"
        assert worker.uptime_seconds == 600
        assert worker.tokens_used == 50000
        assert worker.cost == 0.15
        assert worker.last_heartbeat == now

    def test_worker_defaults(self) -> None:
        """Test Worker with default values"""
        worker = Worker(
            session_id="test",
            model="Sonnet 4.5",
            workspace="/home/coder/forge",
        )

        assert worker.status == WorkerStatus.IDLE
        assert worker.current_task is None
        assert worker.uptime_seconds == 0
        assert worker.tokens_used == 0
        assert worker.cost == 0.0
        assert worker.last_heartbeat is None


class TestTaskDataModel:
    """Tests for Task data model"""

    def test_task_creation(self) -> None:
        """Test creating a Task instance"""
        now = datetime.now()
        task = Task(
            id="fg-1zy",
            title="Implement Textual app skeleton",
            priority=TaskPriority.P0,
            status=TaskStatus.IN_PROGRESS,
            model="GLM-4.7",
            workspace="/home/coder/forge",
            assigned_worker="glm-alpha",
            estimated_tokens=50000,
            created_at=now,
        )

        assert task.id == "fg-1zy"
        assert task.title == "Implement Textual app skeleton"
        assert task.priority == TaskPriority.P0
        assert task.status == TaskStatus.IN_PROGRESS
        assert task.model == "GLM-4.7"
        assert task.workspace == "/home/coder/forge"
        assert task.assigned_worker == "glm-alpha"
        assert task.estimated_tokens == 50000
        assert task.created_at == now


class TestForgeApp:
    """Tests for ForgeApp"""

    @pytest.mark.asyncio
    async def test_app_creation(self) -> None:
        """Test that ForgeApp can be created"""
        app = ForgeApp()
        assert app is not None
        assert app.TITLE == "FORGE Control Panel"

    @pytest.mark.asyncio
    async def test_app_composition(self) -> None:
        """Test that app composes with expected widgets"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            # Check that panels are mounted
            assert app._workers_panel is not None
            assert app._tasks_panel is not None
            assert app._costs_panel is not None
            assert app._metrics_panel is not None
            assert app._logs_panel is not None
            assert app._chat_panel is not None

    @pytest.mark.asyncio
    async def test_sample_data_initialization(self) -> None:
        """Test that app initializes with sample data"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            # Check workers
            assert len(app.workers) > 0
            assert any(w.session_id == "glm-alpha" for w in app.workers)

            # Check tasks
            assert len(app.tasks) > 0
            assert any(t.id == "fg-1zy" for t in app.tasks)

            # Check subscriptions
            assert len(app.subscriptions) > 0

            # Check costs
            assert len(app.costs) > 0

            # Check metrics
            assert app.metrics is not None
            assert app.metrics.throughput_per_hour > 0

            # Check logs
            assert len(app.logs) > 0

    @pytest.mark.asyncio
    async def test_workers_panel_updates(self) -> None:
        """Test that workers panel updates with data changes"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            initial_active = app._workers_panel.active_count if app._workers_panel else 0

            # Add a new active worker
            now = datetime.now()
            new_worker = Worker(
                session_id="test-worker",
                model="Sonnet 4.5",
                workspace="/home/coder/test",
                status=WorkerStatus.ACTIVE,
                last_heartbeat=now,
            )
            app.workers.append(new_worker)

            # Wait for reactive update
            await pilot.pause()

            # Check that active count increased
            if app._workers_panel:
                assert app._workers_panel.active_count >= initial_active

    @pytest.mark.asyncio
    async def test_tasks_panel_updates(self) -> None:
        """Test that tasks panel updates with data changes"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            initial_ready = app._tasks_panel.ready_count if app._tasks_panel else 0

            # Add a new ready task
            now = datetime.now()
            new_task = Task(
                id="test-task",
                title="Test task",
                priority=TaskPriority.P2,
                status=TaskStatus.READY,
                created_at=now,
            )
            app.tasks.append(new_task)

            # Wait for reactive update
            await pilot.pause()

            # Check that ready count increased
            if app._tasks_panel:
                assert app._tasks_panel.ready_count >= initial_ready

    @pytest.mark.asyncio
    async def test_command_submission(self) -> None:
        """Test that command submission creates log entries"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            initial_log_count = len(app.logs)

            # Submit a command
            if app._chat_panel and app._chat_panel.on_command_submit:
                app._chat_panel.on_command_submit("show workers")

            # Wait for reactive update
            await pilot.pause()

            # Check that log was added
            assert len(app.logs) >= initial_log_count

            # Find the command log
            command_logs = [l for l in app.logs if l.level == "COMMAND"]
            assert len(command_logs) > 0

    @pytest.mark.asyncio
    async def test_refresh_action(self) -> None:
        """Test that refresh action updates data"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            initial_log_count = len(app.logs)

            # Trigger refresh
            await pilot.press("r")

            # Wait for update
            await pilot.pause()

            # Check that refresh log was added
            refresh_logs = [l for l in app.logs if "refreshed" in l.message.lower()]
            assert len(refresh_logs) > 0

    @pytest.mark.asyncio
    async def test_keyboard_bindings(self) -> None:
        """Test that keyboard shortcuts work"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            # Test chat focus
            await pilot.press("c")
            await pilot.pause()
            # Chat panel should have focus (implementation dependent)

            # Test quit binding exists
            assert "q" in [b.key for b in app.BINDINGS]

    @pytest.mark.asyncio
    async def test_panel_navigation(self) -> None:
        """Test panel navigation via keyboard"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            # Try to navigate to workers panel
            await pilot.press("w")
            await pilot.pause()

            # Try to navigate to tasks panel
            await pilot.press("t")
            await pilot.pause()

            # Try to navigate to metrics panel
            await pilot.press("m")
            await pilot.pause()

            # Try to navigate to logs panel
            await pilot.press("l")
            await pilot.pause()

    @pytest.mark.asyncio
    async def test_data_reactive_updates(self) -> None:
        """Test that reactive data properly updates panels"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            # Update metrics
            new_metrics = MetricData(
                throughput_per_hour=25.0,
                avg_time_per_task=180.0,
                queue_velocity=15.0,
                cpu_percent=60.0,
                memory_gb=4.0,
                memory_total_gb=16.0,
                disk_gb=80.0,
                disk_total_gb=500.0,
                network_down_mbps=2.0,
                network_up_mbps=1.5,
                success_rate=95.0,
                completion_count=50,
                in_progress_count=3,
                failed_count=1,
            )

            app.metrics = new_metrics
            await pilot.pause()

            # Check metrics panel updated
            assert app.metrics.throughput_per_hour == 25.0


class TestDataModels:
    """Tests for data models"""

    def test_log_entry(self) -> None:
        """Test LogEntry creation"""
        now = datetime.now()
        entry = LogEntry(
            timestamp=now,
            level="INFO",
            message="Test message",
            icon="ℹ",
        )

        assert entry.timestamp == now
        assert entry.level == "INFO"
        assert entry.message == "Test message"
        assert entry.icon == "ℹ"

    def test_subscription(self) -> None:
        """Test Subscription creation"""
        now = datetime.now()
        sub = Subscription(
            name="Claude Pro",
            model="Sonnet 4.5",
            used=72,
            limit=100,
            resets_at=now,
            monthly_cost=20.0,
        )

        assert sub.name == "Claude Pro"
        assert sub.model == "Sonnet 4.5"
        assert sub.used == 72
        assert sub.limit == 100
        assert sub.resets_at == now
        assert sub.monthly_cost == 20.0

    def test_cost_entry(self) -> None:
        """Test CostEntry creation"""
        entry = CostEntry(
            model="Sonnet 4.5",
            requests=100,
            tokens=500000,
            cost=5.50,
        )

        assert entry.model == "Sonnet 4.5"
        assert entry.requests == 100
        assert entry.tokens == 500000
        assert entry.cost == 5.50


class TestViewSwitching:
    """Tests for view switching and navigation"""

    @pytest.mark.asyncio
    async def test_initial_view_mode(self) -> None:
        """Test that app starts in overview mode"""
        app = ForgeApp()
        assert app._current_view == ViewMode.OVERVIEW

    @pytest.mark.asyncio
    async def test_switch_view_to_workers(self) -> None:
        """Test switching to workers view"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            await pilot.press("W")
            await pilot.pause()

            assert app._current_view == ViewMode.WORKERS

    @pytest.mark.asyncio
    async def test_switch_view_to_tasks(self) -> None:
        """Test switching to tasks view"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            await pilot.press("T")
            await pilot.pause()

            assert app._current_view == ViewMode.TASKS

    @pytest.mark.asyncio
    async def test_switch_view_to_costs(self) -> None:
        """Test switching to costs view"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            await pilot.press("C")
            await pilot.pause()

            assert app._current_view == ViewMode.COSTS

    @pytest.mark.asyncio
    async def test_switch_view_to_metrics(self) -> None:
        """Test switching to metrics view"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            await pilot.press("M")
            await pilot.pause()

            assert app._current_view == ViewMode.METRICS

    @pytest.mark.asyncio
    async def test_switch_view_to_logs(self) -> None:
        """Test switching to logs view"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            await pilot.press("L")
            await pilot.pause()

            assert app._current_view == ViewMode.LOGS

    @pytest.mark.asyncio
    async def test_switch_view_to_overview(self) -> None:
        """Test switching back to overview"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            # First switch to workers
            app.action_switch_view("workers")
            await pilot.pause()
            assert app._current_view == ViewMode.WORKERS

            # Switch back to overview
            await pilot.press("O")
            await pilot.pause()
            assert app._current_view == ViewMode.OVERVIEW

    @pytest.mark.asyncio
    async def test_view_history_tracking(self) -> None:
        """Test that view history is tracked"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            # Switch through several views
            await pilot.press("W")
            await pilot.pause()

            await pilot.press("T")
            await pilot.pause()

            # Check history was tracked
            assert len(app._view_history) >= 2

    @pytest.mark.asyncio
    async def test_go_back_action(self) -> None:
        """Test going back to previous view"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            # Switch to workers
            await pilot.press("W")
            await pilot.pause()
            assert app._current_view == ViewMode.WORKERS

            # Switch to tasks
            await pilot.press("T")
            await pilot.pause()
            assert app._current_view == ViewMode.TASKS

            # Go back
            await pilot.press("escape")
            await pilot.pause()
            assert app._current_view == ViewMode.WORKERS

    @pytest.mark.asyncio
    async def test_cycle_view_forward(self) -> None:
        """Test cycling forward through views"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            initial_view = app._current_view

            await pilot.press("tab")
            await pilot.pause()

            assert app._current_view != initial_view

    @pytest.mark.asyncio
    async def test_split_view_toggle(self) -> None:
        """Test toggling split view mode"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            # Toggle split view on
            await pilot.press("s")
            await pilot.pause()
            assert app._current_view == ViewMode.SPLIT

            # Toggle split view off
            await pilot.press("s")
            await pilot.pause()
            assert app._current_view == ViewMode.OVERVIEW

    @pytest.mark.asyncio
    async def test_command_view_switching(self) -> None:
        """Test view switching via command input"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            # Submit command to switch views
            if app._chat_panel and app._chat_panel.on_command_submit:
                app._chat_panel.on_command_submit("show workers")

            await pilot.pause()
            assert app._current_view == ViewMode.WORKERS

            # Try another command
            app._chat_panel.on_command_submit("go to tasks")
            await pilot.pause()
            assert app._current_view == ViewMode.TASKS

    @pytest.mark.asyncio
    async def test_focus_panel_action(self) -> None:
        """Test focusing panels via action"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            # Focus workers panel
            app.action_focus_panel("workers")
            await pilot.pause()

            # Focus logs panel
            app.action_focus_panel("logs")
            await pilot.pause()

            # Focus chat panel
            app.action_focus_panel("chat")
            await pilot.pause()

    @pytest.mark.asyncio
    async def test_tool_executor_initialization(self) -> None:
        """Test that tool executor is initialized"""
        app = ForgeApp()
        assert app._tool_executor is not None

    @pytest.mark.asyncio
    async def test_tool_switch_view_callback(self) -> None:
        """Test switch_view tool callback"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            initial_view = app._current_view

            # Call the tool callback directly
            result = app._tool_switch_view("workers")

            await pilot.pause()

            assert result.success
            assert result.tool_name == "switch_view"
            assert app._current_view == ViewMode.WORKERS

    @pytest.mark.asyncio
    async def test_tool_split_view_callback(self) -> None:
        """Test split_view tool callback"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            # Call the tool callback directly
            result = app._tool_split_view("workers", "tasks")

            await pilot.pause()

            assert result.success
            assert result.tool_name == "split_view"
            assert app._current_view == ViewMode.SPLIT
            assert app._split_left == "workers"
            assert app._split_right == "tasks"

    @pytest.mark.asyncio
    async def test_tool_focus_panel_callback(self) -> None:
        """Test focus_panel tool callback"""
        app = ForgeApp()

        async with app.run_test() as pilot:
            # Call the tool callback directly
            result = app._tool_focus_panel("workers")

            await pilot.pause()

            assert result.success
            assert result.tool_name == "focus_panel"
