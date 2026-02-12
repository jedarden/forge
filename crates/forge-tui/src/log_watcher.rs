//! Real-time log watching for API usage metrics extraction.
//!
//! This module provides file watching and parsing for worker log files
//! to extract API usage metrics (token counts, costs, model info) in real-time.
//!
//! ## Architecture
//!
//! - Uses `notify` crate for file system watching with debouncing
//! - Incremental parsing with file position tracking for efficiency
//! - Handles log rotation via inode tracking
//! - Emits parsed API calls via async channel for TUI integration
//!
//! ## Example
//!
//! ```no_run
//! use forge_tui::log_watcher::{LogWatcher, LogWatcherConfig};
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = LogWatcherConfig::default();
//!     let (watcher, mut rx) = LogWatcher::new(config).unwrap();
//!
//!     while let Some(event) = rx.recv().await {
//!         match event {
//!             forge_tui::log_watcher::LogWatcherEvent::ApiCallParsed { call } => {
//!                 println!("API call: {} tokens, cost ${:.4}",
//!                     call.input_tokens + call.output_tokens,
//!                     call.cost_usd);
//!             }
//!             _ => {}
//!         }
//!     }
//! }
//! ```

use chrono::{DateTime, Utc};
use forge_cost::{ApiCall, LogParser};
use notify::{Event as NotifyEvent, EventKind, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, new_debouncer};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, trace, warn};

/// Default log directory (~/.forge/logs/).
pub const DEFAULT_LOG_DIR: &str = ".forge/logs";

/// Default debounce duration in milliseconds.
pub const DEFAULT_DEBOUNCE_MS: u64 = 100;

/// Default poll interval for file reading in milliseconds.
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 500;

/// Default channel buffer size.
pub const DEFAULT_CHANNEL_BUFFER: usize = 256;

/// Errors that can occur during log watching.
#[derive(Error, Debug)]
pub enum LogWatcherError {
    /// Failed to initialize watcher
    #[error("Failed to initialize log watcher: {0}")]
    InitError(String),

    /// Failed to read log file
    #[error("Failed to read log file {path}: {source}")]
    ReadError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse log entry
    #[error("Failed to parse log entry: {0}")]
    ParseError(String),

    /// Channel error
    #[error("Channel error: {0}")]
    ChannelError(String),
}

/// Events emitted by the log watcher.
#[derive(Debug, Clone)]
pub enum LogWatcherEvent {
    /// An API call was parsed from the logs
    ApiCallParsed { call: ApiCall },

    /// A new log file was discovered
    FileDiscovered { path: PathBuf, worker_id: String },

    /// A log file was rotated
    FileRotated { path: PathBuf, worker_id: String },

    /// An error occurred
    Error { message: String },
}

/// Configuration for the log watcher.
#[derive(Debug, Clone)]
pub struct LogWatcherConfig {
    /// Directory to watch for log files
    pub log_dir: PathBuf,

    /// Debounce duration for file system events
    pub debounce_duration: Duration,

    /// Poll interval for reading new content
    pub poll_interval: Duration,

    /// Channel buffer size
    pub channel_buffer: usize,

    /// Whether to parse existing log files on startup
    pub parse_existing: bool,
}

impl Default for LogWatcherConfig {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        Self {
            log_dir: PathBuf::from(home).join(DEFAULT_LOG_DIR),
            debounce_duration: Duration::from_millis(DEFAULT_DEBOUNCE_MS),
            poll_interval: Duration::from_millis(DEFAULT_POLL_INTERVAL_MS),
            channel_buffer: DEFAULT_CHANNEL_BUFFER,
            parse_existing: false,
        }
    }
}

impl LogWatcherConfig {
    /// Create a new config with the given log directory.
    pub fn new(log_dir: PathBuf) -> Self {
        Self {
            log_dir,
            ..Default::default()
        }
    }

    /// Set the debounce duration.
    pub fn with_debounce(mut self, duration: Duration) -> Self {
        self.debounce_duration = duration;
        self
    }

    /// Set the poll interval.
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Set whether to parse existing files on startup.
    pub fn with_parse_existing(mut self, parse: bool) -> Self {
        self.parse_existing = parse;
        self
    }
}

/// Tracked file state for incremental parsing.
#[derive(Debug)]
struct TrackedFile {
    /// Current file position (bytes read so far)
    position: u64,

    /// File inode for rotation detection (Unix only)
    #[cfg(unix)]
    inode: Option<u64>,

    /// Worker ID extracted from filename
    worker_id: String,
}

/// Real-time log watcher that monitors log files and parses API usage metrics.
pub struct LogWatcher {
    /// Log directory being watched
    log_dir: PathBuf,

    /// Log parser for extracting API calls
    parser: LogParser,

    /// Tracked files with their positions
    tracked_files: HashMap<PathBuf, TrackedFile>,

    /// File tailer for reading new content
    tailer: FileTailer,
}

impl LogWatcher {
    /// Create a new log watcher with default configuration.
    pub fn new(config: LogWatcherConfig) -> Result<(Self, mpsc::Receiver<LogWatcherEvent>), LogWatcherError> {
        // Create the log directory if it doesn't exist
        if !config.log_dir.exists() {
            std::fs::create_dir_all(&config.log_dir).map_err(|e| {
                LogWatcherError::InitError(format!(
                    "Failed to create log directory {:?}: {}",
                    config.log_dir, e
                ))
            })?;
        }

        let (event_tx, event_rx) = mpsc::channel();

        let watcher = Self {
            log_dir: config.log_dir.clone(),
            parser: LogParser::new(),
            tracked_files: HashMap::new(),
            tailer: FileTailer::new(event_tx.clone()),
        };

        // Start file system watcher
        let log_dir_clone = config.log_dir.clone();
        let event_tx_clone = event_tx.clone();

        let mut debouncer = new_debouncer(
            config.debounce_duration,
            None,
            move |result: DebounceEventResult| {
                match result {
                    Ok(events) => {
                        for event in events {
                            if let Err(e) = process_file_event(&event.event, &log_dir_clone, &event_tx_clone) {
                                let _ = event_tx_clone.send(LogWatcherEvent::Error {
                                    message: e.to_string(),
                                });
                            }
                        }
                    }
                    Err(errors) => {
                        for error in errors {
                            warn!("File watcher error: {:?}", error);
                        }
                    }
                }
            },
        )
        .map_err(|e| LogWatcherError::InitError(format!("Failed to create debouncer: {}", e)))?;

        debouncer
            .watch(&config.log_dir, RecursiveMode::NonRecursive)
            .map_err(|e| {
                LogWatcherError::InitError(format!("Failed to watch directory {:?}: {}", config.log_dir, e))
            })?;

        // Keep debouncer alive by leaking it (simplified for sync context)
        // In production, this would be properly managed
        std::mem::forget(debouncer);

        Ok((watcher, event_rx))
    }

    /// Poll for new log content and parse it.
    ///
    /// This should be called regularly (e.g., in the TUI event loop).
    pub fn poll(&mut self) -> Vec<LogWatcherEvent> {
        let mut events = Vec::new();

        // Check for new log files
        if let Ok(entries) = std::fs::read_dir(&self.log_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("log") {
                    if !self.tracked_files.contains_key(&path) {
                        // New file discovered
                        let worker_id = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown")
                            .to_string();

                        self.tracked_files.insert(
                            path.clone(),
                            TrackedFile {
                                position: 0,
                                #[cfg(unix)]
                                inode: None,
                                worker_id: worker_id.clone(),
                            },
                        );

                        events.push(LogWatcherEvent::FileDiscovered { path, worker_id });
                    }
                }
            }
        }

        // Collect paths to process (avoid borrow issues)
        let paths: Vec<PathBuf> = self.tracked_files.keys().cloned().collect();

        // Read new content from tracked files
        for path in paths {
            if let Some(tracked) = self.tracked_files.get_mut(&path) {
                match Self::read_new_content_static(&path, tracked, &self.parser) {
                    Ok(calls) => {
                        for call in calls {
                            events.push(LogWatcherEvent::ApiCallParsed { call });
                        }
                    }
                    Err(e) => {
                        trace!("Error reading log file {:?}: {}", path, e);
                    }
                }
            }
        }

        events
    }

    /// Static version of read_new_content to avoid borrow checker issues.
    fn read_new_content_static(
        path: &Path,
        tracked: &mut TrackedFile,
        parser: &LogParser,
    ) -> Result<Vec<ApiCall>, LogWatcherError> {
        let mut file = File::open(path).map_err(|e| LogWatcherError::ReadError {
            path: path.to_path_buf(),
            source: e,
        })?;

        // Check for rotation (inode change on Unix)
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = file.metadata().map_err(|e| LogWatcherError::ReadError {
                path: path.to_path_buf(),
                source: e,
            })?;
            let current_inode = metadata.ino();

            if let Some(prev_inode) = tracked.inode {
                if current_inode != prev_inode {
                    // File rotated - reset position
                    debug!("Log file rotated: {:?}", path);
                    tracked.position = 0;
                }
            }
            tracked.inode = Some(current_inode);
        }

        // Seek to last position
        file.seek(SeekFrom::Start(tracked.position))
            .map_err(|e| LogWatcherError::ReadError {
                path: path.to_path_buf(),
                source: e,
            })?;

        let mut reader = BufReader::new(file);
        let mut calls = Vec::new();

        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let line = line.trim();
                    if line.is_empty() || !line.starts_with('{') {
                        continue;
                    }

                    // Parse the line for API usage
                    match parser.parse_line(line, &tracked.worker_id) {
                        Ok(Some(call)) => {
                            calls.push(call);
                        }
                        Ok(None) => {
                            // Line didn't contain usage data - skip
                        }
                        Err(e) => {
                            trace!("Failed to parse log line: {}", e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Error reading line from {:?}: {}", path, e);
                    break;
                }
            }
        }

        // Update position
        tracked.position = reader
            .stream_position()
            .map_err(|e| LogWatcherError::ReadError {
                path: path.to_path_buf(),
                source: e,
            })?;

        Ok(calls)
    }

    /// Get the log directory being watched.
    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }

    /// Get the number of tracked files.
    pub fn tracked_file_count(&self) -> usize {
        self.tracked_files.len()
    }
}

/// Simple file tailer for reading new lines from log files.
pub struct FileTailer {
    /// Event sender
    event_tx: Sender<LogWatcherEvent>,
}

impl FileTailer {
    /// Create a new file tailer.
    pub fn new(event_tx: Sender<LogWatcherEvent>) -> Self {
        Self { event_tx }
    }

    /// Tail a file, sending events for new lines.
    pub fn tail_file(&self, path: &Path, position: &mut u64, worker_id: &str) {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return,
        };

        let mut reader = BufReader::new(file);

        // Seek to position
        if reader.seek(SeekFrom::Start(*position)).is_err() {
            return;
        }

        let parser = LogParser::new();

        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let line = line.trim();
                    if line.is_empty() || !line.starts_with('{') {
                        continue;
                    }

                    if let Ok(Some(call)) = parser.parse_line(line, worker_id) {
                        let _ = self.event_tx.send(LogWatcherEvent::ApiCallParsed { call });
                    }
                }
                Err(_) => break,
            }
        }

        // Update position
        if let Ok(new_pos) = reader.stream_position() {
            *position = new_pos;
        }
    }
}

/// Process a file system event.
fn process_file_event(
    event: &NotifyEvent,
    log_dir: &Path,
    tx: &Sender<LogWatcherEvent>,
) -> Result<(), LogWatcherError> {
    for path in &event.paths {
        // Only process .log files
        if path.extension().and_then(|e| e.to_str()) != Some("log") {
            continue;
        }

        // Extract worker ID from filename
        let worker_id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };

        debug!("Processing log event {:?} for worker {}", event.kind, worker_id);

        match event.kind {
            EventKind::Create(_) => {
                let _ = tx.send(LogWatcherEvent::FileDiscovered {
                    path: path.clone(),
                    worker_id,
                });
            }
            EventKind::Modify(_) => {
                // Modifications are handled by polling
            }
            EventKind::Remove(_) => {
                // File removed - this could be rotation
                let _ = tx.send(LogWatcherEvent::FileRotated {
                    path: path.clone(),
                    worker_id,
                });
            }
            _ => {}
        }
    }

    Ok(())
}

/// Real-time metrics aggregator for parsed API calls.
#[derive(Debug, Default, Clone)]
pub struct RealtimeMetrics {
    /// Total API calls today
    pub total_calls: i64,

    /// Total cost today (USD)
    pub total_cost: f64,

    /// Total input tokens today
    pub total_input_tokens: i64,

    /// Total output tokens today
    pub total_output_tokens: i64,

    /// Calls per model
    pub calls_by_model: HashMap<String, i64>,

    /// Cost per model
    pub cost_by_model: HashMap<String, f64>,

    /// Last update timestamp
    pub last_update: Option<DateTime<Utc>>,
}

impl RealtimeMetrics {
    /// Create new empty metrics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an API call.
    pub fn record_call(&mut self, call: &ApiCall) {
        self.total_calls += 1;
        self.total_cost += call.cost_usd;
        self.total_input_tokens += call.input_tokens;
        self.total_output_tokens += call.output_tokens;

        *self.calls_by_model.entry(call.model.clone()).or_insert(0) += 1;
        *self.cost_by_model.entry(call.model.clone()).or_insert(0.0) += call.cost_usd;

        self.last_update = Some(Utc::now());
    }

    /// Get average cost per call.
    pub fn avg_cost_per_call(&self) -> f64 {
        if self.total_calls == 0 {
            0.0
        } else {
            self.total_cost / self.total_calls as f64
        }
    }

    /// Get total tokens.
    pub fn total_tokens(&self) -> i64 {
        self.total_input_tokens + self.total_output_tokens
    }

    /// Clear all metrics.
    pub fn clear(&mut self) {
        self.total_calls = 0;
        self.total_cost = 0.0;
        self.total_input_tokens = 0;
        self.total_output_tokens = 0;
        self.calls_by_model.clear();
        self.cost_by_model.clear();
        self.last_update = None;
    }

    /// Check if we have any data.
    pub fn has_data(&self) -> bool {
        self.total_calls > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_log_watcher_config_default() {
        let config = LogWatcherConfig::default();
        assert!(config.log_dir.to_str().unwrap().contains(".forge/logs"));
        assert_eq!(config.debounce_duration, Duration::from_millis(100));
        assert!(!config.parse_existing);
    }

    #[test]
    fn test_log_watcher_config_custom() {
        let config = LogWatcherConfig::new(PathBuf::from("/tmp/test"))
            .with_debounce(Duration::from_millis(200))
            .with_parse_existing(true);

        assert_eq!(config.log_dir, PathBuf::from("/tmp/test"));
        assert_eq!(config.debounce_duration, Duration::from_millis(200));
        assert!(config.parse_existing);
    }

    #[test]
    fn test_realtime_metrics() {
        let mut metrics = RealtimeMetrics::new();
        assert!(!metrics.has_data());
        assert_eq!(metrics.total_calls, 0);

        // Record a call
        let call = ApiCall::new(
            Utc::now(),
            "worker-1",
            "claude-opus",
            1000,
            500,
            0.05,
        );
        metrics.record_call(&call);

        assert!(metrics.has_data());
        assert_eq!(metrics.total_calls, 1);
        assert!((metrics.total_cost - 0.05).abs() < 0.001);
        assert_eq!(metrics.total_input_tokens, 1000);
        assert_eq!(metrics.total_output_tokens, 500);
        assert_eq!(*metrics.calls_by_model.get("claude-opus").unwrap(), 1);

        // Record another call
        let call2 = ApiCall::new(
            Utc::now(),
            "worker-2",
            "claude-sonnet",
            500,
            250,
            0.02,
        );
        metrics.record_call(&call2);

        assert_eq!(metrics.total_calls, 2);
        assert!((metrics.total_cost - 0.07).abs() < 0.001);
        assert_eq!(metrics.total_tokens(), 2250);
        assert!((metrics.avg_cost_per_call() - 0.035).abs() < 0.0001);

        // Clear
        metrics.clear();
        assert!(!metrics.has_data());
        assert_eq!(metrics.total_calls, 0);
    }

    #[test]
    fn test_log_watcher_creates_directory() {
        let tmp_dir = TempDir::new().unwrap();
        let log_dir = tmp_dir.path().join("logs");

        // Directory doesn't exist yet
        assert!(!log_dir.exists());

        let config = LogWatcherConfig::new(log_dir.clone());
        let (_watcher, _rx) = LogWatcher::new(config).unwrap();

        // Directory should now exist
        assert!(log_dir.exists());
    }

    #[test]
    fn test_log_watcher_detects_new_files() {
        let tmp_dir = TempDir::new().unwrap();
        let log_dir = tmp_dir.path().to_path_buf();

        let config = LogWatcherConfig::new(log_dir.clone()).with_parse_existing(false);
        let (mut watcher, _rx) = LogWatcher::new(config).unwrap();

        // Initially no tracked files
        assert_eq!(watcher.tracked_file_count(), 0);

        // Create a log file
        let log_path = log_dir.join("test-worker.log");
        let mut file = fs::File::create(&log_path).unwrap();
        writeln!(file, r#"{{"type":"system","message":"init"}}"#).unwrap();

        // Poll should detect the new file
        let events = watcher.poll();
        assert_eq!(watcher.tracked_file_count(), 1);

        // Should have FileDiscovered event
        let discovered = events.iter().any(|e| matches!(e, LogWatcherEvent::FileDiscovered { .. }));
        assert!(discovered);
    }

    #[test]
    fn test_log_watcher_parses_api_calls() {
        let tmp_dir = TempDir::new().unwrap();
        let log_dir = tmp_dir.path().to_path_buf();

        let config = LogWatcherConfig::new(log_dir.clone()).with_parse_existing(false);
        let (mut watcher, _rx) = LogWatcher::new(config).unwrap();

        // Create a log file with API usage data
        let log_path = log_dir.join("worker-1.log");
        let mut file = fs::File::create(&log_path).unwrap();

        // Write a result event with usage data (matching real log format)
        // Format from parser tests: total_cost_usd at top level, usage has token counts
        let result_line = r#"{"type":"result","total_cost_usd":0.01,"usage":{"input_tokens":100,"output_tokens":50}}"#;
        writeln!(file, "{}", result_line).unwrap();

        // Sync to ensure content is written to disk
        file.sync_all().unwrap();

        // First poll should discover file AND read content
        let events = watcher.poll();

        // Should have at least one ApiCallParsed event on first poll
        let parsed = events.iter().any(|e| matches!(e, LogWatcherEvent::ApiCallParsed { .. }));
        assert!(parsed, "Expected at least one ApiCallParsed event, got: {:?}", events);
    }

    #[test]
    fn test_log_watcher_handles_malformed_entries() {
        let tmp_dir = TempDir::new().unwrap();
        let log_dir = tmp_dir.path().to_path_buf();

        let config = LogWatcherConfig::new(log_dir.clone()).with_parse_existing(false);
        let (mut watcher, _rx) = LogWatcher::new(config).unwrap();

        // Create a log file with mixed content
        let log_path = log_dir.join("worker.log");
        let mut file = fs::File::create(&log_path).unwrap();

        writeln!(file, "not json at all").unwrap();
        writeln!(file, "{}", r#"{"type":"system"}"#).unwrap();
        // Valid result with usage data (matching real log format)
        writeln!(file, "{}", r#"{"type":"result","total_cost_usd":0.01,"usage":{"input_tokens":100,"output_tokens":50}}"#).unwrap();
        writeln!(file, "{}", r#"{"malformed":"json""#).unwrap(); // Invalid JSON
        file.sync_all().unwrap();

        // First poll discovers file and reads content
        let events = watcher.poll();

        // Should still parse the valid entry
        let parsed_count = events
            .iter()
            .filter(|e| matches!(e, LogWatcherEvent::ApiCallParsed { .. }))
            .count();
        assert!(parsed_count >= 1, "Expected at least one parsed call, got {} events: {:?}", parsed_count, events);
    }

    #[test]
    fn test_log_watcher_incremental_parsing() {
        let tmp_dir = TempDir::new().unwrap();
        let log_dir = tmp_dir.path().to_path_buf();

        let config = LogWatcherConfig::new(log_dir.clone()).with_parse_existing(false);
        let (mut watcher, _rx) = LogWatcher::new(config).unwrap();

        // Create a log file
        let log_path = log_dir.join("worker.log");
        let mut file = fs::File::create(&log_path).unwrap();

        // Write first entry (matching real log format)
        let line1 = r#"{"type":"result","total_cost_usd":0.01,"usage":{"input_tokens":100,"output_tokens":50}}"#;
        write!(file, "{}\n", line1).unwrap();
        file.sync_all().unwrap();

        // First poll discovers file and reads first entry
        let events1 = watcher.poll();
        let parsed1 = events1
            .iter()
            .filter(|e| matches!(e, LogWatcherEvent::ApiCallParsed { .. }))
            .count();
        assert_eq!(parsed1, 1, "Expected 1 parsed call, got {} events: {:?}", parsed1, events1);

        // Append second entry
        let mut file = fs::OpenOptions::new().append(true).open(&log_path).unwrap();
        let line2 = r#"{"type":"result","total_cost_usd":0.02,"usage":{"input_tokens":200,"output_tokens":100}}"#;
        write!(file, "{}\n", line2).unwrap();
        file.sync_all().unwrap();

        // Poll again - should only get new entry
        let events2 = watcher.poll();
        let parsed2 = events2
            .iter()
            .filter(|e| matches!(e, LogWatcherEvent::ApiCallParsed { .. }))
            .count();
        assert_eq!(parsed2, 1, "Expected 1 new parsed call, got {} events: {:?}", parsed2, events2);

        // Poll again - no new content
        let events3 = watcher.poll();
        let parsed3 = events3
            .iter()
            .filter(|e| matches!(e, LogWatcherEvent::ApiCallParsed { .. }))
            .count();
        assert_eq!(parsed3, 0);
    }
}
