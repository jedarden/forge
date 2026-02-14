//! FORGE internal performance metrics tracking.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

const WINDOW_SIZE: usize = 60;

#[derive(Debug, Clone)]
pub struct PerfMetrics {
    event_loop_latency: VecDeque<u64>,
    render_time: VecDeque<u64>,
    frame_timestamps: VecDeque<Instant>,
    db_query_times: VecDeque<u64>,
    worker_spawns: VecDeque<Instant>,
    worker_exits: VecDeque<Instant>,
    last_memory_rss: u64,
    memory_history: VecDeque<u64>,
    alerts: Vec<PerfAlert>,
    last_update: Instant,
    total_frames: u64,
    total_events: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PerfAlertType {
    SlowEventLoop,
    SlowRender,
    HighMemory,
    SlowDbQuery,
}

#[derive(Debug, Clone)]
pub struct PerfAlert {
    pub alert_type: PerfAlertType,
    pub message: String,
    pub timestamp: Instant,
    pub value: u64,
}

impl PerfAlert {
    pub fn new(alert_type: PerfAlertType, message: impl Into<String>, value: u64) -> Self {
        Self { alert_type, message: message.into(), timestamp: Instant::now(), value }
    }
    pub fn is_recent(&self) -> bool { self.timestamp.elapsed() < Duration::from_secs(10) }
}

impl Default for PerfMetrics {
    fn default() -> Self { Self::new() }
}

impl PerfMetrics {
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

    pub fn record_frame(&mut self, event_loop_us: u64, render_us: u64) {
        self.total_frames += 1;
        if self.event_loop_latency.len() >= WINDOW_SIZE { self.event_loop_latency.pop_front(); }
        self.event_loop_latency.push_back(event_loop_us);
        if self.render_time.len() >= WINDOW_SIZE { self.render_time.pop_front(); }
        self.render_time.push_back(render_us);
        if self.frame_timestamps.len() >= WINDOW_SIZE { self.frame_timestamps.pop_front(); }
        self.frame_timestamps.push_back(Instant::now());
        if event_loop_us > 16_667 {
            self.add_alert(PerfAlertType::SlowEventLoop,
                format!("Event loop latency {}ms exceeds 16ms target", event_loop_us / 1000), event_loop_us);
        }
        if render_us > 8_000 {
            self.add_alert(PerfAlertType::SlowRender,
                format!("Render time {}ms is slow", render_us / 1000), render_us);
        }
        self.last_update = Instant::now();
    }

    pub fn record_db_query(&mut self, query_us: u64) {
        if self.db_query_times.len() >= WINDOW_SIZE { self.db_query_times.pop_front(); }
        self.db_query_times.push_back(query_us);
        if query_us > 100_000 {
            self.add_alert(PerfAlertType::SlowDbQuery, format!("DB query took {}ms", query_us / 1000), query_us);
        }
    }

    pub fn record_event(&mut self) { self.total_events += 1; }
    pub fn record_worker_spawn(&mut self) {
        if self.worker_spawns.len() >= WINDOW_SIZE { self.worker_spawns.pop_front(); }
        self.worker_spawns.push_back(Instant::now());
    }
    pub fn record_worker_exit(&mut self) {
        if self.worker_exits.len() >= WINDOW_SIZE { self.worker_exits.pop_front(); }
        self.worker_exits.push_back(Instant::now());
    }

    pub fn update_memory(&mut self, rss_bytes: u64) {
        self.last_memory_rss = rss_bytes;
        if self.memory_history.len() >= WINDOW_SIZE { self.memory_history.pop_front(); }
        self.memory_history.push_back(rss_bytes);
        if rss_bytes > 500_000_000 {
            let prev_high = self.memory_history.iter().max().copied().unwrap_or(0);
            if rss_bytes > prev_high {
                self.add_alert(PerfAlertType::HighMemory, format!("Memory usage is {}MB", rss_bytes / 1_000_000), rss_bytes);
            }
        }
    }

    fn add_alert(&mut self, alert_type: PerfAlertType, message: String, value: u64) {
        self.alerts.retain(|a| a.alert_type != alert_type);
        self.alerts.push(PerfAlert::new(alert_type, message, value));
    }

    pub fn prune_alerts(&mut self) { self.alerts.retain(|a| a.is_recent()); }

    pub fn event_loop_samples(&self) -> Vec<u64> { self.event_loop_latency.iter().copied().collect() }
    pub fn render_time_samples(&self) -> Vec<u64> { self.render_time.iter().copied().collect() }
    pub fn memory_samples_mb(&self) -> Vec<u64> { self.memory_history.iter().map(|&b| b / 1_000_000).collect() }
    pub fn db_query_samples(&self) -> Vec<u64> { self.db_query_times.iter().copied().collect() }

    pub fn avg_event_loop_us(&self) -> u64 {
        if self.event_loop_latency.is_empty() { return 0; }
        self.event_loop_latency.iter().sum::<u64>() / self.event_loop_latency.len() as u64
    }
    pub fn p95_event_loop_us(&self) -> u64 { percentile(&self.event_loop_latency, 95) }
    pub fn p99_event_loop_us(&self) -> u64 { percentile(&self.event_loop_latency, 99) }
    pub fn avg_render_us(&self) -> u64 {
        if self.render_time.is_empty() { return 0; }
        self.render_time.iter().sum::<u64>() / self.render_time.len() as u64
    }
    pub fn p95_render_us(&self) -> u64 { percentile(&self.render_time, 95) }
    pub fn p99_render_us(&self) -> u64 { percentile(&self.render_time, 99) }

    pub fn current_fps(&self) -> f64 {
        if self.frame_timestamps.len() < 2 { return 0.0; }
        let first = self.frame_timestamps.front().unwrap();
        let last = self.frame_timestamps.back().unwrap();
        let elapsed = last.duration_since(*first).as_secs_f64();
        if elapsed > 0.0 { (self.frame_timestamps.len() as f64 - 1.0) / elapsed } else { 0.0 }
    }

    pub fn memory_rss(&self) -> u64 { self.last_memory_rss }
    pub fn memory_mb(&self) -> u64 { self.last_memory_rss / 1_000_000 }
    pub fn avg_db_query_us(&self) -> u64 {
        if self.db_query_times.is_empty() { return 0; }
        self.db_query_times.iter().sum::<u64>() / self.db_query_times.len() as u64
    }
    pub fn max_db_query_us(&self) -> u64 { self.db_query_times.iter().max().copied().unwrap_or(0) }
    pub fn recent_worker_spawns(&self) -> usize {
        let cutoff = Instant::now() - Duration::from_secs(60);
        self.worker_spawns.iter().filter(|&&t| t > cutoff).count()
    }
    pub fn recent_worker_exits(&self) -> usize {
        let cutoff = Instant::now() - Duration::from_secs(60);
        self.worker_exits.iter().filter(|&&t| t > cutoff).count()
    }
    pub fn total_frames(&self) -> u64 { self.total_frames }
    pub fn total_events(&self) -> u64 { self.total_events }
    pub fn alerts(&self) -> &[PerfAlert] { &self.alerts }
    pub fn has_alerts(&self) -> bool { self.alerts.iter().any(|a| a.is_recent()) }
    pub fn last_update(&self) -> Instant { self.last_update }
    pub fn is_60fps_capable(&self) -> bool { self.p95_event_loop_us() < 16_667 }

    pub fn health_status(&self) -> HealthStatus {
        let mut issues = 0;
        if self.p95_event_loop_us() > 16_667 { issues += 1; }
        if self.p95_render_us() > 8_000 { issues += 1; }
        if self.memory_mb() > 500 { issues += 1; }
        if self.max_db_query_us() > 100_000 { issues += 1; }
        match issues { 0 => HealthStatus::Excellent, 1 => HealthStatus::Good, 2 => HealthStatus::Fair, _ => HealthStatus::Poor }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus { Excellent, Good, Fair, Poor }

impl HealthStatus {
    pub fn label(&self) -> &'static str {
        match self { HealthStatus::Excellent => "Excellent", HealthStatus::Good => "Good", HealthStatus::Fair => "Fair", HealthStatus::Poor => "Poor" }
    }
}

fn percentile(samples: &VecDeque<u64>, p: u8) -> u64 {
    if samples.is_empty() { return 0; }
    let mut sorted: Vec<u64> = samples.iter().copied().collect();
    sorted.sort_unstable();
    sorted[((p as usize * sorted.len()) / 100).min(sorted.len() - 1)]
}

pub fn get_memory_rss() -> u64 {
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        if let Ok(contents) = fs::read_to_string("/proc/self/status") {
            for line in contents.lines() {
                if line.starts_with("VmRSS:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        if let Ok(kb) = parts[1].parse::<u64>() { return kb * 1024; }
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
    }
    #[test]
    fn test_record_frame() {
        let mut metrics = PerfMetrics::new();
        metrics.record_frame(5000, 2000);
        assert_eq!(metrics.total_frames(), 1);
    }
}
