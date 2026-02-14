//! FORGE internal performance metrics tracking.
//!
//! This module tracks performance metrics for the FORGE TUI itself, including:
//! - Event loop latency (time between frames)
//! - Render time per frame
//! - Memory usage (RSS)
//! - Database query times
//! - Worker spawn/exit rate
//!
//! Metrics are collected in a rolling window and displayed with sparklines.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Number of samples to keep in rolling windows.
const WINDOW_SIZE: usize = 60;

/// Performance metrics collected for FORGE's internal monitoring.
#[derive(Debug, Clone)]
pub struct PerfMetrics {
    /// Event loop latency samples (microseconds).
    event_loop_latency: VecDeque<u64>,
    /// Render time samples (microseconds).
    render_time: VecDeque<u64>,
    /// Frame timestamps for FPS calculation.
    frame_timestamps: VecDeque<Instant>,
    /// Database query time samples (microseconds).
    db_query_times: VecDeque<u64>,
    /// Worker spawn timestamps.
    worker_spawns: VecDeque<Instant>,
    /// Worker exit timestamps.
    worker_exits: VecDeque<Instant>,
    /// Last memory reading (bytes).
    last_memory_rss: u64,
    /// Memory usage history (bytes).
    memory_history: VecDeque<u64>,
    /// Performance alerts.
    alerts: Vec<PerfAlert>,
    /// Last update time.
    last_update: Instant,
    /// Total frames rendered.
    total_frames: u64,
    /// Total events processed.
    total_events: u64,
}

/// Performance alert types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PerfAlertType {
    /// Event loop latency exceeds threshold.
    SlowEventLoop,
    /// Render time exceeds threshold.
    SlowRender,
    /// Memory usage is high.
    HighMemory,
    /// Database query is slow.
    SlowDbQuery,
}

/// A performance alert.
#[derive(Debug, Clone)]
pub struct PerfAlert {
    /// Alert type.
    pub alert_type: PerfAlertType,
    /// Alert message.
    pub message: String,
    /// When the alert was triggered.
    pub timestamp: Instant,
    /// Metric value that triggered the alert.
    pub value: u64,
}

impl PerfAlert {
    /// Create a new performance alert.
    pub fn new(alert_type: PerfAlertType, message: impl Into<String>, value: u64) -> Self {
        Self {
            alert_type,
            message: message.into(),
            timestamp: Instant::now(),
            value,
        }
    }

    /// Check if this alert is still relevant (within last 10 seconds).
    pub fn is_recent(&self) -> bool {
        self.timestamp.elapsed() < Duration::from_secs(10)
    }
}

impl Default for PerfMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl PerfMetrics {
    /// Create a new performance metrics tracker.
    pub fn new() -> Self {
        Self {
            event_loop_latency: VecDeque::with_capacity(WINDOW_SIZE),
            render_time: VecDeque::with_capacity(WINDOW_SIZE),
            frame_timestamps: VecDeque::with_capacity(WINDOW_SIZE),
            db_query_times: VecDeque::with_capacity(WINDOW_SIZE),
            worker_spawns: VecDeque::with_capacity(WINDOW_SIZE),
            worker_exits: VecDeque::with_capacity(WINDOW_SIZE),
            last_memory_rss: 0,
            memory_history: VecDeque::with_capacity(WINDOW_SIZE),
            alerts: Vec::new(),
            last_update: Instant::now(),
            total_frames: 0,
            total_events: 0,
        }
    }

    /// Record a frame render with event loop latency.
    pub fn record_frame(&mut self, event_loop_us: u64, render_us: u64) {
        self.total_frames += 1;

        // Add to rolling windows
        if self.event_loop_latency.len() >= WINDOW_SIZE {
            self.event_loop_latency.pop_front();
        }
        self.event_loop_latency.push_back(event_loop_us);

        if self.render_time.len() >= WINDOW_SIZE {
            self.render_time.pop_front();
        }
        self.render_time.push_back(render_us);

        // Track frame timestamp for FPS
        if self.frame_timestamps.len() >= WINDOW_SIZE {
            self.frame_timestamps.pop_front();
        }
        self.frame_timestamps.push_back(Instant::now());

        // Check for performance alerts
        if event_loop_us > 16_667 {
            // > 16.67ms (60fps threshold)
            self.add_alert(PerfAlertType::SlowEventLoop,
                format!("Event loop latency {}ms exceeds 16ms target", event_loop_us / 1000),
                event_loop_us);
        }
        if render_us > 8_000 {
            // > 8ms (half frame budget)
            self.add_alert(PerfAlertType::SlowRender,
                format!("Render time {}ms is slow", render_us / 1000),
                render_us);
        }

        self.last_update = Instant::now();
    }

    /// Record a database query.
    pub fn record_db_query(&mut self, query_us: u64) {
        if self.db_query_times.len() >= WINDOW_SIZE {
            self.db_query_times.pop_front();
        }
        self.db_query_times.push_back(query_us);

        if query_us > 100_000 {
            // > 100ms
            self.add_alert(PerfAlertType::SlowDbQuery,
                format!("DB query took {}ms", query_us / 1000),
                query_us);
        }
    }

    /// Record an event being processed.
    pub fn record_event(&mut self) {
        self.total_events += 1;
    }

    /// Record a worker spawn.
    pub fn record_worker_spawn(&mut self) {
        if self.worker_spawns.len() >= WINDOW_SIZE {
            self.worker_spawns.pop_front();
        }
        self.worker_spawns.push_back(Instant::now());
    }

    /// Record a worker exit.
    pub fn record_worker_exit(&mut self) {
        if self.worker_exits.len() >= WINDOW_SIZE {
            self.worker_exits.pop_front();
        }
        self.worker_exits.push_back(Instant::now());
    }

    /// Update memory usage reading.
    pub fn update_memory(&mut self, rss_bytes: u64) {
        self.last_memory_rss = rss_bytes;

        if self.memory_history.len() >= WINDOW_SIZE {
            self.memory_history.pop_front();
        }
        self.memory_history.push_back(rss_bytes);

        // Alert on high memory (> 500MB)
        if rss_bytes > 500_000_000 {
            let mb = rss_bytes / 1_000_000;
            // Only alert if this is a new high
            let prev_high = self.memory_history.iter().max().copied().unwrap_or(0);
            if rss_bytes > prev_high {
                self.add_alert(PerfAlertType::HighMemory,
                    format!("Memory usage is {}MB", mb),
                    rss_bytes);
            }
        }
    }

    /// Add a performance alert.
    fn add_alert(&mut self, alert_type: PerfAlertType, message: String, value: u64) {
        // Deduplicate: only keep one alert of each type at a time
        self.alerts.retain(|a| a.alert_type != alert_type);
        self.alerts.push(PerfAlert::new(alert_type, message, value));
    }

    /// Clean up old alerts.
    pub fn prune_alerts(&mut self) {
        self.alerts.retain(|a| a.is_recent());
    }

    // ========== Getters ==========

    /// Get event loop latency samples as a vector.
    pub fn event_loop_samples(&self) -> Vec<u64> {
        self.event_loop_latency.iter().copied().collect()
    }

    /// Get render time samples as a vector.
    pub fn render_time_samples(&self) -> Vec<u64> {
        self.render_time.iter().copied().collect()
    }

    /// Get memory history samples as a vector (in MB).
    pub fn memory_samples_mb(&self) -> Vec<u64> {
        self.memory_history.iter().map(|&b| b / 1_000_000).collect()
    }

    /// Get database query time samples as a vector.
    pub fn db_query_samples(&self) -> Vec<u64> {
        self.db_query_times.iter().copied().collect()
    }

    /// Calculate average event loop latency (microseconds).
    pub fn avg_event_loop_us(&self) -> u64 {
        if self.event_loop_latency.is_empty() {
            return 0;
        }
        let sum: u64 = self.event_loop_latency.iter().sum();
        sum / self.event_loop_latency.len() as u64
    }

    /// Calculate 95th percentile event loop latency (microseconds).
    pub fn p95_event_loop_us(&self) -> u64 {
        percentile(&self.event_loop_latency, 95)
    }

    /// Calculate 99th percentile event loop latency (microseconds).
    pub fn p99_event_loop_us(&self) -> u64 {
        percentile(&self.event_loop_latency, 99)
    }

    /// Calculate average render time (microseconds).
    pub fn avg_render_us(&self) -> u64 {
        if self.render_time.is_empty() {
            return 0;
        }
        let sum: u64 = self.render_time.iter().sum();
        sum / self.render_time.len() as u64
    }

    /// Calculate 95th percentile render time (microseconds).
    pub fn p95_render_us(&self) -> u64 {
        percentile(&self.render_time, 95)
    }

    /// Calculate 99th percentile render time (microseconds).
    pub fn p99_render_us(&self) -> u64 {
        percentile(&self.render_time, 99)
    }

    /// Calculate current FPS (frames per second).
    pub fn current_fps(&self) -> f64 {
        if self.frame_timestamps.len() < 2 {
            return 0.0;
        }

        let first = self.frame_timestamps.front().unwrap();
        let last = self.frame_timestamps.back().unwrap();
        let elapsed = last.duration_since(*first).as_secs_f64();
        let frames = self.frame_timestamps.len() as f64 - 1.0;

        if elapsed > 0.0 {
            frames / elapsed
        } else {
            0.0
        }
    }

    /// Get current memory RSS (bytes).
    pub fn memory_rss(&self) -> u64 {
        self.last_memory_rss
    }

    /// Get memory usage in MB.
    pub fn memory_mb(&self) -> u64 {
        self.last_memory_rss / 1_000_000
    }

    /// Calculate average DB query time (microseconds).
    pub fn avg_db_query_us(&self) -> u64 {
        if self.db_query_times.is_empty() {
            return 0;
        }
        let sum: u64 = self.db_query_times.iter().sum();
        sum / self.db_query_times.len() as u64
    }

    /// Calculate max DB query time (microseconds).
    pub fn max_db_query_us(&self) -> u64 {
        self.db_query_times.iter().max().copied().unwrap_or(0)
    }

    /// Count worker spawns in the last minute.
    pub fn recent_worker_spawns(&self) -> usize {
        let cutoff = Instant::now() - Duration::from_secs(60);
        self.worker_spawns.iter().filter(|&&t| t > cutoff).count()
    }

    /// Count worker exits in the last minute.
    pub fn recent_worker_exits(&self) -> usize {
        let cutoff = Instant::now() - Duration::from_secs(60);
        self.worker_exits.iter().filter(|&&t| t > cutoff).count()
    }

    /// Get total frames rendered.
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Get total events processed.
    pub fn total_events(&self) -> u64 {
        self.total_events
    }

    /// Get active alerts.
    pub fn alerts(&self) -> &[PerfAlert] {
        &self.alerts
    }

    /// Check if there are any active alerts.
    pub fn has_alerts(&self) -> bool {
        self.alerts.iter().any(|a| a.is_recent())
    }

    /// Get last update time.
    pub fn last_update(&self) -> Instant {
        self.last_update
    }

    /// Check if event loop is meeting 60fps target.
    pub fn is_60fps_capable(&self) -> bool {
        self.p95_event_loop_us() < 16_667 // < 16.67ms
    }

    /// Get overall health status.
    pub fn health_status(&self) -> HealthStatus {
        let mut issues = 0;

        if self.p95_event_loop_us() > 16_667 {
            issues += 1;
        }
        if self.p95_render_us() > 8_000 {
            issues += 1;
        }
        if self.memory_mb() > 500 {
            issues += 1;
        }
        if self.max_db_query_us() > 100_000 {
            issues += 1;
        }

        match issues {
            0 => HealthStatus::Excellent,
            1 => HealthStatus::Good,
            2 => HealthStatus::Fair,
            _ => HealthStatus::Poor,
        }
    }
}

/// Health status indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Excellent,
    Good,
    Fair,
    Poor,
}

impl HealthStatus {
    /// Get a display label for the health status.
    pub fn label(&self) -> &'static str {
        match self {
            HealthStatus::Excellent => "Excellent",
            HealthStatus::Good => "Good",
            HealthStatus::Fair => "Fair",
            HealthStatus::Poor => "Poor",
        }
    }
}

/// Calculate percentile of a sorted collection.
fn percentile(samples: &VecDeque<u64>, p: u8) -> u64 {
    if samples.is_empty() {
        return 0;
    }

    let mut sorted: Vec<u64> = samples.iter().copied().collect();
    sorted.sort_unstable();

    let idx = ((p as usize * sorted.len()) / 100).min(sorted.len() - 1);
    sorted[idx]
}

/// Get current process memory RSS in bytes.
/// Uses /proc/self/status on Linux.
pub fn get_memory_rss() -> u64 {
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        if let Ok(contents) = fs::read_to_string("/proc/self/status") {
            for line in contents.lines() {
                if line.starts_with("VmRSS:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        if let Ok(kb) = parts[1].parse::<u64>() {
                            return kb * 1024; // Convert KB to bytes
                        }
                    }
                }
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perf_metrics_new() {
        let metrics = PerfMetrics::new();
        assert!(metrics.event_loop_samples().is_empty());
        assert!(metrics.render_time_samples().is_empty());
        assert_eq!(metrics.total_frames(), 0);
        assert_eq!(metrics.total_events(), 0);
    }

    #[test]
    fn test_record_frame() {
        let mut metrics = PerfMetrics::new();

        metrics.record_frame(5000, 2000);
        metrics.record_frame(8000, 3000);
        metrics.record_frame(12000, 4000);

        assert_eq!(metrics.total_frames(), 3);
        assert_eq!(metrics.avg_event_loop_us(), 8333);
        assert_eq!(metrics.avg_render_us(), 3000);
    }

    #[test]
    fn test_percentile() {
        let mut samples: VecDeque<u64> = VecDeque::new();
        for i in 1..=100 {
            samples.push_back(i);
        }

        assert_eq!(percentile(&samples, 50), 50);
        assert_eq!(percentile(&samples, 95), 95);
        assert_eq!(percentile(&samples, 99), 99);
    }

    #[test]
    fn test_percentile_empty() {
        let samples: VecDeque<u64> = VecDeque::new();
        assert_eq!(percentile(&samples, 50), 0);
    }

    #[test]
    fn test_fps_calculation() {
        let mut metrics = PerfMetrics::new();

        // Can't test FPS precisely without time passing, but can check it doesn't crash
        metrics.record_frame(1000, 500);
        let fps = metrics.current_fps();
        assert!(fps >= 0.0);
    }

    #[test]
    fn test_slow_event_loop_alert() {
        let mut metrics = PerfMetrics::new();

        // This should trigger an alert (> 16.67ms)
        metrics.record_frame(20_000, 1000);

        assert!(metrics.has_alerts());
        assert!(metrics.alerts().iter().any(|a| a.alert_type == PerfAlertType::SlowEventLoop));
    }

    #[test]
    fn test_health_status() {
        let mut metrics = PerfMetrics::new();

        // Record good performance
        for _ in 0..10 {
            metrics.record_frame(5000, 2000);
        }

        assert_eq!(metrics.health_status(), HealthStatus::Excellent);

        // Record poor performance
        for _ in 0..60 {
            metrics.record_frame(25_000, 15_000);
        }

        // Should be poor now
        assert_ne!(metrics.health_status(), HealthStatus::Excellent);
    }

    #[test]
    fn test_worker_tracking() {
        let mut metrics = PerfMetrics::new();

        metrics.record_worker_spawn();
        metrics.record_worker_spawn();
        metrics.record_worker_exit();

        assert_eq!(metrics.recent_worker_spawns(), 2);
        assert_eq!(metrics.recent_worker_exits(), 1);
    }

    #[test]
    fn test_db_query_tracking() {
        let mut metrics = PerfMetrics::new();

        metrics.record_db_query(5000);
        metrics.record_db_query(10000);
        metrics.record_db_query(15000);

        assert_eq!(metrics.avg_db_query_us(), 10000);
        assert_eq!(metrics.max_db_query_us(), 15000);
    }

    #[test]
    fn test_memory_tracking() {
        let mut metrics = PerfMetrics::new();

        metrics.update_memory(100_000_000);  // 100MB
        metrics.update_memory(150_000_000);  // 150MB

        assert_eq!(metrics.memory_mb(), 150);
        assert_eq!(metrics.memory_samples_mb(), vec![100, 150]);
    }

    #[test]
    fn test_alert_pruning() {
        let mut metrics = PerfMetrics::new();

        metrics.record_frame(25_000, 1000); // Triggers alert
        assert!(metrics.has_alerts());

        // Manually age the alert
        if let Some(alert) = metrics.alerts.first_mut() {
            // We can't modify timestamp directly, but we can test the logic
        }

        metrics.prune_alerts();
    }

    #[test]
    fn test_is_60fps_capable() {
        let mut metrics = PerfMetrics::new();

        // Record good performance
        for _ in 0..60 {
            metrics.record_frame(8000, 4000);
        }

        assert!(metrics.is_60fps_capable());

        // Clear and record poor performance
        let mut metrics = PerfMetrics::new();
        for _ in 0..60 {
            metrics.record_frame(25_000, 10000);
        }

        assert!(!metrics.is_60fps_capable());
    }
}
