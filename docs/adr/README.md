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
