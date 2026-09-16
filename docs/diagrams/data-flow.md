# FORGE Data Flow Diagrams

This document illustrates how data flows through the FORGE system.

## Status Update Flow

When a worker agent updates its status, the data flows as follows:

```mermaid
sequenceDiagram
    participant Worker as Worker Agent
    participant File as Status File
    participant Watcher as StatusWatcher
    participant Channel as mpsc Channel
    participant TUI as TUI App
    participant Display as Terminal

    Worker->>File: Write JSON status
    Note over File: ~/.forge/status/worker-1.json
    File-->>Watcher: inotify event
    Watcher->>File: Read updated file
    Watcher->>Channel: Send StatusEvent
    Channel->>TUI: Receive event
    TUI->>TUI: Update worker state
    TUI->>Display: Render frame
```

## Worker Spawn Flow

```mermaid
sequenceDiagram
    participant User as User
    participant TUI as TUI App
    participant Launcher as WorkerLauncher
    participant Script as Launcher Script
    participant Tmux as tmux
    participant Agent as Worker Agent

    User->>TUI: Press spawn key (g/s/o/h)
    TUI->>Launcher: spawn(request)
    Launcher->>Launcher: Validate config
    Launcher->>Script: Execute with env vars
    Note over Script: FORGE_WORKER_ID, FORGE_MODEL, etc.
    Script->>Tmux: new-session -d -s name
    Script->>Tmux: send-keys (start agent)
    Tmux->>Agent: Start process
    Script-->>Launcher: JSON output {pid, session}
    Launcher->>Launcher: Parse response
    Launcher->>Tmux: Verify session exists
    Launcher-->>TUI: WorkerHandle
    TUI->>TUI: Add to worker list
    TUI->>User: Display new worker
```

## Chat Query Flow

```mermaid
sequenceDiagram
    participant User as User
    participant TUI as TUI App
    participant Channel as mpsc Channel
    participant Backend as ChatBackend
    participant Provider as ChatProvider
    participant API as Claude API

    User->>TUI: Type query + Enter
    TUI->>TUI: Build DashboardContext
    TUI->>Channel: Send ChatRequest
    Note over TUI: Non-blocking send
    TUI->>TUI: Set chat_pending=true

    Channel->>Backend: Receive request
    Backend->>Backend: Inject context
    Backend->>Backend: Check rate limit
    Backend->>Provider: process(prompt, context)
    Provider->>API: HTTP request
    API-->>Provider: Response
    Provider-->>Backend: ProviderResponse
    Backend->>Backend: Parse tools used
    Backend->>Backend: Log to audit file
    Backend->>Channel: Send ChatResponse

    Channel->>TUI: Receive response
    TUI->>TUI: chat_pending=false
    TUI->>TUI: Add to history
    TUI->>User: Display response
```

## Cost Tracking Flow

```mermaid
sequenceDiagram
    participant Agent as Worker Agent
    participant Log as Log File
    participant Parser as LogParser
    participant DB as CostDatabase
    participant Query as CostQuery
    participant TUI as TUI App

    Agent->>Log: Write usage JSON
    Note over Log: ~/.forge/logs/worker-1.log

    loop Every refresh interval
        TUI->>Parser: parse_directory()
        Parser->>Log: Read new entries
        Log-->>Parser: JSON lines
        Parser->>Parser: Extract tokens/cost
        Parser->>DB: insert_api_calls()

        TUI->>Query: get_today_costs()
        Query->>DB: SELECT from api_calls
        DB-->>Query: Cost rows
        Query-->>TUI: Aggregated costs
        TUI->>TUI: Update cost panel
    end
```

## Bead Queue Flow

```mermaid
sequenceDiagram
    participant TUI as TUI App
    participant Manager as BeadManager
    participant Store as bead store
    participant CLI as bead CLI (writes only)

    loop Every 30 seconds
        TUI->>Manager: poll_updates()
        Manager->>Store: read_all_beads() — no subprocess
        Note over Store: bead-rs checkpoint .beads/checkpoint/:<br/>current.json → active snapshot in objects/*.jsonl<br/>(fallback: forensic.jsonl)<br/>or legacy flat .beads/issues.jsonl (read-only compat)
        Store-->>Manager: Bead data
        Manager->>Manager: Compute ready / blocked / in-progress<br/>(bead list --ready semantics)

        Manager->>CLI: timeout_task() → bead release
        CLI-->>Store: Atomic claim release

        Manager-->>TUI: WorkspaceBeads
        TUI->>TUI: Update task panel
    end
```

## Health Check Flow

```mermaid
sequenceDiagram
    participant Monitor as HealthMonitor
    participant Status as Status File
    participant Tmux as tmux
    participant Agent as Worker Agent
    participant TUI as TUI App

    loop Every check interval
        Monitor->>Status: Read file
        Status-->>Monitor: WorkerStatusInfo

        Monitor->>Monitor: Check timestamp freshness
        Monitor->>Tmux: Check PID exists
        Tmux-->>Monitor: Process status

        alt Status stale
            Monitor->>Monitor: Set HealthLevel::Degraded
        else PID dead
            Monitor->>Monitor: Set HealthLevel::Critical
            Monitor->>TUI: Alert event
        else Memory over limit
            Monitor->>Monitor: Set HealthLevel::Critical
            Monitor->>Tmux: Kill session
        else All healthy
            Monitor->>Monitor: Set HealthLevel::Healthy
        end
    end
```

## Theme Hot-Reload Flow

```mermaid
sequenceDiagram
    participant User as User
    participant Editor as Text Editor
    participant File as config.yaml
    participant Watcher as ConfigWatcher
    participant TUI as TUI App

    User->>Editor: Edit ~/.forge/config.yaml
    Editor->>File: Save changes
    File-->>Watcher: inotify modify event
    Watcher->>Watcher: Debounce 50ms
    Watcher->>File: Read config
    Watcher->>Watcher: Validate YAML
    alt Valid config
        Watcher->>TUI: Apply new settings
        TUI->>TUI: Update theme/colors
        TUI->>TUI: Set dirty=true
    else Invalid config
        Watcher->>Watcher: Log warning
        Note over Watcher: Keep previous config
    end
```

## Component Data Dependencies

```mermaid
graph TB
    subgraph "Data Sources"
        StatusFiles[Status Files<br/>~/.forge/status/]
        LogFiles[Log Files<br/>~/.forge/logs/]
        BeadFiles[Bead Checkpoint<br/>.beads/checkpoint/]
        ConfigFile[Config File<br/>~/.forge/config.yaml]
    end

    subgraph "Processing Layer"
        StatusWatcher[StatusWatcher]
        LogParser[LogParser]
        BeadManager[BeadManager]
        ConfigWatcher[ConfigWatcher]
    end

    subgraph "Storage Layer"
        SQLite[(SQLite DB<br/>~/.forge/costs.db)]
    end

    subgraph "Application Layer"
        DataManager[DataManager]
        CostQuery[CostQuery]
        App[App State]
    end

    subgraph "Display Layer"
        Panels[TUI Panels]
    end

    StatusFiles --> StatusWatcher
    StatusWatcher --> DataManager
    DataManager --> App

    LogFiles --> LogParser
    LogParser --> SQLite
    SQLite --> CostQuery
    CostQuery --> DataManager

    BeadFiles --> BeadManager
    BeadManager --> DataManager

    ConfigFile --> ConfigWatcher
    ConfigWatcher --> App

    App --> Panels
```

## Related Documentation

- [Architecture Overview](../ARCHITECTURE.md) - System design
- [Worker Lifecycle](./worker-lifecycle.md) - Worker state machine
- [Event Flow](./event-flow.md) - Event handling pipeline
- [ADR 0020: Bead Write Authority](../adr/0020-bead-write-authority.md) -
  bead store format and write access
- [ADR 0007: Bead Integration Strategy](../adr/0007-bead-integration-strategy.md) -
  read-only consumer model
