# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.1.0]: https://github.com/jedarden/forge/releases/tag/v0.1.0
