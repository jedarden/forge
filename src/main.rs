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
use forge_init::{detection, generator, guidance, validator, wizard};
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
    /// Initialize FORGE configuration (first-time setup)
    Init {
        /// Run in non-interactive mode (for CI/automation)
        /// Auto-selects first available backend or uses FORGE_CHAT_BACKEND env var
        #[arg(long)]
        non_interactive: bool,

        /// Force re-initialization even if config exists
        #[arg(long)]
        force: bool,
    },

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

    /// Validate FORGE configuration
    Validate {
        /// Show detailed validation results
        #[arg(long, short = 'v')]
        verbose: bool,

        /// Attempt automatic fixes for issues
        #[arg(long)]
        fix: bool,

        /// Skip chat backend connectivity test
        #[arg(long)]
        skip_backend_test: bool,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
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
            Commands::Init {
                non_interactive,
                force,
            } => {
                return handle_init_command(*non_interactive, *force);
            }
            Commands::Worker { action } => {
                return handle_worker_action(action);
            }
            Commands::Validate {
                verbose,
                fix,
                skip_backend_test,
                json,
            } => {
                return handle_validate_command(*verbose, *fix, *skip_backend_test, *json);
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

        if let Err(e) = run_onboarding(false, None) {
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
///
/// # Arguments
///
/// * `non_interactive` - If true, suppress output and auto-select backend
/// * `preferred_backend` - Optional backend to use (from FORGE_CHAT_BACKEND env var)
fn run_onboarding(
    non_interactive: bool,
    preferred_backend: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Starting onboarding flow (non_interactive={}, preferred_backend={:?})",
        non_interactive, preferred_backend
    );

    // Detect CLI tools with diagnostics
    if !non_interactive {
        eprintln!("🔍 Detecting available CLI tools...");
    }
    let (tools, diagnostics) = detection::detect_cli_tools_with_diagnostics()?;

    // Filter to only show tools that are ready to use
    let ready_tools: Vec<_> = tools.iter().filter(|t| t.is_ready()).collect();

    if ready_tools.is_empty() {
        if non_interactive {
            // In non-interactive mode, output to stderr and exit
            eprintln!("error: No compatible CLI tools available");
            eprintln!("Install one of: claude (Claude Code), opencode, aider");
        } else {
            // Use the guidance module to show detailed instructions
            eprint!("{}", guidance::generate_guidance(Some(&diagnostics)));
        }
        return Err("No compatible CLI tools available".into());
    }

    // Select the backend to use
    let selected_tool: detection::CliToolDetection = if let Some(backend) = preferred_backend {
        // Try to find the specified backend
        let normalized_backend = normalize_backend_name(backend);
        let found = tools.iter().find(|t| {
            let tool_name = normalize_backend_name(&t.name);
            tool_name == normalized_backend
        });

        match found {
            Some(tool) => {
                if !tool.is_ready() {
                    eprintln!(
                        "error: Backend '{}' is not ready: {}",
                        backend,
                        tool.status_message()
                    );
                    if tool.status == detection::ToolStatus::MissingApiKey {
                        if let Some(env_var) = &tool.api_key_env_var {
                            eprintln!("Set the {} environment variable and try again", env_var);
                        }
                    }
                    return Err(format!("Backend '{}' is not ready", backend).into());
                }
                tool.clone()
            }
            None => {
                eprintln!("error: Backend '{}' not found", backend);
                eprintln!(
                    "Available backends: {}",
                    tools
                        .iter()
                        .map(|t| t.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return Err(format!("Backend '{}' not found", backend).into());
            }
        }
    } else if non_interactive {
        // Auto-select the first ready tool in non-interactive mode
        tools
            .iter()
            .find(|t| t.is_ready())
            .ok_or("No ready tools available. Please set required API keys.")?
            .clone()
    } else {
        // Interactive mode: show the TUI wizard for selection
        match wizard::run_wizard(tools.clone()) {
            Ok(Some(tool)) => {
                // User selected a tool
                eprintln!(
                    "\n✨ Using: {} ({})",
                    tool.name,
                    tool.binary_path.display()
                );
                tool
            }
            Ok(None) => {
                // User chose "Manual Setup" - skip auto-configuration
                eprintln!("\n📝 Manual setup selected.");
                eprintln!("   Create ~/.forge/config.yaml manually to configure FORGE.");
                return Ok(());
            }
            Err(wizard::WizardError::UserCancelled) => {
                eprintln!("\n👋 Setup cancelled.");
                return Err("User cancelled setup".into());
            }
            Err(wizard::WizardError::NoToolsAvailable) => {
                // This shouldn't happen since we already checked, but handle it
                eprint!("{}", guidance::generate_guidance(Some(&diagnostics)));
                return Err("No compatible CLI tools available".into());
            }
            Err(e) => {
                eprintln!("\nerror: Wizard failed: {}", e);
                return Err(format!("Wizard failed: {}", e).into());
            }
        }
    };

    if non_interactive {
        // Only print in non-interactive mode (wizard already prints in interactive mode)
        info!(
            "Auto-selected: {} ({})",
            selected_tool.name,
            selected_tool.binary_path.display()
        );
    }

    // Create directory structure
    let forge_dir = get_forge_dir();
    if !non_interactive {
        eprintln!("\n📁 Creating directory structure...");
    }
    generator::create_directory_structure(&forge_dir)?;

    // Generate config.yaml
    let config_path = forge_dir.join("config.yaml");
    if !non_interactive {
        eprintln!("📝 Generating config.yaml...");
    }
    generator::generate_config_yaml(&selected_tool, &config_path)?;

    // Generate launcher script
    let launcher_name = format!("{}-launcher", selected_tool.name);
    let launcher_path = forge_dir.join("launchers").join(&launcher_name);
    if !non_interactive {
        eprintln!("🚀 Generating launcher script...");
    }
    generator::generate_launcher_script(&selected_tool, &launcher_path)?;

    if !non_interactive {
        eprintln!("\n✅ Configuration complete!");
        eprintln!("   Config: {}", config_path.display());
        eprintln!("   Launcher: {}", launcher_path.display());
    }

    info!(
        "Onboarding complete: using {} backend",
        selected_tool.name
    );
    Ok(())
}

/// Normalize backend name for comparison.
///
/// Maps common aliases to canonical names:
/// - "claude", "claude-code", "claudecode" -> "claude-code"
/// - "opencode" -> "opencode"
/// - "aider" -> "aider"
fn normalize_backend_name(name: &str) -> String {
    let lower = name.to_lowercase();
    match lower.as_str() {
        "claude" | "claude-code" | "claudecode" => "claude-code".to_string(),
        other => other.to_string(),
    }
}

/// Handle the `forge init` command.
fn handle_init_command(non_interactive: bool, force: bool) -> ExitCode {
    info!(
        "Handling init command (non_interactive={}, force={})",
        non_interactive, force
    );

    // Check if config already exists
    if !force && !needs_onboarding() {
        let forge_dir = get_forge_dir();
        let config_path = forge_dir.join("config.yaml");

        if non_interactive {
            eprintln!(
                "error: Configuration already exists at {}",
                config_path.display()
            );
            eprintln!("Use --force to overwrite existing configuration");
            return ExitCode::from(1);
        } else {
            eprintln!(
                "Configuration already exists at {}",
                config_path.display()
            );
            eprintln!("Use --force to overwrite existing configuration");
            return ExitCode::SUCCESS;
        }
    }

    // Get preferred backend from environment variable
    let preferred_backend = std::env::var("FORGE_CHAT_BACKEND").ok();

    if non_interactive {
        // Non-interactive mode: quiet output, clear error messages to stderr
        match run_onboarding(true, preferred_backend.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::from(1),
        }
    } else {
        // Interactive mode: show welcome message and full output
        eprintln!("🚀 FORGE Setup");
        eprintln!();

        match run_onboarding(false, preferred_backend.as_deref()) {
            Ok(()) => {
                eprintln!("\n✅ Setup complete!");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("❌ Setup failed: {}", e);
                ExitCode::from(1)
            }
        }
    }
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

/// Handle the `forge validate` command.
fn handle_validate_command(verbose: bool, fix: bool, skip_backend_test: bool, json: bool) -> ExitCode {
    info!(
        "Handling validate command (verbose={}, fix={}, skip_backend_test={}, json={})",
        verbose, fix, skip_backend_test, json
    );

    let forge_dir = get_forge_dir();

    // Run comprehensive validation
    let results = validator::validate_comprehensive(&forge_dir, verbose, fix, skip_backend_test);

    // Output results
    if json {
        // JSON output
        match serde_json::to_string_pretty(&results) {
            Ok(json_output) => {
                println!("{}", json_output);
            }
            Err(e) => {
                eprintln!("❌ Failed to serialize results to JSON: {}", e);
                return ExitCode::from(1);
            }
        }
    } else {
        // Human-readable output
        print_validation_results(&results, verbose);
    }

    // Return appropriate exit code
    if results.passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// Print validation results in human-readable format.
fn print_validation_results(results: &validator::ComprehensiveValidationResults, verbose: bool) {
    // Config file
    if results.config_valid {
        println!("✅ Config file: valid");
    } else {
        println!("❌ Config file: invalid");
    }

    // Launchers
    if results.launcher_valid {
        let count = results.launcher_count;
        let plural = if count == 1 { "" } else { "s" };
        println!("✅ Launchers: {} found, executable", count);
        if verbose {
            for launcher in &results.launcher_names {
                println!("   - {}", launcher);
            }
        }
    } else {
        println!("❌ Launchers: {}", results.launcher_message);
    }

    // Directories
    if results.directories_valid {
        println!("✅ Directories: all present");
    } else {
        println!("❌ Directories: missing");
        for dir in &results.missing_directories {
            println!("   - {}", dir);
        }
    }

    // Chat backend (only shown if tested)
    match &results.backend_status {
        validator::BackendStatus::NotTested => {
            println!("⚠️  Chat backend: not tested (use --test-backend)");
        }
        validator::BackendStatus::Skipped => {
            println!("⏭️  Chat backend: skipped (--skip-backend-test)");
        }
        validator::BackendStatus::Ready { command } => {
            println!("✅ Chat backend: ready ({})", command);
        }
        validator::BackendStatus::NotConfigured => {
            println!("⚠️  Chat backend: not configured");
        }
        validator::BackendStatus::CommandNotFound { command } => {
            println!("❌ Chat backend: command not found ({})", command);
        }
        validator::BackendStatus::Error { message } => {
            println!("❌ Chat backend: error - {}", message);
        }
    }

    // Warnings
    if !results.warnings.is_empty() {
        println!();
        println!("Warnings:");
        for warning in &results.warnings {
            println!("  ⚠️  {}", warning);
        }
    }

    // Fixes applied
    if !results.fixes_applied.is_empty() {
        println!();
        println!("Fixes applied:");
        for fix in &results.fixes_applied {
            println!("  🔧 {}", fix);
        }
    }

    // Verbose details
    if verbose && !results.details.is_empty() {
        println!();
        println!("Details:");
        for detail in &results.details {
            println!("  ℹ️  {}", detail);
        }
    }

    // Summary
    println!();
    if results.passed {
        println!("✅ Validation passed");
    } else {
        println!("❌ Validation failed");
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
