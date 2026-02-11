// Forge CI/CD pipeline using Dagger
//
// This module provides build, test, and lint functions for the forge Rust project.
// Can be run locally with `dagger call` or from CI systems.
//
// Version Management:
//   The release command auto-increments the patch version if Cargo.toml was not
//   changed in the current commit. If you manually bump the version, it uses that.
//
// Examples:
//   dagger call fmt --source=.
//   dagger call clippy --source=.
//   dagger call test --source=.
//   dagger call build --source=.
//   dagger call ci --source=.  # runs all checks
//   dagger call version --source=.
//   dagger call next-version --source=.  # shows version that will be released
//   dagger call version-changed --source=.  # check if version was bumped in commit
//   dagger call release --source=. --github-token=env:GITHUB_TOKEN

package main

import (
	"context"
	"fmt"
	"regexp"
	"strings"

	"dagger/forge/internal/dagger"
)

type Forge struct{}

// Base Rust container with required tools
func (m *Forge) rustContainer(source *dagger.Directory) *dagger.Container {
	return dag.Container().
		From("rust:1.83-slim").
		WithWorkdir("/app").
		WithMountedDirectory("/app", source).
		WithMountedCache("/root/.cargo/registry", dag.CacheVolume("cargo-registry")).
		WithMountedCache("/root/.cargo/git", dag.CacheVolume("cargo-git")).
		WithMountedCache("/app/target", dag.CacheVolume("forge-target")).
		WithEnvVariable("CARGO_HOME", "/root/.cargo").
		WithEnvVariable("CARGO_TERM_COLOR", "always").
		WithEnvVariable("RUST_BACKTRACE", "1").
		WithExec([]string{"rustup", "component", "add", "rustfmt", "clippy"})
}

// Fmt checks code formatting
func (m *Forge) Fmt(ctx context.Context, source *dagger.Directory) (string, error) {
	return m.rustContainer(source).
		WithExec([]string{"cargo", "fmt", "--all", "--", "--check"}).
		Stdout(ctx)
}

// Clippy runs linting checks
func (m *Forge) Clippy(ctx context.Context, source *dagger.Directory) (string, error) {
	return m.rustContainer(source).
		WithExec([]string{"cargo", "clippy", "--all-targets", "--all-features", "--", "-D", "warnings"}).
		Stdout(ctx)
}

// Check verifies that code compiles
func (m *Forge) Check(ctx context.Context, source *dagger.Directory) (string, error) {
	return m.rustContainer(source).
		WithExec([]string{"cargo", "check", "--all-targets", "--all-features"}).
		Stdout(ctx)
}

// Test runs all tests
func (m *Forge) Test(ctx context.Context, source *dagger.Directory) (string, error) {
	return m.rustContainer(source).
		WithEnvVariable("CI", "true").
		WithExec([]string{"cargo", "test", "--all-features"}).
		Stdout(ctx)
}

// Build compiles the release binary
func (m *Forge) Build(ctx context.Context, source *dagger.Directory) *dagger.File {
	return m.rustContainer(source).
		WithExec([]string{"cargo", "build", "--release"}).
		File("/app/target/release/forge")
}

// BuildContainer creates a minimal container with the forge binary
func (m *Forge) BuildContainer(ctx context.Context, source *dagger.Directory) *dagger.Container {
	binary := m.Build(ctx, source)

	return dag.Container().
		From("debian:bookworm-slim").
		WithExec([]string{"apt-get", "update"}).
		WithExec([]string{"apt-get", "install", "-y", "ca-certificates", "tmux"}).
		WithExec([]string{"rm", "-rf", "/var/lib/apt/lists/*"}).
		WithFile("/usr/local/bin/forge", binary).
		WithEntrypoint([]string{"/usr/local/bin/forge"})
}

// Ci runs the full CI pipeline (fmt, clippy, check, test, build)
func (m *Forge) Ci(ctx context.Context, source *dagger.Directory) (string, error) {
	// Run fmt, clippy, and check in parallel
	fmtCh := make(chan error, 1)
	clippyCh := make(chan error, 1)
	checkCh := make(chan error, 1)

	go func() {
		_, err := m.Fmt(ctx, source)
		fmtCh <- err
	}()

	go func() {
		_, err := m.Clippy(ctx, source)
		clippyCh <- err
	}()

	go func() {
		_, err := m.Check(ctx, source)
		checkCh <- err
	}()

	// Wait for parallel checks
	if err := <-fmtCh; err != nil {
		return "", err
	}
	if err := <-clippyCh; err != nil {
		return "", err
	}
	if err := <-checkCh; err != nil {
		return "", err
	}

	// Run tests after checks pass
	if _, err := m.Test(ctx, source); err != nil {
		return "", err
	}

	// Build release binary
	_ = m.Build(ctx, source)

	return "CI pipeline completed successfully", nil
}

// Publish builds and pushes the container image to a registry
func (m *Forge) Publish(
	ctx context.Context,
	source *dagger.Directory,
	// Container registry (e.g., "docker.io/username")
	registry string,
	// Image tag
	tag string,
	// Registry username
	username string,
	// Registry password secret
	password *dagger.Secret,
) (string, error) {
	container := m.BuildContainer(ctx, source)

	addr, err := container.
		WithRegistryAuth(registry, username, password).
		Publish(ctx, registry+"/forge:"+tag)
	if err != nil {
		return "", err
	}

	return addr, nil
}

// Version extracts the semver version from Cargo.toml workspace
func (m *Forge) Version(ctx context.Context, source *dagger.Directory) (string, error) {
	// Read Cargo.toml and extract workspace version
	cargoToml, err := source.File("Cargo.toml").Contents(ctx)
	if err != nil {
		return "", fmt.Errorf("failed to read Cargo.toml: %w", err)
	}

	// Match workspace version: version = "x.y.z"
	re := regexp.MustCompile(`\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"`)
	matches := re.FindStringSubmatch(cargoToml)
	if len(matches) < 2 {
		// Fallback: try package version directly
		re = regexp.MustCompile(`(?m)^version\s*=\s*"([^"]+)"`)
		matches = re.FindStringSubmatch(cargoToml)
		if len(matches) < 2 {
			return "", fmt.Errorf("failed to extract version from Cargo.toml")
		}
	}

	return matches[1], nil
}

// crossContainer creates a Rust container with cross-compilation support
func (m *Forge) crossContainer(source *dagger.Directory, target string) *dagger.Container {
	container := dag.Container().
		From("rust:1.83-slim").
		WithWorkdir("/app").
		WithMountedDirectory("/app", source).
		WithMountedCache("/root/.cargo/registry", dag.CacheVolume("cargo-registry")).
		WithMountedCache("/root/.cargo/git", dag.CacheVolume("cargo-git")).
		WithMountedCache("/app/target", dag.CacheVolume("forge-target-"+target)).
		WithEnvVariable("CARGO_HOME", "/root/.cargo").
		WithEnvVariable("CARGO_TERM_COLOR", "always").
		WithEnvVariable("RUST_BACKTRACE", "1")

	// Install target-specific toolchains
	switch target {
	case "x86_64-unknown-linux-gnu":
		// Default target, no extra setup needed
		container = container.
			WithExec([]string{"rustup", "target", "add", target})
	case "aarch64-unknown-linux-gnu":
		container = container.
			WithExec([]string{"apt-get", "update"}).
			WithExec([]string{"apt-get", "install", "-y", "gcc-aarch64-linux-gnu"}).
			WithExec([]string{"rustup", "target", "add", target}).
			WithEnvVariable("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER", "aarch64-linux-gnu-gcc")
	case "x86_64-apple-darwin", "aarch64-apple-darwin":
		// macOS cross-compilation requires osxcross or building on macOS
		// For now, we'll skip these in Dagger and rely on GitHub Actions matrix
		container = container.
			WithExec([]string{"rustup", "target", "add", target})
	}

	return container
}

// BuildRelease builds the release binary for a specific target
func (m *Forge) BuildRelease(
	ctx context.Context,
	source *dagger.Directory,
	// Target triple (e.g., x86_64-unknown-linux-gnu)
	// +optional
	// +default="x86_64-unknown-linux-gnu"
	target string,
) *dagger.File {
	if target == "" {
		target = "x86_64-unknown-linux-gnu"
	}

	container := m.crossContainer(source, target)

	// Build for target
	container = container.WithExec([]string{
		"cargo", "build", "--release", "--target", target,
	})

	return container.File("/app/target/" + target + "/release/forge")
}

// BuildAllTargets builds release binaries for all supported targets
func (m *Forge) BuildAllTargets(ctx context.Context, source *dagger.Directory) *dagger.Directory {
	targets := []string{
		"x86_64-unknown-linux-gnu",
		"aarch64-unknown-linux-gnu",
	}

	outputDir := dag.Directory()

	for _, target := range targets {
		binary := m.BuildRelease(ctx, source, target)

		// Name binary with target suffix
		parts := strings.Split(target, "-")
		arch := parts[0]
		os := "linux"
		if strings.Contains(target, "apple") {
			os = "darwin"
		}

		filename := fmt.Sprintf("forge-%s-%s", os, arch)
		outputDir = outputDir.WithFile(filename, binary)
	}

	return outputDir
}

// VersionChanged checks if the version in Cargo.toml was changed in the current commit
func (m *Forge) VersionChanged(
	ctx context.Context,
	source *dagger.Directory,
) (bool, error) {
	// Use git to check if Cargo.toml version changed from HEAD~1
	container := dag.Container().
		From("alpine/git:latest").
		WithMountedDirectory("/repo", source).
		WithWorkdir("/repo")

	// Get current version from Cargo.toml
	currentVersion, err := m.Version(ctx, source)
	if err != nil {
		return false, fmt.Errorf("failed to get current version: %w", err)
	}

	// Get previous version from git (HEAD~1)
	// Extract version from previous commit's Cargo.toml
	prevCargoToml, err := container.
		WithExec([]string{"git", "show", "HEAD~1:Cargo.toml"}).
		Stdout(ctx)

	if err != nil {
		// No previous commit or file didn't exist - treat as changed
		return true, nil
	}

	// Extract version from previous Cargo.toml
	re := regexp.MustCompile(`\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"`)
	matches := re.FindStringSubmatch(prevCargoToml)
	if len(matches) < 2 {
		// Fallback: try package version directly
		re = regexp.MustCompile(`(?m)^version\s*=\s*"([^"]+)"`)
		matches = re.FindStringSubmatch(prevCargoToml)
		if len(matches) < 2 {
			// Couldn't parse previous version - treat as changed
			return true, nil
		}
	}
	prevVersion := matches[1]

	// Compare versions
	return currentVersion != prevVersion, nil
}

// NextVersion returns the version to use for release
// If version was changed in commit, uses that version
// If version was NOT changed, auto-increments patch version
func (m *Forge) NextVersion(
	ctx context.Context,
	source *dagger.Directory,
) (string, error) {
	// Get current version from Cargo.toml
	version, err := m.Version(ctx, source)
	if err != nil {
		return "", fmt.Errorf("failed to get version: %w", err)
	}

	// Check if version was changed in this commit
	changed, err := m.VersionChanged(ctx, source)
	if err != nil {
		// If we can't determine, default to using current version
		return version, nil
	}

	if changed {
		// Version was intentionally changed, use as-is
		return version, nil
	}

	// Version was not changed, auto-increment patch
	return incrementVersion(version), nil
}

// incrementVersion bumps the patch version of a semver string
func incrementVersion(version string) string {
	// Remove v prefix if present
	v := strings.TrimPrefix(version, "v")
	parts := strings.Split(v, ".")
	if len(parts) != 3 {
		return version
	}

	var major, minor, patch int
	fmt.Sscanf(parts[0], "%d", &major)
	fmt.Sscanf(parts[1], "%d", &minor)
	fmt.Sscanf(parts[2], "%d", &patch)

	return fmt.Sprintf("%d.%d.%d", major, minor, patch+1)
}

// Release creates a GitHub release with built binaries
// Auto-increments patch version if Cargo.toml version was not changed in commit
func (m *Forge) Release(
	ctx context.Context,
	source *dagger.Directory,
	// GitHub token with repo permissions
	githubToken *dagger.Secret,
	// Override version (optional, defaults to auto-detected version)
	// +optional
	version string,
	// Mark as draft release
	// +optional
	// +default=false
	draft bool,
	// Mark as prerelease
	// +optional
	// +default=false
	prerelease bool,
	// Use exact Cargo.toml version without auto-increment logic
	// +optional
	// +default=false
	strict bool,
) (string, error) {
	var originalVersion string
	var err error

	// Get the version to use
	if version != "" {
		// Explicit version provided, use it
		originalVersion = version
	} else if strict {
		// Strict mode: use exact Cargo.toml version
		version, err = m.Version(ctx, source)
		if err != nil {
			return "", fmt.Errorf("failed to get version: %w", err)
		}
		originalVersion = version
	} else {
		// Auto mode: check if version was changed in commit
		originalVersion, err = m.Version(ctx, source)
		if err != nil {
			return "", fmt.Errorf("failed to get version: %w", err)
		}

		version, err = m.NextVersion(ctx, source)
		if err != nil {
			return "", fmt.Errorf("failed to determine next version: %w", err)
		}
	}

	// Ensure version has v prefix for git tag
	tag := version
	if !strings.HasPrefix(tag, "v") {
		tag = "v" + tag
	}

	// Log if version was auto-incremented
	versionNote := ""
	if version != originalVersion {
		versionNote = fmt.Sprintf(" (auto-incremented from %s)", originalVersion)
	}

	// Build all target binaries
	binaries := m.BuildAllTargets(ctx, source)

	// Create release using gh CLI
	releaseContainer := dag.Container().
		From("ghcr.io/cli/cli:latest").
		WithSecretVariable("GITHUB_TOKEN", githubToken).
		WithWorkdir("/release").
		WithMountedDirectory("/release/binaries", binaries).
		WithMountedDirectory("/release/source", source)

	// Build gh release command
	releaseCmd := []string{
		"gh", "release", "create", tag,
		"--repo", "jedarden/forge",
		"--title", fmt.Sprintf("Forge %s", tag),
		"--generate-notes",
	}

	if draft {
		releaseCmd = append(releaseCmd, "--draft")
	}
	if prerelease {
		releaseCmd = append(releaseCmd, "--prerelease")
	}

	// Add binary files
	releaseCmd = append(releaseCmd, "/release/binaries/*")

	// Execute release creation
	output, err := releaseContainer.
		WithExec(releaseCmd).
		Stdout(ctx)
	if err != nil {
		return "", fmt.Errorf("failed to create release: %w", err)
	}

	return fmt.Sprintf("Created release %s%s\n%s", tag, versionNote, output), nil
}

// BumpVersion increments the version in Cargo.toml
func (m *Forge) BumpVersion(
	ctx context.Context,
	source *dagger.Directory,
	// Version bump type: major, minor, or patch
	// +default="patch"
	bump string,
) (*dagger.Directory, error) {
	if bump == "" {
		bump = "patch"
	}

	// Get current version
	currentVersion, err := m.Version(ctx, source)
	if err != nil {
		return nil, fmt.Errorf("failed to get current version: %w", err)
	}

	// Parse and increment version
	parts := strings.Split(currentVersion, ".")
	if len(parts) != 3 {
		return nil, fmt.Errorf("invalid semver format: %s", currentVersion)
	}

	var major, minor, patch int
	fmt.Sscanf(parts[0], "%d", &major)
	fmt.Sscanf(parts[1], "%d", &minor)
	fmt.Sscanf(parts[2], "%d", &patch)

	switch bump {
	case "major":
		major++
		minor = 0
		patch = 0
	case "minor":
		minor++
		patch = 0
	case "patch":
		patch++
	default:
		return nil, fmt.Errorf("invalid bump type: %s (must be major, minor, or patch)", bump)
	}

	newVersion := fmt.Sprintf("%d.%d.%d", major, minor, patch)

	// Update Cargo.toml using sed in container
	updated := dag.Container().
		From("alpine:latest").
		WithMountedDirectory("/app", source).
		WithWorkdir("/app").
		WithExec([]string{
			"sed", "-i",
			fmt.Sprintf(`s/version = "%s"/version = "%s"/g`, currentVersion, newVersion),
			"Cargo.toml",
		}).
		Directory("/app")

	return updated, nil
}
