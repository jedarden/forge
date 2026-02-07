# ADR 0005: Dumb Orchestrator Architecture

**Date**: 2026-02-07
**Status**: Accepted
**Deciders**: Jed Arden, Claude Sonnet 4.5

---

## Context

FORGE needs to orchestrate multiple AI coding agents across workspaces, but should not be tightly coupled to specific AI implementations or spawn mechanisms.

**Key Insight**: FORGE itself has **no built-in AI or worker management logic**. It's a "dumb" orchestrator that:
- Displays information collected from external sources
- Triggers external launchers to spawn workers
- Parses logs from well-known locations
- Delegates chat to a configured headless CLI instance

---

## Decision

FORGE is a **dumb orchestrator** that integrates with external components through well-defined integration surfaces.

**Architecture**:
```
┌─────────────────────────────────────────────────┐
│              FORGE (Dumb TUI)                    │
│  - Displays information                         │
│  - Triggers external launchers                  │
│  - Parses logs from folders                     │
│  - Delegates chat to headless CLI               │
└────┬────────────────┬────────────────┬──────────┘
     │                │                │
     ↓                ↓                ↓
┌──────────┐   ┌──────────┐   ┌──────────────┐
│ Headless │   │ Worker   │   │ Log Folders  │
│ CLI      │   │ Launchers│   │ (Filesystem) │
│ Backend  │   │ (Scripts)│   │              │
└──────────┘   └──────────┘   └──────────────┘
     │                │                │
     ↓                ↓                ↓
External         External         External
LLM Service      Worker Spawn     Worker Logs
```

FORGE does NOT:
- Contain AI logic
- Spawn workers directly
- Understand worker internals
- Generate responses

FORGE ONLY:
- Displays data from external sources
- Invokes external launchers
- Parses structured logs
- Forwards chat to external LLM

---

## Integration Surfaces

### 1. Headless CLI Backend (Chat)

**Purpose**: Provide conversational interface and tool calls

**FORGE expects**:
- A CLI tool that can be invoked: `<command> chat --tools=forge.json`
- Accepts natural language input on stdin
- Returns tool calls as structured JSON on stdout
- Supports streaming responses

**Example backends**:
- Claude Code with `--headless` flag
- OpenCode with `--mode=api`
- Custom LLM wrapper conforming to protocol

**Configuration** (`~/.forge/config.yaml`):
```yaml
chat_backend:
  command: "claude-code"
  args: ["chat", "--headless", "--tools=/path/to/forge-tools.json"]
  model: "sonnet"
  api_key_env: "ANTHROPIC_API_KEY"  # Optional
  timeout: 30  # seconds
```

**Protocol**:
```json
// Input (stdin):
{
  "message": "Show me all P0 tasks",
  "context": {
    "current_view": "workers",
    "visible_workers": ["sonnet-alpha", "opus-beta"],
    "visible_tasks": ["bd-abc", "bd-def"]
  }
}

// Output (stdout):
{
  "tool_calls": [
    {
      "tool": "filter_tasks",
      "arguments": {
        "priority": "P0"
      }
    }
  ],
  "message": "Filtering task queue to P0 priority tasks."
}
```

**See**: [Integration Guide: Headless CLI Backend](#integration-guide-headless-cli-backend)

---

### 2. Worker Launchers

**Purpose**: Spawn AI coding workers in various configurations

**FORGE expects**:
- Executable scripts at `~/.forge/launchers/<launcher-name>`
- Accepts standard arguments: `--model`, `--workspace`, `--session-name`
- Returns worker ID on stdout
- Logs to `~/.forge/logs/<worker-id>.log`

**Example launchers**:
- `claude-code-launcher` - Spawns Claude Code in tmux
- `opencode-launcher` - Spawns OpenCode subprocess
- `aider-launcher` - Spawns Aider in tmux
- `custom-launcher` - User-defined spawn logic

**Configuration** (`~/.forge/config.yaml`):
```yaml
launchers:
  claude-code:
    executable: "~/.forge/launchers/claude-code-launcher"
    default_args:
      - "--tmux"
      - "--detached"
    models: ["sonnet", "opus", "haiku"]

  opencode:
    executable: "~/.forge/launchers/opencode-launcher"
    default_args:
      - "--background"
    models: ["gpt4", "gpt35"]
```

**Launcher Protocol**:
```bash
# Launch command:
~/.forge/launchers/claude-code-launcher \
  --model=sonnet \
  --workspace=/path/to/project \
  --session-name=sonnet-alpha

# Expected stdout:
{"worker_id": "sonnet-alpha", "pid": 12345, "status": "spawned"}

# Expected log location:
~/.forge/logs/sonnet-alpha.log

# Expected status file:
~/.forge/status/sonnet-alpha.json
```

**See**: [Integration Guide: Worker Launchers](#integration-guide-worker-launchers)

---

### 3. Worker Configurations (Shareable)

**Purpose**: Define reusable worker configurations across projects

**FORGE expects**:
- Worker configs at `~/.forge/workers/<worker-type>.yaml`
- Can reference GitHub repos for community configs
- Defines model, launcher, environment, paths

**Configuration format** (`~/.forge/workers/claude-code-sonnet.yaml`):
```yaml
name: "claude-code-sonnet"
description: "Claude Code with Sonnet 4.5"
launcher: "claude-code"
model: "sonnet"
tier: "standard"  # For cost routing
cost_per_million_tokens:
  input: 3.0
  output: 15.0
subscription:
  enabled: true
  monthly_cost: 20
  quota_type: "unlimited"  # or "tokens"
environment:
  ANTHROPIC_API_KEY: "${ANTHROPIC_API_KEY}"
  CLAUDE_CONFIG_DIR: "~/.config/claude-code"
spawn_args:
  - "--tmux"
  - "--session=${session_name}"
log_path: "~/.forge/logs/${worker_id}.log"
status_path: "~/.forge/status/${worker_id}.json"
```

**Shareable repos** (`~/.forge/config.yaml`):
```yaml
worker_repos:
  - "https://github.com/forge-community/worker-configs"
  - "https://github.com/jedarden/custom-workers"
```

**See**: [Integration Guide: Worker Configurations](#integration-guide-worker-configurations)

---

### 4. Log Collection

**Purpose**: Collect worker activity for metrics and display

**FORGE expects**:
- Logs in `~/.forge/logs/<worker-id>.log`
- Structured format (JSON lines or key-value)
- Standard fields: timestamp, level, message, worker_id, task_id

**Log format** (`~/.forge/logs/sonnet-alpha.log`):
```json
{"timestamp": "2026-02-07T10:30:00Z", "level": "info", "worker_id": "sonnet-alpha", "message": "Worker started"}
{"timestamp": "2026-02-07T10:30:05Z", "level": "info", "worker_id": "sonnet-alpha", "task_id": "bd-abc", "event": "task_started"}
{"timestamp": "2026-02-07T10:35:00Z", "level": "info", "worker_id": "sonnet-alpha", "task_id": "bd-abc", "event": "task_completed", "tokens_used": {"input": 1000, "output": 500}}
{"timestamp": "2026-02-07T10:35:01Z", "level": "error", "worker_id": "sonnet-alpha", "task_id": "bd-def", "event": "task_failed", "error": "API rate limit"}
```

**Alternative format** (key-value):
```
2026-02-07T10:30:00Z level=info worker_id=sonnet-alpha message="Worker started"
2026-02-07T10:30:05Z level=info worker_id=sonnet-alpha task_id=bd-abc event=task_started
2026-02-07T10:35:00Z level=info worker_id=sonnet-alpha task_id=bd-abc event=task_completed tokens_input=1000 tokens_output=500
```

**Status files** (`~/.forge/status/<worker-id>.json`):
```json
{
  "worker_id": "sonnet-alpha",
  "status": "active",  // active, idle, failed, stopped
  "pid": 12345,
  "model": "sonnet",
  "workspace": "/path/to/project",
  "current_task": "bd-abc",
  "uptime_seconds": 300,
  "tasks_completed": 5,
  "last_activity": "2026-02-07T10:35:00Z"
}
```

**See**: [Integration Guide: Log Collection](#integration-guide-log-collection)

---

### 5. File System Conventions

**FORGE expects**:
```
~/.forge/
├── config.yaml                    # Main configuration
├── tools.json                     # Tool definitions for LLM
├── launchers/                     # Worker launcher scripts
│   ├── claude-code-launcher
│   ├── opencode-launcher
│   └── custom-launcher
├── workers/                       # Worker configuration templates
│   ├── claude-code-sonnet.yaml
│   ├── opencode-gpt4.yaml
│   └── custom-worker.yaml
├── logs/                          # Worker logs (FORGE reads)
│   ├── sonnet-alpha.log
│   ├── opus-beta.log
│   └── haiku-gamma.log
├── status/                        # Worker status files (FORGE reads)
│   ├── sonnet-alpha.json
│   ├── opus-beta.json
│   └── haiku-gamma.json
└── layouts/                       # Saved dashboard layouts
    ├── default.yaml
    └── monitoring.yaml
```

**See**: [Integration Guide: File System Layout](#integration-guide-file-system-layout)

---

## Rationale

### Why "Dumb" Orchestrator?

**Decoupling**:
- FORGE doesn't need to understand LLM internals
- Workers can be any tool (Claude Code, Aider, custom scripts)
- Easy to add new worker types without modifying FORGE
- Community can share worker configurations

**Simplicity**:
- FORGE is just a dashboard + glue code
- No complex AI logic to maintain
- Easier to test and debug
- Clear separation of concerns

**Flexibility**:
- Users can swap LLM backends (Claude → GPT → local)
- Users can customize launchers per environment
- Workers can be deployed anywhere (local, remote, containers)
- Log formats can be adapted via parsers

**Shareability**:
- Worker configs can be shared via GitHub
- Launchers can be packaged and distributed
- Community can build ecosystem around FORGE
- No proprietary lock-in

---

## Consequences

### Positive

- **Decoupled**: FORGE doesn't depend on specific AI implementations
- **Extensible**: Easy to add new worker types, launchers, backends
- **Shareable**: Worker configs can be community-maintained
- **Flexible**: Users control all integration points
- **Simple**: FORGE is "just" a TUI + glue code
- **Testable**: Integration surfaces are well-defined

### Negative

- **Configuration complexity**: Users must configure integrations
- **Documentation burden**: Must document all integration surfaces thoroughly
- **Setup friction**: Requires external tools to be installed
- **Error handling**: Failures in external components harder to diagnose
- **Version management**: FORGE must handle different launcher/backend versions

### Neutral

- **Opinionated**: File system layout, log formats are prescribed
- **Convention over configuration**: Works best when conventions followed
- **Community dependency**: Quality depends on community worker configs

---

## Migration Path

### Phase 1: Core Infrastructure
- [ ] Define integration protocols (headless CLI, launchers, logs)
- [ ] Implement configuration loading
- [ ] Build log parser with format detection
- [ ] Create launcher invocation system

### Phase 2: Reference Implementations
- [ ] Reference headless CLI wrapper
- [ ] Reference Claude Code launcher
- [ ] Reference OpenCode launcher
- [ ] Reference worker configurations

### Phase 3: Community Ecosystem
- [ ] Worker config registry (GitHub repo)
- [ ] Launcher marketplace
- [ ] Log format adapters
- [ ] Backend plugins

### Phase 4: Developer Tools
- [ ] Launcher testing framework
- [ ] Worker config validator
- [ ] Log format debugger
- [ ] Integration testing suite

---

## Integration Points Summary

| Component | Interface | Configuration | Data Flow |
|-----------|-----------|---------------|-----------|
| **Chat Backend** | stdin/stdout protocol | `config.yaml` | FORGE → LLM |
| **Launchers** | CLI executable | `~/.forge/launchers/` | FORGE → spawn script |
| **Workers** | Config templates | `~/.forge/workers/` | GitHub → FORGE |
| **Logs** | JSON/key-value files | `~/.forge/logs/` | Workers → FORGE |
| **Status** | JSON files | `~/.forge/status/` | Workers → FORGE |

---

## References

- [Integration Guide: Complete Documentation](#integration-guides) (see below)
- [Worker Configuration Spec](#worker-configuration-specification)
- [Launcher Protocol Spec](#launcher-protocol-specification)
- [Log Format Spec](#log-format-specification)
- ADR 0004: Tool-Based Conversational Interface

---

## Future Enhancements

1. **Remote workers**: Workers running on different machines
2. **Container launchers**: Docker/Kubernetes worker deployment
3. **Plugin system**: Custom log parsers, backends, launchers
4. **Worker marketplace**: Community registry of configs
5. **Auto-discovery**: Detect workers without explicit config
6. **Health checks**: Active worker health monitoring
7. **Migration tools**: Convert between worker config formats

---

**FORGE** - Federated Orchestration & Resource Generation Engine

A dumb orchestrator that gets smart by integrating with intelligent external components.
