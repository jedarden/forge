# FORGE Development Guide for Claude Code

This document provides context and procedures for AI assistants working on the FORGE project.

## Project Overview

FORGE is a terminal-based Agent Orchestration Dashboard written in Rust using ratatui. It manages AI worker agents, tracks tasks/beads, monitors costs, and includes a conversational chat interface.

- **Language**: Rust (edition 2024, requires 1.88+)
- **TUI Framework**: ratatui 0.29 with crossterm 0.28
- **Async Runtime**: tokio 1.43
- **Database**: SQLite via rusqlite (bundled)
- **Version**: 0.1.9 (workspace.version)

## Development Workflow

### Building

```bash
# Debug build (faster compilation)
cargo build

# Release build (optimized, what gets shipped)
cargo build --release

# Run tests
cargo test

# Run with logging
cargo run -- --debug

# Run release binary directly
./target/release/forge
```

### Testing in Tmux Sessions

**CRITICAL**: Always test the TUI in a separate tmux session, never inside Claude Code itself.

**Why**: The TUI uses alternate screen mode and raw terminal mode. Testing within Claude Code's terminal emulator can cause:
- Visual artifacts (ghost text, misalignment)
- Key capture issues
- Screen corruption on exit
- Terminal state not restored properly

**IMPORTANT: Test in Multiple Terminal Dimensions**

Visual artifacts often appear only at specific terminal sizes. FORGE has layout modes:
- **Narrow**: < 120 columns (single-view mode)
- **Wide**: 120-198 columns (2-column mode)
- **UltraWide**: 199+ columns (3-column mode with all panels)

Always test at **multiple dimensions** to catch layout-specific bugs:

```bash
# Test at narrow size (common for small terminals / split panes)
tmux new-session -d -s forge-narrow -x 80 -y 24

# Test at standard wide size
tmux new-session -d -s forge-wide -x 140 -y 40

# Test at ultra-wide size
tmux new-session -d -s forge-ultrawide -x 200 -y 50

# Test at minimum viable size
tmux new-session -d -s forge-min -x 60 -y 20
```

**Common dimension-specific issues**:
- Text overflow in narrow mode (< 80 cols)
- Panel clipping when height < 30 rows
- Timestamp misalignment at narrow widths
- Chat history truncation in small areas

**How to test correctly**:

```bash
# 1. Build the project
cd /home/coder/forge
cargo build --release

# 2. Create a fresh tmux session for testing
tmux new-session -d -s forge-test -x 120 -y 40

# 3. Run forge in the test session
tmux send-keys -t forge-test "cd /home/coder/forge && ./target/release/forge --debug" Enter

# 4. Attach to interact
tmux attach -t forge-test

# 5. When done, detach (Ctrl+B, D) or kill (Ctrl+B, :kill-session)
```

**Automated testing script**: Use `test-forge-chat.sh` for chat UI validation:
```bash
./test-forge-chat.sh
# This script:
# - Creates a tmux session
# - Starts forge
# - Switches to Chat view
# - Submits test queries
# - Validates rendering and responses
```

### Tmux Testing Commands

```bash
# List active sessions
tmux list-sessions

# Attach to a session
tmux attach -t <session-name>

# Kill a session
tmux kill-session -t <session-name>

# Capture pane content (for debugging)
tmux capture-pane -t <session-name> -p

# Send keys to a session (for automated testing)
tmux send-keys -t <session-name> "command" Enter
```

### Creating GitHub Releases

**Prerequisites**:
- All tests passing
- Version updated in `Cargo.toml` (workspace.version)
- CHANGELOG.md updated with release notes
- Binary tested in real tmux session

**Release Process**:

```bash
# 1. Update version in workspace Cargo.toml
# Edit workspace.package.version = "0.x.y"

# 2. Update CHANGELOG.md
# Add new section with release notes

# 3. Commit changes
git add Cargo.toml CHANGELOG.md
git commit -m "chore: bump version to 0.x.y"

# 4. Create git tag
git tag -a "v0.x.y" -m "Release v0.x.y"
git tag -a "forge-v0.x.y" -m "FORGE v0.x.y"

# 5. Push to GitHub
git push origin main
git push origin "v0.x.y"
git push origin "forge-v0.x.y"

# 6. GitHub Actions will build and publish release automatically
# Check: https://github.com/jedarden/forge/releases
```

**Release Checklist**:
- [ ] Run `cargo test` - all tests pass
- [ ] Run `cargo clippy` - no warnings
- [ ] Test TUI in separate tmux session (not in Claude Code)
- [ ] Test chat feature with real queries
- [ ] Verify no visual artifacts in Chat view
- [ ] Test all hotkeys and view navigation
- [ ] Update CHANGELOG.md with new features/fixes
- [ ] Update workspace.version in Cargo.toml
- [ ] Commit changes with descriptive message
- [ ] Create and push git tags
- [ ] Verify GitHub release created successfully

## Key File Locations

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point, CLI argument parsing |
| `crates/forge-tui/src/app.rs` | Main TUI application logic (~1400 lines) |
| `crates/forge-tui/src/view.rs` | View enum (Overview, Workers, Tasks, Chat, etc.) |
| `crates/forge-tui/src/event.rs` | Input event handling |
| `crates/forge-tui/src/theme.rs` | Color theme definitions |
| `crates/forge-chat/` | Chat backend integration |
| `crates/forge-core/` | Shared types and utilities |

## Known Issues and Workarounds

### Chat Visual Artifacts

**Issue**: Chat view may show visual artifacts when:
- Terminal is too narrow (<80 columns)
- Very long responses from chat backend
- Rapid switching between views

**Workaround**: Test in wide terminal (120+ columns) until proper wrapping is implemented.

**Location to fix**: `crates/forge-tui/src/app.rs` in `draw_chat()` function (line 2139)

### TUI Testing in Claude Code

**Issue**: Running ratatui apps within Claude Code's terminal can cause:
- Screen corruption
- Key events not captured correctly
- Alternate screen not restored on exit

**Workaround**: Always test in separate tmux session (see Testing section above).

## Common Development Tasks

### Adding a New View

1. Add variant to `View` enum in `crates/forge-tui/src/view.rs`
2. Add hotkey mapping in `View::hotkey()`
3. Add title in `View::title()`
4. Implement `draw_<view>()` method in `app.rs`
5. Add case to main `draw()` method match statement
6. Update `View::ALL` array
7. Test navigation and rendering

### Adding Chat Commands

1. Parse command in `app.rs` event handler (`handle_key()`)
2. Extract command arguments from `self.chat_input`
3. Execute command logic
4. Format response for chat history
5. Test command execution in Chat view

## Git Workflow

```bash
# Create feature branch
git checkout -b feature/my-feature

# Make changes and commit
git add .
git commit -m "feat: add my feature"

# Push to GitHub
git push origin feature/my-feature

# Create PR via GitHub web UI
# https://github.com/jedarden/forge/compare
```

## Useful Commands

```bash
# Format code
cargo fmt

# Lint code
cargo clippy -- -D warnings

# Run all tests
cargo test --workspace

# Run specific test
cargo test --package forge-tui test_name

# Build documentation
cargo doc --open

# Check for unused dependencies
cargo +nightly udeps
```

## Contact

- **Repository**: https://github.com/jedarden/forge
- **Issues**: https://github.com/jedarden/forge/issues
- **Discussions**: https://github.com/jedarden/forge/discussions
