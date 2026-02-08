"""
Tests for chat backend integration.
"""

import json
import pytest
from pathlib import Path
from unittest.mock import Mock, MagicMock, patch

from forge.chat_backend import (
    ChatBackendConfig,
    ToolCall,
    BackendResponse,
    ChatBackend,
    export_tools_to_file,
    ensure_tools_exported,
    load_config,
    create_backend_from_config,
    DEFAULT_TOOLS_FILE,
    DEFAULT_CONFIG_FILE,
)


class TestChatBackendDataStructures:
    """Tests for chat backend data structures."""

    def test_chat_backend_config_default(self) -> None:
        """Test ChatBackendConfig default values."""
        config = ChatBackendConfig()
        assert config.command == "claude-code"
        assert config.args == []
        assert config.model == "sonnet"
        assert config.timeout == 30
        assert config.max_retries == 3
        assert config.tools_file == DEFAULT_TOOLS_FILE

    def test_chat_backend_config_to_dict(self) -> None:
        """Test ChatBackendConfig.to_dict()."""
        config = ChatBackendConfig(
            command="test-cmd",
            args=["--arg1", "--arg2"],
            model="gpt4",
        )
        config_dict = config.to_dict()
        assert config_dict["command"] == "test-cmd"
        assert config_dict["args"] == ["--arg1", "--arg2"]
        assert config_dict["model"] == "gpt4"

    def test_tool_call_default(self) -> None:
        """Test ToolCall default values."""
        call = ToolCall()
        assert call.id is None
        assert call.tool == ""
        assert call.arguments == {}

    def test_tool_call_to_dict(self) -> None:
        """Test ToolCall.to_dict()."""
        call = ToolCall(
            id="call_123",
            tool="test_tool",
            arguments={"param1": "value1"}
        )
        call_dict = call.to_dict()
        assert call_dict["id"] == "call_123"
        assert call_dict["tool"] == "test_tool"
        assert call_dict["arguments"] == {"param1": "value1"}

    def test_backend_response_default(self) -> None:
        """Test BackendResponse default values."""
        response = BackendResponse()
        assert response.tool_calls == []
        assert response.message == ""
        assert response.reasoning == ""
        assert response.requires_confirmation is False
        assert response.error is None

    def test_backend_response_to_dict(self) -> None:
        """Test BackendResponse.to_dict()."""
        response = BackendResponse(
            tool_calls=[
                ToolCall(id="call_1", tool="tool1", arguments={})
            ],
            message="Test message",
            reasoning="Test reasoning",
        )
        response_dict = response.to_dict()
        assert len(response_dict["tool_calls"]) == 1
        assert response_dict["message"] == "Test message"
        assert response_dict["reasoning"] == "Test reasoning"


class TestChatBackend:
    """Tests for ChatBackend class."""

    def test_init_with_default_config(self) -> None:
        """Test ChatBackend initialization with default config."""
        backend = ChatBackend()
        assert backend.config.command == "claude-code"
        assert backend._process is None
        assert backend._initialized is False

    def test_init_with_custom_config(self) -> None:
        """Test ChatBackend initialization with custom config."""
        config = ChatBackendConfig(command="test-backend")
        backend = ChatBackend(config)
        assert backend.config.command == "test-backend"

    def test_is_running_initial_state(self) -> None:
        """Test is_running returns False initially."""
        backend = ChatBackend()
        assert backend.is_running() is False

    @pytest.mark.skipif(True, reason="Requires subprocess mocking")
    def test_start_stop_backend(self) -> None:
        """Test starting and stopping the backend subprocess."""
        # This test would require mocking subprocess.Popen
        # Skipping for now as it's complex to test
        pass

    def test_send_init_without_process(self) -> None:
        """Test send_init raises error when no process."""
        backend = ChatBackend()
        with pytest.raises(RuntimeError, match="Chat backend not running"):
            backend.send_init()

    def test_send_message_without_process(self) -> None:
        """Test send_message raises error when no process."""
        backend = ChatBackend()
        with pytest.raises(RuntimeError, match="Chat backend not running"):
            backend.send_message("test message")

    def test_send_telemetry_without_process(self) -> None:
        """Test send_telemetry raises error when no process."""
        backend = ChatBackend()
        with pytest.raises(RuntimeError, match="Chat backend not running"):
            backend.send_telemetry("test_event", {})

    def test_context_manager(self) -> None:
        """Test ChatBackend as context manager."""
        # Note: This will fail to actually start, but tests the interface
        with pytest.raises(RuntimeError):
            with ChatBackend() as backend:
                backend.is_running()


class TestToolExport:
    """Tests for tool export functions."""

    def test_export_tools_to_file(self, tmp_path: Path) -> None:
        """Test export_tools_to_file creates valid JSON file."""
        output_file = tmp_path / "test_tools.json"

        result_path = export_tools_to_file(path=output_file)
        assert result_path == output_file
        assert output_file.exists()

        # Verify JSON is valid
        content = json.loads(output_file.read_text())
        assert isinstance(content, list)
        assert len(content) >= 30

    def test_export_tools_to_file_default_path(self) -> None:
        """Test export_tools_to_file with default path."""
        # Use a temp directory for HOME
        with patch("pathlib.Path.home", return_value=Path("/tmp/test_home")):
            result_path = export_tools_to_file()
            assert result_path == Path("/tmp/test_home/.forge/tools.json")

    def test_export_tools_to_file_with_category(self, tmp_path: Path) -> None:
        """Test export_tools_to_file with category filter."""
        from forge.tool_definitions import ToolCategory

        output_file = tmp_path / "test_tools_view_only.json"

        result_path = export_tools_to_file(
            path=output_file,
            category="view_control"
        )
        assert result_path == output_file

        # Verify only view control tools are exported
        content = json.loads(output_file.read_text())
        assert len(content) > 0
        # All tools should be view control
        for tool in content:
            func = tool["function"]
            # View control tools: switch_view, split_view, focus_panel
            assert func["name"] in ["switch_view", "split_view", "focus_panel"]

    def test_ensure_tools_exported(self) -> None:
        """Test ensure_tools_exported creates tools file."""
        with patch("pathlib.Path.home", return_value=Path("/tmp/test_home")):
            result_path = ensure_tools_exported()
            assert result_path == Path("/tmp/test_home/.forge/tools.json")


class TestConfigLoading:
    """Tests for configuration loading."""

    def test_load_config_default(self) -> None:
        """Test load_config with non-existent file returns defaults."""
        with patch("pathlib.Path.exists", return_value=False):
            config = load_config()
            assert config.command == "claude-code"
            assert config.model == "sonnet"

    def test_load_config_from_file(self, tmp_path: Path) -> None:
        """Test load_config reads from existing file."""
        import yaml

        config_file = tmp_path / "config.yaml"
        config_data = {
            "chat_backend": {
                "command": "test-backend",
                "model": "gpt4",
                "args": ["--test"],
                "timeout": 60,
            }
        }
        config_file.write_text(yaml.dump(config_data))

        config = load_config(config_file)
        assert config.command == "test-backend"
        assert config.model == "gpt4"
        assert config.args == ["--test"]
        assert config.timeout == 60

    def test_create_backend_from_config(self) -> None:
        """Test create_backend_from_config creates backend."""
        with patch("forge.chat_backend.load_config") as mock_load:
            mock_config = ChatBackendConfig(command="test-cmd")
            mock_load.return_value = mock_config

            backend = create_backend_from_config()
            assert backend.config.command == "test-cmd"


class TestToolInjection:
    """Tests for tool injection functionality."""

    def test_init_message_structure(self) -> None:
        """Test that init message has correct structure."""
        from forge.chat_backend import create_init_message

        msg = create_init_message()
        assert msg["type"] == "init"
        assert "tools" in msg
        assert isinstance(msg["tools"], list)
        assert len(msg["tools"]) >= 30
        assert "forge_version" in msg
        assert "protocol_version" in msg
        assert "tool_count" in msg

    def test_chat_message_structure(self) -> None:
        """Test that chat message has correct structure."""
        from forge.chat_backend import create_chat_message

        msg = create_chat_message("test message")
        assert msg["type"] == "message"
        assert msg["message"] == "test message"
        assert "tools" in msg
        assert isinstance(msg["tools"], list)

    def test_chat_message_with_context(self) -> None:
        """Test chat message includes context."""
        from forge.chat_backend import create_chat_message

        context = {"current_view": "workers"}
        msg = create_chat_message("test", context=context)
        assert msg["context"] == context

    def test_telemetry_message_structure(self) -> None:
        """Test that telemetry message has correct structure."""
        from forge.chat_backend import create_telemetry_message

        msg = create_telemetry_message("test_event", {"data": "value"})
        assert msg["type"] == "telemetry"
        assert msg["event"] == "test_event"
        assert msg["context"] == {"data": "value"}
        assert "tools" in msg


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
