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

use std::io::Write;
use std::panic;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use forge_core::{LogGuard, StatusWriter, init_logging};
use forge_init::{detection, generator, guidance};
use forge_tui::{App, ForgeConfig};
use tracing::{error, info};

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

    /// Subcommands
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Update forge binary from a URL
    Update {
        /// URL to download the new binary from
        url: String,

        /// Expected SHA256 checksum of the binary
        checksum: String,

        /// Filename to save in staging (defaults to "forge")
        #[arg(short, long, default_value = "forge")]
        filename: String,
    },

    /// Rollback to the previous version
    Rollback,

    /// Clean the staging directory
    CleanStaging,

    /// Manage workers
    Worker {
        #[command(subcommand)]
        action: WorkerAction,
    },
}

#[derive(Subcommand, Debug)]
enum WorkerAction {
    /// Pause a worker (stop claiming new tasks)
    Pause {
        /// Worker ID to pause, or "all" to pause all workers
        id: String,
    },

    /// Resume a paused worker
    Resume {
        /// Worker ID to resume, or "all" to resume all workers
        id: String,
    },
}

fn main() -> ExitCode {
    // CRITICAL: Check for rollback BEFORE parsing CLI or initializing anything
    // This must be the first thing we do to detect crashes from previous updates
    #[cfg(feature = "self-update")]
    {
        use forge_core::RollbackResult;

        match forge_core::check_and_rollback() {
            RollbackResult::RolledBack {
                failed_version,
                restored_version,
            } => {
                let restored = restored_version
                    .map(|v| format!("v{}", v))
                    .unwrap_or_else(|| "previous version".to_string());
                eprintln!(
                    "⚠️  Update to v{} failed on startup - rolled back to {}",
                    failed_version, restored
                );
                eprintln!("❌ Update failed, rolled back to previous version");
                eprintln!("Please check ~/.forge/logs/forge.log for error details\n");
                // Continue running with the rolled-back version
            }
            RollbackResult::Failed(err) => {
                eprintln!("❌ Critical: Rollback failed: {}", err);
                eprintln!("You may need to manually restore from backup\n");
                // Continue anyway - better to try running than to abort
            }
            RollbackResult::NotNeeded => {
                // Normal startup, no crash detected
            }
        }
    }

    // Mark that we're starting up (for crash detection on next run)
    #[cfg(feature = "self-update")]
    {
        if let Err(e) = forge_core::mark_startup_in_progress() {
            eprintln!("Warning: Failed to create startup marker: {}", e);
            // Don't fail startup just because of this
        }
    }

    let cli = Cli::parse();

    // Initialize logging
    let _guard = match setup_logging(&cli) {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("Failed to initialize logging: {}", e);
            return ExitCode::from(1);
        }
    };

    // Check if this is a freshly exec'd process that needs to install itself
    #[cfg(feature = "self-update")]
    {
        match forge_core::check_and_perform_self_install() {
            Ok(Some(install_path)) => {
                eprintln!("✅ Update installed successfully to: {}", install_path.display());
                eprintln!("🚀 Restarting FORGE with new version...\n");
                info!("Self-install completed to {:?}", install_path);
                // Continue with normal startup
            }
            Ok(None) => {
                // Normal startup, not an auto-restart
            }
            Err(e) => {
                eprintln!("❌ Self-install failed: {}", e);
                error!("Self-install error: {}", e);
                return ExitCode::from(1);
            }
        }
    }

    #[cfg(not(feature = "self-update"))]
    {
        // No self-update feature, skip installation check
    }

    // Handle subcommands that don't require the TUI
    if let Some(command) = &cli.command {
        match command {
            Commands::Worker { action } => {
                return handle_worker_action(action);
            }
            _ => {
                // Other commands fall through to TUI startup
            }
        }
    }

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

    // Validate config file if it exists
    if !needs_onboarding() {
        if let Err(e) = validate_config() {
            eprintln!("\n{}", e);
            return ExitCode::from(1);
        }
    }

    // Check for required dependencies
    if !forge_core::check_dependencies() {
        // check_dependencies already prints error messages
        return ExitCode::from(1);
    }

    info!("Starting FORGE dashboard");

    // Mark startup as successful (app initialized without crashing)
    #[cfg(feature = "self-update")]
    {
        if let Err(e) = forge_core::mark_startup_successful() {
            error!("Failed to mark startup as successful: {}", e);
            // Don't fail just because of this
        }
    }

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

    // Detect CLI tools with diagnostics
    eprintln!("🔍 Detecting available CLI tools...");
    let (tools, diagnostics) = detection::detect_cli_tools_with_diagnostics()?;

    // Filter to only show tools that are ready to use
    let ready_tools: Vec<_> = tools.iter().filter(|t| t.is_ready()).collect();

    if ready_tools.is_empty() {
        // Use the guidance module to show detailed instructions
        eprint!("{}", guidance::generate_guidance(Some(&diagnostics)));
        return Err("No compatible CLI tools available".into());
    }

    // Display detected tools
    eprintln!("\n📦 Detected tools:");
    for tool in &tools {
        let status_icon = match tool.status {
            detection::ToolStatus::Ready => "✅",
            detection::ToolStatus::MissingApiKey => "⚠️ ",
            _ => "❌",
        };
        eprintln!(
            "  {} {} - {} ({})",
            status_icon,
            tool.name,
            tool.status_message(),
            tool.binary_path.display()
        );
        if let Some(version) = &tool.version {
            eprintln!("     Version: {}", version);
        }
        if tool.api_key_required
            && !tool.api_key_detected
            && let Some(env_var) = &tool.api_key_env_var
        {
            eprintln!("     Missing: {}", env_var);
        }
    }

    // Select the first ready tool
    let selected_tool = tools
        .iter()
        .find(|t| t.is_ready())
        .ok_or("No ready tools available. Please set required API keys.")?;

    eprintln!(
        "\n✨ Using: {} ({})",
        selected_tool.name,
        selected_tool.binary_path.display()
    );

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

/// Validate the config file and offer recovery if invalid.
fn validate_config() -> Result<(), String> {
    use std::io::{self, Write};

    match ForgeConfig::load_with_error() {
        Ok(_config) => {
            info!("Configuration validated successfully");
            Ok(())
        }
        Err(e) => {
            // Format the error with line/column information
            eprintln!("❌ Configuration Error");
            eprintln!();

            let path_str = e.path().map(|p| p.display().to_string())
                .unwrap_or_else(|| "~/.forge/config.yaml".to_string());

            if let (Some(line), Some(col)) = (e.line_number(), e.column_number()) {
                eprintln!("  File: {}", path_str);
                eprintln!("  Line: {}, Column: {}", line, col);
                eprintln!("  Error: {}", e);
            } else {
                eprintln!("  {}", e);
            }
            eprintln!();

            // Offer recovery options
            eprintln!("Recovery options:");
            eprintln!("  1) Reset to default configuration (creates backup)");
            eprintln!("  2) Use defaults without modifying file (temporary)");
            eprintln!("  3) Exit and fix manually");
            eprintln!();
            eprint!("Choose option [1-3]: ");
            io::stdout().flush().map_err(|e| e.to_string())?;

            let mut choice = String::new();
            io::stdin().read_line(&mut choice).map_err(|e| e.to_string())?;

            match choice.trim() {
                "1" => {
                    if let Err(e) = reset_config_to_defaults() {
                        return Err(format!("Failed to reset config: {}", e));
                    }
                    eprintln!("✅ Configuration reset to defaults");
                    eprintln!("   Previous config backed up");
                    eprintln!();
                    Ok(())
                }
                "2" => {
                    eprintln!("⚠️  Using default configuration (file not modified)");
                    eprintln!("   Fix the config file manually or delete it to regenerate");
                    eprintln!();
                    Ok(())
                }
                "3" | "" => {
                    Err("Exiting. Please fix the config file and try again.".to_string())
                }
                _ => {
                    Err("Invalid choice. Exiting.".to_string())
                }
            }
        }
    }
}

/// Reset config file to defaults with backup.
fn reset_config_to_defaults() -> Result<(), Box<dyn std::error::Error>> {
    let forge_dir = get_forge_dir();
    let config_path = forge_dir.join("config.yaml");

    // Create backup
    if config_path.exists() {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let backup_path = forge_dir.join(format!("config.yaml.backup.{}", timestamp));
        std::fs::copy(&config_path, &backup_path)?;
        info!("Created config backup: {}", backup_path.display());
    }

    // Generate default config
    let default_config = r#"# FORGE Configuration
# This file was automatically generated after a config error.

dashboard:
  refresh_interval_ms: 1000
  max_fps: 60
  default_layout: overview

theme:
  name: default

cost_tracking:
  enabled: true
  budget_warning_threshold: 70
  budget_critical_threshold: 90
  # monthly_budget_usd: 100.0

# Chat backend configuration (optional)
# Uncomment and configure if using the chat feature:
# chat_backend:
#   command: claude-code
#   args:
#     - --headless
#     - --model
#     - sonnet
#   model: sonnet
"#;

    std::fs::write(&config_path, default_config)?;
    info!("Reset config to defaults: {}", config_path.display());
    Ok(())
}

/// Handle worker subcommand actions.
fn handle_worker_action(action: &WorkerAction) -> ExitCode {
    let writer = match StatusWriter::new(None) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("❌ Failed to initialize status writer: {}", e);
            return ExitCode::from(1);
        }
    };

    match action {
        WorkerAction::Pause { id } => {
            if id.to_lowercase() == "all" {
                match writer.pause_all() {
                    Ok(count) => {
                        if count == 0 {
                            eprintln!("No workers to pause");
                        } else {
                            eprintln!("⏸️  Paused {} worker(s)", count);
                        }
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to pause workers: {}", e);
                        ExitCode::from(1)
                    }
                }
            } else {
                match writer.pause_worker(id) {
                    Ok(()) => {
                        eprintln!("⏸️  Paused worker: {}", id);
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to pause worker '{}': {}", id, e);
                        ExitCode::from(1)
                    }
                }
            }
        }
        WorkerAction::Resume { id } => {
            if id.to_lowercase() == "all" {
                match writer.resume_all() {
                    Ok(count) => {
                        if count == 0 {
                            eprintln!("No workers to resume");
                        } else {
                            eprintln!("▶️  Resumed {} worker(s)", count);
                        }
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to resume workers: {}", e);
                        ExitCode::from(1)
                    }
                }
            } else {
                match writer.resume_worker(id) {
                    Ok(()) => {
                        eprintln!("▶️  Resumed worker: {}", id);
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to resume worker '{}': {}", id, e);
                        ExitCode::from(1)
                    }
                }
            }
        }
    }
}

/// Run the TUI application.
fn run_app() -> forge_tui::AppResult<()> {
    use std::time::Instant;

    let start = Instant::now();
    info!("⏱️ run_app() started - creating App...");
    let mut app = App::new();
    info!("⏱️ App created in {:?}", start.elapsed());

    let run_start = Instant::now();
    info!("⏱️ Starting app.run()...");
    let result = app.run();
    info!("⏱️ app.run() completed in {:?}", run_start.elapsed());

    result
}
