"""
FORGE Chat Backend Integration

Handles communication with headless LLM chat backends.
Provides tool definitions and processes tool calls.
"""

import json
import subprocess
import sys
from pathlib import Path
from typing import Any, Literal
from dataclasses import dataclass, field

from .tool_definitions import (
    ToolDefinition,
    create_init_message,
    create_chat_message,
    create_telemetry_message,
    get_tools_for_llm,
    export_tools_json,
)


# =============================================================================
# Configuration
# =============================================================================

DEFAULT_TOOLS_FILE = Path.home() / ".forge" / "tools.json"
DEFAULT_CONFIG_FILE = Path.home() / ".forge" / "config.yaml"


# =============================================================================
# Data Structures
# =============================================================================

@dataclass
class ChatBackendConfig:
    """Configuration for chat backend integration."""
    command: str = "claude-code"
    args: list[str] = field(default_factory=list)
    model: str = "sonnet"
    env: dict[str, str] = field(default_factory=dict)
    timeout: int = 30
    max_retries: int = 3
    tools_file: Path = field(default_factory=lambda: DEFAULT_TOOLS_FILE)

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary."""
        return {
            "command": self.command,
            "args": self.args,
            "model": self.model,
            "env": self.env,
            "timeout": self.timeout,
            "max_retries": self.max_retries,
            "tools_file": str(self.tools_file),
        }


@dataclass
class ToolCall:
    """A tool call from the chat backend."""
    id: str | None = None
    tool: str = ""
    arguments: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary."""
        return {
            "id": self.id,
            "tool": self.tool,
            "arguments": self.arguments,
        }


@dataclass
class BackendResponse:
    """Response from the chat backend."""
    tool_calls: list[ToolCall] = field(default_factory=list)
    message: str = ""
    reasoning: str = ""
    requires_confirmation: bool = False
    error: str | None = None

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary."""
        return {
            "tool_calls": [tc.to_dict() for tc in self.tool_calls],
            "message": self.message,
            "reasoning": self.reasoning,
            "requires_confirmation": self.requires_confirmation,
            "error": self.error,
        }


# =============================================================================
# Chat Backend Interface
# =============================================================================

class ChatBackend:
    """
    Interface to headless LLM chat backend.

    Manages subprocess communication, tool injection, and response parsing.
    """

    def __init__(self, config: ChatBackendConfig | None = None) -> None:
        """
        Initialize chat backend interface.

        Args:
            config: Backend configuration (uses defaults if None)
        """
        self.config = config or ChatBackendConfig()
        self._process: subprocess.Popen | None = None
        self._initialized = False

    def start(self) -> None:
        """Start the chat backend subprocess."""
        if self._process is not None:
            raise RuntimeError("Chat backend already running")

        # Build command with tools file argument
        cmd = [self.config.command]
        cmd.extend(self.config.args)

        # Add tools file argument if not already present
        if not any("--tools" in str(arg) or arg.startswith(str(self.config.tools_file)) for arg in cmd):
            cmd.extend(["--tools", str(self.config.tools_file)])

        # Add model argument if not already present
        if not any("--model" in str(arg) for arg in cmd):
            cmd.extend(["--model", self.config.model])

        # Build environment
        env = dict(self.config.env)
        env.setdefault("FORGE_TOOLS_FILE", str(self.config.tools_file))

        # Start subprocess
        try:
            self._process = subprocess.Popen(
                cmd,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=env,
                text=True,
                bufsize=1,  # Line buffered
            )
        except FileNotFoundError as e:
            raise RuntimeError(f"Chat backend command not found: {self.config.command}") from e
        except Exception as e:
            raise RuntimeError(f"Failed to start chat backend: {e}") from e

    def stop(self) -> None:
        """Stop the chat backend subprocess."""
        if self._process is not None:
            self._process.terminate()
            try:
                self._process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self._process.kill()
                self._process.wait()
            self._process = None
        self._initialized = False

    def is_running(self) -> bool:
        """Check if the backend is running."""
        return self._process is not None and self._process.poll() is None

    def send_init(self, format: Literal["openai", "anthropic"] = "openai") -> None:
        """
        Send initialization message with tool definitions.

        Args:
            format: Tool schema format
        """
        if not self.is_running():
            raise RuntimeError("Chat backend not running")

        init_msg = create_init_message(format=format)
        self._send_message(init_msg)
        self._initialized = True

    def send_message(
        self,
        user_message: str,
        context: dict[str, Any] | None = None,
        format: Literal["openai", "anthropic"] = "openai"
    ) -> BackendResponse:
        """
        Send a user message to the chat backend.

        Args:
            user_message: User's natural language message
            context: Optional context (current view, data, etc.)
            format: Tool schema format

        Returns:
            Backend response with tool calls
        """
        if not self.is_running():
            raise RuntimeError("Chat backend not running")

        # Send init if not done
        if not self._initialized:
            self.send_init(format=format)

        msg = create_chat_message(user_message, context, format)
        self._send_message(msg)

        # Read response
        return self._read_response()

    def send_telemetry(
        self,
        event: str,
        telemetry_data: dict[str, Any],
        format: Literal["openai", "anthropic"] = "openai"
    ) -> BackendResponse:
        """
        Send telemetry data for autonomous analysis.

        Args:
            event: Event type
            telemetry_data: Telemetry context
            format: Tool schema format

        Returns:
            Backend response with recommended tool calls
        """
        if not self.is_running():
            raise RuntimeError("Chat backend not running")

        # Send init if not done
        if not self._initialized:
            self.send_init(format=format)

        msg = create_telemetry_message(event, telemetry_data, format)
        self._send_message(msg)

        # Read response
        return self._read_response()

    def _send_message(self, message: dict[str, Any]) -> None:
        """Send a JSON message to the backend."""
        if self._process is None or self._process.stdin is None:
            raise RuntimeError("Chat backend not running")

        json_str = json.dumps(message)
        try:
            self._process.stdin.write(json_str + "\n")
            self._process.stdin.flush()
        except BrokenPipeError:
            raise RuntimeError("Chat backend process terminated unexpectedly")

    def _read_response(self, timeout: int | None = None) -> BackendResponse:
        """
        Read a JSON response from the backend.

        Args:
            timeout: Read timeout in seconds (uses config default if None)

        Returns:
            Parsed backend response
        """
        if self._process is None or self._process.stdout is None:
            raise RuntimeError("Chat backend not running")

        timeout = timeout or self.config.timeout

        try:
            # Read line (should be complete JSON)
            line = self._process.stdout.readline()
            if not line:
                raise RuntimeError("Chat backend closed connection")

            response_data = json.loads(line.strip())

            # Parse tool calls
            tool_calls = []
            for tc_data in response_data.get("tool_calls", []):
                tool_calls.append(ToolCall(
                    id=tc_data.get("id"),
                    tool=tc_data.get("tool", ""),
                    arguments=tc_data.get("arguments", {}),
                ))

            return BackendResponse(
                tool_calls=tool_calls,
                message=response_data.get("message", ""),
                reasoning=response_data.get("reasoning", ""),
                requires_confirmation=response_data.get("requires_confirmation", False),
                error=response_data.get("error"),
            )

        except json.JSONDecodeError as e:
            return BackendResponse(
                error=f"Invalid JSON response: {e}",
                message=f"Failed to parse backend response: {line[:100]}..."
            )
        except Exception as e:
            return BackendResponse(
                error=f"Error reading response: {e}",
                message=""
            )

    def __enter__(self):
        """Context manager entry."""
        self.start()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        """Context manager exit."""
        self.stop()


# =============================================================================
# Tool Export
# =============================================================================

def export_tools_to_file(
    path: Path | None = None,
    format: Literal["openai", "anthropic"] = "openai",
    category: str | None = None
) -> Path:
    """
    Export tool definitions to a JSON file.

    This file can be loaded by headless LLM backends via --tools argument.

    Args:
        path: Output file path (defaults to ~/.forge/tools.json)
        format: Tool schema format
        category: Optional category filter

    Returns:
        Path to the exported file
    """
    path = path or DEFAULT_TOOLS_FILE

    # Create parent directory
    path.parent.mkdir(parents=True, exist_ok=True)

    # Export tools
    from .tool_definitions import ToolCategory
    cat_enum = ToolCategory(category) if category else None
    tools_json = export_tools_json(category=cat_enum, format=format)

    # Write to file
    path.write_text(tools_json)

    return path


def ensure_tools_exported() -> Path:
    """
    Ensure tools are exported to the default location.

    This is called on FORGE startup to make tools available to backends.

    Returns:
        Path to the tools.json file
    """
    return export_tools_to_file()


# =============================================================================
# Configuration Loading
# =============================================================================

def load_config(config_path: Path | None = None) -> ChatBackendConfig:
    """
    Load chat backend configuration from file.

    Args:
        config_path: Path to config file (defaults to ~/.forge/config.yaml)

    Returns:
        Chat backend configuration
    """
    config_path = config_path or DEFAULT_CONFIG_FILE

    # Default config
    config = ChatBackendConfig()

    if not config_path.exists():
        return config

    try:
        import yaml
        with open(config_path) as f:
            data = yaml.safe_load(f) or {}

        # Parse chat_backend section
        backend_config = data.get("chat_backend", {})

        if "command" in backend_config:
            config.command = backend_config["command"]
        if "args" in backend_config:
            config.args = backend_config["args"]
        if "model" in backend_config:
            config.model = backend_config["model"]
        if "env" in backend_config:
            config.env.update(backend_config["env"])
        if "timeout" in backend_config:
            config.timeout = backend_config["timeout"]
        if "max_retries" in backend_config:
            config.max_retries = backend_config["max_retries"]
        if "tools_file" in backend_config:
            config.tools_file = Path(backend_config["tools_file"]).expanduser()

    except Exception:
        # Use defaults on error
        pass

    return config


def create_backend_from_config(config_path: Path | None = None) -> ChatBackend:
    """
    Create a ChatBackend instance from configuration file.

    Args:
        config_path: Path to config file (defaults to ~/.forge/config.yaml)

    Returns:
        Configured ChatBackend instance
    """
    config = load_config(config_path)
    return ChatBackend(config)


# =============================================================================
# CLI Helpers
# =============================================================================

def main() -> None:
    """CLI entry point for testing chat backend integration."""
    import argparse

    parser = argparse.ArgumentParser(description="FORGE Chat Backend Tools")
    subparsers = parser.add_subparsers(dest="command", help="Command")

    # Export command
    export_parser = subparsers.add_parser("export", help="Export tool definitions")
    export_parser.add_argument("-o", "--output", type=Path, help="Output file")
    export_parser.add_argument("-f", "--format", choices=["openai", "anthropic"], default="openai")
    export_parser.add_argument("-c", "--category", help="Filter by category")

    # Test command
    test_parser = subparsers.add_parser("test", help="Test chat backend")
    test_parser.add_argument("-m", "--message", default="What can you do?", help="Test message")
    test_parser.add_argument("-c", "--config", type=Path, help="Config file path")

    args = parser.parse_args()

    if args.command == "export":
        path = export_tools_to_file(args.output, args.format, args.category)
        print(f"Exported tools to: {path}")
        print(f"Total tools: {len(export_tools_json(args.format, args.category))}")

    elif args.command == "test":
        backend = create_backend_from_config(args.config)
        print(f"Starting chat backend: {backend.config.command}")
        print(f"Model: {backend.config.model}")
        print(f"Tools file: {backend.config.tools_file}")
        print()

        # Ensure tools are exported
        ensure_tools_exported()

        try:
            with backend:
                response = backend.send_message(args.message)
                print(f"Response: {response.message}")
                print(f"Tool calls: {len(response.tool_calls)}")
                for tc in response.tool_calls:
                    print(f"  - {tc.tool}: {tc.arguments}")
        except Exception as e:
            print(f"Error: {e}")
            sys.exit(1)

    else:
        parser.print_help()


if __name__ == "__main__":
    main()
