"""
FORGE Tool Definitions and Execution Engine

Defines tools for the conversational interface and provides
the execution framework for tool calls.

Supports:
- OpenAI function calling compatible JSON format
- YAML documentation format
- Tool injection to chat backends
"""

from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Callable
import json
from pathlib import Path


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
    NOTIFICATION = "notification"
    SYSTEM = "system"
    WORKSPACE = "workspace"
    ANALYTICS = "analytics"


@dataclass
class ToolParameter:
    """Tool parameter definition"""
    name: str
    type: str  # "string", "integer", "boolean", "array", "number"
    description: str
    required: bool = False
    default: Any = None
    enum: list[Any] | None = None  # For enum parameters
    minimum: int | float | None = None  # For numeric parameters
    maximum: int | float | None = None  # For numeric parameters
    items: dict[str, Any] | None = None  # For array item types


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

    def to_openai_format(self) -> dict[str, Any]:
        """
        Convert to OpenAI function calling format.

        Returns a dictionary compatible with OpenAI/Anthropic function calling API.
        """
        properties: dict[str, Any] = {}
        required: list[str] = []

        for param in self.parameters:
            prop: dict[str, Any] = {
                "type": param.type,
                "description": param.description,
            }

            # Add enum if present
            if param.enum:
                prop["enum"] = param.enum

            # Add numeric constraints
            if param.minimum is not None:
                prop["minimum"] = param.minimum
            if param.maximum is not None:
                prop["maximum"] = param.maximum

            # Add array item type
            if param.items:
                prop["items"] = param.items

            # Add default if present
            if param.default is not None:
                prop["default"] = param.default

            properties[param.name] = prop

            if param.required:
                required.append(param.name)

        return {
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": {
                    "type": "object",
                    "properties": properties,
                    "required": required,
                }
            }
        }

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization"""
        return {
            "name": self.name,
            "description": self.description,
            "category": self.category.value,
            "parameters": [
                {
                    "name": p.name,
                    "type": p.type,
                    "description": p.description,
                    "required": p.required,
                    "default": p.default,
                    "enum": p.enum,
                    "minimum": p.minimum,
                    "maximum": p.maximum,
                    "items": p.items,
                }
                for p in self.parameters
            ],
            "requires_confirmation": self.requires_confirmation,
            "confirmation_message": self.confirmation_message,
            "rate_limit": self.rate_limit,
        }

    def to_yaml(self) -> str:
        """Convert to YAML format for documentation"""
        lines = [
            f"### `{self.name}`",
            f"{self.description}",
            "",
            "**Parameters**:",
        ]

        for param in self.parameters:
            req = "required" if param.required else "optional"
            type_str = param.type
            if param.enum:
                type_str += f", enum: {param.enum}"
            if param.minimum is not None:
                type_str += f", min: {param.minimum}"
            if param.maximum is not None:
                type_str += f", max: {param.maximum}"

            lines.append(f"- `{param.name}` ({type_str}, {req}): {param.description}")

        if self.requires_confirmation:
            lines.append("")
            lines.append("**Requires confirmation**: Yes")
            if self.confirmation_message:
                lines.append(f"**Message**: {self.confirmation_message}")

        lines.append("")
        lines.append("---")
        lines.append("")

        return "\n".join(lines)


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


# Worker Management Tools
WORKER_TOOLS = [
    ToolDefinition(
        name="spawn_worker",
        description="Spawn new AI coding workers with specified model and workspace.",
        category=ToolCategory.WORKER_MANAGEMENT,
        parameters=[
            ToolParameter(
                name="model",
                type="string",
                description="Model type for the worker (e.g., sonnet, opus, haiku, gpt4, qwen)",
                required=True,
                enum=["sonnet", "opus", "haiku", "gpt4", "qwen", "claude-opus", "claude-sonnet", "gpt-4", "gpt-3.5-turbo"]
            ),
            ToolParameter(
                name="count",
                type="integer",
                description="Number of workers to spawn",
                required=True,
                minimum=1,
                maximum=10,
                default=1
            ),
            ToolParameter(
                name="workspace",
                type="string",
                description="Workspace path (optional, defaults to current workspace)",
                required=False
            )
        ],
        requires_confirmation=True,
        confirmation_message="Spawn {count} {model} workers in {workspace}?",
        rate_limit=10
    ),
    ToolDefinition(
        name="kill_worker",
        description="Terminate a specific worker or all workers.",
        category=ToolCategory.WORKER_MANAGEMENT,
        parameters=[
            ToolParameter(
                name="worker_id",
                type="string",
                description="Worker identifier (e.g., sonnet-alpha) or 'all' for all workers",
                required=True
            ),
            ToolParameter(
                name="filter",
                type="string",
                description="Optional filter to apply when using 'all' (e.g., idle, active, failed)",
                required=False,
                enum=["idle", "active", "failed"]
            )
        ],
        requires_confirmation=True,
        confirmation_message="Terminate worker {worker_id}?"
    ),
    ToolDefinition(
        name="list_workers",
        description="List workers with optional filtering by status.",
        category=ToolCategory.WORKER_MANAGEMENT,
        parameters=[
            ToolParameter(
                name="filter",
                type="string",
                description="Filter workers by status",
                required=False,
                enum=["idle", "active", "failed", "all"]
            )
        ]
    ),
    ToolDefinition(
        name="restart_worker",
        description="Restart a worker (kills and respawns with same configuration).",
        category=ToolCategory.WORKER_MANAGEMENT,
        parameters=[
            ToolParameter(
                name="worker_id",
                type="string",
                description="Worker identifier to restart",
                required=True
            )
        ],
        requires_confirmation=True,
        confirmation_message="Restart worker {worker_id}?"
    ),
]


# Task Management Tools
TASK_TOOLS = [
    ToolDefinition(
        name="filter_tasks",
        description="Filter the task queue display by priority, status, or labels.",
        category=ToolCategory.TASK_MANAGEMENT,
        parameters=[
            ToolParameter(
                name="priority",
                type="string",
                description="Filter by task priority",
                required=False,
                enum=["P0", "P1", "P2", "P3", "P4"]
            ),
            ToolParameter(
                name="status",
                type="string",
                description="Filter by task status",
                required=False,
                enum=["open", "in_progress", "blocked", "completed"]
            ),
            ToolParameter(
                name="labels",
                type="array",
                description="Filter by labels (array of label strings)",
                required=False,
                items={"type": "string"}
            )
        ]
    ),
    ToolDefinition(
        name="create_task",
        description="Create a new task (bead) with title and priority.",
        category=ToolCategory.TASK_MANAGEMENT,
        parameters=[
            ToolParameter(
                name="title",
                type="string",
                description="Task title",
                required=True
            ),
            ToolParameter(
                name="priority",
                type="string",
                description="Task priority level",
                required=True,
                enum=["P0", "P1", "P2", "P3", "P4"],
                default="P2"
            ),
            ToolParameter(
                name="description",
                type="string",
                description="Detailed task description",
                required=False
            )
        ]
    ),
    ToolDefinition(
        name="assign_task",
        description="Assign a task to a specific worker or auto-assign.",
        category=ToolCategory.TASK_MANAGEMENT,
        parameters=[
            ToolParameter(
                name="task_id",
                type="string",
                description="Task/bead ID (e.g., bd-abc)",
                required=True
            ),
            ToolParameter(
                name="worker_id",
                type="string",
                description="Worker ID or 'auto' for automatic assignment",
                required=False,
                default="auto"
            )
        ]
    ),
]


# Cost & Analytics Tools
COST_TOOLS = [
    ToolDefinition(
        name="show_costs",
        description="Display cost analysis for a specific time period and breakdown type.",
        category=ToolCategory.COST_ANALYTICS,
        parameters=[
            ToolParameter(
                name="period",
                type="string",
                description="Time period for cost analysis",
                required=False,
                enum=["today", "yesterday", "this_week", "last_week", "this_month", "last_month"],
                default="today"
            ),
            ToolParameter(
                name="breakdown",
                type="string",
                description="Breakdown type for cost analysis",
                required=False,
                enum=["by_model", "by_worker", "by_task", "by_workspace"]
            )
        ]
    ),
    ToolDefinition(
        name="optimize_routing",
        description="Run cost optimization analysis and update routing rules based on historical data.",
        category=ToolCategory.COST_ANALYTICS,
        parameters=[],
        requires_confirmation=True,
        confirmation_message="Apply cost optimization recommendations? This will update routing rules.",
        rate_limit=2
    ),
    ToolDefinition(
        name="forecast_costs",
        description="Forecast future costs based on current usage patterns.",
        category=ToolCategory.COST_ANALYTICS,
        parameters=[
            ToolParameter(
                name="days",
                type="integer",
                description="Number of days to forecast",
                required=False,
                minimum=1,
                maximum=90,
                default=30
            )
        ]
    ),
    ToolDefinition(
        name="show_metrics",
        description="Display performance metrics for a specific metric type and time period.",
        category=ToolCategory.COST_ANALYTICS,
        parameters=[
            ToolParameter(
                name="metric_type",
                type="string",
                description="Type of metrics to display",
                required=False,
                enum=["throughput", "latency", "success_rate", "all"],
                default="all"
            ),
            ToolParameter(
                name="period",
                type="string",
                description="Time period for metrics",
                required=False,
                enum=["today", "yesterday", "this_week", "last_week", "this_month", "last_month"],
                default="today"
            )
        ]
    ),
]


# Data Export Tools
EXPORT_TOOLS = [
    ToolDefinition(
        name="export_logs",
        description="Export activity logs in the specified format and time period.",
        category=ToolCategory.DATA_EXPORT,
        parameters=[
            ToolParameter(
                name="format",
                type="string",
                description="Output format for logs",
                required=False,
                enum=["json", "csv", "txt"],
                default="json"
            ),
            ToolParameter(
                name="period",
                type="string",
                description="Time period for log export",
                required=False,
                enum=["today", "yesterday", "this_week", "last_week", "this_month", "last_month"],
                default="today"
            )
        ]
    ),
    ToolDefinition(
        name="export_metrics",
        description="Export metrics data in the specified format.",
        category=ToolCategory.DATA_EXPORT,
        parameters=[
            ToolParameter(
                name="metric_type",
                type="string",
                description="Type of metrics to export",
                required=False,
                enum=["performance", "costs", "workers", "all"],
                default="all"
            ),
            ToolParameter(
                name="format",
                type="string",
                description="Output format for metrics",
                required=False,
                enum=["json", "csv"],
                default="json"
            )
        ]
    ),
    ToolDefinition(
        name="screenshot",
        description="Take a screenshot of the dashboard or specific panel.",
        category=ToolCategory.DATA_EXPORT,
        parameters=[
            ToolParameter(
                name="panel",
                type="string",
                description="Specific panel name or 'all' for full dashboard",
                required=False,
                default="all",
                enum=["workers", "tasks", "costs", "metrics", "logs", "chat", "all"]
            )
        ]
    ),
]


# Configuration Tools
CONFIG_TOOLS = [
    ToolDefinition(
        name="set_config",
        description="Update a configuration setting with a new value.",
        category=ToolCategory.CONFIGURATION,
        parameters=[
            ToolParameter(
                name="key",
                type="string",
                description="Configuration key to set",
                required=True
            ),
            ToolParameter(
                name="value",
                type="string",
                description="Configuration value to set",
                required=True
            )
        ],
        requires_confirmation=True,
        confirmation_message="Set {key} = {value}?"
    ),
    ToolDefinition(
        name="get_config",
        description="View configuration settings for a specific key or all settings.",
        category=ToolCategory.CONFIGURATION,
        parameters=[
            ToolParameter(
                name="key",
                type="string",
                description="Specific config key to view, or omit for all settings",
                required=False
            )
        ]
    ),
    ToolDefinition(
        name="save_layout",
        description="Save the current dashboard layout as a named preset.",
        category=ToolCategory.CONFIGURATION,
        parameters=[
            ToolParameter(
                name="name",
                type="string",
                description="Layout name for the saved preset",
                required=True
            )
        ]
    ),
    ToolDefinition(
        name="load_layout",
        description="Load a previously saved dashboard layout.",
        category=ToolCategory.CONFIGURATION,
        parameters=[
            ToolParameter(
                name="name",
                type="string",
                description="Layout name to load",
                required=True
            )
        ]
    ),
]


# Help & Discovery Tools
HELP_TOOLS = [
    ToolDefinition(
        name="help",
        description="Get help on a specific topic or general usage information.",
        category=ToolCategory.HELP_DISCOVERY,
        parameters=[
            ToolParameter(
                name="topic",
                type="string",
                description="Topic name (e.g., spawning, costs, tasks, tools)",
                required=False,
                enum=["spawning", "costs", "tasks", "tools", "workers", "metrics", "configuration"]
            )
        ]
    ),
    ToolDefinition(
        name="search_docs",
        description="Search documentation for a specific query.",
        category=ToolCategory.HELP_DISCOVERY,
        parameters=[
            ToolParameter(
                name="query",
                type="string",
                description="Search query string",
                required=True
            )
        ]
    ),
    ToolDefinition(
        name="list_capabilities",
        description="List all available tools and features in FORGE.",
        category=ToolCategory.HELP_DISCOVERY,
        parameters=[]
    ),
]


# Notification Tools
NOTIFICATION_TOOLS = [
    ToolDefinition(
        name="show_notification",
        description="Display a notification message to the user.",
        category=ToolCategory.NOTIFICATION,
        parameters=[
            ToolParameter(
                name="message",
                type="string",
                description="Notification message to display",
                required=True
            ),
            ToolParameter(
                name="level",
                type="string",
                description="Notification level",
                required=False,
                enum=["info", "warning", "error", "success"],
                default="info"
            )
        ]
    ),
    ToolDefinition(
        name="show_warning",
        description="Display a warning message to the user.",
        category=ToolCategory.NOTIFICATION,
        parameters=[
            ToolParameter(
                name="message",
                type="string",
                description="Warning message to display",
                required=True
            ),
            ToolParameter(
                name="details",
                type="string",
                description="Additional details about the warning",
                required=False
            )
        ]
    ),
    ToolDefinition(
        name="ask_user",
        description="Prompt the user for input with a question and options.",
        category=ToolCategory.NOTIFICATION,
        parameters=[
            ToolParameter(
                name="question",
                type="string",
                description="Question to ask the user",
                required=True
            ),
            ToolParameter(
                name="options",
                type="array",
                description="List of options for the user to choose from",
                required=False,
                items={"type": "string"}
            )
        ]
    ),
    ToolDefinition(
        name="highlight_beads",
        description="Highlight specific beads in the task queue.",
        category=ToolCategory.NOTIFICATION,
        parameters=[
            ToolParameter(
                name="bead_ids",
                type="array",
                description="List of bead IDs to highlight",
                required=True,
                items={"type": "string"}
            ),
            ToolParameter(
                name="reason",
                type="string",
                description="Reason for highlighting",
                required=False
            )
        ]
    ),
]


# System Tools
SYSTEM_TOOLS = [
    ToolDefinition(
        name="get_status",
        description="Get the current status of FORGE and all workers.",
        category=ToolCategory.SYSTEM,
        parameters=[
            ToolParameter(
                name="component",
                type="string",
                description="Specific component to check, or 'all' for everything",
                required=False,
                enum=["all", "workers", "backend", "system"],
                default="all"
            )
        ]
    ),
    ToolDefinition(
        name="refresh",
        description="Refresh the current view or all data.",
        category=ToolCategory.SYSTEM,
        parameters=[
            ToolParameter(
                name="scope",
                type="string",
                description="What to refresh",
                required=False,
                enum=["current", "all", "workers", "tasks", "costs"],
                default="current"
            )
        ]
    ),
    ToolDefinition(
        name="ping_worker",
        description="Check if a worker is responsive.",
        category=ToolCategory.SYSTEM,
        parameters=[
            ToolParameter(
                name="worker_id",
                type="string",
                description="Worker ID to ping",
                required=True
            )
        ]
    ),
    ToolDefinition(
        name="get_worker_info",
        description="Get detailed information about a specific worker.",
        category=ToolCategory.SYSTEM,
        parameters=[
            ToolParameter(
                name="worker_id",
                type="string",
                description="Worker ID to get info for",
                required=True
            )
        ]
    ),
    ToolDefinition(
        name="pause_worker",
        description="Pause a worker (temporarily stop processing).",
        category=ToolCategory.SYSTEM,
        parameters=[
            ToolParameter(
                name="worker_id",
                type="string",
                description="Worker ID to pause",
                required=True
            )
        ],
        requires_confirmation=True,
        confirmation_message="Pause worker {worker_id}?"
    ),
    ToolDefinition(
        name="resume_worker",
        description="Resume a paused worker.",
        category=ToolCategory.SYSTEM,
        parameters=[
            ToolParameter(
                name="worker_id",
                type="string",
                description="Worker ID to resume",
                required=True
            )
        ]
    ),
]


# Workspace Tools
WORKSPACE_TOOLS = [
    ToolDefinition(
        name="switch_workspace",
        description="Switch to a different workspace.",
        category=ToolCategory.WORKSPACE,
        parameters=[
            ToolParameter(
                name="path",
                type="string",
                description="Workspace path to switch to",
                required=True
            )
        ],
        requires_confirmation=True,
        confirmation_message="Switch workspace to {path}?"
    ),
    ToolDefinition(
        name="list_workspaces",
        description="List all available workspaces.",
        category=ToolCategory.WORKSPACE,
        parameters=[
            ToolParameter(
                name="filter",
                type="string",
                description="Filter workspaces by status",
                required=False,
                enum=["active", "inactive", "all"]
            )
        ]
    ),
    ToolDefinition(
        name="create_workspace",
        description="Create a new workspace.",
        category=ToolCategory.WORKSPACE,
        parameters=[
            ToolParameter(
                name="path",
                type="string",
                description="Workspace path to create",
                required=True
            ),
            ToolParameter(
                name="template",
                type="string",
                description="Template to use for workspace",
                required=False,
                enum=["empty", "python", "javascript", "rust"]
            )
        ]
    ),
    ToolDefinition(
        name="get_workspace_info",
        description="Get information about the current workspace.",
        category=ToolCategory.WORKSPACE,
        parameters=[]
    ),
]


# Analytics Tools
ANALYTICS_TOOLS = [
    ToolDefinition(
        name="show_throughput",
        description="Display task throughput metrics.",
        category=ToolCategory.ANALYTICS,
        parameters=[
            ToolParameter(
                name="period",
                type="string",
                description="Time period for analysis",
                required=False,
                enum=["today", "yesterday", "this_week", "last_week", "this_month", "last_month"],
                default="today"
            )
        ]
    ),
    ToolDefinition(
        name="show_latency",
        description="Display task latency metrics.",
        category=ToolCategory.ANALYTICS,
        parameters=[
            ToolParameter(
                name="period",
                type="string",
                description="Time period for analysis",
                required=False,
                enum=["today", "yesterday", "this_week", "last_week", "this_month", "last_month"],
                default="today"
            )
        ]
    ),
    ToolDefinition(
        name="show_success_rate",
        description="Display task success rate metrics.",
        category=ToolCategory.ANALYTICS,
        parameters=[
            ToolParameter(
                name="period",
                type="string",
                description="Time period for analysis",
                required=False,
                enum=["today", "yesterday", "this_week", "last_week", "this_month", "last_month"],
                default="today"
            )
        ]
    ),
    ToolDefinition(
        name="show_worker_efficiency",
        description="Display worker efficiency comparison.",
        category=ToolCategory.ANALYTICS,
        parameters=[
            ToolParameter(
                name="by_model",
                type="boolean",
                description="Group by model type",
                required=False,
                default=True
            )
        ]
    ),
    ToolDefinition(
        name="show_task_distribution",
        description="Display task distribution across priorities.",
        category=ToolCategory.ANALYTICS,
        parameters=[]
    ),
    ToolDefinition(
        name="show_trends",
        description="Display trends for a specific metric over time.",
        category=ToolCategory.ANALYTICS,
        parameters=[
            ToolParameter(
                name="metric",
                type="string",
                description="Metric to show trends for",
                required=True,
                enum=["costs", "throughput", "latency", "success_rate", "worker_count"]
            ),
            ToolParameter(
                name="period",
                type="string",
                description="Time period for trends",
                required=False,
                enum=["today", "this_week", "this_month", "last_week", "last_month"],
                default="this_week"
            )
        ]
    ),
    ToolDefinition(
        name="analyze_bottlenecks",
        description="Analyze potential bottlenecks in the workflow.",
        category=ToolCategory.ANALYTICS,
        parameters=[]
    ),
]


# All tools catalog
ALL_TOOLS = (
    VIEW_TOOLS +
    WORKER_TOOLS +
    TASK_TOOLS +
    COST_TOOLS +
    EXPORT_TOOLS +
    CONFIG_TOOLS +
    HELP_TOOLS +
    NOTIFICATION_TOOLS +
    SYSTEM_TOOLS +
    WORKSPACE_TOOLS +
    ANALYTICS_TOOLS
)


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

    def __init__(self, register_all: bool = True) -> None:
        self._tools: dict[str, ToolDefinition] = {}
        self._callbacks: dict[str, Callable[..., ToolCallResult]] = {}
        self._call_history: list[dict[str, Any]] = []
        self._rate_limits: dict[str, list[float]] = {}

        # Register tools
        if register_all:
            # Register all tools from catalog
            for tool in ALL_TOOLS:
                self.register_tool(tool)
        else:
            # Register only view tools for backward compatibility
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

    def list_tools_openai(self, category: ToolCategory | None = None) -> list[dict[str, Any]]:
        """
        List tools in OpenAI function calling format.

        Returns a list of tool definitions compatible with OpenAI/Anthropic function calling API.
        Each tool is formatted as a dictionary with 'type' and 'function' keys.

        Args:
            category: Optional category filter

        Returns:
            List of tool definitions in OpenAI format
        """
        tools = self.list_tools(category)
        return [tool.to_openai_format() for tool in tools]

    def list_tools_openai_json(self, category: ToolCategory | None = None) -> str:
        """
        Export tools as JSON in OpenAI function calling format.

        Returns a JSON string with tool definitions that can be passed directly
        to LLM APIs that support function calling.

        Args:
            category: Optional category filter

        Returns:
            JSON string with tool definitions in OpenAI format
        """
        tools = self.list_tools_openai(category)
        return json.dumps(tools, indent=2)

    def export_tools_file(
        self,
        path: str | Path,
        format: str = "openai"
    ) -> None:
        """
        Export tool definitions to a file.

        Args:
            path: Output file path
            format: Export format - 'openai' (function calling) or 'json' (simple)
        """
        path = Path(path).expanduser()
        path.parent.mkdir(parents=True, exist_ok=True)

        if format == "openai":
            content = self.list_tools_openai_json()
        elif format == "json":
            content = self.list_tools_json()
        else:
            raise ValueError(f"Unknown format: {format}")

        path.write_text(content)

    def get_tool_count(self) -> int:
        """Get the total number of registered tools"""
        return len(self._tools)

    def get_tools_by_category(self) -> dict[str, list[str]]:
        """Get tool names grouped by category"""
        result: dict[str, list[str]] = {}
        for tool in self._tools.values():
            category = tool.category.value
            if category not in result:
                result[category] = []
            result[category].append(tool.name)
        return result

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


def get_tools_for_llm(format: str = "openai") -> str:
    """
    Get all tool definitions formatted for LLM consumption.

    Args:
        format: Export format - 'openai' (function calling) or 'json' (simple)

    Returns JSON string with tool definitions that can be included
    in system prompts or passed to LLMs with tool calling capabilities.
    """
    executor = ToolExecutor(register_all=True)
    if format == "openai":
        return executor.list_tools_openai_json()
    return executor.list_tools_json()


def generate_tools_file(
    output_path: str | Path = "~/.forge/tools.json",
    format: str = "openai"
) -> Path:
    """
    Generate tools.json file for chat backend integration.

    This creates a tool definitions file that can be passed to external
    LLM backends (claude-code, opencode, etc.) for function calling.

    Args:
        output_path: Where to write the tools.json file
        format: Export format - 'openai' (function calling) or 'json' (simple)

    Returns:
        Path to the generated file
    """
    executor = ToolExecutor(register_all=True)
    executor.export_tools_file(output_path, format=format)
    return Path(output_path).expanduser()


def inject_tools_to_backend(
    tools: list[dict[str, Any]] | None = None,
    backend_config: dict[str, Any] | None = None
) -> dict[str, Any]:
    """
    Prepare tool definitions for injection to chat backend.

    This function formats tool definitions for use with external chat backends,
    following the protocol specified in INTEGRATION_GUIDE.md.

    Args:
        tools: Optional list of tool definitions (uses all tools if None)
        backend_config: Optional backend configuration for tool injection

    Returns:
        Dictionary with formatted tools and metadata for backend integration

    Example:
        >>> payload = inject_tools_to_backend()
        >>> print(payload["tools"])
        >>> # Pass to backend via stdin or CLI args
    """
    if tools is None:
        executor = ToolExecutor(register_all=True)
        tools = executor.list_tools_openai()

    return {
        "version": "1.0",
        "tools": tools,
        "count": len(tools),
        "metadata": {
            "categories": list(ToolCategory),
            "generated_by": "FORGE ToolExecutor",
            "format": "openai_function_calling"
        },
        "backend_config": backend_config or {}
    }


def get_tool_catalog_yaml() -> str:
    """
    Generate tool catalog documentation in YAML format.

    Returns a YAML formatted string documenting all available tools,
    suitable for documentation or CLI help output.
    """
    lines = [
        "# FORGE Tool Catalog",
        "",
        "Complete reference for all tools available in the conversational interface.",
        "",
    ]

    for category in ToolCategory:
        lines.append(f"## {category.value.replace('_', ' ').title()}")
        lines.append("")

        executor = ToolExecutor(register_all=True)
        category_tools = [
            t for t in executor.list_tools()
            if t.category == category
        ]

        for tool in category_tools:
            lines.append(tool.to_yaml())

    return "\n".join(lines)


def initialize_tools(
    output_path: str | Path = "~/.forge/tools.json",
    format: str = "openai",
    force: bool = False
) -> dict[str, Any]:
    """
    Initialize tool definitions for FORGE.

    This function:
    1. Generates tools.json for chat backend integration
    2. Returns injection payload for runtime tool definition

    Args:
        output_path: Where to write tools.json (default: ~/.forge/tools.json)
        format: Export format - 'openai' (function calling) or 'json' (simple)
        force: Regenerate even if file exists

    Returns:
        Dictionary with tool definitions ready for backend injection

    Example:
        >>> # On FORGE startup
        >>> tools_payload = initialize_tools()
        >>> # Pass to chat backend when spawning
    """
    output_path = Path(output_path).expanduser()

    # Generate tools.json if it doesn't exist or force is True
    if force or not output_path.exists():
        generate_tools_file(output_path, format=format)

    # Prepare injection payload
    return inject_tools_to_backend()


def get_default_tools_path() -> Path:
    """Get the default path for tools.json"""
    return Path.home() / ".forge" / "tools.json"


def load_tools_from_file(path: str | Path | None = None) -> list[dict[str, Any]]:
    """
    Load tool definitions from a JSON file.

    Args:
        path: Path to tools.json file (uses default if None)

    Returns:
        List of tool definitions in OpenAI format

    Raises:
        FileNotFoundError: If the tools file doesn't exist
        json.JSONDecodeError: If the file contains invalid JSON
    """
    if path is None:
        path = get_default_tools_path()

    path = Path(path).expanduser()

    if not path.exists():
        raise FileNotFoundError(f"Tools file not found: {path}")

    content = path.read_text()
    data = json.loads(content)

    # Handle both array and object formats
    if isinstance(data, list):
        return data
    elif isinstance(data, dict) and "tools" in data:
        return data["tools"]
    else:
        raise ValueError(f"Invalid tools file format: {path}")
