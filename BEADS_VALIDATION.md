# FORGE Project Beads - Gaps, Features & Validation

This document provides a comprehensive overview of all project beads (issues/tasks) with detailed validation instructions.

## Quick Reference

```bash
# View all beads
br list

# View ready beads (unblocked)
br ready

# Show bead details
br show <bead-id>

# Update bead status
br update <bead-id> --status in_progress
br close <bead-id>

# Run tests
cd /home/coder/forge
./test-forge-chat.sh
./tests/run-all-tests.sh
```

---

## 🔴 P0 - Critical Blockers

### fg-32q: Fix chat responses not displaying in UI
**Type:** Bug | **Status:** ✅ VALIDATED
**Labels:** chat, ui, blocking

**Problem:** Chat backend processes queries successfully and sends responses via channel, but nothing appears in UI.

**Resolution:** The chat UI rendering was validated working on 2026-02-11. The architecture correctly:
1. Sends queries to background thread via channel
2. Receives responses via `poll_chat_responses()` each frame
3. Updates chat history and marks UI dirty for redraw
4. Displays responses in Chat History panel with timestamps

**Validation Script:** `./test-forge-chat.sh`

**Last Validation Run (2026-02-11):**
```
TEST RESULTS:
✅ CHECK 1: Chat view displayed - PASS
✅ CHECK 2: User query in history - PASS
✅ CHECK 3: Response received - PASS
✅ CHECK 4: Timestamp displayed - PASS
✅ CHECK 5: Multiple exchanges - PASS
✅ CHECK 6: View persistence after switch - PASS
```

**Pass Criteria (all met):**
- ✅ Response text length > 0 in logs
- ✅ Chat history count increases
- ✅ Text visible in tmux pane
- ✅ No errors in diagnostic logs

---

### fg-3v4: Investigate 65-second initialization hang
**Type:** Bug | **Status:** Open  
**Labels:** performance, initialization

**Problem:** Forge hangs ~65 seconds during startup between status watcher and theme loading.

**Validation:**
```bash
# Time startup
time timeout 10 forge --version

# Check timing logs
tail -100 ~/.forge/logs/forge.log.$(date +%Y-%m-%d) | grep '⏱️'
```

**Expected Output:**
```
⏱️ StatusWatcher initialized in XXXms
⏱️ BeadManager initialized in XXXms
⏱️ Cost database initialized in XXXms
⏱️ Initial poll_updates() took XXXms
⏱️ DataManager::new() completed in XXXms
```

**Pass Criteria:**
- ✅ Total startup < 2 seconds
- ✅ No component > 500ms
- ✅ UI appears within 1 second

---

### fg-2xh: Validate chat UI rendering after response received
**Type:** Task | **Status:** Open  
**Labels:** testing, validation

**Purpose:** Automated validation that chat responses display correctly.

**Validation:**
```bash
./test-forge-chat.sh

# Visual inspection
tmux attach -t forge-test-XXXXX

# Check for:
# - User query visible
# - Assistant response visible
# - Timestamps present
# - No errors
```

---

### fg-2ir: Measure and optimize initialization performance
**Type:** Task | **Status:** Open  
**Labels:** performance, initialization

**Purpose:** Profile startup and optimize slow components.

**Validation:**
```bash
# Baseline
time forge --version

# With 100 status files
for i in {1..100}; do
  echo '{}' > ~/.forge/status/test-$i.json
done
time forge --version
```

**Performance Targets:**
- StatusWatcher: < 100ms
- BeadManager: < 50ms
- Cost DB: < 100ms
- poll_updates: < 500ms
- **Total: < 1500ms**

---

## 🟡 P1 - High Priority

### fg-3qx: Implement automated tmux-based testing framework
**Type:** Feature | **Status:** Open  
**Labels:** testing, automation, infrastructure

**Scripts to Create:**
1. ✅ test-forge-chat.sh (DONE)
2. ⬜ test-forge-workers.sh
3. ⬜ test-forge-views.sh
4. ⬜ test-forge-theme.sh
5. ⬜ run-all-tests.sh

**Validation:**
```bash
# Individual tests
./tests/test-forge-chat.sh
./tests/test-forge-workers.sh

# Full suite
./tests/run-all-tests.sh

# Should output:
# ✅ test-forge-chat.sh PASSED (2.1s)
# ✅ test-forge-workers.sh PASSED (3.5s)
# ...
```

---

### fg-1qi: Implement worker management tests
**Type:** Task | **Status:** Open  
**Labels:** testing, workers

**Test Coverage:**
- Spawn worker (s key)
- Kill worker (k key)
- Status updates
- Multiple workers
- Worker panel display

**Validation:**
```bash
./tests/test-forge-workers.sh

# Should test:
# 1. Press 's' to spawn
# 2. Verify worker in status
# 3. Press 'k' to kill
# 4. Verify worker removed
```

---

### fg-1uo: Implement view navigation tests
**Type:** Task | **Status:** Open  
**Labels:** testing, ui

**Test Coverage:**
- All view keys: w, t, c, m, l, o
- Next/prev navigation
- Help panel (? key)
- View persistence

**Validation:**
```bash
./tests/test-forge-views.sh

# Should test each key and verify:
# - View switches correctly
# - Panel title updates
# - Content renders
```

---

### fg-3bq: Fix status file current_task format inconsistency
**Type:** Bug | **Status:** Open  
**Labels:** status-files, parsing

**Problem:** Status files have inconsistent `current_task` formats (string vs object).

**Solution:** Custom deserializer handles both:
- String: `"bd-abc"`
- Object: `{"bead_id": "bd-abc", "bead_title": "...", "priority": 1}`

**Validation:**
```bash
# Test string format
echo '{"worker_id": "test", "status": "active", "current_task": "bd-123"}' > /tmp/test.json
br show bd-123  # Should work

# Test object format
echo '{"worker_id": "test", "status": "active", "current_task": {"bead_id": "bd-456", "priority": 1}}' > /tmp/test.json
br show bd-456  # Should work

# Check no warnings in logs
forge &
sleep 3
pkill forge
tail -50 ~/.forge/logs/forge.log.$(date +%Y-%m-%d) | grep -i "failed to parse"
# Should be empty
```

---

### fg-6ri: Write comprehensive test validation guidelines
**Type:** Task | **Status:** Open  
**Labels:** documentation, testing

**Deliverable:** `tests/README.md` with:
- Test framework overview
- How to run tests
- How to write new tests
- Tmux testing patterns
- Log parsing techniques
- Troubleshooting

**Validation:**
- ✅ README.md exists
- ✅ Contains examples
- ✅ Documents all test scripts
- ✅ Explains assertion patterns

---

## 🟢 P2 - Medium Priority

### fg-1y4: Add version bump automation script
**Type:** Task | **Status:** Open  
**Labels:** automation, release

**Deliverable:** `scripts/bump-version.sh`

**Usage:**
```bash
./scripts/bump-version.sh patch   # 0.1.4 -> 0.1.5
./scripts/bump-version.sh minor   # 0.1.4 -> 0.2.0
./scripts/bump-version.sh major   # 0.1.4 -> 1.0.0
./scripts/bump-version.sh --dry-run patch
```

**Validation:**
```bash
# Test patch bump
OLD=$(forge --version)
./scripts/bump-version.sh patch
NEW=$(forge --version)
# NEW should be incremented

# Test dry-run
./scripts/bump-version.sh --dry-run patch
# Version unchanged
```

---

### fg-3mg: Document chat backend architecture
**Type:** Task | **Status:** Open  
**Labels:** documentation

**Deliverable:** `docs/CHAT_BACKEND.md` with:
- Architecture overview
- Config structure
- Provider interface
- Headless mode requirements
- Error handling
- Testing approach

---

### fg-2e3: Set up GitHub Actions CI pipeline
**Type:** Feature | **Status:** Open  
**Labels:** infrastructure, ci-cd

**Deliverable:** `.github/workflows/ci.yml`

**Pipeline Steps:**
1. cargo check
2. cargo test
3. cargo clippy
4. Run automated tmux tests
5. Validate version consistency
6. Check formatting

**Validation:**
```bash
# Test locally with act
act -j test

# Or trigger workflow
git push origin main
# Check GitHub Actions tab
```

---

### fg-cmx: Document forge architecture
**Type:** Task | **Status:** Open  
**Labels:** documentation, architecture

**Deliverable:** `docs/ARCHITECTURE.md` with:
- System overview
- Module structure
- Data flow diagrams
- TUI rendering pipeline
- Chat backend design
- Worker management
- Cost tracking
- Beads integration

---

## Testing Workflow

### For Bug Fixes

1. Read bead details: `br show <bead-id>`
2. Mark in progress: `br update <bead-id> --status in_progress`
3. Reproduce issue using validation steps
4. Fix the bug
5. Run relevant tests
6. Verify all pass criteria met
7. Close bead: `br close <bead-id>`

### For Features

1. Read bead details: `br show <bead-id>`
2. Check dependencies: `br show <bead-id> | grep depends`
3. Mark in progress: `br update <bead-id> --status in_progress`
4. Implement feature
5. Write tests (if testing bead)
6. Run validation steps
7. Verify acceptance criteria
8. Close bead: `br close <bead-id>`

### For Tests

1. Implement test script
2. Make executable: `chmod +x tests/test-*.sh`
3. Run test: `./tests/test-*.sh`
4. Verify exit code 0
5. Check output formatting
6. Test cleanup (no orphaned tmux sessions)
7. Add to run-all-tests.sh
8. Document in tests/README.md

---

## Priority Guide

- **P0 (Critical):** Blocks other work, breaks core functionality
- **P1 (High):** Important for release, high user impact
- **P2 (Medium):** Nice to have, quality improvements
- **P3 (Low):** Future enhancements, documentation

---

## Commands Reference

```bash
# Beads Management
br list                          # List all beads
br ready                         # Show ready beads
br show <bead-id>                # Show details
br update <bead-id> --status in_progress
br close <bead-id>              # Mark complete

# Testing
./test-forge-chat.sh            # Chat test
./tests/run-all-tests.sh        # Full suite
tmux attach -t forge-test-XXX   # Inspect test

# Logs
tail -f ~/.forge/logs/forge.log.$(date +%Y-%m-%d)
grep '⏱️' ~/.forge/logs/forge.log.$(date +%Y-%m-%d)

# Performance
time forge --version
time timeout 10 forge

# Version
./scripts/bump-version.sh patch
forge --version
```

---

**Last Updated:** 2026-02-11  
**Total Beads:** 13  
**P0:** 4 | **P1:** 5 | **P2:** 4
