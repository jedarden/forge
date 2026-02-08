"""
Tests for FORGE Worker Launcher Integration

Tests the worker launcher subprocess spawning, JSON output parsing,
protocol validation, and error handling per ADR 0014.
"""

import json
import os
import tempfile
from pathlib import Path
from unittest.mock import Mock, patch

import pytest

from forge.launcher import (
    LauncherConfig,
    LauncherErrorType,
    LauncherResult,
    ProtocolValidationResult,
    WorkerLauncher,
    spawn_worker,
    validate_launcher,
)


# =============================================================================
# Test Fixtures
# =============================================================================


@pytest.fixture
def temp_workspace():
    """Create a temporary workspace directory for testing"""
    with tempfile.TemporaryDirectory() as tmpdir:
        yield Path(tmpdir)


@pytest.fixture
def temp_forge_dir():
    """Create a temporary FORGE directory structure"""
    with tempfile.TemporaryDirectory() as tmpdir:
        forge_dir = Path(tmpdir)
        status_dir = forge_dir / "status"
        log_dir = forge_dir / "logs"
        status_dir.mkdir(parents=True)
        log_dir.mkdir(parents=True)
        yield forge_dir, status_dir, log_dir


@pytest.fixture
def example_passing_launcher(temp_forge_dir, temp_workspace):
    """Create a passing example launcher script"""
    _, status_dir, log_dir = temp_forge_dir

    launcher_content = f"""#!/bin/bash
set -e

MODEL=""
WORKSPACE=""
SESSION_NAME=""

while [[ $# -gt 0 ]]; do
  case $1 in
    --model=*)
      MODEL="${{1#*=}}"
      shift
      ;;
    --workspace=*)
      WORKSPACE="${{1#*=}}"
      shift
      ;;
    --session-name=*)
      SESSION_NAME="${{1#*=}}"
      shift
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$MODEL" ]] || [[ -z "$WORKSPACE" ]] || [[ -z "$SESSION_NAME" ]]; then
  echo "Error: Missing required arguments" >&2
  exit 1
fi

# Create directories
mkdir -p {log_dir} {status_dir}

# Simulate worker process - use nohup to daemonize it properly
nohup sleep 3600 > /dev/null 2>&1 &
PID=$!

# Output worker metadata (JSON on stdout)
cat << EOF
{{
  "worker_id": "$SESSION_NAME",
  "pid": $PID,
  "status": "spawned",
  "launcher": "test-launcher",
  "timestamp": "$(date -Iseconds)"
}}
EOF

# Create status file
cat > {status_dir}/$SESSION_NAME.json << EOF
{{
  "worker_id": "$SESSION_NAME",
  "status": "active",
  "model": "$MODEL",
  "workspace": "$WORKSPACE",
  "pid": $PID,
  "started_at": "$(date -Iseconds)",
  "last_activity": "$(date -Iseconds)",
  "current_task": null,
  "tasks_completed": 0
}}
EOF

# Write initial log entry
echo "{{\\"timestamp\\": \\"$(date -Iseconds)\\", \\"level\\": \\"info\\", \\"worker_id\\": \\"$SESSION_NAME\\", \\"message\\": \\"Worker started\\"}}" \\
  >> {log_dir}/$SESSION_NAME.log

# Exit immediately after outputting JSON
exit 0
"""

    launcher_path = temp_workspace / "test-passing-launcher.sh"
    launcher_path.write_text(launcher_content)
    launcher_path.chmod(0o755)

    return launcher_path


@pytest.fixture
def example_failing_launcher(temp_workspace):
    """Create a launcher that fails with non-zero exit"""
    launcher_content = """#!/bin/bash
echo "Error: Cannot spawn worker" >&2
exit 1
"""

    launcher_path = temp_workspace / "test-failing-launcher.sh"
    launcher_path.write_text(launcher_content)
    launcher_path.chmod(0o755)

    return launcher_path


@pytest.fixture
def example_invalid_json_launcher(temp_workspace):
    """Create a launcher that outputs invalid JSON"""
    launcher_content = """#!/bin/bash
# Output invalid JSON
echo "this is not json"
exit 0
"""

    launcher_path = temp_workspace / "test-invalid-json-launcher.sh"
    launcher_path.write_text(launcher_content)
    launcher_path.chmod(0o755)

    return launcher_path


@pytest.fixture
def example_missing_fields_launcher(temp_workspace):
    """Create a launcher that outputs JSON without required fields"""
    launcher_content = """#!/bin/bash
# Output JSON missing required fields
echo '{"worker_id": "test-worker"}'
exit 0
"""

    launcher_path = temp_workspace / "test-missing-fields-launcher.sh"
    launcher_path.write_text(launcher_content)
    launcher_path.chmod(0o755)

    return launcher_path


@pytest.fixture
def example_wrong_status_launcher(temp_workspace):
    """Create a launcher that outputs wrong status value"""
    launcher_content = """#!/bin/bash
# Output JSON with wrong status
echo '{"worker_id": "test-worker", "pid": 12345, "status": "running"}'
exit 0
"""

    launcher_path = temp_workspace / "test-wrong-status-launcher.sh"
    launcher_path.write_text(launcher_content)
    launcher_path.chmod(0o755)

    return launcher_path


@pytest.fixture
def non_executable_launcher(temp_workspace):
    """Create a launcher that is not executable"""
    launcher_content = """#!/bin/bash
echo "test"
"""
    launcher_path = temp_workspace / "test-non-executable.sh"
    launcher_path.write_text(launcher_content)
    # Don't make it executable
    return launcher_path


@pytest.fixture
def example_timeout_launcher(temp_workspace):
    """Create a launcher that hangs (times out)"""
    launcher_content = """#!/bin/bash
# Hang forever
sleep 1000
"""
    launcher_path = temp_workspace / "test-timeout-launcher.sh"
    launcher_path.write_text(launcher_content)
    launcher_path.chmod(0o755)

    return launcher_path


# =============================================================================
# LauncherResult Tests
# =============================================================================


class TestLauncherResult:
    """Tests for LauncherResult dataclass"""

    def test_success_result_creation(self):
        """Test creating a successful LauncherResult"""
        result = LauncherResult(
            success=True,
            worker_id="test-worker",
            pid=12345,
            status="spawned",
        )

        assert result.success is True
        assert result.worker_id == "test-worker"
        assert result.pid == 12345
        assert result.status == "spawned"
        assert result.error is None
        assert result.error_type is None

    def test_failure_result_creation(self):
        """Test creating a failed LauncherResult"""
        result = LauncherResult(
            success=False,
            error_type=LauncherErrorType.NOT_FOUND,
            error="Launcher not found",
            guidance=["Check launcher path", "Verify installation"],
        )

        assert result.success is False
        assert result.error_type == LauncherErrorType.NOT_FOUND
        assert result.error == "Launcher not found"
        assert len(result.guidance) == 2

    def test_to_dict(self):
        """Test converting LauncherResult to dictionary"""
        result = LauncherResult(
            success=True,
            worker_id="test-worker",
            pid=12345,
            status="spawned",
        )

        result_dict = result.to_dict()

        assert result_dict["success"] is True
        assert result_dict["worker_id"] == "test-worker"
        assert result_dict["pid"] == 12345
        assert result_dict["status"] == "spawned"


# =============================================================================
# ProtocolValidationResult Tests
# =============================================================================


class TestProtocolValidationResult:
    """Tests for ProtocolValidationResult dataclass"""

    def test_valid_result(self):
        """Test creating a valid ProtocolValidationResult"""
        result = ProtocolValidationResult(valid=True)

        assert result.valid is True
        assert len(result.violations) == 0
        assert len(result.warnings) == 0
        assert result.score == 1.0

    def test_invalid_result(self):
        """Test creating an invalid ProtocolValidationResult"""
        result = ProtocolValidationResult(
            valid=False,
            violations=["Missing field: worker_id", "Invalid status value"],
            warnings=["Log file is empty"],
            score=0.5,
        )

        assert result.valid is False
        assert len(result.violations) == 2
        assert len(result.warnings) == 1
        assert result.score == 0.5


# =============================================================================
# LauncherConfig Tests
# =============================================================================


class TestLauncherConfig:
    """Tests for LauncherConfig dataclass"""

    def test_config_creation(self):
        """Test creating a LauncherConfig"""
        config = LauncherConfig(
            launcher_path="/path/to/launcher",
            model="sonnet",
            workspace="/path/to/workspace",
            session_name="test-worker",
        )

        assert str(config.launcher_path) == "/path/to/launcher"
        assert config.model == "sonnet"
        assert str(config.workspace) == "/path/to/workspace"
        assert config.session_name == "test-worker"
        assert config.timeout_seconds == 30
        assert config.validate_output is True
        assert config.check_files is True

    def test_path_expansion(self):
        """Test path expansion in LauncherConfig"""
        config = LauncherConfig(
            launcher_path="~/launcher",
            workspace="~/workspace",
            model="test",
            session_name="test",
        )

        # Paths should be expanded
        assert str(config.launcher_path).startswith("/")
        assert str(config.workspace).startswith("/")


# =============================================================================
# WorkerLauncher Tests
# =============================================================================


class TestWorkerLauncher:
    """Tests for WorkerLauncher class"""

    def test_initialization(self, temp_forge_dir):
        """Test WorkerLauncher initialization"""
        forge_dir, status_dir, log_dir = temp_forge_dir

        launcher = WorkerLauncher(
            forge_dir=forge_dir,
            status_dir=status_dir,
            log_dir=log_dir,
        )

        assert launcher.forge_dir == forge_dir
        assert launcher.status_dir == status_dir
        assert launcher.log_dir == log_dir

    def test_default_initialization(self):
        """Test WorkerLauncher initialization with defaults"""
        launcher = WorkerLauncher()

        assert launcher.forge_dir == Path.home() / ".forge"
        assert launcher.status_dir == Path.home() / ".forge" / "status"
        assert launcher.log_dir == Path.home() / ".forge" / "logs"

    def test_ensure_directories(self):
        """Test that ensure_directories creates required directories"""
        with tempfile.TemporaryDirectory() as tmpdir:
            forge_dir = Path(tmpdir) / "forge"
            status_dir = forge_dir / "status"
            log_dir = forge_dir / "logs"

            launcher = WorkerLauncher(
                forge_dir=forge_dir,
                status_dir=status_dir,
                log_dir=log_dir,
            )

            assert forge_dir.exists()
            assert status_dir.exists()
            assert log_dir.exists()

    def test_validate_launcher_not_found(self):
        """Test validation of non-existent launcher"""
        launcher = WorkerLauncher()
        result = launcher._validate_launcher(Path("/nonexistent/launcher"))

        assert result.valid is False
        assert "not found" in result.violations[0].lower()

    def test_validate_launcher_not_executable(self, temp_workspace):
        """Test validation of non-executable launcher"""
        launcher_path = temp_workspace / "non-executable.sh"
        launcher_path.write_text("#!/bin/bash\necho test")

        launcher = WorkerLauncher()
        result = launcher._validate_launcher(launcher_path)

        assert result.valid is False
        assert "not executable" in result.violations[0].lower()

    def test_validate_launcher_success(self, temp_workspace):
        """Test validation of valid launcher"""
        launcher_path = temp_workspace / "valid-launcher.sh"
        launcher_path.write_text("#!/bin/bash\necho test")
        launcher_path.chmod(0o755)

        launcher = WorkerLauncher()
        result = launcher._validate_launcher(launcher_path)

        assert result.valid is True
        assert len(result.violations) == 0

    def test_build_command_args(self, temp_workspace):
        """Test building command line arguments"""
        config = LauncherConfig(
            launcher_path=temp_workspace / "launcher.sh",
            model="sonnet",
            workspace=temp_workspace,
            session_name="test-worker",
        )

        launcher = WorkerLauncher()
        args = launcher._build_command_args(config)

        assert len(args) == 4
        assert str(temp_workspace / "launcher.sh") in args[0]
        assert "--model=sonnet" in args[1]
        assert f"--workspace={temp_workspace}" in args[2]
        assert "--session-name=test-worker" in args[3]

    def test_spawn_success(
        self, temp_forge_dir, temp_workspace, example_passing_launcher
    ):
        """Test successful worker spawn"""
        forge_dir, status_dir, log_dir = temp_forge_dir

        launcher = WorkerLauncher(
            forge_dir=forge_dir,
            status_dir=status_dir,
            log_dir=log_dir,
        )

        config = LauncherConfig(
            launcher_path=example_passing_launcher,
            model="sonnet",
            workspace=temp_workspace,
            session_name="test-success",
            timeout_seconds=15,
        )

        result = launcher.spawn(config)

        assert result.success is True
        assert result.worker_id == "test-success"
        assert result.pid is not None
        assert result.status == "spawned"
        assert result.error is None

        # Cleanup background process
        if result.pid:
            try:
                import signal
                os.kill(result.pid, signal.SIGTERM)
            except Exception:
                pass

    def test_spawn_launcher_not_found(self, temp_workspace):
        """Test spawn with non-existent launcher"""
        launcher = WorkerLauncher()

        config = LauncherConfig(
            launcher_path="/nonexistent/launcher",
            model="test",
            workspace=temp_workspace,
            session_name="test",
        )

        result = launcher.spawn(config)

        assert result.success is False
        assert result.error_type == LauncherErrorType.NOT_FOUND
        assert "not found" in result.error.lower()

    def test_spawn_launcher_not_executable(
        self, temp_workspace, non_executable_launcher
    ):
        """Test spawn with non-executable launcher"""
        launcher = WorkerLauncher()

        config = LauncherConfig(
            launcher_path=non_executable_launcher,
            model="test",
            workspace=temp_workspace,
            session_name="test",
        )

        result = launcher.spawn(config)

        assert result.success is False
        assert result.error_type == LauncherErrorType.NOT_EXECUTABLE

    def test_spawn_timeout(self, temp_workspace, example_timeout_launcher):
        """Test spawn with launcher timeout"""
        launcher = WorkerLauncher()

        config = LauncherConfig(
            launcher_path=example_timeout_launcher,
            model="test",
            workspace=temp_workspace,
            session_name="test",
            timeout_seconds=2,  # Short timeout
        )

        result = launcher.spawn(config)

        assert result.success is False
        assert result.error_type == LauncherErrorType.TIMEOUT
        assert result.error is not None
        assert "timeout" in str(result.error).lower() or "timed out" in str(result.error).lower()

    def test_spawn_nonzero_exit(
        self, temp_workspace, example_failing_launcher
    ):
        """Test spawn with non-zero exit code"""
        launcher = WorkerLauncher()

        config = LauncherConfig(
            launcher_path=example_failing_launcher,
            model="test",
            workspace=temp_workspace,
            session_name="test",
        )

        result = launcher.spawn(config)

        assert result.success is False
        assert result.error_type == LauncherErrorType.EXIT_CODE_NONZERO
        assert "code 1" in result.error
        assert result.exit_code == 1

    def test_spawn_invalid_json(
        self, temp_workspace, example_invalid_json_launcher
    ):
        """Test spawn with invalid JSON output"""
        launcher = WorkerLauncher()

        config = LauncherConfig(
            launcher_path=example_invalid_json_launcher,
            model="test",
            workspace=temp_workspace,
            session_name="test",
        )

        result = launcher.spawn(config)

        assert result.success is False
        assert result.error_type == LauncherErrorType.INVALID_OUTPUT
        assert result.error is not None
        # Check for json-related error
        assert "json" in str(result.error).lower()

    def test_spawn_missing_fields(
        self, temp_workspace, example_missing_fields_launcher
    ):
        """Test spawn with missing required fields"""
        launcher = WorkerLauncher()

        config = LauncherConfig(
            launcher_path=example_missing_fields_launcher,
            model="test",
            workspace=temp_workspace,
            session_name="test",
        )

        result = launcher.spawn(config)

        assert result.success is False
        assert result.error_type == LauncherErrorType.PROTOCOL_VIOLATION
        assert "Missing required fields" in result.error

    def test_spawn_wrong_status(
        self, temp_workspace, example_wrong_status_launcher
    ):
        """Test spawn with wrong status value"""
        launcher = WorkerLauncher()

        config = LauncherConfig(
            launcher_path=example_wrong_status_launcher,
            model="test",
            workspace=temp_workspace,
            session_name="test",
        )

        result = launcher.spawn(config)

        assert result.success is False
        assert result.error_type == LauncherErrorType.PROTOCOL_VIOLATION
        assert "Invalid status value" in result.error

    def test_check_required_files_success(
        self, temp_forge_dir, temp_workspace
    ):
        """Test checking required files with valid files"""
        forge_dir, status_dir, log_dir = temp_forge_dir

        # Create required files
        worker_id = "test-worker"
        status_file = status_dir / f"{worker_id}.json"
        log_file = log_dir / f"{worker_id}.log"

        # Write valid status file
        status_data = {
            "worker_id": worker_id,
            "status": "active",
            "model": "sonnet",
            "workspace": str(temp_workspace),
        }
        status_file.write_text(json.dumps(status_data))

        # Write log file
        log_file.write_text("test log entry\n")

        launcher = WorkerLauncher(
            forge_dir=forge_dir,
            status_dir=status_dir,
            log_dir=log_dir,
        )

        result = launcher._check_required_files(worker_id)

        assert result.valid is True
        assert len(result.violations) == 0

    def test_check_required_files_missing_status(self, temp_forge_dir):
        """Test checking required files with missing status file"""
        forge_dir, status_dir, log_dir = temp_forge_dir

        launcher = WorkerLauncher(
            forge_dir=forge_dir,
            status_dir=status_dir,
            log_dir=log_dir,
        )

        result = launcher._check_required_files("nonexistent-worker")

        assert result.valid is False
        assert "Status file not created" in result.violations[0]

    def test_check_required_files_invalid_status(
        self, temp_forge_dir, temp_workspace
    ):
        """Test checking required files with invalid status file"""
        forge_dir, status_dir, log_dir = temp_forge_dir

        worker_id = "test-worker"
        status_file = status_dir / f"{worker_id}.json"

        # Write invalid JSON
        status_file.write_text("not valid json")

        launcher = WorkerLauncher(
            forge_dir=forge_dir,
            status_dir=status_dir,
            log_dir=log_dir,
        )

        result = launcher._check_required_files(worker_id)

        assert result.valid is False
        assert "invalid JSON" in result.violations[0]

    def test_validate_protocol_compliance_success(
        self, temp_forge_dir, temp_workspace, example_passing_launcher
    ):
        """Test protocol compliance validation with valid launcher"""
        forge_dir, status_dir, log_dir = temp_forge_dir

        launcher = WorkerLauncher(
            forge_dir=forge_dir,
            status_dir=status_dir,
            log_dir=log_dir,
        )

        result = launcher.validate_protocol_compliance(example_passing_launcher)

        assert result.valid is True
        assert len(result.violations) == 0

    def test_validate_protocol_compliance_failure(
        self, temp_workspace, example_failing_launcher
    ):
        """Test protocol compliance validation with failing launcher"""
        launcher = WorkerLauncher()

        result = launcher.validate_protocol_compliance(
            example_failing_launcher,
        )

        assert result.valid is False
        assert len(result.violations) > 0

    def test_cleanup_test_worker(self, temp_forge_dir):
        """Test cleanup of test worker artifacts"""
        forge_dir, status_dir, log_dir = temp_forge_dir

        worker_id = "test-cleanup"
        status_file = status_dir / f"{worker_id}.json"
        log_file = log_dir / f"{worker_id}.log"

        # Create test files
        status_file.write_text("{}")
        log_file.write_text("test log\n")

        launcher = WorkerLauncher(
            forge_dir=forge_dir,
            status_dir=status_dir,
            log_dir=log_dir,
        )

        launcher._cleanup_test_worker(worker_id)

        # Files should be removed
        assert not status_file.exists()
        assert not log_file.exists()


# =============================================================================
# Convenience Functions Tests
# =============================================================================


class TestConvenienceFunctions:
    """Tests for convenience functions"""

    def test_spawn_worker_success(
        self, temp_forge_dir, temp_workspace, example_passing_launcher
    ):
        """Test spawn_worker convenience function"""
        forge_dir, status_dir, log_dir = temp_forge_dir

        # Patch default directories
        with patch("forge.launcher.WorkerLauncher") as MockLauncher:
            mock_instance = Mock()
            mock_instance.spawn.return_value = LauncherResult(
                success=True,
                worker_id="test",
                pid=12345,
                status="spawned",
            )
            MockLauncher.return_value = mock_instance

            result = spawn_worker(
                launcher_path=example_passing_launcher,
                model="sonnet",
                workspace=temp_workspace,
                session_name="test",
            )

            assert result.success is True
            mock_instance.spawn.assert_called_once()

    def test_validate_launcher_convenience(
        self, temp_workspace, example_passing_launcher
    ):
        """Test validate_launcher convenience function"""
        with patch("forge.launcher.WorkerLauncher") as MockLauncher:
            mock_instance = Mock()
            mock_instance.validate_protocol_compliance.return_value = (
                ProtocolValidationResult(valid=True)
            )
            MockLauncher.return_value = mock_instance

            result = validate_launcher(example_passing_launcher)

            assert result.valid is True
            mock_instance.validate_protocol_compliance.assert_called_once()
