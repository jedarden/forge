# ADR 0002: Use Text User Interface (TUI) for Control Panel

**Date**: 2026-02-07
**Status**: Accepted
**Deciders**: Jed Arden, Claude Sonnet 4.5

---

## Context

FORGE needs a primary user interface for monitoring and controlling AI worker orchestration. Options considered:
1. Web-based dashboard (HTTP server + browser UI)
2. Command-line interface (CLI) only
3. Text User Interface (TUI) in terminal
4. Desktop GUI application

Key requirements:
- Real-time monitoring of worker status
- Task queue visualization
- Cost tracking and metrics
- Conversational interface for queries
- Must run in terminal environment (SSH, tmux sessions)
- Should support multiple terminal sizes
- Low resource overhead

---

## Decision

Use a **Text User Interface (TUI)** as the primary control panel interface.

Primary frameworks considered:
- **Textual** (Python) - Rich TUI library with excellent widget support
- **Ratatui** (Rust) - High-performance TUI library for Rust

---

## Rationale

### Why TUI over alternatives

**TUI vs Web Dashboard**:
- ✅ No HTTP server needed (simpler deployment)
- ✅ Works seamlessly in SSH/tmux sessions
- ✅ Lower resource overhead
- ✅ Native terminal integration
- ✅ Better security (no exposed ports)
- ❌ Less familiar for non-technical users

**TUI vs CLI only**:
- ✅ Real-time updates without polling
- ✅ Richer visualization (tables, graphs, panels)
- ✅ Better for monitoring tasks
- ✅ Multiple views simultaneously
- ❌ More complex to implement

**TUI vs Desktop GUI**:
- ✅ Works over SSH
- ✅ Works in headless environments
- ✅ Consistent with developer workflows
- ✅ No additional dependencies
- ❌ Less polished appearance

### Framework Selection: Textual vs Ratatui

**Textual (Python)**:
- ✅ Rapid prototyping
- ✅ Excellent documentation
- ✅ Rich widget library
- ✅ CSS-like styling
- ✅ Python ecosystem integration
- ❌ Performance overhead (Python runtime)
- ❌ Larger binary distribution

**Ratatui (Rust)**:
- ✅ High performance
- ✅ Small binary size (<5MB)
- ✅ Better for atomic binary updates
- ✅ Type safety
- ❌ Steeper learning curve
- ❌ More boilerplate code

**Decision**: Start with **Textual** for MVP, migrate to **Ratatui** if performance/distribution becomes critical.

---

## Design Decisions

### Responsive Layouts

Support multiple terminal sizes:
- **199×55** (ultra-wide, tall): 3-column layout with extended panels
- **199×38** (ultra-wide, standard): 3-column compact layout
- **140-179×30** (wide): 2-column with toggle
- **100-139×25** (standard): Tabbed interface
- **80-99×24** (narrow): Accordion panels
- **<80 cols** (minimal): CLI fallback

### Key Panels

1. **Worker Status**: Real-time worker health, model types, workspaces
2. **Task Queue**: Bead-based task list with priorities
3. **Activity Log**: Live event stream
4. **Cost Tracking**: Real-time cost metrics, subscription usage
5. **Performance Metrics**: Throughput, latency, success rates
6. **Command Input**: Conversational interface (`:` key activation)

### Conversational Interface

- Press `:` to activate command input
- Backend: Restricted Claude Code/OpenCode instance
- Chat history: 10-turn context window
- Tool call transparency: Show agent actions with cancel option
- Scrolling support: Pagination, truncation, auto-scroll

---

## Consequences

### Positive
- Developer-friendly interface (terminal-native)
- Works in all deployment environments (SSH, tmux, local)
- Low resource overhead
- Real-time updates without polling
- Responsive design for different terminal sizes
- Can fall back to CLI for scripting

### Negative
- Requires terminal emulator (no browser fallback)
- Learning curve for TUI navigation (mitigated with keyboard shortcuts)
- Less accessible for non-technical users
- Complex implementation for responsive layouts

### Neutral
- Framework choice (Textual vs Ratatui) deferred until MVP validation
- Can add web dashboard later as complementary interface
- Mobile access limited (requires SSH client)

---

## Implementation Plan

### Phase 1: Basic TUI (MVP)
- [ ] Basic 3-column layout for 199×38 terminal
- [ ] Worker status panel with live updates
- [ ] Task queue panel (static beads list)
- [ ] Simple activity log
- [ ] Keyboard shortcuts (h: help, q: quit, r: refresh)

### Phase 2: Intelligence
- [ ] Real-time cost tracking panel
- [ ] Performance metrics visualization
- [ ] Responsive layout support (5 breakpoints)
- [ ] Enhanced keyboard navigation

### Phase 3: Conversational
- [ ] Command input activation (`:` key)
- [ ] Conversational interface integration
- [ ] Chat history management
- [ ] Tool call transparency

### Phase 4: Polish
- [ ] Scrolling support for large messages
- [ ] Color themes
- [ ] Custom keybindings
- [ ] Export/screenshot functionality

---

## Alternatives Considered

### Web Dashboard
- **Pros**: Familiar interface, remote access, rich visualizations
- **Cons**: Requires HTTP server, port exposure, heavier resource usage
- **Verdict**: Not selected for MVP, could add later as complementary interface

### CLI Only
- **Pros**: Simplest to implement, scriptable, universal
- **Cons**: No real-time updates, poor monitoring experience, no rich visualization
- **Verdict**: CLI commands still available (`forge status`, `forge spawn`), but TUI is primary interface

### Desktop GUI (Electron, Tauri)
- **Pros**: Rich UI capabilities, familiar to users
- **Cons**: Doesn't work over SSH, heavy resource usage, not terminal-native
- **Verdict**: Not aligned with developer workflow

---

## References

- [Dashboard Design](../notes/dashboard-design.md)
- [Dashboard Mockup 199×38](../notes/dashboard-mockup-199x38.md)
- [Dashboard Mockup 199×55](../notes/dashboard-mockup-199x55.md)
- [Responsive Layout Strategy](../notes/responsive-layout-strategy.md)
- [Conversational Interface Design](../notes/conversational-interface.md)
- [TUI Framework Comparison](../notes/tui-framework-comparison-matrix.md)
