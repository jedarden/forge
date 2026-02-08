# ADR 0006: Technology Stack Selection

**Status**: Superseded by revision (2026-02-08)
**Date**: 2026-02-07 (Original), 2026-02-08 (Revised)
**Deciders**: FORGE Architecture Team

---

## Revision History

**2026-02-08**: Decision reversed to Rust + Ratatui after Python/Textual prototype revealed framework brittleness (property conflicts, coroutine warnings, easy to break internals). Long-term robustness prioritized over short-term development speed.

**2026-02-07**: Original decision for Python + Textual (now superseded).

---

## Context

FORGE requires a TUI framework and implementation language to build the orchestrator. Initial prototyping with Python + Textual revealed framework fragility issues, prompting a reassessment of the technology stack.

### Requirements

1. **TUI Framework**: Support for 199×55 dashboard with 6+ panels
2. **Performance**: Handle real-time log streaming (100+ lines/sec)
3. **Distribution**: Easy binary distribution and atomic updates
4. **Development Speed**: Rapid prototyping and iteration
5. **Worker Spawning**: Launch tmux sessions, parse logs, manage files
6. **State Management**: Persist metrics, configs, history

### Evaluation Criteria

- **Binary size** - smaller is better for atomic updates
- **Startup time** - faster UX
- **Memory footprint** - resource efficiency
- **TUI capabilities** - layout flexibility, widgets, styling
- **File I/O performance** - log parsing, status file reads
- **Cross-platform** - Linux/macOS support minimum
- **Ecosystem** - libraries for YAML, JSON, SQLite, tmux integration
- **Maintenance burden** - complexity of codebase

---

## Decision

**Use Rust with Ratatui TUI framework.**

### Implementation Details

- **Language**: Rust 1.75+ (stable channel)
- **TUI Framework**: Ratatui 0.26+ (modern terminal UI library)
- **Async Runtime**: Tokio 1.35+ for async I/O and task management
- **State Storage**: SQLite (rusqlite) for metrics/history, YAML (serde_yaml) for configs
- **Log Parsing**: serde_json for JSON parsing, custom parsers for other formats
- **Worker Integration**: tokio::process for subprocess management, notify for file watching
- **Distribution**: Single static binary via cargo build --release

---

## Rationale

### Why Rust + Ratatui? (Revised Decision)

**Advantages:**
1. **Framework Robustness**: Type system prevents property conflicts and internal breakage
2. **Binary Quality**: 2-5MB single static binary, <50ms startup, 10-20MB memory
3. **Compile-Time Safety**: Catches issues at build time, not runtime
4. **Long-Term Maintainability**: No surprises from framework internals, explicit state management
5. **Performance Headroom**: Handles 1000+ workers, 10k+ log lines/sec if needed
6. **Mature Async**: Tokio ecosystem is battle-tested for concurrent I/O
7. **Clean Architecture**: Forced to think through state ownership and lifetimes

**Trade-offs Accepted:**
1. **Development Time**: 4-6 weeks to MVP vs 2-3 weeks (Python)
   - Mitigation: Investment in quality pays off long-term, fewer bugs
2. **Learning Curve**: Rust ownership model, lifetimes, async programming
   - Mitigation: Team has Rust experience, excellent learning resources
3. **Boilerplate**: More explicit code vs Python's brevity
   - Mitigation: Clarity and correctness over conciseness

### Why NOT Python + Textual? (Original Decision Reversed)

**Textual issues discovered during prototyping:**
1. **Framework Brittleness**: Easy to accidentally override internal properties (workers, tasks)
   - Property name conflicts broke WorkerManager cleanup
   - No compile-time protection against these mistakes
2. **Runtime Surprises**: Coroutine cleanup warnings, unclear lifecycle management
3. **Debugging Complexity**: Stack traces through framework internals confusing
4. **Binary Size**: 15-20MB vs 2-5MB (Rust) - 3-4x larger
5. **Startup Overhead**: 200-500ms vs <50ms - noticeable lag
6. **Memory Footprint**: 40-60MB vs 10-20MB - 3-4x larger

**When Python made sense (initial assessment):**
- Rapid prototyping to validate concept (✅ achieved)
- Unknown requirements, fast iteration needed (✅ no longer unknown)
- Short-lived project or throwaway code (❌ FORGE is long-term)

**Why Rust makes sense for FORGE:**
- FORGE is a long-term project requiring maintainability
- Developer tool where polish and reliability matter
- Binary distribution benefits from small size and fast startup
- Concurrent I/O (file watching, log parsing, UI updates) suits Tokio
- Type safety prevents entire classes of bugs (no property conflicts)

### State Storage Strategy

**SQLite for time-series metrics:**
```python
# Schema for cost tracking
CREATE TABLE cost_events (
    timestamp INTEGER,
    worker_id TEXT,
    model TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    cost_usd REAL
);

CREATE INDEX idx_timestamp ON cost_events(timestamp);
CREATE INDEX idx_worker ON cost_events(worker_id);
```

**JSON files for configuration:**
```python
# ~/.forge/config.yaml - main config
# ~/.forge/workers/*.yaml - worker templates
# ~/.forge/layouts/*.json - saved layouts
```

**In-memory for real-time state:**
```python
# Worker status, current view, active filters
# Rebuilt on startup from status files + SQLite
```

**Retention Policy:**
- Metrics: 30 days in SQLite, 24 hours in memory
- Logs: Managed by workers, FORGE only reads
- Configs: Persistent (user-managed)

---

## Consequences

### Positive

1. **Binary Quality**: 2-5MB single static binary, <50ms startup, 10-20MB memory
2. **Compile-Time Safety**: Type system catches errors before runtime
3. **Framework Robustness**: No accidental property conflicts or internal breakage
4. **Performance Headroom**: Can scale to 1000+ workers if needed
5. **Clean Architecture**: Ownership model enforces clear state management
6. **Memory Safety**: No segfaults, use-after-free, or data races
7. **Cross-Platform**: Single codebase for Linux/macOS/Windows

### Negative

1. **Development Time**: 4-6 weeks to MVP vs 2-3 weeks (Python)
   - Mitigation: Investment in quality, fewer rewrites later
2. **Learning Curve**: Rust ownership, lifetimes, async programming
   - Mitigation: Team has Rust experience, excellent docs
3. **Slower Iteration**: Compile times vs interpreted Python
   - Mitigation: Use cargo watch, incremental compilation
4. **Community**: Smaller contributor pool (Rust vs Python)
   - Mitigation: Better code quality compensates for fewer contributors

### Migration Path (from Python prototype)

Python prototype achieved validation goals. Now migrate to Rust:

1. **Reuse Designs**: UI layouts, protocols, integration surfaces proven
2. **Port Incrementally**: Start with core (config, parsing), then TUI, then integrations
3. **Parallel Development**: Keep Python version as reference during migration
4. **Testing**: Port test harnesses first to validate Rust implementation
5. **Documentation**: ADRs and protocol specs remain valid

**Migration Timeline:** 4-6 weeks for feature parity with Python prototype

---

## Implementation Plan

### Phase 1: Project Setup & Core (Week 1)
- Cargo workspace structure (forge-core, forge-tui, forge-worker, forge-config)
- Configuration management (YAML parsing, validation)
- Logging infrastructure (tracing)
- Error types (thiserror, anyhow)

### Phase 2: TUI Foundation (Week 2)
- Ratatui app skeleton with 6 panels
- Layout system (199×38, 199×55 layouts)
- Event handling (crossterm)
- View switching and navigation
- Hotkey bindings

### Phase 3: Worker Integration (Week 3)
- Worker launcher (tokio::process + tmux)
- Status file monitoring (notify)
- Log parsing (serde_json, custom parsers)
- Worker health checks

### Phase 4: Chat Backend (Week 4)
- Tool definition generation
- stdin/stdout protocol with headless CLI
- Tool execution engine
- Error handling and display

### Phase 5: State & Persistence (Week 5-6)
- SQLite metrics storage (rusqlite)
- Cost tracking and aggregation
- Bead integration (JSONL reading)
- Real-time file watching

### Dependencies

```toml
[dependencies]
# Core TUI
ratatui = "0.26"
crossterm = "0.27"

# Async Runtime
tokio = { version = "1.35", features = ["full"] }
tokio-util = "0.7"

# Parsing & Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml = "0.9"

# File I/O
notify = "6.1"  # File system events
notify-debouncer-full = "0.3"

# Database
rusqlite = { version = "0.31", features = ["bundled"] }

# Error Handling
anyhow = "1.0"
thiserror = "1.0"

# CLI
clap = { version = "4.4", features = ["derive"] }

# Utilities
chrono = "0.4"
tracing = "0.1"
tracing-subscriber = "0.3"
```

### Build Configuration

```toml
# Cargo.toml
[package]
name = "forge"
version = "0.1.0"
edition = "2021"

[profile.release]
opt-level = "z"     # Optimize for size
lto = true          # Link-time optimization
codegen-units = 1   # Better optimization
strip = true        # Strip symbols
panic = "abort"     # Smaller binaries

[[bin]]
name = "forge"
path = "src/main.rs"

[workspace]
members = [
    "crates/forge-tui",
    "crates/forge-core",
    "crates/forge-worker",
    "crates/forge-config",
]
```

---

## Alternatives Considered

### Rust + Ratatui
**Pros**: Smaller binary, faster startup, memory safety
**Cons**: 3-5x slower development, complex async, harder debugging
**Verdict**: Over-engineering for a dumb orchestrator

### Go + tview
**Pros**: Good balance of performance and productivity
**Cons**: Weaker ecosystem for TUI, less mature than Textual
**Verdict**: Possible alternative, but Python ecosystem richer

### Node.js + blessed/ink
**Pros**: Large ecosystem, good async model
**Cons**: Even larger binaries (~50MB), slower than Python
**Verdict**: No advantages over Python for this use case

### Shell Script + tmux
**Pros**: Minimal dependencies, universal
**Cons**: No real TUI framework, unmaintainable at scale
**Verdict**: Not viable for complex dashboard

---

## References

- ADR 0002: Terminal User Interface (chose TUI over web)
- ADR 0005: Dumb Orchestrator Architecture (Python suits glue code)
- Textual Documentation: https://textual.textualize.io/
- PyInstaller Atomic Updates: docs/notes/binary-atomic-updates.md
- Ratatui Comparison: docs/notes/tui-framework-comparison.md

---

## Notes

- **Target Rust Version**: 1.75+ (stable channel, MSRV policy)
- **Binary Update Strategy**: atomic rename(), no zero-downtime requirement (TUI can restart)
- **Cross-Platform**: Linux/macOS primary, Windows future consideration
- **Performance Benchmarks**: Target <16ms UI refresh (60 FPS), <1ms log parse per line
- **Memory Budget**: <50MB total footprint with 10 workers, 10k log lines cached
- **Architecture**: Workspace with multiple crates for modularity

**Lessons from Python Prototype:**
1. ✅ UI layout and panel design validated
2. ✅ Integration protocols proven (launcher, backend, status, logs)
3. ✅ Interactive setup wizard UX validated
4. ❌ Textual framework fragility discovered (property conflicts, coroutine issues)
5. ❌ Binary size/startup/memory larger than desired

**Decision Confidence**: High - Rust+Ratatui is right choice for long-term maintainability and quality

---

**FORGE** - Federated Orchestration & Resource Generation Engine
