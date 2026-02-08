"""
FORGE Tool Definitions and Execution Engine

Defines tools for the conversational interface and provides
the execution framework for tool calls.
"""

from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Callable
import json


# =============================================================================
# Tool Definitions
# =============================================================================


class ToolCategory(Enum):
    """Tool category classifications"""
    VIEW_CONTROL = "view_control"
    WORKER_MANAGEMENT = "worker_management"
    TASK_MANAGEMENT = "task_management"
    COST_ANALYTICS = "cost_analytics"
    DATA_EXPORT = "data_export"
    CONFIGURATION = "configuration"
    HELP_DISCOVERY = "help_discovery"


@dataclass
class ToolParameter:
    """Tool parameter definition"""
    name: str
    type: str  # "string", "integer", "boolean", "array"
    description: str
    required: bool = False
    default: Any = None
    enum: list[Any] | None = None  # For enum parameters


@dataclass
class ToolDefinition:
    """Tool definition for the conversational interface"""
    name: str
    description: str
    category: ToolCategory
    parameters: list[ToolParameter] = field(default_factory=list)
    requires_confirmation: bool = False
    confirmation_message: str | None = None
    rate_limit: int | None = None  # Max calls per minute


# View Control Tools
VIEW_TOOLS = [
    ToolDefinition(
        name="switch_view",
        description="Switch to a different dashboard view. Shows full-screen view of a specific panel.",
        category=ToolCategory.VIEW_CONTROL,
        parameters=[
            ToolParameter(
                name="view",
                type="string",
                description="View name to switch to",
                required=True,
                enum=["workers", "tasks", "costs", "metrics", "logs", "overview"]
            )
        ]
    ),
    ToolDefinition(
        name="split_view",
        description="Create a split-screen layout with two views side by side.",
        category=ToolCategory.VIEW_CONTROL,
        parameters=[
            ToolParameter(
                name="left",
                type="string",
                description="Left panel view",
                required=True,
                enum=["workers", "tasks", "costs", "metrics", "logs"]
            ),
            ToolParameter(
                name="right",
                type="string",
                description="Right panel view",
                required=True,
                enum=["workers", "tasks", "costs", "metrics", "logs"]
            )
        ]
    ),
    ToolDefinition(
        name="focus_panel",
        description="Focus on a specific panel within the current view for detailed interaction.",
        category=ToolCategory.VIEW_CONTROL,
        parameters=[
            ToolParameter(
                name="panel",
                type="string",
                description="Panel name to focus",
                required=True,
                enum=["workers", "tasks", "costs", "metrics", "logs", "chat"]
            )
        ]
    ),
]


# =============================================================================
# Tool Call Result
# =============================================================================


@dataclass
class ToolCallResult:
    """Result of a tool execution"""
    success: bool
    tool_name: str
    message: str
    data: dict[str, Any] | None = None
    error: str | None = None
    requires_confirmation: bool = False

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization"""
        return {
            "success": self.success,
            "tool_name": self.tool_name,
            "message": self.message,
            "data": self.data,
            "error": self.error,
            "requires_confirmation": self.requires_confirmation,
        }


# =============================================================================
# Tool Executor
# =============================================================================


class ToolExecutor:
    """
    Executes tool calls and provides feedback to the user.

    Tools are registered with callbacks that are invoked when the tool is called.
    """

    def __init__(self) -> None:
        self._tools: dict[str, ToolDefinition] = {}
        self._callbacks: dict[str, Callable[..., ToolCallResult]] = {}
        self._call_history: list[dict[str, Any]] = []
        self._rate_limits: dict[str, list[float]] = {}

        # Register default view tools
        for tool in VIEW_TOOLS:
            self.register_tool(tool)

    def register_tool(
        self,
        tool: ToolDefinition,
        callback: Callable[..., ToolCallResult] | None = None
    ) -> None:
        """Register a tool with optional execution callback"""
        self._tools[tool.name] = tool
        if callback is not None:
            self._callbacks[tool.name] = callback

        # Initialize rate limit tracking
        if tool.rate_limit is not None:
            self._rate_limits[tool.name] = []

    def get_tool(self, name: str) -> ToolDefinition | None:
        """Get a tool definition by name"""
        return self._tools.get(name)

    def list_tools(self, category: ToolCategory | None = None) -> list[ToolDefinition]:
        """List all tools, optionally filtered by category"""
        tools = list(self._tools.values())
        if category is not None:
            tools = [t for t in tools if t.category == category]
        return tools

    def list_tools_json(self, category: ToolCategory | None = None) -> str:
        """List tools as JSON for LLM consumption"""
        tools = self.list_tools(category)
        tool_list = []
        for tool in tools:
            tool_dict = {
                "name": tool.name,
                "description": tool.description,
                "category": tool.category.value,
                "parameters": [
                    {
                        "name": p.name,
                        "type": p.type,
                        "description": p.description,
                        "required": p.required,
                        "default": p.default,
                        "enum": p.enum,
                    }
                    for p in tool.parameters
                ],
                "requires_confirmation": tool.requires_confirmation,
            }
            tool_list.append(tool_dict)
        return json.dumps(tool_list, indent=2)

    def execute(
        self,
        tool_name: str,
        parameters: dict[str, Any],
        timestamp: float | None = None
    ) -> ToolCallResult:
        """
        Execute a tool call.

        Args:
            tool_name: Name of the tool to execute
            parameters: Tool parameters
            timestamp: Call timestamp for rate limiting (optional)

        Returns:
            ToolCallResult with execution status and message
        """
        # Check if tool exists
        tool = self._tools.get(tool_name)
        if tool is None:
            return ToolCallResult(
                success=False,
                tool_name=tool_name,
                message=f"Unknown tool: {tool_name}",
                error=f"Tool '{tool_name}' not found"
            )

        # Check rate limit
        if not self._check_rate_limit(tool, timestamp):
            return ToolCallResult(
                success=False,
                tool_name=tool_name,
                message=f"Rate limit exceeded for {tool_name}",
                error=f"Tool '{tool_name}' has exceeded rate limit"
            )

        # Validate parameters
        validation_error = self._validate_parameters(tool, parameters)
        if validation_error:
            return ToolCallResult(
                success=False,
                tool_name=tool_name,
                message=f"Invalid parameters: {validation_error}",
                error=validation_error
            )

        # Check if confirmation is required
        if tool.requires_confirmation:
            return ToolCallResult(
                success=False,
                tool_name=tool_name,
                message=tool.confirmation_message or f"Confirm execution of {tool_name}?",
                requires_confirmation=True
            )

        # Execute the tool
        callback = self._callbacks.get(tool_name)
        if callback is None:
            # Default behavior: return success without action
            result = ToolCallResult(
                success=True,
                tool_name=tool_name,
                message=f"Tool '{tool_name}' executed (no callback registered)",
                data=parameters
            )
        else:
            try:
                result = callback(**parameters)
            except Exception as e:
                result = ToolCallResult(
                    success=False,
                    tool_name=tool_name,
                    message=f"Error executing {tool_name}",
                    error=str(e)
                )

        # Record in history
        self._call_history.append({
            "tool_name": tool_name,
            "parameters": parameters,
            "result": result.to_dict(),
            "timestamp": timestamp,
        })

        return result

    def _validate_parameters(
        self,
        tool: ToolDefinition,
        parameters: dict[str, Any]
    ) -> str | None:
        """Validate tool parameters"""
        # Check required parameters
        for param in tool.parameters:
            if param.required and param.name not in parameters:
                return f"Missing required parameter: {param.name}"

            # Check enum values
            if param.name in parameters and param.enum is not None:
                value = parameters[param.name]
                if value not in param.enum:
                    return f"Invalid value for {param.name}: {value}. Must be one of {param.enum}"

        return None

    def _check_rate_limit(
        self,
        tool: ToolDefinition,
        timestamp: float | None
    ) -> bool:
        """Check if tool call is within rate limit"""
        if tool.rate_limit is None:
            return True

        if timestamp is None:
            import time
            timestamp = time.time()

        # Get recent calls
        calls = self._rate_limits.get(tool.name, [])

        # Remove calls older than 60 seconds
        cutoff = timestamp - 60
        calls = [c for c in calls if c > cutoff]

        # Check if we're within limit
        if len(calls) >= tool.rate_limit:
            return False

        # Add this call
        calls.append(timestamp)
        self._rate_limits[tool.name] = calls
        return True

    def get_call_history(self, limit: int = 100) -> list[dict[str, Any]]:
        """Get recent tool call history"""
        return self._call_history[-limit:]


# =============================================================================
# Convenience Functions
# =============================================================================


def create_success_result(
    tool_name: str,
    message: str,
    data: dict[str, Any] | None = None
) -> ToolCallResult:
    """Create a successful tool call result"""
    return ToolCallResult(
        success=True,
        tool_name=tool_name,
        message=message,
        data=data
    )


def create_error_result(
    tool_name: str,
    message: str,
    error: str | None = None
) -> ToolCallResult:
    """Create an error tool call result"""
    return ToolCallResult(
        success=False,
        tool_name=tool_name,
        message=message,
        error=error or message
    )


# =============================================================================
# Export all tools for LLM consumption
# =============================================================================


def get_tools_for_llm() -> str:
    """
    Get all tool definitions formatted for LLM consumption.

    Returns JSON string with tool definitions that can be included
    in system prompts or passed to LLMs with tool calling capabilities.
    """
    executor = ToolExecutor()
    return executor.list_tools_json()
