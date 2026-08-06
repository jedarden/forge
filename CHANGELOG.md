# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-05-04

### Production Release
This release marks FORGE as production-ready with all core features fully implemented and tested.

### Completed
- **All Phase 1-3 Features**: Complete implementation of MVP, Intelligence, and Advanced Features phases
- **forge-config Crate**: Full configuration management with validation, sanitization, and hot-reload
- **Cost Tracking**: Complete database implementation with monthly cost queries
- **Chat Backend**: Pluggable provider system with comprehensive test coverage
- **Worker Management**: Full TUI integration with spawn, kill, pause, and resume functionality
- **Test Coverage**: 1000+ tests passing across all crates

### Infrastructure
- **Self-Update**: Built-in binary update mechanism with Ctrl+U hotkey
- **Hot-Reload**: Configuration changes applied without restart
- **Graceful Error Recovery**: Configurable recovery policies for workers
- **Audit Logging**: Complete audit trail in forge-chat

### Changes from v0.2.0
- Version bump to 0.3.0 for production release
- All tests passing (forge-cost test_monthly_costs fixed)
- No dead code warnings in workspace
- Clean compilation with minimal clippy warnings

## [0.2.0] - 2026-03-25

### Added
- **Intelligent Model Routing**: Automatic model selection based on task complexity
  - Complexity scoring system (0-100 scale) for incoming tasks
  - Three-tier routing: Budget (Haiku), Standard (Sonnet), Premium (Opus)
  - Cost savings estimation from intelligent routing decisions
  - New Routing view accessible via `[r]` hotkey
- **Routing Analytics Panel**: Visual dashboard for routing statistics
  - Tier distribution with percentage bars
  - Recent routing decisions table
  - Average complexity and routing efficiency metrics

### Fixed
- Panel focus visual indicators standardized across all views
- Chat rendering visual artifacts in narrow terminals

## [0.1.9] - 2026-02-12

### Added
- **TUI Onboarding Wizard**: Interactive TUI wizard integrated into onboarding flow
- **Paused Worker Indicators**: Visual indicators for paused workers in dashboard
- **Activity Monitoring**: Idle vs stuck detection for worker activity
- **Performance Metrics Dashboard**: Real-time performance metrics display
- **Non-interactive Onboarding**: FORGE_CHAT_BACKEND environment variable support
- **Streaming Tokens**: Chat display shows streaming tokens in real-time
- **Response Time Tracking**: Worker health monitoring with response time metrics
- **Confirmation Dialogs**: Confirmation dialog for destructive actions
- **Pause Signal Handling**: Workers can now handle pause signals gracefully
- **CLI Guidance**: Helpful guidance displayed when no CLI tools are detected
- **Memory Monitoring**: Per-worker memory usage monitoring
- **Graceful Error Recovery**: Complete error recovery epic with configurable policies
- **Auto-Recovery Actions**: Automated recovery actions for worker management
- **Stuck Task Detection**: Detection system for tasks that are stuck
- **Crash Recovery Module**: Worker crash recovery with exponential backoff
- **Network Timeout Recovery**: Graceful handling of network timeouts
- **Invalid Config Handling**: Graceful handling of invalid config files
- **Database Retry Logic**: Exponential backoff for database operations
- **Panel Focus Indicators**: Enhanced visual indicators for focused panels
- **Task Filtering and Search**: Filter and search tasks in the dashboard

### Changed
- Version bump for release

## [0.1.8] - 2026-02-12

### Added
- **Worker Status Tests**: Comprehensive real-time worker status update tests
- **Responsive Layout Tests**: Tests for responsive layout adaptation across terminal sizes
- **Task Priority Filtering**: Filter tasks by priority using 0-4 keys
- **Worker Kill Functionality**: Kill workers with K key
- **Worker Spawn Functionality**: Spawn workers with S key
- **Help Overlay Tests**: Comprehensive tests for ? and h key help overlay

### Fixed
- Header format alignment in TUI tests
- CI configuration to create minimal forge config preventing onboarding during tests
- Replaced Dagger release workflow with cargo-based workflow

## [0.1.7] - 2026-02-11

### Added
- **GitHub Release Automation**: Automated release workflow with auto-versioning

## [0.1.6] - 2026-02-11

### Changed
- Version bump (no functional changes, tag created for v0.1.7)

## [0.1.5] - 2026-02-11

### Added
- **Dagger CI Module**: CI/CD module using Dagger for builds
- **Visual Feedback for Updates**: Visual feedback when pressing Ctrl+U for updates
- **GitHub Actions CI Pipeline**: Automated CI pipeline for testing and linting
- **Worker Management Tests**: Comprehensive worker management test suite
- **View Navigation Tests**: Tests for view navigation functionality
- **Version Bump Script**: Automation script for version bumping
- **Automated Testing Framework**: tmux-based automated testing framework
- **Initialization Diagnostics**: Timing diagnostics for hang investigation
- **Chat Backend Integration**: Integrated ChatBackend with TUI for interactive chat
- **Config Validation**: Validation for configuration files
- **Onboarding Flow**: Complete onboarding flow with CLI tool detection
- **Update Notification Banner**: Dashboard banner for update notifications
- **Semver Version Display**: Display version in dashboard header
- **Update Helper Script**: update-forge.sh helper script
- **Internal Updater**: Ctrl+U hotkey for updates
- **Terminal Dimensions Display**: Show terminal dimensions in dashboard header

### Fixed
- Chat responses not displaying in UI
- Chat requests made non-blocking using background threads
- ChatConfig parsing from config.yaml
- OpenCode headless support detection
- API key requirement removed from CLI tool detection
- Clippy and formatting issues for CI pipeline
- Status file current_task format inconsistency

### Changed
- Removed demo/mock subscription data from dashboard
- Updated README to document responsive layout modes

### Documentation
- Comprehensive architecture documentation
- Chat backend architecture documentation
- Test validation guidelines
- ADR 0016 for onboarding flow and CLI detection

## [0.1.4] - 2026-02-11

### Changed
- Internal version bump (changes included in 0.1.5)

## [0.1.3] - 2026-02-11

### Changed
- Internal version bump (changes included in 0.1.5)

## [0.1.2] - 2026-02-11

### Changed
- Internal version bump (changes included in 0.1.5)

## [0.1.1] - 2026-02-10

### Changed
- Internal version bump (changes included in 0.1.5)

## [0.1.0] - 2026-02-09

### Added
- **Provider Architecture**: Pluggable chat provider system with MockProvider, ClaudeCliProvider, and ClaudeApiProvider
- **Comprehensive Testing**: 65 tests including 22 new provider integration tests
- **Chat Backend**: Refactored backend with pluggable CLI worker support
- **Provider Factory**: Configuration-based provider creation with environment variable override
- **Theme Support**: Configurable color themes (Default, Dark, Light, Cyberpunk)
- **Performance Metrics**: Real-time visualization panel for worker performance
- **Sparkline Charts**: Reusable sparkline widget for metrics visualization
- **Progress Bars**: Enhanced progress bar widget library
- **Quick Actions**: Hotkey panel for rapid worker management
- **Documentation**: Updated README with architecture and usage examples

### Changed
- Extracted ClaudeApiProvider into separate module for better code organization
- Optimized FORGE performance across the dashboard

### Fixed
- Status integration for worker health monitoring
- Provider configuration and factory initialization

### Technical
- ChatProvider trait with process(), name(), model(), and supports_streaming() methods
- ProviderResponse with token usage, cost tracking, and finish reasons
- MockProvider with call tracking, multiple responses, and error simulation
- Tool execution integration with provider responses
- Rate limiting enforcement across all providers
- Concurrent provider usage support

## [Unreleased]

### Added - Phase 4: Team Collaboration & Enterprise Features

#### Team Collaboration (forge-server)
- **Server Mode** (`forge --server`): Multi-user collaborative sessions with real-time state synchronization
  - WebSocket server using Axum framework
  - Real-time broadcast of state changes to all connected clients
  - Session management with activity tracking and cleanup
  - Audit logging for compliance and security
  
- **Client Mode** (`forge --connect ws://host:port`): Connect to remote FORGE server
  - Real-time state synchronization from server
  - Command submission with user attribution
  - Connection status indicator in TUI header
  - Full TUI functionality in client mode

- **Role-Based Access Control (RBAC)**:
  - Three roles: Viewer (read-only), Operator (workers/tasks), Admin (full access)
  - Pluggable `AuthProvider` trait for custom authentication
  - SimpleAuth provider with default users (admin/admin123, operator/operator123, viewer/viewer123)
  - Permission checking system for all actions
  - User attribution for all mutating operations

- **Bead Assignment System**:
  - Shared task queue with assignment tracking
  - Assign/unassign/reassign operations with attribution
  - User assignment counts and queries
  - Real-time broadcast of assignment changes
  - Assignment history and statistics

- **Sessions View** (hotkey `s`):
  - View all connected users
  - User roles color-coded (Admin=red, Operator=yellow, Viewer=blue)
  - Current view being observed by each user
  - Connection status and last activity time
  - Session metadata display

#### Audit Logging System
- **Comprehensive Audit Trail**:
  - SQLite backend at `~/.forge/audit.db`
  - Append-only immutable log for compliance
  - 14 event types (WorkerSpawn, WorkerKill, BeadStatusChange, ConfigChange, etc.)
  - Severity levels (Info, Warning, Error, Critical)
  - Actor attribution for all events

- **Audit Log View** (hotkey `Z`):
  - TUI panel for viewing audit events
  - Time range filtering (last hour, day, week, custom)
  - Filter by event type, actor, entity
  - Statistics dashboard with event counts
  - Export functionality

- **Export & Query**:
  - Export to JSON (hotkey `E`)
  - Export to CSV (hotkey `C`)
  - Configurable retention policy (default 90 days)
  - Query by time range, entity, actor, event type
  - Maximum 10,000 records per query

#### Architecture & Infrastructure
- **forge-server Crate**:
  - `auth.rs` - Authentication and authorization with RBAC
  - `session.rs` - Session management and tracking
  - `assignment.rs` - Bead assignment tracking
  - `protocol.rs` - WebSocket protocol messages
  - `websocket.rs` - WebSocket server/client implementation
  - `client.rs` - Client mode TUI integration

- **Protocol Design**:
  - Server messages: Welcome, StateUpdate, UserJoined, UserLeft, BeadAssigned, WorkerChanged, BeadChanged, ChatMessage, Ping
  - Client messages: Authenticate, SyncState, AssignBead, SpawnWorker, KillWorker, ChangeBeadStatus, ChatMessage, UpdateView, Pong
  - JSON-based message format
  - Bidirectional real-time communication

- **Integration Tests**:
  - Team collaboration integration tests
  - Multi-user session tests
  - Authentication and authorization tests
  - Bead assignment workflow tests
  - CI coverage for all forge-server features

### Changed
- **CLI Arguments**: Added `--server` and `--connect` flags
- **Configuration**: New `server` section in config.yaml for server/client settings
- **Main Loop**: TUI app loop supports both standalone and server/client modes
- **State Management**: Real-time state updates from server in client mode

### Security Considerations
- SimpleAuth uses plaintext password comparison (development only)
- Default users with hardcoded credentials (must be replaced for production)
- WebSocket connections use `ws://` protocol (upgrade to `wss://` for production)
- Audit logging provides compliance trail for all actions
- Network security recommendations in TEAM_COLLABORATION.md

### Documentation
- **TEAM_COLLABORATION.md**: Complete guide for team collaboration features
- **Architecture Documentation**: Updated with server/client architecture
- **API Reference**: Server/client protocol message documentation
- **Troubleshooting Guide**: Common issues and solutions
- **Security Guidelines**: Production deployment recommendations

[Unreleased]: https://github.com/jedarden/forge/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/jedarden/forge/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/jedarden/forge/compare/v0.1.9...v0.2.0
[0.1.9]: https://github.com/jedarden/forge/compare/v0.1.8...v0.1.9
[0.1.8]: https://github.com/jedarden/forge/compare/v0.1.7...v0.1.8
[0.1.7]: https://github.com/jedarden/forge/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/jedarden/forge/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/jedarden/forge/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/jedarden/forge/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/jedarden/forge/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/jedarden/forge/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/jedarden/forge/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/jedarden/forge/releases/tag/v0.1.0
