# FORGE Development Guide for Claude Code

This document provides context and procedures for AI assistants working on the FORGE project.

## Project Overview

FORGE is a terminal-based Agent Orchestration Dashboard written in Rust using ratatui. It manages AI worker agents, tracks tasks/beads, monitors costs, and includes a conversational chat interface.

- **Language**: Rust (edition 2024, requires 1.88+)
- **TUI Framework**: ratatui 0.29 with crossterm 0.28
- **Async Runtime**: tokio 1.43
- **Database**: SQLite via rusqlite (bundled)
- **Version**: 0.3.0 (workspace.version)

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
cd /home/coding/FORGE
cargo build --release

# 2. Create a fresh tmux session for testing
tmux new-session -d -s forge-test -x 120 -y 40

# 3. Run forge in the test session
tmux send-keys -t forge-test "cd /home/coding/FORGE && ./target/release/forge --debug" Enter

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

**IMPORTANT: Cleanup Test Sessions**

Always clean up tmux sessions after testing to prevent accumulation:

```bash
# Kill your test session when done
tmux kill-session -t forge-test

# Clean up all forge-related test sessions
tmux list-sessions | grep '^forge-' | cut -d: -f1 | xargs -I{} tmux kill-session -t {}

# The test framework provides cleanup helpers (see tests/lib/test-helpers.sh)
stop_forge "$session"           # Clean up main test session
cleanup_spawned_workers         # Clean up worker sessions spawned during test
```

For detailed testing architecture and cleanup requirements, see:
- **[ADR 0017: Tmux-Based Testing with Agent Control and Cleanup](docs/adr/0017-tmux-based-testing-with-cleanup.md)**
- **[Bead bd-3er6](https://github.com/jedarden/forge/issues)** - Implements cleanup for test-forge-workers.sh

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

### Releasing

**There are no GitHub Actions here.** They are disabled org-wide and must never
be re-enabled. Releases are built by the `forge-ci` Argo WorkflowTemplate in the
`iad-ci` cluster, triggered automatically by a push of `main` to the Forgejo
origin. Nothing is tagged or released by hand.

**How the pipeline works**:

1. A push of `refs/heads/main` to `git.ardenone.com/jedarden/forge` fires a
   Forgejo webhook (`https://webhooks-ci.ardenone.com/forge`).
2. The `forge-ci-sensor` (Argo Events, in `iad-ci`) filters that webhook to
   `push` events on `main` only and submits a `forge-ci-*` Workflow in the
   `argo-workflows` namespace.
3. The workflow clones `main` from Forgejo, runs
   `scripts/definition-of-done.sh --all` (fmt + clippy + tests — a failing DoD
   fails the release), builds the release binary, tags `v<version>` (version
   taken from `workspace.package.version` in `Cargo.toml`), pushes that tag to
   **Forgejo**, and publishes the GitHub release with the binary. GitHub is the
   public artifact mirror, not the build system.

Two consequences of that design:

- **Do not tag by hand.** The workflow creates `v<version>` itself; the old
  dual-tag convention (`forge-v0.x.y` alongside `v0.x.y`) is retired. The tag
  must live on Forgejo anyway — every mirror sync prunes refs that exist only
  on GitHub.
- A release ships on the **first push to `main` after a version bump**. Later
  pushes with the same version are skipped (the workflow exits early if that
  version is already published), and runs are serialized by a `forge-ci`
  mutex, so repeated pushes queue instead of colliding.

**Prerequisites** (before pushing the version-bump commit):
- All tests passing, `cargo clippy` clean (forge-ci re-runs the full DoD remotely)
- Version updated in `Cargo.toml` (workspace.version)
- CHANGELOG.md updated with release notes
- Binary tested in real tmux session

**Release Process**:

```bash
# 1. Update version in workspace Cargo.toml
# Edit workspace.package.version = "0.x.y"

# 2. Update CHANGELOG.md
# Add new section with release notes

# 3. Commit directly to main, staging precise paths (never `git add .`)
git add Cargo.toml CHANGELOG.md
git commit -m "chore: bump version to 0.x.y"

# 4. Push to the Forgejo origin — this triggers forge-ci. No manual tagging.
git push origin main

# 5. Watch for the forge-ci run (read-only, credential-free endpoint)
kubectl --server=http://traefik-iad-ci:8001 \
  get workflows -n argo-workflows --sort-by=.metadata.creationTimestamp | tail -5

# 6. Check the run's phase once it appears (name is forge-ci-<suffix>)
kubectl --server=http://traefik-iad-ci:8001 \
  get workflow forge-ci-xxxxx -n argo-workflows \
  -o jsonpath='{.status.phase} - {.status.message}'
```

The tag and release appear at https://github.com/jedarden/forge/releases once
the workflow reaches `Succeeded`. For per-step detail use the Argo UI
(https://argo-ci.ardenone.com, Google SSO, VPN only); completed-run logs are
kept there for 30min on success / 2h on failure. To read logs with kubectl you
must catch the pod while it is running — `podGC: OnPodCompletion` deletes pods
the moment they finish.

**If a release run fails**: fix forward on `main` and push again — every push
re-triggers forge-ci. As a last resort a run can be submitted manually with the
`iad-ci` kubeconfig (`workflowTemplateRef: forge-ci`) — the documented
exception allowing `kubectl create` of an Argo Workflow in `argo-workflows`.

**Release Checklist**:
- [ ] Run `cargo test` - all tests pass
- [ ] Run `cargo clippy` - no warnings
- [ ] Test TUI in separate tmux session (not in Claude Code)
- [ ] Test chat feature with real queries
- [ ] Verify no visual artifacts in Chat view
- [ ] Test all hotkeys and view navigation
- [ ] Update CHANGELOG.md with new features/fixes
- [ ] Update workspace.version in Cargo.toml
- [ ] Commit to main with precise staged paths
- [ ] Push to Forgejo origin (`git push origin main`)
- [ ] Verify the `forge-ci-*` workflow reaches `Succeeded`
- [ ] Verify tag + release appear at https://github.com/jedarden/forge/releases

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

Forgejo (`https://git.ardenone.com/jedarden/forge`) is the origin and source of
truth; GitHub (`jedarden/forge`) is a read-only mirror kept current by Forgejo's
server-side push mirror. **Push only to the Forgejo origin.**

```bash
# Work directly on main — no feature branches, no PR flow
git checkout main

# Make changes, then stage precise paths
# (never `git add .`, `git add -A`, or `git commit -a`)
git add crates/forge-tui/src/app.rs tests/
git commit -m "feat: add my feature"

# Push to Forgejo; the GitHub mirror updates automatically
git push origin main
```

- **Never create feature branches or PRs** — commit straight to `main` and push.
- **Never force-push** (`--force` or `--force-with-lease`). If local and origin
  history diverge, reconcile with a merge commit.
- Stage explicit paths — blanket staging sweeps in unrelated working-tree state.
- Every push to `main` triggers a `forge-ci` run (see Releasing) — keep the
  tree green before pushing.

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

- **Repository (origin)**: https://git.ardenone.com/jedarden/forge
- **GitHub mirror**: https://github.com/jedarden/forge (read-only; releases published here)
