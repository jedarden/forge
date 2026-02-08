# ADR 0006: Technology Stack Selection

**Status**: Accepted
**Date**: 2026-02-07
**Deciders**: FORGE Architecture Team

---

## Context

FORGE requires a TUI framework and implementation language to build the orchestrator. ADR 0002 deferred this decision with "start with Textual, migrate if needed." We now need to make a concrete choice before implementation begins.

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

**Use Python 3.11+ with Textual TUI framework.**

### Implementation Details

- **Language**: Python 3.11+ (for better performance, match/case syntax)
- **TUI Framework**: Textual 0.50+ (Rich-based terminal UI)
- **State Storage**: SQLite for metrics/history, JSON files for configs
- **Log Parsing**: Python stdlib + orjson for JSON performance
- **Worker Integration**: subprocess module for tmux, file watching via watchdog
- **Distribution**: PyInstaller for single-file binaries with atomic update support

---

## Rationale

### Why Python + Textual?

**Advantages:**
1. **Development Speed**: 3-5x faster prototyping than Rust
2. **Textual Maturity**: Production-ready with excellent widget library
3. **Rich Ecosystem**:
   - PyYAML, orjson for config/log parsing
   - watchdog for file system events
   - libtmux for tmux integration
   - sqlite3 built-in
4. **Layout Flexibility**: Textual's CSS-like layout matches dashboard requirements
5. **Rapid Iteration**: Hot reload during development, easier debugging
6. **Dumb Orchestrator Pattern**: Python's expressiveness suits glue code

**Trade-offs Accepted:**
1. **Binary Size**: ~15-20MB (PyInstaller) vs ~2-5MB (Rust)
   - Mitigation: Still small enough for atomic rename() updates
2. **Startup Time**: ~200-500ms vs ~50ms (Rust)
   - Mitigation: Acceptable for TUI application (not CLI tool)
3. **Memory**: ~40-60MB vs ~10-20MB (Rust)
   - Mitigation: Negligible on modern systems, FORGE is always-on dashboard

### Why NOT Rust + Ratatui?

**Ratatui disadvantages:**
1. **Complexity**: Terminal state management, event loops, async I/O more complex
2. **Development Time**: 3-5x longer to reach MVP
3. **Layout System**: Manual grid calculations vs Textual's declarative CSS
4. **Ecosystem Gaps**: YAML/JSON libraries good, but tmux integration less mature
5. **Over-Engineering**: FORGE is a dumb orchestrator, not a performance-critical system

**When Rust makes sense:**
- High-frequency trading systems (microsecond latency)
- Embedded systems (memory constrained)
- System daemons (minimal footprint)
- Security-critical code (memory safety)

FORGE doesn't match these profiles. It's a TUI dashboard that:
- Parses logs at 10-100 Hz (not 10kHz)
- Manages 2-10 workers (not 1000s)
- Runs on developer workstations (not production servers)

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

1. **Faster MVP**: Can prototype full dashboard in 2-3 weeks vs 6-8 weeks
2. **Easier Contributions**: Python lowers barrier for community contributions
3. **Rich Widgets**: Textual's DataTable, Tree, Log widgets perfect for FORGE
4. **Debugging**: Python stacktraces, pdb, logging easier than Rust debugging
5. **Integration Testing**: Easier to mock tmux, file systems, subprocess calls
6. **Flexibility**: Dynamic typing suits evolving integration protocols

### Negative

1. **Binary Size**: 15-20MB vs 2-5MB (Rust)
   - Acceptable: Still under 50MB limit for fast atomic updates
2. **Performance Ceiling**: Can't handle 1000+ workers or 10k+ log lines/sec
   - Acceptable: Target is 2-10 workers, 100 lines/sec max
3. **Packaging Complexity**: PyInstaller has edge cases (hidden imports, data files)
   - Mitigation: Test on Linux/macOS early, document known issues
4. **Runtime Dependency**: Python 3.11+ required (or bundle interpreter)
   - Mitigation: PyInstaller bundles Python runtime, zero user dependencies

### Migration Path (if needed)

If Python becomes bottleneck later:

1. **Profile First**: Use py-spy to identify actual bottlenecks
2. **Optimize Python**: Use orjson, cython, pypy before rewriting
3. **Hybrid Approach**: Rewrite hot paths in Rust (PyO3 bindings)
4. **Full Rewrite**: Only if profiling shows Python is fundamental limit

**Threshold for migration:** >10s lag in UI, >500MB memory, or >1 CPU core usage

---

## Implementation Plan

### Phase 1: Core TUI (Week 1-2)
- Textual app skeleton with 6 panels
- Dashboard layouts (199×38, 199×55)
- View switching (workers, tasks, costs, metrics, logs)
- Hotkey bindings

### Phase 2: Integration (Week 3-4)
- Worker launcher integration (subprocess + tmux)
- Log parsing (JSON lines, key-value)
- Status file monitoring (watchdog)
- Worker health checks

### Phase 3: Chat Backend (Week 5-6)
- Tool definition generation
- stdin/stdout protocol with headless CLI
- Tool execution dispatch
- Error handling

### Phase 4: State & Persistence (Week 7-8)
- SQLite metrics storage
- Cost tracking and aggregation
- Config management
- Layout persistence

### Dependencies

```txt
# Core TUI
textual>=0.50.0
rich>=13.7.0

# Performance
orjson>=3.9.0          # Fast JSON parsing
uvloop>=0.19.0         # Fast async I/O (if needed)

# File I/O
watchdog>=4.0.0        # File system events
pyyaml>=6.0.1          # Config parsing

# Worker Integration
libtmux>=0.25.0        # Tmux control

# State Management
# sqlite3 is built-in

# Packaging
pyinstaller>=6.3.0     # Binary distribution
```

### Build Configuration

```python
# pyinstaller.spec
a = Analysis(
    ['forge/main.py'],
    pathex=[],
    binaries=[],
    datas=[
        ('forge/tools.json', 'forge'),
        ('forge/widgets', 'forge/widgets'),
    ],
    hiddenimports=['textual', 'rich', 'orjson'],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=['tkinter', 'matplotlib'],  # Reduce size
    noarchive=False,
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name='forge',
    debug=False,
    bootloader_ignore_signals=False,
    strip=True,  # Reduce size
    upx=True,    # Compress
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)
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

- **Target Python Version**: 3.11+ for match/case, better performance
- **Binary Update Strategy**: atomic rename(), no zero-downtime requirement (TUI can restart)
- **Cross-Platform**: Linux/macOS primary, Windows future consideration
- **Performance Benchmarks**: Target <100ms UI refresh, <10ms log parse per line
- **Memory Budget**: <100MB total footprint with 10 workers, 10k log lines cached

**Decision Confidence**: High - Python+Textual is proven choice for TUI dashboards

---

**FORGE** - Federated Orchestration & Resource Generation Engine
