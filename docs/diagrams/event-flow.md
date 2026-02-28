# FORGE Event Flow Diagrams

This document illustrates how events flow through the FORGE system.

## Event Loop Overview

```mermaid
flowchart TB
    subgraph "Event Sources"
        Keyboard[Keyboard Input]
        Resize[Terminal Resize]
        StatusWatch[StatusWatcher]
        LogWatch[LogWatcher]
        BeadPoll[BeadManager Poll]
        ChatResp[Chat Response]
        ConfigChange[Config Change]
    end

    subgraph "Event Loop"
        Poll[Poll Events<br/>50ms timeout]
        Handle[Handle Event]
        Update[Update State]
        Check[Check Background]
        Render{Dirty?}
        Draw[Render Frame]
    end

    subgraph "Output"
        Terminal[Terminal Display]
        Actions[Side Effects]
    end

    Keyboard --> Poll
    Resize --> Poll
    StatusWatch --> Poll
    LogWatch --> Poll
    BeadPoll --> Poll
    ChatResp --> Poll
    ConfigChange --> Poll

    Poll --> Handle --> Update --> Check --> Render
    Render -->|Yes| Draw
    Render -->|No| Poll
    Draw --> Terminal
    Update --> Actions
    Actions --> Terminal
```

## AppEvent Categories

```mermaid
mindmap
    root((AppEvent))
        Navigation
            SwitchView
            NextView
            PrevView
            NavigateUp/Down
            PageUp/Down
            GoToTop/Bottom
        Worker Actions
            SpawnWorker
            KillWorker
            PauseWorker
            ResumeWorker
            PauseAllWorkers
            ResumeAllWorkers
        Text Input
            TextInput
            Backspace
            Submit
        Configuration
            OpenConfig
            CycleTheme
            OpenBudgetConfig
            OpenWorkerConfig
        Alerts
            AcknowledgeAlert
            AcknowledgeAllAlerts
        Application
            Quit
            ForceQuit
            Refresh
            Update
            Cancel
        Help
            ShowHelp
            HideHelp
```

## Input Handling Flow

```mermaid
flowchart TB
    subgraph "Input"
        Key[Key Event]
    end

    subgraph "InputHandler"
        Mode{Chat Mode?}
        ChatInput[handle_chat_input]
        NormalInput[handle_normal_mode]
    end

    subgraph "Event Generation"
        Chat[Chat Events<br/>TextInput/Backspace/Submit]
        Nav[Navigation Events<br/>View switching]
        Action[Action Events<br/>Spawn/Kill/Pause]
    end

    Key --> Mode
    Mode -->|Yes| ChatInput --> Chat
    Mode -->|No| NormalInput --> Nav
    NormalInput --> Action
```

## State Update Flow

```mermaid
stateDiagram-v2
    [*] --> Idle: App starts

    Idle --> Processing: Event received

    Processing --> Updating: State change needed
    Processing --> Idle: No action (None event)

    Updating --> Dirty: Data modified
    Updating --> Processing: Multiple changes

    Dirty --> Rendering: Frame ready
    Rendering --> Idle: Frame complete

    Processing --> Blocked: Waiting for async
    Blocked --> Processing: Async completes

    Idle --> Exiting: Quit event
    Exiting --> [*]: Cleanup done
```

## Dirty Flag Pattern

```mermaid
sequenceDiagram
    participant Loop as Event Loop
    participant App as App State
    participant Render as Renderer
    participant Terminal as Terminal

    Loop->>App: Handle event
    App->>App: Modify data
    App->>App: Set dirty=true

    Loop->>App: Check dirty
    App-->>Loop: dirty=true

    Loop->>Render: render_frame()
    Render->>Terminal: Draw widgets
    Render->>App: Set dirty=false

    Loop->>App: Check dirty
    App-->>Loop: dirty=false

    Note over Loop: Skip render, continue polling
```

## Background Event Sources

```mermaid
graph TB
    subgraph "Background Threads"
        StatusThread[StatusWatcher Thread]
        LogThread[LogWatcher Thread]
        ConfigThread[ConfigWatcher Thread]
        ChatThread[Chat Backend Thread]
    end

    subgraph "Channels"
        StatusRX[StatusEvent RX]
        LogRX[LogEvent RX]
        ConfigRX[ConfigEvent RX]
        ChatRX[ChatResponse RX]
    end

    subgraph "Main Loop"
        Poll[Poll All Channels]
        Process[Process Events]
    end

    StatusThread -->|mpsc| StatusRX
    LogThread -->|mpsc| LogRX
    ConfigThread -->|mpsc| ConfigRX
    ChatThread -->|mpsc| ChatRX

    StatusRX --> Poll
    LogRX --> Poll
    ConfigRX --> Poll
    ChatRX --> Poll

    Poll --> Process
```

## View-Specific Key Bindings

```mermaid
graph TB
    subgraph "Global Keys"
        CtrlC[Ctrl+C] --> ForceQuit
        CtrlL[Ctrl+L] --> Refresh
        CtrlU[Ctrl+U] --> Update
        Esc[Esc] --> Cancel
    end

    subgraph "View Hotkeys"
        O[o] --> Overview
        W[w] --> Workers
        T[t] --> Tasks
        C[c] --> Costs
        M[m] --> Metrics
        L[l] --> Logs
        U[u] --> Subscriptions
        A[a] --> Alerts
        Colon[: ] --> Chat
    end

    subgraph "Workers View"
        G[g] --> SpawnGLM
        S[s] --> SpawnSonnet
        O2[o] --> SpawnOpus
        H[h] --> SpawnHaiku
        K[k] --> KillWorker
        P[p] --> PauseWorker
        R[r] --> ResumeWorker
    end

    subgraph "Alerts View"
        Enter[Enter] --> AckAlert
        A2[A] --> AckAll
    end
```

## Chat Event Flow

```mermaid
sequenceDiagram
    participant User as User
    participant TUI as TUI App
    participant TX as Request TX
    participant RX as Response RX
    participant Backend as ChatBackend
    participant API as Claude API

    User->>TUI: Type query
    TUI->>TUI: Build input string

    User->>TUI: Press Enter
    TUI->>TUI: Create ChatRequest
    TUI->>TX: send(request)
    Note over TUI: Non-blocking, returns immediately
    TUI->>TUI: chat_pending = true
    TUI->>TUI: dirty = true

    TX->>Backend: receive(request)
    Backend->>Backend: Inject context
    Backend->>Backend: Check rate limit
    Backend->>API: API request
    API-->>Backend: Response
    Backend->>RX: send(response)

    loop Every poll cycle
        TUI->>RX: try_recv()
    end

    RX-->>TUI: response
    TUI->>TUI: chat_pending = false
    TUI->>TUI: Add to history
    TUI->>TUI: dirty = true
    TUI->>User: Display response
```

## Status Event Types

```mermaid
classDiagram
    class StatusEvent {
        +WorkerStarted(WorkerId)
        +WorkerUpdated(WorkerId, WorkerStatusInfo)
        +WorkerStopped(WorkerId)
        +WorkerFailed(WorkerId, String)
    }

    class WorkerStatusInfo {
        +String worker_id
        +WorkerStatus status
        +String current_task
        +DateTime last_update
        +int pid
    }

    StatusEvent --> WorkerStatusInfo
```

## Log Event Types

```mermaid
classDiagram
    class LogWatcherEvent {
        +NewLogEntry(LogEntry)
        +LogRotated
        +WorkerLogCreated(WorkerId)
    }

    class LogEntry {
        +DateTime timestamp
        +LogLevel level
        +String message
        +Option~String~ worker_id
    }

    LogWatcherEvent --> LogEntry
```

## Bead Event Types

```mermaid
classDiagram
    class BeadEvent {
        +QueueUpdated(Vec~Bead~)
        +BeadAssigned(BeadId, WorkerId)
        +BeadCompleted(BeadId)
        +BeadBlocked(BeadId, Vec~BeadId~)
    }

    class Bead {
        +String id
        +String title
        +String status
        +u8 priority
        +Vec~String~ labels
    }

    BeadEvent --> Bead
```

## Error Recovery Events

```mermaid
flowchart TB
    Error[Error Occurs] --> Category{Error Category?}

    Category -->|Database| DB[DatabaseError]
    Category -->|Network| Net[NetworkError]
    Category -->|Config| Cfg[ConfigError]
    Category -->|Worker| WC[WorkerError]
    Category -->|Chat| Chat[ChatError]

    DB --> Guidance[Show Guidance Modal]
    Net --> Guidance
    Cfg --> Guidance
    WC --> Guidance
    Chat --> Guidance

    Guidance --> Action{User Action?}

    Action -->|Retry| Retry[Retry Operation]
    Action -->|Ignore| Dismiss[Dismiss Error]
    Action -->|Fix| Manual[Manual Fix]

    Retry --> Result{Success?}
    Result -->|Yes| Continue[Continue]
    Result -->|No| Guidance

    Dismiss --> Continue
    Manual --> Blocked[Block Feature]
```

## Event Priority Order

```mermaid
graph TB
    subgraph "Polling Priority"
        P1[1. Keyboard Input<br/>Blocking with timeout]
        P2[2. Status Events<br/>Non-blocking try_recv]
        P3[3. Log Events<br/>Non-blocking try_recv]
        P4[4. Chat Responses<br/>Non-blocking try_recv]
        P5[5. Config Changes<br/>Non-blocking try_recv]
        P6[6. Timer Events<br/>Interval-based]
    end

    P1 --> P2 --> P3 --> P4 --> P5 --> P6
```

## Timing Targets

| Operation | Target | Notes |
|-----------|--------|-------|
| Key press → Event | < 1ms | Direct mapping |
| State update | < 5ms | In-memory changes |
| Frame render | < 16ms | 60 FPS target |
| Background poll | 50ms timeout | Balance CPU/response |
| Bead poll interval | 30s | External CLI calls |
| Status watch latency | < 100ms | File system events |

## Event Batching

```mermaid
sequenceDiagram
    participant Events as Multiple Events
    participant Batch as Batch Processor
    participant State as App State
    participant Render as Renderer

    Events->>Batch: Event 1
    Events->>Batch: Event 2
    Events->>Batch: Event 3

    Batch->>State: Apply all updates
    State->>State: dirty = true (once)

    Batch->>Render: Single render call
    Render-->>Events: Frame displayed

    Note over Events,Render: Multiple events → One render
```

## Related Documentation

- [Architecture Overview](./ARCHITECTURE.md) - System design
- [Events Documentation](../EVENTS.md) - Detailed event docs
- [Data Flow](./data-flow.md) - Data movement
- [Worker Lifecycle](./worker-lifecycle.md) - Worker states
