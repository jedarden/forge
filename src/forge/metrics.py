"""
FORGE Metrics Storage Module

Implements SQLite-based storage for worker metrics and cost events.
Features:
- Worker metrics (CPU, memory, disk, network) storage
- Batch inserts with 10-second interval
- Configurable retention policy (default 30 days)
- Indexed queries for performance
- Cost summaries and performance metrics aggregation
"""

from __future__ import annotations

import asyncio
import sqlite3
import threading
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from enum import Enum
from pathlib import Path
from typing import Any

# =============================================================================
# Metrics Data Models
# =============================================================================


class MetricType(Enum):
    """Types of worker metrics"""
    # Resource metrics
    CPU_PERCENT = "cpu_percent"
    MEMORY_PERCENT = "memory_percent"
    MEMORY_BYTES = "memory_bytes"
    DISK_USAGE_BYTES = "disk_usage_bytes"
    DISK_FREE_BYTES = "disk_free_bytes"
    DISK_PERCENT = "disk_percent"

    # Network metrics
    NETWORK_BYTES_SENT = "network_bytes_sent"
    NETWORK_BYTES_RECV = "network_bytes_recv"
    NETWORK_PACKETS_SENT = "network_packets_sent"
    NETWORK_PACKETS_RECV = "network_packets_recv"

    # Task metrics
    TASKS_COMPLETED = "tasks_completed"
    TASKS_FAILED = "tasks_failed"
    TASKS_IN_PROGRESS = "tasks_in_progress"

    # Performance metrics
    AVG_RESPONSE_TIME_MS = "avg_response_time_ms"
    API_CALLS_COUNT = "api_calls_count"
    TOKENS_PROCESSED = "tokens_processed"

    # Health metrics
    HEALTH_CHECK_STATUS = "health_check_status"  # 0=unhealthy, 1=healthy
    UPTIME_SECONDS = "uptime_seconds"


class MetricUnit(Enum):
    """Units for metric values"""
    PERCENT = "percent"
    BYTES = "bytes"
    COUNT = "count"
    MILLISECONDS = "ms"
    SECONDS = "s"
    STATUS = "status"
    TOKENS = "tokens"


@dataclass
class WorkerMetric:
    """
    Represents a single worker metric measurement.

    Attributes:
        timestamp: When the metric was recorded
        worker_id: Worker session ID
        metric_type: Type of metric
        value: Numeric value of the metric
        unit: Unit of measurement
        tags: Optional key-value tags for additional context
    """
    timestamp: datetime
    worker_id: str
    metric_type: MetricType
    value: float
    unit: MetricUnit
    tags: dict[str, str] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization"""
        return {
            "timestamp": self.timestamp.isoformat(),
            "worker_id": self.worker_id,
            "metric_type": self.metric_type.value,
            "value": self.value,
            "unit": self.unit.value,
            "tags": self.tags,
        }


@dataclass
class CostEvent:
    """
    Represents a cost event (extended from APICallEvent).

    Attributes:
        timestamp: When the cost event occurred
        worker_id: Worker session ID
        model: Model name/ID
        input_tokens: Number of input tokens
        output_tokens: Number of output tokens
        total_tokens: Total tokens used
        cost: Calculated cost in USD
        event_type: Type of cost event (api_call, tool_use, etc.)
    """
    timestamp: datetime
    worker_id: str
    model: str
    input_tokens: int
    output_tokens: int
    total_tokens: int
    cost: float
    event_type: str = "api_call"

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization"""
        return {
            "timestamp": self.timestamp.isoformat(),
            "worker_id": self.worker_id,
            "model": self.model,
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
            "total_tokens": self.total_tokens,
            "cost": self.cost,
            "event_type": self.event_type,
        }


@dataclass
class MetricsSummary:
    """
    Summary of metrics for a time period.

    Attributes:
        period_start: Start of period
        period_end: End of period
        worker_id: Worker ID (if scoped to specific worker)
        metric_type: Metric type (if scoped to specific metric)
        avg_value: Average value
        min_value: Minimum value
        max_value: Maximum value
        count: Number of samples
        unit: Unit of measurement
    """
    period_start: datetime
    period_end: datetime
    worker_id: str | None = None
    metric_type: MetricType | None = None
    avg_value: float = 0.0
    min_value: float = 0.0
    max_value: float = 0.0
    count: int = 0
    unit: MetricUnit = MetricUnit.COUNT


@dataclass
class CostSummaryPeriod:
    """
    Cost summary for a time period.

    Attributes:
        period_start: Start of period
        period_end: End of period
        total_cost: Total cost in USD
        total_requests: Total number of requests
        total_tokens: Total tokens used
        by_model: Cost breakdown by model
        by_worker: Cost breakdown by worker
    """
    period_start: datetime
    period_end: datetime
    total_cost: float = 0.0
    total_requests: int = 0
    total_tokens: int = 0
    by_model: dict[str, dict[str, Any]] = field(default_factory=dict)
    by_worker: dict[str, dict[str, Any]] = field(default_factory=dict)


# =============================================================================
# Metrics Tracker (SQLite Storage + Batch Insert)
# =============================================================================


class MetricsTracker:
    """
    Tracks worker metrics with SQLite storage and batch insertion.

    Features:
    - Store worker metrics with configurable retention
    - Batch insert every 10 seconds
    - Query metrics by worker, type, time period
    - Automatic cleanup of old data
    """

    def __init__(
        self,
        db_path: str | Path = "forge_metrics.db",
        batch_interval: float = 10.0,
        max_batch_size: int = 1000,
        retention_days: int = 30,
    ) -> None:
        """
        Initialize the metrics tracker.

        Args:
            db_path: Path to SQLite database
            batch_interval: Seconds between batch inserts (default: 10s)
            max_batch_size: Maximum pending events before immediate flush
            retention_days: Days to keep metrics before cleanup (default: 30)
        """
        self._db_path = Path(db_path)
        self._batch_interval = batch_interval
        self._max_batch_size = max_batch_size
        self._retention_days = retention_days

        # Pending metrics for batch insert
        self._pending_metrics: list[WorkerMetric] = []
        self._pending_costs: list[CostEvent] = []
        self._pending_lock = threading.Lock()

        # Background task management
        self._flush_task: asyncio.Task[None] | None = None
        self._running = False
        self._stop_event = asyncio.Event()

        # Initialize database
        self._init_db()

    def _init_db(self) -> None:
        """Initialize SQLite database with schema and indexes"""
        conn = sqlite3.connect(self._db_path)
        cursor = conn.cursor()

        # Create worker_metrics table
        cursor.execute("""
            CREATE TABLE IF NOT EXISTS worker_metrics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                worker_id TEXT NOT NULL,
                metric_type TEXT NOT NULL,
                value REAL NOT NULL,
                unit TEXT NOT NULL,
                tags TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )
        """)

        # Create cost_events table
        cursor.execute("""
            CREATE TABLE IF NOT EXISTS cost_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                worker_id TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                total_tokens INTEGER NOT NULL,
                cost REAL NOT NULL,
                event_type TEXT NOT NULL DEFAULT 'api_call',
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )
        """)

        # Create indexes for efficient querying
        # Worker metrics indexes
        cursor.execute("""
            CREATE INDEX IF NOT EXISTS idx_worker_metrics_timestamp
            ON worker_metrics(timestamp)
        """)

        cursor.execute("""
            CREATE INDEX IF NOT EXISTS idx_worker_metrics_worker
            ON worker_metrics(worker_id)
        """)

        cursor.execute("""
            CREATE INDEX IF NOT EXISTS idx_worker_metrics_type
            ON worker_metrics(metric_type)
        """)

        cursor.execute("""
            CREATE INDEX IF NOT EXISTS idx_worker_metrics_worker_type
            ON worker_metrics(worker_id, metric_type)
        """)

        # Cost events indexes
        cursor.execute("""
            CREATE INDEX IF NOT EXISTS idx_cost_events_timestamp
            ON cost_events(timestamp)
        """)

        cursor.execute("""
            CREATE INDEX IF NOT EXISTS idx_cost_events_worker
            ON cost_events(worker_id)
        """)

        cursor.execute("""
            CREATE INDEX IF NOT EXISTS idx_cost_events_model
            ON cost_events(model)
        """)

        conn.commit()
        conn.close()

    def add_metric(self, metric: WorkerMetric) -> None:
        """
        Add a metric to the pending batch.

        Args:
            metric: Worker metric to add
        """
        with self._pending_lock:
            self._pending_metrics.append(metric)

            # Flush immediately if batch is full
            if len(self._pending_metrics) >= self._max_batch_size:
                self._flush_pending_sync()

    def add_cost_event(self, event: CostEvent) -> None:
        """
        Add a cost event to the pending batch.

        Args:
            event: Cost event to add
        """
        with self._pending_lock:
            self._pending_costs.append(event)

            # Flush immediately if batch is full
            if len(self._pending_costs) >= self._max_batch_size:
                self._flush_pending_sync()

    def _flush_pending_sync(self) -> None:
        """Flush pending events to database (synchronous, called with lock held)"""
        conn = sqlite3.connect(self._db_path)
        cursor = conn.cursor()

        # Flush pending metrics
        if self._pending_metrics:
            for metric in self._pending_metrics:
                import json
                tags_json = json.dumps(metric.tags) if metric.tags else None
                cursor.execute("""
                    INSERT INTO worker_metrics (
                        timestamp, worker_id, metric_type, value, unit, tags
                    ) VALUES (?, ?, ?, ?, ?, ?)
                """, (
                    metric.timestamp.isoformat(),
                    metric.worker_id,
                    metric.metric_type.value,
                    metric.value,
                    metric.unit.value,
                    tags_json,
                ))
            self._pending_metrics.clear()

        # Flush pending cost events
        if self._pending_costs:
            for event in self._pending_costs:
                cursor.execute("""
                    INSERT INTO cost_events (
                        timestamp, worker_id, model, input_tokens,
                        output_tokens, total_tokens, cost, event_type
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                """, (
                    event.timestamp.isoformat(),
                    event.worker_id,
                    event.model,
                    event.input_tokens,
                    event.output_tokens,
                    event.total_tokens,
                    event.cost,
                    event.event_type,
                ))
            self._pending_costs.clear()

        conn.commit()
        conn.close()

    async def _flush_loop(self) -> None:
        """Background task to periodically flush pending events"""
        while not self._stop_event.is_set():
            try:
                # Wait for batch interval or stop event
                await asyncio.wait_for(
                    self._stop_event.wait(),
                    timeout=self._batch_interval,
                )
            except asyncio.TimeoutError:
                # Normal timeout, flush pending events
                pass

            # Flush pending events
            with self._pending_lock:
                self._flush_pending_sync()

    def start(self) -> None:
        """Start the background flush task"""
        if self._running:
            return

        self._running = True
        self._stop_event.clear()
        self._flush_task = asyncio.create_task(self._flush_loop())

    async def stop(self) -> None:
        """Stop the background flush task and flush remaining events"""
        if not self._running:
            return

        self._running = False
        self._stop_event.set()

        if self._flush_task is not None:
            await self._flush_task

        # Final flush
        with self._pending_lock:
            self._flush_pending_sync()

    def flush(self) -> None:
        """Manually flush pending events (synchronous)"""
        with self._pending_lock:
            self._flush_pending_sync()

    # =============================================================================
    # Query Methods - Worker Metrics
    # =============================================================================

    def get_metrics_summary(
        self,
        worker_id: str | None = None,
        metric_type: MetricType | None = None,
        start: datetime | None = None,
        end: datetime | None = None,
    ) -> MetricsSummary:
        """
        Get metrics summary for a time period.

        Args:
            worker_id: Optional worker ID to filter by
            metric_type: Optional metric type to filter by
            start: Period start (defaults to 24h ago)
            end: Period end (defaults to now)

        Returns:
            MetricsSummary with aggregated data
        """
        if end is None:
            end = datetime.now()
        if start is None:
            start = end - timedelta(hours=24)

        conn = sqlite3.connect(self._db_path)
        cursor = conn.cursor()

        start_str = start.isoformat()
        end_str = end.isoformat()

        # Build query with filters
        where_clause = "timestamp >= ? AND timestamp <= ?"
        params: list[Any] = [start_str, end_str]

        if worker_id:
            where_clause += " AND worker_id = ?"
            params.append(worker_id)

        if metric_type:
            where_clause += " AND metric_type = ?"
            params.append(metric_type.value)

        cursor.execute(f"""
            SELECT
                COUNT(*) as count,
                AVG(value) as avg_value,
                MIN(value) as min_value,
                MAX(value) as max_value,
                unit
            FROM worker_metrics
            WHERE {where_clause}
        """, params)

        row = cursor.fetchone()
        conn.close()

        count = row[0] or 0
        avg_value = row[1] or 0.0
        min_value = row[2] or 0.0
        max_value = row[3] or 0.0
        unit_str = row[4] or "count"

        # Map unit string back to enum
        try:
            unit = MetricUnit(unit_str)
        except ValueError:
            unit = MetricUnit.COUNT

        return MetricsSummary(
            period_start=start,
            period_end=end,
            worker_id=worker_id,
            metric_type=metric_type,
            avg_value=avg_value,
            min_value=min_value,
            max_value=max_value,
            count=count,
            unit=unit,
        )

    def get_worker_metrics(
        self,
        worker_id: str,
        metric_type: MetricType | None = None,
        hours: int = 24,
        limit: int = 1000,
    ) -> list[WorkerMetric]:
        """
        Get raw metrics for a worker.

        Args:
            worker_id: Worker session ID
            metric_type: Optional metric type filter
            hours: Hours to look back
            limit: Maximum records to return

        Returns:
            List of WorkerMetric objects
        """
        conn = sqlite3.connect(self._db_path)
        cursor = conn.cursor()

        cutoff = datetime.now() - timedelta(hours=hours)
        cutoff_str = cutoff.isoformat()

        where_clause = "worker_id = ? AND timestamp >= ?"
        params: list[Any] = [worker_id, cutoff_str]

        if metric_type:
            where_clause += " AND metric_type = ?"
            params.append(metric_type.value)

        cursor.execute(f"""
            SELECT timestamp, worker_id, metric_type, value, unit, tags
            FROM worker_metrics
            WHERE {where_clause}
            ORDER BY timestamp DESC
            LIMIT ?
        """, params + [limit])

        import json
        metrics = []
        for row in cursor.fetchall():
            tags = json.loads(row[5]) if row[5] else {}
            metrics.append(WorkerMetric(
                timestamp=datetime.fromisoformat(row[0]),
                worker_id=row[1],
                metric_type=MetricType(row[2]),
                value=row[3],
                unit=MetricUnit(row[4]),
                tags=tags,
            ))

        conn.close()
        return metrics

    def get_all_workers(self) -> list[str]:
        """Get list of all workers with recorded metrics"""
        conn = sqlite3.connect(self._db_path)
        cursor = conn.cursor()

        cursor.execute("""
            SELECT DISTINCT worker_id FROM worker_metrics ORDER BY worker_id
        """)

        workers = [row[0] for row in cursor.fetchall()]
        conn.close()

        return workers

    # =============================================================================
    # Query Methods - Cost Events
    # =============================================================================

    def get_cost_summary(
        self,
        start: datetime | None = None,
        end: datetime | None = None,
    ) -> CostSummaryPeriod:
        """
        Get cost summary for a time period.

        Args:
            start: Period start (defaults to 24h ago)
            end: Period end (defaults to now)

        Returns:
            CostSummaryPeriod with aggregated data
        """
        if end is None:
            end = datetime.now()
        if start is None:
            start = end - timedelta(hours=24)

        conn = sqlite3.connect(self._db_path)
        cursor = conn.cursor()

        start_str = start.isoformat()
        end_str = end.isoformat()

        # Get totals
        cursor.execute("""
            SELECT
                COUNT(*) as total_requests,
                SUM(cost) as total_cost,
                SUM(total_tokens) as total_tokens
            FROM cost_events
            WHERE timestamp >= ? AND timestamp <= ?
        """, (start_str, end_str))

        row = cursor.fetchone()
        total_requests = row[0] or 0
        total_cost = row[1] or 0.0
        total_tokens = row[2] or 0

        # Get breakdown by model
        cursor.execute("""
            SELECT
                model,
                COUNT(*) as requests,
                SUM(cost) as cost,
                SUM(total_tokens) as tokens
            FROM cost_events
            WHERE timestamp >= ? AND timestamp <= ?
            GROUP BY model
            ORDER BY cost DESC
        """, (start_str, end_str))

        by_model: dict[str, dict[str, Any]] = {}
        for model, requests, cost, tokens in cursor.fetchall():
            by_model[model] = {
                "requests": requests,
                "cost": cost,
                "tokens": tokens,
                "avg_cost_per_request": cost / requests if requests > 0 else 0,
                "avg_cost_per_mtok": (cost / tokens * 1_000_000) if tokens > 0 else 0,
            }

        # Get breakdown by worker
        cursor.execute("""
            SELECT
                worker_id,
                COUNT(*) as requests,
                SUM(cost) as cost,
                SUM(total_tokens) as tokens,
                model
            FROM cost_events
            WHERE timestamp >= ? AND timestamp <= ?
            GROUP BY worker_id, model
            ORDER BY worker_id, cost DESC
        """, (start_str, end_str))

        by_worker: dict[str, dict[str, Any]] = {}
        for worker_id, requests, cost, tokens, model in cursor.fetchall():
            if worker_id not in by_worker:
                by_worker[worker_id] = {
                    "requests": 0,
                    "cost": 0.0,
                    "tokens": 0,
                    "models": {},
                }

            by_worker[worker_id]["requests"] += requests
            by_worker[worker_id]["cost"] += cost
            by_worker[worker_id]["tokens"] += tokens
            by_worker[worker_id]["models"][model] = by_worker[worker_id]["models"].get(model, 0) + requests

        conn.close()

        return CostSummaryPeriod(
            period_start=start,
            period_end=end,
            total_cost=total_cost,
            total_requests=total_requests,
            total_tokens=total_tokens,
            by_model=by_model,
            by_worker=by_worker,
        )

    def get_costs_last_24h(self) -> CostSummaryPeriod:
        """Get cost summary for the last 24 hours"""
        return self.get_cost_summary()

    def get_costs_today(self) -> CostSummaryPeriod:
        """Get cost summary for today (since midnight)"""
        now = datetime.now()
        period_start = now.replace(hour=0, minute=0, second=0, microsecond=0)
        return self.get_cost_summary(period_start, now)

    def get_costs_by_worker(self, worker_id: str, hours: int = 24) -> dict[str, Any]:
        """
        Get cost breakdown for a specific worker.

        Args:
            worker_id: Worker session ID
            hours: Number of hours to look back (default: 24)

        Returns:
            Dict with cost breakdown
        """
        conn = sqlite3.connect(self._db_path)
        cursor = conn.cursor()

        cutoff = datetime.now() - timedelta(hours=hours)
        cutoff_str = cutoff.isoformat()

        cursor.execute("""
            SELECT
                COUNT(*) as requests,
                SUM(cost) as cost,
                SUM(total_tokens) as tokens,
                model
            FROM cost_events
            WHERE worker_id = ? AND timestamp >= ?
            GROUP BY model
        """, (worker_id, cutoff_str))

        total_requests = 0
        total_cost = 0.0
        total_tokens = 0
        model_counts: dict[str, int] = {}

        for requests, cost, tokens, model in cursor.fetchall():
            total_requests += requests
            total_cost += cost
            total_tokens += tokens
            model_counts[model] = model_counts.get(model, 0) + requests

        conn.close()

        return {
            "worker_id": worker_id,
            "total_requests": total_requests,
            "total_cost": total_cost,
            "total_tokens": total_tokens,
            "model_counts": model_counts,
            "avg_cost_per_request": total_cost / total_requests if total_requests > 0 else 0,
        }

    # =============================================================================
    # Retention Policy
    # =============================================================================

    def apply_retention_policy(self) -> int:
        """
        Delete metrics older than retention period.

        Returns:
            Number of rows deleted
        """
        cutoff = datetime.now() - timedelta(days=self._retention_days)
        cutoff_str = cutoff.isoformat()

        conn = sqlite3.connect(self._db_path)
        cursor = conn.cursor()

        # Delete old metrics
        cursor.execute("""
            DELETE FROM worker_metrics
            WHERE timestamp < ?
        """, (cutoff_str,))
        metrics_deleted = cursor.rowcount

        # Delete old cost events
        cursor.execute("""
            DELETE FROM cost_events
            WHERE timestamp < ?
        """, (cutoff_str,))
        costs_deleted = cursor.rowcount

        conn.commit()
        conn.close()

        return metrics_deleted + costs_deleted

    def get_retention_info(self) -> dict[str, Any]:
        """
        Get information about data retention.

        Returns:
            Dict with retention stats
        """
        conn = sqlite3.connect(self._db_path)
        cursor = conn.cursor()

        # Get counts
        cursor.execute("SELECT COUNT(*) FROM worker_metrics")
        metrics_count = cursor.fetchone()[0]

        cursor.execute("SELECT COUNT(*) FROM cost_events")
        costs_count = cursor.fetchone()[0]

        # Get oldest records
        cursor.execute("SELECT MIN(timestamp) FROM worker_metrics")
        oldest_metric = cursor.fetchone()[0]

        cursor.execute("SELECT MIN(timestamp) FROM cost_events")
        oldest_cost = cursor.fetchone()[0]

        # Get database size
        cursor.execute("SELECT page_count * page_size as size FROM pragma_page_count(), pragma_page_size()")
        db_size_bytes = cursor.fetchone()[0]

        conn.close()

        return {
            "retention_days": self._retention_days,
            "metrics_count": metrics_count,
            "cost_events_count": costs_count,
            "oldest_metric_timestamp": oldest_metric,
            "oldest_cost_timestamp": oldest_cost,
            "database_size_bytes": db_size_bytes,
            "database_size_mb": db_size_bytes / (1024 * 1024) if db_size_bytes else 0,
        }


# =============================================================================
# Singleton Instance
# =============================================================================

_default_tracker: MetricsTracker | None = None


def get_metrics_tracker() -> MetricsTracker:
    """Get the default metrics tracker instance"""
    global _default_tracker
    if _default_tracker is None:
        # Default database in user's home directory
        db_path = Path.home() / ".forge" / "forge_metrics.db"
        db_path.parent.mkdir(parents=True, exist_ok=True)

        _default_tracker = MetricsTracker(db_path=db_path)
        _default_tracker.start()

    return _default_tracker
