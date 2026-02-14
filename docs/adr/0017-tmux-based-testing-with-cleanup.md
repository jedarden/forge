# ADR 0017: Tmux-Based Testing with Agent Control and Cleanup

**Date**: 2026-02-13
**Status**: Accepted
**Deciders**: FORGE Development Team

## Context

FORGE is a terminal-based TUI application built with ratatui that requires testing in actual terminal environments. Early testing approaches faced several challenges:

1. **Testing TUI applications in Claude Code's terminal emulator** causes visual artifacts and incorrect rendering due to:
   - Screen corruption from alternate screen mode
   - Key events not captured correctly
   - Terminal state not properly restored on exit

2. **Test sessions accumulating** - The test suite spawns tmux sessions but doesn't clean them up, leading to:
   - 12+ orphaned `forge-forge-*` sessions after running test suite
   - Sessions stuck in `sleep 300` loops consuming resources
   - Manual cleanup required between test runs
   - CI/CD environments accumulating sessions over time

3. **Agent worker testing** - AI agents (via bead workers) need to:
   - Run FORGE in controlled environments
   - Interact with the TUI programmatically
   - Validate rendering at multiple terminal sizes
   - Clean up test artifacts automatically

## Decision

**All FORGE testing MUST occur in dedicated tmux sessions that are agent-controllable and automatically cleaned up.**

Key requirements:

1. **Tmux-First Testing**
   - All test scripts create dedicated tmux sessions (not Claude Code's terminal)
   - Sessions use deterministic naming: `forge-test-<script>-<timestamp>-<pid>`
   - Tests interact via `tmux send-keys` and `tmux capture-pane`

2. **Agent Control Protocol**
   - Tests expose session names for agent attachment
   - Agents can send keys, capture output, and validate rendering
   - Support for multiple terminal dimensions (80x24, 120x40, 199x55)

3. **Mandatory Cleanup**
   - **Session Tracking**: Record all created sessions (test + spawned workers)
   - **Cleanup Functions**: `cleanup_spawned_workers()` kills sessions after test
   - **Trap Handlers**: EXIT/INT/TERM signals trigger cleanup even on failure
   - **Naming Conventions**: Identify test sessions for automated cleanup

4. **Test Framework Integration**
   - `test-helpers.sh` provides session management utilities
   - `start_forge()` creates trackable sessions
   - `stop_forge()` ensures clean teardown
   - All tests inherit cleanup behavior

## Rationale

### Why Tmux Sessions?

1. **Isolation**: Separate environment from Claude Code's terminal
2. **Control**: Programmatic interaction via tmux commands
3. **Inspection**: Capture pane contents for assertions
4. **Realism**: True terminal environment with correct ANSI handling
5. **Parallel Testing**: Multiple concurrent test sessions

### Why Mandatory Cleanup?

1. **Resource Conservation**: Prevent tmux session accumulation
2. **CI/CD Reliability**: Clean state for every test run
3. **Developer Experience**: No manual cleanup between iterations
4. **Production Readiness**: Tests model real-world cleanup patterns

### Why Agent-Controllable?

1. **Autonomous Testing**: AI workers can validate their own work
2. **Continuous Validation**: Tests run as part of development workflow
3. **Multi-Dimensional Testing**: Agents test at various terminal sizes
4. **Integration Testing**: End-to-end workflows with real interactions

## Consequences

### Positive

✅ **Reliable Testing**
- No visual artifacts from Claude Code's terminal emulator
- Consistent rendering across test runs
- True terminal behavior validation

✅ **Clean Environments**
- No orphaned tmux sessions
- Predictable resource usage
- Fast test iterations

✅ **Agent Enablement**
- Workers can test their own changes
- Automated validation of TUI features
- Self-service testing for bead workers

✅ **Production Patterns**
- Tests model real-world session management
- Cleanup patterns reusable in production
- Better error handling

### Negative

❌ **Test Complexity**
- Requires tmux knowledge for test authoring
- More setup/teardown code
- Harder to debug (session indirection)

❌ **Dependencies**
- Requires tmux installed (assumed in devpod)
- Platform-specific (Linux/macOS, not Windows without WSL)

### Neutral

⚖️ **Test Duration**
- Slightly slower (tmux session overhead ~100ms)
- Offset by parallelization opportunities

⚖️ **Tooling Requirements**
- Test helpers abstract tmux complexity
- Once understood, pattern is consistent

## Implementation

### Session Lifecycle

```bash
# 1. Create tracked session
session=$(get_session_name)  # forge-test-workers-20260213-12345
start_forge "$session"

# 2. Interact with session
send_key_wait "$session" "w" 1
if pane_contains "$session" "Worker Pool"; then
    log_success "Workers view loaded"
fi

# 3. Cleanup (automatic via trap or explicit)
stop_forge "$session"
cleanup_spawned_workers  # Kill any worker sessions created
```

### Cleanup Strategy

```bash
# Track sessions before test
before_sessions=$(tmux list-sessions | grep '^forge-forge' | cut -d: -f1)

# After test, kill new sessions
cleanup_spawned_workers() {
    local after=$(tmux list-sessions | grep '^forge-forge' | cut -d: -f1)
    while IFS= read -r session; do
        if ! echo "$before" | grep -q "^$session$"; then
            tmux kill-session -t "$session"
        fi
    done <<< "$after"
}

# Trap handler ensures cleanup on failure
trap cleanup_spawned_workers EXIT INT TERM
```

### Session Naming Convention

| Pattern | Purpose | Cleanup Responsibility |
|---------|---------|------------------------|
| `forge-test-*` | Main test session | `stop_forge()` |
| `forge-forge-*` | Spawned workers | `cleanup_spawned_workers()` |
| `forge-test-e2e-*` | E2E test sessions | `stop_forge()` |
| `claude-code-glm-5-*` | Bead workers (not test) | Worker framework |

## Alternatives Considered

### 1. Virtual Terminal Emulation (rejected)

**Approach**: Use terminal emulation libraries (e.g., `vty`, `termbox`)

**Pros**:
- No tmux dependency
- Faster (no process overhead)

**Cons**:
- Doesn't test real terminal behavior
- Misses ANSI escape sequence bugs
- Agents can't easily inspect output

### 2. Headless Testing with Expect (rejected)

**Approach**: Use `expect` scripts for interaction

**Pros**:
- Mature scripting language
- Good for automation

**Cons**:
- Additional dependency
- Less flexible than tmux
- Harder agent integration

### 3. Screenshot-Based Testing (rejected)

**Approach**: Render TUI to images, compare with baselines

**Pros**:
- Visual regression testing
- Catches layout bugs

**Cons**:
- Fragile (font/terminal differences)
- Can't test functionality
- No agent interaction

### 4. Manual Cleanup (rejected)

**Approach**: Document cleanup steps, rely on developers

**Pros**:
- Simpler test code
- No trap handlers needed

**Cons**:
- CI/CD sessions accumulate
- Developer friction
- Inconsistent cleanup

## References

- **Testing Guide**: `docs/CLAUDE.md` (lines 37-104)
- **Test Helpers**: `tests/lib/test-helpers.sh`
- **Example Tests**: `tests/test-forge-workers.sh`, `tests/test-forge-e2e.sh`
- **Related Bead**: `bd-3er6` - Add cleanup of spawned worker sessions
- **Worker Framework**: `/home/coder/claude-config/scripts/bead-worker.sh`

## Related ADRs

- **ADR 0002**: Use TUI for Control Panel Interface (defines why tmux testing is needed)
- **ADR 0007**: Bead Integration Strategy (agent workers need testable environment)
- **ADR 0009**: Dual Role Architecture (agents act as both developers and testers)

---

**Next Steps**:
1. Implement `cleanup_spawned_workers()` in `test-forge-workers.sh` (bd-3er6)
2. Add trap handlers to `test-helpers.sh`
3. Document session naming conventions in `tests/README.md`
4. Update all existing tests to use cleanup pattern
