"""
FORGE Worker Launcher Integration

Implements subprocess spawning of launcher scripts with protocol compliance
validation and graceful error handling per ADR 0014.

Protocol Reference: docs/INTEGRATION_GUIDE.md
Error Handling: docs/adr/0014-error-handling-strategy.md
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from pathlib import Path
from typing import Any

import subprocess


# =============================================================================
# Result Types
# =============================================================================


class LauncherErrorType(Enum):
    """Categorization of launcher errors for targeted handling"""
    NOT_FOUND = "not_found"
    NOT_EXECUTABLE = "not_executable"
    TIMEOUT = "timeout"
    INVALID_OUTPUT = "invalid_output"
    EXIT_CODE_NONZERO = "exit_code_nonzero"
    PROTOCOL_VIOLATION = "protocol_violation"
    STATUS_FILE_MISSING = "status_file_missing"
    STATUS_FILE_INVALID = "status_file_invalid"
    LOG_FILE_MISSING = "log_file_missing"


@dataclass
class LauncherResult:
    """
    Result of a launcher execution attempt.

    Attributes:
        success: True if worker was spawned successfully
        worker_id: Worker identifier (if successful)
        pid: Process ID of spawned worker (if successful)
        status: Worker status from launcher output
        error_type: Category of error (if failed)
        error: Human-readable error message (if failed)
        stdout: Captured stdout from launcher
        stderr: Captured stderr from launcher
        exit_code: Launcher process exit code
        guidance: List of actionable suggestions for recovery
    """
    success: bool
    worker_id: str | None = None
    pid: int | None = None
    status: str | None = None
    error_type: LauncherErrorType | None = None
    error: str | None = None
    stdout: str | None = None
    stderr: str | None = None
    exit_code: int | None = None
    guidance: list[str] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization"""
        return {
            "success": self.success,
            "worker_id": self.worker_id,
            "pid": self.pid,
            "status": self.status,
            "error_type": self.error_type.value if self.error_type else None,
            "error": self.error,
            "stdout": self.stdout,
            "stderr": self.stderr,
            "exit_code": self.exit_code,
            "guidance": self.guidance,
        }


@dataclass
class ProtocolValidationResult:
    """
    Result of launcher protocol compliance validation.

    Attributes:
        valid: True if launcher follows protocol correctly
        violations: List of protocol violations found
        warnings: List of non-critical issues
        guidance: List of actionable suggestions for recovery
        score: Compliance score (0.0 to 1.0)
    """
    valid: bool
    violations: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    guidance: list[str] = field(default_factory=list)
    score: float = 1.0


# =============================================================================
# Launcher Configuration
# =============================================================================


@dataclass
class LauncherConfig:
    """
    Configuration for spawning a worker via launcher.

    Attributes:
        launcher_path: Path to the launcher executable
        model: Model identifier (e.g., "sonnet", "opus", "haiku")
        workspace: Path to the workspace directory
        session_name: Unique session name for the worker
        timeout_seconds: Maximum time to wait for launcher (default 30)
        validate_output: Whether to validate JSON output format
        check_files: Whether to verify status/log files created
    """
    launcher_path: Path | str
    model: str
    workspace: Path | str
    session_name: str
    timeout_seconds: int = 30
    validate_output: bool = True
    check_files: bool = True

    def __post_init__(self):
        """Convert paths to Path objects"""
        self.launcher_path = Path(self.launcher_path).expanduser()
        self.workspace = Path(self.workspace).expanduser()


# =============================================================================
# Main Launcher Integration
# =============================================================================


class WorkerLauncher:
    """
    Manages worker launcher execution with protocol validation and error handling.

    Implements the launcher protocol as specified in INTEGRATION_GUIDE.md:
    - Spawns launcher subprocess with required arguments
    - Parses JSON output for worker metadata
    - Validates protocol compliance
    - Handles failures gracefully per ADR 0014
    """

    # Default paths
    DEFAULT_FORGE_DIR = Path.home() / ".forge"
    DEFAULT_STATUS_DIR = DEFAULT_FORGE_DIR / "status"
    DEFAULT_LOG_DIR = DEFAULT_FORGE_DIR / "logs"

    def __init__(
        self,
        forge_dir: Path | str | None = None,
        status_dir: Path | str | None = None,
        log_dir: Path | str | None = None,
    ):
        """
        Initialize the worker launcher.

        Args:
            forge_dir: Base FORGE directory (defaults to ~/.forge)
            status_dir: Directory for worker status files
            log_dir: Directory for worker log files
        """
        self.forge_dir = Path(forge_dir) if forge_dir else self.DEFAULT_FORGE_DIR
        self.status_dir = Path(status_dir) if status_dir else self.DEFAULT_STATUS_DIR
        self.log_dir = Path(log_dir) if log_dir else self.DEFAULT_LOG_DIR

        # Ensure directories exist
        self._ensure_directories()

    def _ensure_directories(self) -> None:
        """Ensure required directories exist"""
        for directory in [self.forge_dir, self.status_dir, self.log_dir]:
            directory.mkdir(parents=True, exist_ok=True)

    def spawn(self, config: LauncherConfig) -> LauncherResult:
        """
        Spawn a worker using the configured launcher.

        Args:
            config: Launcher configuration

        Returns:
            LauncherResult with success status and error details if failed
        """
        # Validate launcher exists and is executable
        validation_result = self._validate_launcher(config.launcher_path)
        if not validation_result.valid:
            return self._validation_error_to_result(validation_result, config)

        # Build command arguments
        args = self._build_command_args(config)

        # Execute launcher
        try:
            process_result = subprocess.run(
                args,
                capture_output=True,
                text=True,
                timeout=config.timeout_seconds,
                check=False,  # We handle exit codes manually
            )
        except subprocess.TimeoutExpired as e:
            return LauncherResult(
                success=False,
                error_type=LauncherErrorType.TIMEOUT,
                error=f"Launcher timed out after {config.timeout_seconds} seconds",
                stdout=e.stdout.decode() if e.stdout else None,
                stderr=e.stderr.decode() if e.stderr else None,
                exit_code=-1,
                guidance=[
                    "Check if launcher hangs on input (missing arguments)",
                    "Verify launcher doesn't require interactive input",
                    "Test launcher manually: "
                    + " ".join([str(a) for a in args]),
                ],
            )
        except Exception as e:
            return LauncherResult(
                success=False,
                error_type=LauncherErrorType.EXIT_CODE_NONZERO,
                error=f"Unexpected error executing launcher: {e}",
                guidance=[
                    "Check launcher permissions: chmod +x "
                    + str(config.launcher_path),
                    "Verify launcher is a valid script or binary",
                ],
            )

        # Check exit code
        if process_result.returncode != 0:
            return LauncherResult(
                success=False,
                error_type=LauncherErrorType.EXIT_CODE_NONZERO,
                error=f"Launcher exited with code {process_result.returncode}",
                stdout=process_result.stdout,
                stderr=process_result.stderr,
                exit_code=process_result.returncode,
                guidance=self._generate_exit_code_guidance(config, process_result),
            )

        # Parse JSON output
        try:
            output_data = json.loads(process_result.stdout)
        except json.JSONDecodeError as e:
            return LauncherResult(
                success=False,
                error_type=LauncherErrorType.INVALID_OUTPUT,
                error=f"Invalid launcher output (not JSON): {str(e)[:100]}",
                stdout=process_result.stdout,
                stderr=process_result.stderr,
                exit_code=process_result.returncode,
                guidance=[
                    "Launcher must output JSON on stdout",
                    f"Received: {process_result.stdout[:200]}",
                    "Check launcher script for echo/print statements",
                    "Ensure launcher outputs JSON after worker spawn",
                ],
            )

        # Validate output has required fields
        required_fields = ["worker_id", "pid", "status"]
        missing_fields = [
            f for f in required_fields if f not in output_data
        ]

        if missing_fields:
            return LauncherResult(
                success=False,
                error_type=LauncherErrorType.PROTOCOL_VIOLATION,
                error=f"Missing required fields in output: {', '.join(missing_fields)}",
                stdout=process_result.stdout,
                stderr=process_result.stderr,
                exit_code=process_result.returncode,
                guidance=[
                    "Launcher output must include: worker_id, pid, status",
                    f"Missing: {', '.join(missing_fields)}",
                    "Reference: docs/INTEGRATION_GUIDE.md",
                ],
            )

        # Validate status value
        if output_data["status"] != "spawned":
            return LauncherResult(
                success=False,
                error_type=LauncherErrorType.PROTOCOL_VIOLATION,
                error=f'Invalid status value: "{output_data["status"]}" (expected "spawned")',
                stdout=process_result.stdout,
                stderr=process_result.stderr,
                exit_code=process_result.returncode,
                guidance=[
                    'Status field must be "spawned"',
                    'Check launcher output: "status": "spawned"',
                ],
            )

        # Optional: Check files were created
        if config.check_files:
            file_check_result = self._check_required_files(
                output_data["worker_id"]
            )
            if not file_check_result.valid:
                return LauncherResult(
                    success=False,
                    worker_id=output_data.get("worker_id"),
                    pid=output_data.get("pid"),
                    status=output_data.get("status"),
                    error_type=LauncherErrorType.STATUS_FILE_MISSING,
                    error="Required files not created",
                    stdout=process_result.stdout,
                    stderr=process_result.stderr,
                    exit_code=process_result.returncode,
                    guidance=file_check_result.violations,
                )

        # Success!
        return LauncherResult(
            success=True,
            worker_id=output_data["worker_id"],
            pid=int(output_data["pid"]),
            status=output_data["status"],
            stdout=process_result.stdout,
            stderr=process_result.stderr,
            exit_code=process_result.returncode,
        )

    def _validate_launcher(
        self, launcher_path: Path
    ) -> ProtocolValidationResult:
        """
        Validate launcher executable exists and is executable.

        Args:
            launcher_path: Path to the launcher script

        Returns:
            ProtocolValidationResult with any violations
        """
        result = ProtocolValidationResult(valid=True)

        # Check if launcher exists
        if not launcher_path.exists():
            result.valid = False
            result.violations.append(
                f"Launcher not found: {launcher_path}"
            )
            result.guidance = [
                "Verify launcher path in config",
                "Check launcher is installed in ~/.forge/launchers/",
                f"Create launcher: {launcher_path}",
            ]
            return result

        # Check if launcher is executable
        if not os.access(launcher_path, os.X_OK):
            result.valid = False
            result.violations.append(
                f"Launcher not executable: {launcher_path}"
            )
            result.guidance = [
                "Make launcher executable: chmod +x " + str(launcher_path),
                "Check launcher has shebang (#!/bin/bash or #!/usr/bin/env python3)",
            ]
            return result

        return result

    def _build_command_args(self, config: LauncherConfig) -> list[str]:
        """
        Build command line arguments for launcher.

        Args:
            config: Launcher configuration

        Returns:
            List of command line arguments
        """
        return [
            str(config.launcher_path),
            f"--model={config.model}",
            f"--workspace={config.workspace}",
            f"--session-name={config.session_name}",
        ]

    def _generate_exit_code_guidance(
        self,
        config: LauncherConfig,
        process_result: subprocess.CompletedProcess[str],
    ) -> list[str]:
        """
        Generate actionable guidance for non-zero exit codes.

        Args:
            config: Launcher configuration used
            process_result: Completed process result

        Returns:
            List of actionable suggestions
        """
        guidance = [
            "Check launcher stderr output for error details",
            "Verify workspace path exists: " + str(config.workspace),
            f"Test launcher manually: {config.launcher_path} --model={config.model} "
            f"--workspace=/tmp --session-name=test",
        ]

        # Add stderr to guidance if present
        if process_result.stderr:
            guidance.append(f"Stderr: {process_result.stderr[:200]}")

        return guidance

    def _check_required_files(
        self, worker_id: str
    ) -> ProtocolValidationResult:
        """
        Check that required status and log files were created.

        Args:
            worker_id: Worker identifier

        Returns:
            ProtocolValidationResult with any violations
        """
        result = ProtocolValidationResult(valid=True)

        # Check status file
        status_file = self.status_dir / f"{worker_id}.json"
        if not status_file.exists():
            result.valid = False
            result.violations.append(
                f"Status file not created: {status_file}"
            )
        else:
            # Try to validate status file content
            try:
                with open(status_file) as f:
                    status_data = json.load(f)

                # Check required fields
                required_fields = ["worker_id", "status", "model", "workspace"]
                missing = [f for f in required_fields if f not in status_data]

                if missing:
                    result.valid = False
                    result.violations.append(
                        f"Status file missing fields: {', '.join(missing)}"
                    )
            except json.JSONDecodeError as e:
                result.valid = False
                result.violations.append(
                    f"Status file has invalid JSON: {str(e)[:50]}"
                )

        # Check log file
        log_file = self.log_dir / f"{worker_id}.log"
        if not log_file.exists():
            result.warnings.append(
                f"Log file not created: {log_file}"
            )
        elif log_file.stat().st_size == 0:
            result.warnings.append(
                f"Log file is empty: {log_file}"
            )

        return result

    def _validation_error_to_result(
        self,
        validation_result: ProtocolValidationResult,
        config: LauncherConfig,
    ) -> LauncherResult:
        """
        Convert validation error to LauncherResult.

        Args:
            validation_result: Validation result from _validate_launcher
            config: Launcher configuration

        Returns:
            LauncherResult with error details
        """
        # Determine error type from first violation
        if "not found" in validation_result.violations[0].lower():
            error_type = LauncherErrorType.NOT_FOUND
        elif "not executable" in validation_result.violations[0].lower():
            error_type = LauncherErrorType.NOT_EXECUTABLE
        else:
            error_type = LauncherErrorType.PROTOCOL_VIOLATION

        return LauncherResult(
            success=False,
            error_type=error_type,
            error=validation_result.violations[0],
            guidance=validation_result.guidance
            if validation_result.guidance
            else validation_result.violations,
        )

    def validate_protocol_compliance(
        self,
        launcher_path: Path | str,
        test_model: str = "test",
        test_workspace: str = "/tmp/forge-test",
        test_session: str = "forge-protocol-test",
    ) -> ProtocolValidationResult:
        """
        Validate launcher protocol compliance by running a test spawn.

        Args:
            launcher_path: Path to the launcher to validate
            test_model: Model name to use for testing
            test_workspace: Workspace path to use for testing
            test_session: Session name to use for testing

        Returns:
            ProtocolValidationResult with compliance details
        """
        launcher_path = Path(launcher_path).expanduser()
        result = ProtocolValidationResult(valid=True)

        # First check launcher is valid
        validation = self._validate_launcher(launcher_path)
        if not validation.valid:
            return validation

        # Create test workspace
        test_workspace_path = Path(test_workspace)
        test_workspace_path.mkdir(parents=True, exist_ok=True)

        # Try to spawn a test worker
        config = LauncherConfig(
            launcher_path=launcher_path,
            model=test_model,
            workspace=test_workspace_path,
            session_name=test_session,
            timeout_seconds=15,
        )

        spawn_result = self.spawn(config)

        if not spawn_result.success:
            result.valid = False
            result.violations.append(
                f"Failed to spawn test worker: {spawn_result.error}"
            )
            if spawn_result.guidance:
                result.guidance.extend(spawn_result.guidance)
        else:
            # Success - validate the output format
            result.warnings.append(
                "Launcher protocol compliance validated successfully"
            )

        # Cleanup test artifacts
        self._cleanup_test_worker(test_session)

        return result

    def _cleanup_test_worker(self, worker_id: str) -> None:
        """
        Cleanup test worker artifacts.

        Args:
            worker_id: Worker identifier to cleanup
        """
        # Remove test files
        status_file = self.status_dir / f"{worker_id}.json"
        log_file = self.log_dir / f"{worker_id}.log"

        for path in [status_file, log_file]:
            if path.exists():
                try:
                    path.unlink()
                except Exception:
                    pass

        # Try to kill the worker process
        # (This is best-effort as the PID may have been recycled)
        try:
            if status_file.exists():
                with open(status_file) as f:
                    status = json.load(f)
                    pid = status.get("pid")
                    if pid:
                        os.kill(pid, 15)  # SIGTERM
        except Exception:
            pass


# =============================================================================
# Convenience Functions
# =============================================================================


def spawn_worker(
    launcher_path: Path | str,
    model: str,
    workspace: Path | str,
    session_name: str,
    timeout_seconds: int = 30,
) -> LauncherResult:
    """
    Convenience function to spawn a worker.

    Args:
        launcher_path: Path to the launcher executable
        model: Model identifier
        workspace: Path to the workspace directory
        session_name: Unique session name for the worker
        timeout_seconds: Maximum time to wait for launcher

    Returns:
        LauncherResult with success status and details
    """
    launcher = WorkerLauncher()
    config = LauncherConfig(
        launcher_path=launcher_path,
        model=model,
        workspace=workspace,
        session_name=session_name,
        timeout_seconds=timeout_seconds,
    )
    return launcher.spawn(config)


def validate_launcher(
    launcher_path: Path | str,
) -> ProtocolValidationResult:
    """
    Convenience function to validate launcher protocol compliance.

    Args:
        launcher_path: Path to the launcher to validate

    Returns:
        ProtocolValidationResult with compliance details
    """
    launcher = WorkerLauncher()
    return launcher.validate_protocol_compliance(launcher_path)
