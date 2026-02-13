//! Error recovery module for FORGE TUI.
//!
//! This module implements graceful error recovery following ADR 0014:
//! - No automatic retry - user decides if/when to retry
//! - No silent failures - every error surfaced to user
//! - Degrade gracefully - broken component doesn't crash entire app
//! - Clear error messages with actionable guidance
//!
//! ## Error Categories
//!
//! 1. **Recoverable** - Component degrades, app continues (chat unavailable, etc.)
//! 2. **Fatal** - App cannot continue (terminal init failure, etc.)
//! 3. **Warning** - Non-critical issue, logged but doesn't affect operation

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tracing::{error, info, warn};

/// Maximum number of errors to keep in history for display.
const MAX_ERROR_HISTORY: usize = 50;

/// Error severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    /// Informational - not really an error, just noteworthy
    Info,
    /// Warning - something went wrong but operation continues
    Warning,
    /// Error - component failed, degraded mode
    Error,
    /// Fatal - app cannot continue
    Fatal,
}

impl fmt::Display for ErrorSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorSeverity::Info => write!(f, "INFO"),
            ErrorSeverity::Warning => write!(f, "WARNING"),
            ErrorSeverity::Error => write!(f, "ERROR"),
            ErrorSeverity::Fatal => write!(f, "FATAL"),
        }
    }
}

/// Error category for grouping related errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    /// Database errors (SQLite)
    Database,
    /// Configuration errors (YAML parsing, validation)
    Config,
    /// Network/API errors (HTTP, timeouts)
    Network,
    /// Worker errors (spawn, health, crashes)
    Worker,
    /// Chat backend errors
    Chat,
    /// File system errors (I/O, permissions)
    FileSystem,
    /// Terminal/UI errors
    Terminal,
    /// Internal errors (bugs)
    Internal,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCategory::Database => write!(f, "Database"),
            ErrorCategory::Config => write!(f, "Config"),
            ErrorCategory::Network => write!(f, "Network"),
            ErrorCategory::Worker => write!(f, "Worker"),
            ErrorCategory::Chat => write!(f, "Chat"),
            ErrorCategory::FileSystem => write!(f, "FileSystem"),
            ErrorCategory::Terminal => write!(f, "Terminal"),
            ErrorCategory::Internal => write!(f, "Internal"),
        }
    }
}

/// A recorded error with context.
#[derive(Debug, Clone)]
pub struct RecordedError {
    /// Unique identifier for this error
    pub id: usize,
    /// Error category
    pub category: ErrorCategory,
    /// Error severity
    pub severity: ErrorSeverity,
    /// Short title for display
    pub title: String,
    /// Detailed error message
    pub message: String,
    /// Actionable guidance for the user
    pub guidance: Vec<String>,
    /// When the error occurred
    pub timestamp: Instant,
    /// Whether this error has been acknowledged by the user
    pub acknowledged: bool,
    /// Component that is in degraded state (if any)
    pub degraded_component: Option<String>,
}

impl fmt::Display for RecordedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {} {}: {}",
            self.severity, self.category, self.title, self.message
        )
    }
}

/// Error recovery manager that tracks errors and provides recovery guidance.
#[derive(Debug, Default)]
pub struct ErrorRecoveryManager {
    /// All recorded errors
    errors: Vec<RecordedError>,
    /// Next error ID
    next_id: usize,
    /// Components currently in degraded state
    degraded_components: Vec<String>,
    /// Counts by category
    category_counts: std::collections::HashMap<ErrorCategory, usize>,
}

impl ErrorRecoveryManager {
    /// Create a new error recovery manager.
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            next_id: 1,
            degraded_components: Vec::new(),
            category_counts: std::collections::HashMap::new(),
        }
    }

    /// Record a new error and return its ID.
    pub fn record_error(
        &mut self,
        category: ErrorCategory,
        severity: ErrorSeverity,
        title: impl Into<String>,
        message: impl Into<String>,
        guidance: Vec<String>,
    ) -> usize {
        let id = self.next_id;
        self.next_id += 1;

        let error = RecordedError {
            id,
            category,
            severity,
            title: title.into(),
            message: message.into(),
            guidance,
            timestamp: Instant::now(),
            acknowledged: false,
            degraded_component: None,
        };

        // Log based on severity
        match severity {
            ErrorSeverity::Info => info!("{}", error),
            ErrorSeverity::Warning => warn!("{}", error),
            ErrorSeverity::Error | ErrorSeverity::Fatal => error!("{}", error),
        }

        // Update category count
        *self.category_counts.entry(category).or_insert(0) += 1;

        // Trim history if needed
        if self.errors.len() >= MAX_ERROR_HISTORY {
            self.errors.remove(0);
        }

        self.errors.push(error);
        id
    }

    /// Mark a component as degraded (operating in fallback mode).
    pub fn mark_degraded(&mut self, component: impl Into<String>, error_id: usize) {
        let component = component.into();
        if !self.degraded_components.contains(&component) {
            self.degraded_components.push(component.clone());
            warn!("Component '{}' entered degraded mode", component);
        }

        // Link the error to the degraded component
        if let Some(error) = self.errors.iter_mut().find(|e| e.id == error_id) {
            error.degraded_component = Some(component);
        }
    }

    /// Mark a component as recovered (no longer degraded).
    pub fn mark_recovered(&mut self, component: &str) {
        self.degraded_components.retain(|c| c != component);
        info!("Component '{}' recovered from degraded mode", component);
    }

    /// Check if a component is currently degraded.
    pub fn is_degraded(&self, component: &str) -> bool {
        self.degraded_components.contains(&component.to_string())
    }

    /// Get all current degraded components.
    pub fn degraded_components(&self) -> &[String] {
        &self.degraded_components
    }

    /// Acknowledge an error (user has seen it).
    pub fn acknowledge(&mut self, error_id: usize) {
        if let Some(error) = self.errors.iter_mut().find(|e| e.id == error_id) {
            error.acknowledged = true;
        }
    }

    /// Get all unacknowledged errors.
    pub fn unacknowledged_errors(&self) -> Vec<&RecordedError> {
        self.errors.iter().filter(|e| !e.acknowledged).collect()
    }

    /// Get recent errors (last N errors).
    pub fn recent_errors(&self, count: usize) -> Vec<&RecordedError> {
        self.errors.iter().rev().take(count).collect::<Vec<_>>().into_iter().rev().collect()
    }

    /// Get errors by category.
    pub fn errors_by_category(&self, category: ErrorCategory) -> Vec<&RecordedError> {
        self.errors.iter().filter(|e| e.category == category).collect()
    }

    /// Get errors by severity.
    pub fn errors_by_severity(&self, severity: ErrorSeverity) -> Vec<&RecordedError> {
        self.errors.iter().filter(|e| e.severity == severity).collect()
    }

    /// Get count of errors by category.
    pub fn count_by_category(&self, category: ErrorCategory) -> usize {
        *self.category_counts.get(&category).unwrap_or(&0)
    }

    /// Check if there are any fatal errors.
    pub fn has_fatal_errors(&self) -> bool {
        self.errors.iter().any(|e| e.severity == ErrorSeverity::Fatal)
    }

    /// Get the most recent fatal error (if any).
    pub fn latest_fatal(&self) -> Option<&RecordedError> {
        self.errors.iter().rev().find(|e| e.severity == ErrorSeverity::Fatal)
    }

    /// Clear acknowledged errors older than the specified duration.
    pub fn cleanup_old_errors(&mut self, max_age_secs: u64) {
        let cutoff = Instant::now() - std::time::Duration::from_secs(max_age_secs);
        self.errors.retain(|e| !e.acknowledged || e.timestamp > cutoff);
    }

    /// Get total error count.
    pub fn total_errors(&self) -> usize {
        self.errors.len()
    }
}

/// Thread-safe error recovery manager wrapper.
#[derive(Debug, Clone)]
pub struct SharedErrorRecoveryManager {
    inner: Arc<Mutex<ErrorRecoveryManager>>,
}

impl Default for SharedErrorRecoveryManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedErrorRecoveryManager {
    /// Create a new shared error recovery manager.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ErrorRecoveryManager::new())),
        }
    }

    /// Record an error (thread-safe).
    pub fn record_error(
        &self,
        category: ErrorCategory,
        severity: ErrorSeverity,
        title: impl Into<String>,
        message: impl Into<String>,
        guidance: Vec<String>,
    ) -> usize {
        self.inner
            .lock()
            .map(|mut mgr| mgr.record_error(category, severity, title, message, guidance))
            .unwrap_or(0)
    }

    /// Check if a component is degraded.
    pub fn is_degraded(&self, component: &str) -> bool {
        self.inner
            .lock()
            .map(|mgr| mgr.is_degraded(component))
            .unwrap_or(false)
    }

    /// Mark a component as degraded.
    pub fn mark_degraded(&self, component: impl Into<String>, error_id: usize) {
        if let Ok(mut mgr) = self.inner.lock() {
            mgr.mark_degraded(component, error_id);
        }
    }

    /// Mark a component as recovered.
    pub fn mark_recovered(&self, component: &str) {
        if let Ok(mut mgr) = self.inner.lock() {
            mgr.mark_recovered(component);
        }
    }

    /// Get unacknowledged errors.
    pub fn unacknowledged_errors(&self) -> Vec<RecordedError> {
        self.inner
            .lock()
            .map(|mgr| mgr.unacknowledged_errors().into_iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get recent errors.
    pub fn recent_errors(&self, count: usize) -> Vec<RecordedError> {
        self.inner
            .lock()
            .map(|mgr| mgr.recent_errors(count).into_iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Check for fatal errors.
    pub fn has_fatal_errors(&self) -> bool {
        self.inner
            .lock()
            .map(|mgr| mgr.has_fatal_errors())
            .unwrap_or(false)
    }

    /// Get the latest fatal error.
    pub fn latest_fatal(&self) -> Option<RecordedError> {
        self.inner
            .lock()
            .ok()
            .and_then(|mgr| mgr.latest_fatal().cloned())
    }

    /// Acknowledge an error (user has seen it).
    pub fn acknowledge(&self, error_id: usize) {
        if let Ok(mut mgr) = self.inner.lock() {
            mgr.acknowledge(error_id);
        }
    }

    /// Get all current degraded components.
    pub fn degraded_components(&self) -> Vec<String> {
        self.inner
            .lock()
            .map(|mgr| mgr.degraded_components().to_vec())
            .unwrap_or_default()
    }
}

// ============================================================================
// Helper functions for common error scenarios
// ============================================================================

/// Create guidance for database locked errors.
pub fn db_locked_guidance() -> Vec<String> {
    vec![
        "Another FORGE instance may be running".to_string(),
        "Close other instances and try again".to_string(),
        "Check for stale lock files in ~/.forge/".to_string(),
    ]
}

/// Create guidance for invalid config errors.
pub fn invalid_config_guidance(path: &str) -> Vec<String> {
    vec![
        format!("Check YAML syntax in {}", path),
        "Run 'forge validate' to see detailed errors".to_string(),
        "Using default configuration for now".to_string(),
    ]
}

/// Create guidance for network timeout errors.
pub fn network_timeout_guidance() -> Vec<String> {
    vec![
        "Check your network connection".to_string(),
        "The API server may be experiencing issues".to_string(),
        "Try again in a few moments".to_string(),
    ]
}

/// Create guidance for worker crash errors.
pub fn worker_crash_guidance(worker_id: &str) -> Vec<String> {
    vec![
        format!("Check worker logs in ~/.forge/logs/{}", worker_id),
        "The worker may have run out of memory".to_string(),
        "Try restarting the worker".to_string(),
    ]
}

/// Create guidance for chat backend errors.
pub fn chat_backend_guidance() -> Vec<String> {
    vec![
        "Chat is unavailable - using hotkey-only mode".to_string(),
        "Press W for workers, T for tasks, C for costs".to_string(),
        "Check ~/.forge/logs/ for backend error details".to_string(),
        "Try ':restart-backend' command".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_error() {
        let mut mgr = ErrorRecoveryManager::new();

        let id = mgr.record_error(
            ErrorCategory::Database,
            ErrorSeverity::Error,
            "Database locked",
            "Cannot acquire database lock",
            db_locked_guidance(),
        );

        assert_eq!(id, 1);
        assert_eq!(mgr.total_errors(), 1);
        assert_eq!(mgr.count_by_category(ErrorCategory::Database), 1);
    }

    #[test]
    fn test_degraded_component() {
        let mut mgr = ErrorRecoveryManager::new();

        let id = mgr.record_error(
            ErrorCategory::Chat,
            ErrorSeverity::Error,
            "Chat unavailable",
            "Backend process crashed",
            chat_backend_guidance(),
        );

        mgr.mark_degraded("chat", id);

        assert!(mgr.is_degraded("chat"));
        assert!(!mgr.is_degraded("database"));

        mgr.mark_recovered("chat");
        assert!(!mgr.is_degraded("chat"));
    }

    #[test]
    fn test_fatal_errors() {
        let mut mgr = ErrorRecoveryManager::new();

        mgr.record_error(
            ErrorCategory::Terminal,
            ErrorSeverity::Fatal,
            "Terminal init failed",
            "Cannot initialize terminal",
            vec![],
        );

        assert!(mgr.has_fatal_errors());
        assert!(mgr.latest_fatal().is_some());
    }

    #[test]
    fn test_shared_manager() {
        let mgr = SharedErrorRecoveryManager::new();

        let id = mgr.record_error(
            ErrorCategory::Config,
            ErrorSeverity::Warning,
            "Config warning",
            "Using defaults",
            vec![],
        );

        assert!(id > 0);

        let errors = mgr.unacknowledged_errors();
        assert_eq!(errors.len(), 1);
    }
}
