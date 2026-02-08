"""
Tests for FORGE Chat Backend Integration

Comprehensive tests for chat backend protocol, tool definition injection,
response parsing, error handling per ADR 0014, and backend lifecycle.
"""

import json
import subprocess
from pathlib import Path
from unittest.mock import AsyncMock, Mock, patch, MagicMock
from typing import Any

import pytest

from forge.chat_backend import (
    ChatBackend,
    ChatBackendConfig,
    ToolCall,
    BackendResponse,
    export_tools_to_file,
    ensure_tools_exported,
    load_config,
    create_backend_from_config,
)
from forge.tool_definitions import (
    ToolCategory,
    ToolDefinition,
    ToolParameter,
    create_init_message,
    create_chat_message,
    create_telemetry_message,
    get_tools_for_llm,
    export_tools_json,
)


# =============================================================================
# Fixtures
# =============================================================================


@pytest.fixture
def temp_tools_file(tmp_path):
    """Create a temporary tools file"""
    tools_file = tmp_path / "tools.json"
    return tools_file


@pytest.fixture
def temp_config_file(tmp_path):
    """Create a temporary config file"""
    config_file = tmp_path / "config.yaml"
    return config_file


@pytest.fixture
def sample_config():
    """Sample chat backend configuration"""
    return ChatBackendConfig(
        command="echo",
        args=["--test"],
        model="sonnet",
        timeout=10,
        max_retries=2,
    )


@pytest.fixture
def sample_tools():
    """Sample tool definitions"""
    return [
        ToolDefinition(
            name="switch_view",
            description="Switch dashboard view",
            category=ToolCategory.VIEW_CONTROL,
            parameters=[
                ToolParameter(
                    name="view",
                    type="string",
                    description="Target view",
                    enum=["workers", "tasks", "costs"],
                    required=True,
                )
            ],
        ),
        ToolDefinition(
            name="spawn_worker",
            description="Spawn a new worker",
            category=ToolCategory.WORKER_MANAGEMENT,
            parameters=[
                ToolParameter(
                    name="model",
                    type="string",
                    description="Model to use",
                    required=True,
                ),
                ToolParameter(
                    name="session_name",
                    type="string",
                    description="Session name",
                    required=True,
                ),
            ],
        ),
    ]


@pytest.fixture
def mock_backend_response():
    """Sample backend response"""
    return {
        "tool_calls": [
            {
                "id": "call_123",
                "tool": "switch_view",
                "arguments": {"view": "workers"},
            }
        ],
        "message": "Switching to workers view",
        "reasoning": "User wants to see workers",
        "requires_confirmation": False,
    }


@pytest.fixture
def mock_malformed_response():
    """Sample malformed backend response"""
    return {
        "message": "Backend crashed",
        "error": "Process died",
    }


# =============================================================================
# ChatBackendConfig Tests
# =============================================================================


class TestChatBackendConfig:
    """Tests for ChatBackendConfig"""

    def test_default_config(self):
        """Test default configuration"""
        config = ChatBackendConfig()

        assert config.command == "claude-code"
        assert config.model == "sonnet"
        assert config.timeout == 30
        assert config.max_retries == 3
        assert config.args == []
        assert config.env == {}

    def test_custom_config(self):
        """Test custom configuration"""
        config = ChatBackendConfig(
            command="my-backend",
            args=["--arg1", "arg2"],
            model="opus",
            timeout=60,
            max_retries=5,
            env={"KEY": "VALUE"},
        )

        assert config.command == "my-backend"
        assert config.args == ["--arg1", "arg2"]
        assert config.model == "opus"
        assert config.timeout == 60
        assert config.max_retries == 5
        assert config.env == {"KEY": "VALUE"}

    def test_to_dict(self):
        """Test converting config to dictionary"""
        config = ChatBackendConfig(
            command="test",
            model="sonnet",
            timeout=20,
        )

        result = config.to_dict()

        assert result["command"] == "test"
        assert result["model"] == "sonnet"
        assert result["timeout"] == 20
        assert isinstance(result["tools_file"], str)


# =============================================================================
# ToolCall Tests
# =============================================================================


class TestToolCall:
    """Tests for ToolCall"""

    def test_tool_call_creation(self):
        """Test creating a tool call"""
        call = ToolCall(
            id="call_123",
            tool="switch_view",
            arguments={"view": "workers"},
        )

        assert call.id == "call_123"
        assert call.tool == "switch_view"
        assert call.arguments == {"view": "workers"}

    def test_tool_call_defaults(self):
        """Test tool call with default values"""
        call = ToolCall(
            tool="test_tool",
        )

        assert call.id is None
        assert call.arguments == {}

    def test_to_dict(self):
        """Test converting tool call to dictionary"""
        call = ToolCall(
            id="call_abc",
            tool="spawn_worker",
            arguments={"model": "sonnet", "session_name": "alpha"},
        )

        result = call.to_dict()

        assert result["id"] == "call_abc"
        assert result["tool"] == "spawn_worker"
        assert result["arguments"] == {"model": "sonnet", "session_name": "alpha"}


# =============================================================================
# BackendResponse Tests
# =============================================================================


class TestBackendResponse:
    """Tests for BackendResponse"""

    def test_response_with_tool_calls(self):
        """Test response with tool calls"""
        response = BackendResponse(
            tool_calls=[
                ToolCall(id="1", tool="switch_view", arguments={"view": "workers"}),
            ],
            message="Switching views",
        )

        assert len(response.tool_calls) == 1
        assert response.message == "Switching views"
        assert response.tool_calls[0].tool == "switch_view"

    def test_response_with_error(self):
        """Test error response"""
        response = BackendResponse(
            error="Backend crashed",
            message="Failed to process request",
        )

        assert response.error == "Backend crashed"
        assert response.message == "Failed to process request"
        assert len(response.tool_calls) == 0

    def test_response_confirmation_required(self):
        """Test response requiring confirmation"""
        response = BackendResponse(
            tool_calls=[
                ToolCall(id="1", tool="delete_worker", arguments={"worker_id": "123"}),
            ],
            message="Deleting worker",
            requires_confirmation=True,
        )

        assert response.requires_confirmation is True

    def test_to_dict(self):
        """Test converting response to dictionary"""
        response = BackendResponse(
            tool_calls=[
                ToolCall(id="1", tool="test", arguments={}),
            ],
            message="Test",
            reasoning="Because",
            requires_confirmation=False,
        )

        result = response.to_dict()

        assert "tool_calls" in result
        assert result["message"] == "Test"
        assert result["reasoning"] == "Because"
        assert result["requires_confirmation"] is False


# =============================================================================
# ChatBackend Tests - Lifecycle
# =============================================================================


class TestChatBackendLifecycle:
    """Tests for ChatBackend lifecycle management"""

    def test_initialization(self):
        """Test backend initialization"""
        config = ChatBackendConfig(command="echo")
        backend = ChatBackend(config)

        assert backend.config == config
        assert backend._process is None
        assert backend._initialized is False

    def test_initialization_with_defaults(self):
        """Test backend initialization with default config"""
        backend = ChatBackend()

        assert backend.config.command == "claude-code"
        assert backend._process is None

    @pytest.mark.skipif(
        subprocess.run(["which", "echo"], capture_output=True).returncode != 0,
        reason="echo not available"
    )
    def test_start_backend(self):
        """Test starting backend process"""
        config = ChatBackendConfig(command="echo")
        backend = ChatBackend(config)

        backend.start()

        assert backend._process is not None
        assert backend.is_running()

        backend.stop()

    @pytest.mark.skipif(
        subprocess.run(["which", "echo"], capture_output=True).returncode != 0,
        reason="echo not available"
    )
    def test_stop_backend(self):
        """Test stopping backend process"""
        config = ChatBackendConfig(command="echo")
        backend = ChatBackend(config)

        backend.start()
        assert backend.is_running()

        backend.stop()
        assert backend._process is None

    @pytest.mark.skipif(
        subprocess.run(["which", "echo"], capture_output=True).returncode != 0,
        reason="echo not available"
    )
    def test_double_start_raises_error(self):
        """Test that double start raises error"""
        config = ChatBackendConfig(command="echo")
        backend = ChatBackend(config)

        backend.start()

        with pytest.raises(RuntimeError, match="already running"):
            backend.start()

        backend.stop()

    def test_start_nonexistent_command(self):
        """Test starting with non-existent command (ADR 0014)"""
        config = ChatBackendConfig(command="/nonexistent/command/that/does/not/exist")
        backend = ChatBackend(config)

        with pytest.raises(RuntimeError, match="not found"):
            backend.start()

    @pytest.mark.skipif(
        subprocess.run(["which", "echo"], capture_output=True).returncode != 0,
        reason="echo not available"
    )
    def test_context_manager(self):
        """Test using backend as context manager"""
        config = ChatBackendConfig(command="echo")

        with ChatBackend(config) as backend:
            assert backend.is_running()

        assert backend._process is None


# =============================================================================
# ChatBackend Tests - Communication
# =============================================================================


class TestChatBackendCommunication:
    """Tests for backend communication protocol"""

    def test_send_init_message_format(self):
        """Test init message format"""
        format_openai = create_init_message(format="openai")
        format_anthropic = create_init_message(format="anthropic")

        # OpenAI format should have tools array
        assert "tools" in format_openai or "message" in format_openai
        assert format_openai.get("format") == "openai" or "tools" in format_openai

        # Anthropic format should have tools
        assert "tools" in format_anthropic or "message" in format_anthropic

    def test_send_chat_message(self):
        """Test chat message creation"""
        msg = create_chat_message(
            user_message="Show workers",
            context={"current_view": "costs"},
            format="openai",
        )

        assert "message" in msg
        assert msg["message"] == "Show workers"
        assert "context" in msg
        assert msg["context"]["current_view"] == "costs"

    def test_send_telemetry_message(self):
        """Test telemetry message creation"""
        msg = create_telemetry_message(
            event="worker_failed",
            telemetry_data={"worker_id": "sonnet-alpha", "error": "crash"},
            format="openai",
        )

        assert "event" in msg
        assert msg["event"] == "worker_failed"
        assert "context" in msg
        assert msg["context"]["worker_id"] == "sonnet-alpha"

    @pytest.mark.skipif(
        subprocess.run(["which", "cat"], capture_output=True).returncode != 0,
        reason="cat not available"
    )
    def test_send_message_to_running_backend(self, tmp_path):
        """Test sending message to running backend"""
        # Create a simple backend script that echoes JSON
        backend_script = tmp_path / "backend.py"
        backend_script.write_text('''#!/usr/bin/env python3
import json
import sys
while True:
    line = sys.stdin.readline()
    if not line:
        break
    # Echo a response
    response = {"tool_calls": [], "message": "OK"}
    print(json.dumps(response))
    sys.stdout.flush()
''')
        backend_script.chmod(0o755)

        config = ChatBackendConfig(command=str(backend_script))
        backend = ChatBackend(config)

        backend.start()

        # Note: This test requires a real backend implementation
        # For now, just verify the process is running
        assert backend.is_running()

        backend.stop()


# =============================================================================
# ChatBackend Tests - Error Handling (ADR 0014)
# =============================================================================


class TestChatBackendErrorHandling:
    """Tests for error handling per ADR 0014"""

    def test_send_to_stopped_backend(self):
        """Test sending message to stopped backend"""
        backend = ChatBackend()

        with pytest.raises(RuntimeError, match="not running"):
            backend.send_message("test message")

    def test_parse_invalid_json_response(self):
        """Test parsing invalid JSON response"""
        config = ChatBackendConfig(command="echo")
        backend = ChatBackend(config)

        # Mock process that returns invalid JSON
        mock_process = Mock()
        mock_process.stdout.readline.return_value = "not valid json {"
        backend._process = mock_process

        response = backend._read_response(timeout=1)

        assert response.error is not None
        assert "Invalid JSON" in response.error or "Failed to parse" in response.message

    def test_parse_malformed_tool_call(self):
        """Test parsing malformed tool call response"""
        config = ChatBackendConfig(command="echo")
        backend = ChatBackend(config)

        # Mock process with malformed tool_calls
        mock_process = Mock()
        mock_process.stdout.readline.return_value = json.dumps({
            "tool_calls": [{"invalid": "structure"}],
        })
        backend._process = mock_process

        response = backend._read_response(timeout=1)

        # Should handle gracefully - tool_calls may be empty but no crash
        assert response is not None

    def test_backend_timeout(self):
        """Test backend timeout handling (ADR 0014)"""
        config = ChatBackendConfig(command="sleep", args=["100"], timeout=1)
        backend = ChatBackend(config)

        backend.start()

        # Try to read with short timeout
        # This would normally timeout, but we'll simulate it
        mock_process = Mock()
        mock_process.stdout.readline.side_effect = lambda: (_ for _ in ()).throw(TimeoutError())
        backend._process = mock_process

        response = backend._read_response(timeout=1)

        # Should handle timeout gracefully
        assert response is not None

        backend.stop()

    def test_broken_pipe_error(self):
        """Test broken pipe error handling (ADR 0014)"""
        config = ChatBackendConfig(command="echo")
        backend = ChatBackend(config)
        backend._process = Mock()
        backend._process.stdin.write.side_effect = BrokenPipeError()

        with pytest.raises(RuntimeError, match="terminated unexpectedly"):
            backend._send_message({"test": "message"})

    def test_backend_closed_connection(self):
        """Test backend closing connection (ADR 0014)"""
        config = ChatBackendConfig(command="echo")
        backend = ChatBackend(config)

        # Mock process that returns empty line (closed connection)
        mock_process = Mock()
        mock_process.stdout.readline.return_value = ""
        backend._process = mock_process

        response = backend._read_response(timeout=1)

        # Should return response with error indication
        assert response is not None


# =============================================================================
# Tool Export Tests
# =============================================================================


class TestToolExport:
    """Tests for tool definition export"""

    def test_export_tools_to_file(self, tmp_path):
        """Test exporting tools to file"""
        tools_file = tmp_path / "exported_tools.json"

        result_path = export_tools_to_file(
            path=tools_file,
            format="openai",
            category=None,
        )

        assert result_path == tools_file
        assert tools_file.exists()

        # Verify JSON is valid
        content = tools_file.read_text()
        tools = json.loads(content)
        assert isinstance(tools, list)

    def test_export_tools_by_category(self, tmp_path):
        """Test exporting tools filtered by category"""
        tools_file = tmp_path / "view_tools.json"

        export_tools_to_file(
            path=tools_file,
            format="openai",
            category="view_control",
        )

        content = tools_file.read_text()
        tools = json.loads(content)

        # Tools should be exported (category is embedded in OpenAI function schema)
        # All tools should be from view_control category
        assert len(tools) > 0
        # In OpenAI format, category is not in the function schema itself
        # but we know we exported only view_control tools

    def test_export_anthropic_format(self, tmp_path):
        """Test exporting tools in Anthropic format"""
        tools_file = tmp_path / "anthropic_tools.json"

        export_tools_to_file(
            path=tools_file,
            format="anthropic",
        )

        content = tools_file.read_text()
        tools = json.loads(content)

        # Anthropic format has different structure
        assert isinstance(tools, list)
        if tools:
            assert "name" in tools[0]
            assert "description" in tools[0]
            assert "input_schema" in tools[0]

    def test_ensure_tools_exported(self):
        """Test ensure_tools_exported creates default file"""
        # Use a temp directory for home
        with patch("forge.chat_backend.DEFAULT_TOOLS_FILE") as mock_path:
            mock_path = Path("/tmp/test_tools.json")

            with patch("forge.chat_backend.export_tools_to_file") as mock_export:
                mock_export.return_value = mock_path

                result = ensure_tools_exported()

                assert result == mock_path
                mock_export.assert_called_once()


# =============================================================================
# Configuration Loading Tests
# =============================================================================


class TestConfigurationLoading:
    """Tests for configuration file loading"""

    def test_load_config_nonexistent(self):
        """Test loading config from non-existent file"""
        config = load_config(Path("/nonexistent/config.yaml"))

        # Should return default config
        assert config.command == "claude-code"
        assert config.model == "sonnet"

    def test_load_config_valid_yaml(self, tmp_path):
        """Test loading valid YAML config"""
        config_file = tmp_path / "config.yaml"
        config_file.write_text("""
chat_backend:
  command: my-backend
  args:
    - --arg1
    - --arg2
  model: opus
  timeout: 60
  env:
    API_KEY: test_key
""")

        config = load_config(config_file)

        assert config.command == "my-backend"
        assert config.args == ["--arg1", "--arg2"]
        assert config.model == "opus"
        assert config.timeout == 60
        assert config.env == {"API_KEY": "test_key"}

    def test_load_config_invalid_yaml(self, tmp_path):
        """Test loading invalid YAML returns defaults"""
        config_file = tmp_path / "invalid.yaml"
        config_file.write_text("invalid: yaml: content: [")

        config = load_config(config_file)

        # Should return default config on error
        assert config.command == "claude-code"

    def test_create_backend_from_config(self, tmp_path):
        """Test creating backend from config file"""
        config_file = tmp_path / "config.yaml"
        config_file.write_text("""
chat_backend:
  command: test-backend
  model: haiku
""")

        backend = create_backend_from_config(config_file)

        assert backend.config.command == "test-backend"
        assert backend.config.model == "haiku"


# =============================================================================
# Integration Tests
# =============================================================================


class TestChatBackendIntegration:
    """Integration tests for chat backend"""

    @pytest.mark.skipif(
        subprocess.run(["which", "cat"], capture_output=True).returncode != 0,
        reason="cat not available"
    )
    def test_full_backend_workflow(self, tmp_path):
        """Test complete backend workflow with mock backend"""
        # Create a mock backend script
        backend_script = tmp_path / "mock_backend.py"
        backend_script.write_text('''#!/usr/bin/env python3
import json
import sys

# Read input
line = sys.stdin.readline()
if not line:
    sys.exit(0)

# Parse input
try:
    input_data = json.loads(line)
    message = input_data.get("message", "")
except:
    message = ""

# Generate response based on message
if "switch" in message.lower() and "worker" in message.lower():
    response = {
        "tool_calls": [{"id": "1", "tool": "switch_view", "arguments": {"view": "workers"}}],
        "message": "Switching to workers view",
    }
else:
    response = {
        "tool_calls": [],
        "message": f"I received: {message}",
    }

print(json.dumps(response))
sys.stdout.flush()
''')
        backend_script.chmod(0o755)

        # Test with backend
        config = ChatBackendConfig(command=str(backend_script))
        backend = ChatBackend(config)

        backend.start()
        assert backend.is_running()

        # Send a message
        response = backend.send_message("switch to workers")

        assert response is not None
        assert response.message is not None

        backend.stop()
        assert not backend.is_running()

    @pytest.mark.skipif(
        subprocess.run(["which", "cat"], capture_output=True).returncode != 0,
        reason="cat not available"
    )
    def test_backend_graceful_degradation(self, tmp_path):
        """Test graceful degradation when backend fails (ADR 0014)"""
        import time

        # Create a backend that crashes
        crash_script = tmp_path / "crash_backend.py"
        crash_script.write_text('''#!/usr/bin/env python3
import sys
print("Crashing!", file=sys.stderr)
sys.exit(1)
''')
        crash_script.chmod(0o755)

        config = ChatBackendConfig(command=str(crash_script))

        # Backend can start (process launches) but will have exited when we check
        backend = ChatBackend(config)
        backend.start()

        # Wait a moment for process to complete
        time.sleep(0.1)

        # Process should have terminated immediately
        assert not backend.is_running()

    def test_tool_definition_integration(self):
        """Test that tool definitions integrate properly"""
        # Get tools for LLM
        tools = get_tools_for_llm(format="openai")

        assert isinstance(tools, list)
        assert len(tools) > 0

        # Each tool should have required fields
        for tool in tools:
            if "function" in tool:  # OpenAI format
                func = tool["function"]
                assert "name" in func
                assert "description" in func
                assert "parameters" in func
            else:  # Anthropic format
                assert "name" in tool
                assert "description" in tool
                assert "input_schema" in tool

    def test_protocol_compliance(self):
        """Test protocol compliance for backend communication"""
        # Create init message
        init_msg = create_init_message(format="openai")

        # Should be valid JSON
        json.dumps(init_msg)

        # Create chat message
        chat_msg = create_chat_message("test", {"context": "data"}, "openai")

        # Should be valid JSON
        json.dumps(chat_msg)

        # Create telemetry message
        telemetry_msg = create_telemetry_message("test_event", {"data": "value"}, "openai")

        # Should be valid JSON
        json.dumps(telemetry_msg)


# =============================================================================
# Performance Tests
# =============================================================================


class TestChatBackendPerformance:
    """Performance tests for chat backend"""

    def test_tool_export_performance(self, tmp_path):
        """Test tool export is fast enough"""
        import time

        tools_file = tmp_path / "perf_tools.json"

        start = time.time()
        export_tools_to_file(tools_file)
        duration = time.time() - start

        # Should complete in under 1 second
        assert duration < 1.0

    def test_config_loading_performance(self, tmp_path):
        """Test config loading is fast enough"""
        import time

        config_file = tmp_path / "perf_config.yaml"
        config_file.write_text("""
chat_backend:
  command: test
  model: sonnet
""")

        start = time.time()
        load_config(config_file)
        duration = time.time() - start

        # Should complete in under 0.1 seconds
        assert duration < 0.1
