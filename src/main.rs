//! FORGE - Agent Orchestration Dashboard
//!
//! A terminal-based dashboard for managing FORGE AI worker agents.
//!
//! ## Usage
//!
//! ```bash
//! # Start the TUI dashboard
//! forge
//!
//! # With verbose logging
//! forge -v
//!
//! # With debug mode (verbose tracing to ~/.forge/logs/forge.log in JSON format)
//! forge --debug
//!
//! # With custom log directory
//! forge --log-dir /path/to/logs/
//!
//! # Show version
//! forge --version
//! ```

use std::fs;
use std::io::Write;
use std::panic;
use std::process::ExitCode;

use clap::Parser;
use forge_core::{init_logging, LogGuard};
use forge_init::{detection, generator};
use forge_tui::App;
use tracing::{error, info, warn};

/// FORGE Agent Orchestration Dashboard
///
/// A terminal-based interface for managing AI worker agents,
/// monitoring task queues, and tracking costs.
#[derive(Parser, Debug)]
#[command(name = "forge")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Enable verbose logging (increases log level)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Enable debug mode with verbose tracing output to ~/.forge/logs/forge.log
    #[arg(long)]
    debug: bool,

    /// Directory for log files (defaults to ~/.forge/logs/)
    #[arg(long)]
    log_dir: Option<std::path::PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Initialize logging
    let _guard = match setup_logging(&cli) {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("Failed to initialize logging: {}", e);
            return ExitCode::from(1);
        }
    };

    // Install panic hook to ensure terminal cleanup
    install_panic_hook();

    // Check if onboarding is needed
    if needs_onboarding() {
        eprintln!("🚀 Welcome to FORGE!");
        eprintln!("No configuration found. Running first-time setup...\n");

        if let Err(e) = run_onboarding() {
            eprintln!("❌ Onboarding failed: {}", e);
            eprintln!("You can manually create ~/.forge/config.yaml or try again.");
            return ExitCode::from(1);
        }

        eprintln!("\n✅ Setup complete! Starting FORGE dashboard...\n");
    }

    info!("Starting FORGE dashboard");

    // Log terminal dimensions for debugging
    if let Ok((cols, rows)) = crossterm::terminal::size() {
        info!("Terminal size: {}x{} (columns x rows)", cols, rows);
        eprintln!("Terminal size: {}x{} (columns x rows)", cols, rows);
    } else {
        error!("Failed to get terminal size");
    }

    // Run the TUI application
    match run_app() {
        Ok(()) => {
            info!("FORGE dashboard exited normally");
            ExitCode::SUCCESS
        }
        Err(e) => {
            error!("FORGE dashboard error: {}", e);
            eprintln!("Error: {}", e);
            ExitCode::from(1)
        }
    }
}

/// Install a panic hook that restores the terminal before printing the panic message.
///
/// This ensures that even if the application panics while in raw mode with the
/// alternate screen enabled, the terminal will be properly restored so the user
/// can see the panic message and continue using their terminal.
fn install_panic_hook() {
    let original_hook = panic::take_hook();

    panic::set_hook(Box::new(move |panic_info| {
        // Attempt to restore terminal state
        let _ = restore_terminal();

        // Call the original panic hook to print the panic message
        original_hook(panic_info);
    }));
}

/// Restore terminal to its normal state.
///
/// This function is called both on normal exit and during panic handling.
fn restore_terminal() -> std::io::Result<()> {
    let mut stdout = std::io::stdout();

    // Disable raw mode first
    let _ = crossterm::terminal::disable_raw_mode();

    // Leave alternate screen and disable mouse capture
    crossterm::execute!(
        stdout,
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;

    // Show cursor
    crossterm::execute!(stdout, crossterm::cursor::Show)?;

    // Flush to ensure all escape sequences are written
    stdout.flush()?;

    Ok(())
}

/// Set up logging based on CLI arguments.
fn setup_logging(cli: &Cli) -> forge_core::Result<LogGuard> {
    // Initialize logging with the specified log directory
    // Either --debug or -v/--verbose enables debug logging
    let debug = cli.debug || cli.verbose > 0;
    init_logging(cli.log_dir.clone(), debug)
}

/// Check if onboarding is needed (no config.yaml exists).
fn needs_onboarding() -> bool {
    let forge_dir = get_forge_dir();
    let config_path = forge_dir.join("config.yaml");
    !config_path.exists()
}

/// Get the FORGE directory path (~/.forge).
fn get_forge_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".forge")
}

/// Run the onboarding flow.
fn run_onboarding() -> Result<(), Box<dyn std::error::Error>> {
    info!("Starting onboarding flow");

    // Detect CLI tools
    eprintln!("🔍 Detecting available CLI tools...");
    let tools = detection::detect_cli_tools()?;

    if tools.is_empty() {
        eprintln!("\n❌ No compatible CLI tools found!");
        eprintln!("\nFORGE requires one of:");
        eprintln!("  - Claude Code (https://claude.com/claude-code)");
        eprintln!("  - OpenCode");
        eprintln!("\nPlease install a compatible tool and try again.");
        return Err("No CLI tools available".into());
    }

    // Display detected tools
    eprintln!("\n📦 Detected tools:");
    for tool in &tools {
        let status_icon = match tool.status {
            detection::ToolStatus::Ready => "✅",
            detection::ToolStatus::MissingApiKey => "⚠️ ",
            _ => "❌",
        };
        eprintln!("  {} {} - {} ({})",
            status_icon,
            tool.name,
            tool.status_message(),
            tool.binary_path.display()
        );
        if let Some(version) = &tool.version {
            eprintln!("     Version: {}", version);
        }
        if tool.api_key_required && !tool.api_key_detected {
            if let Some(env_var) = &tool.api_key_env_var {
                eprintln!("     Missing: {}", env_var);
            }
        }
    }

    // Select the first ready tool
    let selected_tool = tools.iter()
        .find(|t| t.is_ready())
        .ok_or("No ready tools available. Please set required API keys.")?;

    eprintln!("\n✨ Using: {} ({})", selected_tool.name, selected_tool.binary_path.display());

    // Create directory structure
    let forge_dir = get_forge_dir();
    eprintln!("\n📁 Creating directory structure...");
    generator::create_directory_structure(&forge_dir)?;

    // Generate config.yaml
    let config_path = forge_dir.join("config.yaml");
    eprintln!("📝 Generating config.yaml...");
    generator::generate_config_yaml(selected_tool, &config_path)?;

    // Generate launcher script
    let launcher_name = format!("{}-launcher", selected_tool.name);
    let launcher_path = forge_dir.join("launchers").join(&launcher_name);
    eprintln!("🚀 Generating launcher script...");
    generator::generate_launcher_script(selected_tool, &launcher_path)?;

    eprintln!("\n✅ Configuration complete!");
    eprintln!("   Config: {}", config_path.display());
    eprintln!("   Launcher: {}", launcher_path.display());

    Ok(())
}

/// Run the TUI application.
fn run_app() -> forge_tui::AppResult<()> {
    let mut app = App::new();
    app.run()
}
