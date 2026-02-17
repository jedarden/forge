//! FORGE initialization and onboarding.
//!
//! This crate provides the onboarding flow for first-time FORGE users,
//! including CLI tool detection, configuration generation, and validation.
//!
//! ## Validation
//!
//! The `validator` module provides comprehensive configuration validation
//! for the `forge validate` command.

pub mod detection;
pub mod generator;
pub mod guidance;
pub mod validator;
pub mod wizard;

pub use detection::CliToolDetection;
pub use guidance::{PathDiagnostics, Platform, RejectionReason, ToolFixInfo, generate_guidance, generate_not_ready_guidance};
pub use validator::{BackendStatus, ComprehensiveValidationResults, validate_comprehensive};
