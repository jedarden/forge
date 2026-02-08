"""
FORGE Configuration Management

Provides centralized configuration loading with multi-layer support:
1. Hardcoded defaults
2. User config file (~/.forge/config.yaml)
3. Workspace overrides (.forge/config.yaml)
4. Environment variables

Features:
- Schema validation with clear error messages
- Environment variable expansion (${VAR} syntax)
- Path expansion (~ and environment variables)
- Hot-reload for non-critical settings
- set_config/get_config tools for runtime modification
"""

from __future__ import annotations

import os
import re
from dataclasses import dataclass, field, fields
from enum import Enum
from fnmatch import fnmatch
from pathlib import Path
from typing import Any, ClassVar

import yaml


# =============================================================================
# Configuration Schema Data Classes
# =============================================================================


class ConfigReloadable(Enum):
    """Settings that can be hot-reloaded"""
    DASHBOARD_REFRESH = "dashboard_refresh_interval_ms"
    LOG_POLL_INTERVAL = "log_poll_interval_seconds"
    COST_FORECAST = "forecast_days"
    DEBUG_LOGGING = "debug_logging"


@dataclass
class ChatBackendConfig:
    """Chat backend configuration"""
    command: str = "claude-code"
    args: list[str] = field(default_factory=lambda: ["chat", "--headless"])
    model: str = "sonnet"
    env: dict[str, str] = field(default_factory=dict)
    timeout: int = 30
    max_retries: int = 3


@dataclass
class LauncherConfig:
    """Worker launcher configuration"""
    executable: str = ""
    models: list[str] = field(default_factory=list)
    default_args: list[str] = field(default_factory=list)


@dataclass
class WorkerRepoConfig:
    """Worker configuration repository"""
    url: str = ""
    branch: str = "main"
    path: str = "configs/"


@dataclass
class LogCollectionConfig:
    """Log collection settings"""
    paths: list[str] = field(default_factory=lambda: ["~/.forge/logs/*.log"])
    format: str = "jsonl"  # or "keyvalue" or "auto-detect"
    poll_interval_seconds: int = 1
    max_age_days: int = 30
    max_size_mb: int = 1000


@dataclass
class CostTrackingConfig:
    """Cost tracking settings"""
    enabled: bool = True
    database_path: str = "~/.forge/costs.db"
    forecast_days: int = 30


@dataclass
class DashboardConfig:
    """Dashboard settings"""
    refresh_interval_ms: int = 1000
    max_fps: int = 60
    default_layout: str = "overview"


@dataclass
class HotkeyConfig:
    """Hotkey customization"""
    workers_view: str = "w"
    tasks_view: str = "t"
    costs_view: str = "c"
    metrics_view: str = "m"
    logs_view: str = "l"
    overview: str = "o"
    spawn_worker: str = "s"
    kill_worker: str = "k"
    chat_input: str = ":"


@dataclass
class PriorityTierConfig:
    """Priority tier mapping for routing"""
    P0: str = "premium"
    P1: str = "premium"
    P2: str = "standard"
    P3: str = "budget"
    P4: str = "budget"


@dataclass
class ScoringWeightsConfig:
    """Task scoring weights for model routing"""
    priority: float = 0.4
    blockers: float = 0.3
    age: float = 0.2
    labels: float = 0.1


@dataclass
class RoutingConfig:
    """Model routing (cost optimization)"""
    priority_tiers: PriorityTierConfig = field(default_factory=PriorityTierConfig)
    subscription_first: bool = True
    fallback_to_api: bool = True
    scoring_weights: ScoringWeightsConfig = field(default_factory=ScoringWeightsConfig)


@dataclass
class ForgeConfig:
    """Main FORGE configuration"""
    # Configuration sections
    chat_backend: ChatBackendConfig = field(default_factory=ChatBackendConfig)
    launchers: dict[str, LauncherConfig] = field(default_factory=dict)
    worker_repos: list[WorkerRepoConfig] = field(default_factory=list)
    log_collection: LogCollectionConfig = field(default_factory=LogCollectionConfig)
    status_path: str = "~/.forge/status/"
    cost_tracking: CostTrackingConfig = field(default_factory=CostTrackingConfig)
    dashboard: DashboardConfig = field(default_factory=DashboardConfig)
    hotkeys: HotkeyConfig = field(default_factory=HotkeyConfig)
    routing: RoutingConfig = field(default_factory=RoutingConfig)

    # Internal settings
    debug_logging: bool = False
    log_level: str = "INFO"

    # Workspace context (for overrides)
    _workspace_path: Path | None = field(default=None, repr=False, compare=False)

    # Class-level default for tracking which settings are reloadable
    _RELOADABLE_SETTINGS: ClassVar[set[str]] = {
        "dashboard.refresh_interval_ms",
        "log_collection.poll_interval_seconds",
        "cost_tracking.forecast_days",
        "debug_logging",
        "log_level",
    }


# =============================================================================
# Configuration Errors
# =============================================================================


class ConfigError(Exception):
    """Base configuration error"""
    pass


class ConfigValidationError(ConfigError):
    """Configuration validation error with detailed context"""

    def __init__(self, message: str, path: str = "", value: Any = None):
        self.message = message
        self.path = path
        self.value = value
        full_msg = f"Validation error"
        if path:
            full_msg += f" at '{path}'"
        full_msg += f": {message}"
        if value is not None:
            full_msg += f" (got: {repr(value)})"
        super().__init__(full_msg)


class ConfigNotFoundError(ConfigError):
    """Configuration file not found (non-fatal, will use defaults)"""
    pass


# =============================================================================
# Configuration Utilities
# =============================================================================


def expand_env_vars(value: str, env: dict[str, str] | None = None) -> str:
    """
    Expand environment variables in a string.

    Supports ${VAR} and $VAR syntax. Non-existent variables are left unexpanded.

    Args:
        value: String potentially containing environment variables
        env: Optional environment dictionary (defaults to os.environ)

    Returns:
        String with environment variables expanded

    Examples:
        >>> expand_env_vars("${HOME}/.forge")
        '/home/user/.forge'
        >>> expand_env_vars("$FOO/bar", {"FOO": "baz"})
        'baz/bar'
    """
    if env is None:
        env = os.environ

    # Match ${VAR} or $VAR (but not ${VAR} with no closing brace)
    pattern = r'\$\{([^}]+)\}|\$([a-zA-Z_][a-zA-Z0-9_]*)'

    def replacer(match: re.Match) -> str:
        var_name = match.group(1) or match.group(2)
        return env.get(var_name, match.group(0))

    return re.sub(pattern, replacer, value)


def expand_path(path: str | Path) -> Path:
    """
    Expand a path string with ~ and environment variables.

    Args:
        path: Path string potentially containing ~ or ${VAR}

    Returns:
        Expanded Path object
    """
    path_str = str(path)
    # First expand environment variables
    expanded = expand_env_vars(path_str)
    # Then expand ~
    return Path(expanded).expanduser()


def validate_numeric_range(
    value: Any,
    min_val: int | float | None = None,
    max_val: int | float | None = None,
    path: str = "",
) -> None:
    """
    Validate that a numeric value is within range.

    Args:
        value: Value to validate
        min_val: Minimum allowed value (inclusive)
        max_val: Maximum allowed value (inclusive)
        path: Configuration path for error messages

    Raises:
        ConfigValidationError: If validation fails
    """
    if not isinstance(value, (int, float)):
        raise ConfigValidationError(f"Expected numeric value, got {type(value).__name__}", path, value)

    if min_val is not None and value < min_val:
        raise ConfigValidationError(f"Value must be >= {min_val}", path, value)

    if max_val is not None and value > max_val:
        raise ConfigValidationError(f"Value must be <= {max_val}", path, value)


def validate_enum(value: Any, allowed: set[str] | list[str], path: str = "") -> None:
    """
    Validate that a value is in the allowed set.

    Args:
        value: Value to validate
        allowed: Set of allowed values
        path: Configuration path for error messages

    Raises:
        ConfigValidationError: If validation fails
    """
    if value not in allowed:
        allowed_str = ", ".join(repr(v) for v in sorted(allowed))
        raise ConfigValidationError(f"Value must be one of: {allowed_str}", path, value)


# =============================================================================
# Configuration Loader
# =============================================================================


class ConfigLoader:
    """
    Loads and validates FORGE configuration from multiple sources.

    Loading order (later sources override earlier ones):
    1. Hardcoded defaults
    2. User config file (~/.forge/config.yaml or $FORGE_CONFIG)
    3. Workspace override (.forge/config.yaml in workspace)
    4. Environment variables (FORGE_* prefix)
    """

    DEFAULT_USER_CONFIG_PATH = "~/.forge/config.yaml"
    WORKSPACE_CONFIG_FILE = ".forge/config.yaml"

    # Map of environment variable names to config paths
    ENV_VAR_MAP: dict[str, str] = {
        "FORGE_DEBUG": "debug_logging",
        "FORGE_LOG_LEVEL": "log_level",
        "FORGE_STATUS_PATH": "status_path",
        "FORGE_COSTS_DB": "cost_tracking.database_path",
    }

    def __init__(
        self,
        workspace_path: Path | str | None = None,
        user_config_path: Path | str | None = None,
    ):
        """
        Initialize the configuration loader.

        Args:
            workspace_path: Optional workspace path for override loading
            user_config_path: Optional custom user config path
        """
        self.workspace_path = expand_path(workspace_path) if workspace_path else None
        self.user_config_path = expand_path(user_config_path) if user_config_path else None

    def load(self) -> ForgeConfig:
        """
        Load configuration from all sources.

        Returns:
            Fully loaded and validated ForgeConfig

        Raises:
            ConfigValidationError: If configuration is invalid
        """
        # Start with defaults
        config = ForgeConfig()

        # Load user config
        user_config_path = self.user_config_path or self._get_user_config_path()
        user_data = self._load_yaml_file(user_config_path, required=False)
        if user_data:
            self._apply_config_dict(config, user_data, "user config")

        # Load workspace override
        if self.workspace_path:
            workspace_data = self._load_workspace_config()
            if workspace_data:
                self._apply_config_dict(config, workspace_data, "workspace config")
                config._workspace_path = self.workspace_path

        # Apply environment variable overrides
        self._apply_env_vars(config)

        # Validate final configuration
        self._validate_config(config)

        return config

    def _get_user_config_path(self) -> Path:
        """Get the user config path, checking FORGE_CONFIG env var."""
        env_path = os.environ.get("FORGE_CONFIG")
        if env_path:
            return expand_path(env_path)
        return expand_path(self.DEFAULT_USER_CONFIG_PATH)

    def _load_yaml_file(self, path: Path, required: bool = True) -> dict[str, Any] | None:
        """
        Load a YAML configuration file.

        Args:
            path: Path to YAML file
            required: If True, raise error if file not found

        Returns:
            Parsed YAML data or None if file not found and not required

        Raises:
            ConfigError: If file is invalid YAML
        """
        if not path.exists():
            if required:
                raise ConfigNotFoundError(f"Configuration file not found: {path}")
            return None

        try:
            with open(path, "r") as f:
                data = yaml.safe_load(f)
                return data if isinstance(data, dict) else {}
        except yaml.YAMLError as e:
            raise ConfigError(f"Invalid YAML in {path}: {e}")

    def _load_workspace_config(self) -> dict[str, Any] | None:
        """Load workspace override configuration if workspace is set."""
        if not self.workspace_path:
            return None

        workspace_config_path = self.workspace_path / self.WORKSPACE_CONFIG_FILE
        return self._load_yaml_file(workspace_config_path, required=False)

    def _apply_config_dict(
        self,
        config: ForgeConfig,
        data: dict[str, Any],
        source_name: str,
        prefix: str = "",
    ) -> None:
        """
        Apply configuration dictionary to the config object.

        Args:
            config: Config object to update
            data: Configuration dictionary to apply
            source_name: Name of source for error messages
            prefix: Current config path (for nested structures)
        """
        for key, value in data.items():
            current_path = f"{prefix}.{key}" if prefix else key

            if value is None:
                continue  # Skip null values

            # Handle nested configuration objects
            if key == "chat_backend" and isinstance(value, dict):
                self._apply_config_dict(config.chat_backend, value, source_name, current_path)
            elif key == "launchers" and isinstance(value, dict):
                for name, launcher_data in value.items():
                    if isinstance(launcher_data, dict):
                        launcher = LauncherConfig()
                        self._apply_config_dict(launcher, launcher_data, source_name, current_path)
                        config.launchers[name] = launcher
            elif key == "worker_repos" and isinstance(value, list):
                config.worker_repos = [
                    WorkerRepoConfig(**item) if isinstance(item, dict) else item
                    for item in value
                ]
            elif key == "log_collection" and isinstance(value, dict):
                self._apply_config_dict(config.log_collection, value, source_name, current_path)
            elif key == "cost_tracking" and isinstance(value, dict):
                self._apply_config_dict(config.cost_tracking, value, source_name, current_path)
            elif key == "dashboard" and isinstance(value, dict):
                self._apply_config_dict(config.dashboard, value, source_name, current_path)
            elif key == "hotkeys" and isinstance(value, dict):
                self._apply_config_dict(config.hotkeys, value, source_name, current_path)
            elif key == "routing" and isinstance(value, dict):
                if "priority_tiers" in value and isinstance(value["priority_tiers"], dict):
                    self._apply_config_dict(
                        config.routing.priority_tiers,
                        value["priority_tiers"],
                        source_name,
                        f"{current_path}.priority_tiers",
                    )
                if "scoring_weights" in value and isinstance(value["scoring_weights"], dict):
                    self._apply_config_dict(
                        config.routing.scoring_weights,
                        value["scoring_weights"],
                        source_name,
                        f"{current_path}.scoring_weights",
                    )
                if "subscription_first" in value:
                    config.routing.subscription_first = bool(value["subscription_first"])
                if "fallback_to_api" in value:
                    config.routing.fallback_to_api = bool(value["fallback_to_api"])
            else:
                # Handle direct attributes
                if hasattr(config, key):
                    setattr(config, key, value)

    def _apply_env_vars(self, config: ForgeConfig) -> None:
        """Apply environment variable overrides to configuration."""
        for env_var, config_path in self.ENV_VAR_MAP.items():
            value = os.environ.get(env_var)
            if value is None:
                continue

            # Navigate to the nested attribute
            parts = config_path.split(".")
            obj = config

            for part in parts[:-1]:
                if hasattr(obj, part):
                    obj = getattr(obj, part)
                else:
                    break  # Invalid path, skip
            else:
                final_attr = parts[-1]
                if hasattr(obj, final_attr):
                    # Type conversion for boolean and numeric values
                    current_value = getattr(obj, final_attr)
                    if isinstance(current_value, bool):
                        value = value.lower() in ("1", "true", "yes", "on")
                    elif isinstance(current_value, int):
                        try:
                            value = int(value)
                        except ValueError:
                            continue
                    elif isinstance(current_value, float):
                        try:
                            value = float(value)
                        except ValueError:
                            continue

                    setattr(obj, final_attr, value)

    def _validate_config(self, config: ForgeConfig) -> None:
        """
        Validate the complete configuration.

        Args:
            config: Configuration to validate

        Raises:
            ConfigValidationError: If configuration is invalid
        """
        # Validate chat_backend
        if config.chat_backend.timeout <= 0:
            raise ConfigValidationError(
                "timeout must be positive",
                "chat_backend.timeout",
                config.chat_backend.timeout,
            )

        if config.chat_backend.max_retries < 0:
            raise ConfigValidationError(
                "max_retries must be non-negative",
                "chat_backend.max_retries",
                config.chat_backend.max_retries,
            )

        # Validate log_collection
        validate_enum(
            config.log_collection.format,
            {"jsonl", "keyvalue", "auto-detect"},
            "log_collection.format",
        )
        validate_numeric_range(
            config.log_collection.poll_interval_seconds,
            min_val=1,
            path="log_collection.poll_interval_seconds",
        )
        validate_numeric_range(
            config.log_collection.max_age_days,
            min_val=1,
            path="log_collection.max_age_days",
        )

        # Validate dashboard
        validate_numeric_range(
            config.dashboard.refresh_interval_ms,
            min_val=100,
            path="dashboard.refresh_interval_ms",
        )
        validate_numeric_range(
            config.dashboard.max_fps,
            min_val=1,
            max_val=120,
            path="dashboard.max_fps",
        )

        # Validate cost_tracking
        validate_numeric_range(
            config.cost_tracking.forecast_days,
            min_val=1,
            path="cost_tracking.forecast_days",
        )

        # Validate log_level
        validate_enum(
            config.log_level,
            {"DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"},
            "log_level",
        )


# =============================================================================
# Global Configuration Instance
# =============================================================================

_global_config: ForgeConfig | None = None


def get_config(
    workspace_path: Path | str | None = None,
    reload: bool = False,
) -> ForgeConfig:
    """
    Get the global configuration instance.

    Args:
        workspace_path: Optional workspace path for override loading
        reload: If True, reload configuration even if already loaded

    Returns:
        The global ForgeConfig instance
    """
    global _global_config

    if _global_config is None or reload:
        loader = ConfigLoader(workspace_path=workspace_path)
        _global_config = loader.load()

    return _global_config


def reload_config(workspace_path: Path | str | None = None) -> ForgeConfig:
    """
    Force reload the global configuration.

    This is useful for hot-reloading non-critical settings.

    Args:
        workspace_path: Optional workspace path for override loading

    Returns:
        The reloaded ForgeConfig instance
    """
    return get_config(workspace_path=workspace_path, reload=True)


# =============================================================================
# Config Tools (get_config/set_config)
# =============================================================================


def get_config_value(path: str, workspace_path: Path | str | None = None) -> Any:
    """
    Get a configuration value by path.

    Supports dot notation for nested values (e.g., "dashboard.refresh_interval_ms").

    Args:
        path: Configuration path (dot-separated)
        workspace_path: Optional workspace path for override loading

    Returns:
        The configuration value

    Raises:
        ConfigError: If path is invalid

    Examples:
        >>> get_config_value("dashboard.refresh_interval_ms")
        1000
        >>> get_config_value("chat_backend.model")
        'sonnet'
    """
    config = get_config(workspace_path=workspace_path)
    parts = path.split(".")

    value = config
    for part in parts:
        if isinstance(value, dict):
            value = value.get(part)
        elif hasattr(value, part):
            value = getattr(value, part)
        else:
            raise ConfigError(f"Invalid configuration path: {path}")

    return value


def set_config_value(
    path: str,
    value: Any,
    reloadable_only: bool = True,
    workspace_path: Path | str | None = None,
) -> bool:
    """
    Set a configuration value by path at runtime.

    Args:
        path: Configuration path (dot-separated)
        value: New value to set
        reloadable_only: If True, only allow changes to reloadable settings
        workspace_path: Optional workspace path for override loading

    Returns:
        True if value was set successfully

    Raises:
        ConfigError: If path is invalid or setting is not allowed

    Examples:
        >>> set_config_value("dashboard.refresh_interval_ms", 500)
        True
        >>> set_config_value("debug_logging", True)
        True
    """
    config = get_config(workspace_path=workspace_path)

    # Check if setting is reloadable
    if reloadable_only and path not in ForgeConfig._RELOADABLE_SETTINGS:
        raise ConfigError(
            f"Cannot set '{path}' at runtime (not reloadable). "
            f"Reloadable settings: {', '.join(sorted(ForgeConfig._RELOADABLE_SETTINGS))}"
        )

    parts = path.split(".")

    # Navigate to the parent object
    obj = config
    for part in parts[:-1]:
        if isinstance(obj, dict):
            obj = obj[part]
        elif hasattr(obj, part):
            obj = getattr(obj, part)
        else:
            raise ConfigError(f"Invalid configuration path: {path}")

    # Set the value
    final_attr = parts[-1]
    if isinstance(obj, dict):
        obj[final_attr] = value
    elif hasattr(obj, final_attr):
        setattr(obj, final_attr, value)
    else:
        raise ConfigError(f"Invalid configuration path: {path}")

    return True


# =============================================================================
# Initialize Default Configuration
# =============================================================================


def init_default_config(output_path: Path | str | None = None) -> Path:
    """
    Create a default configuration file.

    Args:
        output_path: Optional output path (defaults to ~/.forge/config.yaml)

    Returns:
        Path where configuration was written
    """
    if output_path is None:
        output_path = expand_path(ConfigLoader.DEFAULT_USER_CONFIG_PATH)
    else:
        output_path = expand_path(output_path)

    # Create parent directory
    output_path.parent.mkdir(parents=True, exist_ok=True)

    # Default configuration
    default_config = {
        "chat_backend": {
            "command": "claude-code",
            "args": ["chat", "--headless", "--tools=${FORGE_TOOLS_FILE}"],
            "model": "sonnet",
            "env": {
                "ANTHROPIC_API_KEY": "${ANTHROPIC_API_KEY}",
                "FORGE_TOOLS_FILE": "~/.forge/tools.json",
            },
            "timeout": 30,
            "max_retries": 3,
        },
        "launchers": {
            "claude-code": {
                "executable": "~/.forge/launchers/claude-code-launcher",
                "models": ["sonnet", "opus", "haiku"],
                "default_args": ["--tmux"],
            },
        },
        "worker_repos": [
            {
                "url": "https://github.com/forge-community/worker-configs",
                "branch": "main",
                "path": "configs/",
            }
        ],
        "log_collection": {
            "paths": ["~/.forge/logs/*.log"],
            "format": "jsonl",
            "poll_interval_seconds": 1,
            "max_age_days": 30,
            "max_size_mb": 1000,
        },
        "status_path": "~/.forge/status/",
        "cost_tracking": {
            "enabled": True,
            "database_path": "~/.forge/costs.db",
            "forecast_days": 30,
        },
        "dashboard": {
            "refresh_interval_ms": 1000,
            "max_fps": 60,
            "default_layout": "overview",
        },
        "hotkeys": {
            "workers_view": "w",
            "tasks_view": "t",
            "costs_view": "c",
            "metrics_view": "m",
            "logs_view": "l",
            "overview": "o",
            "spawn_worker": "s",
            "kill_worker": "k",
            "chat_input": ":",
        },
        "routing": {
            "priority_tiers": {
                "P0": "premium",
                "P1": "premium",
                "P2": "standard",
                "P3": "budget",
                "P4": "budget",
            },
            "subscription_first": True,
            "fallback_to_api": True,
            "scoring_weights": {
                "priority": 0.4,
                "blockers": 0.3,
                "age": 0.2,
                "labels": 0.1,
            },
        },
    }

    # Write configuration file
    with open(output_path, "w") as f:
        yaml.dump(default_config, f, default_flow_style=False, sort_keys=False)

    return output_path
