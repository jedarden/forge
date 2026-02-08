"""
FORGE Tool Execution Engine

Implements secure tool call validation, execution, and rate limiting
as specified in ADR 0004.

Key Features:
- Schema validation with type coercion
- Confirmation threshold checking
- Rate limiting (token bucket algorithm)
- Security checks (path traversal, injection prevention)
- Execution history tracking
"""

import re
import json
import time
from collections import deque
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from enum import Enum
from pathlib import Path
from typing import Any, Callable, Literal

from .tool_definitions import ToolDefinition, TOOL_INDEX


# =============================================================================
# Exceptions
# =============================================================================


class ToolExecutionError(Exception):
    """Base exception for tool execution errors."""
    pass


class ValidationError(ToolExecutionError):
    """Raised when tool call validation fails."""
    pass


class SecurityError(ToolExecutionError):
    """Raised when security check fails."""
    pass


class RateLimitError(ToolExecutionError):
    """Raised when rate limit is exceeded."""
    pass


class ConfirmationRequiredError(ToolExecutionError):
    """Raised when user confirmation is required."""
    def __init__(self, message: str, tool: str, args: dict[str, Any]):
        super().__init__(message)
        self.tool = tool
        self.args = args


# =============================================================================
# Rate Limiting (Token Bucket Algorithm)
# =============================================================================


@dataclass
class TokenBucket:
    """
    Token bucket for rate limiting.

    Allows bursts up to capacity while maintaining average rate.
    """
    capacity: int  # Maximum tokens
    refill_rate: float  # Tokens per second
    tokens: float = 10.0  # Current tokens (default to full capacity)
    last_refill: float = field(default_factory=time.time)

    def consume(self, tokens: int = 1) -> bool:
        """
        Try to consume tokens.

        Returns True if successful, False if not enough tokens.
        """
        now = time.time()
        # Refill tokens based on elapsed time
        elapsed = now - self.last_refill
        self.tokens = min(self.capacity, self.tokens + elapsed * self.refill_rate)
        self.last_refill = now

        if self.tokens >= tokens:
            self.tokens -= tokens
            return True
        return False

    def wait_time(self, tokens: int = 1) -> float:
        """Calculate seconds to wait for tokens to be available."""
        now = time.time()
        elapsed = now - self.last_refill
        available = min(self.capacity, self.tokens + elapsed * self.refill_rate)

        if available >= tokens:
            return 0.0

        needed = tokens - available
        return needed / self.refill_rate


# =============================================================================
# Tool Call Validation
# =============================================================================


class ToolCallValidator:
    """
    Validates tool calls against schema and security policies.
    """

    # Security patterns to reject
    PATH_TRAVERSAL_PATTERNS = [
        r'\.\./',  # ../
        r'\.\.\\',  # ..\
        r'~/',  # home directory expansion
        r'^/',  # absolute paths (context-dependent)
    ]

    COMMAND_INJECTION_PATTERNS = [
        r';\s*\w+',  # command chaining
        r'\|\s*\w+',  # pipe to command
        r'`\s*\w+',  # command substitution
        r'\$\([^)]*\)',  # command substitution
        r'&&\s*\w+',  # command chaining
        r'\|\|\s*\w+',  # command chaining
    ]

    def __init__(self, strict_mode: bool = True) -> None:
        """
        Initialize validator.

        Args:
            strict_mode: If True, reject suspicious patterns. If False, allow with warning.
        """
        self.strict_mode = strict_mode

    def validate(
        self,
        tool_name: str,
        arguments: dict[str, Any],
        tool_definition: ToolDefinition | None = None
    ) -> dict[str, Any]:
        """
        Validate a tool call.

        Args:
            tool_name: Name of the tool being called
            arguments: Arguments passed to the tool
            tool_definition: Optional tool definition (uses TOOL_INDEX if None)

        Returns:
            Validated and coerced arguments

        Raises:
            ValidationError: If validation fails
            SecurityError: If security check fails
            ConfirmationRequiredError: If confirmation threshold exceeded
        """
        # Get tool definition
        if tool_definition is None:
            tool_definition = TOOL_INDEX.get(tool_name)
            if tool_definition is None:
                raise ValidationError(f"Unknown tool: {tool_name}")

        # Check required parameters
        self._validate_required(tool_definition, arguments)

        # Coerce and validate parameter types
        coerced_args = self._coerce_parameters(tool_definition, arguments)

        # Check enum values
        self._validate_enums(tool_definition, coerced_args)

        # Check numeric constraints
        self._validate_numeric_constraints(tool_definition, coerced_args)

        # Check confirmation thresholds
        self._check_confirmation_thresholds(tool_definition, coerced_args)

        # Security checks
        self._security_check(tool_name, coerced_args)

        return coerced_args

    def _validate_required(
        self,
        tool: ToolDefinition,
        arguments: dict[str, Any]
    ) -> None:
        """Check that all required parameters are present."""
        for param in tool.parameters:
            if param.required and param.name not in arguments:
                raise ValidationError(
                    f"Missing required parameter '{param.name}' for tool '{tool.name}'"
                )

    def _coerce_parameters(
        self,
        tool: ToolDefinition,
        arguments: dict[str, Any]
    ) -> dict[str, Any]:
        """
        Coerce parameter values to correct types.

        Uses tool definition schema to convert values.
        """
        coerced = {}

        for param in tool.parameters:
            if param.name not in arguments:
                # Use default if available
                if param.default is not None:
                    coerced[param.name] = param.default
                continue

            value = arguments[param.name]

            # Skip if None or already correct type
            if value is None:
                coerced[param.name] = None
                continue

            # Type coercion based on parameter type
            try:
                if param.type == "string":
                    coerced[param.name] = str(value)
                elif param.type == "integer":
                    if isinstance(value, bool):
                        # Don't coerce bool to int
                        coerced[param.name] = value
                    else:
                        coerced[param.name] = int(value)
                elif param.type == "number":
                    coerced[param.name] = float(value)
                elif param.type == "boolean":
                    if isinstance(value, str):
                        coerced[param.name] = value.lower() in ("true", "1", "yes", "on")
                    else:
                        coerced[param.name] = bool(value)
                elif param.type == "array":
                    if isinstance(value, str):
                        # Try JSON parsing
                        try:
                            coerced[param.name] = json.loads(value)
                        except json.JSONDecodeError:
                            # Split by comma if not JSON
                            coerced[param.name] = [v.strip() for v in value.split(",")]
                    elif isinstance(value, list):
                        coerced[param.name] = value
                    else:
                        coerced[param.name] = [value]
                elif param.type == "object":
                    if isinstance(value, str):
                        try:
                            coerced[param.name] = json.loads(value)
                        except json.JSONDecodeError:
                            raise ValidationError(
                                f"Invalid JSON for parameter '{param.name}': {value}"
                            )
                    else:
                        coerced[param.name] = value
                else:
                    coerced[param.name] = value
            except (ValueError, TypeError) as e:
                raise ValidationError(
                    f"Type coercion failed for parameter '{param.name}': {e}"
                )

        return coerced

    def _validate_enums(
        self,
        tool: ToolDefinition,
        arguments: dict[str, Any]
    ) -> None:
        """Validate enum parameters."""
        for param in tool.parameters:
            if param.name not in arguments or param.enum is None:
                continue

            value = arguments[param.name]

            # Handle arrays
            if param.type == "array":
                if isinstance(value, list):
                    for item in value:
                        if item not in param.enum:
                            raise ValidationError(
                                f"Invalid value '{item}' for parameter '{param.name}'. "
                                f"Must be one of {param.enum}"
                            )
            else:
                if value not in param.enum:
                    raise ValidationError(
                        f"Invalid value '{value}' for parameter '{param.name}'. "
                        f"Must be one of {param.enum}"
                    )

    def _validate_numeric_constraints(
        self,
        tool: ToolDefinition,
        arguments: dict[str, Any]
    ) -> None:
        """Validate numeric minimum/maximum constraints."""
        for param in tool.parameters:
            if param.name not in arguments:
                continue

            value = arguments[param.name]

            if param.minimum is not None and isinstance(value, (int, float)):
                if value < param.minimum:
                    raise ValidationError(
                        f"Parameter '{param.name}' value {value} is below minimum {param.minimum}"
                    )

            if param.maximum is not None and isinstance(value, (int, float)):
                if value > param.maximum:
                    raise ValidationError(
                        f"Parameter '{param.name}' value {value} exceeds maximum {param.maximum}"
                    )

    def _check_confirmation_thresholds(
        self,
        tool: ToolDefinition,
        arguments: dict[str, Any]
    ) -> None:
        """Check if confirmation is required based on thresholds."""
        if tool.confirmation_threshold is None:
            return

        for key, threshold in tool.confirmation_threshold.items():
            if key in arguments:
                value = arguments[key]

                # Numeric threshold
                if isinstance(threshold, (int, float)):
                    if isinstance(value, (int, float)) and value > threshold:
                        message = tool.confirmation_message or (
                            f"Tool '{tool.name}' requires confirmation: "
                            f"{key}={value} exceeds threshold {threshold}"
                        )
                        raise ConfirmationRequiredError(message, tool.name, arguments)

                # List length threshold
                elif key == "count" and isinstance(value, int):
                    if value > threshold:
                        message = tool.confirmation_message or (
                            f"Tool '{tool.name}' requires confirmation: "
                            f"count={value} exceeds threshold {threshold}"
                        )
                        raise ConfirmationRequiredError(message, tool.name, arguments)

    def _security_check(
        self,
        tool_name: str,
        arguments: dict[str, Any]
    ) -> None:
        """
        Perform security checks on tool arguments.

        Checks for:
        - Path traversal attempts
        - Command injection patterns
        - Suspicious string values
        """
        for key, value in arguments.items():
            # Skip non-string values
            if not isinstance(value, str):
                continue

            # Check for path traversal
            for pattern in self.PATH_TRAVERSAL_PATTERNS:
                if re.search(pattern, value):
                    if self.strict_mode:
                        raise SecurityError(
                            f"Path traversal detected in parameter '{key}': {value}"
                        )

            # Check for command injection
            for pattern in self.COMMAND_INJECTION_PATTERNS:
                if re.search(pattern, value):
                    if self.strict_mode:
                        raise SecurityError(
                            f"Command injection detected in parameter '{key}': {value}"
                        )


# =============================================================================
# Execution Result
# =============================================================================


@dataclass
class ToolExecutionResult:
    """Result of a tool execution."""
    tool_name: str
    success: bool
    message: str
    data: dict[str, Any] | None = None
    error: str | None = None
    requires_confirmation: bool = False
    confirmation_message: str | None = None
    executed_at: datetime = field(default_factory=datetime.utcnow)

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization."""
        return {
            "tool_name": self.tool_name,
            "success": self.success,
            "message": self.message,
            "data": self.data,
            "error": self.error,
            "requires_confirmation": self.requires_confirmation,
            "confirmation_message": self.confirmation_message,
            "executed_at": self.executed_at.isoformat(),
        }


# =============================================================================
# Tool Execution Engine
# =============================================================================


class ToolExecutionEngine:
    """
    Main tool execution engine.

    Validates, rate limits, and executes tool calls.
    """

    # Global rate limits (calls per minute)
    GLOBAL_RATE_LIMIT = 60  # 60 calls per minute = 1 per second

    def __init__(
        self,
        strict_security: bool = True,
        global_rate_limit: int | None = None
    ) -> None:
        """
        Initialize execution engine.

        Args:
            strict_security: Enable strict security checks
            global_rate_limit: Global rate limit (calls per minute)
        """
        self.validator = ToolCallValidator(strict_mode=strict_security)
        self.global_rate_limit = global_rate_limit or self.GLOBAL_RATE_LIMIT

        # Token bucket for global rate limiting
        self._global_bucket = TokenBucket(
            capacity=self.global_rate_limit,
            refill_rate=self.global_rate_limit / 60.0  # per second
        )

        # Per-tool rate limit buckets
        self._tool_buckets: dict[str, TokenBucket] = {}

        # Execution history
        self._execution_history: deque[ToolExecutionResult] = deque(maxlen=1000)

        # Tool callbacks
        self._callbacks: dict[str, Callable[..., ToolExecutionResult]] = {}

    def register_callback(
        self,
        tool_name: str,
        callback: Callable[..., ToolExecutionResult]
    ) -> None:
        """Register an execution callback for a tool."""
        self._callbacks[tool_name] = callback

    def execute(
        self,
        tool_name: str,
        arguments: dict[str, Any],
        tool_definition: ToolDefinition | None = None,
        skip_rate_limit: bool = False,
        skip_confirmation: bool = False
    ) -> ToolExecutionResult:
        """
        Execute a tool call with full validation and security checks.

        Args:
            tool_name: Name of the tool to execute
            arguments: Tool arguments
            tool_definition: Optional tool definition
            skip_rate_limit: Skip rate limit check (for internal calls)
            skip_confirmation: Execute without confirmation

        Returns:
            ToolExecutionResult with execution status

        Raises:
            ValidationError: If validation fails
            SecurityError: If security check fails
            RateLimitError: If rate limit exceeded
            ConfirmationRequiredError: If confirmation required
        """
        # Get tool definition
        if tool_definition is None:
            tool_definition = TOOL_INDEX.get(tool_name)
            if tool_definition is None:
                return ToolExecutionResult(
                    tool_name=tool_name,
                    success=False,
                    message=f"Unknown tool: {tool_name}",
                    error=f"Tool '{tool_name}' not found"
                )

        # Check global rate limit
        if not skip_rate_limit:
            if not self._global_bucket.consume(1):
                wait_time = self._global_bucket.wait_time(1)
                raise RateLimitError(
                    f"Global rate limit exceeded. Try again in {wait_time:.1f} seconds."
                )

            # Check per-tool rate limit
            if tool_definition.rate_limit is not None:
                bucket = self._get_tool_bucket(tool_definition)
                if not bucket.consume(1):
                    wait_time = bucket.wait_time(1)
                    raise RateLimitError(
                        f"Tool '{tool_name}' rate limit exceeded. "
                        f"Try again in {wait_time:.1f} seconds."
                    )

        # Validate and coerce arguments
        try:
            validated_args = self.validator.validate(tool_name, arguments, tool_definition)
        except ConfirmationRequiredError:
            # Re-raise with proper argument handling
            raise
        except (ValidationError, SecurityError) as e:
            result = ToolExecutionResult(
                tool_name=tool_name,
                success=False,
                message=str(e),
                error=str(e)
            )
            self._execution_history.append(result)
            return result

        # Check if confirmation is required by tool definition
        if tool_definition.requires_confirmation and not skip_confirmation:
            message = tool_definition.confirmation_message or (
                f"Tool '{tool_name}' requires confirmation"
            )
            result = ToolExecutionResult(
                tool_name=tool_name,
                success=False,
                message=message,
                requires_confirmation=True,
                confirmation_message=message,
                data=validated_args
            )
            self._execution_history.append(result)
            return result

        # Execute the tool
        callback = self._callbacks.get(tool_name)
        if callback is None:
            # No callback registered - return success with data
            result = ToolExecutionResult(
                tool_name=tool_name,
                success=True,
                message=f"Tool '{tool_name}' executed (no callback registered)",
                data=validated_args
            )
        else:
            try:
                result = callback(**validated_args)
            except Exception as e:
                result = ToolExecutionResult(
                    tool_name=tool_name,
                    success=False,
                    message=f"Error executing '{tool_name}'",
                    error=str(e)
                )

        # Record in history
        self._execution_history.append(result)

        return result

    def execute_batch(
        self,
        tool_calls: list[tuple[str, dict[str, Any]]],
        skip_rate_limit: bool = False,
        skip_confirmation: bool = False
    ) -> list[ToolExecutionResult]:
        """
        Execute multiple tool calls in batch.

        Args:
            tool_calls: List of (tool_name, arguments) tuples
            skip_rate_limit: Skip rate limit check
            skip_confirmation: Execute without confirmation

        Returns:
            List of execution results
        """
        results = []
        for tool_name, arguments in tool_calls:
            result = self.execute(
                tool_name,
                arguments,
                skip_rate_limit=skip_rate_limit,
                skip_confirmation=skip_confirmation
            )
            results.append(result)

            # Stop on confirmation requirement
            if result.requires_confirmation:
                break

        return results

    def _get_tool_bucket(self, tool: ToolDefinition) -> TokenBucket:
        """Get or create rate limit bucket for a tool."""
        if tool.name not in self._tool_buckets:
            self._tool_buckets[tool.name] = TokenBucket(
                capacity=tool.rate_limit or 10,
                refill_rate=(tool.rate_limit or 10) / 60.0
            )
        return self._tool_buckets[tool.name]

    def get_execution_history(
        self,
        tool_name: str | None = None,
        limit: int = 100
    ) -> list[ToolExecutionResult]:
        """
        Get execution history.

        Args:
            tool_name: Filter by tool name (optional)
            limit: Maximum number of results

        Returns:
            List of execution results
        """
        history = list(self._execution_history)

        if tool_name is not None:
            history = [r for r in history if r.tool_name == tool_name]

        return history[-limit:]

    def get_execution_stats(self) -> dict[str, Any]:
        """Get execution statistics."""
        history = list(self._execution_history)

        total = len(history)
        success = sum(1 for r in history if r.success)
        failed = total - success
        confirmations = sum(1 for r in history if r.requires_confirmation)

        # Tool breakdown
        tool_counts: dict[str, int] = {}
        for result in history:
            tool_counts[result.tool_name] = tool_counts.get(result.tool_name, 0) + 1

        return {
            "total_executions": total,
            "successful": success,
            "failed": failed,
            "confirmations_required": confirmations,
            "success_rate": success / total if total > 0 else 0,
            "tool_counts": tool_counts,
        }


# =============================================================================
# Helper Functions
# =============================================================================


def create_success_result(
    tool_name: str,
    message: str,
    data: dict[str, Any] | None = None
) -> ToolExecutionResult:
    """Create a successful execution result."""
    return ToolExecutionResult(
        tool_name=tool_name,
        success=True,
        message=message,
        data=data
    )


def create_error_result(
    tool_name: str,
    message: str,
    error: str | None = None
) -> ToolExecutionResult:
    """Create an error execution result."""
    return ToolExecutionResult(
        tool_name=tool_name,
        success=False,
        message=message,
        error=error or message
    )


def create_confirmation_result(
    tool_name: str,
    message: str,
    arguments: dict[str, Any]
) -> ToolExecutionResult:
    """Create a confirmation-required result."""
    return ToolExecutionResult(
        tool_name=tool_name,
        success=False,
        message=message,
        requires_confirmation=True,
        confirmation_message=message,
        data=arguments
    )
