//! Interactive TUI wizard for onboarding.
//!
//! Provides an interactive setup flow showing detected CLI tools and allowing
//! the user to select which one to configure.

use crate::detection::CliToolDetection;
use thiserror::Error;

/// Errors that can occur during wizard flow.
#[derive(Debug, Error)]
pub enum WizardError {
    #[error("User cancelled setup")]
    UserCancelled,

    #[error("TUI rendering failed: {0}")]
    RenderFailed(String),

    #[error("No CLI tools available")]
    NoToolsAvailable,
}

/// Result type for wizard operations.
pub type Result<T> = std::result::Result<T, WizardError>;

/// Run the interactive onboarding wizard.
///
/// Shows detected tools and lets the user select which one to configure.
/// Returns the selected tool or None if user cancelled.
pub fn run_wizard(_tools: Vec<CliToolDetection>) -> Result<Option<CliToolDetection>> {
    // TODO: Implement in fg-15p
    todo!()
}
