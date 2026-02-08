"""
Tests for tool execution engine.

Tests the new forge.tool_execution module which provides:
- Schema validation with type coercion
- Confirmation threshold checking
- Rate limiting (token bucket algorithm)
- Security checks (path traversal, injection prevention)
- Execution history tracking
"""

import pytest
import time
from datetime import datetime
from forge.tool_execution import (
    ToolExecutionEngine,
    ToolCallValidator,
    TokenBucket,
    ToolExecutionResult,
    ConfirmationRequiredError,
    ValidationError,
    SecurityError,
    RateLimitError,
    create_success_result,
    create_error_result,
    create_confirmation_result,
)
from forge.tool_definitions import (
    ToolDefinition,
    ToolParameter,
    ToolCategory,
    TOOL_INDEX,
)


# =============================================================================
# TokenBucket Tests
# =============================================================================


class TestTokenBucket:
    """Tests for TokenBucket rate limiter."""

    def test_token_bucket_consume(self) -> None:
        """Test basic token consumption."""
        bucket = TokenBucket(capacity=10, refill_rate=10.0, tokens=10.0)  # Start with full capacity

        # Should be able to consume up to capacity
        for _ in range(10):
            assert bucket.consume(1) is True

        # Should fail when empty
        assert bucket.consume(1) is False

    def test_token_bucket_refill(self) -> None:
        """Test token refill over time."""
        bucket = TokenBucket(capacity=10, refill_rate=10.0, tokens=10.0)  # Start with full capacity

        # Consume all tokens
        for _ in range(10):
            bucket.consume(1)

        assert bucket.consume(1) is False

        # Wait for refill
        time.sleep(0.15)  # Should refill ~1.5 tokens

        # Should be able to consume 1 token
        assert bucket.consume(1) is True

    def test_token_bucket_wait_time(self) -> None:
        """Test wait time calculation."""
        bucket = TokenBucket(capacity=10, refill_rate=10.0, tokens=10.0)

        # Consume all tokens
        for _ in range(10):
            bucket.consume(1)

        # Wait time for 1 token should be ~0.1 seconds
        wait_time = bucket.wait_time(1)
        assert 0.08 <= wait_time <= 0.15  # Allow for timing variance

    def test_token_bucket_multi_consume(self) -> None:
        """Test consuming multiple tokens at once."""
        bucket = TokenBucket(capacity=10, refill_rate=10.0, tokens=10.0)  # Start with full capacity

        # Consume 5 tokens at once
        assert bucket.consume(5) is True
        assert bucket.consume(5) is True
        assert bucket.consume(1) is False


# =============================================================================
# ToolCallValidator Tests
# =============================================================================


class TestToolCallValidator:
    """Tests for ToolCallValidator."""

    def test_validate_required_params(self) -> None:
        """Test validation of required parameters."""
        validator = ToolCallValidator()
        tool = ToolDefinition(
            name="test_tool",
            description="Test tool",
            category=ToolCategory.VIEW_CONTROL,
            parameters=[
                ToolParameter(
                    name="required_param",
                    type="string",
                    description="Required parameter",
                    required=True,
                ),
                ToolParameter(
                    name="optional_param",
                    type="string",
                    description="Optional parameter",
                    required=False,
                ),
            ]
        )

        # Missing required parameter
        with pytest.raises(ValidationError, match="Missing required parameter"):
            validator.validate("test_tool", {}, tool)

        # With required parameter
        result = validator.validate("test_tool", {"required_param": "value"}, tool)
        assert result == {"required_param": "value"}

    def test_type_coercion_string(self) -> None:
        """Test type coercion for string parameters."""
        validator = ToolCallValidator()
        tool = ToolDefinition(
            name="test_tool",
            description="Test tool",
            category=ToolCategory.VIEW_CONTROL,
            parameters=[
                ToolParameter(
                    name="str_param",
                    type="string",
                    description="String parameter",
                    required=False,
                ),
            ]
        )

        # Already string
        result = validator.validate("test_tool", {"str_param": "hello"}, tool)
        assert result["str_param"] == "hello"

        # Number to string
        result = validator.validate("test_tool", {"str_param": 123}, tool)
        assert result["str_param"] == "123"

    def test_type_coercion_integer(self) -> None:
        """Test type coercion for integer parameters."""
        validator = ToolCallValidator()
        tool = ToolDefinition(
            name="test_tool",
            description="Test tool",
            category=ToolCategory.VIEW_CONTROL,
            parameters=[
                ToolParameter(
                    name="int_param",
                    type="integer",
                    description="Integer parameter",
                    required=False,
                ),
            ]
        )

        # Already int
        result = validator.validate("test_tool", {"int_param": 42}, tool)
        assert result["int_param"] == 42

        # String to int
        result = validator.validate("test_tool", {"int_param": "42"}, tool)
        assert result["int_param"] == 42

        # Float to int
        result = validator.validate("test_tool", {"int_param": 42.7}, tool)
        assert result["int_param"] == 42

    def test_type_coercion_boolean(self) -> None:
        """Test type coercion for boolean parameters."""
        validator = ToolCallValidator()
        tool = ToolDefinition(
            name="test_tool",
            description="Test tool",
            category=ToolCategory.VIEW_CONTROL,
            parameters=[
                ToolParameter(
                    name="bool_param",
                    type="boolean",
                    description="Boolean parameter",
                    required=False,
                ),
            ]
        )

        # Already bool
        result = validator.validate("test_tool", {"bool_param": True}, tool)
        assert result["bool_param"] is True

        # String "true" to bool
        result = validator.validate("test_tool", {"bool_param": "true"}, tool)
        assert result["bool_param"] is True

        # String "false" to bool
        result = validator.validate("test_tool", {"bool_param": "false"}, tool)
        assert result["bool_param"] is False

        # String "1" to bool
        result = validator.validate("test_tool", {"bool_param": "1"}, tool)
        assert result["bool_param"] is True

    def test_enum_validation(self) -> None:
        """Test enum parameter validation."""
        validator = ToolCallValidator()
        tool = ToolDefinition(
            name="test_tool",
            description="Test tool",
            category=ToolCategory.VIEW_CONTROL,
            parameters=[
                ToolParameter(
                    name="enum_param",
                    type="string",
                    description="Enum parameter",
                    required=False,
                    enum=["option1", "option2", "option3"],
                ),
            ]
        )

        # Valid enum value
        result = validator.validate("test_tool", {"enum_param": "option1"}, tool)
        assert result["enum_param"] == "option1"

        # Invalid enum value
        with pytest.raises(ValidationError, match="Invalid value"):
            validator.validate("test_tool", {"enum_param": "invalid"}, tool)

    def test_confirmation_threshold(self) -> None:
        """Test confirmation threshold checking."""
        validator = ToolCallValidator()
        tool = ToolDefinition(
            name="test_tool",
            description="Test tool",
            category=ToolCategory.WORKER_MANAGEMENT,
            parameters=[
                ToolParameter(
                    name="count",
                    type="integer",
                    description="Count parameter",
                    required=False,
                ),
            ],
            confirmation_threshold={"count": 5},
            confirmation_message="Spawn {count} workers?",
        )

        # Below threshold
        result = validator.validate("test_tool", {"count": 3}, tool)
        assert result["count"] == 3

        # Above threshold
        with pytest.raises(ConfirmationRequiredError):
            validator.validate("test_tool", {"count": 10}, tool)


# =============================================================================
# ToolExecutionEngine Tests
# =============================================================================


class TestToolExecutionEngine:
    """Tests for ToolExecutionEngine."""

    def test_engine_initialization(self) -> None:
        """Test engine initializes correctly."""
        engine = ToolExecutionEngine()
        assert engine.validator is not None
        assert engine._global_bucket is not None
        assert len(engine._execution_history) == 0

    def test_register_callback(self) -> None:
        """Test callback registration."""
        engine = ToolExecutionEngine()

        def dummy_callback(**kwargs) -> ToolExecutionResult:
            return create_success_result("test", "Success")

        engine.register_callback("test_tool", dummy_callback)
        assert "test_tool" in engine._callbacks

    def test_execute_unknown_tool(self) -> None:
        """Test executing unknown tool returns error."""
        engine = ToolExecutionEngine()
        result = engine.execute("unknown_tool", {})
        assert result.success is False
        assert "not found" in result.error.lower()

    def test_execute_validation_error(self) -> None:
        """Test execution fails on validation error."""
        engine = ToolExecutionEngine()

        # Use a real tool with required params
        if "switch_view" in TOOL_INDEX:
            result = engine.execute("switch_view", {})  # Missing required 'view' param
            assert result.success is False
            assert "Missing required parameter" in result.error

    def test_execute_success(self) -> None:
        """Test successful execution."""
        engine = ToolExecutionEngine()

        def test_callback(**kwargs) -> ToolExecutionResult:
            return create_success_result("switch_view", "View switched", kwargs)

        engine.register_callback("switch_view", test_callback)

        result = engine.execute("switch_view", {"view": "workers"})
        assert result.success is True
        assert result.message == "View switched"
        assert result.data == {"view": "workers"}

    def test_execute_callback_exception(self) -> None:
        """Test execution handles callback exceptions."""
        engine = ToolExecutionEngine()

        def failing_callback(**kwargs) -> ToolExecutionResult:
            raise ValueError("Test error")

        engine.register_callback("switch_view", failing_callback)

        result = engine.execute("switch_view", {"view": "workers"})
        assert result.success is False
        assert "Test error" in result.error


# =============================================================================
# Helper Function Tests
# =============================================================================


class TestHelperFunctions:
    """Tests for helper functions."""

    def test_create_success_result(self) -> None:
        """Test create_success_result helper."""
        result = create_success_result("test_tool", "Success message", {"key": "value"})
        assert result.success is True
        assert result.tool_name == "test_tool"
        assert result.message == "Success message"
        assert result.data == {"key": "value"}

    def test_create_error_result(self) -> None:
        """Test create_error_result helper."""
        result = create_error_result("test_tool", "Error message", "Detailed error")
        assert result.success is False
        assert result.tool_name == "test_tool"
        assert result.message == "Error message"
        assert result.error == "Detailed error"

    def test_create_confirmation_result(self) -> None:
        """Test create_confirmation_result helper."""
        args = {"count": 10, "model": "sonnet"}
        result = create_confirmation_result("spawn_worker", "Confirm spawning 10 workers?", args)
        assert result.success is False
        assert result.requires_confirmation is True
        assert result.data == args


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
