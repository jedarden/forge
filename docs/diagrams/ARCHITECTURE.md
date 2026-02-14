# FORGE Architecture Diagrams

This directory contains Mermaid diagrams illustrating FORGE's system architecture.

## Available Diagrams

- [architecture.md](./architecture.md) - High-level system architecture
- [data-flow.md](./data-flow.md) - Data flow between components
- [worker-lifecycle.md](./worker-lifecycle.md) - Worker spawning and management
- [event-flow.md](./event-flow.md) - Event handling pipeline

## Usage

These diagrams can be viewed in:
- GitHub/GitLab: Native Mermaid rendering
- VS Code: With Mermaid preview extension
- CLI: `mmdc` command to convert to images
- Online: https://mermaid.live

## Diagram: High-Level Architecture

```mermaid
graph TB
    subgraph "User Interface"
        TUI[Ratatui TUI]
        Chat[Chat Interface]
    end

    subgraph "Core Services"
        Core[forge-core]
        TUI[forge-tui]
        Worker[forge-worker]
        Cost[forge-cost]
        ChatBackend[forge-chat]
        Init[forge-init]
    end

    subgraph "External Systems"
        Tmux[Tmux Sessions]
        Beads[Bead Task Queues]
        Logs[Log Files]
        DB[(SQLite DB)]
        Claude[Anthropic API]
    end

    TUI --> Core
    Chat --> ChatBackend
    ChatBackend --> Claude

    Worker --> Tmux
    Worker --> Beads
    Worker --> Logs

    Cost --> Logs
    Cost --> DB

    TUI --> Worker
    TUI --> Cost

    Init --> Core
```

## Diagram: Data Flow

```mermaid
sequenceDiagram
    participant User
    participant TUI
    participant Worker
    participant Agent
    participant Beads

    User->>TUI: Press key
    TUI->>TUI: Handle event
    TUI->>Worker: Spawn request
    Worker->>Agent: Start in tmux
    Agent->>Beads: Read tasks
    Beads-->>Agent: Task list
    Agent->>Agent: Work on task
    Agent->>TUI: Status update
    TUI->>User: Display update
```

## Diagram: Worker Lifecycle

```mermaid
stateDiagram-v2
    [*] --> Spawning: Launcher.spawn()
    Spawning --> Created: Tmux session OK
    Created --> Starting: Agent initializing
    Starting --> Active: Working on task
    Active --> Idle: Task complete
    Idle --> Active: New task assigned
    Active --> Failed: Crash/error
    Failed --> [*]
    Idle --> Stopped: User killed
    Active --> Stopped: User killed
    Stopped --> [*]
```

## Diagram: Event Flow

```mermaid
graph LR
    Input[Keyboard] --> Handler[InputHandler]
    Handler --> Event[AppEvent]
    Event --> Router[EventRouter]

    Router -->|View| View[View Handler]
    Router -->|Worker| Worker[Worker Action]
    Router -->|Chat| Chat[Chat Submit]

    View --> State[State Update]
    Worker --> State
    Chat --> State

    State --> Render[Frame Render]
    Render --> Screen[Terminal Display]
```
