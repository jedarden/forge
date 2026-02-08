# FORGE Makefile
# Agent Orchestration Dashboard build and install automation

.PHONY: all build install test clean run help release check fmt lint clippy

# Default target
all: build

# Build the project in debug mode
build:
	cargo build

# Build in release mode (optimized)
release:
	cargo build --release

# Install to ~/.cargo/bin (ensures 'forge' command uses latest built version)
install: release
	@echo "Installing forge to ~/.cargo/bin..."
	cargo install --path .
	@echo "Installation complete! Run 'forge --version' to verify."

# Quick install: skip building if release binary exists
install-quick:
	@if [ -f target/release/forge ]; then \
		echo "Installing from existing release build..."; \
		cargo install --path . --force; \
	else \
		echo "No release binary found, running full build..."; \
		$(MAKE) install; \
	fi

# Run tests
test:
	cargo test

# Run tests with output
test-verbose:
	cargo test -- --nocapture

# Clean build artifacts
clean:
	cargo clean

# Run the application (development)
run: build
	./target/debug/forge

# Run the application (release)
run-release: release
	./target/release/forge

# Check code without building
check:
	cargo check

# Format code
fmt:
	cargo fmt

# Check if code is formatted
fmt-check:
	cargo fmt -- --check

# Run clippy linter
lint:
	cargo clippy -- -D warnings

# Run clippy with suggestions
clippy:
	cargo clippy

# Update dependencies
update:
	cargo update

# Show help
help:
	@echo "FORGE Makefile - Available targets:"
	@echo ""
	@echo "  all          - Build project (debug mode)"
	@echo "  build        - Build project (debug mode)"
	@echo "  release      - Build project (release mode, optimized)"
	@echo "  install      - Build and install to ~/.cargo/bin (recommended)"
	@echo "  install-quick- Quick install using existing release binary"
	@echo "  test         - Run tests"
	@echo "  test-verbose - Run tests with output"
	@echo "  clean        - Remove build artifacts"
	@echo "  run          - Build and run (debug)"
	@echo "  run-release  - Build and run (release)"
	@echo "  check        - Check code without building"
	@echo "  fmt          - Format code"
	@echo "  fmt-check    - Check if code is formatted"
	@echo "  lint         - Run clippy linter (strict)"
	@echo "  clippy       - Run clippy with suggestions"
	@echo "  update       - Update dependencies"
	@echo "  help         - Show this help message"
	@echo ""
	@echo "Version info: $$(cargo metadata --format-version 1 | jq -r '.packages[] | select(.name == "forge") | .version')"

# Show version info
version:
	@cargo metadata --format-version 1 | jq -r '.packages[] | select(.name == "forge") | .version'
