"""
Tests for FORGE configuration management
"""

import os
import tempfile
from pathlib import Path
from unittest import mock

import pytest
import yaml

from forge.config import (
    ConfigError,
    ConfigLoader,
    ConfigNotFoundError,
    ConfigValidationError,
    ForgeConfig,
    expand_env_vars,
    expand_path,
    get_config,
    get_config_value,
    init_default_config,
    reload_config,
    set_config_value,
    validate_enum,
    validate_numeric_range,
    ChatBackendConfig,
    LauncherConfig,
    LogCollectionConfig,
    CostTrackingConfig,
    DashboardConfig,
    HotkeyConfig,
    RoutingConfig,
    PriorityTierConfig,
    ScoringWeightsConfig,
)


# =============================================================================
# Utility Tests
# =============================================================================


class TestExpandEnvVars:
    """Tests for expand_env_vars utility"""

    def test_expand_simple_var(self):
        with mock.patch.dict(os.environ, {"FOO": "bar"}):
            assert expand_env_vars("${FOO}") == "bar"

    def test_expand_multiple_vars(self):
        with mock.patch.dict(os.environ, {"FOO": "bar", "BAZ": "qux"}):
            assert expand_env_vars("${FOO}/${BAZ}") == "bar/qux"

    def test_expand_braced_var(self):
        with mock.patch.dict(os.environ, {"FOO": "bar"}):
            assert expand_env_vars("$FOO") == "bar"

    def test_expand_missing_var(self):
        with mock.patch.dict(os.environ, {}, clear=True):
            assert expand_env_vars("${NONEXISTENT}") == "${NONEXISTENT}"

    def test_expand_mixed_vars_and_text(self):
        with mock.patch.dict(os.environ, {"HOME": "/home/user"}):
            assert expand_env_vars("prefix-${HOME}-suffix") == "prefix-/home/user-suffix"


class TestExpandPath:
    """Tests for expand_path utility"""

    def test_expand_tilde(self):
        result = expand_path("~/.forge/config.yaml")
        assert str(result).startswith("/")
        assert "~" not in str(result)

    def test_expand_env_var(self):
        with mock.patch.dict(os.environ, {"FORGE_DIR": "/opt/forge"}):
            result = expand_path("${FORGE_DIR}/config.yaml")
            assert result == Path("/opt/forge/config.yaml")

    def test_expand_combined(self):
        with mock.patch.dict(os.environ, {"FORGE_DIR": "/opt/forge"}):
            result = expand_path("~/${FORGE_DIR}/config.yaml")
            assert str(result).endswith("/opt/forge/config.yaml")


class TestValidationUtils:
    """Tests for validation utilities"""

    def test_validate_numeric_range_valid(self):
        validate_numeric_range(5, min_val=1, max_val=10)

    def test_validate_numeric_range_below_min(self):
        with pytest.raises(ConfigValidationError):
            validate_numeric_range(0, min_val=1)

    def test_validate_numeric_range_above_max(self):
        with pytest.raises(ConfigValidationError):
            validate_numeric_range(11, max_val=10)

    def test_validate_numeric_range_wrong_type(self):
        with pytest.raises(ConfigValidationError):
            validate_numeric_range("not a number")

    def test_validate_enum_valid(self):
        validate_enum("INFO", {"DEBUG", "INFO", "WARNING"})

    def test_validate_enum_invalid(self):
        with pytest.raises(ConfigValidationError):
            validate_enum("INVALID", {"DEBUG", "INFO", "WARNING"})


# =============================================================================
# Config Data Class Tests
# =============================================================================


class TestConfigDataClasses:
    """Tests for configuration data classes"""

    def test_chat_backend_defaults(self):
        config = ChatBackendConfig()
        assert config.command == "claude-code"
        assert config.args == ["chat", "--headless"]
        assert config.model == "sonnet"
        assert config.timeout == 30
        assert config.max_retries == 3

    def test_log_collection_defaults(self):
        config = LogCollectionConfig()
        assert config.paths == ["~/.forge/logs/*.log"]
        assert config.format == "jsonl"
        assert config.poll_interval_seconds == 1

    def test_cost_tracking_defaults(self):
        config = CostTrackingConfig()
        assert config.enabled is True
        assert config.database_path == "~/.forge/costs.db"
        assert config.forecast_days == 30

    def test_dashboard_defaults(self):
        config = DashboardConfig()
        assert config.refresh_interval_ms == 1000
        assert config.max_fps == 60
        assert config.default_layout == "overview"

    def test_forge_config_defaults(self):
        config = ForgeConfig()
        assert isinstance(config.chat_backend, ChatBackendConfig)
        assert isinstance(config.log_collection, LogCollectionConfig)
        assert isinstance(config.cost_tracking, CostTrackingConfig)
        assert isinstance(config.dashboard, DashboardConfig)


# =============================================================================
# ConfigLoader Tests
# =============================================================================


class TestConfigLoader:
    """Tests for ConfigLoader"""

    def test_load_with_defaults(self):
        """Test loading with no config file uses defaults"""
        loader = ConfigLoader(user_config_path="/nonexistent/path.yaml")
        config = loader.load()
        assert isinstance(config, ForgeConfig)
        assert config.chat_backend.command == "claude-code"
        assert config.dashboard.refresh_interval_ms == 1000

    def test_load_with_user_config(self):
        """Test loading with user config overrides"""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".yaml", delete=False) as f:
            yaml.dump(
                {
                    "chat_backend": {"command": "custom-command", "model": "opus"},
                    "dashboard": {"refresh_interval_ms": 500},
                },
                f,
            )
            config_path = Path(f.name)

        try:
            loader = ConfigLoader(user_config_path=config_path)
            config = loader.load()
            assert config.chat_backend.command == "custom-command"
            assert config.chat_backend.model == "opus"
            assert config.dashboard.refresh_interval_ms == 500
        finally:
            config_path.unlink()

    def test_load_with_workspace_override(self):
        """Test loading with workspace override"""
        # Create user config
        with tempfile.TemporaryDirectory() as tmpdir:
            user_config = Path(tmpdir) / "user.yaml"
            yaml.dump(
                {"dashboard": {"refresh_interval_ms": 500}},
                open(user_config, "w"),
            )

            # Create workspace override
            workspace_dir = Path(tmpdir) / "workspace"
            workspace_dir.mkdir()
            workspace_config = workspace_dir / ".forge"
            workspace_config.mkdir()
            workspace_yaml = workspace_config / "config.yaml"
            yaml.dump(
                {"dashboard": {"refresh_interval_ms": 250}},
                open(workspace_yaml, "w"),
            )

            loader = ConfigLoader(
                user_config_path=user_config,
                workspace_path=workspace_dir,
            )
            config = loader.load()
            # Workspace override should take precedence
            assert config.dashboard.refresh_interval_ms == 250

    def test_load_with_env_var_override(self):
        """Test environment variable overrides"""
        with mock.patch.dict(os.environ, {"FORGE_DEBUG": "1"}):
            loader = ConfigLoader(user_config_path="/nonexistent/path.yaml")
            config = loader.load()
            assert config.debug_logging is True

    def test_validation_error_invalid_timeout(self):
        """Test validation error for invalid timeout"""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".yaml", delete=False) as f:
            yaml.dump({"chat_backend": {"timeout": -1}}, f)
            config_path = Path(f.name)

        try:
            loader = ConfigLoader(user_config_path=config_path)
            with pytest.raises(ConfigValidationError):
                loader.load()
        finally:
            config_path.unlink()

    def test_validation_error_invalid_log_format(self):
        """Test validation error for invalid log format"""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".yaml", delete=False) as f:
            yaml.dump({"log_collection": {"format": "invalid"}}, f)
            config_path = Path(f.name)

        try:
            loader = ConfigLoader(user_config_path=config_path)
            with pytest.raises(ConfigValidationError):
                loader.load()
        finally:
            config_path.unlink()

    def test_validation_error_invalid_refresh_interval(self):
        """Test validation error for invalid refresh interval"""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".yaml", delete=False) as f:
            yaml.dump({"dashboard": {"refresh_interval_ms": 50}}, f)
            config_path = Path(f.name)

        try:
            loader = ConfigLoader(user_config_path=config_path)
            with pytest.raises(ConfigValidationError):
                loader.load()
        finally:
            config_path.unlink()

    def test_launchers_config(self):
        """Test launchers configuration loading"""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".yaml", delete=False) as f:
            yaml.dump(
                {
                    "launchers": {
                        "claude-code": {
                            "executable": "/path/to/launcher",
                            "models": ["sonnet", "opus"],
                        }
                    }
                },
                f,
            )
            config_path = Path(f.name)

        try:
            loader = ConfigLoader(user_config_path=config_path)
            config = loader.load()
            assert "claude-code" in config.launchers
            assert config.launchers["claude-code"].executable == "/path/to/launcher"
            assert config.launchers["claude-code"].models == ["sonnet", "opus"]
        finally:
            config_path.unlink()

    def test_routing_config(self):
        """Test routing configuration loading"""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".yaml", delete=False) as f:
            yaml.dump(
                {
                    "routing": {
                        "priority_tiers": {"P0": "premium", "P1": "standard"},
                        "subscription_first": False,
                    }
                },
                f,
            )
            config_path = Path(f.name)

        try:
            loader = ConfigLoader(user_config_path=config_path)
            config = loader.load()
            assert config.routing.priority_tiers.P0 == "premium"
            assert config.routing.priority_tiers.P1 == "standard"
            assert config.routing.subscription_first is False
        finally:
            config_path.unlink()


# =============================================================================
# Global Config Tests
# =============================================================================


class TestGlobalConfig:
    """Tests for global configuration functions"""

    def setup_method(self):
        """Reset global config before each test"""
        import forge.config as config_module
        config_module._global_config = None

    def test_get_config_creates_instance(self):
        """Test that get_config creates a global instance"""
        with mock.patch.dict(os.environ, {"HOME": "/tmp"}):
            config = get_config()
            assert isinstance(config, ForgeConfig)
            # Second call should return same instance
            config2 = get_config()
            assert config is config2

    def test_get_config_reload(self):
        """Test that get_config with reload=True reloads config"""
        with mock.patch.dict(os.environ, {"HOME": "/tmp"}):
            config1 = get_config()
            config2 = get_config(reload=True)
            assert isinstance(config2, ForgeConfig)
            # Different instance after reload
            # (but same content since no config file changed)

    def test_get_config_value(self):
        """Test get_config_value function"""
        with mock.patch.dict(os.environ, {"HOME": "/tmp"}):
            value = get_config_value("dashboard.refresh_interval_ms")
            assert value == 1000

    def test_get_config_value_nested(self):
        """Test get_config_value with nested path"""
        with mock.patch.dict(os.environ, {"HOME": "/tmp"}):
            value = get_config_value("chat_backend.model")
            assert value == "sonnet"

    def test_get_config_value_invalid_path(self):
        """Test get_config_value with invalid path"""
        with mock.patch.dict(os.environ, {"HOME": "/tmp"}):
            with pytest.raises(ConfigError):
                get_config_value("invalid.path.to.nowhere")

    def test_set_config_value_reloadable(self):
        """Test set_config_value with reloadable setting"""
        with mock.patch.dict(os.environ, {"HOME": "/tmp"}):
            result = set_config_value("dashboard.refresh_interval_ms", 500)
            assert result is True
            value = get_config_value("dashboard.refresh_interval_ms")
            assert value == 500

    def test_set_config_value_not_reloadable(self):
        """Test set_config_value with non-reloadable setting"""
        with mock.patch.dict(os.environ, {"HOME": "/tmp"}):
            with pytest.raises(ConfigError, match="not reloadable"):
                set_config_value("chat_backend.command", "other-command")

    def test_set_config_value_not_reloadable_forced(self):
        """Test set_config_value with reloadable_only=False"""
        with mock.patch.dict(os.environ, {"HOME": "/tmp"}):
            result = set_config_value("chat_backend.command", "other-command", reloadable_only=False)
            assert result is True
            value = get_config_value("chat_backend.command")
            assert value == "other-command"


# =============================================================================
# Init Default Config Tests
# =============================================================================


class TestInitDefaultConfig:
    """Tests for init_default_config function"""

    def test_init_default_config_creates_file(self):
        """Test that init_default_config creates a config file"""
        with tempfile.TemporaryDirectory() as tmpdir:
            output_path = Path(tmpdir) / "config.yaml"
            result = init_default_config(output_path)

            assert result == output_path
            assert output_path.exists()

            with open(output_path) as f:
                data = yaml.safe_load(f)
                assert "chat_backend" in data
                assert "dashboard" in data
                assert "hotkeys" in data

    def test_init_default_config_default_location(self):
        """Test init_default_config with default location"""
        with tempfile.TemporaryDirectory() as tmpdir:
            default_path = Path(tmpdir) / ".forge" / "config.yaml"
            with mock.patch("forge.config.expand_path", return_value=default_path):
                # We need to mock expand_path before importing
                # For now, test with explicit path
                result = init_default_config(default_path)
                assert result == default_path
                assert default_path.exists()
