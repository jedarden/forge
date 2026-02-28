# FORGE Worker Lifecycle Diagrams

This document illustrates the lifecycle and state transitions of FORGE workers.

## Worker State Machine

```mermaid
stateDiagram-v2
    [*] --> Spawning: spawn() called

    Spawning --> Starting: Tmux session created
    Spawning --> Failed: Launcher error

    Starting --> Active: Agent initialized
    Starting --> Failed: Initialization timeout

    Active --> Idle: Task completed
    Active --> Paused: pause() called
    Active --> Failed: Crash detected
    Active --> Stopped: kill() called

    Idle --> Active: Task assigned
    Idle --> Paused: pause() called
    Idle --> Stopped: kill() called

    Paused --> Active: resume() called
    Paused --> Stopped: kill() called

    Failed --> Starting: Auto-recovery (if enabled)
    Failed --> [*]: Final state

    Stopped --> [*]: Final state
```

## Worker Status Values

| Status | Color | Indicator | Description |
|--------|-------|-----------|-------------|
| `Active` | Green | ✅ | Working on a task |
| `Idle` | Gray | 💤 | Running, no current task |
| `Starting` | Yellow | 🔄 | Initializing |
| `Paused` | Blue | ⏸️ | Not claiming new tasks |
| `Failed` | Red | ❌ | Crashed or error state |
| `Stopped` | Dark Gray | ⏹️ | Intentionally stopped |
| `Error` | Orange | ⚠️ | Status file corrupted |

## Spawn Sequence

```mermaid
sequenceDiagram
    participant TUI as TUI App
    participant Launcher as WorkerLauncher
    participant Script as Launcher Script
    participant Tmux as Tmux Session
    participant Agent as Worker Agent
    participant Status as Status File

    Note over TUI,Status: Phase 1: Validation
    TUI->>Launcher: SpawnRequest
    Launcher->>Launcher: Validate launcher exists
    Launcher->>Launcher: Check workspace valid

    Note over TUI,Status: Phase 2: Session Creation
    Launcher->>Script: Execute with env vars
    Script->>Tmux: tmux new-session -d -s name
    Tmux-->>Script: Session created

    Note over TUI,Status: Phase 3: Agent Start
    Script->>Tmux: send-keys (agent command)
    Tmux->>Agent: Start process
    Agent->>Agent: Initialize

    Note over TUI,Status: Phase 4: Status Output
    Script-->>Launcher: JSON {pid, session, model}
    Launcher->>Tmux: Verify session exists
    Launcher-->>TUI: WorkerHandle

    Note over TUI,Status: Phase 5: First Status
    Agent->>Status: Write initial status
    Status-->>TUI: StatusWatcher event
```

## Health Check States

```mermaid
stateDiagram-v2
    [*] --> Checking: Start check

    Checking --> PIDCheck: Check status file
    PIDCheck --> ActivityCheck: PID valid
    PIDCheck --> Critical: PID dead

    ActivityCheck --> MemoryCheck: Activity fresh
    ActivityCheck --> Degraded: Activity stale (>5 min)

    MemoryCheck --> Healthy: Memory OK
    MemoryCheck --> Critical: Memory over limit

    Degraded --> Healthy: New activity detected
    Degraded --> Critical: Still stale after warning

    Critical --> RecoveryAttempt: Auto-recovery enabled
    Critical --> Failed: No recovery

    RecoveryAttempt --> Healthy: Restart successful
    RecoveryAttempt --> Failed: Recovery failed

    Healthy --> [*]: Check complete
    Failed --> [*]: Check complete
```

## Health Levels

```mermaid
graph LR
    subgraph "Health Levels"
        Healthy[🟢 Healthy<br/>All checks pass]
        Degraded[🟡 Degraded<br/>Stale status]
        Critical[🔴 Critical<br/>PID dead or OOM]
        Failed[⚫ Failed<br/>Recovery failed]
    end

    Healthy -->|Status stale| Degraded
    Degraded -->|Status fresh| Healthy
    Degraded -->|Still stale| Critical
    Critical -->|Restart OK| Healthy
    Critical -->|Restart fail| Failed
```

## Task Assignment Flow

```mermaid
sequenceDiagram
    participant Manager as BeadManager
    participant Router as TaskRouter
    participant Scorer as TaskScorer
    participant Pool as Worker Pool
    participant Worker as Worker Agent

    Manager->>Manager: Get ready beads
    Manager->>Router: Request assignment

    Router->>Pool: Get available workers
    Pool-->>Router: Worker list

    loop For each bead
        Router->>Scorer: score_bead(bead, workers)
        Scorer->>Scorer: Calculate priority score
        Scorer->>Scorer: Match tier to priority
        Scorer-->>Router: ScoredAssignment
    end

    Router->>Router: Sort by score
    Router->>Worker: Assign top bead
    Worker->>Worker: Update status with task
    Worker-->>Manager: Status update event
```

## Priority to Tier Mapping

```mermaid
graph LR
    subgraph "Priority Levels"
        P0[P0 - Critical]
        P1[P1 - High]
        P2[P2 - Normal]
        P3[P3 - Low]
        P4[P4 - Backlog]
    end

    subgraph "Worker Tiers"
        Premium[Premium<br/>Opus 4.6]
        Standard[Standard<br/>Sonnet/GLM]
        Budget[Budget<br/>Haiku]
    end

    P0 --> Premium
    P1 --> Premium
    P2 --> Standard
    P3 --> Budget
    P4 --> Budget
```

## Pause/Resume Flow

```mermaid
sequenceDiagram
    participant User as User
    participant TUI as TUI App
    participant Writer as StatusWriter
    participant File as Status File
    participant Agent as Worker Agent

    Note over User,Agent: Pause Operation
    User->>TUI: Press 'p' on worker
    TUI->>Writer: set_paused(worker_id, true)
    Writer->>File: Write paused=true
    File-->>Agent: Read status
    Agent->>Agent: Stop claiming tasks
    Agent->>Agent: Continue current task
    Agent-->>TUI: Status shows Paused

    Note over User,Agent: Resume Operation
    User->>TUI: Press 'r' on worker
    TUI->>Writer: set_paused(worker_id, false)
    Writer->>File: Write paused=false
    File-->>Agent: Read status
    Agent->>Agent: Resume claiming tasks
    Agent-->>TUI: Status shows Active/Idle
```

## Recovery Flow

```mermaid
flowchart TB
    Crash[Crash Detected] --> Check{Auto-Recovery<br/>Enabled?}

    Check -->|No| MarkFailed[Mark as Failed]
    Check -->|Yes| Attempts{Attempt<br/>Count?}

    Attempts -->|< Max| Restart[Attempt Restart]
    Attempts -->|>= Max| MarkFailed

    Restart --> Spawn[Re-spawn worker]
    Spawn --> Verify{Started OK?}

    Verify -->|Yes| Healthy[Mark Healthy]
    Verify -->|No| Increment[Increment attempts]
    Increment --> Attempts

    Healthy --> Alert[Notify TUI]
    MarkFailed --> Alert

    Alert --> Done[Done]
```

## Worker Types

```mermaid
graph TB
    subgraph "Worker Executors"
        GLM[GLM-4.7<br/>claude-code-glm-47-*]
        Sonnet[Sonnet 4.5<br/>claude-code-sonnet-*]
        Opus[Opus 4.6<br/>claude-code-opus-*]
        Haiku[Haiku 4.5<br/>claude-code-haiku-*]
    end

    subgraph "Tiers"
        Premium[Premium Tier]
        Standard[Standard Tier]
        Budget[Budget Tier]
    end

    Opus --> Premium
    Sonnet --> Standard
    GLM --> Standard
    Haiku --> Budget

    subgraph "Cost (per 1M tokens)"
        OpusCost[$15 input / $75 output]
        SonnetCost[$3 input / $15 output]
        GLMCost[$1 input / $2 output]
        HaikuCost[$0.80 input / $4 output]
    end

    Opus --- OpusCost
    Sonnet --- SonnetCost
    GLM --- GLMCost
    Haiku --- HaikuCost
```

## Session Naming Convention

```mermaid
graph LR
    subgraph "Session Name Format"
        Prefix[claude-code-]
        Model[MODEL-]
        Suffix[suffix]
    end

    Prefix --> Model --> Suffix

    subgraph "Examples"
        Ex1[claude-code-glm-47-alpha]
        Ex2[claude-code-sonnet-bravo]
        Ex3[claude-code-opus-charlie]
        Ex4[claude-code-haiku-delta]
    end

    Model -->|glm-47| Ex1
    Model -->|sonnet| Ex2
    Model -->|opus| Ex3
    Model -->|haiku| Ex4
```

## Environment Variables

| Variable | Example | Description |
|----------|---------|-------------|
| `FORGE_WORKER_ID` | `claude-code-sonnet-alpha` | Unique worker identifier |
| `FORGE_SESSION` | `forge-worker-alpha` | Tmux session name |
| `FORGE_MODEL` | `sonnet` | Model to use |
| `FORGE_WORKSPACE` | `/home/coder/project` | Working directory |
| `FORGE_ASSIGNED_BEAD` | `fg-123` | Assigned bead ID (optional) |

## Related Documentation

- [Architecture Overview](./ARCHITECTURE.md) - System design
- [Data Flow](./data-flow.md) - Data movement
- [Event Flow](./event-flow.md) - Event handling
- [Workers Documentation](../WORKERS.md) - Detailed worker docs
