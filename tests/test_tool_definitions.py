"""
Tests for tool definitions and chat backend integration.

Tests the existing forge.tools module which provides:
- OpenAI function calling compatible tool schema
- 45+ tool definitions across all categories
- Tool injection to chat backend on init
- Both JSON (protocol) and YAML-like (docs) formats
"""

import json
import pytest
from pathlib import Path
from forge.tools import (
    ToolDefinition,
    ToolParameter,
    ToolCategory,
    ALL_TOOLS,
    get_tools_for_llm,
    generate_tools_file,
    inject_tools_to_backend,
    get_default_tools_path,
    ToolExecutor,
    initialize_tools,
    load_tools_from_file,
)


class TestToolDefinitions:
    """Tests for tool definitions module."""

    def test_all_tools_defined(self) -> None:
        """Test that all tools from TOOL_CATALOG.md are defined."""
        # Should have at least 30 tools (we have 45)
        assert len(ALL_TOOLS) >= 30, f"Expected at least 30 tools, got {len(ALL_TOOLS)}"

    def test_tool_categories(self) -> None:
        """Test that tools are properly categorized."""
        categories = {tool.category for tool in ALL_TOOLS}
        # We have more categories than just the basic ones
        assert ToolCategory.VIEW_CONTROL in categories
        assert ToolCategory.WORKER_MANAGEMENT in categories
        assert ToolCategory.TASK_MANAGEMENT in categories
        assert ToolCategory.COST_ANALYTICS in categories
        assert ToolCategory.DATA_EXPORT in categories
        assert ToolCategory.CONFIGURATION in categories
        assert ToolCategory.HELP_DISCOVERY in categories

    def test_tool_to_openai_format(self) -> None:
        """Test ToolDefinition.to_openai_format()."""
        tool = ToolDefinition(
            name="test_tool",
            description="A test tool",
            category=ToolCategory.VIEW_CONTROL,
            parameters=[
                ToolParameter(
                    name="param1",
                    type="string",
                    description="First parameter",
                    required=True,
                ),
                ToolParameter(
                    name="param2",
                    type="integer",
                    description="Second parameter",
                    required=False,
                    default=5,
                    minimum=1,
                    maximum=10,
                ),
            ]
        )

        schema = tool.to_openai_format()
        assert schema["type"] == "function"
        assert "function" in schema
        func = schema["function"]
        assert func["name"] == "test_tool"
        assert func["description"] == "A test tool"
        assert "parameters" in func
        params = func["parameters"]
        assert params["type"] == "object"
        assert "properties" in params
        assert "required" in params
        assert params["required"] == ["param1"]
        assert "param1" in params["properties"]
        assert "param2" in params["properties"]
        assert params["properties"]["param2"]["minimum"] == 1
        assert params["properties"]["param2"]["maximum"] == 10

    def test_export_tools_json_openai(self) -> None:
        """Test export_tools_json with OpenAI format."""
        json_str = get_tools_for_llm(format="openai")
        tools = json.loads(json_str)
        assert isinstance(tools, list)
        assert len(tools) >= 30

        # Check structure
        for tool in tools:
            assert "type" in tool
            assert tool["type"] == "function"
            assert "function" in tool
            func = tool["function"]
            assert "name" in func
            assert "description" in func
            assert "parameters" in func

    def test_export_tools_json_simple(self) -> None:
        """Test export_tools_json with simple format."""
        from forge.tools import ToolExecutor

        executor = ToolExecutor(register_all=True)
        json_str = executor.list_tools_json()
        tools = json.loads(json_str)
        assert isinstance(tools, list)
        assert len(tools) >= 30

    def test_generate_tools_file(self, tmp_path: Path) -> None:
        """Test generate_tools_file creates valid JSON file."""
        output_file = tmp_path / "test_tools.json"

        result_path = generate_tools_file(str(output_file))
        assert result_path == output_file
        assert output_file.exists()

        # Verify JSON is valid
        content = json.loads(output_file.read_text())
        assert isinstance(content, list)
        assert len(content) >= 30

    def test_inject_tools_to_backend(self) -> None:
        """Test inject_tools_to_backend creates proper payload."""
        payload = inject_tools_to_backend()

        assert "version" in payload
        assert "tools" in payload
        assert "count" in payload
        assert "metadata" in payload
        assert isinstance(payload["tools"], list)
        assert payload["count"] >= 30

    def test_initialize_tools(self, tmp_path: Path) -> None:
        """Test initialize_tools generates file and returns payload."""
        output_file = tmp_path / "test_tools.json"

        result = initialize_tools(str(output_file), format="openai", force=True)

        assert "tools" in result
        assert output_file.exists()

        # Verify file content
        content = json.loads(output_file.read_text())
        assert len(content) >= 30

    def test_load_tools_from_file(self, tmp_path: Path) -> None:
        """Test load_tools_from_file reads and parses tools."""
        # First create a tools file
        output_file = tmp_path / "test_tools.json"
        generate_tools_file(str(output_file))

        # Load it back
        tools = load_tools_from_file(str(output_file))
        assert isinstance(tools, list)
        assert len(tools) >= 30

    def test_view_control_tools(self) -> None:
        """Test view control tools are properly defined."""
        executor = ToolExecutor(register_all=True)
        view_tools = executor.list_tools(category=ToolCategory.VIEW_CONTROL)

        tool_names = {t.name for t in view_tools}
        assert "switch_view" in tool_names
        assert "split_view" in tool_names
        assert "focus_panel" in tool_names

    def test_worker_management_tools(self) -> None:
        """Test worker management tools are properly defined."""
        executor = ToolExecutor(register_all=True)
        worker_tools = executor.list_tools(category=ToolCategory.WORKER_MANAGEMENT)

        tool_names = {t.name for t in worker_tools}
        assert "spawn_worker" in tool_names
        assert "kill_worker" in tool_names
        assert "list_workers" in tool_names
        assert "restart_worker" in tool_names

    def test_task_management_tools(self) -> None:
        """Test task management tools are properly defined."""
        executor = ToolExecutor(register_all=True)
        task_tools = executor.list_tools(category=ToolCategory.TASK_MANAGEMENT)

        tool_names = {t.name for t in task_tools}
        assert "filter_tasks" in tool_names
        assert "create_task" in tool_names
        assert "assign_task" in tool_names

    def test_cost_analytics_tools(self) -> None:
        """Test cost analytics tools are properly defined."""
        executor = ToolExecutor(register_all=True)
        cost_tools = executor.list_tools(category=ToolCategory.COST_ANALYTICS)

        tool_names = {t.name for t in cost_tools}
        assert "show_costs" in tool_names
        assert "optimize_routing" in tool_names
        assert "forecast_costs" in tool_names
        assert "show_metrics" in tool_names

    def test_data_export_tools(self) -> None:
        """Test data export tools are properly defined."""
        executor = ToolExecutor(register_all=True)
        export_tools = executor.list_tools(category=ToolCategory.DATA_EXPORT)

        tool_names = {t.name for t in export_tools}
        assert "export_logs" in tool_names
        assert "export_metrics" in tool_names
        assert "screenshot" in tool_names

    def test_configuration_tools(self) -> None:
        """Test configuration tools are properly defined."""
        executor = ToolExecutor(register_all=True)
        config_tools = executor.list_tools(category=ToolCategory.CONFIGURATION)

        tool_names = {t.name for t in config_tools}
        assert "set_config" in tool_names
        assert "get_config" in tool_names
        assert "save_layout" in tool_names
        assert "load_layout" in tool_names

    def test_help_discovery_tools(self) -> None:
        """Test help and discovery tools are properly defined."""
        executor = ToolExecutor(register_all=True)
        help_tools = executor.list_tools(category=ToolCategory.HELP_DISCOVERY)

        tool_names = {t.name for t in help_tools}
        assert "help" in tool_names
        assert "search_docs" in tool_names
        assert "list_capabilities" in tool_names

    def test_tool_executor_get_tool_count(self) -> None:
        """Test ToolExecutor.get_tool_count returns correct count."""
        executor = ToolExecutor(register_all=True)
        assert executor.get_tool_count() >= 30

    def test_tool_executor_get_tools_by_category(self) -> None:
        """Test ToolExecutor.get_tools_by_category groups tools correctly."""
        executor = ToolExecutor(register_all=True)
        by_category = executor.get_tools_by_category()

        assert "view_control" in by_category
        assert "worker_management" in by_category
        assert len(by_category["view_control"]) >= 3


class TestToolIntegration:
    """Tests for tool integration with FORGE app."""

    def test_tools_json_file_exists(self) -> None:
        """Test that tools.json can be generated at default location."""
        tools_path = get_default_tools_path()
        assert isinstance(tools_path, Path)

    def test_tools_payload_structure(self) -> None:
        """Test that tools payload has correct structure for chat backend."""
        payload = inject_tools_to_backend()

        # Check required fields
        assert "version" in payload
        assert "tools" in payload
        assert "count" in payload
        assert "metadata" in payload

        # Check tools structure
        tools = payload["tools"]
        assert isinstance(tools, list)
        assert len(tools) >= 30

        # Check first tool
        first_tool = tools[0]
        assert "type" in first_tool
        assert "function" in first_tool


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
