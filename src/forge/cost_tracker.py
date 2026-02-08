"""
FORGE Cost Tracker Module

Implements cost tracking and aggregation for LLM API calls.
Parses api_call_completed events from logs, calculates costs using
MODEL_PRICING, and stores results in SQLite for querying.

Features:
- Parse api_call_completed events from worker logs
- Calculate costs from token counts using MODEL_PRICING
- Batch insert to SQLite every 10 seconds
- Cost summary queries (last 24h, by model, by worker)
- Integration with cost panel display
"""

from __future__ import annotations

import asyncio
import sqlite3
import time
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from enum import Enum
from pathlib import Path
from threading import Lock
from typing import Any


# =============================================================================
# Model Pricing Constants
# =============================================================================


class ModelProvider(Enum):
    """LLM model providers"""
    ANTHROPIC = "anthropic"
    OPENAI = "openai"
    DEEPSEEK = "deepseek"
    ZAI = "zai"
    OTHER = "other"


@dataclass(frozen=True)
class ModelPricing:
    """
    Pricing information for an LLM model.

    Attributes:
        model_id: Model identifier (e.g., "claude-sonnet-4-5", "gpt-4o")
        provider: Model provider
        input_cost_per_mtok: Input token cost per million tokens
        output_cost_per_mtok: Output token cost per million tokens
        context_window: Maximum context window in tokens
    """
    model_id: str
    provider: ModelProvider
    input_cost_per_mtok: float
    output_cost_per_mtok: float
    context_window: int = 200000

    @property
    def blended_cost_per_mtok(self) -> float:
        """
        Calculate blended cost per million tokens.
        Assumes typical 1:3 input-to-output ratio (25% input, 75% output).
        """
        return (self.input_cost_per_mtok * 0.25) + (self.output_cost_per_mtok * 0.75)

    def calculate_cost(self, input_tokens: int, output_tokens: int) -> float:
        """
        Calculate cost for a specific API call.

        Args:
            input_tokens: Number of input tokens
            output_tokens: Number of output tokens

        Returns:
            Cost in USD
        """
        input_cost = (input_tokens / 1_000_000) * self.input_cost_per_mtok
        output_cost = (output_tokens / 1_000_000) * self.output_cost_per_mtok
        return input_cost + output_cost


# Model pricing database (from PRICING_RESEARCH_SUMMARY.md)
MODEL_PRICING: dict[str, ModelPricing] = {
    # Anthropic Models
    "claude-opus-4-6": ModelPricing(
        model_id="claude-opus-4-6",
        provider=ModelProvider.ANTHROPIC,
        input_cost_per_mtok=15.00,
        output_cost_per_mtok=75.00,
        context_window=200000,
    ),
    "claude-sonnet-4-5": ModelPricing(
        model_id="claude-sonnet-4-5",
        provider=ModelProvider.ANTHROPIC,
        input_cost_per_mtok=3.00,
        output_cost_per_mtok=15.00,
        context_window=200000,
    ),
    "claude-haiku-4-20250514": ModelPricing(
        model_id="claude-haiku-4-20250514",
        provider=ModelProvider.ANTHROPIC,
        input_cost_per_mtok=0.25,
        output_cost_per_mtok=1.25,
        context_window=200000,
    ),
    "claude-opus-4-5": ModelPricing(
        model_id="claude-opus-4-5",
        provider=ModelProvider.ANTHROPIC,
        input_cost_per_mtok=15.00,
        output_cost_per_mtok=75.00,
        context_window=200000,
    ),
    # OpenAI Models
    "gpt-4o": ModelPricing(
        model_id="gpt-4o",
        provider=ModelProvider.OPENAI,
        input_cost_per_mtok=2.50,
        output_cost_per_mtok=10.00,
        context_window=128000,
    ),
    "gpt-4-turbo": ModelPricing(
        model_id="gpt-4-turbo",
        provider=ModelProvider.OPENAI,
        input_cost_per_mtok=10.00,
        output_cost_per_mtok=30.00,
        context_window=128000,
    ),
    "gpt-3.5-turbo": ModelPricing(
        model_id="gpt-3.5-turbo",
        provider=ModelProvider.OPENAI,
        input_cost_per_mtok=0.50,
        output_cost_per_mtok=1.50,
        context_window=16385,
    ),
    # DeepSeek Models
    "deepseek-v3": ModelPricing(
        model_id="deepseek-v3",
        provider=ModelProvider.DEEPSEEK,
        input_cost_per_mtok=0.14,
        output_cost_per_mtok=0.28,
        context_window=64000,
    ),
    # ZAI Proxy Models (via z.ai)
    "glm-4.7": ModelPricing(
        model_id="glm-4.7",
        provider=ModelProvider.ZAI,
        input_cost_per_mtok=0.50,
        output_cost_per_mtok=0.50,
        context_window=128000,
    ),
    # Legacy/Other model name mappings
    "opus": ModelPricing(
        model_id="opus",
        provider=ModelProvider.ANTHROPIC,
        input_cost_per_mtok=15.00,
        output_cost_per_mtok=75.00,
    ),
    "sonnet": ModelPricing(
        model_id="sonnet",
        provider=ModelProvider.ANTHROPIC,
        input_cost_per_mtok=3.00,
        output_cost_per_mtok=15.00,
    ),
    "haiku": ModelPricing(
        model_id="haiku",
        provider=ModelProvider.ANTHROPIC,
        input_cost_per_mtok=0.25,
        output_cost_per_mtok=1.25,
    ),
}


def get_model_pricing(model_name: str) -> ModelPricing | None:
    """
    Get pricing for a model by name.

    Args:
        model_name: Model identifier (e.g., "claude-sonnet-4-5", "sonnet")

    Returns:
        ModelPricing if found, None otherwise
    """
    # Try exact match first
    if model_name in MODEL_PRICING:
        return MODEL_PRICING[model_name]

    # Try lowercase match
    model_lower = model_name.lower()
    for key, pricing in MODEL_PRICING.items():
        if key.lower() == model_lower:
            return pricing

    # Try partial match for versioned models
    for key, pricing in MODEL_PRICING.items():
        if model_lower in key.lower() or key.lower() in model_lower:
            return pricing

    return None


# =============================================================================
# Cost Entry Data Model
# =============================================================================


@dataclass
class APICallEvent:
    """
    Represents an API call completed event.

    Attributes:
        timestamp: When the API call completed
        worker_id: Worker session ID
        model: Model name/ID
        input_tokens: Number of input tokens
        output_tokens: Number of output tokens
        total_tokens: Total tokens used
        cost: Calculated cost in USD
        raw_event: Raw event data for debugging
    """
    timestamp: datetime
    worker_id: str
    model: str
    input_tokens: int
    output_tokens: int
    total_tokens: int
    cost: float
    raw_event: dict[str, Any] = field(default_factory=dict)

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
        }


# =============================================================================
# Cost Summary Data Models
# =============================================================================


@dataclass
class CostSummary:
    """
    Cost summary for a time period.

    Attributes:
        period_start: Start of period
        period_end: End of period
        total_cost: Total cost in USD
        total_requests: Total number of API requests
        total_tokens: Total tokens used
        by_model: Cost breakdown by model
        by_worker: Cost breakdown by worker
    """
    period_start: datetime
    period_end: datetime
    total_cost: float
    total_requests: int
    total_tokens: int
    by_model: dict[str, dict[str, Any]] = field(default_factory=dict)
    by_worker: dict[str, dict[str, Any]] = field(default_factory=dict)


@dataclass
class ModelCostBreakdown:
    """Cost breakdown for a specific model"""
    model: str
    total_cost: float
    total_requests: int
    total_tokens: int
    avg_cost_per_request: float
    avg_cost_per_mtok: float


@dataclass
class WorkerCostBreakdown:
    """Cost breakdown for a specific worker"""
    worker_id: str
    total_cost: float
    total_requests: int
    total_tokens: int
    model_counts: dict[str, int]  # model -> request count


# =============================================================================
# Cost Tracker (SQLite Storage + Batch Insert)
# =============================================================================


class CostTracker:
    """
    Tracks API call costs with SQLite storage and batch insertion.

    Features:
    - Parse api_call_completed events from logs
    - Calculate costs using MODEL_PRICING
    - Batch insert to SQLite every 10 seconds
    - Query costs by time period, model, worker
    """

    def __init__(
        self,
        db_path: str | Path = "forge_costs.db",
        batch_interval: float = 10.0,
        max_batch_size: int = 1000,
    ) -> None:
        """
        Initialize the cost tracker.

        Args:
            db_path: Path to SQLite database
            batch_interval: Seconds between batch inserts (default: 10s)
            max_batch_size: Maximum pending events before immediate flush
        """
        self._db_path = Path(db_path)
        self._batch_interval = batch_interval
        self._max_batch_size = max_batch_size

        # Pending events for batch insert
        self._pending_events: list[APICallEvent] = []
        self._pending_lock = Lock()

        # Background task management
        self._flush_task: asyncio.Task[None] | None = None
        self._running = False
        self._stop_event = asyncio.Event()

        # Initialize database
        self._init_db()

    def _init_db(self) -> None:
        """Initialize SQLite database with schema"""
        conn = sqlite3.connect(self._db_path)
        cursor = conn.cursor()

        # Create api_calls table
        cursor.execute("""
            CREATE TABLE IF NOT EXISTS api_calls (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                worker_id TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                total_tokens INTEGER NOT NULL,
                cost REAL NOT NULL,
                raw_event TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )
        """)

        # Create indexes for efficient querying
        cursor.execute("""
            CREATE INDEX IF NOT EXISTS idx_api_calls_timestamp
            ON api_calls(timestamp)
        """)

        cursor.execute("""
            CREATE INDEX IF NOT EXISTS idx_api_calls_worker
            ON api_calls(worker_id)
        """)

        cursor.execute("""
            CREATE INDEX IF NOT EXISTS idx_api_calls_model
            ON api_calls(model)
        """)

        # Create summary table for cached aggregations
        cursor.execute("""
            CREATE TABLE IF NOT EXISTS cost_summaries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                period_start TEXT NOT NULL,
                period_end TEXT NOT NULL,
                total_cost REAL NOT NULL,
                total_requests INTEGER NOT NULL,
                total_tokens INTEGER NOT NULL,
                breakdown_json TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )
        """)

        conn.commit()
        conn.close()

    def parse_api_call_event(self, log_entry: dict[str, Any]) -> APICallEvent | None:
        """
        Parse an api_call_completed event from a log entry.

        Expected log format:
        {
            "event": "api_call_completed",
            "timestamp": "2026-02-08T12:34:56Z",
            "worker_id": "worker-abc123",
            "model": "claude-sonnet-4-5",
            "input_tokens": 1000,
            "output_tokens": 500,
            ...
        }

        Args:
            log_entry: Parsed log entry dictionary

        Returns:
            APICallEvent if valid, None otherwise
        """
        # Check event type
        if log_entry.get("event") != "api_call_completed":
            return None

        # Extract required fields
        try:
            timestamp_str = log_entry.get("timestamp")
            worker_id = log_entry.get("worker_id")
            model = log_entry.get("model")
            input_tokens = log_entry.get("input_tokens", log_entry.get("prompt_tokens", 0))
            output_tokens = log_entry.get("output_tokens", log_entry.get("completion_tokens", 0))

            # Validate required fields
            if not all([timestamp_str, worker_id, model]):
                return None

            # Parse timestamp
            try:
                timestamp = datetime.fromisoformat(timestamp_str.replace("Z", "+00:00"))
            except (ValueError, AttributeError):
                # Try alternative formats
                timestamp = datetime.now()

            # Convert tokens to int
            input_tokens = int(input_tokens) if input_tokens else 0
            output_tokens = int(output_tokens) if output_tokens else 0
            total_tokens = input_tokens + output_tokens

            # Get pricing and calculate cost
            pricing = get_model_pricing(model)
            if pricing is None:
                # Unknown model, use default pricing
                cost = 0.0
            else:
                cost = pricing.calculate_cost(input_tokens, output_tokens)

            return APICallEvent(
                timestamp=timestamp,
                worker_id=worker_id,
                model=model,
                input_tokens=input_tokens,
                output_tokens=output_tokens,
                total_tokens=total_tokens,
                cost=cost,
                raw_event=log_entry,
            )

        except (ValueError, TypeError, KeyError):
            return None

    def add_event(self, event: APICallEvent) -> None:
        """
        Add an event to the pending batch.

        Args:
            event: API call event to add
        """
        with self._pending_lock:
            self._pending_events.append(event)

            # Flush immediately if batch is full
            if len(self._pending_events) >= self._max_batch_size:
                self._flush_pending_sync()

    def add_event_from_log(self, log_entry: dict[str, Any]) -> bool:
        """
        Parse and add an event from a log entry.

        Args:
            log_entry: Parsed log entry

        Returns:
            True if event was added, False otherwise
        """
        event = self.parse_api_call_event(log_entry)
        if event is not None:
            self.add_event(event)
            return True
        return False

    def _flush_pending_sync(self) -> None:
        """Flush pending events to database (synchronous, called with lock held)"""
        if not self._pending_events:
            return

        conn = sqlite3.connect(self._db_path)
        cursor = conn.cursor()

        for event in self._pending_events:
            cursor.execute("""
                INSERT INTO api_calls (
                    timestamp, worker_id, model, input_tokens,
                    output_tokens, total_tokens, cost, raw_event
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """, (
                event.timestamp.isoformat(),
                event.worker_id,
                event.model,
                event.input_tokens,
                event.output_tokens,
                event.total_tokens,
                event.cost,
                str(event.raw_event),
            ))

        conn.commit()
        conn.close()

        self._pending_events.clear()

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
    # Query Methods
    # =============================================================================

    def get_costs_last_24h(self) -> CostSummary:
        """Get cost summary for the last 24 hours"""
        now = datetime.now()
        period_start = now - timedelta(hours=24)
        return self.get_costs_period(period_start, now)

    def get_costs_today(self) -> CostSummary:
        """Get cost summary for today (since midnight)"""
        now = datetime.now()
        period_start = now.replace(hour=0, minute=0, second=0, microsecond=0)
        return self.get_costs_period(period_start, now)

    def get_costs_period(
        self, start: datetime, end: datetime
    ) -> CostSummary:
        """
        Get cost summary for a specific time period.

        Args:
            start: Period start
            end: Period end

        Returns:
            CostSummary with aggregated data
        """
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
            FROM api_calls
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
            FROM api_calls
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
            FROM api_calls
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

        return CostSummary(
            period_start=start,
            period_end=end,
            total_cost=total_cost,
            total_requests=total_requests,
            total_tokens=total_tokens,
            by_model=by_model,
            by_worker=by_worker,
        )

    def get_costs_by_model(self, model: str, hours: int = 24) -> ModelCostBreakdown:
        """
        Get cost breakdown for a specific model.

        Args:
            model: Model name/ID
            hours: Number of hours to look back (default: 24)

        Returns:
            ModelCostBreakdown
        """
        conn = sqlite3.connect(self._db_path)
        cursor = conn.cursor()

        cutoff = datetime.now() - timedelta(hours=hours)
        cutoff_str = cutoff.isoformat()

        cursor.execute("""
            SELECT
                COUNT(*) as requests,
                SUM(cost) as cost,
                SUM(total_tokens) as tokens
            FROM api_calls
            WHERE model = ? AND timestamp >= ?
        """, (model, cutoff_str))

        row = cursor.fetchone()
        requests = row[0] or 0
        cost = row[1] or 0.0
        tokens = row[2] or 0

        conn.close()

        return ModelCostBreakdown(
            model=model,
            total_cost=cost,
            total_requests=requests,
            total_tokens=tokens,
            avg_cost_per_request=cost / requests if requests > 0 else 0,
            avg_cost_per_mtok=(cost / tokens * 1_000_000) if tokens > 0 else 0,
        )

    def get_costs_by_worker(self, worker_id: str, hours: int = 24) -> WorkerCostBreakdown:
        """
        Get cost breakdown for a specific worker.

        Args:
            worker_id: Worker session ID
            hours: Number of hours to look back (default: 24)

        Returns:
            WorkerCostBreakdown
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
            FROM api_calls
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

        return WorkerCostBreakdown(
            worker_id=worker_id,
            total_cost=total_cost,
            total_requests=total_requests,
            total_tokens=total_tokens,
            model_counts=model_counts,
        )

    def get_all_models(self) -> list[str]:
        """Get list of all models with recorded API calls"""
        conn = sqlite3.connect(self._db_path)
        cursor = conn.cursor()

        cursor.execute("""
            SELECT DISTINCT model FROM api_calls ORDER BY model
        """)

        models = [row[0] for row in cursor.fetchall()]
        conn.close()

        return models

    def get_all_workers(self) -> list[str]:
        """Get list of all workers with recorded API calls"""
        conn = sqlite3.connect(self._db_path)
        cursor = conn.cursor()

        cursor.execute("""
            SELECT DISTINCT worker_id FROM api_calls ORDER BY worker_id
        """)

        workers = [row[0] for row in cursor.fetchall()]
        conn.close()

        return workers


# =============================================================================
# Singleton Instance
# =============================================================================

_default_tracker: CostTracker | None = None


def get_cost_tracker() -> CostTracker:
    """Get the default cost tracker instance"""
    global _default_tracker
    if _default_tracker is None:
        # Default database in user's home directory
        db_path = Path.home() / ".forge" / "forge_costs.db"
        db_path.parent.mkdir(parents=True, exist_ok=True)

        _default_tracker = CostTracker(db_path=db_path)
        _default_tracker.start()

    return _default_tracker
