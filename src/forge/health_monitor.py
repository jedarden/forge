"""
FORGE Worker Health Monitor Module

Implements comprehensive worker health checking per ADR 0014:
- PID existence check (process liveness)
- Log activity check (recent activity within 5 minutes)
- Status file validation (format and consistency)
- Tmux session aliveness check (for tmux-based workers)

Marks workers as failed when health checks fail with clear error messages.
No auto-recovery - user decides per ADR 0014.

Reference: docs/adr/0014-error-handling-strategy.md
Architecture: docs/adr/0008-real-time-update-architecture.md
"""

from __future__ import annotations

import asyncio
import os
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from enum import Enum
from pathlib import Path
from typing import Any
import subprocess

# Import status watcher types
from forge.status_watcher import (
    WorkerStatusFile,
    WorkerStatusValue,
    StatusFileParser,
)


# =============================================================================
# Health Status Data Models
# =============================================================================


class HealthCheckType(Enum):
    """Types of health checks"""
    PID_EXISTS = "pid_exists"
    LOG_ACTIVITY = "log_activity"
    STATUS_FILE = "status_file"
    TMUX_SESSION = "tmux_session"


class ErrorType(Enum):
    """Types of worker errors"""
    DEAD_PROCESS = "dead_process"
    STALE_LOG = "stale_log"
    CORRUPTED_STATUS = "corrupted_status"
    MISSING_SESSION = "missing_session"
    UNKNOWN = "unknown"


@dataclass
class HealthCheckResult:
    """
    Result of a single health check.

    Attributes:
        check_type: Type of health check performed
        passed: Whether the check passed
        error_type: Type of error if check failed
        error_message: Human-readable error message
        timestamp: When the check was performed
    """
    check_type: HealthCheckType
    passed: bool
    error_type: ErrorType | None = None
    error_message: str | None = None
    timestamp: datetime = field(default_factory=datetime.now)

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization"""
        return {
            "check_type": self.check_type.value,
            "passed": self.passed,
            "error_type": self.error_type.value if self.error_type else None,
            "error_message": self.error_message,
            "timestamp": self.timestamp.isoformat(),
        }


@dataclass
class WorkerHealthStatus:
    """
    Overall health status of a worker.

    Attributes:
        worker_id: Worker identifier
        is_healthy: Whether all health checks passed
        health_score: Float from 0.0 (unhealthy) to 1.0 (healthy)
        checks: List of individual health check results
        last_check: When health was last checked
        failed_checks: List of check types that failed
        primary_error: Primary error message for UI display
        guidance: Suggested actions for user
    """
    worker_id: str
    is_healthy: bool
    health_score: float
    checks: list[HealthCheckResult] = field(default_factory=list)
    last_check: datetime = field(default_factory=datetime.now)
    failed_checks: list[HealthCheckType] = field(default_factory=list)
    primary_error: str | None = None
    guidance: list[str] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization"""
        return {
            "worker_id": self.worker_id,
            "is_healthy": self.is_healthy,
            "health_score": self.health_score,
            "checks": [c.to_dict() for c in self.checks],
            "last_check": self.last_check.isoformat(),
            "failed_checks": [c.value for c in self.failed_checks],
            "primary_error": self.primary_error,
            "guidance": self.guidance,
        }


# =============================================================================
# Health Check Implementation
# =============================================================================


class WorkerHealthMonitor:
    """
    Monitor worker health via multiple liveness and activity checks.

    Implements ADR 0014 error handling:
    - No silent failures
    - Clear error messages with actionable guidance
    - No automatic recovery
    - Graceful degradation
    """

    # Health check thresholds
    LOG_ACTIVITY_THRESHOLD_SECONDS = 5 * 60  # 5 minutes
    HEALTH_CHECK_INTERVAL = 10  # seconds

    def __init__(
        self,
        status_dir: Path | str,
        log_dir: Path | str,
    ):
        """
        Initialize the health monitor.

        Args:
            status_dir: Directory containing worker status files
            log_dir: Directory containing worker log files
        """
        self.status_dir = Path(status_dir).expanduser()
        self.log_dir = Path(log_dir).expanduser()
        self.parser = StatusFileParser()

    def check_worker_health(self, worker_id: str) -> WorkerHealthStatus:
        """
        Perform comprehensive health check on a worker.

        Runs all health checks:
        1. PID existence check
        2. Log activity check
        3. Status file validation
        4. Tmux session check (if applicable)

        Args:
            worker_id: Worker identifier

        Returns:
            WorkerHealthStatus with check results and guidance
        """
        checks: list[HealthCheckResult] = []
        failed_checks: list[HealthCheckType] = []

        # Load status file first
        status_file = self.status_dir / f"{worker_id}.json"
        status = self.parser.parse(status_file)

        # Run health checks
        pid_result = self._check_pid_exists(status)
        checks.append(pid_result)
        if not pid_result.passed:
            failed_checks.append(HealthCheckType.PID_EXISTS)

        log_result = self._check_log_activity(worker_id, status)
        checks.append(log_result)
        if not log_result.passed:
            failed_checks.append(HealthCheckType.LOG_ACTIVITY)

        status_result = self._check_status_file(status)
        checks.append(status_result)
        if not status_result.passed:
            failed_checks.append(HealthCheckType.STATUS_FILE)

        # Tmux check only if worker has tmux session info
        tmux_result = self._check_tmux_session(worker_id, status)
        if tmux_result is not None:
            checks.append(tmux_result)
            if not tmux_result.passed:
                failed_checks.append(HealthCheckType.TMUX_SESSION)

        # Calculate health score (0.0 to 1.0)
        health_score = sum(1 for c in checks if c.passed) / len(checks) if checks else 0.0
        is_healthy = health_score == 1.0

        # Generate primary error and guidance
        primary_error, guidance = self._generate_error_messages(failed_checks, status, worker_id)

        return WorkerHealthStatus(
            worker_id=worker_id,
            is_healthy=is_healthy,
            health_score=health_score,
            checks=checks,
            failed_checks=failed_checks,
            primary_error=primary_error,
            guidance=guidance,
        )

    def _check_pid_exists(self, status: WorkerStatusFile) -> HealthCheckResult:
        """
        Check if worker process is still running.

        Uses os.kill(pid, 0) which doesn't actually send a signal
        but checks if the process exists.

        Args:
            status: Worker status file data

        Returns:
            HealthCheckResult for PID check
        """
        if status.pid is None:
            return HealthCheckResult(
                check_type=HealthCheckType.PID_EXISTS,
                passed=False,
                error_type=ErrorType.UNKNOWN,
                error_message="No PID in status file - cannot check process liveness",
            )

        try:
            # Signal 0 checks if process exists without sending actual signal
            os.kill(status.pid, 0)
            return HealthCheckResult(
                check_type=HealthCheckType.PID_EXISTS,
                passed=True,
            )
        except OSError:
            # Process doesn't exist
            return HealthCheckResult(
                check_type=HealthCheckType.PID_EXISTS,
                passed=False,
                error_type=ErrorType.DEAD_PROCESS,
                error_message=f"Worker process died (PID {status.pid} no longer exists)",
            )
        except Exception as e:
            return HealthCheckResult(
                check_type=HealthCheckType.PID_EXISTS,
                passed=False,
                error_type=ErrorType.UNKNOWN,
                error_message=f"Failed to check process: {str(e)[:50]}",
            )

    def _check_log_activity(
        self,
        worker_id: str,
        status: WorkerStatusFile,
    ) -> HealthCheckResult:
        """
        Check if worker has recent log activity.

        Checks if the last log entry is within 5 minutes.

        Args:
            worker_id: Worker identifier
            status: Worker status file data

        Returns:
            HealthCheckResult for log activity check
        """
        log_file = self.log_dir / f"{worker_id}.log"

        if not log_file.exists():
            return HealthCheckResult(
                check_type=HealthCheckType.LOG_ACTIVITY,
                passed=False,
                error_type=ErrorType.UNKNOWN,
                error_message=f"Log file not found: {log_file}",
            )

        try:
            # Get last modification time
            mtime = datetime.fromtimestamp(log_file.stat().st_mtime)
            age_seconds = (datetime.now() - mtime).total_seconds()

            if age_seconds > self.LOG_ACTIVITY_THRESHOLD_SECONDS:
                age_minutes = int(age_seconds / 60)
                return HealthCheckResult(
                    check_type=HealthCheckType.LOG_ACTIVITY,
                    passed=False,
                    error_type=ErrorType.STALE_LOG,
                    error_message=f"No log activity for {age_minutes} minutes (last: {mtime.strftime('%H:%M:%S')})",
                )

            return HealthCheckResult(
                check_type=HealthCheckType.LOG_ACTIVITY,
                passed=True,
            )

        except Exception as e:
            return HealthCheckResult(
                check_type=HealthCheckType.LOG_ACTIVITY,
                passed=False,
                error_type=ErrorType.UNKNOWN,
                error_message=f"Failed to check log activity: {str(e)[:50]}",
            )

    def _check_status_file(self, status: WorkerStatusFile) -> HealthCheckResult:
        """
        Check if status file is valid.

        Checks for parsing errors, missing fields, and consistency.

        Args:
            status: Worker status file data

        Returns:
            HealthCheckResult for status file check
        """
        # Check if parser detected errors
        if status.error is not None:
            return HealthCheckResult(
                check_type=HealthCheckType.STATUS_FILE,
                passed=False,
                error_type=ErrorType.CORRUPTED_STATUS,
                error_message=status.error,
            )

        # Check status consistency
        if status.status == WorkerStatusValue.ACTIVE:
            # Active workers should have recent last_activity
            if status.last_activity:
                try:
                    last_activity = datetime.fromisoformat(
                        status.last_activity.replace('Z', '+00:00')
                    )
                    age_minutes = (datetime.now(last_activity.tzinfo) - last_activity).total_seconds() / 60

                    if age_minutes > 10:
                        return HealthCheckResult(
                            check_type=HealthCheckType.STATUS_FILE,
                            passed=False,
                            error_type=ErrorType.STALE_LOG,
                            error_message=f"Worker marked active but last_activity is {age_minutes:.0f} minutes old",
                        )
                except:
                    pass

        return HealthCheckResult(
            check_type=HealthCheckType.STATUS_FILE,
            passed=True,
        )

    def _check_tmux_session(
        self,
        worker_id: str,
        status: WorkerStatusFile,
    ) -> HealthCheckResult | None:
        """
        Check if tmux session exists (for tmux-based workers).

        This check is optional - only run if we can determine session name.

        Args:
            worker_id: Worker identifier
            status: Worker status file data

        Returns:
            HealthCheckResult for tmux check, or None if not applicable
        """
        # Try to determine session name
        # Common patterns: worker_id, or from raw_data
        session_name = worker_id

        # Check if there's a session name in raw data
        if status.raw_data:
            session_name = status.raw_data.get("tmux_session", session_name)

        try:
            # Run tmux has-session command
            result = subprocess.run(
                ["tmux", "has-session", "-t", session_name],
                capture_output=True,
                timeout=5,
            )

            if result.returncode == 0:
                return HealthCheckResult(
                    check_type=HealthCheckType.TMUX_SESSION,
                    passed=True,
                )
            else:
                return HealthCheckResult(
                    check_type=HealthCheckType.TMUX_SESSION,
                    passed=False,
                    error_type=ErrorType.MISSING_SESSION,
                    error_message=f"Tmux session '{session_name}' not found",
                )

        except FileNotFoundError:
            # tmux not installed - skip this check
            return None
        except subprocess.TimeoutExpired:
            return HealthCheckResult(
                check_type=HealthCheckType.TMUX_SESSION,
                passed=False,
                error_type=ErrorType.UNKNOWN,
                error_message="Tmux check timed out",
            )
        except Exception as e:
            return HealthCheckResult(
                check_type=HealthCheckType.TMUX_SESSION,
                passed=False,
                error_type=ErrorType.UNKNOWN,
                error_message=f"Failed to check tmux: {str(e)[:50]}",
            )

    def _generate_error_messages(
        self,
        failed_checks: list[HealthCheckType],
        status: WorkerStatusFile,
        worker_id: str,
    ) -> tuple[str | None, list[str]]:
        """
        Generate primary error message and user guidance per ADR 0014.

        Args:
            failed_checks: List of failed health check types
            status: Worker status file data
            worker_id: Worker identifier

        Returns:
            Tuple of (primary_error, guidance_list)
        """
        if not failed_checks:
            return None, []

        # Map error types to messages and guidance
        guidance: list[str] = []

        # Check for dead process (most critical)
        if HealthCheckType.PID_EXISTS in failed_checks:
            primary_error = "Worker process died"

            if status.pid:
                guidance.extend([
                    f"Check process: ps -p {status.pid}",
                    f"View logs: :logs {worker_id}",
                    "Restart worker manually",
                ])
            else:
                guidance.extend([
                    "Worker has no PID - may not have started properly",
                    f"View logs: :logs {worker_id}",
                    "Check worker launcher configuration",
                ])

        # Check for stale log
        elif HealthCheckType.LOG_ACTIVITY in failed_checks:
            primary_error = "Worker appears stuck (no recent activity)"

            guidance.extend([
                f"Attach to worker: tmux attach -t {worker_id}",
                f"View logs: :logs {worker_id}",
                "Force restart if needed: :restart --force",
            ])

        # Check for corrupted status
        elif HealthCheckType.STATUS_FILE in failed_checks and status.error:
            primary_error = "Worker status file corrupted"

            guidance.extend([
                f"Error: {status.error}",
                f"Delete status: rm ~/.forge/status/{worker_id}.json",
                "Restart worker to regenerate status file",
            ])

        # Check for missing tmux session
        elif HealthCheckType.TMUX_SESSION in failed_checks:
            primary_error = "Worker tmux session not found"

            guidance.extend([
                "Check if tmux is installed: tmux -V",
                "List sessions: tmux list-sessions",
                "Restart worker to create new session",
            ])

        else:
            primary_error = "Worker health check failed"
            guidance.extend([
                "Run manual health check",
                f"View logs: :logs {worker_id}",
                "Check system resources: htop",
            ])

        return primary_error, guidance


# =============================================================================
# Health Monitoring Loop
# =============================================================================


class HealthMonitoringLoop:
    """
    Periodic health checking loop for all workers.

    Runs health checks every 10 seconds on all active/idle workers
    and marks them as failed when health checks fail.
    """

    def __init__(
        self,
        health_monitor: WorkerHealthMonitor,
        status_dir: Path | str,
        on_worker_unhealthy: callable,
    ):
        """
        Initialize the health monitoring loop.

        Args:
            health_monitor: Worker health monitor instance
            status_dir: Directory containing status files
            on_worker_unhealthy: Callback(worker_id, health_status) when worker is unhealthy
        """
        self.health_monitor = health_monitor
        self.status_dir = Path(status_dir).expanduser()
        self.on_worker_unhealthy = on_worker_unhealthy

        self._running = False
        self._task: asyncio.Task[None] | None = None

    async def start(self, interval_seconds: int = 10) -> None:
        """
        Start the health monitoring loop.

        Args:
            interval_seconds: Seconds between health checks (default 10)
        """
        if self._running:
            return

        self._running = True
        self._task = asyncio.create_task(self._health_check_loop(interval_seconds))

    async def stop(self) -> None:
        """Stop the health monitoring loop"""
        self._running = False

        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
            self._task = None

    async def _health_check_loop(self, interval_seconds: int) -> None:
        """Main health check loop"""
        while self._running:
            try:
                await self._run_health_checks()
                await asyncio.sleep(interval_seconds)
            except asyncio.CancelledError:
                break
            except Exception as e:
                import sys
                print(f"[FORGE] Health check loop error: {e}", file=sys.stderr)
                await asyncio.sleep(interval_seconds)

    async def _run_health_checks(self) -> None:
        """Run health checks on all workers"""
        if not self.status_dir.exists():
            return

        # Get all status files
        status_files = list(self.status_dir.glob("*.json"))

        for status_file in status_files:
            worker_id = status_file.stem

            # Load status to check if it's active/idle (skip stopped/failed)
            status = self.health_monitor.parser.parse(status_file)

            # Only check active/idle/spawned workers
            if status.status not in (
                WorkerStatusValue.ACTIVE,
                WorkerStatusValue.IDLE,
                WorkerStatusValue.SPAWNED,
                WorkerStatusValue.STARTING,
            ):
                continue

            # Skip already failed workers
            if status.status == WorkerStatusValue.FAILED or status.error:
                continue

            # Run health check
            health_status = self.health_monitor.check_worker_health(worker_id)

            # If unhealthy, mark worker as failed
            if not health_status.is_healthy:
                await self._mark_worker_unhealthy(worker_id, health_status)

    async def _mark_worker_unhealthy(
        self,
        worker_id: str,
        health_status: WorkerHealthStatus,
    ) -> None:
        """
        Mark a worker as unhealthy by updating its status file.

        Args:
            worker_id: Worker identifier
            health_status: Health check results
        """
        # Read current status
        status_file = self.status_dir / f"{worker_id}.json"
        status = self.health_monitor.parser.parse(status_file)

        # Update status to failed with error message
        import json

        updated_data = status.raw_data.copy() if status.raw_data else {}
        updated_data["status"] = "failed"
        updated_data["health_error"] = health_status.primary_error
        updated_data["health_guidance"] = health_status.guidance
        updated_data["health_score"] = health_status.health_score
        updated_data["failed_at"] = datetime.now().isoformat()

        # Write updated status
        try:
            with open(status_file, 'w') as f:
                json.dump(updated_data, f, indent=2)

            # Notify callback
            if self.on_worker_unhealthy:
                self.on_worker_unhealthy(worker_id, health_status)

        except Exception as e:
            import sys
            print(f"[FORGE] Failed to mark worker {worker_id} as unhealthy: {e}", file=sys.stderr)

    @property
    def is_running(self) -> bool:
        """Check if health monitoring loop is running"""
        return self._running


# =============================================================================
# Convenience Functions
# =============================================================================


def check_worker_health(
    worker_id: str,
    status_dir: Path | str = "~/.forge/status",
    log_dir: Path | str = "~/.forge/logs",
) -> WorkerHealthStatus:
    """
    Convenience function to check a single worker's health.

    Args:
        worker_id: Worker identifier
        status_dir: Directory containing status files
        log_dir: Directory containing log files

    Returns:
        WorkerHealthStatus with check results
    """
    monitor = WorkerHealthMonitor(status_dir, log_dir)
    return monitor.check_worker_health(worker_id)
