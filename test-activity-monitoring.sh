#!/bin/bash
# Test script for activity monitoring feature
# This script verifies that the 15-minute activity threshold works correctly

set -euo pipefail

echo "🧪 Activity Monitoring Test Script"
echo "=================================="
echo ""

# Create test directories
mkdir -p ~/.forge/status ~/.forge/logs

# Test 1: Create a mock worker with recent activity (should be healthy)
echo "📝 Test 1: Worker with recent activity (should be healthy)"
WORKER_ID="test-worker-recent"
STATUS_FILE=~/.forge/status/${WORKER_ID}.json

cat > "$STATUS_FILE" << EOF
{
  "worker_id": "$WORKER_ID",
  "status": "active",
  "model": "sonnet",
  "workspace": "/home/coder/forge",
  "pid": 12345,
  "started_at": "$(date -Iseconds -d '30 minutes ago')",
  "last_activity": "$(date -Iseconds -d '5 minutes ago')",
  "current_task": "fg-test",
  "tasks_completed": 1
}
EOF

echo "✅ Created status file for $WORKER_ID with activity 5 minutes ago"
echo ""

# Test 2: Create a mock worker with stale activity (should trigger WorkerStale)
echo "📝 Test 2: Worker with stale activity (should trigger WorkerStale alert)"
WORKER_ID_STALE="test-worker-stale"
STATUS_FILE_STALE=~/.forge/status/${WORKER_ID_STALE}.json

cat > "$STATUS_FILE_STALE" << EOF
{
  "worker_id": "$WORKER_ID_STALE",
  "status": "idle",
  "model": "haiku",
  "workspace": "/home/coder/forge",
  "pid": 12346,
  "started_at": "$(date -Iseconds -d '60 minutes ago')",
  "last_activity": "$(date -Iseconds -d '20 minutes ago')",
  "current_task": null,
  "tasks_completed": 0
}
EOF

echo "✅ Created status file for $WORKER_ID_STALE with activity 20 minutes ago"
echo ""

# Test 3: Create a mock worker stuck on a task (should trigger TaskStuck)
echo "📝 Test 3: Worker stuck on task (should trigger TaskStuck alert)"
WORKER_ID_STUCK="test-worker-stuck"
STATUS_FILE_STUCK=~/.forge/status/${WORKER_ID_STUCK}.json

cat > "$STATUS_FILE_STUCK" << EOF
{
  "worker_id": "$WORKER_ID_STUCK",
  "status": "active",
  "model": "opus",
  "workspace": "/home/coder/forge",
  "pid": 12347,
  "started_at": "$(date -Iseconds -d '90 minutes ago')",
  "last_activity": "$(date -Iseconds -d '45 minutes ago')",
  "current_task": "fg-stuck-task",
  "tasks_completed": 0
}
EOF

echo "✅ Created status file for $WORKER_ID_STUCK with activity 45 minutes ago"
echo ""

echo "📊 Test Setup Complete"
echo "====================="
echo ""
echo "Status files created:"
echo "  1. $WORKER_ID (healthy) - last activity 5 min ago"
echo "  2. $WORKER_ID_STALE (stale) - last activity 20 min ago"
echo "  3. $WORKER_ID_STUCK (stuck) - last activity 45 min ago, active on task"
echo ""
echo "🚀 Launch FORGE to verify:"
echo "   1. $WORKER_ID should show healthy (●)"
echo "   2. $WORKER_ID_STALE should show WorkerStale alert (◐ or ○)"
echo "   3. $WORKER_ID_STUCK should show TaskStuck alert (◐ or ○)"
echo ""
echo "To launch FORGE in tmux:"
echo "  tmux new-session -s forge-activity-test"
echo "  cd /home/coder/forge"
echo "  ./target/release/forge"
echo ""
echo "Then press 'o' for Overview to see alerts"
echo "Press 'w' for Workers to see health indicators"
echo ""
echo "To clean up test workers:"
echo "  rm ~/.forge/status/test-worker-*.json"
echo ""
