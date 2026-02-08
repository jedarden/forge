# ADR 0014: Error Handling Strategy

**Status**: Accepted
**Date**: 2026-02-07
**Deciders**: FORGE Architecture Team

---

## Context

FORGE integrates with multiple external components that can fail:
1. **Chat backend** - Headless CLI crashes or hangs
2. **Worker launchers** - Fail to spawn workers
3. **Log parsing** - Malformed log entries
4. **File system** - Corrupted status files
5. **Bead integration** - Invalid JSONL format

Traditional error handling might use fallbacks and automatic recovery. However, FORGE is a **developer tool** where visibility is more important than automated recovery.

---

## Decision

**Prefer graceful degradation over fallback. No automatic recovery.**

### Core Principles

1. **Visibility First**: Show errors clearly in TUI
2. **No Silent Failures**: Every error is visible to user
3. **No Automatic Retry**: User decides if/when to retry
4. **Degrade Gracefully**: Broken component doesn't crash entire app
5. **Clear Error Messages**: Actionable guidance, not technical jargon

```
┌─────────────────────────────────────────────────┐
│  Error Handling Philosophy                      │
├─────────────────────────────────────────────────┤
│  ❌ Hide error, retry automatically             │
│  ❌ Fallback to degraded mode silently          │
│  ❌ Generic error: "Something went wrong"       │
│                                                  │
│  ✅ Show error in UI with context               │
│  ✅ Degrade component, keep app running         │
│  ✅ Specific error: "Backend crashed: ..."      │
│  ✅ Actionable guidance: "Restart with ..."     │
└─────────────────────────────────────────────────┘
```

---

## Implementation Details

### 1. Chat Backend Failure

**Scenario**: Headless CLI crashes or becomes unresponsive

**Strategy: Degrade to hotkey-only mode**

```python
class ChatBackend:
    """Chat backend wrapper with error handling"""

    def __init__(self, command: list[str]):
        self.command = command
        self.process = None
        self.status = "stopped"
        self.error = None

    async def start(self):
        """Start backend process"""
        try:
            self.process = await asyncio.create_subprocess_exec(
                *self.command,
                stdin=asyncio.subprocess.PIPE,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            self.status = "running"
            self.error = None
        except FileNotFoundError:
            self.status = "error"
            self.error = f"Backend command not found: {self.command[0]}"
        except Exception as e:
            self.status = "error"
            self.error = f"Failed to start backend: {e}"

    async def send_message(self, message: str) -> dict | None:
        """Send message to backend, return None on failure"""
        if self.status != "running":
            # Don't try to send if backend is dead
            return None

        try:
            # Send message, wait for response
            response = await self._communicate(message)
            return response
        except BrokenPipeError:
            self.status = "error"
            self.error = "Backend process died (broken pipe)"
            return None
        except asyncio.TimeoutError:
            self.status = "error"
            self.error = "Backend timed out (>30s)"
            return None
        except Exception as e:
            self.status = "error"
            self.error = f"Backend communication failed: {e}"
            return None


class ForgeApp(App):
    """Main app with degraded chat handling"""

    def on_chat_input(self, message: str):
        """Handle chat input"""
        if self.backend.status != "running":
            # Backend unavailable, show error
            self.show_error(
                title="Chat Unavailable",
                message=f"Backend error: {self.backend.error}",
                guidance=[
                    "Use hotkeys for navigation (W/T/C/M/L)",
                    "Restart backend with :restart-backend",
                    "Check backend logs: ~/.forge/logs/backend.log"
                ]
            )
            return

        # Send to backend
        response = await self.backend.send_message(message)

        if response is None:
            # Backend failed during request
            self.show_error(
                title="Backend Failed",
                message=f"{self.backend.error}",
                guidance=[
                    "Switched to hotkey-only mode",
                    "Press W for workers, T for tasks, C for costs",
                    "Restart backend: :restart-backend"
                ]
            )
            # Degrade to hotkey mode - app keeps running
            self.chat_mode = "disabled"
```

**TUI Display**:
```
┌─ CHAT ──────────────────────────────────────────┐
│ ⚠️  Chat backend unavailable                    │
│                                                  │
│ Error: Backend process died (broken pipe)       │
│                                                  │
│ Using hotkey-only mode:                         │
│   W - Workers    T - Tasks    C - Costs         │
│   M - Metrics    L - Logs                       │
│                                                  │
│ Restart backend: :restart-backend               │
│ View logs: :logs backend                        │
└─────────────────────────────────────────────────┘
```

**No automatic restart**: User decides when to fix backend

### 2. Launcher Failure

**Scenario**: Worker launcher fails to spawn worker

**Strategy: Show error, don't retry**

```python
def spawn_worker(
    config: WorkerConfig,
    workspace: Path,
    session_name: str
) -> WorkerSpawnResult:
    """Spawn worker, return detailed error on failure"""

    try:
        result = subprocess.run(
            [config.launcher, "--model", config.model, "--workspace", str(workspace)],
            capture_output=True,
            text=True,
            timeout=30,
            check=True
        )

        # Parse JSON output
        try:
            output = json.loads(result.stdout)
            return WorkerSpawnResult(
                success=True,
                worker_id=output["worker_id"],
                pid=output["pid"],
            )
        except json.JSONDecodeError as e:
            return WorkerSpawnResult(
                success=False,
                error="Invalid launcher output (not JSON)",
                stdout=result.stdout,
                stderr=result.stderr,
            )

    except subprocess.TimeoutExpired:
        return WorkerSpawnResult(
            success=False,
            error="Launcher timed out after 30s",
            guidance=[
                "Check if launcher hangs on input",
                "Verify launcher is executable: chmod +x",
                "Test manually: " + " ".join([config.launcher, "--help"]),
            ]
        )

    except subprocess.CalledProcessError as e:
        return WorkerSpawnResult(
            success=False,
            error=f"Launcher exited with code {e.returncode}",
            stdout=e.stdout,
            stderr=e.stderr,
            guidance=[
                "Check launcher stderr output",
                "Verify workspace path exists",
                "Test launcher: " + config.launcher,
            ]
        )

    except FileNotFoundError:
        return WorkerSpawnResult(
            success=False,
            error=f"Launcher not found: {config.launcher}",
            guidance=[
                "Verify launcher path in config",
                "Make launcher executable: chmod +x",
                "Check ~/.forge/launchers/ directory",
            ]
        )


class ForgeApp(App):
    """Handle spawn failures in UI"""

    def on_spawn_worker_requested(self, config: WorkerConfig):
        """User requested worker spawn via chat or hotkey"""
        result = spawn_worker(config, self.workspace, "new-worker")

        if result.success:
            self.show_notification(f"✅ Worker spawned: {result.worker_id}")
            self.refresh_workers()
        else:
            # Show detailed error, don't retry
            self.show_error(
                title="Failed to Spawn Worker",
                message=result.error,
                details={
                    "Launcher": config.launcher,
                    "Model": config.model,
                    "Workspace": str(self.workspace),
                    "Stdout": result.stdout or "(empty)",
                    "Stderr": result.stderr or "(empty)",
                },
                guidance=result.guidance
            )
```

**Error Dialog**:
```
┌─ ERROR: Failed to Spawn Worker ────────────────┐
│                                                 │
│ Launcher exited with code 1                    │
│                                                 │
│ Launcher: ~/.forge/launchers/claude-code       │
│ Model: claude-sonnet-4.5                       │
│ Workspace: /workspace/project                  │
│                                                 │
│ Stderr:                                        │
│   Error: ANTHROPIC_API_KEY not set             │
│   Cannot start Claude Code without API key     │
│                                                 │
│ Suggestions:                                   │
│   • Check launcher stderr output               │
│   • Verify workspace path exists               │
│   • Test launcher: ~/.forge/launchers/...      │
│                                                 │
│ [View Full Logs] [Edit Config] [Dismiss]       │
└─────────────────────────────────────────────────┘
```

**No retry queue**: User fixes issue manually and tries again

### 3. Log Parse Errors

**Scenario**: Worker writes malformed log entries

**Strategy: Skip bad entries, show warning**

```python
class LogParser:
    """Parse log entries with error tolerance"""

    def __init__(self):
        self.parse_errors = 0
        self.last_error = None

    def parse_entry(self, line: str) -> LogEntry | None:
        """Parse log entry, return None on error"""
        try:
            if line.strip().startswith('{'):
                # JSON format
                data = orjson.loads(line)
                return LogEntry.from_json(data)
            else:
                # Key-value format
                return LogEntry.from_keyvalue(line)

        except orjson.JSONDecodeError as e:
            self.parse_errors += 1
            self.last_error = f"Invalid JSON: {str(e)[:50]}"
            # Skip this entry, continue with next
            return None

        except KeyError as e:
            self.parse_errors += 1
            self.last_error = f"Missing required field: {e}"
            return None

        except Exception as e:
            self.parse_errors += 1
            self.last_error = f"Parse error: {str(e)[:50]}"
            return None


class LogPanel(Widget):
    """Log panel with parse error indicator"""

    def compose(self):
        """Render log panel"""
        yield LogTable(self.entries)

        # Show parse error indicator if errors occurred
        if self.parser.parse_errors > 0:
            yield Static(
                f"⚠️  {self.parser.parse_errors} malformed log entries skipped\n"
                f"   Last error: {self.parser.last_error}",
                classes="warning"
            )
```

**Log Panel with Warning**:
```
┌─ LOGS (sonnet-alpha) ───────────────────────────┐
│ 10:45:23 info  Worker started                   │
│ 10:45:24 info  Loading workspace                │
│ 10:45:25 info  API call completed               │
│ 10:45:26 info  Task bd-abc started              │
│                                                  │
│ ⚠️  2 malformed log entries skipped             │
│    Last error: Invalid JSON: Expecting ',' ...  │
│                                                  │
│ [View Raw Logs] [Report Issue]                  │
└─────────────────────────────────────────────────┘
```

**Graceful degradation**: Most logs still visible, bad entries skipped

### 4. Status File Corruption

**Scenario**: Worker status file has invalid JSON

**Strategy: Mark worker as unknown, show error**

```python
def load_worker_status(status_file: Path) -> WorkerStatus:
    """Load worker status, handle corruption"""

    try:
        with open(status_file) as f:
            data = json.load(f)

        # Validate required fields
        required = ["worker_id", "status", "model", "workspace"]
        missing = [f for f in required if f not in data]

        if missing:
            return WorkerStatus(
                worker_id=status_file.stem,
                status="error",
                error=f"Corrupted status file (missing: {', '.join(missing)})",
                model="unknown",
                workspace=Path("unknown"),
            )

        return WorkerStatus.from_dict(data)

    except json.JSONDecodeError as e:
        return WorkerStatus(
            worker_id=status_file.stem,
            status="error",
            error=f"Corrupted status file (invalid JSON: {str(e)[:50]})",
            model="unknown",
            workspace=Path("unknown"),
        )

    except FileNotFoundError:
        # Status file deleted while we were reading - worker stopped
        return WorkerStatus(
            worker_id=status_file.stem,
            status="stopped",
            model="unknown",
            workspace=Path("unknown"),
        )

    except Exception as e:
        return WorkerStatus(
            worker_id=status_file.stem,
            status="error",
            error=f"Failed to read status: {str(e)[:50]}",
            model="unknown",
            workspace=Path("unknown"),
        )
```

**Worker Panel with Error**:
```
┌─ WORKERS ───────────────────────────────────────┐
│ ID            │ Status  │ Model     │ Workspace │
├───────────────┼─────────┼───────────┼───────────┤
│ sonnet-alpha  │ active  │ sonnet-4.5│ /project  │
│ haiku-beta    │ idle    │ haiku-4.5 │ /project  │
│ opus-gamma    │ ⚠️ error│ unknown   │ unknown   │
│               │ Corrupted status file           │
│               │ [Delete Status] [Restart]       │
└─────────────────────────────────────────────────┘
```

**User action required**: Manually delete status file or restart worker

### 5. File System Errors

**Scenario**: Cannot write to `~/.forge/` directory

**Strategy: Fail fast on startup with clear message**

```python
class ForgeApp(App):
    """Main app with startup validation"""

    def on_mount(self):
        """Validate environment on startup"""
        errors = self.validate_environment()

        if errors:
            # Show blocking error screen
            self.show_fatal_error(
                title="Cannot Start FORGE",
                errors=errors,
                guidance=[
                    "Fix the errors above and restart",
                    "Check file permissions: ls -la ~/.forge",
                    "Verify disk space: df -h",
                ]
            )
            # Exit immediately
            self.exit(1)

    def validate_environment(self) -> list[str]:
        """Validate FORGE can operate"""
        errors = []

        # Check ~/.forge/ exists and writable
        forge_dir = Path.home() / ".forge"
        try:
            forge_dir.mkdir(parents=True, exist_ok=True)
            test_file = forge_dir / ".write-test"
            test_file.write_text("test")
            test_file.unlink()
        except PermissionError:
            errors.append(f"Cannot write to {forge_dir} (permission denied)")
        except OSError as e:
            errors.append(f"Cannot access {forge_dir}: {e}")

        # Check required subdirectories
        for subdir in ["status", "logs", "launchers", "workers"]:
            path = forge_dir / subdir
            try:
                path.mkdir(exist_ok=True)
            except Exception as e:
                errors.append(f"Cannot create {path}: {e}")

        return errors
```

**Fatal Error Screen** (blocks app startup):
```
┌─────────────────────────────────────────────────┐
│                                                 │
│        ⚠️  Cannot Start FORGE                   │
│                                                 │
│  Errors:                                       │
│    • Cannot write to /home/user/.forge         │
│      (permission denied)                       │
│                                                 │
│  Fix:                                          │
│    • Check file permissions:                   │
│      ls -la ~/.forge                           │
│    • Verify disk space: df -h                  │
│    • Check directory ownership:                │
│      ls -ld ~/.forge                           │
│                                                 │
│  Press any key to exit                         │
│                                                 │
└─────────────────────────────────────────────────┘
```

**No degraded mode**: App cannot function without write access

---

## Error Display Patterns

### 1. Transient Errors (Non-Blocking)

Show notification, don't interrupt workflow:

```python
self.notify("⚠️  Cost update delayed (database locked)", severity="warning")
```

### 2. Component Errors (Degrade Component)

Show error in component panel:

```python
self.show_component_error(
    component="chat",
    error="Backend unavailable",
    fallback="Using hotkey-only mode"
)
```

### 3. Fatal Errors (Block Startup)

Show full-screen error, exit app:

```python
self.show_fatal_error(
    title="Cannot Start",
    errors=[...],
    guidance=[...]
)
self.exit(1)
```

### 4. User Action Required

Show dialog with actionable buttons:

```python
self.show_error_dialog(
    title="Worker Spawn Failed",
    message=error_message,
    actions=[
        ("View Logs", self.view_logs),
        ("Edit Config", self.edit_config),
        ("Retry", self.retry_spawn),
        ("Dismiss", None),
    ]
)
```

---

## Consequences

### Positive

1. **Transparency**: Developers see exactly what failed and why
2. **No Surprises**: No silent failures or mysterious behavior
3. **Debuggability**: Clear error messages enable quick fixes
4. **Simplicity**: No complex retry/fallback logic to maintain
5. **Trust**: Users know FORGE won't hide problems

### Negative

1. **Manual Recovery**: User must fix errors manually
2. **Verbose UI**: Many error messages if things go wrong
3. **No Convenience**: No automatic retry for transient failures
4. **Learning Curve**: Users need to understand error messages

### Mitigations

**Actionable Guidance**: Every error includes fix suggestions
```
Error: Backend failed
Fix:
  1. Check API key: echo $ANTHROPIC_API_KEY
  2. Restart backend: :restart-backend
  3. View logs: ~/.forge/logs/backend.log
```

**Error Categories**: Help users understand severity
- ⚠️ Warning (yellow): Degraded but functional
- ❌ Error (red): Component failed
- 🔴 Fatal (red): Cannot continue

**Quick Actions**: Buttons for common fixes
```
[Restart Backend] [View Logs] [Edit Config] [Dismiss]
```

---

## Testing Strategy

### Error Injection Tests

```python
def test_backend_crash_handling():
    """Test graceful degradation when backend crashes"""
    app = ForgeApp()

    # Start backend
    await app.backend.start()
    assert app.chat_mode == "enabled"

    # Kill backend process
    app.backend.process.kill()

    # Try to send message
    result = await app.on_chat_input("test")

    # Should degrade to hotkey mode, not crash
    assert app.chat_mode == "disabled"
    assert app.backend.status == "error"
    assert "Backend process died" in app.backend.error

def test_malformed_log_parsing():
    """Test log parser skips bad entries"""
    parser = LogParser()

    # Valid entry
    entry = parser.parse_entry('{"timestamp": "2026-02-07T10:00:00", "level": "info"}')
    assert entry is not None

    # Malformed entry
    entry = parser.parse_entry('invalid json {')
    assert entry is None
    assert parser.parse_errors == 1

    # Parser still works after error
    entry = parser.parse_entry('{"timestamp": "2026-02-07T10:00:01", "level": "info"}')
    assert entry is not None

def test_corrupted_status_file():
    """Test worker status with corrupted file"""
    # Create corrupted status file
    status_file = Path("/tmp/test-worker.json")
    status_file.write_text("invalid json {")

    # Should return error status, not crash
    status = load_worker_status(status_file)
    assert status.status == "error"
    assert "Corrupted status file" in status.error
```

---

## References

- ADR 0008: Real-Time Update Architecture (error handling in file watchers)
- ADR 0010: Security & Credential Management (validation errors)
- Python Exception Handling Best Practices

---

## Notes

- **Developer Tool Philosophy**: Visibility > Convenience
- **No Silent Failures**: Every error is surfaced to user
- **No Automatic Retry**: Retries can mask underlying issues
- **Clear Error Messages**: Include context, cause, and fix suggestions

**Decision Confidence**: High - Transparency is critical for developer tools

---

**FORGE** - Federated Orchestration & Resource Generation Engine
