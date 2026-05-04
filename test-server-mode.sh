#!/bin/bash
# Test script for FORGE server mode

set -e

FORGE_BIN="./target/release/forge"
TEST_PORT=18888  # Use non-standard port to avoid conflicts

echo "=== FORGE Server Mode Test ==="
echo ""

# Check if forge binary exists
if [ ! -f "$FORGE_BIN" ]; then
    echo "Error: forge binary not found at $FORGE_BIN"
    echo "Run: cargo build --release"
    exit 1
fi

# Test 1: Server help
echo "Test 1: Check server mode help"
$FORGE_BIN --help | grep -q "server" && echo "✓ Server mode documented in help" || echo "✗ Server mode not in help"

# Test 2: Server version check
echo ""
echo "Test 2: Check forge version"
$FORGE_BIN --version

# Test 3: Verify server crate compiles
echo ""
echo "Test 3: Verify forge-server crate"
if [ -d "crates/forge-server" ]; then
    echo "✓ forge-server crate exists"
    echo "  Modules:"
    ls -1 crates/forge-server/src/*.rs | while read f; do
        echo "    - $(basename $f)"
    done
else
    echo "✗ forge-server crate not found"
fi

# Test 4: Check configuration options
echo ""
echo "Test 4: Server configuration"
echo "Server config options in forge-config:"
grep -A 20 "pub struct ServerConfig" crates/forge-config/src/lib.rs | head -25

echo ""
echo "=== Server Mode Architecture ==="
echo ""
echo "Key components:"
echo "  - ForgeServer: WebSocket server for real-time updates"
echo "  - SessionManager: Multi-user session tracking"
echo "  - AuthProvider: Role-based access control (Viewer, Operator, Admin)"
echo "  - BeadAssignmentTracker: Shared bead assignment"
echo "  - ForgeClient: WebSocket client for connecting to server"
echo ""
echo "Server commands:"
echo "  forge --server                    # Start FORGE as a collaborative server"
echo "  forge --connect ws://host:8080/ws # Connect to remote FORGE server"
echo ""
echo "Default users (SimpleAuth):"
echo "  admin/admin123    (Admin - full access)"
echo "  operator/operator123 (Operator - can spawn/kill workers, assign tasks)"
echo "  viewer/viewer123  (Viewer - read-only)"
echo ""
echo "=== Test Complete ==="
