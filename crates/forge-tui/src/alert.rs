//! Worker health alert system for FORGE.
//!
//! This module provides an alert system that tracks and displays notifications
//! when workers become unresponsive, crash, or exhibit degraded health.
//!
//! ## Alert Types
//!
//! - **Critical**: Worker crashed (process died, PID missing)
//! - **Warning**: Worker degraded (stale activity, stuck task)
//! - **Info**: Recovery events, auto-restart triggered
//!
//! ## Features
//!
//! - Alert badge count in header
//! - Alert summary in Overview panel
//! - Acknowledgment/dismiss system
//! - Auto-clear when worker recovers
//! - Sound notification (future)

use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Alert severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AlertSeverity {
    /// Informational (recovery, auto-restart)
    Info = 0,
    /// Warning (degraded performance, stale activity)
    Warning = 1,
    /// Critical (crashed, dead process)
    Critical = 2,
}

impl AlertSeverity {
    /// Get the icon for this severity level.
    pub fn icon(&self) -> &'static str {
        match self {
            AlertSeverity::Info => "ℹ",
            AlertSeverity::Warning => "⚠",
            AlertSeverity::Critical => "✖",
        }
    }

    /// Check if this severity should trigger a notification.
    pub fn should_notify(&self) -> bool {
        matches!(self, AlertSeverity::Critical | AlertSeverity::Warning)
    }
}

/// Types of worker health alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlertType {
    /// Worker process has died (PID check failed)
    WorkerCrashed,
    /// Worker process is a zombie
    WorkerZombie,
    /// Worker has no activity for too long
    WorkerStale,
    /// Worker task has been stuck for too long
    TaskStuck,
    /// Worker memory usage is too high
    MemoryHigh,
    /// Worker not responding to health checks
    WorkerUnresponsive,
    /// Auto-restart triggered
    AutoRestartTriggered,
    /// Recovery attempts exhausted
    RecoveryExhausted,
    /// Worker recovered (info only)
    WorkerRecovered,
    /// New worker spawned (info only)
    WorkerSpawned,
}

impl AlertType {
    /// Get the default severity for this alert type.
    pub fn default_severity(&self) -> AlertSeverity {
        match self {
            AlertType::WorkerCrashed
            | AlertType::WorkerZombie
            | AlertType::RecoveryExhausted => AlertSeverity::Critical,
            AlertType::WorkerStale
            | AlertType::TaskStuck
            | AlertType::WorkerUnresponsive
            | AlertType::AutoRestartTriggered => AlertSeverity::Warning,
            AlertType::MemoryHigh => AlertSeverity::Warning,
            AlertType::WorkerRecovered | AlertType::WorkerSpawned => AlertSeverity::Info,
        }
    }

    /// Get a human-readable title for this alert type.
    pub fn title(&self) -> &'static str {
        match self {
            AlertType::WorkerCrashed => "Worker Crashed",
            AlertType::WorkerZombie => "Worker Zombie Process",
            AlertType::WorkerStale => "Worker Stale Activity",
            AlertType::TaskStuck => "Task Stuck",
            AlertType::MemoryHigh => "Memory Usage High",
            AlertType::WorkerUnresponsive => "Worker Unresponsive",
            AlertType::AutoRestartTriggered => "Auto-Restart Triggered",
            AlertType::RecoveryExhausted => "Recovery Exhausted",
            AlertType::WorkerRecovered => "Worker Recovered",
            AlertType::WorkerSpawned => "Worker Spawned",
        }
    }

    /// Get the default message for this alert type.
    pub fn default_message(&self) -> &'static str {
        match self {
            AlertType::WorkerCrashed => "Worker process has terminated unexpectedly",
            AlertType::WorkerZombie => "Worker process is in zombie state",
            AlertType::WorkerStale => "No activity detected from worker",
            AlertType::TaskStuck => "Current task has been running too long",
            AlertType::MemoryHigh => "Memory usage exceeds threshold",
            AlertType::WorkerUnresponsive => "Worker not responding to health checks",
            AlertType::AutoRestartTriggered => "Worker will be restarted automatically",
            AlertType::RecoveryExhausted => "Max recovery attempts reached",
            AlertType::WorkerRecovered => "Worker health restored",
            AlertType::WorkerSpawned => "New worker started",
        }
    }
}

/// A single health alert.
#[derive(Debug, Clone)]
pub struct HealthAlert {
    /// Unique alert identifier
    pub id: u64,
    /// Type of alert
    pub alert_type: AlertType,
    /// Severity level
    pub severity: AlertSeverity,
    /// Worker ID this alert is for
    pub worker_id: String,
    /// Alert title
    pub title: String,
    /// Detailed message
    pub message: String,
    /// When the alert was first raised
    pub created_at: DateTime<Utc>,
    /// When the alert was last updated
    pub updated_at: DateTime<Utc>,
    /// Whether the alert has been acknowledged
    pub acknowledged: bool,
    /// Number of times this alert has occurred
    pub occurrence_count: u32,
    /// Whether this is an active alert (vs historical)
    pub is_active: bool,
}

impl HealthAlert {
    /// Create a new alert.
    pub fn new(
        id: u64,
        alert_type: AlertType,
        worker_id: impl Into<String>,
    ) -> Self {
        Self {
            id,
            alert_type,
            severity: alert_type.default_severity(),
            worker_id: worker_id.into(),
            title: alert_type.title().to_string(),
            message: alert_type.default_message().to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            acknowledged: false,
            occurrence_count: 1,
            is_active: true,
        }
    }

    /// Create with a custom message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// Create with a custom severity.
    pub fn with_severity(mut self, severity: AlertSeverity) -> Self {
        self.severity = severity;
        self
    }

    /// Acknowledge this alert.
    pub fn acknowledge(&mut self) {
        self.acknowledged = true;
        self.updated_at = Utc::now();
    }

    /// Mark this alert as resolved/inactive.
    pub fn resolve(&mut self) {
        self.is_active = false;
        self.updated_at = Utc::now();
    }

    /// Increment occurrence count (for deduplication).
    pub fn increment_occurrence(&mut self) {
        self.occurrence_count += 1;
        self.updated_at = Utc::now();
    }

    /// Format for display in one line.
    pub fn format_compact(&self) -> String {
        let ack_marker = if self.acknowledged { "✓" } else { " " };
        let count_marker = if self.occurrence_count > 1 {
            format!(" (x{})", self.occurrence_count)
        } else {
            String::new()
        };
        format!(
            "{} {} [{}] {}{}",
            ack_marker,
            self.severity.icon(),
            self.worker_id,
            self.title,
            count_marker
        )
    }

    /// Format for detailed display.
    pub fn format_detail(&self) -> String {
        let time = self.created_at.format("%H:%M:%S");
        let ack_status = if self.acknowledged { "acknowledged" } else { "active" };
        let count_info = if self.occurrence_count > 1 {
            format!(" (occurred {} times)", self.occurrence_count)
        } else {
            String::new()
        };

        format!(
            "[{}] {} {} - {}{}\n  {}",
            time,
            self.severity.icon(),
            self.worker_id,
            self.title,
            count_info,
            self.message
        )
    }
}

/// Alert manager that tracks and manages health alerts.
#[derive(Debug, Clone)]
pub struct AlertManager {
    /// All alerts by ID
    alerts: HashMap<u64, HealthAlert>,
    /// Active alerts by (worker_id, alert_type) for deduplication
    active_keys: HashMap<(String, AlertType), u64>,
    /// Next alert ID
    next_id: u64,
    /// Maximum active alerts to track
    max_active: usize,
    /// Count of unacknowledged critical alerts
    critical_count: usize,
    /// Count of unacknowledged warning alerts
    warning_count: usize,
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::new(100)
    }
}

impl AlertManager {
    /// Create a new alert manager with a maximum number of active alerts.
    pub fn new(max_active: usize) -> Self {
        Self {
            alerts: HashMap::new(),
            active_keys: HashMap::new(),
            next_id: 1,
            max_active,
            critical_count: 0,
            warning_count: 0,
        }
    }

    /// Raise a new alert or increment occurrence if duplicate.
    pub fn raise(
        &mut self,
        alert_type: AlertType,
        worker_id: impl Into<String>,
        message: Option<String>,
    ) -> u64 {
        let worker_id = worker_id.into();
        let key = (worker_id.clone(), alert_type);

        // Check for existing active alert
        if let Some(&existing_id) = self.active_keys.get(&key) {
            if let Some(alert) = self.alerts.get_mut(&existing_id) {
                if alert.is_active {
                    alert.increment_occurrence();
                    if let Some(msg) = message {
                        alert.message = msg;
                    }
                    return existing_id;
                }
            }
        }

        // Create new alert
        let id = self.next_id;
        self.next_id += 1;

        let mut alert = HealthAlert::new(id, alert_type, worker_id.clone());
        if let Some(msg) = message {
            alert = alert.with_message(msg);
        }

        // Update counts
        if !alert.acknowledged {
            match alert.severity {
                AlertSeverity::Critical => self.critical_count += 1,
                AlertSeverity::Warning => self.warning_count += 1,
                AlertSeverity::Info => {}
            }
        }

        // Store alert
        self.alerts.insert(id, alert);
        self.active_keys.insert(key, id);

        // Prune if needed
        if self.alerts.len() > self.max_active {
            self.prune_oldest_resolved();
        }

        id
    }

    /// Acknowledge an alert by ID.
    pub fn acknowledge(&mut self, alert_id: u64) -> bool {
        if let Some(alert) = self.alerts.get_mut(&alert_id) {
            if !alert.acknowledged {
                alert.acknowledge();
                // Update counts
                match alert.severity {
                    AlertSeverity::Critical => self.critical_count = self.critical_count.saturating_sub(1),
                    AlertSeverity::Warning => self.warning_count = self.warning_count.saturating_sub(1),
                    AlertSeverity::Info => {}
                }
            }
            return true;
        }
        false
    }

    /// Acknowledge all alerts for a worker.
    pub fn acknowledge_all_for_worker(&mut self, worker_id: &str) -> usize {
        let mut count = 0;
        for alert in self.alerts.values_mut() {
            if alert.worker_id == worker_id && !alert.acknowledged && alert.is_active {
                alert.acknowledge();
                count += 1;
                // Update counts
                match alert.severity {
                    AlertSeverity::Critical => self.critical_count = self.critical_count.saturating_sub(1),
                    AlertSeverity::Warning => self.warning_count = self.warning_count.saturating_sub(1),
                    AlertSeverity::Info => {}
                }
            }
        }
        count
    }

    /// Acknowledge all active alerts.
    pub fn acknowledge_all(&mut self) -> usize {
        let mut count = 0;
        for alert in self.alerts.values_mut() {
            if !alert.acknowledged && alert.is_active {
                alert.acknowledge();
                count += 1;
            }
        }
        self.critical_count = 0;
        self.warning_count = 0;
        count
    }

    /// Resolve (clear) an alert - called when worker recovers.
    pub fn resolve(&mut self, alert_id: u64) -> bool {
        if let Some(alert) = self.alerts.get_mut(&alert_id) {
            if alert.is_active {
                alert.resolve();
                // Remove from active keys
                let key = (alert.worker_id.clone(), alert.alert_type);
                self.active_keys.remove(&key);
                // Update counts
                if !alert.acknowledged {
                    match alert.severity {
                        AlertSeverity::Critical => self.critical_count = self.critical_count.saturating_sub(1),
                        AlertSeverity::Warning => self.warning_count = self.warning_count.saturating_sub(1),
                        AlertSeverity::Info => {}
                    }
                }
            }
            return true;
        }
        false
    }

    /// Resolve all alerts for a worker (called on recovery).
    pub fn resolve_all_for_worker(&mut self, worker_id: &str) -> usize {
        let mut count = 0;
        let ids: Vec<u64> = self
            .alerts
            .iter()
            .filter(|(_, a)| a.worker_id == worker_id && a.is_active)
            .map(|(id, _)| *id)
            .collect();

        for id in ids {
            if self.resolve(id) {
                count += 1;
            }
        }
        count
    }

    /// Get an alert by ID.
    pub fn get(&self, alert_id: u64) -> Option<&HealthAlert> {
        self.alerts.get(&alert_id)
    }

    /// Get all active alerts (unacknowledged and acknowledged).
    pub fn active_alerts(&self) -> Vec<&HealthAlert> {
        self.alerts
            .values()
            .filter(|a| a.is_active)
            .collect()
    }

    /// Get unacknowledged alerts only.
    pub fn unacknowledged_alerts(&self) -> Vec<&HealthAlert> {
        self.alerts
            .values()
            .filter(|a| a.is_active && !a.acknowledged)
            .collect()
    }

    /// Get alerts sorted by severity (critical first) and then by time.
    pub fn alerts_by_severity(&self) -> Vec<&HealthAlert> {
        let mut alerts: Vec<_> = self.active_alerts();
        alerts.sort_by(|a, b| {
            // Sort by: severity desc, then unacknowledged first, then time desc
            b.severity
                .cmp(&a.severity)
                .then(a.acknowledged.cmp(&b.acknowledged))
                .then(b.created_at.cmp(&a.created_at))
        });
        alerts
    }

    /// Get count of active alerts.
    pub fn active_count(&self) -> usize {
        self.alerts.values().filter(|a| a.is_active).count()
    }

    /// Get count of unacknowledged alerts.
    pub fn unacknowledged_count(&self) -> usize {
        self.alerts
            .values()
            .filter(|a| a.is_active && !a.acknowledged)
            .count()
    }

    /// Get count of critical unacknowledged alerts.
    pub fn critical_count(&self) -> usize {
        self.critical_count
    }

    /// Get count of warning unacknowledged alerts.
    pub fn warning_count(&self) -> usize {
        self.warning_count
    }

    /// Check if there are any active alerts.
    pub fn has_alerts(&self) -> bool {
        self.alerts.values().any(|a| a.is_active)
    }

    /// Check if there are any unacknowledged alerts.
    pub fn has_unacknowledged(&self) -> bool {
        self.critical_count > 0 || self.warning_count > 0
    }

    /// Get alert summary for badge display.
    pub fn badge_summary(&self) -> AlertBadge {
        AlertBadge {
            critical: self.critical_count,
            warning: self.warning_count,
            total: self.unacknowledged_count(),
        }
    }

    /// Clear all alerts (for testing/reset).
    pub fn clear(&mut self) {
        self.alerts.clear();
        self.active_keys.clear();
        self.critical_count = 0;
        self.warning_count = 0;
    }

    /// Prune oldest resolved alerts.
    fn prune_oldest_resolved(&mut self) {
        let mut resolved: Vec<_> = self
            .alerts
            .iter()
            .filter(|(_, a)| !a.is_active)
            .collect();
        resolved.sort_by_key(|(_, a)| a.created_at);

        let to_remove: Vec<u64> = resolved
            .into_iter()
            .take(self.alerts.len().saturating_sub(self.max_active))
            .map(|(id, _)| *id)
            .collect();

        for id in to_remove {
            self.alerts.remove(&id);
        }
    }
}

/// Summary badge for alert display.
#[derive(Debug, Clone, Copy, Default)]
pub struct AlertBadge {
    /// Critical alert count
    pub critical: usize,
    /// Warning alert count
    pub warning: usize,
    /// Total unacknowledged count
    pub total: usize,
}

impl AlertBadge {
    /// Check if badge should be displayed.
    pub fn should_display(&self) -> bool {
        self.total > 0
    }

    /// Format for display in header.
    pub fn format_header(&self) -> String {
        if self.critical > 0 {
            format!("⚠ {} critical | {} warning", self.critical, self.warning)
        } else if self.warning > 0 {
            format!("⚠ {} warning", self.warning)
        } else {
            String::new()
        }
    }

    /// Format compact badge (just count and icon).
    pub fn format_compact(&self) -> String {
        if self.critical > 0 {
            format!("⚠️{}", self.critical)
        } else if self.warning > 0 {
            format!("⚡{}", self.warning)
        } else {
            String::new()
        }
    }
}

/// Notification manager for alert sounds and visual feedback.
///
/// Handles terminal bell notifications and tracks cooldowns to avoid spam.
#[derive(Debug)]
pub struct AlertNotifier {
    /// Last time a bell was triggered
    last_bell: Option<std::time::Instant>,
    /// Minimum interval between bells (in seconds)
    bell_interval_secs: u64,
    /// Whether bell is enabled for critical alerts
    bell_on_critical: bool,
    /// Whether bell is enabled for warning alerts
    bell_on_warning: bool,
    /// Whether visual flash is enabled
    visual_flash_enabled: bool,
    /// Pending bell to be triggered on next render
    pending_bell: bool,
    /// Pending flash to be triggered on next render
    pending_flash: bool,
    /// Flash start time (for timing the flash duration)
    flash_start: Option<std::time::Instant>,
}

impl Default for AlertNotifier {
    fn default() -> Self {
        Self {
            last_bell: None,
            bell_interval_secs: 30,
            bell_on_critical: true,
            bell_on_warning: false,
            visual_flash_enabled: true,
            pending_bell: false,
            pending_flash: false,
            flash_start: None,
        }
    }
}

impl AlertNotifier {
    /// Create a new alert notifier with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure the notifier from settings.
    pub fn configure(
        &mut self,
        bell_on_critical: bool,
        bell_on_warning: bool,
        bell_interval_secs: u64,
        visual_flash_enabled: bool,
    ) {
        self.bell_on_critical = bell_on_critical;
        self.bell_on_warning = bell_on_warning;
        self.bell_interval_secs = bell_interval_secs;
        self.visual_flash_enabled = visual_flash_enabled;
    }

    /// Notify about a new alert (queues bell/flash if applicable).
    pub fn notify(&mut self, severity: AlertSeverity) {
        let should_bell = match severity {
            AlertSeverity::Critical => self.bell_on_critical,
            AlertSeverity::Warning => self.bell_on_warning,
            AlertSeverity::Info => false,
        };

        if should_bell && self.can_ring_bell() {
            self.pending_bell = true;
        }

        if self.visual_flash_enabled && severity == AlertSeverity::Critical {
            self.pending_flash = true;
            self.flash_start = Some(std::time::Instant::now());
        }
    }

    /// Check if enough time has passed to ring the bell again.
    fn can_ring_bell(&self) -> bool {
        match self.last_bell {
            None => true,
            Some(last) => {
                last.elapsed().as_secs() >= self.bell_interval_secs
            }
        }
    }

    /// Take and clear the pending bell flag, returning whether to ring the bell.
    ///
    /// Call this during the render loop to trigger the bell.
    pub fn take_pending_bell(&mut self) -> bool {
        if self.pending_bell {
            self.pending_bell = false;
            self.last_bell = Some(std::time::Instant::now());
            true
        } else {
            false
        }
    }

    /// Check if visual flash is currently active (within 200ms of alert).
    pub fn is_flashing(&self) -> bool {
        if let Some(start) = self.flash_start {
            start.elapsed().as_millis() < 200
        } else {
            false
        }
    }

    /// Clear the flash state after it has been displayed.
    pub fn clear_flash(&mut self) {
        self.pending_flash = false;
        // Keep flash_start for timing
    }

    /// Ring the terminal bell (BEL character).
    ///
    /// This outputs the ASCII BEL character (0x07) which causes most terminals
    /// to emit an audible beep or visual bell depending on terminal settings.
    pub fn ring_bell() {
        // Output BEL character (ASCII 7) to trigger terminal bell
        print!("\x07");
        // Flush to ensure it's sent immediately
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_severity_ordering() {
        assert!(AlertSeverity::Critical > AlertSeverity::Warning);
        assert!(AlertSeverity::Warning > AlertSeverity::Info);
    }

    #[test]
    fn test_alert_type_default_severity() {
        assert_eq!(
            AlertType::WorkerCrashed.default_severity(),
            AlertSeverity::Critical
        );
        assert_eq!(
            AlertType::WorkerStale.default_severity(),
            AlertSeverity::Warning
        );
        assert_eq!(
            AlertType::WorkerRecovered.default_severity(),
            AlertSeverity::Info
        );
    }

    #[test]
    fn test_health_alert_new() {
        let alert = HealthAlert::new(1, AlertType::WorkerCrashed, "worker-1");
        assert_eq!(alert.id, 1);
        assert_eq!(alert.worker_id, "worker-1");
        assert_eq!(alert.severity, AlertSeverity::Critical);
        assert!(!alert.acknowledged);
        assert!(alert.is_active);
        assert_eq!(alert.occurrence_count, 1);
    }

    #[test]
    fn test_health_alert_with_message() {
        let alert = HealthAlert::new(1, AlertType::WorkerCrashed, "worker-1")
            .with_message("Process 12345 not found");
        assert_eq!(alert.message, "Process 12345 not found");
    }

    #[test]
    fn test_health_alert_acknowledge() {
        let mut alert = HealthAlert::new(1, AlertType::WorkerCrashed, "worker-1");
        assert!(!alert.acknowledged);
        alert.acknowledge();
        assert!(alert.acknowledged);
    }

    #[test]
    fn test_alert_manager_new() {
        let manager = AlertManager::new(50);
        assert_eq!(manager.active_count(), 0);
        assert!(!manager.has_alerts());
    }

    #[test]
    fn test_alert_manager_raise() {
        let mut manager = AlertManager::new(100);

        let id = manager.raise(AlertType::WorkerCrashed, "worker-1", None);
        assert_eq!(id, 1);
        assert_eq!(manager.active_count(), 1);
        assert_eq!(manager.critical_count(), 1);
        assert!(manager.has_alerts());
        assert!(manager.has_unacknowledged());
    }

    #[test]
    fn test_alert_manager_raise_duplicate() {
        let mut manager = AlertManager::new(100);

        let id1 = manager.raise(AlertType::WorkerStale, "worker-1", None);
        let id2 = manager.raise(AlertType::WorkerStale, "worker-1", None);

        // Should return same ID for duplicate
        assert_eq!(id1, id2);
        assert_eq!(manager.active_count(), 1);

        // Occurrence count should be incremented
        let alert = manager.get(id1).unwrap();
        assert_eq!(alert.occurrence_count, 2);
    }

    #[test]
    fn test_alert_manager_acknowledge() {
        let mut manager = AlertManager::new(100);

        manager.raise(AlertType::WorkerCrashed, "worker-1", None);
        assert_eq!(manager.critical_count(), 1);

        manager.acknowledge(1);
        assert_eq!(manager.critical_count(), 0);
        assert!(manager.get(1).unwrap().acknowledged);
    }

    #[test]
    fn test_alert_manager_resolve() {
        let mut manager = AlertManager::new(100);

        manager.raise(AlertType::WorkerCrashed, "worker-1", None);
        assert!(manager.has_alerts());

        manager.resolve(1);
        assert!(!manager.has_alerts());
        assert!(!manager.get(1).unwrap().is_active);
    }

    #[test]
    fn test_alert_manager_resolve_all_for_worker() {
        let mut manager = AlertManager::new(100);

        manager.raise(AlertType::WorkerCrashed, "worker-1", None);
        manager.raise(AlertType::WorkerStale, "worker-1", None);
        manager.raise(AlertType::WorkerCrashed, "worker-2", None);

        assert_eq!(manager.active_count(), 3);

        let resolved = manager.resolve_all_for_worker("worker-1");
        assert_eq!(resolved, 2);
        assert_eq!(manager.active_count(), 1);
    }

    #[test]
    fn test_alert_badge() {
        let mut manager = AlertManager::new(100);

        // No alerts
        let badge = manager.badge_summary();
        assert!(!badge.should_display());

        // Add warning
        manager.raise(AlertType::WorkerStale, "worker-1", None);
        let badge = manager.badge_summary();
        assert!(badge.should_display());
        assert_eq!(badge.warning, 1);
        assert_eq!(badge.critical, 0);
        assert_eq!(badge.total, 1);

        // Add critical
        manager.raise(AlertType::WorkerCrashed, "worker-2", None);
        let badge = manager.badge_summary();
        assert_eq!(badge.critical, 1);
        assert_eq!(badge.warning, 1);
        assert_eq!(badge.total, 2);
    }

    #[test]
    fn test_alerts_by_severity() {
        let mut manager = AlertManager::new(100);

        manager.raise(AlertType::WorkerStale, "worker-1", None); // Warning
        manager.raise(AlertType::WorkerCrashed, "worker-2", None); // Critical
        manager.raise(AlertType::WorkerRecovered, "worker-3", None); // Info

        let alerts = manager.alerts_by_severity();
        assert_eq!(alerts.len(), 3);
        // Critical should be first
        assert_eq!(alerts[0].alert_type, AlertType::WorkerCrashed);
    }

    #[test]
    fn test_format_compact() {
        let alert = HealthAlert::new(1, AlertType::WorkerCrashed, "worker-1");
        let compact = alert.format_compact();
        assert!(compact.contains("✖"));
        assert!(compact.contains("worker-1"));
        assert!(compact.contains("Worker Crashed"));
    }

    #[test]
    fn test_alert_notifier_new() {
        let mut notifier = AlertNotifier::new();
        // Default: bell on critical enabled, bell on warning disabled
        assert!(!notifier.take_pending_bell());
        assert!(!notifier.is_flashing());
    }

    #[test]
    fn test_alert_notifier_notify_critical() {
        let mut notifier = AlertNotifier::new();
        // Configure to enable bell on critical
        notifier.configure(true, false, 30, true);

        // Notify critical alert
        notifier.notify(AlertSeverity::Critical);

        // Should have pending bell
        assert!(notifier.take_pending_bell());
        // Should be flashing
        assert!(notifier.is_flashing());
    }

    #[test]
    fn test_alert_notifier_notify_warning_disabled() {
        let mut notifier = AlertNotifier::new();
        // Configure: bell on critical only, warning disabled
        notifier.configure(true, false, 30, false);

        // Notify warning alert
        notifier.notify(AlertSeverity::Warning);

        // Should NOT have pending bell (warning disabled)
        assert!(!notifier.take_pending_bell());
    }

    #[test]
    fn test_alert_notifier_notify_warning_enabled() {
        let mut notifier = AlertNotifier::new();
        // Configure: bell on warning enabled
        notifier.configure(false, true, 30, false);

        // Notify warning alert
        notifier.notify(AlertSeverity::Warning);

        // Should have pending bell
        assert!(notifier.take_pending_bell());
    }

    #[test]
    fn test_alert_notifier_bell_interval() {
        let mut notifier = AlertNotifier::new();
        // Configure with 60 second interval
        notifier.configure(true, false, 60, false);

        // First notification - should ring
        notifier.notify(AlertSeverity::Critical);
        assert!(notifier.take_pending_bell());

        // Second notification immediately after - should NOT ring (within interval)
        notifier.notify(AlertSeverity::Critical);
        assert!(!notifier.take_pending_bell());
    }

    #[test]
    fn test_alert_notifier_info_does_not_bell() {
        let mut notifier = AlertNotifier::new();
        notifier.configure(true, true, 30, true);

        // Notify info alert
        notifier.notify(AlertSeverity::Info);

        // Should NOT have pending bell (info never bells)
        assert!(!notifier.take_pending_bell());
    }

    #[test]
    fn test_alert_notifier_clear_flash() {
        let mut notifier = AlertNotifier::new();
        notifier.configure(true, false, 30, true);

        // Trigger flash
        notifier.notify(AlertSeverity::Critical);
        assert!(notifier.is_flashing());

        // Clear flash
        notifier.clear_flash();
        // Flash is still "active" based on time, not pending_flash flag
        // The is_flashing() check is time-based (200ms)
    }
}
