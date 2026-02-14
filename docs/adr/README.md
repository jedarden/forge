# Architecture Decision Records (ADRs)

This directory contains Architecture Decision Records for the FORGE project.

## What are ADRs?

Architecture Decision Records document significant architectural decisions made during the development of FORGE. Each ADR captures:
- **Context**: The situation and problem being addressed
- **Decision**: The architectural choice made
- **Rationale**: Why this decision was made over alternatives
- **Consequences**: The positive, negative, and neutral outcomes

## ADR Index

| ADR | Title | Status | Date |
|-----|-------|--------|------|
| [0001](0001-use-forge-as-project-name.md) | Use "FORGE" as Project Name | Accepted | 2026-02-07 |
| [0002](0002-use-tui-for-control-panel-interface.md) | Use Text User Interface (TUI) for Control Panel | Accepted | 2026-02-07 |
| [0003](0003-cost-optimization-strategy.md) | Cost Optimization Strategy - Subscription-First Routing | Accepted | 2026-02-07 |
| [0004](0004-tool-based-conversational-interface.md) | Tool-Based Conversational Interface | Accepted | 2026-02-07 |
| [0005](0005-dumb-orchestrator-architecture.md) | Dumb Orchestrator Architecture | Accepted | 2026-02-07 |
| [0006](0006-technology-stack-selection.md) | Technology Stack Selection | Accepted | 2026-02-07 |
| [0007](0007-bead-integration-strategy.md) | Bead Integration Strategy | Accepted | 2026-02-07 |
| [0008](0008-real-time-update-architecture.md) | Real-Time Update Architecture | Accepted | 2026-02-07 |
| [0009](0009-dual-role-architecture.md) | Dual Role Architecture - Orchestrator First, Dashboard Second | Accepted | 2026-02-07 |
| [0010](0010-security-and-credential-management.md) | Security & Credential Management | Accepted | 2026-02-07 |
| [0014](0014-error-handling-strategy.md) | Error Handling Strategy | Accepted | 2026-02-08 |
| [0015](0015-bead-aware-launcher-protocol.md) | Bead-Aware Launcher Protocol | Accepted | 2026-02-08 |
| [0016](0016-onboarding-and-cli-detection.md) | Onboarding Flow and CLI Worker Detection | Accepted | 2026-02-08 |
| [0017](0017-tmux-based-testing-with-cleanup.md) | Tmux-Based Testing with Agent Control and Cleanup | Accepted | 2026-02-13 |

## ADR Statuses

- **Proposed**: Under consideration
- **Accepted**: Decision made and implemented
- **Deprecated**: No longer relevant
- **Superseded**: Replaced by a newer ADR

## Creating a New ADR

1. Copy the template (if available)
2. Number sequentially (next available number)
3. Use clear, descriptive title
4. Fill in all sections
5. Reference related ADRs and documentation

## Format

Each ADR follows this structure:

```markdown
# ADR NNNN: Title

**Date**: YYYY-MM-DD
**Status**: Proposed | Accepted | Deprecated | Superseded
**Deciders**: Names

## Context
What is the issue we're facing?

## Decision
What are we doing?

## Rationale
Why this choice over alternatives?

## Consequences
What are the outcomes?

## Alternatives Considered
What other options did we evaluate?

## References
Links to related docs
```

---

**FORGE** - Federated Orchestration & Resource Generation Engine
