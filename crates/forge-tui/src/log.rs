//! Log streaming and buffer management for the FORGE TUI.
//!
//! This module provides real-time log streaming from worker log files
//! using async file tailing and a ring buffer to prevent unbounded memory growth.
//!
//! ## Architecture (per ADR 0008)
//!
//! - **Async tail**: 100ms polling interval per log file
//! - **Ring buffer**: VecDeque<LogEntry> with configurable capacity (default: 1000)
//! - **Batch updates**: Flush N entries at once to UI
//! - **Log rotation handling**: Detect inode changes, reopen file
//!
//! ## Example
//!
//! ```no_run
//! use forge_tui::log::{LogBuffer, LogEntry, LogLevel};
//!
//! let mut buffer = LogBuffer::new(100);
//! buffer.push(LogEntry::new(LogLevel::Info, "Worker started".to_string()));
//! assert_eq!(buffer.len(), 1);
//! ```

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, warn};

/// Default ring buffer capacity (1000 log entries per ADR 0008).
pub const DEFAULT_BUFFER_CAPACITY: usize = 1000;

/// Default polling interval in milliseconds (100ms per ADR 0008).
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 100;

/// Default batch size for UI updates (10 entries per ADR 0008).
pub const DEFAULT_BATCH_SIZE: usize = 10;

/// Errors that can occur during log streaming.
#[derive(Error, Debug)]
pub enum LogError {
    /// Failed to open log file
    #[error("Failed to open log file {path}: {source}")]
    OpenFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to read log file
    #[error("Failed to read log file {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse log entry
    #[error("Failed to parse log entry: {message}")]
    ParseError { message: String },

    /// Log file was rotated (inode changed)
    #[error("Log file rotated: {path}")]
    FileRotated { path: PathBuf },
}

/// Result type for log operations.
pub type LogResult<T> = Result<T, LogError>;

/// Log severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Trace level (most verbose)
    Trace,
    /// Debug level
    Debug,
    /// Informational level
    #[default]
    Info,
    /// Warning level
    Warn,
    /// Error level
    Error,
}

impl LogLevel {
    /// Parse a log level from a string.
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "trace" => LogLevel::Trace,
            "debug" => LogLevel::Debug,
            "info" => LogLevel::Info,
            "warn" | "warning" => LogLevel::Warn,
            "error" | "err" => LogLevel::Error,
            _ => LogLevel::Info,
        }
    }

    /// Get the display symbol for this level.
    pub fn symbol(&self) -> &'static str {
        match self {
            LogLevel::Trace => "→",
            LogLevel::Debug => "○",
            LogLevel::Info => "●",
            LogLevel::Warn => "⚠",
            LogLevel::Error => "✖",
        }
    }

    /// Get the ANSI color code for this level.
    pub fn color(&self) -> &'static str {
        match self {
            LogLevel::Trace => "gray",
            LogLevel::Debug => "blue",
            LogLevel::Info => "green",
            LogLevel::Warn => "yellow",
            LogLevel::Error => "red",
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Trace => write!(f, "TRACE"),
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

/// A parsed log entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    /// Timestamp of the log entry
    #[serde(default = "default_timestamp")]
    pub timestamp: DateTime<Utc>,

    /// Log severity level
    #[serde(default)]
    pub level: LogLevel,

    /// Log message content
    pub message: String,

    /// Source of the log (worker ID, component, etc.)
    #[serde(default)]
    pub source: Option<String>,

    /// Target/module that generated the log
    #[serde(default)]
    pub target: Option<String>,

    /// Additional structured fields
    #[serde(default, flatten)]
    pub fields: std::collections::HashMap<String, serde_json::Value>,
}

fn default_timestamp() -> DateTime<Utc> {
    Utc::now()
}

impl LogEntry {
    /// Create a new log entry with the current timestamp.
    pub fn new(level: LogLevel, message: String) -> Self {
        Self {
            timestamp: Utc::now(),
            level,
            message,
            source: None,
            target: None,
            fields: std::collections::HashMap::new(),
        }
    }

    /// Create a log entry with a specific source.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Create a log entry with a specific target.
    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// Parse a log entry from a JSON line.
    pub fn from_json(line: &str) -> LogResult<Self> {
        serde_json::from_str(line).map_err(|e| LogError::ParseError {
            message: format!("Invalid JSON: {}", e),
        })
    }

    /// Parse a log entry from a simple text format.
    ///
    /// Expected format: `TIMESTAMP [LEVEL] MESSAGE`
    /// Example: `2026-02-08T14:23:45Z [INFO] Worker started`
    pub fn from_text(line: &str) -> LogResult<Self> {
        let line = line.trim();
        if line.is_empty() {
            return Err(LogError::ParseError {
                message: "Empty line".to_string(),
            });
        }

        // Try to parse timestamp at the beginning
        let (timestamp, rest) =
            if line.len() > 20 && line.chars().take(4).all(|c| c.is_ascii_digit()) {
                // Looks like ISO timestamp
                if let Some(space_idx) = line.find(' ') {
                    if let Ok(ts) = DateTime::parse_from_rfc3339(&line[..space_idx]) {
                        (ts.with_timezone(&Utc), &line[space_idx + 1..])
                    } else {
                        (Utc::now(), line)
                    }
                } else {
                    (Utc::now(), line)
                }
            } else {
                (Utc::now(), line)
            };

        // Try to parse level in brackets
        let (level, message) = if rest.starts_with('[') {
            if let Some(close_idx) = rest.find(']') {
                let level_str = &rest[1..close_idx];
                let msg = rest[close_idx + 1..].trim();
                (LogLevel::from_str(level_str), msg.to_string())
            } else {
                (LogLevel::Info, rest.to_string())
            }
        } else {
            (LogLevel::Info, rest.to_string())
        };

        Ok(Self {
            timestamp,
            level,
            message,
            source: None,
            target: None,
            fields: std::collections::HashMap::new(),
        })
    }

    /// Try to parse a log entry from a line, falling back gracefully.
    ///
    /// First tries JSON parsing, then text format, then creates a raw entry.
    pub fn parse(line: &str) -> Self {
        let line = line.trim();
        if line.is_empty() {
            return Self::new(LogLevel::Info, String::new());
        }

        // Try JSON first
        if line.starts_with('{') {
            if let Ok(entry) = Self::from_json(line) {
                return entry;
            }
        }

        // Try text format
        if let Ok(entry) = Self::from_text(line) {
            return entry;
        }

        // Fallback: treat entire line as message
        Self::new(LogLevel::Info, line.to_string())
    }

    /// Format the entry for display.
    pub fn format_display(&self) -> String {
        let ts = self.timestamp.with_timezone(&Local).format("%H:%M:%S");
        let source = self.source.as_deref().unwrap_or("");
        let source_prefix = if source.is_empty() {
            String::new()
        } else {
            format!("[{}] ", source)
        };

        format!(
            "{} {} {}{}",
            ts,
            self.level.symbol(),
            source_prefix,
            self.message
        )
    }
}

/// A ring buffer for log entries.
///
/// Uses VecDeque with a maximum capacity. When the buffer is full,
/// new entries push out the oldest entries.
#[derive(Debug, Clone)]
pub struct LogBuffer {
    /// The underlying buffer
    entries: VecDeque<LogEntry>,
    /// Maximum capacity
    capacity: usize,
    /// Total entries ever added (for stats)
    total_added: usize,
    /// Entries dropped due to capacity limits
    dropped_count: usize,
}

impl LogBuffer {
    /// Create a new log buffer with the specified capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity.min(10000)), // Cap allocation
            capacity,
            total_added: 0,
            dropped_count: 0,
        }
    }

    /// Create a new log buffer with default capacity (1000 entries).
    pub fn default_capacity() -> Self {
        Self::new(DEFAULT_BUFFER_CAPACITY)
    }

    /// Push a log entry to the buffer.
    ///
    /// If the buffer is at capacity, the oldest entry is removed.
    pub fn push(&mut self, entry: LogEntry) {
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
            self.dropped_count += 1;
        }
        self.entries.push_back(entry);
        self.total_added += 1;
    }

    /// Push multiple entries to the buffer.
    pub fn push_batch(&mut self, entries: impl IntoIterator<Item = LogEntry>) {
        for entry in entries {
            self.push(entry);
        }
    }

    /// Get the number of entries in the buffer.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get the buffer capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get the total number of entries ever added.
    pub fn total_added(&self) -> usize {
        self.total_added
    }

    /// Get the number of entries dropped due to capacity limits.
    pub fn dropped_count(&self) -> usize {
        self.dropped_count
    }

    /// Check if the buffer is at capacity.
    pub fn is_full(&self) -> bool {
        self.entries.len() >= self.capacity
    }

    /// Get an iterator over the entries (oldest first).
    pub fn iter(&self) -> impl Iterator<Item = &LogEntry> {
        self.entries.iter()
    }

    /// Get the last N entries (most recent).
    pub fn last_n(&self, n: usize) -> impl Iterator<Item = &LogEntry> {
        let skip = self.entries.len().saturating_sub(n);
        self.entries.iter().skip(skip)
    }

    /// Get entries filtered by log level.
    pub fn filter_level(&self, min_level: LogLevel) -> Vec<&LogEntry> {
        let min_ord = match min_level {
            LogLevel::Trace => 0,
            LogLevel::Debug => 1,
            LogLevel::Info => 2,
            LogLevel::Warn => 3,
            LogLevel::Error => 4,
        };

        self.entries
            .iter()
            .filter(|e| {
                let e_ord = match e.level {
                    LogLevel::Trace => 0,
                    LogLevel::Debug => 1,
                    LogLevel::Info => 2,
                    LogLevel::Warn => 3,
                    LogLevel::Error => 4,
                };
                e_ord >= min_ord
            })
            .collect()
    }

    /// Get entries filtered by source.
    pub fn filter_source(&self, source: &str) -> Vec<&LogEntry> {
        self.entries
            .iter()
            .filter(|e| e.source.as_deref() == Some(source))
            .collect()
    }

    /// Clear all entries from the buffer.
    pub fn clear(&mut self) {
        self.entries.clear();
        // Don't reset total_added or dropped_count for stats continuity
    }

    /// Get the oldest entry.
    pub fn oldest(&self) -> Option<&LogEntry> {
        self.entries.front()
    }

    /// Get the newest entry.
    pub fn newest(&self) -> Option<&LogEntry> {
        self.entries.back()
    }

    /// Convert to a Vec for serialization or display.
    pub fn to_vec(&self) -> Vec<LogEntry> {
        self.entries.iter().cloned().collect()
    }
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::default_capacity()
    }
}

/// Events emitted by the log tailer.
#[derive(Debug, Clone)]
pub enum LogEvent {
    /// New log entries received
    NewEntries(Vec<LogEntry>),

    /// Log file was rotated
    FileRotated { path: PathBuf },

    /// Error reading log file
    Error { path: PathBuf, error: String },

    /// Tailer started
    Started { path: PathBuf },

    /// Tailer stopped
    Stopped { path: PathBuf },
}

/// Configuration for the log tailer.
#[derive(Debug, Clone)]
pub struct LogTailerConfig {
    /// Path to the log file
    pub path: PathBuf,

    /// Polling interval in milliseconds
    pub poll_interval_ms: u64,

    /// Batch size for sending entries
    pub batch_size: usize,

    /// Whether to start from end of file (tail -f behavior)
    pub start_from_end: bool,

    /// Source identifier for log entries
    pub source: Option<String>,
}

impl Default for LogTailerConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            batch_size: DEFAULT_BATCH_SIZE,
            start_from_end: true,
            source: None,
        }
    }
}

impl LogTailerConfig {
    /// Create a new config for the given path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            ..Default::default()
        }
    }

    /// Set the polling interval.
    pub fn with_poll_interval_ms(mut self, ms: u64) -> Self {
        self.poll_interval_ms = ms;
        self
    }

    /// Set the batch size.
    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    /// Set whether to start from end of file.
    pub fn with_start_from_end(mut self, start_from_end: bool) -> Self {
        self.start_from_end = start_from_end;
        self
    }

    /// Set the source identifier.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

/// Synchronous log tailer for testing and simple use cases.
///
/// For production use, consider using an async implementation with tokio.
pub struct LogTailer {
    /// Configuration
    config: LogTailerConfig,

    /// Current file position
    position: u64,

    /// Sender for log events
    event_tx: Option<Sender<LogEvent>>,

    /// Current inode (for rotation detection)
    #[cfg(unix)]
    current_inode: Option<u64>,
}

impl LogTailer {
    /// Create a new log tailer with the given configuration.
    pub fn new(config: LogTailerConfig) -> Self {
        Self {
            config,
            position: 0,
            event_tx: None,
            #[cfg(unix)]
            current_inode: None,
        }
    }

    /// Create a log tailer for a specific file path.
    pub fn for_file(path: impl Into<PathBuf>) -> Self {
        Self::new(LogTailerConfig::new(path))
    }

    /// Set the event sender.
    pub fn with_event_sender(mut self, tx: Sender<LogEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Read new lines from the file and parse them.
    ///
    /// Returns parsed log entries or an empty vec if no new content.
    pub fn read_new_lines(&mut self) -> LogResult<Vec<LogEntry>> {
        use std::fs::File;
        use std::io::{BufRead, BufReader, Seek, SeekFrom};

        let path = &self.config.path;

        // Open file
        let file = File::open(path).map_err(|e| LogError::OpenFile {
            path: path.clone(),
            source: e,
        })?;

        // Check for rotation (inode change)
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = file.metadata().map_err(|e| LogError::ReadFile {
                path: path.clone(),
                source: e,
            })?;
            let inode = metadata.ino();

            if let Some(prev_inode) = self.current_inode {
                if inode != prev_inode {
                    // File rotated - reset position
                    debug!(path = ?path, old_inode = prev_inode, new_inode = inode, "Log file rotated");
                    self.position = 0;
                    self.current_inode = Some(inode);

                    if let Some(tx) = &self.event_tx {
                        let _ = tx.send(LogEvent::FileRotated { path: path.clone() });
                    }
                }
            } else {
                self.current_inode = Some(inode);
            }
        }

        let mut reader = BufReader::new(file);

        // Seek to last position (or end if starting fresh)
        if self.position == 0 && self.config.start_from_end {
            reader
                .seek(SeekFrom::End(0))
                .map_err(|e| LogError::ReadFile {
                    path: path.clone(),
                    source: e,
                })?;
            self.position = reader.stream_position().map_err(|e| LogError::ReadFile {
                path: path.clone(),
                source: e,
            })?;
            return Ok(Vec::new());
        }

        reader
            .seek(SeekFrom::Start(self.position))
            .map_err(|e| LogError::ReadFile {
                path: path.clone(),
                source: e,
            })?;

        // Read new lines
        let mut entries = Vec::new();
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let mut entry = LogEntry::parse(&line);

                    // Add source from config if not already set
                    if entry.source.is_none() {
                        entry.source = self.config.source.clone();
                    }

                    if !entry.message.is_empty() {
                        entries.push(entry);
                    }
                }
                Err(e) => {
                    warn!(path = ?path, error = %e, "Error reading log line");
                    break;
                }
            }
        }

        // Update position
        self.position = reader.stream_position().map_err(|e| LogError::ReadFile {
            path: path.clone(),
            source: e,
        })?;

        // Send event if we have entries
        if !entries.is_empty() {
            if let Some(tx) = &self.event_tx {
                let _ = tx.send(LogEvent::NewEntries(entries.clone()));
            }
        }

        Ok(entries)
    }

    /// Get the current file position.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Reset position to start of file.
    pub fn reset(&mut self) {
        self.position = 0;
        #[cfg(unix)]
        {
            self.current_inode = None;
        }
    }
}

/// Aggregate log buffer that combines logs from multiple sources.
#[derive(Debug, Default)]
pub struct AggregateLogBuffer {
    /// Combined buffer of all logs
    buffer: LogBuffer,

    /// Per-source buffers for filtering
    source_buffers: std::collections::HashMap<String, LogBuffer>,
}

impl AggregateLogBuffer {
    /// Create a new aggregate buffer with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: LogBuffer::new(capacity),
            source_buffers: std::collections::HashMap::new(),
        }
    }

    /// Push an entry to the aggregate buffer.
    pub fn push(&mut self, entry: LogEntry) {
        // Add to combined buffer
        self.buffer.push(entry.clone());

        // Add to source-specific buffer if source is set
        if let Some(source) = &entry.source {
            self.source_buffers
                .entry(source.clone())
                .or_insert_with(|| LogBuffer::new(100))
                .push(entry);
        }
    }

    /// Get the combined buffer.
    pub fn all(&self) -> &LogBuffer {
        &self.buffer
    }

    /// Get logs for a specific source.
    pub fn for_source(&self, source: &str) -> Option<&LogBuffer> {
        self.source_buffers.get(source)
    }

    /// Get list of sources.
    pub fn sources(&self) -> Vec<&String> {
        self.source_buffers.keys().collect()
    }

    /// Get total entry count.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Clear all buffers.
    pub fn clear(&mut self) {
        self.buffer.clear();
        for buffer in self.source_buffers.values_mut() {
            buffer.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::sync::mpsc;
    use tempfile::TempDir;

    // ============================================================
    // LogLevel Tests
    // ============================================================

    #[test]
    fn test_log_level_from_str() {
        assert_eq!(LogLevel::from_str("trace"), LogLevel::Trace);
        assert_eq!(LogLevel::from_str("DEBUG"), LogLevel::Debug);
        assert_eq!(LogLevel::from_str("Info"), LogLevel::Info);
        assert_eq!(LogLevel::from_str("warn"), LogLevel::Warn);
        assert_eq!(LogLevel::from_str("warning"), LogLevel::Warn);
        assert_eq!(LogLevel::from_str("error"), LogLevel::Error);
        assert_eq!(LogLevel::from_str("ERR"), LogLevel::Error);
        assert_eq!(LogLevel::from_str("unknown"), LogLevel::Info); // default
    }

    #[test]
    fn test_log_level_display() {
        assert_eq!(LogLevel::Info.to_string(), "INFO");
        assert_eq!(LogLevel::Error.to_string(), "ERROR");
    }

    #[test]
    fn test_log_level_symbol() {
        assert_eq!(LogLevel::Info.symbol(), "●");
        assert_eq!(LogLevel::Error.symbol(), "✖");
        assert_eq!(LogLevel::Warn.symbol(), "⚠");
    }

    // ============================================================
    // LogEntry Tests
    // ============================================================

    #[test]
    fn test_log_entry_new() {
        let entry = LogEntry::new(LogLevel::Info, "Test message".to_string());
        assert_eq!(entry.level, LogLevel::Info);
        assert_eq!(entry.message, "Test message");
        assert!(entry.source.is_none());
    }

    #[test]
    fn test_log_entry_with_source() {
        let entry = LogEntry::new(LogLevel::Info, "Test".to_string()).with_source("worker-1");
        assert_eq!(entry.source, Some("worker-1".to_string()));
    }

    #[test]
    fn test_log_entry_from_json() {
        let json = r#"{"level": "info", "message": "Hello world"}"#;
        let entry = LogEntry::from_json(json).unwrap();
        assert_eq!(entry.level, LogLevel::Info);
        assert_eq!(entry.message, "Hello world");
    }

    #[test]
    fn test_log_entry_from_json_with_timestamp() {
        let json = r#"{"timestamp": "2026-02-08T14:23:45Z", "level": "error", "message": "Something failed"}"#;
        let entry = LogEntry::from_json(json).unwrap();
        assert_eq!(entry.level, LogLevel::Error);
        assert_eq!(entry.message, "Something failed");
    }

    #[test]
    fn test_log_entry_from_text() {
        let text = "[INFO] Worker started successfully";
        let entry = LogEntry::from_text(text).unwrap();
        assert_eq!(entry.level, LogLevel::Info);
        assert_eq!(entry.message, "Worker started successfully");
    }

    #[test]
    fn test_log_entry_from_text_with_timestamp() {
        let text = "2026-02-08T14:23:45Z [WARN] Low memory";
        let entry = LogEntry::from_text(text).unwrap();
        assert_eq!(entry.level, LogLevel::Warn);
        assert_eq!(entry.message, "Low memory");
    }

    #[test]
    fn test_log_entry_parse_fallback() {
        // JSON
        let entry1 = LogEntry::parse(r#"{"level": "debug", "message": "test"}"#);
        assert_eq!(entry1.level, LogLevel::Debug);

        // Text with brackets
        let entry2 = LogEntry::parse("[ERROR] Failed");
        assert_eq!(entry2.level, LogLevel::Error);

        // Plain text
        let entry3 = LogEntry::parse("Just a plain message");
        assert_eq!(entry3.level, LogLevel::Info);
        assert_eq!(entry3.message, "Just a plain message");
    }

    #[test]
    fn test_log_entry_format_display() {
        let entry =
            LogEntry::new(LogLevel::Info, "Test message".to_string()).with_source("worker-1");
        let display = entry.format_display();
        assert!(display.contains("●"));
        assert!(display.contains("[worker-1]"));
        assert!(display.contains("Test message"));
    }

    // ============================================================
    // LogBuffer (Ring Buffer) Tests
    // ============================================================

    #[test]
    fn test_log_buffer_new() {
        let buffer = LogBuffer::new(100);
        assert_eq!(buffer.capacity(), 100);
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_log_buffer_push() {
        let mut buffer = LogBuffer::new(10);
        buffer.push(LogEntry::new(LogLevel::Info, "Message 1".to_string()));
        buffer.push(LogEntry::new(LogLevel::Info, "Message 2".to_string()));

        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.total_added(), 2);
        assert_eq!(buffer.dropped_count(), 0);
    }

    #[test]
    fn test_log_buffer_capacity_limit() {
        let mut buffer = LogBuffer::new(3);

        buffer.push(LogEntry::new(LogLevel::Info, "1".to_string()));
        buffer.push(LogEntry::new(LogLevel::Info, "2".to_string()));
        buffer.push(LogEntry::new(LogLevel::Info, "3".to_string()));

        assert_eq!(buffer.len(), 3);
        assert!(buffer.is_full());
        assert_eq!(buffer.dropped_count(), 0);

        // Push one more - should drop oldest
        buffer.push(LogEntry::new(LogLevel::Info, "4".to_string()));

        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.total_added(), 4);
        assert_eq!(buffer.dropped_count(), 1);

        // Verify oldest entry is gone
        let entries: Vec<_> = buffer.iter().collect();
        assert_eq!(entries[0].message, "2");
        assert_eq!(entries[2].message, "4");
    }

    #[test]
    fn test_log_buffer_ring_behavior() {
        let mut buffer = LogBuffer::new(5);

        // Add 10 entries - first 5 should be dropped
        for i in 1..=10 {
            buffer.push(LogEntry::new(LogLevel::Info, format!("Entry {}", i)));
        }

        assert_eq!(buffer.len(), 5);
        assert_eq!(buffer.total_added(), 10);
        assert_eq!(buffer.dropped_count(), 5);

        // Should have entries 6-10
        let messages: Vec<_> = buffer.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            messages,
            vec!["Entry 6", "Entry 7", "Entry 8", "Entry 9", "Entry 10"]
        );
    }

    #[test]
    fn test_log_buffer_oldest_newest() {
        let mut buffer = LogBuffer::new(10);

        buffer.push(LogEntry::new(LogLevel::Info, "First".to_string()));
        buffer.push(LogEntry::new(LogLevel::Info, "Last".to_string()));

        assert_eq!(buffer.oldest().unwrap().message, "First");
        assert_eq!(buffer.newest().unwrap().message, "Last");
    }

    #[test]
    fn test_log_buffer_last_n() {
        let mut buffer = LogBuffer::new(10);

        for i in 1..=5 {
            buffer.push(LogEntry::new(LogLevel::Info, format!("{}", i)));
        }

        let last_2: Vec<_> = buffer.last_n(2).map(|e| e.message.as_str()).collect();
        assert_eq!(last_2, vec!["4", "5"]);

        // Request more than available
        let last_10: Vec<_> = buffer.last_n(10).map(|e| e.message.as_str()).collect();
        assert_eq!(last_10.len(), 5);
    }

    #[test]
    fn test_log_buffer_filter_level() {
        let mut buffer = LogBuffer::new(10);

        buffer.push(LogEntry::new(LogLevel::Debug, "Debug".to_string()));
        buffer.push(LogEntry::new(LogLevel::Info, "Info".to_string()));
        buffer.push(LogEntry::new(LogLevel::Warn, "Warn".to_string()));
        buffer.push(LogEntry::new(LogLevel::Error, "Error".to_string()));

        let warns_and_above = buffer.filter_level(LogLevel::Warn);
        assert_eq!(warns_and_above.len(), 2);
        assert_eq!(warns_and_above[0].message, "Warn");
        assert_eq!(warns_and_above[1].message, "Error");
    }

    #[test]
    fn test_log_buffer_filter_source() {
        let mut buffer = LogBuffer::new(10);

        buffer.push(LogEntry::new(LogLevel::Info, "A".to_string()).with_source("worker-1"));
        buffer.push(LogEntry::new(LogLevel::Info, "B".to_string()).with_source("worker-2"));
        buffer.push(LogEntry::new(LogLevel::Info, "C".to_string()).with_source("worker-1"));

        let worker1_logs = buffer.filter_source("worker-1");
        assert_eq!(worker1_logs.len(), 2);
        assert_eq!(worker1_logs[0].message, "A");
        assert_eq!(worker1_logs[1].message, "C");
    }

    #[test]
    fn test_log_buffer_clear() {
        let mut buffer = LogBuffer::new(10);

        buffer.push(LogEntry::new(LogLevel::Info, "Test".to_string()));
        assert_eq!(buffer.len(), 1);

        buffer.clear();
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());

        // Stats should persist after clear
        assert_eq!(buffer.total_added(), 1);
    }

    #[test]
    fn test_log_buffer_push_batch() {
        let mut buffer = LogBuffer::new(10);

        let entries = vec![
            LogEntry::new(LogLevel::Info, "1".to_string()),
            LogEntry::new(LogLevel::Info, "2".to_string()),
            LogEntry::new(LogLevel::Info, "3".to_string()),
        ];

        buffer.push_batch(entries);
        assert_eq!(buffer.len(), 3);
    }

    // ============================================================
    // LogTailer Tests
    // ============================================================

    #[test]
    fn test_log_tailer_read_file() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.log");

        // Create log file with initial content
        {
            let mut file = fs::File::create(&log_path).unwrap();
            writeln!(file, "[INFO] Line 1").unwrap();
            writeln!(file, "[WARN] Line 2").unwrap();
        }

        let config = LogTailerConfig::new(&log_path).with_start_from_end(false); // Read from beginning

        let mut tailer = LogTailer::new(config);

        // Read all lines
        let entries = tailer.read_new_lines().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].level, LogLevel::Info);
        assert_eq!(entries[1].level, LogLevel::Warn);

        // Read again - should be empty (no new content)
        let entries = tailer.read_new_lines().unwrap();
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_log_tailer_start_from_end() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.log");

        // Create log file with existing content
        {
            let mut file = fs::File::create(&log_path).unwrap();
            writeln!(file, "[INFO] Old line 1").unwrap();
            writeln!(file, "[INFO] Old line 2").unwrap();
        }

        let config = LogTailerConfig::new(&log_path).with_start_from_end(true); // Skip existing content

        let mut tailer = LogTailer::new(config);

        // First read should skip existing content
        let entries = tailer.read_new_lines().unwrap();
        assert_eq!(entries.len(), 0);

        // Append new content
        {
            let mut file = fs::OpenOptions::new().append(true).open(&log_path).unwrap();
            writeln!(file, "[INFO] New line").unwrap();
        }

        // Now we should see the new line
        let entries = tailer.read_new_lines().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message, "New line");
    }

    #[test]
    fn test_log_tailer_source_assignment() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.log");

        {
            let mut file = fs::File::create(&log_path).unwrap();
            writeln!(file, "[INFO] Test message").unwrap();
        }

        let config = LogTailerConfig::new(&log_path)
            .with_source("worker-42")
            .with_start_from_end(false);

        let mut tailer = LogTailer::new(config);
        let entries = tailer.read_new_lines().unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source, Some("worker-42".to_string()));
    }

    #[test]
    fn test_log_tailer_streaming_simulation() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("streaming.log");

        // Create empty log file
        fs::File::create(&log_path).unwrap();

        // Test approach: write all entries to file first, then verify tailer can stream them
        // This avoids timing issues with file sync

        // Write all entries upfront
        {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .unwrap();
            for i in 1..=5 {
                writeln!(file, "[INFO] Worker message {}", i).unwrap();
            }
            file.sync_all().unwrap();
        }

        // Start tailer from beginning to capture all
        let config = LogTailerConfig::new(&log_path).with_start_from_end(false); // Read from beginning

        let mut tailer = LogTailer::new(config);
        let mut buffer = LogBuffer::new(100);

        // Read all entries
        let entries = tailer.read_new_lines().unwrap();
        buffer.push_batch(entries);

        // Verify all messages were captured
        assert_eq!(buffer.len(), 5);

        let messages: Vec<_> = buffer.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            messages,
            vec![
                "Worker message 1",
                "Worker message 2",
                "Worker message 3",
                "Worker message 4",
                "Worker message 5"
            ]
        );

        // Verify subsequent reads return empty (no new content)
        let entries = tailer.read_new_lines().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_log_tailer_with_event_channel() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("events.log");

        // Write test content to file first
        {
            let mut file = fs::File::create(&log_path).unwrap();
            writeln!(file, "[INFO] Event test").unwrap();
            file.sync_all().unwrap();
        }

        let (tx, rx) = mpsc::channel();

        // Read from beginning to capture the test entry
        let config = LogTailerConfig::new(&log_path).with_start_from_end(false);

        let mut tailer = LogTailer::new(config).with_event_sender(tx);

        // Read entries - should trigger event
        let entries = tailer.read_new_lines().unwrap();
        assert_eq!(entries.len(), 1, "Should have read 1 entry");

        // Event should be in channel
        let event = rx.try_recv().expect("Event should be in channel");
        match event {
            LogEvent::NewEntries(event_entries) => {
                assert_eq!(event_entries.len(), 1);
                assert_eq!(event_entries[0].message, "Event test");
            }
            _ => panic!("Expected NewEntries event"),
        }
    }

    #[test]
    fn test_log_tailer_reset() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("reset.log");

        {
            let mut file = fs::File::create(&log_path).unwrap();
            writeln!(file, "[INFO] Line 1").unwrap();
            writeln!(file, "[INFO] Line 2").unwrap();
        }

        let config = LogTailerConfig::new(&log_path).with_start_from_end(false);

        let mut tailer = LogTailer::new(config);

        // Read all
        let entries = tailer.read_new_lines().unwrap();
        assert_eq!(entries.len(), 2);

        // Reset
        tailer.reset();

        // Should read from beginning again
        let entries = tailer.read_new_lines().unwrap();
        assert_eq!(entries.len(), 2);
    }

    // ============================================================
    // AggregateLogBuffer Tests
    // ============================================================

    #[test]
    fn test_aggregate_buffer_multiple_sources() {
        let mut agg = AggregateLogBuffer::new(100);

        agg.push(LogEntry::new(LogLevel::Info, "A".to_string()).with_source("worker-1"));
        agg.push(LogEntry::new(LogLevel::Info, "B".to_string()).with_source("worker-2"));
        agg.push(LogEntry::new(LogLevel::Info, "C".to_string()).with_source("worker-1"));
        agg.push(LogEntry::new(LogLevel::Info, "D".to_string())); // No source

        assert_eq!(agg.len(), 4);
        assert_eq!(agg.sources().len(), 2);

        let worker1 = agg.for_source("worker-1").unwrap();
        assert_eq!(worker1.len(), 2);

        let worker2 = agg.for_source("worker-2").unwrap();
        assert_eq!(worker2.len(), 1);
    }

    // ============================================================
    // Integration Tests: Streaming Simulation
    // ============================================================

    #[test]
    fn test_integration_log_streaming_with_ring_buffer() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("worker.log");

        // Write all 10 entries upfront
        {
            let mut file = fs::File::create(&log_path).unwrap();
            for i in 1..=10 {
                writeln!(file, r#"{{"level": "info", "message": "Entry {}"}}"#, i).unwrap();
            }
            file.sync_all().unwrap();
        }

        // Small buffer to test ring behavior
        let mut buffer = LogBuffer::new(5);

        let config = LogTailerConfig::new(&log_path)
            .with_source("test-worker")
            .with_start_from_end(false); // Read from beginning

        let mut tailer = LogTailer::new(config);

        // Read all entries
        let entries = tailer.read_new_lines().unwrap();
        assert_eq!(entries.len(), 10, "Should read all 10 entries from file");

        // Push to buffer - ring buffer should keep only last 5
        buffer.push_batch(entries);

        // Buffer should only contain last 5 entries
        assert_eq!(buffer.len(), 5);
        assert_eq!(buffer.dropped_count(), 5);
        assert_eq!(buffer.total_added(), 10);

        let messages: Vec<_> = buffer.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            messages,
            vec!["Entry 6", "Entry 7", "Entry 8", "Entry 9", "Entry 10"]
        );

        // All entries should have source set
        for entry in buffer.iter() {
            assert_eq!(entry.source, Some("test-worker".to_string()));
        }
    }

    #[test]
    fn test_integration_mixed_log_formats() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("mixed.log");

        // Write mixed format logs
        {
            let mut file = fs::File::create(&log_path).unwrap();
            // JSON format
            writeln!(file, r#"{{"level": "info", "message": "JSON entry"}}"#).unwrap();
            // Text format with level
            writeln!(file, "[WARN] Text entry with level").unwrap();
            // Plain text
            writeln!(file, "Plain text message").unwrap();
            // JSON with extra fields
            writeln!(
                file,
                r#"{{"level": "error", "message": "With fields", "worker_id": "w-1"}}"#
            )
            .unwrap();
        }

        let config = LogTailerConfig::new(&log_path).with_start_from_end(false);

        let mut tailer = LogTailer::new(config);
        let entries = tailer.read_new_lines().unwrap();

        assert_eq!(entries.len(), 4);

        assert_eq!(entries[0].level, LogLevel::Info);
        assert_eq!(entries[0].message, "JSON entry");

        assert_eq!(entries[1].level, LogLevel::Warn);
        assert_eq!(entries[1].message, "Text entry with level");

        assert_eq!(entries[2].level, LogLevel::Info);
        assert_eq!(entries[2].message, "Plain text message");

        assert_eq!(entries[3].level, LogLevel::Error);
        assert_eq!(entries[3].message, "With fields");
    }
}
