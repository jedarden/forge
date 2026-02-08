# FORGE Design Gaps Analysis

**Last Updated**: 2026-02-07

This document identifies areas where the FORGE design is incomplete or under-specified.

---

## ✅ What's Complete

### Architecture & Philosophy
- [x] Dumb orchestrator architecture (ADR 0005)
- [x] Integration surfaces defined (5 surfaces)
- [x] Testing framework (all surfaces)
- [x] File system conventions
- [x] Protocol specifications

### User Interface
- [x] TUI decision (ADR 0002)
- [x] Tool-based conversational interface (ADR 0004)
- [x] Hotkeys as optional shortcuts
- [x] Dashboard layouts (199×38, 199×55, responsive)
- [x] Conversational interface design

### Cost & Optimization
- [x] Subscription-first routing (ADR 0003)
- [x] 4-tier model pool
- [x] Task value scoring concept (0-100 scale)
- [x] Cost tracking requirements

### Integration
- [x] Launcher protocol
- [x] Backend protocol
- [x] Worker config format
- [x] Log format specification
- [x] Status file format

---

## ❌ Critical Gaps

### 1. Implementation Technology Stack

**Status**: ✅ **RESOLVED** (ADR 0006)

**Decisions made**:
- **TUI Framework**: Textual 0.50+ (Python)
- **Programming Language**: Python 3.11+
- **State Storage**: SQLite for metrics/history, JSON for configs, in-memory for real-time

**Rationale**:
- Python + Textual: 3-5x faster development than Rust, proven dashboard capabilities
- Binary size trade-off accepted: 15-20MB (PyInstaller) vs 2-5MB (Rust) - still atomic-update friendly
- Rich ecosystem: PyYAML, orjson, watchdog, libtmux built-in support
- Dumb orchestrator pattern suits Python's expressiveness (glue code, not performance-critical)

**Impact**: ✅ **UNBLOCKED** - Implementation can begin

**References**: docs/adr/0006-technology-stack-selection.md

---

### 2. Bead Integration

**Status**: ✅ **RESOLVED** (ADR 0007)

**Decisions made**:

#### 2a. Bead Backend Integration
- **FORGE reads `.beads/*.jsonl` files directly** (read-only)
- `br` CLI is write authority - all mutations via `br update`
- No direct SQLite access (avoids locking conflicts)

#### 2b. Task Assignment Algorithm
- **FORGE suggests, workers decide**
- Greedy assignment: match worker tier to bead priority (P0→Premium, P1→Standard, etc.)
- Workers autonomous: can ignore suggestions, use `br ready` to pull tasks
- No automatic assignment - user confirms via chat or manual `br update`

#### 2c. Task Value Scoring Implementation
- **FORGE calculates scores** using algorithm:
  - Priority: 0-40 points (P0=40, P1=30, P2=20, P3=10, P4=5)
  - Blockers: 0-30 points (10 points per blocked task, max 30)
  - Age: 0-20 points (older tasks prioritized, max at 7 days)
  - Labels: 0-10 points (critical/urgent/blocker tags)
- Configurable weights in `~/.forge/config.yaml`

#### 2d. Dependency Resolution
- **`br` CLI enforces dependencies**
- FORGE visualizes dependency graph, highlights blocked tasks
- `br ready` shows only unblocked tasks
- FORGE never overrides dependency rules

**Impact**: ✅ **UNBLOCKED** - Task queue display and routing ready

**References**: docs/adr/0007-bead-integration-strategy.md

---

### 3. Real-Time Updates

**Status**: ✅ **RESOLVED** (ADR 0008)

**Decisions made**:

#### 3a. Worker Status Updates
- **Hybrid: inotify + fallback polling**
- Primary: watchdog library monitors `~/.forge/status/*.json` (20-50ms latency)
- Fallback: Poll every 5 seconds if inotify unavailable
- Triggers: on_modified, on_created, on_deleted events

#### 3b. Log Streaming
- **Async tail with ring buffer**
- 100ms polling interval per log file (aiofiles async I/O)
- Ring buffer: deque(maxlen=1000) prevents unbounded growth
- Batch updates: flush 10 entries at once to UI
- Handles log rotation gracefully

#### 3c. Cost Tracking Updates
- **Event-driven from logs + periodic aggregation**
- Parse log entries for `api_call_completed` events (real-time)
- Batch insert to SQLite every 10 seconds
- UI refresh every 10 seconds after flush
- Background hourly rollups

#### 3d. TUI Refresh Strategy
- **Reactive data binding (Textual framework)**
- Reactive vars auto-trigger widget re-render on change
- 60 FPS render cap (Textual batches updates)
- Event-driven: no unnecessary redraws
- Configurable: max FPS, auto-scroll, refresh intervals

**Performance**: <10% CPU, <100MB memory, <200ms latency for most updates

**Impact**: ✅ **UNBLOCKED** - Real-time dashboard ready

**References**: docs/adr/0008-real-time-update-architecture.md

---

### 4. Worker Health Monitoring

**Status**: **SIMPLIFIED BY ADR 0014**

**Decisions made** (via ADR 0014 Error Handling):

#### 4a. Health Check Implementation
- **Process exists** (PID check via `ps` or `/proc/<pid>`)
- **Recent log activity** (<5 min last_activity in status file)
- **Status file present** and valid
- **Tmux session alive** (check via `tmux list-sessions`)

#### 4b. Failure Detection
- **Process exit** - PID no longer exists
- **No log activity** - last_activity timestamp >5 min old
- **Status file corrupted** - JSON parse error
- **Tmux session died** - session not in `tmux ls`

#### 4c. Auto-Recovery
- ❌ **No automatic restart** (per ADR 0014)
- ✅ **Mark worker as "failed" status** in UI
- ✅ **Show error with guidance** ("Worker crashed: restart with...")
- ✅ **User decides** when to restart or reassign tasks

#### 4d. Health Check Intervals
- **Every 10 seconds** - background health check task
- **On-demand** - when user views worker panel
- **Event-driven** - when status file changes (inotify)

**Rationale**: Health monitoring is about **detection and visibility**, not automatic recovery. Show users what failed, let them decide how to fix it.

**Impact**: ✅ **RESOLVED** - No ADR 0009 needed, covered by ADR 0014

**Status indicators**:
```
✅ active   - Process alive, recent logs
💤 idle     - Process alive, no recent activity
❌ failed   - Process dead or unresponsive
⚠️ error    - Status file corrupted
```

---

### 5. Security & Credentials

**Status**: ✅ **RESOLVED** (ADR 0010)

**Decisions made**:

#### 5a. API Key Management
- **Environment variables only** - no storage, encryption, or keychain
- Trust sandbox security boundary (container/SSH/devpod)
- Mask credentials in UI (show sk-a...xyz9)

#### 5b. Credential Injection
- **Workers inherit FORGE's environment** via subprocess
- No filtering, trust sandbox to provide correct environment
- No shared secrets file on disk

#### 5c. Multi-User Support
- **Per-user FORGE instances** in separate sandboxes
- Unix file permissions prevent cross-user access
- No user management within FORGE

#### 5d. Audit Logging
- **Minimal structured logging to stderr**
- Sandbox captures logs (delegate retention to sandbox)
- Log worker spawns, tool calls (no PII filtering needed)

**Rationale**: FORGE runs in remote terminal environments (tmux/containers/devpods) where security is handled by the sandbox. No need for complex credential management.

**Impact**: ✅ **UNBLOCKED** - Security via delegation to sandbox

**References**: docs/adr/0010-security-and-credential-management.md

---

### 6. Multi-Workspace Support

**Status**: ✅ **OUT OF SCOPE**

**Decision**: One workspace per FORGE instance
- Each server has one workspace (can contain multiple repos)
- No workspace switching needed
- No cross-workspace aggregation needed
- Multiple workspaces = multiple FORGE instances (different directories)

**Rationale**: Simpler architecture, matches single-user model

**Impact**: ✅ **RESOLVED** - No multi-workspace feature needed for MVP

---

### 7. Metrics Aggregation & Storage

**Status**: ✅ **DECIDED**

**Decisions**:
- **Storage**: SQLite database (per ADR 0006)
- **Format**: Flat files where possible, SQLite for aggregations
- **Aggregation**: Batch processing every 10s (per ADR 0008)
- **Retention**: 30 days in SQLite, configurable

**Implementation**: Covered by bead fg-3of (Implement SQLite metrics storage)

**Impact**: ✅ **RESOLVED** - SQLite is sufficient for metrics

---

### 8. Binary Updates & Versioning

**Status**: ✅ **REFERENCE IMPLEMENTATION EXISTS**

**Decision**: Use ccdash (claude-code dashboard) self-update as reference
- ccdash has working, stable self-update functionality
- Follow same pattern for FORGE

**Next Steps**:
- [ ] Study ccdash update mechanism
- [ ] Adapt for FORGE (single binary, atomic rename)
- [ ] Document in ADR 0013 (optional, can copy ccdash approach)

**Implementation**: Covered by bead fg-1yr (Implement PyInstaller packaging)

**Impact**: ✅ **RESOLVED** - Reference implementation available

---

### 9. Error Handling & Graceful Degradation

**Status**: ✅ **RESOLVED** (ADR 0014)

**Decisions made**:

#### Philosophy: No Fallback, Graceful Degradation Only
- **Visibility first** - show all errors clearly in TUI
- **No automatic retry** - user decides if/when to retry
- **No silent failures** - every error surfaced to user
- **Degrade gracefully** - broken component doesn't crash app

#### 9a. Backend Failure
- **Degrade to hotkey-only mode**
- Show error in chat panel with guidance
- Don't restart automatically - user triggers restart
- App keeps running without chat

#### 9b. Launcher Failure
- **Show detailed error dialog**
- Include stdout/stderr, actionable guidance
- Don't retry automatically - user fixes and retries manually
- No spawn queue

#### 9c. Log Parse Errors
- **Skip malformed entries** - continue parsing
- Show warning indicator: "⚠️ N entries skipped"
- Display most recent parse error
- Graceful degradation: most logs still visible

#### 9d. File System Errors
- **Fatal on startup** - fail fast with clear message
- **Degrade at runtime** - mark worker as "error" status
- Show error in worker panel with fix suggestions
- User action required (delete corrupted file, restart worker)

**Error Display Patterns**:
- Transient errors → notifications
- Component errors → degrade component, show in panel
- Fatal errors → block startup, exit app
- User action required → dialog with actionable buttons

**Rationale**: FORGE is a developer tool where transparency is more important than automated recovery. Developers need to see what's broken and fix it, not have errors hidden by automatic retries.

**Impact**: ✅ **UNBLOCKED** - Clear error handling strategy

**References**: docs/adr/0014-error-handling-strategy.md

---

### 10. Remote Worker Support

**Status**: ✅ **OUT OF SCOPE FOR MVP**

**Decision**: Workers run on same server as FORGE
- All workers local (same machine)
- Keep code flexible for future remote workers
- No remote worker design needed for MVP

**Future consideration**: Add remote workers post-MVP if needed

**Impact**: ✅ **RESOLVED** - Local workers only, flexible architecture

---

## ⚠️ Minor Gaps

### 11. Configuration Management

**Status**: ✅ **DECIDED**

**Decisions**:
- **Format**: Try YAML with schema linter
  - If too cumbersome, fall back to JSON
  - Schema validation is critical (validate before applying)
- **Structure**: `~/.forge/config.yaml`
  - No per-workspace overrides (single workspace per instance)
  - No global + workspace merging needed
- **Migration**: Handle in update process (version-specific migrations)

**Implementation**: Covered by bead fg-1g1 (Implement configuration management)

**Impact**: ✅ **RESOLVED** - YAML with linter, fallback to JSON

---

### 12. Observability

**Status**: ✅ **DECIDED**

**Decisions**:
- **FORGE's own logs**: Yes, separate from worker logs
  - Enable worker to iterate on test FORGE instance in different terminal
  - Log to `~/.forge/logs/forge.log`
  - Structured logging (JSON lines format)
- **Debug mode**: `forge --debug` flag for verbose logging
- **Performance profiling**: Optional (add if needed during development)
- **Telemetry**: No (privacy-first, no external data collection)

**Next Steps**:
- [ ] Add bead for FORGE logging implementation
- [ ] Support `--debug` flag

**Impact**: ✅ **RESOLVED** - FORGE logs to support development workflows

---

### 13. Extensibility

**Status**: ✅ **OUT OF SCOPE FOR MVP**

**Decision**: Covered by integration surface documentation
- Plugin system: Not needed for MVP
- Custom tools: Not needed (30+ built-in tools sufficient)
- Custom log parsers: Handled by launcher protocol compliance
- Custom cost calculators: Not needed (standard model pricing)

**Future consideration**: Add plugin system post-MVP if user demand exists

**Impact**: ✅ **RESOLVED** - Integration surfaces provide extensibility

---

### 14. Documentation

**Status**: ✅ **DECIDED**

**Decisions**:
- **User guide**: Not needed - chat interface should answer questions (self-documenting)
- **Developer guide**: Yes - for contributors (bead fg-wrh)
- **Troubleshooting**: Built into chat interface (LLM answers common questions)
- **Video tutorials**: Not needed for MVP
- **Architecture diagrams**: Nice to have, not critical

**Philosophy**: Chat interface makes FORGE self-documenting. Users ask questions, get answers.

**Impact**: ✅ **RESOLVED** - Minimal documentation, chat-driven help

---

### 15. Testing Infrastructure

**Status**: ✅ **DECIDED**

**Decisions**:
- **Extend existing test harnesses** to confirm functionality
- **Unit tests**: Add for core logic (tool execution, parsing, etc.)
- **Integration tests**: Yes - end-to-end flows (bead fg-2fs)
- **Performance benchmarks**: Add if needed during development
- **Load testing**: Optional (1000s of logs/sec scenarios)

**Implementation**: Covered by bead fg-2fs (Add integration tests for all surfaces)

**Impact**: ✅ **RESOLVED** - Extend test harnesses, add unit/integration tests

---

## Priority Ranking

### P0 - Must Have for MVP
1. **Technology Stack** (ADR 0006) - Cannot build without this
2. **Bead Integration** (ADR 0007) - Core functionality
3. **Real-Time Updates** (ADR 0008) - Basic UX requirement

### P1 - Needed Soon
4. ~~**Worker Health Monitoring** (ADR 0009)~~ - ✅ Covered by ADR 0014
5. **Security & Credentials** (ADR 0010) - ✅ **COMPLETED**
6. **Error Handling** (ADR 0014) - ✅ **COMPLETED**

### P2 - Nice to Have
7. ~~**Multi-Workspace** (ADR 0011)~~ - ✅ Out of scope (single workspace per instance)
8. **Metrics Storage** (ADR 0012) - ✅ Decided (SQLite, flat files)
9. **Binary Updates** (ADR 0013) - ✅ Reference exists (ccdash)

### P3 - Future
10. ~~**Remote Workers** (ADR 0015)~~ - ✅ Out of scope (local workers only, flexible architecture)
11. **Configuration Management** - ✅ Decided (YAML with linter, fallback JSON)
12. **Observability** - ✅ Decided (FORGE logs, --debug flag)
13. ~~**Extensibility**~~ - ✅ Out of scope (integration surfaces sufficient)
14. **Documentation** - ✅ Decided (chat-driven, minimal docs)
15. **Testing Infrastructure** - ✅ Decided (extend harnesses, add unit/integration)

---

## Recommended Next Steps

1. **Immediate** (This Week):
   - [x] Create ADR 0006: Technology Stack (Python/Textual vs Rust/Ratatui) - **COMPLETED**
   - [x] Create ADR 0007: Bead Integration Strategy - **COMPLETED**
   - [x] Create ADR 0008: Real-Time Update Architecture - **COMPLETED**

2. **Short Term** (Next 2 Weeks):
   - [x] ~~Create ADR 0009: Worker Health Monitoring~~ - **Covered by ADR 0014**
   - [x] Create ADR 0010: Security & Credential Management - **COMPLETED**
   - [x] Create ADR 0014: Error Handling Strategy - **COMPLETED**

3. **Medium Term** (Next Month):
   - [ ] Implement MVP with P0 ADRs
   - [ ] Test with real workloads
   - [ ] Address P1 gaps based on feedback

4. **Long Term** (Next Quarter):
   - [ ] Add P2 features (multi-workspace, metrics, updates)
   - [ ] Consider P3 features (remote workers, plugins)
   - [ ] Build community ecosystem

---

## ✅ Resolved Architectural Questions

1. **Single binary or multiple binaries?**
   - ✅ **One `forge` binary** - Everything integrates against single binary (like ccdash)

2. **Client-server or standalone?**
   - ✅ **Standalone application** - TUI is the entire application (no daemon, like ccdash)

3. **Online or offline?**
   - ✅ **Model agnostic** - Works with local or remote models, FORGE is indifferent

4. **Single-user or multi-user?**
   - ✅ **Single-user** - Each user has their own FORGE instance (per-user isolation)

5. **Open-source or commercial?**
   - ✅ **Open-source** - MIT or Apache-2.0 license

---

**FORGE** - Federated Orchestration & Resource Generation Engine

Design is **100% complete**. All gaps resolved. All architectural questions answered. Ready for implementation.
