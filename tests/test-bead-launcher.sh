#!/usr/bin/env bash
# ==============================================================================
# FORGE Bead-Aware Launcher — End-to-End Verification
# ==============================================================================
# Verifies the full bead-aware launcher pipeline from
# docs/BEAD_LAUNCHER_PROTOCOL.md against real bead stores, per ADR 0017
# (dedicated tmux sessions, agent-controllable, mandatory cleanup):
#
#   1. Bead-aware launch: --bead-ref fetches the bead, injects the prompt,
#      marks the bead in_progress with the session as assignee, and reports
#      bead_id in the launcher's JSON output and the status file.
#   2. Success completion: the detached monitor closes the bead via the
#      bead CLI (`bead close --reason "Completed by <session>"`).
#   3. Failure completion: a worker that dies without a clean exit gets its
#      bead released (`bead release` -> open/unassigned).
#   4. Standard mode: no --bead-ref -> no bead state touched.
#   5. Example launcher: its finite simulated worker drives the same
#      close-on-completion path end to end.
#
# Every scenario runs in an isolated temp workspace with its own bead store
# (bead init) and an isolated HOME, so the real ~/.forge and any real bead
# store are never touched. All tmux sessions use the forge-test prefix and
# are killed individually on exit (never `tmux kill-server` — the server is
# shared with other workers).
#
# Usage:
#   ./tests/test-bead-launcher.sh
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FORGE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PROD_LAUNCHER="$FORGE_ROOT/scripts/launchers/bead-worker-launcher.sh"
EXAMPLE_LAUNCHER="$FORGE_ROOT/test/example-launchers/bead-worker-launcher.sh"
BEAD_CLI="${FORGE_BEAD_CLI:-bead}"
export BEAD_CLI

TEST_SESSION_PREFIX="forge-test-beadref"
RUN_ID="$(date +%s)-$$"
TEST_SESSION="${TEST_SESSION_PREFIX}-harness-${RUN_ID}"

# Artifacts and workspaces (all removed on exit)
TEST_HOME="$(mktemp -d "${TMP_DIR:-/tmp}/forge-beadref-home-${RUN_ID}.XXXX")"
WS_SUCCESS="$(mktemp -d "${TMP_DIR:-/tmp}/forge-beadref-ws1-${RUN_ID}.XXXX")"
WS_FAILURE="$(mktemp -d "${TMP_DIR:-/tmp}/forge-beadref-ws2-${RUN_ID}.XXXX")"
WS_STANDARD="$(mktemp -d "${TMP_DIR:-/tmp}/forge-beadref-ws3-${RUN_ID}.XXXX")"
WS_EXAMPLE="$(mktemp -d "${TMP_DIR:-/tmp}/forge-beadref-ws4-${RUN_ID}.XXXX")"
TEST_SESSION_DIR="$(mktemp -d "${TMP_DIR:-/tmp}/forge-beadref-sess-${RUN_ID}.XXXX")"

# The bead CLI records a close reason for audit but never projects it back
# out (`bead show --json` has no close_reason field), so the launcher's
# close reason is verified by capturing the CLI's argv instead: a wrapper
# stands in front of the real binary, logs each invocation on one
# " | "-joined line, and execs it. BEAD_CLI is re-pointed at the wrapper so
# the harness helpers and the launchers (both honor FORGE_BEAD_CLI/BEAD_CLI)
# all route through it.
REAL_BEAD_CLI="$(command -v "${FORGE_BEAD_CLI:-bead}")"
BEAD_CALL_LOG="$TEST_SESSION_DIR/bead-cli-calls.log"
BEAD_CLI_WRAPPER="$TEST_SESSION_DIR/bead-cli-wrapper.sh"
cat > "$BEAD_CLI_WRAPPER" <<WRAPPER
#!/usr/bin/env bash
printf '%s | ' "\$@" >> '$BEAD_CALL_LOG'
printf '\n' >> '$BEAD_CALL_LOG'
exec '$REAL_BEAD_CLI' "\$@"
WRAPPER
chmod +x "$BEAD_CLI_WRAPPER"
BEAD_CLI="$BEAD_CLI_WRAPPER"

PASS=0
FAIL=0

log_info()  { echo -e "\033[0;34m[INFO]\033[0m $*"; }
log_pass()  { echo -e "\033[0;32m[PASS]\033[0m $*"; PASS=$((PASS + 1)); }
log_fail()  { echo -e "\033[0;31m[FAIL]\033[0m $*"; FAIL=$((FAIL + 1)); }

# ------------------------------------------------------------------------------
# Cleanup — mandatory per ADR 0017. Kills each tracked session by name and
# removes every temp dir. Runs on EXIT/INT/TERM.
# ------------------------------------------------------------------------------
cleanup() {
    local session
    for session in $(cat "$TEST_SESSION_DIR/sessions" 2>/dev/null || true); do
        tmux kill-session -t "$session" 2>/dev/null || true
    done
    if [ "${KEEP_ARTIFACTS:-0}" = "1" ]; then
        echo -e "\033[0;33m[KEEP]\033[0m artifacts kept: $TEST_HOME $WS_SUCCESS $WS_FAILURE $WS_STANDARD $WS_EXAMPLE $TEST_SESSION_DIR" >&2
        return
    fi
    rm -rf "$TEST_HOME" "$WS_SUCCESS" "$WS_FAILURE" "$WS_STANDARD" "$WS_EXAMPLE" "$TEST_SESSION_DIR"
    # Report leftovers of this run only (never touch other sessions)
    local leftovers
    leftovers="$(tmux ls 2>/dev/null | grep -F "$TEST_SESSION_PREFIX" | grep -F "$RUN_ID" || true)"
    if [ -n "$leftovers" ]; then
        echo -e "\033[0;31m[FAIL]\033[0m leftover sessions: $leftovers" >&2
    fi
}
trap cleanup EXIT INT TERM

track_session() {
    echo "$1" >> "$TEST_SESSION_DIR/sessions"
}

# ------------------------------------------------------------------------------
# Bead store helpers (each scenario workspace gets its own isolated store)
# ------------------------------------------------------------------------------
init_store() {   # init_store <workspace>
    (cd "$1" && "$BEAD_CLI" init --prefix fg >/dev/null 2>&1)
}

create_bead() {  # create_bead <workspace> <title> -> bead id
    (cd "$1" && "$BEAD_CLI" create --title "$2" --priority 1 --issue-type task 2>/dev/null | head -1)
}

bead_field() {   # bead_field <workspace> <bead-id> <jq-field>
    (cd "$1" && "$BEAD_CLI" show "$2" --json 2>/dev/null | jq -r ".[0].$3 // empty")
}
# wait_for invokes conditions via `bash -c` (a child shell); export the
# helpers it references so they resolve there.
export -f bead_field

# Wait until <condition command> succeeds, at most <timeout> seconds.
wait_for() {     # wait_for <timeout> <description> <command...>
    local timeout="$1" desc="$2"; shift 2
    local waited=0
    until "$@" >/dev/null 2>&1; do
        sleep 1
        waited=$((waited + 1))
        if [ "$waited" -ge "$timeout" ]; then
            log_fail "$desc (timed out after ${timeout}s)"
            return 1
        fi
    done
    log_pass "$desc"
}

# Launch the production launcher inside the dedicated harness tmux session,
# capturing stdout JSON. Keeping the launcher invocation itself inside a
# tracked tmux session follows ADR 0017 (all testing in agent-controllable
# sessions).
run_launcher_tmux() {  # run_launcher_tmux <session> <launcher> <workspace> <session-name> [bead-ref]
    local session="$1" launcher="$2" workspace="$3" name="$4" bead="${5:-}"
    local args=(--model=sonnet "--workspace=$workspace" "--session-name=$name")
    [ -n "$bead" ] && args+=(--bead-ref="$bead")
    tmux new-session -d -s "$session" \
        "HOME='$TEST_HOME' FORGE_BEAD_CLI='$BEAD_CLI' '$launcher' ${args[*]} > '$TEST_SESSION_DIR/$name.json' 2>'$TEST_SESSION_DIR/$name.stderr'"
    track_session "$session"
}

assert_eq() {  # assert_eq <actual> <expected> <description>
    if [ "$1" = "$2" ]; then
        log_pass "$3"
    else
        log_fail "$3 (expected '$2', got '$1')"
    fi
}

# ==============================================================================
# Scenario 1: Bead-aware launch through the production launcher
# ==============================================================================
scenario_bead_aware_launch() {
    log_info "Scenario 1: --bead-ref launch marks the bead in_progress with assignee"

    init_store "$WS_SUCCESS"
    local bead
    bead="$(create_bead "$WS_SUCCESS" "E2E: bead-aware launch")"
    [ -n "$bead" ] || { log_fail "test bead creation"; return; }

    local session="${TEST_SESSION_PREFIX}-s1-${RUN_ID}"
    run_launcher_tmux "$session" "$PROD_LAUNCHER" "$WS_SUCCESS" "beadref-worker-1" "$bead"

    wait_for 20 "launcher JSON captured as valid JSON" \
        bash -c "jq empty '$TEST_SESSION_DIR/beadref-worker-1.json' >/dev/null 2>&1"

    local bead_id_in_json status current_task
    bead_id_in_json="$(jq -r '.bead_id // empty' "$TEST_SESSION_DIR/beadref-worker-1.json" 2>/dev/null)"
    assert_eq "$bead_id_in_json" "$bead" "launcher JSON reports bead_id"

    # in_progress with the session as assignee (protocol §4)
    wait_for 20 "bead is in_progress" \
        bash -c "[ \"\$(bead_field '$WS_SUCCESS' '$bead' 'status')\" = 'in_progress' ]"
    assert_eq "$(bead_field "$WS_SUCCESS" "$bead" 'assignee')" "beadref-worker-1" \
        "bead assignee is the worker session"

    # Status file carries the bead as current_task (ADR 0005 string form)
    status="$TEST_HOME/.forge/status/beadref-worker-1.json"
    wait_for 10 "status file created with valid JSON" \
        bash -c "jq empty '$status' >/dev/null 2>&1"
    current_task="$(jq -r '.current_task // empty' "$status" 2>/dev/null)"
    assert_eq "$current_task" "$bead" "status file current_task is the bead id"

    # The worker session exists and shows the injected prompt
    track_session "beadref-worker-1"
    wait_for 15 "worker tmux session running" tmux has-session -t "beadref-worker-1"
    sleep 1
    # -S -200: the prompt scrolls above the visible pane on an 80x24 session
    if tmux capture-pane -t "beadref-worker-1" -p -S -200 2>/dev/null | grep -q "E2E: bead-aware launch"; then
        log_pass "worker pane shows the injected bead prompt"
    else
        log_fail "worker pane shows the injected bead prompt"
    fi

    echo "$bead" > "$TEST_SESSION_DIR/s1-bead"
}

# ==============================================================================
# Scenario 2: Success completion — monitor closes the bead via the CLI
# ==============================================================================
scenario_success_completion() {
    log_info "Scenario 2: clean worker exit closes the bead"

    local bead
    bead="$(cat "$TEST_SESSION_DIR/s1-bead")"

    # Simulate a clean worker exit by writing the exit-code marker the pane
    # would write on normal completion. (Typing into the pane cannot work:
    # its shell is blocked inside `sleep 300`, so input queues unread, and
    # Ctrl-C would abort the whole pane script — that is the failure path,
    # covered by scenario 3.)
    echo 0 > "$TEST_HOME/.forge/status/beadref-worker-1.exit"

    # The monitor must close the bead while the worker session is still up —
    # proving the transition came from the marker, not session death
    wait_for 40 "bead closed on success" \
        bash -c "[ \"\$(bead_field '$WS_SUCCESS' '$bead' 'status')\" = 'closed' ]"
    if tmux has-session -t "beadref-worker-1" 2>/dev/null; then
        log_pass "bead closed while worker session was still alive"
    else
        log_fail "bead closed while worker session was still alive"
    fi

    # The close reason is not projected by `bead show --json`; read it from
    # the captured close invocation instead.
    close_reason="$(awk -F' \\| ' -v bead="$bead" \
        '$1 == "close" && $2 == bead { for (i = 3; i < NF; i++) if ($i == "--reason") { print $(i + 1); break } }' \
        "$BEAD_CALL_LOG" | tail -1)"
    assert_eq "$close_reason" "Completed by beadref-worker-1" \
        "close reason records the completing worker"
}

# ==============================================================================
# Scenario 3: Failure completion — monitor releases the bead
# ==============================================================================
scenario_failure_completion() {
    log_info "Scenario 3: worker death without a clean exit releases the bead"

    init_store "$WS_FAILURE"
    local bead
    bead="$(create_bead "$WS_FAILURE" "E2E: failure release path")"

    local session="${TEST_SESSION_PREFIX}-s3-${RUN_ID}"
    run_launcher_tmux "$session" "$PROD_LAUNCHER" "$WS_FAILURE" "beadref-worker-3" "$bead"
    track_session "beadref-worker-3"

    wait_for 20 "bead is in_progress" \
        bash -c "[ \"\$(bead_field '$WS_FAILURE' '$bead' 'status')\" = 'in_progress' ]"

    # Kill the worker session: no exit marker -> the monitor must release
    tmux kill-session -t "beadref-worker-3" 2>/dev/null || true

    wait_for 40 "bead released to open on failure" \
        bash -c "[ \"\$(bead_field '$WS_FAILURE' '$bead' 'status')\" = 'open' ]"
    assert_eq "$(bead_field "$WS_FAILURE" "$bead" 'assignee')" "" \
        "released bead has no assignee"
}

# ==============================================================================
# Scenario 4: Standard mode — no --bead-ref touches no bead state
# ==============================================================================
scenario_standard_mode() {
    log_info "Scenario 4: standard mode (no --bead-ref)"

    local session="${TEST_SESSION_PREFIX}-s4-${RUN_ID}"
    run_launcher_tmux "$session" "$PROD_LAUNCHER" "$WS_STANDARD" "beadref-worker-4"
    track_session "beadref-worker-4"

    wait_for 20 "launcher JSON captured as valid JSON" \
        bash -c "jq empty '$TEST_SESSION_DIR/beadref-worker-4.json' >/dev/null 2>&1"

    assert_eq "$(jq -r '.bead_id // empty' "$TEST_SESSION_DIR/beadref-worker-4.json" 2>/dev/null)" "" \
        "standard-mode JSON has no bead_id"

    local status="$TEST_HOME/.forge/status/beadref-worker-4.json"
    wait_for 10 "status file created with valid JSON" \
        bash -c "jq empty '$status' >/dev/null 2>&1"
    assert_eq "$(jq -r '.current_task // empty' "$status" 2>/dev/null)" "" \
        "standard-mode status file has no current_task"
    wait_for 15 "worker tmux session running" tmux has-session -t "beadref-worker-4"
}

# ==============================================================================
# Scenario 5: Example launcher — finite worker drives close end to end
# ==============================================================================
scenario_example_launcher() {
    log_info "Scenario 5: example launcher closes the bead when its worker finishes"

    init_store "$WS_EXAMPLE"
    local bead
    bead="$(create_bead "$WS_EXAMPLE" "E2E: example launcher completion")"

    # The example launcher spawns its own (non-tmux) worker; run it directly
    # with the isolated HOME. Its simulated worker exits after ~10s and the
    # monitor then closes the bead.
    HOME="$TEST_HOME" FORGE_BEAD_CLI="$BEAD_CLI" "$EXAMPLE_LAUNCHER" \
        --model=sonnet --workspace="$WS_EXAMPLE" \
        --session-name="beadref-worker-5" --bead-ref="$bead" \
        > "$TEST_SESSION_DIR/beadref-worker-5.json" 2>/dev/null

    assert_eq "$(jq -r '.bead_ref // empty' "$TEST_SESSION_DIR/beadref-worker-5.json" 2>/dev/null)" "$bead" \
        "example launcher JSON reports bead_ref"

    # ~10s simulated work + up to 2s monitor poll + close
    wait_for 45 "example launcher bead closed on completion" \
        bash -c "[ \"\$(bead_field '$WS_EXAMPLE' '$bead' 'status')\" = 'closed' ]"

    # No orphaned monitor/worker processes from this scenario
    sleep 3
    if pgrep -f "beadref-worker-5" >/dev/null 2>&1; then
        log_fail "no leftover processes for beadref-worker-5"
    else
        log_pass "no leftover processes for beadref-worker-5"
    fi
}

# ==============================================================================
# Main — everything runs inside one dedicated, tracked tmux session (ADR 0017)
# ==============================================================================
main() {
    [ -x "$PROD_LAUNCHER" ] || { echo "launcher not executable: $PROD_LAUNCHER" >&2; exit 1; }
    [ -x "$EXAMPLE_LAUNCHER" ] || { echo "launcher not executable: $EXAMPLE_LAUNCHER" >&2; exit 1; }
    command -v "$BEAD_CLI" >/dev/null 2>&1 || { echo "bead CLI not found: $BEAD_CLI" >&2; exit 1; }
    command -v jq >/dev/null 2>&1 || { echo "jq not found" >&2; exit 1; }

    : > "$TEST_SESSION_DIR/sessions"

    scenario_bead_aware_launch
    scenario_success_completion
    scenario_failure_completion
    scenario_standard_mode
    scenario_example_launcher

    echo
    echo "=============================================="
    echo " Results: $PASS passed, $FAIL failed"
    echo "=============================================="
    [ "$FAIL" -eq 0 ]
}

# Run the scenarios in a tmux session so the whole verification is
# tmux-based per ADR 0017, and stream its output here.
if [ "${1:-}" = "--inner" ]; then
    main
    exit $?
fi

INNER_LOG="$TEST_SESSION_DIR/inner.log"
tmux new-session -d -s "$TEST_SESSION" \
    "'$0' --inner > '$INNER_LOG' 2>&1; echo EXIT:\$? >> '$INNER_LOG'"
track_session "$TEST_SESSION"

echo "Verification running in tmux session: $TEST_SESSION"
while tmux has-session -t "$TEST_SESSION" 2>/dev/null; do
    sleep 2
done
tail -40 "$INNER_LOG"

# Exit with the inner run's status
if grep -q "EXIT:0" "$INNER_LOG"; then
    exit 0
fi
exit 1
