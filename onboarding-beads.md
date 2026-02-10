# Onboarding Feature Implementation Tasks

## Implement CLI tool detection logic
- **Type**: task
- **Priority**: P0
- **Labels**: onboarding, cli-detection, implementation
- **Description**: Implement the CLI tool detection system per ADR 0016. Create crates/forge-init/ with detection.rs module that searches PATH for claude, opencode, aider binaries and detects their capabilities.

## Create onboarding TUI wizard
- **Type**: task
- **Priority**: P0
- **Labels**: onboarding, tui, wizard
- **Description**: Create interactive TUI wizard for first-run setup showing detected tools with status indicators and allowing user selection. Use ratatui widgets matching forge's existing theme.

## Implement config generator
- **Type**: task
- **Priority**: P0
- **Labels**: onboarding, config-generation
- **Description**: Create config.yaml and launcher script generator from detected CLI tool. Functions for generating config, launcher scripts, directory structure, and permissions.

## Add config validation
- **Type**: task
- **Priority**: P1
- **Labels**: onboarding, validation
- **Description**: Validate generated config by testing chat backend connection. Create validator.rs with validation tests.

## Integrate onboarding into main
- **Type**: task
- **Priority**: P0
- **Labels**: onboarding, integration
- **Description**: Integrate onboarding flow into forge main binary. Trigger automatically on missing config.yaml. Add 'forge init' CLI command.

## Write onboarding tests
- **Type**: task
- **Priority**: P1
- **Labels**: onboarding, testing
- **Description**: Write integration tests for full onboarding flow. Test detection, generation, and validation with mocked CLI tools.
