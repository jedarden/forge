//! Test example for the wizard TUI.
//!
//! Run with: cargo run --package forge-init --example test_wizard

use forge_init::detection::{CliToolDetection, ToolStatus};
use forge_init::wizard::run_wizard;
use std::path::PathBuf;

fn main() {
    // Create mock tools for testing
    let tools = vec![
        CliToolDetection::new("claude-code", PathBuf::from("/usr/local/bin/claude"))
            .with_version("1.2.3")
            .with_headless_support(true)
            .with_skip_permissions(true)
            .with_api_key(false, None, true)
            .with_status(ToolStatus::Ready),
        CliToolDetection::new("opencode", PathBuf::from("/usr/local/bin/opencode"))
            .with_version("0.5.0")
            .with_headless_support(true)
            .with_skip_permissions(true)
            .with_api_key(false, None, true)
            .with_status(ToolStatus::Ready),
        CliToolDetection::new("aider", PathBuf::from("/usr/local/bin/aider"))
            .with_version("0.35.0")
            .with_headless_support(false)
            .with_skip_permissions(false)
            .with_api_key(true, Some("OPENAI_API_KEY".to_string()), false)
            .with_status(ToolStatus::MissingApiKey),
    ];

    match run_wizard(tools) {
        Ok(Some(tool)) => {
            println!("\n✅ Selected: {} (v{})", tool.name, tool.version.as_deref().unwrap_or("unknown"));
            println!("   Path: {}", tool.binary_path.display());
            println!("   Status: {}", tool.status_message());
        }
        Ok(None) => {
            println!("\n📝 Manual setup selected - skipping auto-configuration");
        }
        Err(e) => {
            eprintln!("\n❌ Wizard error: {}", e);
            std::process::exit(1);
        }
    }
}
