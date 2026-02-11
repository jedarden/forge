// Forge CI/CD pipeline using Dagger
//
// This module provides build, test, and lint functions for the forge Rust project.
// Can be run locally with `dagger call` or from CI systems.
//
// Examples:
//   dagger call fmt --source=.
//   dagger call clippy --source=.
//   dagger call test --source=.
//   dagger call build --source=.
//   dagger call ci --source=.  # runs all checks

package main

import (
	"context"
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
