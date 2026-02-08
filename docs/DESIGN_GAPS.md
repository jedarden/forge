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

**Status**: **UNDECIDED**

**Missing decisions**:
- **TUI Framework**: Textual (Python) vs Ratatui (Rust)?
  - ADR 0002 defers this decision ("Start with Textual, migrate if needed")
  - Need to make actual choice before implementation

- **Programming Language**: Python vs Rust?
  - Python: Faster prototyping, Textual ecosystem
  - Rust: Better performance, atomic binary updates, smaller binaries

- **State Storage**: How to persist state?
  - SQLite for metrics/history?
  - JSON files?
  - In-memory only?

**Impact**: Cannot start implementation without this decision

**Next Steps**:
- [ ] Create ADR 0006: Technology Stack Selection
- [ ] Benchmark Textual vs Ratatui for 199×55 dashboard
- [ ] Decide Python vs Rust based on binary update requirements
- [ ] Design state storage schema

---

### 2. Bead Integration

**Status**: **PARTIALLY SPECIFIED**

**What's missing**:

#### 2a. Bead Backend Integration
- How does FORGE interact with `br` CLI?
  - Direct SQLite access to `.beads/*.db`?
  - Shell out to `br` commands?
  - Parse `.beads/*.jsonl` files?

#### 2b. Task Assignment Algorithm
- How are tasks assigned to workers?
  - Manual assignment only?
  - Automatic based on task value score?
  - Queue-based with workers pulling?
  - FORGE pushing to workers?

#### 2c. Task Value Scoring Implementation
- Who calculates the 0-100 score?
  - FORGE parses bead description?
  - Pre-scored in bead metadata?
  - LLM backend scores tasks?

#### 2d. Dependency Resolution
- How are bead dependencies (`depends_on`, `blocks`) handled?
  - FORGE enforces them?
  - Workers check themselves?
  - `br` handles it independently?

**Impact**: Cannot display task queue or assign work without this

**Next Steps**:
- [ ] Create ADR 0007: Bead Integration Strategy
- [ ] Design task assignment algorithm
- [ ] Specify task value scoring implementation
- [ ] Document dependency handling

---

### 3. Real-Time Updates

**Status**: **CONCEPTUAL ONLY**

**What's missing**:

#### 3a. Worker Status Updates
- How does FORGE know worker status changed?
  - Poll status files every N seconds?
  - File system watching (inotify/fswatch)?
  - Workers send updates via IPC?

#### 3b. Log Streaming
- How are logs displayed in real-time?
  - Tail log files continuously?
  - Workers stream to FORGE via socket?
  - Polling-based (refresh every N seconds)?

#### 3c. Cost Tracking Updates
- How are costs updated in real-time?
  - Parse logs for token usage events?
  - Workers report costs via API?
  - Batch calculation every minute?

#### 3d. TUI Refresh Strategy
- How often does the TUI redraw?
  - Event-driven (update on change)?
  - Fixed interval (e.g., 1 second)?
  - Different refresh rates per panel?

**Impact**: Dashboard will feel sluggish or outdated without this

**Next Steps**:
- [ ] Create ADR 0008: Real-Time Update Architecture
- [ ] Choose polling interval vs event-driven
- [ ] Design log streaming mechanism
- [ ] Implement efficient file watching

---

### 4. Worker Health Monitoring

**Status**: **REQUIREMENTS ONLY**

**What's missing**:

#### 4a. Health Check Implementation
- What constitutes "healthy"?
  - Process exists (PID check)?
  - Recent log activity (<5 min)?
  - Responds to ping/heartbeat?
  - Making progress on task?

#### 4b. Failure Detection
- How to detect worker failures?
  - Process exit (PID gone)?
  - No log activity for N minutes?
  - Error events in logs?
  - Tmux session died?

#### 4c. Auto-Recovery
- What to do when worker fails?
  - Restart automatically?
  - Alert user?
  - Reassign tasks?
  - Exponential backoff on repeated failures?

#### 4d. Health Check Intervals
- How often to check health?
  - Every 10 seconds? 60 seconds?
  - Different intervals for active vs idle?
  - Adaptive based on failure rate?

**Impact**: Workers can fail silently without detection

**Next Steps**:
- [ ] Create ADR 0009: Worker Health Monitoring
- [ ] Define health criteria
- [ ] Implement failure detection
- [ ] Design auto-recovery strategy

---

### 5. Security & Credentials

**Status**: **UNADDRESSED**

**What's missing**:

#### 5a. API Key Management
- How are API keys stored?
  - Environment variables only?
  - Encrypted config file?
  - System keychain (macOS Keychain, Linux Secret Service)?
  - Prompt on first use?

#### 5b. Credential Injection
- How do workers get API keys?
  - FORGE passes via environment?
  - Workers read from own config?
  - Shared secrets file?

#### 5c. Multi-User Support
- How do multiple users share FORGE?
  - Separate `~/.forge/` per user?
  - Shared workers, separate configs?
  - User-scoped API keys?

#### 5d. Audit Logging
- Who did what, when?
  - Log all tool calls?
  - Log worker spawns?
  - Log cost changes?

**Impact**: Security vulnerabilities, credential leakage

**Next Steps**:
- [ ] Create ADR 0010: Security & Credential Management
- [ ] Design keychain integration
- [ ] Implement audit logging
- [ ] Document multi-user patterns

---

### 6. Multi-Workspace Support

**Status**: **MENTIONED BUT NOT DESIGNED**

**What's missing**:

#### 6a. Workspace Discovery
- How does FORGE find workspaces?
  - User manually registers?
  - Auto-discover git repos in ~/projects?
  - Workspace list in config?

#### 6b. Workspace Switching
- How to switch between workspaces in TUI?
  - Dedicated workspace picker view?
  - Command: `:switch workspace <path>`?
  - Hotkey cycle?

#### 6c. Per-Workspace Workers
- Are workers bound to workspaces?
  - One worker can work on multiple workspaces?
  - Workers are workspace-scoped?
  - Workers can be reassigned?

#### 6d. Cross-Workspace Metrics
- How to view costs across all workspaces?
  - Aggregate view?
  - Filter by workspace?
  - Per-workspace breakdown?

**Impact**: Limited to single-workspace usage

**Next Steps**:
- [ ] Create ADR 0011: Multi-Workspace Architecture
- [ ] Design workspace discovery
- [ ] Implement workspace switching UI
- [ ] Define worker-workspace relationships

---

### 7. Metrics Aggregation & Storage

**Status**: **LOG FORMAT DEFINED, AGGREGATION NOT**

**What's missing**:

#### 7a. Metrics Database
- Where are aggregated metrics stored?
  - SQLite database?
  - In-memory only (lost on restart)?
  - Time-series database?

#### 7b. Aggregation Logic
- How are logs parsed into metrics?
  - Real-time as logs arrive?
  - Batch processing every minute?
  - On-demand when view is opened?

#### 7c. Data Retention
- How long to keep metrics?
  - Last 24 hours in memory?
  - 30 days in database?
  - Configurable retention?

#### 7d. Performance Calculations
- How are throughput, latency, success rate calculated?
  - Sliding window (last 1 hour)?
  - Per-worker aggregations?
  - Percentiles (p50, p95, p99)?

**Impact**: Cannot show historical trends or analytics

**Next Steps**:
- [ ] Create ADR 0012: Metrics Storage & Aggregation
- [ ] Design database schema
- [ ] Implement log parser
- [ ] Define retention policy

---

### 8. Binary Updates & Versioning

**Status**: **RESEARCH DONE, IMPLEMENTATION NOT**

**What's documented**:
- Binary atomic updates research (docs/notes/binary-atomic-updates.md)
- Hot-reload strategy (docs/notes/hot-reload-and-updates.md)

**What's missing**:

#### 8a. Version Checking
- How does FORGE check for updates?
  - Poll GitHub releases API?
  - Check manifest on server?
  - User-initiated only?

#### 8b. Update Manifest Format
- What's in the manifest?
  - Version number, changelog?
  - Binary URLs per platform?
  - SHA256 checksums?
  - Migration notes?

#### 8c. State Migration
- How to migrate state between versions?
  - Backward-compatible formats?
  - Migration scripts?
  - User confirmation required?

#### 8d. Rollback
- How to rollback failed update?
  - Keep previous binary?
  - Automatic rollback on crash?
  - Manual rollback command?

**Impact**: Cannot self-update without this

**Next Steps**:
- [ ] Create ADR 0013: Binary Update Mechanism
- [ ] Implement version checking
- [ ] Design update manifest
- [ ] Build rollback system

---

### 9. Error Handling & Graceful Degradation

**Status**: **SCATTERED, NOT SYSTEMATIC**

**What's missing**:

#### 9a. Backend Failure
- What happens when chat backend crashes?
  - Fall back to hotkeys only?
  - Show error message?
  - Restart backend automatically?
  - Disable chat until fixed?

#### 9b. Launcher Failure
- What happens when launcher fails to spawn worker?
  - Retry N times?
  - Alert user?
  - Queue spawn for later?
  - Suggest troubleshooting?

#### 9c. Log Parse Errors
- What happens when log format is invalid?
  - Skip malformed entries?
  - Show warning?
  - Fall back to raw text display?
  - Alert about format issue?

#### 9d. File System Errors
- What happens when status file is corrupted?
  - Recreate from process inspection?
  - Mark worker as unhealthy?
  - Show error in UI?

**Impact**: Poor user experience on errors

**Next Steps**:
- [ ] Create ADR 0014: Error Handling Strategy
- [ ] Design fallback behaviors
- [ ] Implement retry logic
- [ ] Add user-facing error messages

---

### 10. Remote Worker Support

**Status**: **NOT DESIGNED**

**What's missing**:

#### 10a. SSH Workers
- Can workers run on remote machines?
  - SSH to remote and spawn?
  - Forward logs via SSH?
  - How to monitor remote processes?

#### 10b. Container Workers
- Can workers run in Docker/K8s?
  - FORGE orchestrates containers?
  - Container logs streamed to FORGE?
  - Health checks via container API?

#### 10c. Cloud Workers
- Can workers run in cloud (AWS Lambda, Modal, etc.)?
  - FORGE triggers cloud functions?
  - Different cost tracking?
  - Different spawning mechanism?

#### 10d. Network Topology
- How do remote workers communicate?
  - Workers push logs to FORGE?
  - FORGE pulls logs via API?
  - Centralized log aggregation?

**Impact**: Limited to local-only workers

**Next Steps**:
- [ ] Create ADR 0015: Remote Worker Architecture
- [ ] Design SSH launcher
- [ ] Design container launcher
- [ ] Implement network log streaming

---

## ⚠️ Minor Gaps

### 11. Configuration Management

**What's missing**:
- [ ] Config file schema validation
- [ ] Config migration between versions
- [ ] Per-workspace config overrides
- [ ] Config file merging (global + workspace)

**Impact**: Medium - can work around manually

---

### 12. Observability

**What's missing**:
- [ ] FORGE's own logs (not just worker logs)
- [ ] Performance profiling hooks
- [ ] Debug mode
- [ ] Telemetry (opt-in, privacy-preserving)

**Impact**: Medium - harder to debug FORGE itself

---

### 13. Extensibility

**What's missing**:
- [ ] Plugin system design
- [ ] Custom tool definitions (user-defined tools)
- [ ] Custom log parsers
- [ ] Custom cost calculators

**Impact**: Low - can be added later

---

### 14. Documentation

**What's missing**:
- [ ] User guide (how to use FORGE)
- [ ] Developer guide (how to contribute)
- [ ] Troubleshooting guide (common issues)
- [ ] Video tutorials
- [ ] Architecture diagrams (visual)

**Impact**: Low - can write as we build

---

### 15. Testing Infrastructure

**What's missing**:
- [ ] Unit tests for core logic
- [ ] Integration tests (end-to-end)
- [ ] Performance benchmarks
- [ ] Load testing (1000s of logs)

**Impact**: Low - test harnesses cover protocols

---

## Priority Ranking

### P0 - Must Have for MVP
1. **Technology Stack** (ADR 0006) - Cannot build without this
2. **Bead Integration** (ADR 0007) - Core functionality
3. **Real-Time Updates** (ADR 0008) - Basic UX requirement

### P1 - Needed Soon
4. **Worker Health Monitoring** (ADR 0009) - Reliability
5. **Security & Credentials** (ADR 0010) - Production readiness
6. **Error Handling** (ADR 0014) - User experience

### P2 - Nice to Have
7. **Multi-Workspace** (ADR 0011) - Power user feature
8. **Metrics Storage** (ADR 0012) - Analytics
9. **Binary Updates** (ADR 0013) - Distribution

### P3 - Future
10. **Remote Workers** (ADR 0015) - Advanced use cases
11. Configuration Management
12. Observability
13. Extensibility
14. Documentation
15. Testing Infrastructure

---

## Recommended Next Steps

1. **Immediate** (This Week):
   - [ ] Create ADR 0006: Technology Stack (Python/Textual vs Rust/Ratatui)
   - [ ] Create ADR 0007: Bead Integration Strategy
   - [ ] Create ADR 0008: Real-Time Update Architecture

2. **Short Term** (Next 2 Weeks):
   - [ ] Create ADR 0009: Worker Health Monitoring
   - [ ] Create ADR 0010: Security & Credential Management
   - [ ] Create ADR 0014: Error Handling Strategy

3. **Medium Term** (Next Month):
   - [ ] Implement MVP with P0 ADRs
   - [ ] Test with real workloads
   - [ ] Address P1 gaps based on feedback

4. **Long Term** (Next Quarter):
   - [ ] Add P2 features (multi-workspace, metrics, updates)
   - [ ] Consider P3 features (remote workers, plugins)
   - [ ] Build community ecosystem

---

## Open Questions

1. **Single binary or multiple binaries?**
   - One `forge` binary that does everything?
   - Separate `forge-dashboard`, `forge-launcher`, `forge-worker`?

2. **Client-server or standalone?**
   - TUI talks to background daemon?
   - TUI is the entire application?

3. **Online or offline?**
   - Requires internet for backend?
   - Can work fully offline with local model?

4. **Single-user or multi-user?**
   - Designed for solo developer?
   - Team collaboration features?

5. **Open-source or commercial?**
   - Fully open MIT/Apache-2.0?
   - Open core with paid features?
   - SaaS offering?

---

**FORGE** - Federated Orchestration & Resource Generation Engine

Design is 60% complete. Critical gaps are in implementation details, not architecture.
