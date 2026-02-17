//! FORGE initialization and onboarding.
//!
//! This crate provides the onboarding flow for first-time FORGE users,
//! including CLI tool detection, configuration generation, and validation.

pub mod detection;
pub mod generator;
pub mod guidance;
pub mod validator;
pub mod wizard;

pub use detection::CliToolDetection;
pub use guidance::{PathDiagnostics, Platform, RejectionReason, generate_guidance};
