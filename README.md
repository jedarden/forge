# FORGE

**F**ederated **O**rchestration & **R**esource **G**eneration **E**ngine

> Intelligent control panel for orchestrating AI coding agents across your workspace

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

---

## What is FORGE?

FORGE is a terminal-based control panel that intelligently manages multiple AI coding agents (Claude, GPT, Qwen, etc.) across your workspace. It automatically distributes tasks to the right models based on complexity and cost, optimizes your AI subscriptions, and provides real-time monitoring through a beautiful TUI.

**Think of it as Kubernetes for AI workers.**

---

## Key Features

### 🤖 Multi-Agent Orchestration
- Spawn and manage multiple AI coding agents (Claude Code, OpenCode, Aider, Cursor)
- Distribute work across different models and providers
- Real-time health monitoring and auto-recovery

### 💰 Cost Optimization
- Smart model routing based on task complexity (0-100 scoring)
- Maximize use-or-lose subscriptions before falling back to pay-per-token APIs
- Save 87-94% on AI costs with intelligent routing
- Real-time cost tracking and forecasting

### 📊 Beautiful TUI Dashboard
- Real-time worker status and health metrics
- Task queue visualization with bead integration
- Live activity logs and performance metrics
- Conversational CLI interface for interactive control
- Responsive layouts for multiple terminal sizes (199×38, 199×55, and more)

### 🔄 Self-Updating Binary
- Hot-reload capability for live updates
- Atomic binary swaps (no downtime)
- State preservation across updates
- One-key update process

### 🎯 Task Intelligence
- Automatic task value scoring (P0-P4 priority)
- Model capability matching
- Dependency-aware scheduling
- Bead-level locking for multi-worker coordination

---

## Architecture

FORGE operates as a **federated orchestration system**:

1. **Resource Generation**: Spawn AI workers on-demand across different providers
2. **Intelligent Routing**: Route tasks based on complexity, cost, and model capabilities
3. **Federated Control**: Coordinate workers across multiple workspaces and clusters
4. **Real-time Orchestration**: Monitor, adjust, and optimize continuously

```
┌─────────────────────────────────────────────────────┐
│                    FORGE Control Panel               │
│  Federated Orchestration & Resource Generation      │
└─────────────────────────────────────────────────────┘
           │
           ├─→ Worker Pool (Claude Sonnet, GPT-4, Qwen)
           ├─→ Cost Optimizer (Subscription vs API routing)
           ├─→ Task Scheduler (Value scoring, dependency tracking)
           ├─→ Health Monitor (Auto-recovery, alerts)
           └─→ Bead Integration (Issue tracking, coordination)
```

---

## Quick Start

```bash
# Installation (coming soon)
pip install llmforge

# Launch the control panel
forge dashboard

# Spawn workers
forge spawn --model=sonnet --count=3

# Check status
forge status

# Optimize cost routing
forge optimize
```

---

## Project Status

🚧 **Active Development** - Research phase complete, implementation in progress

See [research/control-panel](./research/) for comprehensive design documentation:
- TUI framework analysis (Textual/Ratatui)
- Multi-model routing algorithms
- Cost optimization strategies
- Conversational interface design
- Hot-reload and atomic update mechanisms
- Dashboard mockups for multiple terminal sizes

---

## Technology Stack

- **TUI Framework**: Ratatui (Rust) or Textual (Python) - TBD
- **Language**: Rust (for performance and atomic binary updates)
- **State Management**: SQLite + JSONL (via Beads)
- **Update Mechanism**: Atomic binary swap using rename() syscall
- **Distribution**: Multi-platform builds (Linux, macOS, Windows)

---

## Why "FORGE"?

The name reflects the core mission:

- **Federated**: Coordinate across multiple AI providers and models
- **Orchestration**: Intelligent task distribution and scheduling
- **Resource**: Optimize subscription and API usage
- **Generation**: Spawn workers dynamically based on demand
- **Engine**: Powerful, efficient, reliable automation

Like a blacksmith's forge that transforms raw materials into refined tools, FORGE transforms AI resources into optimized, coordinated intelligence.

---

## Development Roadmap

### Phase 1: MVP (Current)
- [ ] Basic TUI dashboard implementation
- [ ] Worker spawning and management
- [ ] Simple task queue integration
- [ ] Real-time status monitoring

### Phase 2: Intelligence
- [ ] Task value scoring algorithm
- [ ] Multi-model routing engine
- [ ] Cost optimization logic
- [ ] Subscription tracking

### Phase 3: Advanced Features
- [ ] Conversational CLI interface
- [ ] Hot-reload and self-updating
- [ ] Advanced health monitoring
- [ ] Performance analytics

### Phase 4: Enterprise
- [ ] Multi-workspace coordination
- [ ] Team collaboration features
- [ ] Audit logs and compliance
- [ ] Advanced RBAC

---

## Contributing

Contributions welcome! This project is in active development.

1. Check out the [research documentation](./research/) to understand the architecture
2. Open issues for bugs, features, or questions
3. Submit PRs with clear descriptions and tests

---

## License

MIT License - see [LICENSE](LICENSE) for details

---

## Research & Documentation

Comprehensive research documentation available in [`research/control-panel/`](./research/):

- **Naming Analysis**: Why "FORGE" and alternatives considered
- **TUI Design**: Dashboard layouts, responsive strategies
- **Cost Optimization**: Subscription vs API analysis, 87-94% savings strategies
- **Model Routing**: Task scoring algorithms, capability matching
- **Conversational Interface**: Chat history, tool transparency, scrolling
- **Update Mechanisms**: Hot-reload, atomic binary swaps, state preservation

---

## Acknowledgments

Built with insights from the AI coding tools community, optimized for real-world use cases where managing multiple AI subscriptions and workers becomes complex.

**FORGE** - *Where AI agents are forged, orchestrated, and optimized.*
