"""
Tests for the cost tracker module.

Tests cover:
- Model pricing lookup and cost calculation
- API call event parsing from various log formats
- SQLite batch insert functionality
- Cost summary queries (last 24h, by model, by worker)
"""

import asyncio
import tempfile
from datetime import datetime, timedelta
from pathlib import Path

import pytest

from forge.cost_tracker import (
    APICallEvent,
    CostTracker,
    CostSummary,
    ModelCostBreakdown,
    ModelPricing,
    ModelProvider,
    WorkerCostBreakdown,
    get_model_pricing,
)


# =============================================================================
# Model Pricing Tests
# =============================================================================


class TestModelPricing:
    """Tests for ModelPricing and pricing lookup"""

    def test_model_pricing_cost_calculation(self):
        """Test cost calculation for various token amounts"""
        pricing = ModelPricing(
            model_id="claude-sonnet-4-5",
            provider=ModelProvider.ANTHROPIC,
            input_cost_per_mtok=3.0,
            output_cost_per_mtok=15.0,
            context_window=200000,
        )

        # Test with various input/output combinations
        assert pricing.calculate_cost(1000, 500) == pytest.approx(0.0105, rel=1e-6)
        assert pricing.calculate_cost(0, 1000) == pytest.approx(0.015, rel=1e-6)
        assert pricing.calculate_cost(1000, 0) == pytest.approx(0.003, rel=1e-6)
        assert pricing.calculate_cost(1_000_000, 1_000_000) == pytest.approx(18.0, rel=1e-6)

    def test_blended_cost_calculation(self):
        """Test blended cost per million tokens calculation"""
        pricing = ModelPricing(
            model_id="gpt-4o",
            provider=ModelProvider.OPENAI,
            input_cost_per_mtok=2.50,
            output_cost_per_mtok=10.00,
            context_window=128000,
        )

        # Blended = 25% input + 75% output
        expected = (2.50 * 0.25) + (10.00 * 0.75)
        assert pricing.blended_cost_per_mtok == pytest.approx(expected, rel=1e-6)

    def test_get_model_pricing_exact_match(self):
        """Test exact model name lookup"""
        pricing = get_model_pricing("claude-sonnet-4-5")
        assert pricing is not None
        assert pricing.model_id == "claude-sonnet-4-5"
        assert pricing.provider == ModelProvider.ANTHROPIC
        assert pricing.input_cost_per_mtok == 3.0
        assert pricing.output_cost_per_mtok == 15.0

    def test_get_model_pricing_case_insensitive(self):
        """Test case-insensitive model name lookup"""
        pricing = get_model_pricing("CLAUDE-SONNET-4-5")
        assert pricing is not None
        assert pricing.model_id == "claude-sonnet-4-5"

    def test_get_model_pricing_partial_match(self):
        """Test partial model name matching"""
        # Test short name mapping
        pricing = get_model_pricing("sonnet")
        assert pricing is not None
        assert "sonnet" in pricing.model_id.lower()

        pricing = get_model_pricing("opus")
        assert pricing is not None
        assert "opus" in pricing.model_id.lower()

        pricing = get_model_pricing("haiku")
        assert pricing is not None
        assert "haiku" in pricing.model_id.lower()

    def test_get_model_pricing_unknown_model(self):
        """Test lookup of unknown model returns None"""
        pricing = get_model_pricing("unknown-model-x")
        assert pricing is None

    def test_all_models_have_pricing(self):
        """Test that all models in MODEL_PRICING have valid data"""
        from forge.cost_tracker import MODEL_PRICING

        for model_id, pricing in MODEL_PRICING.items():
            assert pricing.model_id == model_id
            assert pricing.input_cost_per_mtok >= 0
            assert pricing.output_cost_per_mtok >= 0
            assert pricing.context_window > 0
            assert isinstance(pricing.provider, ModelProvider)


# =============================================================================
# API Call Event Parsing Tests
# =============================================================================


class TestAPICallEventParsing:
    """Tests for parsing API call completed events"""

    def test_parse_valid_event(self):
        """Test parsing a valid api_call_completed event"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = CostTracker(db_path=db_path)

            log_entry = {
                "event": "api_call_completed",
                "timestamp": "2026-02-08T12:34:56Z",
                "worker_id": "worker-abc123",
                "model": "claude-sonnet-4-5",
                "input_tokens": 1000,
                "output_tokens": 500,
            }

            event = tracker.parse_api_call_event(log_entry)

            assert event is not None
            assert event.worker_id == "worker-abc123"
            assert event.model == "claude-sonnet-4-5"
            assert event.input_tokens == 1000
            assert event.output_tokens == 500
            assert event.total_tokens == 1500
            assert event.cost == pytest.approx(0.0105, rel=1e-6)

    def test_parse_event_with_alternative_token_names(self):
        """Test parsing with prompt_tokens/completion_tokens"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = CostTracker(db_path=db_path)

            log_entry = {
                "event": "api_call_completed",
                "timestamp": "2026-02-08T12:34:56Z",
                "worker_id": "worker-xyz",
                "model": "gpt-4o",
                "prompt_tokens": 2000,
                "completion_tokens": 1000,
            }

            event = tracker.parse_api_call_event(log_entry)

            assert event is not None
            assert event.input_tokens == 2000
            assert event.output_tokens == 1000
            assert event.total_tokens == 3000

    def test_parse_event_missing_fields(self):
        """Test parsing event with missing required fields"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = CostTracker(db_path=db_path)

            # Missing worker_id
            log_entry = {
                "event": "api_call_completed",
                "timestamp": "2026-02-08T12:34:56Z",
                "model": "claude-sonnet-4-5",
                "input_tokens": 1000,
            }

            event = tracker.parse_api_call_event(log_entry)
            assert event is None

    def test_parse_event_wrong_event_type(self):
        """Test parsing non-api_call_completed event"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = CostTracker(db_path=db_path)

            log_entry = {
                "event": "task_started",
                "timestamp": "2026-02-08T12:34:56Z",
                "worker_id": "worker-abc",
            }

            event = tracker.parse_api_call_event(log_entry)
            assert event is None

    def test_parse_event_unknown_model(self):
        """Test parsing event with unknown model"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = CostTracker(db_path=db_path)

            log_entry = {
                "event": "api_call_completed",
                "timestamp": "2026-02-08T12:34:56Z",
                "worker_id": "worker-abc",
                "model": "unknown-model-x",
                "input_tokens": 1000,
                "output_tokens": 500,
            }

            event = tracker.parse_api_call_event(log_entry)

            assert event is not None
            assert event.cost == 0.0  # Unknown model has zero cost

    def test_parse_event_zero_tokens(self):
        """Test parsing event with zero tokens"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = CostTracker(db_path=db_path)

            log_entry = {
                "event": "api_call_completed",
                "timestamp": "2026-02-08T12:34:56Z",
                "worker_id": "worker-abc",
                "model": "claude-sonnet-4-5",
                "input_tokens": 0,
                "output_tokens": 0,
            }

            event = tracker.parse_api_call_event(log_entry)

            assert event is not None
            assert event.total_tokens == 0
            assert event.cost == 0.0


# =============================================================================
# Cost Tracker Tests
# =============================================================================


class TestCostTracker:
    """Tests for CostTracker functionality"""

    @pytest.fixture
    def tracker(self):
        """Create a temporary tracker for each test"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test_costs.db"
            tracker = CostTracker(db_path=db_path)
            yield tracker
            # Cleanup is handled by tempfile

    @pytest.mark.asyncio
    async def test_add_and_flush_events(self, tracker):
        """Test adding events and flushing to database"""
        event = APICallEvent(
            timestamp=datetime.now(),
            worker_id="worker-1",
            model="claude-sonnet-4-5",
            input_tokens=1000,
            output_tokens=500,
            total_tokens=1500,
            cost=0.0105,
        )

        tracker.add_event(event)
        tracker.flush()

        # Verify event was stored
        summary = tracker.get_costs_period(
            datetime.now() - timedelta(hours=1),
            datetime.now() + timedelta(hours=1),
        )

        assert summary.total_requests == 1
        assert summary.total_cost == pytest.approx(0.0105, rel=1e-6)
        assert summary.total_tokens == 1500

    @pytest.mark.asyncio
    async def test_add_event_from_log(self, tracker):
        """Test adding event from log entry"""
        # Use current time for the log entry to ensure it falls within query window
        now = datetime.now()
        log_entry = {
            "event": "api_call_completed",
            "timestamp": now.isoformat(),
            "worker_id": "worker-1",
            "model": "gpt-4o",
            "input_tokens": 2000,
            "output_tokens": 1000,
        }

        added = tracker.add_event_from_log(log_entry)
        assert added is True

        tracker.flush()

        # Verify - use a wider time range to account for timing issues
        summary = tracker.get_costs_period(
            now - timedelta(hours=1),
            now + timedelta(hours=1),
        )

        assert summary.total_requests == 1

    @pytest.mark.asyncio
    async def test_get_costs_last_24h(self, tracker):
        """Test getting costs for last 24 hours"""
        now = datetime.now()

        # Add events at different times
        for i in range(5):
            event = APICallEvent(
                timestamp=now - timedelta(hours=i),
                worker_id=f"worker-{i}",
                model="claude-sonnet-4-5",
                input_tokens=1000,
                output_tokens=500,
                total_tokens=1500,
                cost=0.0105,
            )
            tracker.add_event(event)

        tracker.flush()

        summary = tracker.get_costs_last_24h()

        assert summary.total_requests == 5
        assert summary.total_cost == pytest.approx(0.0525, rel=1e-6)
        assert summary.period_start <= now
        assert summary.period_end >= now

    @pytest.mark.asyncio
    async def test_get_costs_by_model(self, tracker):
        """Test getting costs broken down by model"""
        now = datetime.now()

        # Add events for different models
        models = ["claude-sonnet-4-5", "gpt-4o", "deepseek-v3"]
        for model in models:
            event = APICallEvent(
                timestamp=now,
                worker_id="worker-1",
                model=model,
                input_tokens=1000,
                output_tokens=500,
                total_tokens=1500,
                cost=0.01,  # Simplified
            )
            tracker.add_event(event)

        tracker.flush()

        summary = tracker.get_costs_last_24h()

        assert len(summary.by_model) == 3
        assert "claude-sonnet-4-5" in summary.by_model
        assert "gpt-4o" in summary.by_model
        assert "deepseek-v3" in summary.by_model

        # Check each model has correct stats
        for model, data in summary.by_model.items():
            assert data["requests"] == 1
            assert data["tokens"] == 1500

    @pytest.mark.asyncio
    async def test_get_costs_by_worker(self, tracker):
        """Test getting costs broken down by worker"""
        now = datetime.now()

        # Add events for different workers
        workers = ["worker-1", "worker-2", "worker-3"]
        for worker_id in workers:
            event = APICallEvent(
                timestamp=now,
                worker_id=worker_id,
                model="claude-sonnet-4-5",
                input_tokens=1000,
                output_tokens=500,
                total_tokens=1500,
                cost=0.01,
            )
            tracker.add_event(event)

        # Add a second event for worker-1 with different model
        event2 = APICallEvent(
            timestamp=now,
            worker_id="worker-1",
            model="gpt-4o",
            input_tokens=2000,
            output_tokens=1000,
            total_tokens=3000,
            cost=0.02,
        )
        tracker.add_event(event2)

        tracker.flush()

        summary = tracker.get_costs_last_24h()

        assert len(summary.by_worker) == 3

        # Check worker-1 has two models
        worker_1_data = summary.by_worker.get("worker-1", {})
        assert worker_1_data["requests"] == 2
        assert len(worker_1_data.get("models", {})) == 2

    @pytest.mark.asyncio
    async def test_get_costs_by_model_specific(self, tracker):
        """Test getting costs for a specific model"""
        now = datetime.now()

        # Add events for sonnet
        event1 = APICallEvent(
            timestamp=now,
            worker_id="worker-1",
            model="claude-sonnet-4-5",
            input_tokens=1000,
            output_tokens=500,
            total_tokens=1500,
            cost=0.0105,
        )
        tracker.add_event(event1)

        # Add event for different model
        event2 = APICallEvent(
            timestamp=now,
            worker_id="worker-1",
            model="gpt-4o",
            input_tokens=2000,
            output_tokens=1000,
            total_tokens=3000,
            cost=0.015,
        )
        tracker.add_event(event2)

        tracker.flush()

        breakdown = tracker.get_costs_by_model("claude-sonnet-4-5")

        assert breakdown.model == "claude-sonnet-4-5"
        assert breakdown.total_requests == 1
        assert breakdown.total_tokens == 1500
        assert breakdown.total_cost == pytest.approx(0.0105, rel=1e-6)

    @pytest.mark.asyncio
    async def test_get_costs_by_worker_specific(self, tracker):
        """Test getting costs for a specific worker"""
        now = datetime.now()

        # Add events for worker-1
        event1 = APICallEvent(
            timestamp=now,
            worker_id="worker-1",
            model="claude-sonnet-4-5",
            input_tokens=1000,
            output_tokens=500,
            total_tokens=1500,
            cost=0.0105,
        )
        tracker.add_event(event1)

        # Add event for worker-2
        event2 = APICallEvent(
            timestamp=now,
            worker_id="worker-2",
            model="claude-sonnet-4-5",
            input_tokens=2000,
            output_tokens=1000,
            total_tokens=3000,
            cost=0.021,
        )
        tracker.add_event(event2)

        tracker.flush()

        breakdown = tracker.get_costs_by_worker("worker-1")

        assert breakdown.worker_id == "worker-1"
        assert breakdown.total_requests == 1
        assert breakdown.total_tokens == 1500
        assert breakdown.total_cost == pytest.approx(0.0105, rel=1e-6)
        assert "claude-sonnet-4-5" in breakdown.model_counts

    @pytest.mark.asyncio
    async def test_get_all_models(self, tracker):
        """Test getting list of all models"""
        now = datetime.now()

        models = ["claude-sonnet-4-5", "gpt-4o", "deepseek-v3"]
        for model in models:
            event = APICallEvent(
                timestamp=now,
                worker_id="worker-1",
                model=model,
                input_tokens=1000,
                output_tokens=500,
                total_tokens=1500,
                cost=0.01,
            )
            tracker.add_event(event)

        tracker.flush()

        all_models = tracker.get_all_models()
        assert len(all_models) == 3
        assert "claude-sonnet-4-5" in all_models
        assert "gpt-4o" in all_models
        assert "deepseek-v3" in all_models

    @pytest.mark.asyncio
    async def test_get_all_workers(self, tracker):
        """Test getting list of all workers"""
        now = datetime.now()

        workers = ["worker-1", "worker-2", "worker-3"]
        for worker_id in workers:
            event = APICallEvent(
                timestamp=now,
                worker_id=worker_id,
                model="claude-sonnet-4-5",
                input_tokens=1000,
                output_tokens=500,
                total_tokens=1500,
                cost=0.01,
            )
            tracker.add_event(event)

        tracker.flush()

        all_workers = tracker.get_all_workers()
        assert len(all_workers) == 3
        assert "worker-1" in all_workers
        assert "worker-2" in all_workers
        assert "worker-3" in all_workers

    @pytest.mark.asyncio
    async def test_time_period_filtering(self, tracker):
        """Test that time period filtering works correctly"""
        now = datetime.now()

        # Add event outside the 24h window
        old_event = APICallEvent(
            timestamp=now - timedelta(hours=25),
            worker_id="worker-old",
            model="claude-sonnet-4-5",
            input_tokens=1000,
            output_tokens=500,
            total_tokens=1500,
            cost=0.0105,
        )
        tracker.add_event(old_event)

        # Add event within the 24h window
        recent_event = APICallEvent(
            timestamp=now - timedelta(hours=1),
            worker_id="worker-recent",
            model="claude-sonnet-4-5",
            input_tokens=1000,
            output_tokens=500,
            total_tokens=1500,
            cost=0.0105,
        )
        tracker.add_event(recent_event)

        tracker.flush()

        # Get last 24h - should only include recent event
        summary = tracker.get_costs_last_24h()
        assert summary.total_requests == 1

        # Get last 48h - should include both
        summary_48h = tracker.get_costs_period(
            now - timedelta(hours=48),
            now,
        )
        assert summary_48h.total_requests == 2


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
            tracker = CostTracker(db_path=db_path, max_batch_size=5)

            # Add 5 events (should trigger flush)
            for i in range(5):
                event = APICallEvent(
                    timestamp=datetime.now(),
                    worker_id=f"worker-{i}",
                    model="claude-sonnet-4-5",
                    input_tokens=1000,
                    output_tokens=500,
                    total_tokens=1500,
                    cost=0.0105,
                )
                tracker.add_event(event)

            # Check that events were flushed
            summary = tracker.get_costs_last_24h()
            assert summary.total_requests == 5

    @pytest.mark.asyncio
    async def test_periodic_flush(self):
        """Test periodic background flush"""
        with tempfile.TemporaryDirectory() as tmpdir:
            db_path = Path(tmpdir) / "test.db"
            tracker = CostTracker(db_path=db_path, batch_interval=0.1)  # 100ms

            tracker.start()

            # Add an event
            event = APICallEvent(
                timestamp=datetime.now(),
                worker_id="worker-1",
                model="claude-sonnet-4-5",
                input_tokens=1000,
                output_tokens=500,
                total_tokens=1500,
                cost=0.0105,
            )
            tracker.add_event(event)

            # Wait for periodic flush
            await asyncio.sleep(0.2)

            # Check that event was flushed
            summary = tracker.get_costs_last_24h()
            assert summary.total_requests == 1

            await tracker.stop()


# =============================================================================
# Cost Summary Tests
# =============================================================================


class TestCostSummary:
    """Tests for CostSummary data class"""

    def test_cost_summary_creation(self):
        """Test creating a CostSummary"""
        now = datetime.now()
        summary = CostSummary(
            period_start=now - timedelta(hours=24),
            period_end=now,
            total_cost=1.50,
            total_requests=100,
            total_tokens=150000,
            by_model={
                "claude-sonnet-4-5": {
                    "requests": 50,
                    "cost": 1.00,
                    "tokens": 100000,
                }
            },
            by_worker={
                "worker-1": {
                    "requests": 100,
                    "cost": 1.50,
                    "tokens": 150000,
                }
            },
        )

        assert summary.total_cost == 1.50
        assert summary.total_requests == 100
        assert summary.total_tokens == 150000
        assert len(summary.by_model) == 1
        assert len(summary.by_worker) == 1
