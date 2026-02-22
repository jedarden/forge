#!/usr/bin/env bash
# Update forge binary from local source
#
# NOTE: This script should only be used when forge is NOT running.
# If forge is running, use Ctrl+U within the TUI to trigger the self-update
# mechanism which handles the "Text file busy" issue properly.
#
# This script is for:
# - Manual updates when forge is not running
# - CI/CD deployment
# - Development updates

set -e

FORGE_SRC="${FORGE_SRC:-/home/coder/forge}"
FORGE_BIN="${FORGE_BIN:-$HOME/.cargo/bin/forge}"

echo "🔨 Building forge..."
cd "$FORGE_SRC"
cargo build --release 2>&1 | tail -10

echo "📦 Installing to $FORGE_BIN..."
cp target/release/forge "$FORGE_BIN"

echo "✅ Forge updated successfully!"
echo "   Run 'forge' to start the updated version."
