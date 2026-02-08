# ADR 0010: Security & Credential Management

**Status**: Accepted
**Date**: 2026-02-07
**Deciders**: FORGE Architecture Team

---

## Context

FORGE needs to handle API keys and credentials for:
1. **Chat backend** - API keys for Claude, GPT, etc.
2. **Workers** - Credentials passed to spawned workers
3. **Multi-user scenarios** - Multiple developers using FORGE

However, FORGE is designed to run in **remote terminal environments** (tmux/devpods/containers) where security boundaries are already established by the sandbox.

---

## Decision

**FORGE does not handle API keys or credentials at all. It invokes headless coding CLIs that manage their own authentication.**

### Security Model

```
┌──────────────────────────────────────────────────┐
│  Sandbox (Container/VM/SSH Session)              │
│  - Headless CLI configs (~/.claude/config.json) │
│  - Environment variables (for CLIs that use env) │
│  - File permissions (user isolation)             │
│  - Network policies (egress control)             │
│  ┌────────────────────────────────────────────┐  │
│  │  FORGE (Ignorant of Credentials)           │  │
│  │  - Invokes headless CLIs                   │  │
│  │  - Sends prompts via stdin                 │  │
│  │  - Receives tool calls via stdout          │  │
│  │  - Zero knowledge of API keys              │  │
│  └────────────────────────────────────────────┘  │
│           │                                       │
│           ↓                                       │
│  ┌────────────────────────────────────────────┐  │
│  │  Headless CLIs (Handle Own Auth)           │  │
│  │  - claude-code (uses ~/.claude/config)     │  │
│  │  - aider (uses $OPENAI_API_KEY)            │  │
│  │  - continue (uses own config)              │  │
│  │  - Custom backends (own auth)              │  │
│  └────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────┘
```

**Principle**: FORGE is a dumb orchestrator. Authentication is the CLI's responsibility.

---

## Implementation Details

### 1. API Key Management

**Strategy: FORGE doesn't manage API keys**

```python
def start_backend(command: list[str]):
    """Start headless CLI backend - it handles its own auth"""

    # FORGE just invokes the CLI
    process = subprocess.Popen(
        command,  # e.g., ["claude-code", "chat", "--headless"]
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    # CLI handles authentication itself:
    # - claude-code reads ~/.claude/config.json
    # - aider reads $OPENAI_API_KEY
    # - continue reads its own config
    # - custom backends handle their own auth

    return process
```

**Zero credential handling**:
- FORGE doesn't read API keys from environment
- FORGE doesn't store credentials
- FORGE doesn't validate authentication
- FORGE doesn't know which provider is being used

**CLI handles everything**:
- claude-code → `~/.claude/config.json`
- aider → `$OPENAI_API_KEY` or `$ANTHROPIC_API_KEY`
- continue → `~/.continue/config.json`
- Custom backends → Whatever they need

### 2. Worker Spawning

**Strategy: Launchers invoke headless CLIs, which handle their own auth**

```python
def spawn_worker(config: WorkerConfig, workspace: Path, session_name: str):
    """Spawn worker via launcher - launcher handles CLI invocation"""

    # FORGE just calls the launcher script
    result = subprocess.run(
        [
            config.launcher,
            "--model", config.model,
            "--workspace", str(workspace),
            "--session-name", session_name,
        ],
        capture_output=True,
        check=True
    )

    # Launcher script spawns tmux session with headless CLI
    # Example launcher internals:
    #   tmux new-session -d -s $SESSION_NAME \
    #     "claude-code chat --headless --workspace $WORKSPACE"
    #
    # claude-code reads its own config for authentication
    # FORGE never touches API keys

    return json.loads(result.stdout)
```

**FORGE's role**: Invoke launcher, parse result
**Launcher's role**: Spawn tmux session with CLI
**CLI's role**: Handle authentication, run coding agent
**API keys**: Completely handled by CLI (not FORGE or launcher)

### 3. Multi-User Support

**Strategy: Sandbox provides isolation, CLIs handle per-user auth**

```
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│  User A         │  │  User B         │  │  User C         │
│  (Container A)  │  │  (Container B)  │  │  (Container C)  │
│                 │  │                 │  │                 │
│  ~/.forge/      │  │  ~/.forge/      │  │  ~/.forge/      │
│  ~/.claude/     │  │  ~/.claude/     │  │  ~/.claude/     │
│    config.json  │  │    config.json  │  │    config.json  │
│                 │  │                 │  │                 │
│  FORGE instance │  │  FORGE instance │  │  FORGE instance │
│      ↓          │  │      ↓          │  │      ↓          │
│  claude-code    │  │  claude-code    │  │  aider          │
│  (uses own cfg) │  │  (uses own cfg) │  │  (uses env var) │
└─────────────────┘  └─────────────────┘  └─────────────────┘
```

**Per-user FORGE instances**: Each user runs own FORGE in their sandbox
**Per-user CLI configs**: Each user has own `~/.claude/`, env vars, etc.
**File permissions**: Unix file permissions prevent cross-user access
**No user management**: FORGE doesn't know about other users or their credentials
**No shared workers**: Workers scoped to user's CLI authentication

### 4. Audit Logging

**Strategy: Minimal logging, no credential leakage**

```python
import logging

logger = logging.getLogger("forge.audit")

def log_worker_spawn(worker_id: str, model: str, workspace: Path):
    """Audit log worker spawn"""
    logger.info(
        "worker_spawned",
        extra={
            "worker_id": worker_id,
            "model": model,
            "workspace": str(workspace),
            "timestamp": datetime.now().isoformat(),
        }
    )

def log_backend_invocation(command: list[str]):
    """Audit log backend invocation (no env vars logged)"""
    logger.info(
        "backend_started",
        extra={
            "command": command,  # e.g., ["claude-code", "chat", "--headless"]
            "timestamp": datetime.now().isoformat(),
        }
    )
    # NOTE: We log the command, not the environment
    # CLIs handle auth via their own config files/env
```

**Log to stderr**: Captured by sandbox's logging infrastructure
**Structured logging**: JSON format for easy parsing
**No credential logging**: Commands logged, not environment or CLI configs
**No rotation**: Sandbox handles log retention

**Example audit log entry**:
```json
{
  "timestamp": "2026-02-07T10:30:45Z",
  "level": "info",
  "event": "backend_started",
  "command": ["claude-code", "chat", "--headless"],
  "note": "CLI handles its own authentication"
}
```

---

## Configuration

**FORGE config has zero credential fields**:

```yaml
# ~/.forge/config.yaml
# NO credential fields:
# ❌ api_keys: {...}
# ❌ secrets: {...}
# ❌ credentials: {...}

# Just specify which CLI to invoke:
backend:
  type: "claude-code"
  command: ["claude-code", "chat", "--headless"]
  # claude-code handles its own auth via ~/.claude/config.json

workers:
  - name: "sonnet"
    launcher: "~/.forge/launchers/claude-code"
    model: "claude-sonnet-4.5"
    # Launcher spawns claude-code, which handles its own auth
```

**Setup documentation instead**:

````markdown
# FORGE Setup

## 1. Set Up Your Headless CLI Authentication

Each headless CLI handles its own authentication:

### claude-code (Claude)
```bash
# Run once to configure:
claude-code auth
# This saves credentials to ~/.claude/config.json
```

### aider (OpenAI/Anthropic)
```bash
# Set environment variable:
export OPENAI_API_KEY="sk-..."
# Or:
export ANTHROPIC_API_KEY="sk-ant-..."

# Make persistent:
echo 'export OPENAI_API_KEY="sk-..."' >> ~/.bashrc
```

### continue
```bash
# Edit config file:
vim ~/.continue/config.json
```

## 2. Launch FORGE

```bash
forge
```

FORGE will invoke your configured CLIs. They handle authentication.
````

---

## Security Boundaries

### What FORGE Trusts

1. **Headless CLIs**: They handle authentication correctly
2. **Sandbox environment**: CLI configs are protected by file permissions
3. **File system**: `~/.forge/` protected by Unix permissions
4. **Process isolation**: Workers can't access other users' processes
5. **Network**: Egress policies enforced by sandbox

### What FORGE Does NOT Handle

1. **API keys**: CLIs handle this (not FORGE)
2. **Credential encryption**: CLIs handle this (not FORGE)
3. **Key rotation**: User updates CLI configs (not FORGE)
4. **Access control**: Sandbox handles user authentication
5. **Secret scanning**: Sandbox prevents accidental commits
6. **Network security**: Sandbox enforces egress rules

### Threat Model

**In scope** (FORGE handles):
- Nothing related to credentials
- FORGE is completely ignorant of authentication

**Out of scope** (CLI/Sandbox handles):
- API key storage (CLI's responsibility)
- Authentication flows (CLI's responsibility)
- Credential encryption (CLI's responsibility)
- Stolen credentials (Sandbox's responsibility)
- Network interception (Sandbox's responsibility)

---

## Backend Status Display

**FORGE shows backend status, not credentials**:

```python
def get_backend_status(backend: ChatBackend) -> str:
    """Get backend status - don't access credentials"""
    if backend.status == "running":
        return "✅ Connected"
    elif backend.status == "error":
        return f"❌ {backend.error}"
    else:
        return "⚠️ Stopped"

# FORGE has no way to check if CLI is authenticated
# It just knows if the backend process is running
```

**Backend panel (no credentials)**:

```
┌─ BACKEND ───────────────────────────────────────┐
│ Type: claude-code                               │
│ Command: claude-code chat --headless            │
│ Status: ✅ Connected                            │
│ Last response: 0.8s ago                         │
├─────────────────────────────────────────────────┤
│ Authentication: Managed by CLI                  │
│ Config: ~/.claude/config.json                   │
│                                                  │
│ ⓘ FORGE does not access or store API keys      │
│   Authentication is handled by the CLI itself   │
└─────────────────────────────────────────────────┘
```

**No credential checking**: FORGE can't verify if CLI is authenticated - it finds out when CLI fails

---

## Consequences

### Positive

1. **Ultimate Simplicity**: Zero credential code in FORGE
2. **Security by Delegation**: CLIs handle auth, FORGE is ignorant
3. **Zero Storage**: No credentials anywhere in FORGE (not even env vars)
4. **CLI Agnostic**: Works with any headless CLI (claude-code, aider, continue, custom)
5. **Flexibility**: Each CLI uses its preferred auth method (config file, env var, etc.)
6. **Maintainability**: No auth code to maintain or update

### Negative

1. **No Validation**: Can't verify if CLI is authenticated before using it
2. **Opaque Errors**: Authentication errors come from CLI, not FORGE
3. **Setup Complexity**: User must configure each CLI separately
4. **No Unified Config**: Each CLI has different auth setup process

### Mitigations

**No validation on startup**: FORGE discovers auth issues when CLI fails

**Documentation**: Clear setup guide for each CLI
```markdown
## Before Using FORGE

Configure your headless CLI:

### claude-code
Run: claude-code auth

### aider
Set: export OPENAI_API_KEY="sk-..."

### continue
Edit: ~/.continue/config.json
```

**Error Messages**: Pass through CLI authentication errors
```
Error: Chat backend failed

Backend stderr:
  Error: No API key found
  Run 'claude-code auth' to configure credentials

Fix:
  1. Configure claude-code: claude-code auth
  2. Restart FORGE
  3. Or switch to different backend in config
```

---

## Alternatives Considered

### Encrypted Credential Store
**Rejected**: Adds complexity, sandbox already provides encryption at rest

### System Keychain Integration
**Rejected**: Not available in all sandboxes (containers, SSH), over-engineering

### Credential Prompting
**Rejected**: Breaks non-interactive usage, credentials should be pre-configured

### Vault/1Password Integration
**Rejected**: Users can integrate via env vars themselves (e.g., `eval $(op signin)`)

---

## Testing Strategy

### Backend Invocation Test
```python
def test_backend_invocation():
    """Test FORGE invokes CLI without credential handling"""

    # Mock subprocess
    with patch('subprocess.Popen') as mock_popen:
        start_backend(["claude-code", "chat", "--headless"])

        # Verify FORGE invoked CLI correctly
        mock_popen.assert_called_once()
        args = mock_popen.call_args[0][0]
        assert args == ["claude-code", "chat", "--headless"]

        # Verify FORGE didn't set any auth-related env vars
        # (CLI inherits user's environment naturally)
```

### CLI Auth Error Handling Test
```python
def test_cli_auth_error_passthrough():
    """Test FORGE passes through CLI authentication errors"""

    backend = ChatBackend(["claude-code", "chat", "--headless"])

    # Mock CLI failing with auth error
    with patch('subprocess.Popen') as mock_popen:
        mock_process = Mock()
        mock_process.stderr.read.return_value = b"Error: No API key found"
        mock_popen.return_value = mock_process

        result = await backend.start()

        # FORGE should capture and display CLI's error
        assert backend.status == "error"
        assert "No API key found" in backend.error
```

### No Credential Leakage Test
```python
def test_no_credential_leakage_in_logs():
    """Test FORGE never logs credentials"""

    with patch('logging.Logger.info') as mock_log:
        log_backend_invocation(["claude-code", "chat", "--headless"])

        # Check all log calls
        for call in mock_log.call_args_list:
            log_data = str(call)
            # Should not contain API keys
            assert "sk-ant-" not in log_data
            assert "API_KEY" not in log_data
```

---

## Documentation

### User Guide Section: Setup

```markdown
## Setting Up Credentials

FORGE reads API keys from environment variables:

### Quick Setup
```bash
export ANTHROPIC_API_KEY="sk-ant-..."
export OPENAI_API_KEY="sk-..."
forge
```

### Persistent Setup
```bash
# Add to shell profile:
echo 'export ANTHROPIC_API_KEY="sk-ant-..."' >> ~/.bashrc
source ~/.bashrc
forge
```

### Container/Kubernetes Setup
```yaml
# kubernetes-pod.yaml
env:
  - name: ANTHROPIC_API_KEY
    valueFrom:
      secretKeyRef:
        name: api-keys
        key: anthropic
```

### Verification
Launch FORGE and check credentials panel (`:credentials` or `C` key).
```

---

## References

- ADR 0006: Technology Stack Selection (Python environment handling)
- ADR 0007: Bead Integration Strategy (no credentials in bead files)
- Twelve-Factor App: https://12factor.net/config (config in environment)

---

## Notes

- **No credential storage**: FORGE is stateless regarding credentials
- **Subprocess environment**: Python's `subprocess` inherits env by default
- **Masked display**: Never show full keys in TUI or logs
- **Trust boundary**: Sandbox is security perimeter, not FORGE

**Decision Confidence**: High - Delegate to sandbox security is standard practice

---

**FORGE** - Federated Orchestration & Resource Generation Engine
