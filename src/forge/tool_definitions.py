"""
FORGE Tool Definitions - OpenAI Function Calling Compatible

Defines all tools available in the FORGE conversational interface.
Tools are exported in OpenAI function calling format for LLM consumption.

Usage:
    from forge.tool_definitions import (
        get_all_tools,
        get_tools_for_llm,
        export_tools_json,
        export_tools_yaml,
    )
"""

from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Literal
import json


# =============================================================================
# Tool Schema Format (OpenAI Function Calling Compatible)
# =============================================================================


class ToolCategory(Enum):
    """Tool category classifications for organization and filtering"""
    VIEW_CONTROL = "view_control"
    WORKER_MANAGEMENT = "worker_management"
    TASK_MANAGEMENT = "task_management"
    COST_ANALYTICS = "cost_analytics"
    DATA_EXPORT = "data_export"
    CONFIGURATION = "configuration"
    HELP_DISCOVERY = "help_discovery"


@dataclass
class ToolParameter:
    """
    Tool parameter definition compatible with OpenAI function calling schema.

    Attributes:
        name: Parameter name
        type: JSON Schema type (string, integer, boolean, array, object)
        description: Human-readable parameter description
        required: Whether this parameter is required
        default: Default value (optional)
        enum: List of allowed values (optional, for enum parameters)
        minimum: Minimum value for numeric types (optional)
        maximum: Maximum value for numeric types (optional)
        items: Type of array items if type is "array" (optional)
        properties: Nested properties if type is "object" (optional)
    """
    name: str
    type: Literal["string", "integer", "boolean", "array", "object", "number"]
    description: str
    required: bool = False
    default: Any = None
    enum: list[Any] | None = None
    minimum: int | float | None = None
    maximum: int | float | None = None
    items: dict[str, Any] | None = None
    properties: dict[str, Any] | None = None

    def to_openai_schema(self) -> dict[str, Any]:
        """Convert to OpenAI function calling parameter schema"""
        schema: dict[str, Any] = {
            "type": self.type,
            "description": self.description,
        }

        if self.enum:
            schema["enum"] = self.enum
        if self.minimum is not None:
            schema["minimum"] = self.minimum
        if self.maximum is not None:
            schema["maximum"] = self.maximum
        if self.items:
            schema["items"] = self.items
        if self.properties:
            schema["properties"] = self.properties
        if self.default is not None:
            schema["default"] = self.default

        return schema


@dataclass
class ToolDefinition:
    """
    Complete tool definition compatible with OpenAI function calling.

    This represents a tool that can be called by an LLM via function calling.
    The format is compatible with:
    - OpenAI Function Calling
    - Anthropic Tool Use
    - Claude Code tool system

    Attributes:
        name: Unique tool identifier (snake_case)
        description: What this tool does and when to use it
        category: Tool category for organization
        parameters: List of parameter definitions
        requires_confirmation: Whether user must confirm before execution
        confirmation_message: Custom message for confirmation dialog
        confirmation_threshold: Conditional confirmation (e.g., count > 5)
        rate_limit: Max calls per minute (optional)
        examples: Example usage with natural language
    """
    name: str
    description: str
    category: ToolCategory
    parameters: list[ToolParameter] = field(default_factory=list)
    requires_confirmation: bool = False
    confirmation_message: str | None = None
    confirmation_threshold: dict[str, Any] | None = None
    rate_limit: int | None = None
    examples: list[dict[str, Any]] = field(default_factory=list)

    def to_openai_schema(self) -> dict[str, Any]:
        """
        Convert to OpenAI function calling schema.

        Returns:
            Dictionary in OpenAI function calling format
        """
        # Build properties object
        properties = {}
        required = []

        for param in self.parameters:
            properties[param.name] = param.to_openai_schema()
            if param.required:
                required.append(param.name)

        # Build function schema
        schema = {
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": {
                    "type": "object",
                    "properties": properties,
                }
            }
        }

        if required:
            schema["function"]["parameters"]["required"] = required

        return schema

    def to_anthropic_schema(self) -> dict[str, Any]:
        """
        Convert to Anthropic tool use schema.

        Returns:
            Dictionary in Anthropic tool use format
        """
        properties = {}
        required = []

        for param in self.parameters:
            properties[param.name] = param.to_openai_schema()
            if param.required:
                required.append(param.name)

        input_schema = {
            "type": "object",
            "properties": properties,
        }

        if required:
            input_schema["required"] = required

        return {
            "name": self.name,
            "description": self.description,
            "input_schema": input_schema
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
                }
                for p in self.parameters
            ],
            "requires_confirmation": self.requires_confirmation,
            "confirmation_message": self.confirmation_message,
            "confirmation_threshold": self.confirmation_threshold,
            "rate_limit": self.rate_limit,
            "examples": self.examples,
        }

    def to_yaml_dict(self) -> dict[str, Any]:
        """Convert to dictionary for YAML serialization (more readable)"""
        params = {}
        required = []

        for p in self.parameters:
            param_dict = {
                "type": p.type,
                "description": p.description,
            }
            if p.enum:
                param_dict["enum"] = p.enum
            if p.minimum is not None:
                param_dict["min"] = p.minimum
            if p.maximum is not None:
                param_dict["max"] = p.maximum
            if p.default is not None:
                param_dict["default"] = p.default
            if p.required:
                required.append(p.name)

            params[p.name] = param_dict

        result = {
            "name": self.name,
            "description": self.description,
            "category": self.category.value,
        }

        if params:
            result["parameters"] = params
        if required:
            result["required"] = required
        if self.requires_confirmation:
            result["requires_confirmation"] = True
            if self.confirmation_message:
                result["confirmation_message"] = self.confirmation_message
        if self.confirmation_threshold:
            result["confirmation_threshold"] = self.confirmation_threshold
        if self.examples:
            result["examples"] = self.examples

        return result


# =============================================================================
# Tool Catalog - All 30+ Tools from TOOL_CATALOG.md
# =============================================================================

# View Control Tools
VIEW_CONTROL_TOOLS = [
    ToolDefinition(
        name="switch_view",
        description="Switch to a different dashboard view. Shows full-screen view of a specific panel. Use this when the user wants to focus on one specific type of information.",
        category=ToolCategory.VIEW_CONTROL,
        parameters=[
            ToolParameter(
                name="view",
                type="string",
                description="View name to switch to",
                required=True,
                enum=["workers", "tasks", "costs", "metrics", "logs", "overview"]
            )
        ],
        examples=[
            {"user": "Show me the worker status", "tool": "switch_view", "arguments": {"view": "workers"}},
            {"user": "Go to cost view", "tool": "switch_view", "arguments": {"view": "costs"}},
            {"user": "Show me the dashboard", "tool": "switch_view", "arguments": {"view": "overview"}},
        ]
    ),
    ToolDefinition(
        name="split_view",
        description="Create a split-screen layout with two views side by side. Use this when the user wants to monitor two different types of information simultaneously.",
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
        ],
        examples=[
            {"user": "Show workers on left and tasks on right", "tool": "split_view", "arguments": {"left": "workers", "right": "tasks"}},
            {"user": "Split screen with costs and metrics", "tool": "split_view", "arguments": {"left": "costs", "right": "metrics"}},
        ]
    ),
    ToolDefinition(
        name="focus_panel",
        description="Focus on a specific panel within the current view for detailed interaction. Use this to expand a panel for more detailed viewing.",
        category=ToolCategory.VIEW_CONTROL,
        parameters=[
            ToolParameter(
                name="panel",
                type="string",
                description="Panel name to focus",
                required=True,
                enum=["workers", "tasks", "costs", "metrics", "logs", "chat", "activity_log", "task_queue", "worker_status", "cost_breakdown"]
            )
        ],
        examples=[
            {"user": "Focus on the activity log", "tool": "focus_panel", "arguments": {"panel": "activity_log"}},
            {"user": "Expand the cost breakdown", "tool": "focus_panel", "arguments": {"panel": "cost_breakdown"}},
        ]
    ),
]

# Worker Management Tools
WORKER_MANAGEMENT_TOOLS = [
    ToolDefinition(
        name="spawn_worker",
        description="Spawn new AI coding workers. Use this when the user needs more workers to handle tasks. Workers run in tmux sessions and work on beads (tasks) autonomously.",
        category=ToolCategory.WORKER_MANAGEMENT,
        parameters=[
            ToolParameter(
                name="model",
                type="string",
                description="Model type for the worker",
                required=True,
                enum=["sonnet", "opus", "haiku", "gpt4", "qwen", "claude-sonnet-4.5", "claude-opus-4", "claude-haiku-4.5", "gpt-4o", "qwen-2.5-72b", "glm-4.7"]
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
        confirmation_threshold={"count": 5},
        confirmation_message="Spawn {count} {model} workers?",
        examples=[
            {"user": "Spawn 3 sonnet workers", "tool": "spawn_worker", "arguments": {"model": "sonnet", "count": 3}},
            {"user": "Start 2 opus workers in the trading project", "tool": "spawn_worker", "arguments": {"model": "opus", "count": 2, "workspace": "/path/to/trading"}},
            {"user": "I need more workers", "tool": "spawn_worker", "arguments": {"model": "sonnet", "count": 2}},
        ]
    ),
    ToolDefinition(
        name="kill_worker",
        description="Terminate a specific worker or all workers. Use this when a worker is misbehaving, stuck, or no longer needed.",
        category=ToolCategory.WORKER_MANAGEMENT,
        parameters=[
            ToolParameter(
                name="worker_id",
                type="string",
                description="Worker identifier (e.g., 'sonnet-alpha') or 'all' for all workers",
                required=True
            )
        ],
        requires_confirmation=True,
        confirmation_message="Terminate worker {worker_id}?",
        examples=[
            {"user": "Kill worker sonnet-alpha", "tool": "kill_worker", "arguments": {"worker_id": "sonnet-alpha"}},
            {"user": "Stop all idle workers", "tool": "kill_worker", "arguments": {"worker_id": "all"}},
            {"user": "Terminate the failed worker", "tool": "kill_worker", "arguments": {"worker_id": "auto"}},
        ]
    ),
    ToolDefinition(
        name="list_workers",
        description="List workers with optional filtering by status. Use this to show the user the current worker status.",
        category=ToolCategory.WORKER_MANAGEMENT,
        parameters=[
            ToolParameter(
                name="filter",
                type="string",
                description="Filter workers by status",
                required=False,
                enum=["idle", "active", "failed", "all", "stuck", "healthy"],
                default="all"
            )
        ],
        examples=[
            {"user": "Show me all workers", "tool": "list_workers", "arguments": {}},
            {"user": "Show idle workers", "tool": "list_workers", "arguments": {"filter": "idle"}},
            {"user": "Which workers are failing?", "tool": "list_workers", "arguments": {"filter": "failed"}},
        ]
    ),
    ToolDefinition(
        name="restart_worker",
        description="Restart a worker (kills and respawns with same configuration). Use this when a worker is hung or behaving oddly.",
        category=ToolCategory.WORKER_MANAGEMENT,
        parameters=[
            ToolParameter(
                name="worker_id",
                type="string",
                description="Worker identifier to restart",
                required=True
            )
        ],
        confirmation_message="Restart worker {worker_id}?",
        examples=[
            {"user": "Restart worker sonnet-beta", "tool": "restart_worker", "arguments": {"worker_id": "sonnet-beta"}},
            {"user": "Restart the hung worker", "tool": "restart_worker", "arguments": {"worker_id": "auto"}},
        ]
    ),
]

# Task Management Tools
TASK_MANAGEMENT_TOOLS = [
    ToolDefinition(
        name="filter_tasks",
        description="Filter the task queue display by priority, status, or labels. Use this to help the user find specific tasks.",
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
                enum=["open", "in_progress", "blocked", "completed", "deferred"]
            ),
            ToolParameter(
                name="labels",
                type="array",
                description="Filter by label(s)",
                required=False,
                items={"type": "string"}
            )
        ],
        examples=[
            {"user": "Show only P0 tasks", "tool": "filter_tasks", "arguments": {"priority": "P0"}},
            {"user": "Show me blocked tasks", "tool": "filter_tasks", "arguments": {"status": "blocked"}},
            {"user": "Show P1 tasks that are in progress", "tool": "filter_tasks", "arguments": {"priority": "P1", "status": "in_progress"}},
        ]
    ),
    ToolDefinition(
        name="create_task",
        description="Create a new task (bead). Use this when the user wants to add a new task to the queue.",
        category=ToolCategory.TASK_MANAGEMENT,
        parameters=[
            ToolParameter(
                name="title",
                type="string",
                description="Task title (brief description)",
                required=True
            ),
            ToolParameter(
                name="priority",
                type="string",
                description="Task priority (P0=critical, P4=backlog)",
                required=True,
                enum=["P0", "P1", "P2", "P3", "P4"]
            ),
            ToolParameter(
                name="description",
                type="string",
                description="Detailed task description",
                required=False
            )
        ],
        examples=[
            {"user": "Create a P1 task to fix the login bug", "tool": "create_task", "arguments": {"title": "Fix login bug", "priority": "P1"}},
            {"user": "Add a P0 task: investigate trading halt failures", "tool": "create_task", "arguments": {"title": "Investigate halt failures", "priority": "P0", "description": "..."}},
        ]
    ),
    ToolDefinition(
        name="assign_task",
        description="Assign a task to a specific worker or let the system auto-assign. Use this when the user wants to manually assign or reassign a task.",
        category=ToolCategory.TASK_MANAGEMENT,
        parameters=[
            ToolParameter(
                name="task_id",
                type="string",
                description="Task/bead ID (e.g., 'bd-abc') or 'auto' to pick the top task",
                required=True
            ),
            ToolParameter(
                name="worker_id",
                type="string",
                description="Worker ID (e.g., 'sonnet-alpha') or 'auto' for automatic assignment",
                required=False
            )
        ],
        examples=[
            {"user": "Assign bd-abc to sonnet-alpha", "tool": "assign_task", "arguments": {"task_id": "bd-abc", "worker_id": "sonnet-alpha"}},
            {"user": "Assign the top task to the best worker", "tool": "assign_task", "arguments": {"task_id": "auto", "worker_id": "auto"}},
        ]
    ),
]

# Cost & Analytics Tools
COST_ANALYTICS_TOOLS = [
    ToolDefinition(
        name="show_costs",
        description="Display cost analysis for a time period with optional breakdown. Use this when the user asks about spending or costs.",
        category=ToolCategory.COST_ANALYTICS,
        parameters=[
            ToolParameter(
                name="period",
                type="string",
                description="Time period for cost analysis",
                required=False,
                enum=["today", "yesterday", "this_week", "last_week", "this_month", "last_month", "all"],
                default="today"
            ),
            ToolParameter(
                name="breakdown",
                type="string",
                description="Breakdown costs by category",
                required=False,
                enum=["by_model", "by_worker", "by_task", "by_workspace", "by_date"]
            )
        ],
        examples=[
            {"user": "What did I spend today?", "tool": "show_costs", "arguments": {"period": "today"}},
            {"user": "Show me last month's costs by model", "tool": "show_costs", "arguments": {"period": "last_month", "breakdown": "by_model"}},
            {"user": "How much am I spending?", "tool": "show_costs", "arguments": {"period": "today"}},
        ]
    ),
    ToolDefinition(
        name="optimize_routing",
        description="Run cost optimization analysis and update routing rules. Use this when the user wants to reduce costs or optimize worker selection.",
        category=ToolCategory.COST_ANALYTICS,
        parameters=[],
        requires_confirmation=True,
        confirmation_message="Apply cost optimization recommendations?",
        examples=[
            {"user": "Optimize my costs", "tool": "optimize_routing", "arguments": {}},
            {"user": "How can I save money?", "tool": "optimize_routing", "arguments": {}},
        ]
    ),
    ToolDefinition(
        name="forecast_costs",
        description="Forecast future costs based on current usage patterns. Use this when the user asks about future spending.",
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
        ],
        examples=[
            {"user": "What will I spend next month?", "tool": "forecast_costs", "arguments": {"days": 30}},
            {"user": "Project my costs for 2 weeks", "tool": "forecast_costs", "arguments": {"days": 14}},
        ]
    ),
    ToolDefinition(
        name="show_metrics",
        description="Display performance metrics like throughput, latency, or success rate. Use this when the user asks about performance.",
        category=ToolCategory.COST_ANALYTICS,
        parameters=[
            ToolParameter(
                name="metric_type",
                type="string",
                description="Type of metric to display",
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
        ],
        examples=[
            {"user": "Show me performance metrics", "tool": "show_metrics", "arguments": {"metric_type": "all"}},
            {"user": "What's my task throughput today?", "tool": "show_metrics", "arguments": {"metric_type": "throughput", "period": "today"}},
        ]
    ),
]

# Data Export Tools
DATA_EXPORT_TOOLS = [
    ToolDefinition(
        name="export_logs",
        description="Export activity logs to a file. Use this when the user wants to save or analyze logs.",
        category=ToolCategory.DATA_EXPORT,
        parameters=[
            ToolParameter(
                name="format",
                type="string",
                description="Output format",
                required=False,
                enum=["json", "csv", "txt"],
                default="json"
            ),
            ToolParameter(
                name="period",
                type="string",
                description="Time period for logs",
                required=False,
                enum=["today", "yesterday", "this_week", "last_week", "this_month", "last_month", "all"],
                default="today"
            )
        ],
        examples=[
            {"user": "Export today's logs as CSV", "tool": "export_logs", "arguments": {"format": "csv", "period": "today"}},
            {"user": "Save logs", "tool": "export_logs", "arguments": {}},
        ]
    ),
    ToolDefinition(
        name="export_metrics",
        description="Export metrics data to a file. Use this when the user wants to analyze metrics externally.",
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
                description="Output format",
                required=False,
                enum=["json", "csv"],
                default="json"
            )
        ],
        examples=[
            {"user": "Export performance metrics as CSV", "tool": "export_metrics", "arguments": {"metric_type": "performance", "format": "csv"}},
            {"user": "Save cost data", "tool": "export_metrics", "arguments": {"metric_type": "costs"}},
        ]
    ),
    ToolDefinition(
        name="screenshot",
        description="Take a screenshot of the current dashboard or specific panel. Use this when the user wants to save the current view.",
        category=ToolCategory.DATA_EXPORT,
        parameters=[
            ToolParameter(
                name="panel",
                type="string",
                description="Specific panel name, or 'all' for full dashboard",
                required=False,
                enum=["all", "workers", "tasks", "costs", "metrics", "logs", "chat"],
                default="all"
            )
        ],
        examples=[
            {"user": "Take a screenshot", "tool": "screenshot", "arguments": {"panel": "all"}},
            {"user": "Screenshot the cost panel", "tool": "screenshot", "arguments": {"panel": "costs"}},
        ]
    ),
]

# Configuration Tools
CONFIGURATION_TOOLS = [
    ToolDefinition(
        name="set_config",
        description="Update a configuration setting. Use this when the user wants to change FORGE behavior or preferences.",
        category=ToolCategory.CONFIGURATION,
        parameters=[
            ToolParameter(
                name="key",
                type="string",
                description="Configuration key (e.g., 'default_model', 'max_workers')",
                required=True
            ),
            ToolParameter(
                name="value",
                type="string",
                description="New configuration value (will be type-coerced)",
                required=True
            )
        ],
        requires_confirmation=True,
        confirmation_message="Set {key} = {value}?",
        examples=[
            {"user": "Set default model to sonnet", "tool": "set_config", "arguments": {"key": "default_model", "value": "sonnet"}},
            {"user": "Change max workers to 10", "tool": "set_config", "arguments": {"key": "max_workers", "value": "10"}},
            {"user": "Enable debug mode", "tool": "set_config", "arguments": {"key": "debug_mode", "value": "true"}},
        ]
    ),
    ToolDefinition(
        name="get_config",
        description="View configuration settings. Use this when the user asks about current configuration.",
        category=ToolCategory.CONFIGURATION,
        parameters=[
            ToolParameter(
                name="key",
                type="string",
                description="Specific config key, or omit for all settings",
                required=False
            )
        ],
        examples=[
            {"user": "What's my current config?", "tool": "get_config", "arguments": {}},
            {"user": "What's the default model?", "tool": "get_config", "arguments": {"key": "default_model"}},
        ]
    ),
    ToolDefinition(
        name="save_layout",
        description="Save the current dashboard layout as a named preset. Use this when the user wants to remember a view configuration.",
        category=ToolCategory.CONFIGURATION,
        parameters=[
            ToolParameter(
                name="name",
                type="string",
                description="Layout name",
                required=True
            )
        ],
        examples=[
            {"user": "Save this layout as 'monitoring'", "tool": "save_layout", "arguments": {"name": "monitoring"}},
            {"user": "Remember this view", "tool": "save_layout", "arguments": {"name": "default"}},
        ]
    ),
    ToolDefinition(
        name="load_layout",
        description="Load a saved dashboard layout. Use this when the user wants to switch to a previously saved view.",
        category=ToolCategory.CONFIGURATION,
        parameters=[
            ToolParameter(
                name="name",
                type="string",
                description="Layout name to load",
                required=True
            )
        ],
        examples=[
            {"user": "Load my monitoring layout", "tool": "load_layout", "arguments": {"name": "monitoring"}},
            {"user": "Switch to default view", "tool": "load_layout", "arguments": {"name": "default"}},
        ]
    ),
]

# Help & Discovery Tools
HELP_DISCOVERY_TOOLS = [
    ToolDefinition(
        name="help",
        description="Get help on a specific topic or general usage. Use this when the user asks for help or how to do something.",
        category=ToolCategory.HELP_DISCOVERY,
        parameters=[
            ToolParameter(
                name="topic",
                type="string",
                description="Help topic (e.g., 'spawning', 'costs', 'tasks', 'tools')",
                required=False,
                enum=["spawning", "costs", "tasks", "tools", "workers", "configuration", "keyboard", "all"]
            )
        ],
        examples=[
            {"user": "How do I spawn workers?", "tool": "help", "arguments": {"topic": "spawning"}},
            {"user": "Help with cost optimization", "tool": "help", "arguments": {"topic": "costs"}},
            {"user": "What can you do?", "tool": "help", "arguments": {}},
        ]
    ),
    ToolDefinition(
        name="search_docs",
        description="Search documentation for a query. Use this when the user asks about specific features or concepts.",
        category=ToolCategory.HELP_DISCOVERY,
        parameters=[
            ToolParameter(
                name="query",
                type="string",
                description="Search query",
                required=True
            )
        ],
        examples=[
            {"user": "How does cost optimization work?", "tool": "search_docs", "arguments": {"query": "cost optimization"}},
            {"user": "Find info about task scoring", "tool": "search_docs", "arguments": {"query": "task scoring"}},
        ]
    ),
    ToolDefinition(
        name="list_capabilities",
        description="List all available tools and features. Use this when the user asks what FORGE can do.",
        category=ToolCategory.HELP_DISCOVERY,
        parameters=[],
        examples=[
            {"user": "What can you do?", "tool": "list_capabilities", "arguments": {}},
            {"user": "Show me all commands", "tool": "list_capabilities", "arguments": {}},
        ]
    ),
]


# =============================================================================
# Tool Registry
# =============================================================================

# All tools combined
ALL_TOOLS = (
    VIEW_CONTROL_TOOLS +
    WORKER_MANAGEMENT_TOOLS +
    TASK_MANAGEMENT_TOOLS +
    COST_ANALYTICS_TOOLS +
    DATA_EXPORT_TOOLS +
    CONFIGURATION_TOOLS +
    HELP_DISCOVERY_TOOLS
)

# Tool lookup by name
TOOL_INDEX: dict[str, ToolDefinition] = {tool.name: tool for tool in ALL_TOOLS}

# Tools by category
TOOLS_BY_CATEGORY: dict[ToolCategory, list[ToolDefinition]] = {
    ToolCategory.VIEW_CONTROL: VIEW_CONTROL_TOOLS,
    ToolCategory.WORKER_MANAGEMENT: WORKER_MANAGEMENT_TOOLS,
    ToolCategory.TASK_MANAGEMENT: TASK_MANAGEMENT_TOOLS,
    ToolCategory.COST_ANALYTICS: COST_ANALYTICS_TOOLS,
    ToolCategory.DATA_EXPORT: DATA_EXPORT_TOOLS,
    ToolCategory.CONFIGURATION: CONFIGURATION_TOOLS,
    ToolCategory.HELP_DISCOVERY: HELP_DISCOVERY_TOOLS,
}


# =============================================================================
# Export Functions
# =============================================================================

def get_all_tools() -> list[ToolDefinition]:
    """Get all tool definitions."""
    return ALL_TOOLS.copy()


def get_tool(name: str) -> ToolDefinition | None:
    """Get a tool definition by name."""
    return TOOL_INDEX.get(name)


def get_tools_by_category(category: ToolCategory) -> list[ToolDefinition]:
    """Get all tools in a specific category."""
    return TOOLS_BY_CATEGORY.get(category, []).copy()


def get_tools_for_llm(
    format: Literal["openai", "anthropic"] = "openai",
    category: ToolCategory | None = None
) -> list[dict[str, Any]]:
    """
    Get tool definitions formatted for LLM consumption.

    Args:
        format: Schema format ("openai" or "anthropic")
        category: Optional category filter

    Returns:
        List of tool definitions in the specified format
    """
    tools = get_all_tools() if category is None else get_tools_by_category(category)

    if format == "openai":
        return [tool.to_openai_schema() for tool in tools]
    elif format == "anthropic":
        return [tool.to_anthropic_schema() for tool in tools]
    else:
        raise ValueError(f"Unknown format: {format}")


def export_tools_json(
    category: ToolCategory | None = None,
    format: Literal["openai", "anthropic"] = "openai",
    indent: int = 2
) -> str:
    """
    Export tools as JSON string.

    Args:
        category: Optional category filter
        format: Schema format ("openai" or "anthropic")
        indent: JSON indentation

    Returns:
        JSON string of tool definitions
    """
    tools = get_tools_for_llm(format=format, category=category)
    return json.dumps(tools, indent=indent)


def export_tools_dict(
    category: ToolCategory | None = None
) -> list[dict[str, Any]]:
    """
    Export tools as dictionaries (for YAML serialization).

    Args:
        category: Optional category filter

    Returns:
        List of tool dictionaries
    """
    tools = get_all_tools() if category is None else get_tools_by_category(category)
    return [tool.to_yaml_dict() for tool in tools]


def count_tools() -> dict[str, int]:
    """Get tool counts by category and total."""
    return {
        "total": len(ALL_TOOLS),
        **{cat.value: len(tools) for cat, tools in TOOLS_BY_CATEGORY.items()}
    }


# =============================================================================
# Tool Injection for Chat Backend
# =============================================================================

def create_init_message(
    format: Literal["openai", "anthropic"] = "openai",
    include_context: bool = True
) -> dict[str, Any]:
    """
    Create the initialization message for the chat backend.

    This message is sent to the chat backend on startup to provide
    tool definitions.

    Args:
        format: Schema format ("openai" or "anthropic")
        include_context: Whether to include context fields

    Returns:
        Initialization message dictionary
    """
    message = {
        "type": "init",
        "tools": get_tools_for_llm(format=format),
    }

    if include_context:
        message["forge_version"] = "0.1.0"
        message["protocol_version"] = "1.0"
        message["tool_count"] = len(ALL_TOOLS)

    return message


def create_chat_message(
    user_message: str,
    context: dict[str, Any] | None = None,
    format: Literal["openai", "anthropic"] = "openai"
) -> dict[str, Any]:
    """
    Create a chat message for the chat backend.

    Args:
        user_message: The user's natural language message
        context: Optional context (current view, visible data, etc.)
        format: Schema format for tools

    Returns:
        Chat message dictionary
    """
    message = {
        "type": "message",
        "message": user_message,
        "tools": get_tools_for_llm(format=format),
    }

    if context:
        message["context"] = context

    return message


def create_telemetry_message(
    event: str,
    telemetry_data: dict[str, Any],
    format: Literal["openai", "anthropic"] = "openai"
) -> dict[str, Any]:
    """
    Create a telemetry message for autonomous analysis.

    Args:
        event: Event type (e.g., "workers_failing", "budget_alert")
        telemetry_data: Telemetry context
        format: Schema format for tools

    Returns:
        Telemetry message dictionary
    """
    return {
        "type": "telemetry",
        "event": event,
        "message": f"Telemetry: {event}",
        "context": telemetry_data,
        "tools": get_tools_for_llm(format=format),
    }


# =============================================================================
# CLI Helpers
# =============================================================================

def print_tool_summary() -> None:
    """Print a summary of available tools."""
    counts = count_tools()
    print(f"FORGE Tool Catalog")
    print(f"=" * 40)
    print(f"Total Tools: {counts['total']}")
    print()
    for cat, count in counts.items():
        if cat != "total":
            print(f"  {cat}: {count}")


if __name__ == "__main__":
    # CLI entry point for testing
    import sys

    if len(sys.argv) > 1:
        command = sys.argv[1]

        if command == "summary":
            print_tool_summary()
        elif command == "json":
            print(export_tools_json())
        elif command == "dict":
            import yaml
            print(yaml.dump(export_tools_dict(), default_flow_style=False))
        elif command == "init":
            import yaml
            print(yaml.dump(create_init_message(), default_flow_style=False))
        else:
            print(f"Unknown command: {command}")
            print("Usage: python tool_definitions.py [summary|json|dict|init]")
            sys.exit(1)
    else:
        print_tool_summary()
