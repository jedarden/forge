#!/usr/bin/env bash
# Update forge binary
# Can be called externally or via Ctrl+U hotkey within forge

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
