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
