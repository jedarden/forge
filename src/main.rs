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
//! # With custom log directory
//! forge --log-dir /path/to/logs/
//!
//! # Show version
//! forge --version
//! ```

use std::process::ExitCode;

use clap::Parser;
use forge_core::{init_logging, LogGuard};
use forge_tui::App;
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

    info!("Starting FORGE dashboard");

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

/// Set up logging based on CLI arguments.
fn setup_logging(cli: &Cli) -> forge_core::Result<LogGuard> {
    // Initialize logging with the specified log directory
    // verbose flag increases log level
    let debug = cli.verbose > 0;
    init_logging(cli.log_dir.clone(), debug)
}

/// Run the TUI application.
fn run_app() -> forge_tui::AppResult<()> {
    let mut app = App::new();
    app.run()
}
