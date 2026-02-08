"""
FORGE Interactive Setup Wizard

Guides first-time users through CLI backend setup and configuration.
Follows ADR 0010: FORGE does not manage credentials - delegates to CLI tools.
"""

import shutil
import subprocess
import sys
from pathlib import Path
from typing import Optional

import click

from forge.launcher_templates import install_launcher_script


# =============================================================================
# CLI Detection
# =============================================================================


def detect_available_clis() -> dict[str, bool]:
    """Detect which CLI tools are available in PATH.

    Returns:
        Dict mapping CLI name to availability bool
    """
    clis = {
        "claude-code": shutil.which("claude") is not None,
        "opencode": shutil.which("opencode") is not None,
        "aider": shutil.which("aider") is not None,
    }
    return clis


def test_cli_backend(cli_name: str) -> tuple[bool, str]:
    """Test if a CLI backend is properly configured.

    Args:
        cli_name: Name of CLI tool (claude-code, opencode, aider)

    Returns:
        Tuple of (success: bool, message: str)
    """
    test_commands = {
        "claude-code": ["claude", "--version"],
        "opencode": ["opencode", "--version"],
        "aider": ["aider", "--version"],
    }

    if cli_name not in test_commands:
        return False, f"Unknown CLI: {cli_name}"

    try:
        result = subprocess.run(
            test_commands[cli_name],
            capture_output=True,
            text=True,
            timeout=5,
        )
        if result.returncode == 0:
            return True, f"{cli_name} is available and working"
        else:
            return False, f"{cli_name} failed: {result.stderr}"
    except subprocess.TimeoutExpired:
        return False, f"{cli_name} timed out"
    except Exception as e:
        return False, f"{cli_name} error: {e}"


# =============================================================================
# Config Generation
# =============================================================================


def get_cli_config_template(cli_name: str) -> dict:
    """Get config template for a specific CLI tool.

    Args:
        cli_name: Name of CLI tool

    Returns:
        Dict with config template
    """
    templates = {
        "claude-code": {
            "chat_backend": {
                "command": "claude",
                "args": ["--dangerously-skip-permissions", "--output-format", "stream-json"],
                "model": "sonnet",
                "timeout": 30,
                "max_retries": 3,
            },
            "launchers": {
                "claude-code": {
                    "executable": "~/.forge/launchers/claude-code-launcher",
                    "models": ["sonnet", "opus", "haiku"],
                }
            },
        },
        "opencode": {
            "chat_backend": {
                "command": "opencode",
                "args": ["--headless"],
                "model": "default",
                "timeout": 30,
                "max_retries": 3,
            },
            "launchers": {
                "opencode": {
                    "executable": "~/.forge/launchers/opencode-launcher",
                    "models": ["default"],
                }
            },
        },
        "aider": {
            "chat_backend": {
                "command": "aider",
                "args": ["--yes", "--no-pretty"],
                "model": "gpt-4",
                "timeout": 30,
                "max_retries": 3,
            },
            "launchers": {
                "aider": {
                    "executable": "~/.forge/launchers/aider-launcher",
                    "models": ["gpt-4", "sonnet"],
                }
            },
        },
    }

    return templates.get(cli_name, {})


def generate_config_file(cli_name: str, config_path: Path) -> None:
    """Generate FORGE config file for a CLI backend.

    Args:
        cli_name: Name of CLI tool
        config_path: Path to write config file
    """
    import yaml

    # Get template
    template = get_cli_config_template(cli_name)

    # Add common settings
    full_config = {
        **template,
        "log_collection": {
            "paths": ["~/.forge/logs/*.log"],
            "format": "jsonl",
            "poll_interval_seconds": 1,
            "max_age_days": 30,
            "max_size_mb": 1000,
        },
        "status_path": "~/.forge/status/",
        "cost_tracking": {
            "enabled": True,
            "database_path": "~/.forge/forge_costs.db",
            "forecast_days": 30,
        },
        "dashboard": {
            "refresh_interval_ms": 1000,
            "max_fps": 60,
            "default_layout": "overview",
        },
        "hotkeys": {
            "workers_view": "w",
            "tasks_view": "t",
            "costs_view": "c",
            "metrics_view": "m",
            "logs_view": "l",
            "overview": "o",
            "spawn_worker": "s",
            "kill_worker": "k",
            "chat_input": ":",
        },
        "routing": {
            "priority_tiers": {
                "P0": "premium",
                "P1": "premium",
                "P2": "standard",
                "P3": "budget",
                "P4": "budget",
            },
            "subscription_first": True,
            "fallback_to_api": True,
        },
    }

    # Ensure parent directory exists
    config_path.parent.mkdir(parents=True, exist_ok=True)

    # Write config
    with open(config_path, "w") as f:
        yaml.dump(full_config, f, default_flow_style=False, sort_keys=False)


def create_forge_directories() -> None:
    """Create FORGE directory structure."""
    forge_home = Path.home() / ".forge"

    directories = [
        forge_home / "logs",
        forge_home / "status",
        forge_home / "launchers",
        forge_home / "workers",
        forge_home / "layouts",
    ]

    for directory in directories:
        directory.mkdir(parents=True, exist_ok=True)


# =============================================================================
# Setup Instructions
# =============================================================================


def get_setup_instructions(cli_name: str) -> str:
    """Get setup instructions for a CLI tool.

    Args:
        cli_name: Name of CLI tool

    Returns:
        Formatted setup instructions
    """
    instructions = {
        "claude-code": """
📝 Setting up Claude Code:

1. If not installed:
   npm install -g @anthropic-ai/claude-code

2. Authenticate:
   claude auth

   This will prompt you to:
   - Visit claude.ai to get your API key
   - Paste it when prompted

3. Your credentials are saved to ~/.claude/config.json

Run 'forge' again once setup is complete!
""",
        "opencode": """
📝 Setting up OpenCode:

1. If not installed:
   pip install opencode

2. Configure OpenCode with your preferred model provider

3. Run 'forge' again once setup is complete!
""",
        "aider": """
📝 Setting up Aider:

1. If not installed:
   pip install aider-chat

2. Set up your API key:
   export ANTHROPIC_API_KEY="sk-ant-..."
   # Or:
   export OPENAI_API_KEY="sk-..."

3. Add to your shell profile to persist:
   echo 'export ANTHROPIC_API_KEY="sk-ant-..."' >> ~/.bashrc

4. Run 'forge' again once setup is complete!
""",
    }

    return instructions.get(cli_name, "No instructions available")


# =============================================================================
# Interactive Setup Wizard
# =============================================================================


def run_interactive_setup() -> bool:
    """Run the interactive setup wizard.

    Returns:
        True if setup completed successfully, False otherwise
    """
    click.clear()
    click.secho("=" * 70, fg="cyan")
    click.secho("  ⚒️  FORGE Setup Wizard", fg="cyan", bold=True)
    click.secho("=" * 70, fg="cyan")
    click.echo()
    click.echo("Welcome to FORGE! Let's get you set up.")
    click.echo()

    # Step 1: Detect available CLIs
    click.echo("🔍 Detecting available CLI tools...")
    available_clis = detect_available_clis()

    available_names = [name for name, available in available_clis.items() if available]

    if not available_names:
        click.secho("❌ No CLI tools found!", fg="red", bold=True)
        click.echo()
        click.echo("FORGE requires a headless CLI backend like:")
        click.echo("  • claude-code (npm install -g @anthropic-ai/claude-code)")
        click.echo("  • opencode (pip install opencode)")
        click.echo("  • aider (pip install aider-chat)")
        click.echo()
        click.echo("Install one of these tools and run 'forge' again.")
        return False

    click.secho(f"✓ Found {len(available_names)} CLI tool(s)", fg="green")
    for name in available_names:
        click.echo(f"  • {name}")
    click.echo()

    # Step 2: Choose backend
    if len(available_names) == 1:
        chosen_cli = available_names[0]
        click.echo(f"Using {chosen_cli} as your backend.")
    else:
        click.echo("Which CLI would you like to use?")
        for i, name in enumerate(available_names, 1):
            click.echo(f"  {i}. {name}")

        choice = click.prompt(
            "Enter number",
            type=click.IntRange(1, len(available_names)),
            default=1,
        )
        chosen_cli = available_names[choice - 1]

    click.echo()

    # Step 3: Test CLI configuration
    click.echo(f"🧪 Testing {chosen_cli} configuration...")
    success, message = test_cli_backend(chosen_cli)

    if not success:
        click.secho(f"⚠️  {message}", fg="yellow")
        click.echo()
        click.echo(f"{chosen_cli} is installed but may not be configured yet.")
        click.echo()

        # Show setup instructions
        click.secho("Setup Instructions:", fg="cyan", bold=True)
        click.echo(get_setup_instructions(chosen_cli))

        proceed = click.confirm("Have you completed the setup?", default=False)
        if not proceed:
            click.echo()
            click.echo("Run 'forge' again once you've completed the setup.")
            return False

        # Re-test
        success, message = test_cli_backend(chosen_cli)
        if not success:
            click.secho(f"❌ Still not working: {message}", fg="red")
            return False

    click.secho(f"✓ {message}", fg="green")
    click.echo()

    # Step 4: Create config and install launcher
    click.echo("📝 Generating FORGE configuration...")
    config_path = Path.home() / ".forge" / "config.yaml"
    launchers_dir = Path.home() / ".forge" / "launchers"

    try:
        create_forge_directories()
        generate_config_file(chosen_cli, config_path)
        click.secho(f"✓ Created config at {config_path}", fg="green")

        # Install launcher script
        click.echo(f"🔧 Installing {chosen_cli} launcher script...")
        launcher_path = install_launcher_script(chosen_cli, launchers_dir)
        click.secho(f"✓ Installed launcher at {launcher_path}", fg="green")

    except Exception as e:
        click.secho(f"❌ Failed to create config: {e}", fg="red")
        return False

    click.echo()

    # Step 5: Success!
    click.secho("=" * 70, fg="green")
    click.secho("  ✅ Setup Complete!", fg="green", bold=True)
    click.secho("=" * 70, fg="green")
    click.echo()
    click.echo("FORGE is ready to use!")
    click.echo()
    click.echo("Next steps:")
    click.echo("  • The dashboard will launch automatically")
    click.echo("  • Press ':' to chat with FORGE")
    click.echo("  • Press '?' for help")
    click.echo()

    return True


def check_first_run() -> bool:
    """Check if this is the first run (no config exists).

    Returns:
        True if first run, False if config exists
    """
    config_path = Path.home() / ".forge" / "config.yaml"
    return not config_path.exists()
