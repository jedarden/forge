//! Chat history persistence module.
//!
//! This module provides functionality for persisting chat conversation history
//! to disk in JSONL format and restoring it on startup.
//!
//! ## File Format
//!
//! History is stored in `~/.forge/chat-history.jsonl` as newline-delimited JSON.
//! Each line represents a single chat exchange:
//!
//! ```json
//! {"timestamp":"2026-02-17T10:30:00Z","user_query":"How many workers?","assistant_response":"There are 3 active workers.","is_error":false,"metadata":{"duration_ms":150,"provider":"claude-api"}}
//! ```
//!
//! ## Commands
//!
//! - `/save` - Save current session history to disk
//! - `/load` - Load history from disk
//! - `/export [path]` - Export history to a file (default: ~/chat-export.json)
//! - `/clear` - Clear current session history

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::{info, warn};

use crate::error::{ChatError, Result};

/// Default history file path relative to forge directory.
const HISTORY_FILE: &str = "chat-history.jsonl";

/// Maximum number of exchanges to keep in history.
const MAX_HISTORY_SIZE: usize = 1000;

/// A single chat exchange for persistence.
///
/// This is a simplified version of the TUI's ChatExchange struct,
/// containing only the data needed for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Timestamp of the exchange.
    pub timestamp: String,
    /// User's query.
    pub user_query: String,
    /// Assistant's response.
    pub assistant_response: String,
    /// Whether this was an error response.
    pub is_error: bool,
    /// Optional metadata about the response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HistoryMetadata>,
    /// Tool calls made during this exchange.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<HistoryToolCall>,
    /// Session identifier (optional, for grouping).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Metadata about a chat response for history.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryMetadata {
    /// Duration in milliseconds.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub duration_ms: u64,
    /// Estimated cost in USD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Provider name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
}

fn is_zero(val: &u64) -> bool {
    *val == 0
}

/// Information about a tool call for history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryToolCall {
    /// Tool name.
    pub name: String,
    /// Whether the call succeeded.
    pub success: bool,
    /// Brief result message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Chat history manager for persistence operations.
pub struct HistoryManager {
    /// Path to the history file.
    history_path: PathBuf,
    /// Current session ID.
    session_id: String,
}

impl HistoryManager {
    /// Create a new history manager with default paths.
    pub fn new() -> Result<Self> {
        let forge_dir = Self::get_forge_dir()?;
        let history_path = forge_dir.join(HISTORY_FILE);

        // Generate session ID from current timestamp
        let session_id = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();

        Ok(Self {
            history_path,
            session_id,
        })
    }

    /// Create a history manager with a custom path (for testing).
    pub fn with_path(path: PathBuf) -> Self {
        let session_id = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
        Self {
            history_path: path,
            session_id,
        }
    }

    /// Get the forge directory path.
    fn get_forge_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| {
            ChatError::ConfigError("Could not determine home directory".to_string())
        })?;
        let forge_dir = home.join(".forge");

        // Ensure directory exists
        if !forge_dir.exists() {
            std::fs::create_dir_all(&forge_dir).map_err(|e| {
                ChatError::ConfigError(format!("Failed to create .forge directory: {}", e))
            })?;
        }

        Ok(forge_dir)
    }

    /// Get the current session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the history file path.
    pub fn history_path(&self) -> &PathBuf {
        &self.history_path
    }

    /// Save a single entry to the history file.
    pub async fn append_entry(&self, entry: &HistoryEntry) -> Result<()> {
        let mut entry = entry.clone();
        entry.session_id = Some(self.session_id.clone());

        let json = serde_json::to_string(&entry).map_err(|e| {
            ChatError::ConfigError(format!("Failed to serialize history entry: {}", e))
        })?;

        // Append to file
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.history_path)
            .map_err(|e| {
                ChatError::ConfigError(format!("Failed to open history file: {}", e))
            })?;

        writeln!(file, "{}", json).map_err(|e| {
            ChatError::ConfigError(format!("Failed to write history entry: {}", e))
        })?;

        info!("Appended entry to history: {}", self.history_path.display());
        Ok(())
    }

    /// Save multiple entries to the history file.
    pub async fn save_entries(&self, entries: &[HistoryEntry]) -> Result<usize> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.history_path)
            .map_err(|e| {
                ChatError::ConfigError(format!("Failed to open history file: {}", e))
            })?;

        let mut saved = 0;
        for entry in entries {
            let mut entry = entry.clone();
            entry.session_id = Some(self.session_id.clone());

            let json = serde_json::to_string(&entry).map_err(|e| {
                ChatError::ConfigError(format!("Failed to serialize history entry: {}", e))
            })?;

            writeln!(file, "{}", json).map_err(|e| {
                ChatError::ConfigError(format!("Failed to write history entry: {}", e))
            })?;
            saved += 1;
        }

        info!("Saved {} entries to history: {}", saved, self.history_path.display());
        Ok(saved)
    }

    /// Load all entries from the history file.
    pub async fn load_entries(&self) -> Result<Vec<HistoryEntry>> {
        if !self.history_path.exists() {
            info!("History file does not exist, returning empty history");
            return Ok(Vec::new());
        }

        let file = std::fs::File::open(&self.history_path).map_err(|e| {
            ChatError::ConfigError(format!("Failed to open history file: {}", e))
        })?;

        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        let mut line_num = 0;

        for line in reader.lines() {
            line_num += 1;
            let line = line.map_err(|e| {
                ChatError::ConfigError(format!("Failed to read history line {}: {}", line_num, e))
            })?;

            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<HistoryEntry>(&line) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    warn!("Skipping malformed history entry at line {}: {}", line_num, e);
                }
            }
        }

        // Truncate to max size (keep most recent)
        if entries.len() > MAX_HISTORY_SIZE {
            entries = entries.split_off(entries.len() - MAX_HISTORY_SIZE);
        }

        info!("Loaded {} entries from history", entries.len());
        Ok(entries)
    }

    /// Load recent entries (last N).
    pub async fn load_recent(&self, count: usize) -> Result<Vec<HistoryEntry>> {
        let entries = self.load_entries().await?;
        let start = entries.len().saturating_sub(count);
        Ok(entries[start..].to_vec())
    }

    /// Export history to a JSON file.
    pub async fn export_to_file(&self, path: Option<PathBuf>) -> Result<PathBuf> {
        let entries = self.load_entries().await?;

        let export_path = path.unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("chat-export.json")
        });

        let json = serde_json::to_string_pretty(&entries).map_err(|e| {
            ChatError::ConfigError(format!("Failed to serialize history for export: {}", e))
        })?;

        fs::write(&export_path, json).await.map_err(|e| {
            ChatError::ConfigError(format!("Failed to write export file: {}", e))
        })?;

        info!("Exported {} entries to {}", entries.len(), export_path.display());
        Ok(export_path)
    }

    /// Clear the history file.
    pub async fn clear(&self) -> Result<()> {
        if self.history_path.exists() {
            fs::remove_file(&self.history_path).await.map_err(|e| {
                ChatError::ConfigError(format!("Failed to remove history file: {}", e))
            })?;
            info!("Cleared history file: {}", self.history_path.display());
        }
        Ok(())
    }

    /// Get history statistics.
    pub async fn stats(&self) -> Result<HistoryStats> {
        let entries = self.load_entries().await?;

        let total_exchanges = entries.len();
        let error_count = entries.iter().filter(|e| e.is_error).count();
        let total_cost: f64 = entries
            .iter()
            .filter_map(|e| e.metadata.as_ref()?.cost_usd)
            .sum();
        let total_duration_ms: u64 = entries
            .iter()
            .filter_map(|e| e.metadata.as_ref().map(|m| m.duration_ms))
            .sum();

        // Count unique sessions
        let sessions: std::collections::HashSet<_> = entries
            .iter()
            .filter_map(|e| e.session_id.as_ref())
            .collect();

        Ok(HistoryStats {
            total_exchanges,
            error_count,
            success_rate: if total_exchanges > 0 {
                ((total_exchanges - error_count) as f64 / total_exchanges as f64) * 100.0
            } else {
                100.0
            },
            total_cost_usd: total_cost,
            total_duration_ms,
            session_count: sessions.len(),
        })
    }

    /// Compact the history file by removing duplicates and old entries.
    pub async fn compact(&self) -> Result<usize> {
        let entries = self.load_entries().await?;
        let original_count = entries.len();

        // For now, just truncate to max size
        let entries = if entries.len() > MAX_HISTORY_SIZE {
            entries[entries.len() - MAX_HISTORY_SIZE..].to_vec()
        } else {
            entries
        };

        // Rewrite the file
        let mut file = std::fs::File::create(&self.history_path).map_err(|e| {
            ChatError::ConfigError(format!("Failed to create history file: {}", e))
        })?;

        for entry in &entries {
            let json = serde_json::to_string(entry).map_err(|e| {
                ChatError::ConfigError(format!("Failed to serialize history entry: {}", e))
            })?;
            writeln!(file, "{}", json).map_err(|e| {
                ChatError::ConfigError(format!("Failed to write history entry: {}", e))
            })?;
        }

        let removed = original_count - entries.len();
        info!("Compacted history: removed {} entries", removed);
        Ok(removed)
    }
}

impl Default for HistoryManager {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| {
            Self {
                history_path: PathBuf::from("chat-history.jsonl"),
                session_id: chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string(),
            }
        })
    }
}

/// Statistics about the chat history.
#[derive(Debug, Clone, Default)]
pub struct HistoryStats {
    /// Total number of exchanges.
    pub total_exchanges: usize,
    /// Number of error responses.
    pub error_count: usize,
    /// Success rate as a percentage.
    pub success_rate: f64,
    /// Total estimated cost in USD.
    pub total_cost_usd: f64,
    /// Total duration in milliseconds.
    pub total_duration_ms: u64,
    /// Number of unique sessions.
    pub session_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_entry(query: &str, response: &str) -> HistoryEntry {
        HistoryEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            user_query: query.to_string(),
            assistant_response: response.to_string(),
            is_error: false,
            metadata: Some(HistoryMetadata {
                duration_ms: 100,
                cost_usd: Some(0.001),
                provider: "test".to_string(),
            }),
            tool_calls: vec![],
            session_id: None,
        }
    }

    #[tokio::test]
    async fn test_append_and_load_entry() {
        let temp_dir = TempDir::new().unwrap();
        let history_path = temp_dir.path().join("test-history.jsonl");
        let manager = HistoryManager::with_path(history_path);

        let entry = create_test_entry("Hello", "Hi there!");
        manager.append_entry(&entry).await.unwrap();

        let entries = manager.load_entries().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].user_query, "Hello");
        assert_eq!(entries[0].assistant_response, "Hi there!");
    }

    #[tokio::test]
    async fn test_save_multiple_entries() {
        let temp_dir = TempDir::new().unwrap();
        let history_path = temp_dir.path().join("test-history.jsonl");
        let manager = HistoryManager::with_path(history_path);

        let entries = vec![
            create_test_entry("Query 1", "Response 1"),
            create_test_entry("Query 2", "Response 2"),
            create_test_entry("Query 3", "Response 3"),
        ];

        let saved = manager.save_entries(&entries).await.unwrap();
        assert_eq!(saved, 3);

        let loaded = manager.load_entries().await.unwrap();
        assert_eq!(loaded.len(), 3);
    }

    #[tokio::test]
    async fn test_load_recent() {
        let temp_dir = TempDir::new().unwrap();
        let history_path = temp_dir.path().join("test-history.jsonl");
        let manager = HistoryManager::with_path(history_path);

        let entries: Vec<_> = (0..10)
            .map(|i| create_test_entry(&format!("Query {}", i), &format!("Response {}", i)))
            .collect();

        manager.save_entries(&entries).await.unwrap();

        let recent = manager.load_recent(3).await.unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].user_query, "Query 7");
        assert_eq!(recent[2].user_query, "Query 9");
    }

    #[tokio::test]
    async fn test_export_to_file() {
        let temp_dir = TempDir::new().unwrap();
        let history_path = temp_dir.path().join("test-history.jsonl");
        let export_path = temp_dir.path().join("export.json");
        let manager = HistoryManager::with_path(history_path);

        let entry = create_test_entry("Test", "Response");
        manager.append_entry(&entry).await.unwrap();

        let result_path = manager.export_to_file(Some(export_path.clone())).await.unwrap();
        assert_eq!(result_path, export_path);
        assert!(export_path.exists());

        // Verify JSON is valid
        let content = std::fs::read_to_string(&export_path).unwrap();
        let exported: Vec<HistoryEntry> = serde_json::from_str(&content).unwrap();
        assert_eq!(exported.len(), 1);
    }

    #[tokio::test]
    async fn test_clear_history() {
        let temp_dir = TempDir::new().unwrap();
        let history_path = temp_dir.path().join("test-history.jsonl");
        let manager = HistoryManager::with_path(history_path.clone());

        let entry = create_test_entry("Test", "Response");
        manager.append_entry(&entry).await.unwrap();
        assert!(history_path.exists());

        manager.clear().await.unwrap();
        assert!(!history_path.exists());
    }

    #[tokio::test]
    async fn test_stats() {
        let temp_dir = TempDir::new().unwrap();
        let history_path = temp_dir.path().join("test-history.jsonl");
        let manager = HistoryManager::with_path(history_path);

        let mut entries = vec![
            create_test_entry("Query 1", "Response 1"),
            create_test_entry("Query 2", "Response 2"),
        ];
        entries[1].is_error = true;

        manager.save_entries(&entries).await.unwrap();

        let stats = manager.stats().await.unwrap();
        assert_eq!(stats.total_exchanges, 2);
        assert_eq!(stats.error_count, 1);
        assert!((stats.success_rate - 50.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_handles_malformed_entries() {
        let temp_dir = TempDir::new().unwrap();
        let history_path = temp_dir.path().join("test-history.jsonl");

        // Write some valid and invalid entries
        let mut file = std::fs::File::create(&history_path).unwrap();
        writeln!(file, r#"{{"timestamp":"2026-01-01T00:00:00Z","user_query":"Good","assistant_response":"OK","is_error":false}}"#).unwrap();
        writeln!(file, "not valid json").unwrap();
        writeln!(file, r#"{{"timestamp":"2026-01-02T00:00:00Z","user_query":"Also good","assistant_response":"Fine","is_error":false}}"#).unwrap();

        let manager = HistoryManager::with_path(history_path);
        let entries = manager.load_entries().await.unwrap();

        // Should skip the malformed entry
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].user_query, "Good");
        assert_eq!(entries[1].user_query, "Also good");
    }
}
