# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

## [0.1.9] - 2026-02-12

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

[Unreleased]: https://github.com/jedarden/forge/compare/v0.1.9...HEAD
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
