"""
Tests for the metrics storage module.

Tests cover:
- Worker metric storage and retrieval
- Cost event storage and retrieval
- Batch insert functionality
- Index verification
- Retention policy cleanup
- Query methods for summaries
"""

import asyncio
import tempfile
from datetime import datetime, timedelta
from pathlib import Path

import pytest

from forge.metrics import (
    CostEvent,
    CostSummaryPeriod,
    MetricType,
    MetricUnit,
    MetricsSummary,
    MetricsTracker,
    WorkerMetric,
    get_metrics_tracker,
)


# =============================================================================
# Data Model Tests
# =============================================================================


class TestDataModels:
    """Tests for metrics data models"""

    def test_worker_metric_creation(self):
        """Test creating a WorkerMetric"""
        now = datetime.now()
        metric = WorkerMetric(
            timestamp=now,
            worker_id="worker-abc123",
            metric_type=MetricType.CPU_PERCENT,
            value=45.5,
            unit=MetricUnit.PERCENT,
            tags={"host": "server1"},
        )

        assert metric.timestamp == now
        assert metric.worker_id == "worker-abc123"
        assert metric.metric_type == MetricType.CPU_PERCENT
        assert metric.value == 45.5
        assert metric.unit == MetricUnit.PERCENT
        assert metric.tags == {"host": "server1"}

    def test_worker_metric_to_dict(self):
        """Test converting WorkerMetric to dictionary"""
        now = datetime.now()
        metric = WorkerMetric(
            timestamp=now,
            worker_id="worker-xyz",
            metric_type=MetricType.MEMORY_BYTES,
            value=1024 * 1024 * 512,  # 512 MB
            unit=MetricUnit.BYTES,
        )

        result = metric.to_dict()

        assert result["timestamp"] == now.isoformat()
        assert result["worker_id"] == "worker-xyz"
        assert result["metric_type"] == "memory_bytes"
        assert result["value"] == 1024 * 1024 * 512
        assert result["unit"] == "bytes"
        assert result["tags"] == {}

    def test_cost_event_creation(self):
        """Test creating a CostEvent"""
        now = datetime.now()
        event = CostEvent(
            timestamp=now,
            worker_id="worker-1",
            model="claude-sonnet-4-5",
            input_tokens=1000,
            output_tokens=500,
            total_tokens=1500,
            cost=0.0105,
            event_type="api_call",
        )

        assert event.worker_id == "worker-1"
        assert event.model == "claude-sonnet-4-5"
        assert event.total_tokens == 1500
        assert event.cost == 0.0105

    def test_metrics_summary_creation(self):
        """Test creating a MetricsSummary"""
        now = datetime.now()
        summary = MetricsSummary(
            period_start=now - timedelta(hours=1),
            period_end=now,
            worker_id="worker-1",
            metric_type=MetricType.CPU_PERCENT,
            avg_value=50.0,
            min_value=10.0,
            max_value=90.0,
            count=100,
            unit=MetricUnit.PERCENT,
        )

        assert summary.worker_id == "worker-1"
        assert summary.avg_value == 50.0
        assert summary.count == 100


# =============================================================================
# Metrics Tracker Tests
# =============================================================================


class TestMetricsTracker:
    """Tests for MetricsTracker functionality"""

    @pytest.fixture
    def tracker(self):
        """Create a temporary tracker for each test"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test_metrics.db"
            tracker = MetricsTracker(
                db_path=db_path,
                retention_days=30,
            )
            yield tracker
            # Cleanup is handled by tempfile

    @pytest.mark.asyncio
    async def test_init_db_creates_tables(self, tracker):
        """Test that initialization creates the correct tables"""
        import sqlite3

        conn = sqlite3.connect(tracker._db_path)
        cursor = conn.cursor()

        # Check worker_metrics table
        cursor.execute("""
            SELECT name FROM sqlite_master
            WHERE type='table' AND name='worker_metrics'
        """)
        assert cursor.fetchone() is not None

        # Check cost_events table
        cursor.execute("""
            SELECT name FROM sqlite_master
            WHERE type='table' AND name='cost_events'
        """)
        assert cursor.fetchone() is not None

        conn.close()

    @pytest.mark.asyncio
    async def test_init_db_creates_indexes(self, tracker):
        """Test that initialization creates the correct indexes"""
        import sqlite3

        conn = sqlite3.connect(tracker._db_path)
        cursor = conn.cursor()

        # Check for expected indexes
        cursor.execute("""
            SELECT name FROM sqlite_master
            WHERE type='index' AND name LIKE 'idx_%'
            ORDER BY name
        """)

        indexes = [row[0] for row in cursor.fetchall()]

        # Worker metrics indexes
        assert "idx_worker_metrics_timestamp" in indexes
        assert "idx_worker_metrics_worker" in indexes
        assert "idx_worker_metrics_type" in indexes
        assert "idx_worker_metrics_worker_type" in indexes

        # Cost events indexes
        assert "idx_cost_events_timestamp" in indexes
        assert "idx_cost_events_worker" in indexes
        assert "idx_cost_events_model" in indexes

        conn.close()

    @pytest.mark.asyncio
    async def test_add_and_flush_metric(self, tracker):
        """Test adding a metric and flushing to database"""
        now = datetime.now()
        metric = WorkerMetric(
            timestamp=now,
            worker_id="worker-1",
            metric_type=MetricType.CPU_PERCENT,
            value=45.5,
            unit=MetricUnit.PERCENT,
        )

        tracker.add_metric(metric)
        tracker.flush()

        # Verify metric was stored
        summary = tracker.get_metrics_summary(
            worker_id="worker-1",
            metric_type=MetricType.CPU_PERCENT,
        )

        assert summary.count == 1
        assert summary.avg_value == 45.5

    @pytest.mark.asyncio
    async def test_add_cost_event(self, tracker):
        """Test adding a cost event"""
        now = datetime.now()
        event = CostEvent(
            timestamp=now,
            worker_id="worker-1",
            model="claude-sonnet-4-5",
            input_tokens=1000,
            output_tokens=500,
            total_tokens=1500,
            cost=0.0105,
        )

        tracker.add_cost_event(event)
        tracker.flush()

        # Verify event was stored
        summary = tracker.get_cost_summary(
            start=now - timedelta(hours=1),
            end=now + timedelta(hours=1),
        )

        assert summary.total_requests == 1
        assert summary.total_cost == pytest.approx(0.0105, rel=1e-6)
        assert summary.total_tokens == 1500

    @pytest.mark.asyncio
    async def test_get_metrics_summary_filtered(self, tracker):
        """Test getting metrics summary with filters"""
        now = datetime.now()

        # Add multiple metrics
        for i in range(5):
            metric = WorkerMetric(
                timestamp=now - timedelta(minutes=i),
                worker_id="worker-1",
                metric_type=MetricType.CPU_PERCENT,
                value=40.0 + i,
                unit=MetricUnit.PERCENT,
            )
            tracker.add_metric(metric)

        # Add a metric for different worker
        metric = WorkerMetric(
            timestamp=now,
            worker_id="worker-2",
            metric_type=MetricType.CPU_PERCENT,
            value=80.0,
            unit=MetricUnit.PERCENT,
        )
        tracker.add_metric(metric)

        tracker.flush()

        # Get summary for worker-1 only
        summary = tracker.get_metrics_summary(
            worker_id="worker-1",
            metric_type=MetricType.CPU_PERCENT,
        )

        assert summary.count == 5
        assert summary.worker_id == "worker-1"
        assert 40.0 <= summary.avg_value <= 44.0

    @pytest.mark.asyncio
    async def test_get_worker_metrics(self, tracker):
        """Test getting raw metrics for a worker"""
        now = datetime.now()

        # Add metrics with different types
        cpu_metric = WorkerMetric(
            timestamp=now,
            worker_id="worker-1",
            metric_type=MetricType.CPU_PERCENT,
            value=50.0,
            unit=MetricUnit.PERCENT,
        )
        mem_metric = WorkerMetric(
            timestamp=now,
            worker_id="worker-1",
            metric_type=MetricType.MEMORY_PERCENT,
            value=60.0,
            unit=MetricUnit.PERCENT,
        )

        tracker.add_metric(cpu_metric)
        tracker.add_metric(mem_metric)
        tracker.flush()

        # Get all metrics for worker
        metrics = tracker.get_worker_metrics("worker-1")

        assert len(metrics) == 2
        metric_types = {m.metric_type for m in metrics}
        assert MetricType.CPU_PERCENT in metric_types
        assert MetricType.MEMORY_PERCENT in metric_types

    @pytest.mark.asyncio
    async def test_get_worker_metrics_filtered_by_type(self, tracker):
        """Test getting worker metrics filtered by type"""
        now = datetime.now()

        cpu_metric = WorkerMetric(
            timestamp=now,
            worker_id="worker-1",
            metric_type=MetricType.CPU_PERCENT,
            value=50.0,
            unit=MetricUnit.PERCENT,
        )
        mem_metric = WorkerMetric(
            timestamp=now,
            worker_id="worker-1",
            metric_type=MetricType.MEMORY_PERCENT,
            value=60.0,
            unit=MetricUnit.PERCENT,
        )

        tracker.add_metric(cpu_metric)
        tracker.add_metric(mem_metric)
        tracker.flush()

        # Get only CPU metrics
        metrics = tracker.get_worker_metrics(
            "worker-1",
            metric_type=MetricType.CPU_PERCENT,
        )

        assert len(metrics) == 1
        assert metrics[0].metric_type == MetricType.CPU_PERCENT

    @pytest.mark.asyncio
    async def test_get_all_workers(self, tracker):
        """Test getting list of all workers"""
        now = datetime.now()

        workers = ["worker-1", "worker-2", "worker-3"]
        for worker_id in workers:
            metric = WorkerMetric(
                timestamp=now,
                worker_id=worker_id,
                metric_type=MetricType.CPU_PERCENT,
                value=50.0,
                unit=MetricUnit.PERCENT,
            )
            tracker.add_metric(metric)

        tracker.flush()

        all_workers = tracker.get_all_workers()
        assert len(all_workers) == 3
        assert "worker-1" in all_workers
        assert "worker-2" in all_workers
        assert "worker-3" in all_workers

    @pytest.mark.asyncio
    async def test_cost_summary_by_model(self, tracker):
        """Test cost summary breakdown by model"""
        now = datetime.now()

        # Add events for different models
        models = ["claude-sonnet-4-5", "gpt-4o", "deepseek-v3"]
        for model in models:
            event = CostEvent(
                timestamp=now,
                worker_id="worker-1",
                model=model,
                input_tokens=1000,
                output_tokens=500,
                total_tokens=1500,
                cost=0.01,
            )
            tracker.add_cost_event(event)

        tracker.flush()

        summary = tracker.get_cost_summary(
            start=now - timedelta(hours=1),
            end=now + timedelta(hours=1),
        )

        assert len(summary.by_model) == 3
        assert "claude-sonnet-4-5" in summary.by_model
        assert "gpt-4o" in summary.by_model
        assert "deepseek-v3" in summary.by_model

    @pytest.mark.asyncio
    async def test_cost_summary_by_worker(self, tracker):
        """Test cost summary breakdown by worker"""
        now = datetime.now()

        # Add events for different workers
        for worker_id in ["worker-1", "worker-2"]:
            event = CostEvent(
                timestamp=now,
                worker_id=worker_id,
                model="claude-sonnet-4-5",
                input_tokens=1000,
                output_tokens=500,
                total_tokens=1500,
                cost=0.01,
            )
            tracker.add_cost_event(event)

        tracker.flush()

        summary = tracker.get_cost_summary()

        assert len(summary.by_worker) == 2
        assert "worker-1" in summary.by_worker
        assert "worker-2" in summary.by_worker

    @pytest.mark.asyncio
    async def test_get_costs_last_24h(self, tracker):
        """Test getting costs for last 24 hours"""
        now = datetime.now()

        # Add event within 24h window
        recent_event = CostEvent(
            timestamp=now - timedelta(hours=1),
            worker_id="worker-1",
            model="claude-sonnet-4-5",
            input_tokens=1000,
            output_tokens=500,
            total_tokens=1500,
            cost=0.0105,
        )
        tracker.add_cost_event(recent_event)

        # Add event outside 24h window
        old_event = CostEvent(
            timestamp=now - timedelta(hours=25),
            worker_id="worker-old",
            model="claude-sonnet-4-5",
            input_tokens=1000,
            output_tokens=500,
            total_tokens=1500,
            cost=0.0105,
        )
        tracker.add_cost_event(old_event)

        tracker.flush()

        summary = tracker.get_costs_last_24h()
        assert summary.total_requests == 1

    @pytest.mark.asyncio
    async def test_get_costs_today(self, tracker):
        """Test getting costs for today"""
        now = datetime.now()

        # Add event today
        event = CostEvent(
            timestamp=now,
            worker_id="worker-1",
            model="claude-sonnet-4-5",
            input_tokens=1000,
            output_tokens=500,
            total_tokens=1500,
            cost=0.0105,
        )
        tracker.add_cost_event(event)

        tracker.flush()

        summary = tracker.get_costs_today()
        assert summary.total_requests == 1

    @pytest.mark.asyncio
    async def test_get_costs_by_worker(self, tracker):
        """Test getting costs for a specific worker"""
        now = datetime.now()

        # Add events for worker-1
        event1 = CostEvent(
            timestamp=now,
            worker_id="worker-1",
            model="claude-sonnet-4-5",
            input_tokens=1000,
            output_tokens=500,
            total_tokens=1500,
            cost=0.0105,
        )
        tracker.add_cost_event(event1)

        # Add event for worker-2
        event2 = CostEvent(
            timestamp=now,
            worker_id="worker-2",
            model="claude-sonnet-4-5",
            input_tokens=2000,
            output_tokens=1000,
            total_tokens=3000,
            cost=0.021,
        )
        tracker.add_cost_event(event2)

        tracker.flush()

        breakdown = tracker.get_costs_by_worker("worker-1")

        assert breakdown["worker_id"] == "worker-1"
        assert breakdown["total_requests"] == 1
        assert breakdown["total_cost"] == pytest.approx(0.0105, rel=1e-6)


# =============================================================================
# Batch Insert Tests
# =============================================================================


class TestBatchInsert:
    """Tests for batch insert functionality"""

    @pytest.mark.asyncio
    async def test_batch_insert_on_max_size(self):
        """Test that events are flushed when max batch size is reached"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = MetricsTracker(db_path=db_path, max_batch_size=5)

            # Add 5 metrics (should trigger flush)
            for i in range(5):
                metric = WorkerMetric(
                    timestamp=datetime.now(),
                    worker_id=f"worker-{i}",
                    metric_type=MetricType.CPU_PERCENT,
                    value=50.0,
                    unit=MetricUnit.PERCENT,
                )
                tracker.add_metric(metric)

            # Check that metrics were flushed
            summary = tracker.get_metrics_summary()
            assert summary.count == 5

    @pytest.mark.asyncio
    async def test_periodic_flush(self):
        """Test periodic background flush"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = MetricsTracker(db_path=db_path, batch_interval=0.1)  # 100ms

            tracker.start()

            # Add a metric
            metric = WorkerMetric(
                timestamp=datetime.now(),
                worker_id="worker-1",
                metric_type=MetricType.CPU_PERCENT,
                value=50.0,
                unit=MetricUnit.PERCENT,
            )
            tracker.add_metric(metric)

            # Wait for periodic flush
            await asyncio.sleep(0.2)

            # Check that metric was flushed
            summary = tracker.get_metrics_summary()
            assert summary.count == 1

            await tracker.stop()

    @pytest.mark.asyncio
    async def test_flush_on_stop(self):
        """Test that pending events are flushed on stop"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = MetricsTracker(db_path=db_path, batch_interval=10.0)

            tracker.start()

            # Add a metric
            metric = WorkerMetric(
                timestamp=datetime.now(),
                worker_id="worker-1",
                metric_type=MetricType.CPU_PERCENT,
                value=50.0,
                unit=MetricUnit.PERCENT,
            )
            tracker.add_metric(metric)

            # Stop should flush
            await tracker.stop()

            # Check that metric was flushed
            summary = tracker.get_metrics_summary()
            assert summary.count == 1


# =============================================================================
# Retention Policy Tests
# =============================================================================


class TestRetentionPolicy:
    """Tests for retention policy functionality"""

    @pytest.mark.asyncio
    async def test_apply_retention_policy(self):
        """Test that old metrics are deleted"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = MetricsTracker(db_path=db_path, retention_days=30)

            now = datetime.now()

            # Add old metric (31 days ago)
            old_metric = WorkerMetric(
                timestamp=now - timedelta(days=31),
                worker_id="worker-old",
                metric_type=MetricType.CPU_PERCENT,
                value=50.0,
                unit=MetricUnit.PERCENT,
            )
            tracker.add_metric(old_metric)

            # Add recent metric (1 day ago)
            recent_metric = WorkerMetric(
                timestamp=now - timedelta(days=1),
                worker_id="worker-recent",
                metric_type=MetricType.CPU_PERCENT,
                value=50.0,
                unit=MetricUnit.PERCENT,
            )
            tracker.add_metric(recent_metric)

            tracker.flush()

            # Apply retention policy
            deleted = tracker.apply_retention_policy()

            # Should have deleted the old metric
            assert deleted > 0

            # Check that only recent metric remains
            all_workers = tracker.get_all_workers()
            assert "worker-recent" in all_workers
            assert "worker-old" not in all_workers

    @pytest.mark.asyncio
    async def test_get_retention_info(self):
        """Test getting retention information"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = MetricsTracker(db_path=db_path, retention_days=30)

            now = datetime.now()

            # Add some data
            for i in range(10):
                metric = WorkerMetric(
                    timestamp=now - timedelta(hours=i),
                    worker_id="worker-1",
                    metric_type=MetricType.CPU_PERCENT,
                    value=50.0,
                    unit=MetricUnit.PERCENT,
                )
                tracker.add_metric(metric)

            for i in range(5):
                event = CostEvent(
                    timestamp=now - timedelta(hours=i),
                    worker_id="worker-1",
                    model="claude-sonnet-4-5",
                    input_tokens=1000,
                    output_tokens=500,
                    total_tokens=1500,
                    cost=0.01,
                )
                tracker.add_cost_event(event)

            tracker.flush()

            # Get retention info
            info = tracker.get_retention_info()

            assert info["retention_days"] == 30
            assert info["metrics_count"] == 10
            assert info["cost_events_count"] == 5
            assert info["database_size_bytes"] > 0
            assert info["database_size_mb"] > 0


# =============================================================================
# Metric Type Tests
# =============================================================================


class TestMetricTypes:
    """Tests for different metric types"""

    @pytest.mark.asyncio
    async def test_all_metric_types(self):
        """Test storing and retrieving all metric types"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = MetricsTracker(db_path=db_path)

            now = datetime.now()

            # Test all metric types
            metric_types = [
                (MetricType.CPU_PERCENT, MetricUnit.PERCENT, 50.0),
                (MetricType.MEMORY_PERCENT, MetricUnit.PERCENT, 60.0),
                (MetricType.MEMORY_BYTES, MetricUnit.BYTES, 1024 * 1024 * 512),
                (MetricType.DISK_USAGE_BYTES, MetricUnit.BYTES, 1024 * 1024 * 1024 * 100),
                (MetricType.TASKS_COMPLETED, MetricUnit.COUNT, 42),
                (MetricType.API_CALLS_COUNT, MetricUnit.COUNT, 100),
                (MetricType.TOKENS_PROCESSED, MetricUnit.TOKENS, 50000),
                (MetricType.HEALTH_CHECK_STATUS, MetricUnit.STATUS, 1),
            ]

            for metric_type, unit, value in metric_types:
                metric = WorkerMetric(
                    timestamp=now,
                    worker_id="worker-1",
                    metric_type=metric_type,
                    value=value,
                    unit=unit,
                )
                tracker.add_metric(metric)

            tracker.flush()

            # Verify all metrics were stored
            for metric_type, unit, value in metric_types:
                summary = tracker.get_metrics_summary(
                    worker_id="worker-1",
                    metric_type=metric_type,
                )
                assert summary.count == 1
                assert summary.avg_value == value
                assert summary.unit == unit

    @pytest.mark.asyncio
    async def test_metric_with_tags(self):
        """Test metrics with tags"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = MetricsTracker(db_path=db_path)

            now = datetime.now()
            metric = WorkerMetric(
                timestamp=now,
                worker_id="worker-1",
                metric_type=MetricType.CPU_PERCENT,
                value=50.0,
                unit=MetricUnit.PERCENT,
                tags={"host": "server1", "region": "us-east"},
            )
            tracker.add_metric(metric)
            tracker.flush()

            # Retrieve and verify tags
            metrics = tracker.get_worker_metrics("worker-1")
            assert len(metrics) == 1
            assert metrics[0].tags == {"host": "server1", "region": "us-east"}


# =============================================================================
# Edge Cases Tests
# =============================================================================


class TestEdgeCases:
    """Tests for edge cases and error handling"""

    @pytest.mark.asyncio
    async def test_empty_database_queries(self):
        """Test queries on empty database"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = MetricsTracker(db_path=db_path)

            # Query empty database
            summary = tracker.get_metrics_summary()
            assert summary.count == 0
            assert summary.avg_value == 0.0

            cost_summary = tracker.get_cost_summary()
            assert cost_summary.total_requests == 0
            assert cost_summary.total_cost == 0.0

    @pytest.mark.asyncio
    async def test_multiple_workers_same_metric(self):
        """Test metrics from multiple workers"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = MetricsTracker(db_path=db_path)

            now = datetime.now()

            # Multiple workers reporting same metric
            workers = ["worker-1", "worker-2", "worker-3"]
            for worker_id in workers:
                metric = WorkerMetric(
                    timestamp=now,
                    worker_id=worker_id,
                    metric_type=MetricType.CPU_PERCENT,
                    value=50.0,
                    unit=MetricUnit.PERCENT,
                )
                tracker.add_metric(metric)

            tracker.flush()

            # All workers should be present
            all_workers = tracker.get_all_workers()
            assert len(all_workers) == 3

            # Summary should include all
            summary = tracker.get_metrics_summary(metric_type=MetricType.CPU_PERCENT)
            assert summary.count == 3

    @pytest.mark.asyncio
    async def test_time_period_filtering(self):
        """Test time period filtering in queries"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = MetricsTracker(db_path=db_path)

            now = datetime.now()

            # Add metric at different times
            old_metric = WorkerMetric(
                timestamp=now - timedelta(hours=25),
                worker_id="worker-1",
                metric_type=MetricType.CPU_PERCENT,
                value=30.0,
                unit=MetricUnit.PERCENT,
            )
            tracker.add_metric(old_metric)

            recent_metric = WorkerMetric(
                timestamp=now - timedelta(hours=1),
                worker_id="worker-1",
                metric_type=MetricType.CPU_PERCENT,
                value=50.0,
                unit=MetricUnit.PERCENT,
            )
            tracker.add_metric(recent_metric)

            tracker.flush()

            # Query last 24h - should only get recent
            summary = tracker.get_metrics_summary(
                start=now - timedelta(hours=24),
                end=now,
            )
            assert summary.count == 1
            assert summary.avg_value == 50.0

            # Query last 48h - should get both
            summary_48h = tracker.get_metrics_summary(
                start=now - timedelta(hours=48),
                end=now,
            )
            assert summary_48h.count == 2
            assert 30.0 <= summary_48h.avg_value <= 50.0
