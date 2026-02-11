#!/usr/bin/env bash
# Version bump automation for Cargo.toml workspace package
# Usage: ./scripts/bump-version.sh [major|minor|patch|<version>]
#
# Examples:
#   ./scripts/bump-version.sh patch     # 0.1.4 -> 0.1.5
#   ./scripts/bump-version.sh minor     # 0.1.4 -> 0.2.0
#   ./scripts/bump-version.sh major     # 0.1.4 -> 1.0.0
#   ./scripts/bump-version.sh 2.0.0     # 0.1.4 -> 2.0.0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CARGO_TOML="$REPO_ROOT/Cargo.toml"

usage() {
    echo "Usage: $0 [major|minor|patch|<version>]"
    echo ""
    echo "Bump types:"
    echo "  major     Increment major version (X.0.0)"
    echo "  minor     Increment minor version (0.X.0)"
    echo "  patch     Increment patch version (0.0.X)"
    echo "  <version> Set explicit version (e.g., 2.0.0)"
    echo ""
    echo "If no argument provided, defaults to 'patch'"
    exit 1
}

# Get current version from workspace.package section
get_current_version() {
    grep -A 10 '^\[workspace\.package\]' "$CARGO_TOML" | \
        grep '^version' | \
        head -1 | \
        sed 's/.*= *"\([^"]*\)".*/\1/'
}

# Validate version format (semver: X.Y.Z)
validate_version() {
    local version="$1"
    if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        echo "Error: Invalid version format '$version'. Expected X.Y.Z (e.g., 1.2.3)" >&2
        exit 1
    fi
}

# Calculate new version based on bump type
calculate_new_version() {
    local current="$1"
    local bump_type="$2"

    local major minor patch
    IFS='.' read -r major minor patch <<< "$current"

    case "$bump_type" in
        major)
            echo "$((major + 1)).0.0"
            ;;
        minor)
            echo "$major.$((minor + 1)).0"
            ;;
        patch)
            echo "$major.$minor.$((patch + 1))"
            ;;
        *)
            # Assume it's an explicit version
            validate_version "$bump_type"
            echo "$bump_type"
            ;;
    esac
}

# Update version in Cargo.toml
update_cargo_toml() {
    local old_version="$1"
    local new_version="$2"

    # Use sed to replace version in workspace.package section
    # Match: version = "X.Y.Z" (with possible whitespace variations)
    sed -i "s/^\(version *= *\"\)${old_version}\"/\1${new_version}\"/" "$CARGO_TOML"

    # Verify the change was made
    local updated_version
    updated_version=$(get_current_version)
    if [[ "$updated_version" != "$new_version" ]]; then
        echo "Error: Version update failed. Expected '$new_version', got '$updated_version'" >&2
        exit 1
    fi
}

main() {
    local bump_type="${1:-patch}"

    if [[ "$bump_type" == "-h" || "$bump_type" == "--help" ]]; then
        usage
    fi

    # Ensure Cargo.toml exists
    if [[ ! -f "$CARGO_TOML" ]]; then
        echo "Error: Cargo.toml not found at $CARGO_TOML" >&2
        exit 1
    fi

    # Get current version
    local current_version
    current_version=$(get_current_version)
    if [[ -z "$current_version" ]]; then
        echo "Error: Could not find version in [workspace.package] section" >&2
        exit 1
    fi

    # Calculate new version
    local new_version
    new_version=$(calculate_new_version "$current_version" "$bump_type")

    # Show what we're doing
    echo "Bumping version: $current_version -> $new_version"

    # Update Cargo.toml
    update_cargo_toml "$current_version" "$new_version"

    echo "Updated $CARGO_TOML"
    echo ""
    echo "Next steps:"
    echo "  1. Review changes: git diff Cargo.toml"
    echo "  2. Update Cargo.lock: cargo check"
    echo "  3. Commit: git add Cargo.toml Cargo.lock && git commit -m \"chore: bump version to $new_version\""
}

main "$@"
