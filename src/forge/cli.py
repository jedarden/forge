"""
FORGE CLI Entry Point

Provides commands for managing FORGE configuration and launching the dashboard.
"""

import sys
from pathlib import Path

import click

from forge.app import ForgeApp
from forge.config import (
    ConfigError,
    ConfigLoader,
    ConfigValidationError,
    init_default_config,
    get_config,
    get_config_value,
    set_config_value,
)
from forge.setup import check_first_run, run_interactive_setup


@click.group(invoke_without_command=True)
@click.version_option(version="0.1.0", prog_name="forge")
@click.pass_context
def cli(ctx: click.Context) -> None:
    """FORGE - Federated Orchestration & Resource Generation Engine

    Terminal-based AI agent control panel for managing workers, tasks, and costs.

    Run 'forge' to launch the dashboard.
    """
    # If no subcommand provided, launch dashboard
    if ctx.invoked_subcommand is None:
        ctx.invoke(dashboard)


@cli.command()
@click.option(
    "--output",
    "-o",
    type=click.Path(path_type=Path),
    help="Output path for config file (defaults to ~/.forge/config.yaml)",
)
@click.option(
    "--force",
    "-f",
    is_flag=True,
    help="Force re-initialization (runs setup wizard even if config exists)",
)
def init(output: Path | None, force: bool) -> None:
    """Initialize FORGE configuration.

    By default, runs the interactive setup wizard.
    Use --force to re-run setup even if config exists.
    """
    if force or check_first_run():
        # Run interactive setup
        setup_success = run_interactive_setup()
        if not setup_success:
            click.echo()
            click.echo("Setup incomplete.")
            sys.exit(1)
    else:
        # Config already exists
        config_path = Path.home() / ".forge" / "config.yaml"
        click.secho(f"✓ Configuration already exists at: {config_path}", fg="green")
        click.echo()
        click.echo("To reconfigure, use:")
        click.echo("  forge init --force")


@cli.command()
@click.option(
    "--config",
    "-c",
    type=click.Path(exists=True, path_type=Path),
    help="Path to config file to validate",
)
@click.option(
    "--workspace",
    "-w",
    type=click.Path(exists=True, path_type=Path),
    help="Workspace path for override validation",
)
def validate(config: Path | None, workspace: Path | None) -> None:
    """Validate FORGE configuration file.

    Checks that the configuration file is valid YAML and conforms to the schema.
    """
    try:
        loader = ConfigLoader(
            user_config_path=config,
            workspace_path=workspace,
        )
        loaded_config = loader.load()
        click.secho("✓ Configuration is valid", fg="green")

        # Display some key settings
        click.echo("\nKey settings:")
        click.echo(f"  Chat backend: {loaded_config.chat_backend.command}")
        click.echo(f"  Dashboard refresh: {loaded_config.dashboard.refresh_interval_ms}ms")
        click.echo(f"  Cost tracking: {'enabled' if loaded_config.cost_tracking.enabled else 'disabled'}")
        click.echo(f"  Log format: {loaded_config.log_collection.format}")

    except ConfigValidationError as e:
        click.secho(f"✗ Configuration validation error: {e}", fg="red")
        sys.exit(1)
    except ConfigError as e:
        click.secho(f"✗ Configuration error: {e}", fg="red")
        sys.exit(1)
    except Exception as e:
        click.secho(f"✗ Unexpected error: {e}", fg="red")
        sys.exit(1)


@cli.command()
@click.argument("path", type=str)
@click.option(
    "--workspace",
    "-w",
    type=click.Path(exists=True, path_type=Path),
    help="Workspace path for context",
)
def get(path: str, workspace: Path | None) -> None:
    """Get a configuration value by path.

    Examples: forge get dashboard.refresh_interval_ms
    """
    try:
        value = get_config_value(path, workspace_path=workspace)
        if isinstance(value, (dict, list)):
            import json
            click.echo(json.dumps(value, indent=2))
        else:
            click.echo(str(value))
    except ConfigError as e:
        click.secho(f"✗ Error: {e}", fg="red")
        sys.exit(1)


@cli.command()
@click.argument("path", type=str)
@click.argument("value", type=str)
@click.option(
    "--workspace",
    "-w",
    type=click.Path(exists=True, path_type=Path),
    help="Workspace path for context",
)
@click.option(
    "--force",
    "-f",
    is_flag=True,
    help="Allow setting non-reloadable settings",
)
def set(path: str, value: str, workspace: Path | None, force: bool) -> None:
    """Set a configuration value at runtime.

    Only reloadable settings can be changed by default.
    Use --force to set any setting (changes are not persisted).

    Examples: forge set dashboard.refresh_interval_ms 500
    """
    try:
        # Try to parse value as JSON for proper type conversion
        import json
        try:
            parsed_value = json.loads(value)
        except json.JSONDecodeError:
            parsed_value = value  # Use as string if not valid JSON

        set_config_value(path, parsed_value, reloadable_only=not force, workspace_path=workspace)
        click.secho(f"✓ Set {path} = {parsed_value}", fg="green")
        click.echo("Note: This change is not persisted to disk.")
    except ConfigError as e:
        click.secho(f"✗ Error: {e}", fg="red")
        sys.exit(1)


@cli.command()
@click.option(
    "--config",
    "-c",
    type=click.Path(path_type=Path),
    help="Path to config file",
)
@click.option(
    "--workspace",
    "-w",
    type=click.Path(exists=True, path_type=Path),
    help="Workspace path for override loading",
)
@click.option(
    "--skip-setup",
    is_flag=True,
    help="Skip interactive setup even if config doesn't exist",
)
def dashboard(config: Path | None, workspace: Path | None, skip_setup: bool) -> None:
    """Launch the FORGE TUI dashboard.

    This is the main FORGE interface for managing AI workers and tasks.

    On first run, this will launch an interactive setup wizard.
    """
    # Check if this is first run (no config exists)
    if not skip_setup and not config and check_first_run():
        click.echo()
        setup_success = run_interactive_setup()

        if not setup_success:
            click.echo()
            click.echo("Setup incomplete. Run 'forge' again when ready.")
            sys.exit(0)

        # Give user a moment to read success message
        click.echo()
        click.pause("Press any key to launch FORGE dashboard...")
        click.clear()

    # Validate config first if specified
    if config:
        try:
            loader = ConfigLoader(user_config_path=config, workspace_path=workspace)
            loader.load()
        except ConfigError as e:
            click.secho(f"✗ Configuration error: {e}", fg="red")
            sys.exit(1)

    # Load config for the app
    try:
        app_config = get_config(workspace_path=workspace)
    except ConfigError as e:
        click.secho(f"✗ Failed to load configuration: {e}", fg="red")
        click.echo()
        click.echo("If you need to reconfigure FORGE, run:")
        click.echo("  forge init --force")
        sys.exit(1)

    # Launch the dashboard
    try:
        app = ForgeApp()
        app.run()
    except Exception as e:
        click.secho(f"✗ Failed to launch dashboard: {e}", fg="red")
        sys.exit(1)


def main() -> None:
    """Main entry point for the CLI"""
    cli()


if __name__ == "__main__":
    main()
