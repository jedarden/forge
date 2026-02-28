# FORGE Architecture Diagrams

This directory contains Mermaid diagrams illustrating FORGE's system architecture.

## Available Diagrams

| File | Description |
|------|-------------|
| [architecture.md](./architecture.md) | High-level system architecture (this file) |
| [data-flow.md](./data-flow.md) | Data flow between components |
| [worker-lifecycle.md](./worker-lifecycle.md) | Worker spawning and management |
| [event-flow.md](./event-flow.md) | Event handling pipeline |

## Usage

These diagrams can be viewed in:
- **GitHub/GitLab**: Native Mermaid rendering
- **VS Code**: With Mermaid preview extension
- **CLI**: `mmdc` command to convert to images
- **Online**: https://mermaid.live

---

## High-Level System Architecture

```mermaid
graph TB
    subgraph "User Interface Layer"
        TUI[forge-tui<br/>Ratatui TUI]
        Chat[Chat Interface]
        Help[Help Overlay]
    end

    subgraph "Application Layer"
        App[App State Machine]
        Input[InputHandler]
        Views[View Controllers]
        Theme[ThemeManager]
    end

    subgraph "Service Layer"
        Core[forge-core<br/>Types & Utilities]
        Worker[forge-worker<br/>Worker Management]
        Cost[forge-cost<br/>Cost Tracking]
        ChatBackend[forge-chat<br/>AI Backend]
        Init[forge-init<br/>Onboarding]
    end

    subgraph "External Systems"
        Tmux[Tmux Sessions]
        Beads[Bead Task Queues]
        Logs[Log Files]
        DB[(SQLite DB)]
        Claude[Anthropic API]
        Status[Status Files]
    end

    TUI --> App
    Chat --> ChatBackend
    ChatBackend --> Claude

    App --> Views
    App --> Input
    App --> Theme

    Views --> Worker
    Views --> Cost
    Views --> ChatBackend

    Worker --> Tmux
    Worker --> Beads
    Worker --> Logs
    Worker --> Status

    Cost --> Logs
    Cost --> DB

    Core --> App
    Core --> Worker
    Core --> Cost

    Init --> Core
```

## Crate Dependency Graph

```mermaid
graph TB
    subgraph "Binary"
        forge[forge<br/>main.rs]
    end

    subgraph "Workspace Crates"
        core[forge-core<br/>Types, Errors, Watcher]
        tui[forge-tui<br/>Terminal UI]
        worker[forge-worker<br/>Worker Management]
        cost[forge-cost<br/>Cost Tracking]
        chat[forge-chat<br/>AI Backend]
        config[forge-config<br/>Configuration]
        init[forge-init<br/>Onboarding]
    end

    forge --> core
    forge --> tui
    forge --> init

    tui --> core
    tui --> worker
    tui --> cost
    tui --> chat

    worker --> core
    worker --> cost

    chat --> core
    chat --> worker
    chat --> cost

    cost --> core

    config --> core

    init --> core
```

## Component Interaction

```mermaid
sequenceDiagram
    participant User
    participant TUI
    participant Worker
    participant Cost
    participant Chat
    participant External

    User->>TUI: Keyboard input
    TUI->>TUI: Handle event
    TUI->>Worker: Spawn/kill request
    Worker->>External: Tmux command
    External-->>Worker: Result
    Worker-->>TUI: WorkerHandle

    User->>TUI: Chat query
    TUI->>Chat: Process query
    Chat->>External: API call
    External-->>Chat: Response
    Chat-->>TUI: ChatResponse

    loop Every refresh
        Worker->>TUI: Status update
        Cost->>TUI: Cost data
    end
```

## View System

```mermaid
graph LR
    subgraph "Available Views"
        Overview[Overview<br/>o]
        Workers[Workers<br/>w]
        Tasks[Tasks<br/>t]
        Costs[Costs<br/>c]
        Metrics[Metrics<br/>m]
        Logs[Logs<br/>l]
        Subs[Subscriptions<br/>u]
        Alerts[Alerts<br/>a]
        Chat[Chat<br/>:]
    end

    Overview --> Workers --> Tasks --> Costs
    Costs --> Metrics --> Logs --> Subs
    Subs --> Alerts --> Chat --> Overview
```

## Layout Modes

```mermaid
graph TB
    subgraph "Terminal Width Detection"
        Check{Width?}
    end

    Check -->|< 120 cols| Narrow[Single View Mode]
    Check -->|120-198 cols| Wide[2-Column Layout]
    Check -->|199+ cols| UltraWide[3-Column Layout]

    Narrow --> N1[Current view only]
    Wide --> W1[4 panels visible]
    UltraWide --> U1[6 panels visible]
```

## Data Storage Architecture

```mermaid
graph TB
    subgraph "File System"
        Home[~/.forge/]
        Status[status/*.json<br/>Worker status]
        Logs[logs/*.log<br/>Worker logs]
        Config[config.yaml<br/>User config]
        DB[costs.db<br/>SQLite database]
    end

    subgraph "Database Tables"
        APICalls[api_calls]
        DailyCosts[daily_costs]
        ModelCosts[model_costs]
        HourlyStats[hourly_stats]
        DailyStats[daily_stats]
        WorkerEff[worker_efficiency]
        ModelPerf[model_performance]
        Subs[subscriptions]
        SubUsage[subscription_usage]
    end

    Home --> Status
    Home --> Logs
    Home --> Config
    Home --> DB

    DB --> APICalls
    DB --> DailyCosts
    DB --> ModelCosts
    DB --> HourlyStats
    DB --> DailyStats
    DB --> WorkerEff
    DB --> ModelPerf
    DB --> Subs
    DB --> SubUsage
```

## Chat Backend Architecture

```mermaid
graph TB
    subgraph "Chat Interface"
        Input[User Input]
        History[Chat History]
        Display[Response Display]
    end

    subgraph "ChatBackend"
        RateLimit[Rate Limiter<br/>10 cmd/min]
        Context[Context Provider<br/>Dashboard State]
        Audit[Audit Logger<br/>JSONL]
        Registry[Tool Registry]
    end

    subgraph "Providers"
        ClaudeAPI[Claude API<br/>HTTP]
        ClaudeCLI[Claude CLI<br/>Subprocess]
        MockProvider[Mock Provider<br/>Testing]
    end

    subgraph "Tools"
        WorkerStatus[worker_status]
        TaskQueue[task_queue]
        CostAnalytics[cost_analytics]
        SpawnWorker[spawn_worker]
        KillWorker[kill_worker]
    end

    Input --> RateLimit
    RateLimit --> Context
    Context --> Registry
    Registry --> ClaudeAPI
    Registry --> ClaudeCLI
    Registry --> MockProvider

    Registry --> WorkerStatus
    Registry --> TaskQueue
    Registry --> CostAnalytics
    Registry --> SpawnWorker
    Registry --> KillWorker

    ClaudeAPI --> Audit
    ClaudeCLI --> Audit

    Audit --> Display
    Audit --> History
```

## Health Monitoring

```mermaid
flowchart LR
    subgraph "Health Checks"
        PID[PID Check]
        Activity[Activity Check]
        Memory[Memory Check]
        Response[Response Ping]
    end

    subgraph "Health Levels"
        Healthy[🟢 Healthy]
        Degraded[🟡 Degraded]
        Critical[🔴 Critical]
        Failed[⚫ Failed]
    end

    subgraph "Actions"
        None[No Action]
        Warn[Warning Alert]
        Restart[Auto-restart]
        Notify[User Notification]
    end

    PID -->|Valid| Activity
    PID -->|Invalid| Critical
    Activity -->|Fresh| Memory
    Activity -->|Stale| Degraded
    Memory -->|OK| Healthy
    Memory -->|Over Limit| Critical

    Healthy --> None
    Degraded --> Warn
    Critical --> Restart
    Restart -->|Success| Healthy
    Restart -->|Fail| Failed
    Failed --> Notify
```

## Task Routing

```mermaid
flowchart TB
    subgraph "Input"
        Beads[Ready Beads]
    end

    subgraph "Router"
        Scorer[Task Scorer]
        Matcher[Tier Matcher]
        Assigner[Assignment]
    end

    subgraph "Worker Tiers"
        Premium[Premium<br/>Opus 4.6]
        Standard[Standard<br/>Sonnet/GLM]
        Budget[Budget<br/>Haiku]
    end

    subgraph "Output"
        Assignment[Worker Assignment]
    end

    Beads --> Scorer
    Scorer -->|Priority Score| Matcher
    Matcher -->|P0/P1| Premium
    Matcher -->|P2| Standard
    Matcher -->|P3/P4| Budget

    Premium --> Assigner
    Standard --> Assigner
    Budget --> Assigner

    Assigner --> Assignment
```

## Error Recovery

```mermaid
stateDiagram-v2
    [*] --> Normal

    Normal --> Error: Exception thrown
    Error --> Categorize: Catch error

    Categorize --> Retryable: Transient error
    Categorize --> Blockable: Config error
    Categorize --> Fatal: Critical error

    Retryable --> Retry: Auto-retry enabled
    Retry --> Normal: Success
    Retry --> Retryable: Retry exhausted

    Blockable --> Guidance: Show guidance
    Guidance --> Normal: User fixes
    Guidance --> Blocked: User skips

    Fatal --> Shutdown: Cannot continue

    Blocked --> [*]
    Shutdown --> [*]
```

## Configuration Hot-Reload

```mermaid
sequenceDiagram
    participant User
    participant Editor as Text Editor
    participant File as config.yaml
    participant Watcher as ConfigWatcher
    participant App as App State

    User->>Editor: Edit config
    Editor->>File: Save
    File-->>Watcher: inotify event

    Watcher->>Watcher: Debounce 50ms
    Watcher->>File: Read changes
    Watcher->>Watcher: Validate YAML

    alt Valid Config
        Watcher->>App: Apply settings
        App->>App: Update theme/refresh
        App-->>User: UI updated
    else Invalid Config
        Watcher->>Watcher: Log warning
        Note over Watcher: Keep previous config
    end
```

---

## Related Documentation

- [Architecture Documentation](../ARCHITECTURE.md) - Detailed system design
- [Database Schema](../DATABASE.md) - Database documentation
- [Workers System](../WORKERS.md) - Worker management details
- [Events System](../EVENTS.md) - Event handling documentation
- [UI Architecture](../UI.md) - TUI design details
