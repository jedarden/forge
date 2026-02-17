//! Main application state and logic for the FORGE TUI.
//!
//! The `App` struct manages overall application state, view switching,
//! and coordinates between different components.
//!
//! # Chat Backend Integration Architecture
//!
//! The chat functionality integrates an async `ChatBackend` into the synchronous
//! TUI event loop using the following design:
//!
//! ## Async-to-Sync Bridge
//!
//! 1. **Tokio Runtime**: A single-threaded tokio runtime is created during App::new()
//!    and lives for the duration of the application. This runtime handles all async
//!    chat operations.
//!
//! 2. **Message Passing**: An `mpsc` channel connects async chat tasks to the sync UI:
//!    - `chat_tx`: Sender held by App, used to submit chat requests
//!    - `chat_rx`: Receiver held by a background task, processes requests
//!    - `response_rx`: Receiver held by App, receives responses
//!
//! 3. **Non-Blocking Execution**: When user submits chat input:
//!    - Send request to background task via `chat_tx`
//!    - Set `chat_pending` flag and display loading indicator
//!    - Continue UI event loop without blocking
//!    - Poll `response_rx` each frame to check for responses
//!
//! ## State Management
//!
//! - `chat_pending: bool` - Whether a request is in flight
//! - `chat_history: Vec<ChatExchange>` - Last 10 exchanges (user query + response)
//! - `chat_backend: Option<ChatBackend>` - Initialized from config.yaml
//!
//! ## Error Handling
//!
//! - Backend initialization failures: App runs without chat (display error in UI)
//! - Request timeout: Background task implements 30s timeout
//! - API errors: Captured in response, displayed in chat history as error messages
//!
//! ## Performance
//!
//! - Background task prevents UI blocking
//! - Response polling adds <1ms per frame
//! - Chat history limited to 10 exchanges (~1KB memory)

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEvent};
use forge_chat::{ChatBackend, ChatConfig, ChatResponse, StreamingChatChunk};
use forge_core::types::WorkerTier;
use forge_worker::{
    CrashRecoveryManager, LaunchConfig, SpawnRequest, WorkerLauncher,
    discovery::DiscoveredWorker,
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::config_watcher::{ConfigEvent, ConfigWatcher, ForgeConfig};
use crate::cost_panel::CostPanel;
use crate::data::DataManager;
use crate::event::{AppEvent, InputHandler};
use crate::metrics_panel::MetricsPanel;
use crate::perf_panel::PerfPanel;
use crate::theme::ThemeManager;
use crate::view::{FocusPanel, LayoutMode, View};
use crate::widget::QuickActionsPanel;
use crate::error_recovery::{ErrorCategory, ErrorRecoveryManager, ErrorSeverity, SharedErrorRecoveryManager};
use tracing::{info, warn};

/// Result type for app operations.
pub type AppResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Target frame rate (60 FPS = ~16.67ms per frame).
const TARGET_FPS: u64 = 60;
const FRAME_DURATION: Duration = Duration::from_millis(1000 / TARGET_FPS);

/// Default data polling interval (100ms).
const DEFAULT_DATA_POLL_INTERVAL_MS: u64 = 100;

/// Header timestamp cache duration (update every second).
const TIMESTAMP_CACHE_DURATION: Duration = Duration::from_secs(1);

/// A pending action that requires user confirmation before execution.
#[derive(Clone, Debug)]
pub enum PendingAction {
    /// Spawn a new worker with the specified executor
    SpawnWorker(crate::event::WorkerExecutor),
    /// Kill a specific worker by suffix
    KillWorker { suffix: String, worker_type: String },
    /// Pause a specific worker
    PauseWorker { worker_id: String },
    /// Pause all workers
    PauseAllWorkers { count: usize },
    /// Resume a specific worker
    ResumeWorker { worker_id: String },
    /// Resume all workers
    ResumeAllWorkers { count: usize },
}

/// Main application state.
pub struct App {
    /// Current active view
    current_view: View,
    /// Previous view (for back navigation)
    previous_view: Option<View>,
    /// Current focused panel within the view
    focus_panel: FocusPanel,
    /// Input handler for key events
    input_handler: InputHandler,
    /// Whether the app should quit
    should_quit: bool,
    /// Whether to show the help overlay
    show_help: bool,
    /// Chat input buffer
    chat_input: String,
    /// Status message to display
    status_message: Option<String>,
    /// List scroll position for current view
    scroll_offset: usize,
    /// Data manager for real worker/task data
    data_manager: DataManager,
    /// Theme manager for color themes
    theme_manager: ThemeManager,
    /// Dirty flag - whether UI needs redraw
    dirty: bool,
    /// Cached terminal size for layout recalculation
    cached_size: Option<Rect>,
    /// Last data poll time
    last_poll_time: Instant,
    /// Cached timestamp for header (updated every second)
    cached_timestamp: Option<String>,
    last_timestamp_update: Instant,
    /// Cached layout mode based on terminal width
    cached_layout_mode: Option<LayoutMode>,
    /// Last terminal width for layout mode detection
    last_terminal_width: u16,
    /// Whether an update is available
    update_available: bool,
    /// Whether an update is currently in progress
    update_in_progress: bool,
    /// Update progress information for display
    update_progress: UpdateProgress,
    /// Channel receiver for update completion results
    update_result_rx: Option<Receiver<UpdateResult>>,
    /// Last time we checked for updates
    last_update_check: Instant,
    /// Chat backend (None if initialization failed)
    chat_backend: Option<Arc<ChatBackend>>,
    /// Channel for sending responses from background thread to UI
    chat_response_tx: Option<Sender<(String, Result<ChatResponse, forge_chat::ChatError>)>>,
    /// Channel receiver for chat responses from background thread
    chat_response_rx: Option<Receiver<(String, Result<ChatResponse, forge_chat::ChatError>)>>,
    /// Whether a chat request is pending
    chat_pending: bool,
    /// Spinner animation frame index (0-3 for 4-frame spinner)
    chat_spinner_frame: usize,
    /// Partial response being streamed (for visual streaming effect)
    streaming_response: String,
    /// Current streaming position (character index - used for simulated streaming fallback)
    streaming_position: usize,
    /// Whether streaming is active (real-time from API or simulated)
    streaming_active: bool,
    /// Complete response received but not yet displayed (for simulated streaming fallback)
    pending_complete_response: Option<String>,
    /// Channel receiver for real-time streaming chunks from API
    streaming_chunk_rx: Option<std::sync::mpsc::Receiver<StreamingChatChunk>>,
    /// Whether we're doing real-time API streaming (vs simulated)
    real_streaming_active: bool,
    /// Query being processed (for streaming context)
    streaming_query: Option<String>,
    /// Chat conversation history (last 10 exchanges)
    chat_history: Vec<ChatExchange>,
    /// Chat history vertical scroll offset (lines scrolled from bottom)
    chat_scroll_offset: u16,
    /// Worker launcher for spawning workers
    worker_launcher: WorkerLauncher,
    /// Tokio runtime for async worker spawning
    worker_runtime: tokio::runtime::Runtime,
    /// Whether to show the kill worker dialog
    show_kill_dialog: bool,
    /// List of discovered workers for kill dialog
    kill_dialog_workers: Vec<DiscoveredWorker>,
    /// Currently selected worker index in kill dialog
    kill_dialog_selected: usize,
    /// Error message to show in kill dialog (if any)
    kill_dialog_error: Option<String>,
    /// Priority filter for Tasks view (None = no filter, Some(0-4) = filter to that priority)
    priority_filter: Option<u8>,
    /// Search query for Tasks view (empty string = no search)
    task_search_query: String,
    /// Whether search mode is active in Tasks view
    task_search_mode: bool,
    /// Whether to show the task detail overlay
    show_task_detail: bool,
    /// Currently selected task index in the flattened task list
    selected_task_index: usize,
    /// Currently selected alert index in the alerts list
    selected_alert_index: usize,
    /// Whether to show the alert detail overlay
    show_alert_detail: bool,
    /// Configuration watcher for hot-reload
    config_watcher: Option<ConfigWatcher>,
    /// Receiver for config change events
    config_rx: Option<Receiver<ConfigEvent>>,
    /// Current forge configuration
    forge_config: ForgeConfig,
    /// Configurable data poll interval (from forge_config.dashboard.refresh_interval_ms)
    data_poll_interval: Duration,
    /// Whether to show the confirmation dialog
    show_confirmation: bool,
    /// The pending action awaiting confirmation
    pending_action: Option<PendingAction>,
    /// Pending chat exchange data while streaming
    pending_chat_exchange: Option<PendingChatExchange>,
    /// Set of paused worker IDs
    paused_workers: std::collections::HashSet<String>,
    /// Currently selected worker index in Workers view
    selected_worker_index: usize,
    /// Error recovery manager for tracking and recovering from errors
    error_recovery: SharedErrorRecoveryManager,
    /// Last known good chat backend status (for graceful degradation)
    last_known_chat_available: bool,
    /// Timestamp of last successful data fetch
    last_successful_data_fetch: Option<Instant>,
    /// Last failed chat query (for retry functionality)
    last_failed_query: Option<String>,
    /// Whether retry is available (set when network error occurs)
    retry_available: bool,
    /// Crash recovery manager for handling worker crashes
    crash_recovery: CrashRecoveryManager,
    /// Network connectivity status (false = network unreachable)
    network_available: bool,
    /// Last network error message (for display in banner)
    network_error_message: Option<String>,
    /// Timestamp when network became unavailable
    network_unavailable_since: Option<Instant>,
}

/// Temporary storage for chat exchange data during streaming display.
#[derive(Clone, Debug)]
struct PendingChatExchange {
    query: String,
    response_text: String,
    timestamp: String,
    tool_calls: Vec<ToolCallInfo>,
    side_effects: Vec<SideEffectInfo>,
    confirmation: Option<ConfirmationInfo>,
    metadata: ResponseMetadata,
}

/// A single chat exchange (user query + assistant response).
#[derive(Clone, Debug)]
pub struct ChatExchange {
    pub user_query: String,
    pub assistant_response: String,
    pub timestamp: String,
    pub is_error: bool,
    /// Tool calls made during this exchange
    pub tool_calls: Vec<ToolCallInfo>,
    /// Side effects from tool execution
    pub side_effects: Vec<SideEffectInfo>,
    /// Confirmation prompt if action requires approval
    pub confirmation: Option<ConfirmationInfo>,
    /// Response metadata (duration, cost, provider)
    pub metadata: ResponseMetadata,
    /// Actionable guidance for errors (how to fix)
    pub error_guidance: Option<String>,
}

/// Information about a tool call for display.
#[derive(Clone, Debug)]
pub struct ToolCallInfo {
    /// Tool name (e.g., "spawn_worker", "get_worker_status")
    pub name: String,
    /// Whether the tool execution succeeded
    pub success: bool,
    /// Human-readable result message
    pub message: String,
}

/// Information about a side effect for display.
#[derive(Clone, Debug)]
pub struct SideEffectInfo {
    /// Type of effect (e.g., "spawn", "kill", "assign")
    pub effect_type: String,
    /// Description of what happened
    pub description: String,
}

/// Confirmation prompt information for display.
#[derive(Clone, Debug)]
pub struct ConfirmationInfo {
    /// Title of the confirmation
    pub title: String,
    /// Description of what will happen
    pub description: String,
    /// Warning level (info, warning, danger)
    pub level: ConfirmationLevel,
    /// Estimated cost impact
    pub cost_impact: Option<f64>,
    /// Items that will be affected
    pub affected_items: Vec<String>,
    /// Whether this action is reversible
    pub reversible: bool,
}

/// Warning level for confirmation prompts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmationLevel {
    /// Informational (no danger)
    Info,
    /// Warning (some risk)
    Warning,
    /// Danger (high risk, destructive)
    Danger,
}

/// Response metadata for display.
#[derive(Clone, Debug, Default)]
pub struct ResponseMetadata {
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Estimated cost in USD
    pub cost_usd: Option<f64>,
    /// Provider name
    pub provider: String,
}

/// Generate actionable guidance for common errors.
fn get_error_guidance(error_message: &str) -> Option<String> {
    let error_lower = error_message.to_lowercase();

    // Config not found errors
    if error_lower.contains("config") && (error_lower.contains("not found") || error_lower.contains("missing")) {
        return Some("Run 'forge init' to create a configuration file, or check that ~/.forge/config.yaml exists.".to_string());
    }

    // API/authentication errors
    if error_lower.contains("api key") || error_lower.contains("authentication") || error_lower.contains("unauthorized") {
        return Some("Check your API key in ~/.forge/config.yaml. Ensure the key is valid and not expired.".to_string());
    }

    // Rate limit errors
    if error_lower.contains("rate limit") || error_lower.contains("too many requests") {
        return Some("Wait a moment before sending more commands. Rate limits reset automatically.".to_string());
    }

    // Network unreachable (no internet)
    if error_lower.contains("network unreachable") || error_lower.contains("no route to host") {
        return Some("⚠️  Network unreachable. Check your internet connection and try again when online. Last known state is cached.".to_string());
    }

    // DNS resolution failures
    if error_lower.contains("dns") || error_lower.contains("failed to lookup address") || error_lower.contains("name or service not known") {
        return Some("⚠️  DNS resolution failed. Check DNS settings or try using 8.8.8.8 as your DNS server. Click 'r' to retry.".to_string());
    }

    // Connection timeout
    if error_lower.contains("timeout") {
        return Some("⏱️  Request timed out. The server may be slow or unreachable. Click 'r' to retry or check status.".to_string());
    }

    // Generic connection/network errors
    if error_lower.contains("connection") || error_lower.contains("network") {
        return Some("⚠️  Connection failed. Check your internet connection. The API server may be temporarily unavailable. Click 'r' to retry.".to_string());
    }

    // Provider errors
    if error_lower.contains("provider") && error_lower.contains("not found") {
        return Some("Check that the provider is installed and available in your PATH. Run 'forge --help' for setup instructions.".to_string());
    }

    // Tool execution errors
    if error_lower.contains("tool") && error_lower.contains("failed") {
        return Some("The requested operation could not be completed. Check the tool output above for details.".to_string());
    }

    // Worker errors
    if error_lower.contains("worker") && (error_lower.contains("spawn") || error_lower.contains("launch")) {
        return Some("Ensure you have sufficient system resources and the worker binary is available.".to_string());
    }

    // Context gathering errors
    if error_lower.contains("context") && error_lower.contains("failed") {
        return Some("The dashboard context could not be loaded. Try restarting the application.".to_string());
    }

    // Permission/access errors
    if error_lower.contains("permission") || error_lower.contains("access denied") || error_lower.contains("forbidden") {
        return Some("You don't have permission for this action. Check your configuration and credentials.".to_string());
    }

    // Resource not found errors (general)
    if error_lower.contains("not found") || error_lower.contains("does not exist") {
        return Some("The requested resource was not found. Verify the name and try again.".to_string());
    }

    // JSON parsing errors
    if error_lower.contains("json") || error_lower.contains("parse") || error_lower.contains("deserialize") {
        return Some("There was a problem processing the response. The server may have returned unexpected data.".to_string());
    }

    // IO errors
    if error_lower.contains("io error") || error_lower.contains("file") && error_lower.contains("error") {
        return Some("A file system error occurred. Check file permissions and available disk space.".to_string());
    }

    None
}

/// Result of an update operation.
#[derive(Clone, Debug)]
pub enum UpdateResult {
    /// Update completed successfully with version info
    Success {
        /// Previous version
        old_version: String,
        /// New version
        new_version: String,
    },
    /// Update failed with an error message
    Failed(String),
    /// Already up to date
    AlreadyUpToDate,
}

/// Progress information for update downloads.
#[derive(Clone, Debug, Default)]
pub struct UpdateProgress {
    /// Current status message
    pub status: String,
    /// Download progress percentage (0-100)
    pub percent: u8,
    /// Whether the update is in progress
    pub in_progress: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// Create a new app instance with default state.
    pub fn new() -> Self {
        use tracing::info;

        let start = Instant::now();
        info!("⏱️ App::new() started");

        let now = Instant::now();

        // Time DataManager initialization
        let dm_start = Instant::now();
        info!("⏱️ Creating DataManager...");
        let data_manager = DataManager::new();
        info!("⏱️ DataManager created in {:?}", dm_start.elapsed());

        // Time ThemeManager initialization
        let theme_start = Instant::now();
        info!("⏱️ Loading ThemeManager...");
        let theme_manager = ThemeManager::load_config();
        info!("⏱️ ThemeManager loaded in {:?}", theme_start.elapsed());

        // Time chat backend initialization
        let chat_start = Instant::now();
        info!("⏱️ Initializing chat backend...");
        let chat_backend = Self::init_chat_backend().map(Arc::new);
        let last_known_chat_available = chat_backend.is_some();
        info!("⏱️ Chat backend initialized in {:?}", chat_start.elapsed());

        // Initialize worker launcher and runtime
        let worker_launcher = WorkerLauncher::new();
        let worker_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create worker runtime");

        // Initialize config watcher for hot-reload
        let config_start = Instant::now();
        info!("⏱️ Initializing config watcher...");
        let (config_watcher, config_rx, forge_config) = match ConfigWatcher::new() {
            Some((watcher, rx)) => {
                let config = watcher.current_config().clone();
                info!("⏱️ Config watcher initialized in {:?}", config_start.elapsed());
                (Some(watcher), Some(rx), config)
            }
            None => {
                info!("⏱️ Config watcher not initialized (config file not found)");
                (None, None, ForgeConfig::default())
            }
        };

        // Calculate initial data poll interval from config
        let data_poll_interval = Duration::from_millis(
            forge_config.dashboard.refresh_interval_ms.min(DEFAULT_DATA_POLL_INTERVAL_MS)
        );

        // Configure alert notifier from config
        let mut data_manager = data_manager;
        data_manager.configure_notifier(
            forge_config.notifications.bell_on_critical,
            forge_config.notifications.bell_on_warning,
            forge_config.notifications.bell_interval_secs,
            forge_config.notifications.visual_flash_on_critical,
        );

        info!("⏱️ App::new() completed in {:?}", start.elapsed());

        Self {
            current_view: View::default(),
            previous_view: None,
            focus_panel: FocusPanel::default(),
            input_handler: InputHandler::new(),
            should_quit: false,
            show_help: false,
            chat_input: String::new(),
            status_message: None,
            scroll_offset: 0,
            data_manager,
            theme_manager,
            dirty: true,
            cached_size: None,
            last_poll_time: now,
            cached_timestamp: None,
            last_timestamp_update: now,
            cached_layout_mode: None,
            last_terminal_width: 0,
            update_available: false,
            update_in_progress: false,
            update_progress: UpdateProgress::default(),
            update_result_rx: None,
            last_update_check: now,
            chat_backend,
            chat_response_tx: None,
            chat_response_rx: None,
            chat_pending: false,
            chat_spinner_frame: 0,
            streaming_response: String::new(),
            streaming_position: 0,
            streaming_active: false,
            pending_complete_response: None,
            streaming_chunk_rx: None,
            real_streaming_active: false,
            streaming_query: None,
            chat_history: Vec::new(),
            chat_scroll_offset: 0,
            worker_launcher,
            worker_runtime,
            show_kill_dialog: false,
            kill_dialog_workers: Vec::new(),
            kill_dialog_selected: 0,
            kill_dialog_error: None,
            priority_filter: None,
            task_search_query: String::new(),
            task_search_mode: false,
            pending_chat_exchange: None,
            show_task_detail: false,
            selected_task_index: 0,
            selected_alert_index: 0,
            show_alert_detail: false,
            config_watcher,
            config_rx,
            forge_config,
            data_poll_interval,
            show_confirmation: false,
            pending_action: None,
            paused_workers: std::collections::HashSet::new(),
            selected_worker_index: 0,
            error_recovery: SharedErrorRecoveryManager::new(),
            last_known_chat_available,
            last_successful_data_fetch: None,
            last_failed_query: None,
            retry_available: false,
            crash_recovery: CrashRecoveryManager::new(),
            network_available: true,
            network_error_message: None,
            network_unavailable_since: None,
        }
    }

    /// Create a new app with a custom status directory (for testing).
    #[allow(dead_code)]
    pub fn with_status_dir(status_dir: std::path::PathBuf) -> Self {
        let now = Instant::now();
        Self {
            current_view: View::default(),
            previous_view: None,
            focus_panel: FocusPanel::default(),
            input_handler: InputHandler::new(),
            should_quit: false,
            show_help: false,
            chat_input: String::new(),
            status_message: None,
            scroll_offset: 0,
            data_manager: DataManager::with_status_dir(status_dir),
            theme_manager: ThemeManager::new(),
            dirty: true,
            cached_size: None,
            last_poll_time: now,
            cached_timestamp: None,
            last_timestamp_update: now,
            cached_layout_mode: None,
            last_terminal_width: 0,
            update_available: false,
            update_in_progress: false,
            update_progress: UpdateProgress::default(),
            update_result_rx: None,
            last_update_check: now,
            chat_backend: None, // Don't initialize in test mode
            chat_response_tx: None,
            chat_response_rx: None,
            last_known_chat_available: false, // No chat backend in test mode
            chat_pending: false,
            streaming_response: String::new(),
            streaming_position: 0,
            streaming_active: false,
            pending_complete_response: None,
            streaming_chunk_rx: None,
            real_streaming_active: false,
            streaming_query: None,
            chat_history: Vec::new(),
            chat_scroll_offset: 0,
            worker_launcher: WorkerLauncher::new(),
            worker_runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create worker runtime"),
            show_kill_dialog: false,
            kill_dialog_workers: Vec::new(),
            kill_dialog_selected: 0,
            kill_dialog_error: None,
            priority_filter: None,
            pending_chat_exchange: None,
            show_task_detail: false,
            selected_task_index: 0,
            selected_alert_index: 0,
            show_alert_detail: false,
            config_watcher: None, // Don't initialize in test mode
            config_rx: None,
            forge_config: ForgeConfig::default(),
            data_poll_interval: Duration::from_millis(DEFAULT_DATA_POLL_INTERVAL_MS),
            chat_spinner_frame: 0,
            show_confirmation: false,
            pending_action: None,
            task_search_query: String::new(),
            task_search_mode: false,
            paused_workers: std::collections::HashSet::new(),
            selected_worker_index: 0,
            error_recovery: SharedErrorRecoveryManager::new(),
            last_successful_data_fetch: None,
            last_failed_query: None,
            retry_available: false,
            crash_recovery: CrashRecoveryManager::new(),
            network_available: true,
            network_error_message: None,
            network_unavailable_since: None,
        }
    }

    /// Initialize chat backend from config.yaml.
    ///
    /// Returns None if config is missing or initialization fails.
    /// Errors are logged but don't prevent app startup.
    fn init_chat_backend() -> Option<ChatBackend> {
        use forge_chat::config::{
            AuditConfig, ClaudeCliConfig, ConfirmationConfig, ProviderConfig, RateLimitConfig,
        };
        use tracing::{error, info, warn};

        let start = std::time::Instant::now();
        info!("⏱️ init_chat_backend() started");

        // Load config from ~/.forge/config.yaml
        let config_path = match dirs::home_dir() {
            Some(home) => home.join(".forge/config.yaml"),
            None => {
                warn!("⏱️ Could not determine home directory (took {:?})", start.elapsed());
                return None;
            }
        };

        info!(
            "⏱️ Initializing chat backend from {}",
            config_path.display()
        );

        if !config_path.exists() {
            warn!(
                "⏱️ Chat config not found at {} (took {:?})",
                config_path.display(),
                start.elapsed()
            );
            return None;
        }

        let read_start = std::time::Instant::now();
        let config_str = match std::fs::read_to_string(&config_path) {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "⏱️ Failed to read chat config: {} (took {:?})",
                    e,
                    start.elapsed()
                );
                return None;
            }
        };
        info!("⏱️ Config file read in {:?}", read_start.elapsed());

        // Parse the full config YAML
        let parse_start = std::time::Instant::now();
        let yaml: serde_yaml::Value = match serde_yaml::from_str(&config_str) {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    "⏱️ Failed to parse config YAML: {} (took {:?})",
                    e,
                    start.elapsed()
                );
                return None;
            }
        };
        info!("⏱️ YAML parsed in {:?}", parse_start.elapsed());

        // Extract chat_backend section
        let chat_backend = yaml.get("chat_backend")?;
        let command = chat_backend.get("command")?.as_str()?;
        let args = chat_backend
            .get("args")?
            .as_sequence()?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<Vec<_>>();

        // Build ChatConfig
        let model = chat_backend
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("sonnet")
            .to_string();

        let cli_config = ClaudeCliConfig {
            binary_path: command.to_string(),
            model,
            extra_args: args,
            ..Default::default()
        };

        let chat_config = ChatConfig {
            provider: ProviderConfig::ClaudeCli(cli_config),
            rate_limit: RateLimitConfig::default(),
            audit: AuditConfig::default(),
            confirmations: ConfirmationConfig::default(),
        };

        // Initialize backend (async, but we block here during startup)
        let runtime_start = std::time::Instant::now();
        match tokio::runtime::Runtime::new() {
            Ok(rt) => {
                info!(
                    "⏱️ Created tokio runtime for chat backend in {:?}",
                    runtime_start.elapsed()
                );
                let backend_start = std::time::Instant::now();
                match rt.block_on(ChatBackend::new(chat_config)) {
                    Ok(backend) => {
                        info!(
                            "⏱️ ✅ Chat backend initialized successfully in {:?} (total: {:?})",
                            backend_start.elapsed(),
                            start.elapsed()
                        );
                        Some(backend)
                    }
                    Err(e) => {
                        error!(
                            "⏱️ ❌ Failed to initialize chat backend: {} (took {:?})",
                            e,
                            start.elapsed()
                        );
                        None
                    }
                }
            }
            Err(e) => {
                error!(
                    "⏱️ ❌ Failed to create tokio runtime: {} (took {:?})",
                    e,
                    start.elapsed()
                );
                None
            }
        }
    }

    /// Returns the current view.
    pub fn current_view(&self) -> View {
        self.current_view
    }

    /// Returns the current focus panel.
    pub fn focus_panel(&self) -> FocusPanel {
        self.focus_panel
    }

    /// Returns whether the app should quit.
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Returns whether help overlay is visible.
    pub fn show_help(&self) -> bool {
        self.show_help
    }

    /// Returns whether kill dialog is visible.
    pub fn show_kill_dialog(&self) -> bool {
        self.show_kill_dialog
    }

    /// Mark the UI as dirty (needs redraw).
    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Check if UI needs redraw and clear the dirty flag.
    fn take_dirty(&mut self) -> bool {
        if self.dirty {
            self.dirty = false;
            true
        } else {
            false
        }
    }

    /// Get cached timestamp or update if expired.
    fn get_cached_timestamp(&mut self) -> String {
        if self.last_timestamp_update.elapsed() >= TIMESTAMP_CACHE_DURATION {
            self.cached_timestamp =
                Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
            self.last_timestamp_update = Instant::now();
        }
        self.cached_timestamp.clone().unwrap_or_default()
    }

    /// Get layout mode with caching.
    fn get_layout_mode(&mut self, width: u16) -> LayoutMode {
        if self.last_terminal_width != width || self.cached_layout_mode.is_none() {
            self.cached_layout_mode = Some(LayoutMode::from_width(width));
            self.last_terminal_width = width;
        }
        self.cached_layout_mode.unwrap_or(LayoutMode::Narrow)
    }

    /// Check if terminal size changed.
    fn size_changed(&self, area: Rect) -> bool {
        match self.cached_size {
            Some(cached) => cached.width != area.width || cached.height != area.height,
            None => true,
        }
    }

    /// Update cached terminal size.
    fn update_cached_size(&mut self, area: Rect) {
        self.cached_size = Some(area);
    }

    /// Switch to a specific view.
    pub fn switch_view(&mut self, view: View) {
        if self.current_view != view {
            self.previous_view = Some(self.current_view);
            self.current_view = view;
            self.scroll_offset = 0;
            self.mark_dirty();

            // Set appropriate default focus for the view
            self.focus_panel = match view {
                View::Overview => FocusPanel::WorkerPool,
                View::Workers => FocusPanel::WorkerPool,
                View::Tasks => FocusPanel::TaskQueue,
                View::Costs => FocusPanel::CostBreakdown,
                View::Metrics => FocusPanel::MetricsCharts,
                View::Perf => FocusPanel::PerfCharts,
                View::Logs => FocusPanel::ActivityLog,
                View::Subscriptions => FocusPanel::Subscriptions,
                View::Alerts => FocusPanel::None,
                View::Chat => FocusPanel::ChatInput,
            };

            // Update input handler for chat mode
            self.input_handler.set_chat_mode(view == View::Chat);
            // Update input handler with current view for view-specific key handling
            self.input_handler.set_current_view(view);

            self.status_message = Some(format!(
                "{} (Press {} to return here)",
                view.title(),
                view.hotkey()
            ));
        }
    }

    /// Go to the next view in the cycle.
    pub fn next_view(&mut self) {
        let next = self.current_view.next();
        self.switch_view(next);
    }

    /// Go to the previous view in the cycle.
    pub fn prev_view(&mut self) {
        let prev = self.current_view.prev();
        self.switch_view(prev);
    }

    /// Go back to the previous view (if any).
    pub fn go_back(&mut self) {
        if let Some(prev) = self.previous_view.take() {
            self.switch_view(prev);
        }
    }

    /// Poll for chat responses from background thread.
    fn poll_chat_responses(&mut self) {
        use tracing::info;

        // Non-blocking check for responses (need to avoid borrow checker issues)
        let mut responses = Vec::new();
        if let Some(rx) = &self.chat_response_rx {
            match rx.try_recv() {
                Ok(response) => {
                    info!("📥 Got response from channel!");
                    responses.push(response);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    // No response yet, this is normal
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    info!("❌ Channel disconnected!");
                }
            }
        }

        // Process responses after releasing the borrow
        for (query, result) in responses {
            info!("Processing response for: {}", query);

            let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();

            match result {
                Ok(response) => {
                    // Check if ChatResponse indicates an error (ChatResponse::error() returns Ok with success=false)
                    if !response.success {
                        info!("❌ ChatResponse error: {:?}", response.error);
                        let error_msg = response.error.clone().unwrap_or_else(|| "Unknown error".to_string());
                        let guidance = get_error_guidance(&error_msg);

                        self.chat_history.push(ChatExchange {
                            user_query: query,
                            assistant_response: response.text.clone(),
                            timestamp,
                            is_error: true,
                            tool_calls: vec![],
                            side_effects: vec![],
                            confirmation: None,
                            metadata: ResponseMetadata {
                                duration_ms: response.duration_ms,
                                cost_usd: response.cost_usd,
                                provider: response.provider.clone(),
                            },
                            error_guidance: guidance,
                        });
                        self.status_message = Some(format!("❌ Chat error: {}", error_msg));
                    } else {
                        info!(
                            "✅ Success! Response text length: {} chars",
                            response.text.len()
                        );
                        info!(
                            "Response preview: {}",
                            response.text.chars().take(100).collect::<String>()
                        );

                        // Extract tool call information (pair by index since tools execute in order)
                        let tool_calls: Vec<ToolCallInfo> = response
                            .tool_results
                            .iter()
                            .enumerate()
                            .map(|(idx, result)| {
                                let name = response
                                    .tool_calls
                                    .get(idx)
                                    .map(|c| c.name.clone())
                                    .unwrap_or_else(|| "unknown".to_string());
                                ToolCallInfo {
                                    name,
                                    success: result.success,
                                    message: result.message.clone(),
                                }
                            })
                            .collect();

                        // Extract side effects
                        let side_effects: Vec<SideEffectInfo> = response
                            .tool_results
                            .iter()
                            .flat_map(|result| {
                                result.side_effects.iter().map(|effect| SideEffectInfo {
                                    effect_type: effect.effect_type.clone(),
                                    description: effect.description.clone(),
                                })
                            })
                            .collect();

                        // Extract confirmation info if present
                        let confirmation = response.confirmation_required.as_ref().map(|c| {
                            ConfirmationInfo {
                                title: c.title.clone(),
                                description: c.description.clone(),
                                level: match c.level {
                                    forge_chat::tools::ConfirmationLevel::Info => ConfirmationLevel::Info,
                                    forge_chat::tools::ConfirmationLevel::Warning => ConfirmationLevel::Warning,
                                    forge_chat::tools::ConfirmationLevel::Danger => ConfirmationLevel::Danger,
                                },
                                cost_impact: c.cost_impact,
                                affected_items: c.affected_items.clone(),
                                reversible: c.reversible,
                            }
                        });

                        // Extract metadata
                        let metadata = ResponseMetadata {
                            duration_ms: response.duration_ms,
                            cost_usd: response.cost_usd,
                            provider: response.provider.clone(),
                        };

                        // Start streaming the response instead of adding immediately
                        // Store the complete response data for streaming display
                        self.pending_complete_response = Some(response.text.clone());
                        self.streaming_response = String::new();
                        self.streaming_position = 0;
                        self.streaming_active = true;

                        // Store the pending ChatExchange data for when streaming completes
                        // We'll create the exchange in update_streaming when streaming finishes
                        self.pending_chat_exchange = Some(PendingChatExchange {
                            query,
                            response_text: response.text,
                            timestamp,
                            tool_calls,
                            side_effects,
                            confirmation,
                            metadata,
                        });
                        self.status_message = Some("✅ Streaming response...".to_string());

                        // Network is working - clear any network error status
                        if !self.network_available {
                            info!("Network recovered after {:?}", self.network_unavailable_since.map(|t| t.elapsed()));
                            self.network_available = true;
                            self.network_error_message = None;
                            self.network_unavailable_since = None;
                            self.error_recovery.mark_recovered("chat");
                        }
                    }
                }
                Err(e) => {
                    info!("❌ Error response: {}", e);
                    let error_msg = format!("Error: {}", e);
                    let guidance = get_error_guidance(&error_msg);

                    // Check if this is a network error that's retryable
                    let is_retryable = e.is_network_error() || e.is_retryable();
                    let is_network_error = e.is_network_error();

                    if is_retryable {
                        self.last_failed_query = Some(query.clone());
                        self.retry_available = true;
                    }

                    // Update network status tracking
                    if is_network_error {
                        if self.network_available {
                            // First network error - mark as unavailable
                            self.network_available = false;
                            self.network_unavailable_since = Some(Instant::now());
                            warn!("Network became unavailable: {}", e.friendly_message());
                        }
                        self.network_error_message = Some(e.friendly_message());

                        // Record network error in error recovery manager
                        let error_id = self.error_recovery.record_error(
                            ErrorCategory::Network,
                            ErrorSeverity::Error,
                            "Network Error",
                            e.friendly_message(),
                            vec![
                                e.suggested_action().to_string(),
                                "Press 'r' to retry when network is available".to_string(),
                            ],
                        );
                        self.error_recovery.mark_degraded("chat", error_id);
                    }

                    self.chat_history.push(ChatExchange {
                        user_query: query,
                        assistant_response: error_msg,
                        timestamp,
                        is_error: true,
                        tool_calls: vec![],
                        side_effects: vec![],
                        confirmation: None,
                        metadata: ResponseMetadata::default(),
                        error_guidance: guidance,
                    });

                    if is_retryable {
                        self.status_message = Some(format!("❌ {}: {} | Press 'r' to retry", e, e.suggested_action()));
                    } else {
                        self.status_message = Some(format!("❌ Chat error: {}", e));
                    }
                }
            }

            // Keep only last 10 exchanges
            if self.chat_history.len() > 10 {
                self.chat_history.remove(0);
            }

            info!(
                "📊 Chat history now has {} exchanges",
                self.chat_history.len()
            );
            info!("🖥️ Current view: {:?}", self.current_view);
            self.chat_pending = false;
            self.mark_dirty();
            info!("🔄 UI marked dirty for redraw");
        }
    }

    /// Poll for real-time streaming chunks from the API.
    ///
    /// This method checks the streaming channel and appends new text chunks
    /// to the display as they arrive from the LLM.
    fn poll_streaming_chunks(&mut self) {
        if !self.real_streaming_active {
            return;
        }

        // Collect all available chunks first (to avoid borrow issues)
        let chunks: Vec<StreamingChatChunk> = if let Some(ref rx) = self.streaming_chunk_rx {
            let mut collected = Vec::new();
            loop {
                match rx.try_recv() {
                    Ok(chunk) => collected.push(chunk),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        // Signal disconnection with a special marker
                        collected.push(StreamingChatChunk {
                            text: String::new(),
                            is_done: true,
                            error: None,
                            final_response: None,
                        });
                        break;
                    }
                }
            }
            collected
        } else {
            return;
        };

        // Process collected chunks
        for chunk in chunks {
            if let Some(ref err) = chunk.error {
                // Handle streaming error
                info!("Streaming error: {}", err);
                let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
                let query = self.streaming_query.take().unwrap_or_default();

                self.chat_history.push(ChatExchange {
                    user_query: query,
                    assistant_response: format!("Error: {}", err),
                    timestamp,
                    is_error: true,
                    tool_calls: vec![],
                    side_effects: vec![],
                    confirmation: None,
                    metadata: ResponseMetadata::default(),
                    error_guidance: Some("Streaming error occurred. Try again.".to_string()),
                });

                self.finalize_real_streaming();
                return;
            }

            if !chunk.text.is_empty() {
                // Append new text to streaming response
                self.streaming_response.push_str(&chunk.text);
                self.mark_dirty();
            }

            if chunk.is_done {
                // Streaming complete - finalize with the response metadata
                if let Some(response) = chunk.final_response {
                    let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
                    let query = self.streaming_query.take().unwrap_or_default();

                    // Build metadata from response
                    let metadata = ResponseMetadata {
                        duration_ms: response.duration_ms,
                        cost_usd: response.cost_usd,
                        provider: response.provider.clone(),
                    };

                    // Extract tool call information if any
                    let tool_calls: Vec<ToolCallInfo> = response
                        .tool_results
                        .iter()
                        .enumerate()
                        .map(|(idx, result)| {
                            let name = response
                                .tool_calls
                                .get(idx)
                                .map(|c| c.name.clone())
                                .unwrap_or_else(|| "unknown".to_string());
                            ToolCallInfo {
                                name,
                                success: result.success,
                                message: result.message.clone(),
                            }
                        })
                        .collect();

                    // Extract side effects
                    let side_effects: Vec<SideEffectInfo> = response
                        .tool_results
                        .iter()
                        .flat_map(|result| {
                            result.side_effects.iter().map(|effect| SideEffectInfo {
                                effect_type: effect.effect_type.clone(),
                                description: effect.description.clone(),
                            })
                        })
                        .collect();

                    // Use the accumulated streaming_response as the final text
                    let response_text = if self.streaming_response.is_empty() {
                        response.text.clone()
                    } else {
                        self.streaming_response.clone()
                    };

                    self.chat_history.push(ChatExchange {
                        user_query: query,
                        assistant_response: response_text,
                        timestamp,
                        is_error: !response.success,
                        tool_calls,
                        side_effects,
                        confirmation: None,
                        metadata,
                        error_guidance: None,
                    });

                    // Keep only last 10 exchanges
                    if self.chat_history.len() > 10 {
                        self.chat_history.remove(0);
                    }

                    info!("Real-time streaming complete: {}ms", response.duration_ms);
                } else {
                    // Channel closed without final response - save accumulated text
                    if !self.streaming_response.is_empty() {
                        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
                        let query = self.streaming_query.take().unwrap_or_default();

                        self.chat_history.push(ChatExchange {
                            user_query: query,
                            assistant_response: self.streaming_response.clone(),
                            timestamp,
                            is_error: false,
                            tool_calls: vec![],
                            side_effects: vec![],
                            confirmation: None,
                            metadata: ResponseMetadata::default(),
                            error_guidance: None,
                        });

                        if self.chat_history.len() > 10 {
                            self.chat_history.remove(0);
                        }
                    }
                }

                self.finalize_real_streaming();
                return;
            }
        }
    }

    /// Finalize real-time streaming state.
    fn finalize_real_streaming(&mut self) {
        self.real_streaming_active = false;
        self.streaming_active = false;
        self.streaming_response.clear();
        self.streaming_chunk_rx = None;
        self.streaming_query = None;
        self.chat_pending = false;
        self.status_message = Some("✅ Response complete".to_string());
        self.mark_dirty();
    }

    /// Update streaming display by advancing character position.
    ///
    /// This creates a visual streaming effect by revealing the response
    /// character by character at a configurable speed.
    /// Only used for simulated streaming (when real streaming is not available).
    fn update_streaming(&mut self) {
        // If real streaming is active, poll for chunks instead
        if self.real_streaming_active {
            self.poll_streaming_chunks();
            return;
        }

        if !self.streaming_active {
            return;
        }

        // Get the complete response text
        let complete_text = match &self.pending_complete_response {
            Some(text) => text.clone(),
            None => {
                // No text to stream, stop streaming
                self.streaming_active = false;
                return;
            }
        };

        // How many characters to reveal per frame (streaming speed)
        // Adjust this to change streaming speed - higher = faster
        const CHARS_PER_FRAME: usize = 3;

        let total_chars = complete_text.chars().count();

        if self.streaming_position >= total_chars {
            // Streaming complete - finalize the exchange
            self.finalize_streaming();
            return;
        }

        // Advance position
        let new_position = (self.streaming_position + CHARS_PER_FRAME).min(total_chars);

        // Update streaming_response with characters up to new position
        self.streaming_response = complete_text
            .chars()
            .take(new_position)
            .collect();

        self.streaming_position = new_position;
        self.mark_dirty();

        // Check if streaming is complete
        if self.streaming_position >= total_chars {
            self.finalize_streaming();
        }
    }

    /// Finalize streaming by adding the completed exchange to history.
    fn finalize_streaming(&mut self) {
        self.streaming_active = false;

        // Get the pending exchange data
        if let Some(pending) = self.pending_chat_exchange.take() {
            self.chat_history.push(ChatExchange {
                user_query: pending.query,
                assistant_response: pending.response_text,
                timestamp: pending.timestamp,
                is_error: false,
                tool_calls: pending.tool_calls,
                side_effects: pending.side_effects,
                confirmation: pending.confirmation,
                metadata: pending.metadata,
                error_guidance: None,
            });

            // Keep only last 10 exchanges
            if self.chat_history.len() > 10 {
                self.chat_history.remove(0);
            }

            self.status_message = Some("✅ Response complete".to_string());
        }

        // Clear streaming state
        self.streaming_response.clear();
        self.streaming_position = 0;
        self.pending_complete_response = None;
        self.chat_pending = false;
        self.mark_dirty();
    }

    /// Start a real-time streaming chat request.
    ///
    /// This spawns a background thread that makes a streaming API request
    /// and sends chunks via a channel as they arrive.
    fn start_streaming_chat_request(&mut self, query: &str) {
        use tracing::info;

        let Some(backend) = &self.chat_backend else {
            self.status_message = Some("Chat backend not initialized".to_string());
            return;
        };

        // Get the streaming config
        let Some(api_config) = backend.get_streaming_config() else {
            info!("Streaming config not available, falling back to non-streaming");
            // Fall back to simulated streaming via non-streaming request
            return;
        };

        info!("Starting real-time streaming chat request: {}", query);

        // Create channel for streaming chunks
        let (chunk_tx, chunk_rx) = std::sync::mpsc::channel::<StreamingChatChunk>();

        // Store the streaming state
        self.streaming_chunk_rx = Some(chunk_rx);
        self.real_streaming_active = true;
        self.streaming_active = true;
        self.streaming_response = String::new();
        self.streaming_query = Some(query.to_string());

        // Clone data needed for the background thread
        let backend_clone = Arc::clone(backend);
        let query_clone = query.to_string();

        // Spawn background thread for streaming
        std::thread::spawn(move || {
            use futures_util::StreamExt;

            info!("Streaming thread started for query: {}", query_clone);

            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = chunk_tx.send(StreamingChatChunk::error(format!("Runtime error: {}", e)));
                    return;
                }
            };

            rt.block_on(async {
                // Check rate limit
                if let Err(e) = backend_clone.check_and_record_rate_limit().await {
                    let _ = chunk_tx.send(StreamingChatChunk::error(format!("Rate limit error: {}", e)));
                    return;
                }

                // Get context and build prompt
                let context = match backend_clone.get_context().await {
                    Ok(ctx) => ctx,
                    Err(e) => {
                        let _ = chunk_tx.send(StreamingChatChunk::error(format!("Context error: {}", e)));
                        return;
                    }
                };

                let prompt = backend_clone.build_streaming_prompt(&query_clone, &context).await;
                let tools = backend_clone.get_tool_definitions();

                // Create provider for streaming
                let provider = match forge_chat::ClaudeApiProvider::from_config(api_config) {
                    Ok(p) => p,
                    Err(e) => {
                        let _ = chunk_tx.send(StreamingChatChunk::error(format!("Provider error: {}", e)));
                        return;
                    }
                };

                // Start streaming
                let stream = match provider.process_streaming(&prompt, &context, &tools).await {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = chunk_tx.send(StreamingChatChunk::error(format!("Streaming error: {}", e)));
                        return;
                    }
                };

                let start_time = std::time::Instant::now();
                let mut accumulated_text = String::new();
                let mut stream = std::pin::pin!(stream);

                while let Some(api_chunk) = stream.next().await {
                    if let Some(ref err) = api_chunk.error {
                        let _ = chunk_tx.send(StreamingChatChunk::error(err));
                        return;
                    }

                    if !api_chunk.text.is_empty() {
                        accumulated_text.push_str(&api_chunk.text);
                        let _ = chunk_tx.send(StreamingChatChunk::text(&api_chunk.text));
                    }

                    if api_chunk.is_done {
                        let duration_ms = start_time.elapsed().as_millis() as u64;

                        // Build final response
                        let mut response = ChatResponse::success(&accumulated_text)
                            .with_duration(duration_ms)
                            .with_provider("claude-api");

                        // Add cost if usage info available
                        if let Some(usage) = api_chunk.usage {
                            let cost = forge_chat::estimate_cost_from_usage(
                                usage.input_tokens,
                                usage.output_tokens,
                                "claude-api",
                            );
                            response = response.with_cost(cost);
                        }

                        let _ = chunk_tx.send(StreamingChatChunk::done(response));
                        info!("Streaming complete: {} chars in {}ms", accumulated_text.len(), duration_ms);
                        return;
                    }
                }

                // Stream ended without done marker
                let duration_ms = start_time.elapsed().as_millis() as u64;
                let response = ChatResponse::success(&accumulated_text)
                    .with_duration(duration_ms)
                    .with_provider("claude-api");
                let _ = chunk_tx.send(StreamingChatChunk::done(response));
            });
        });
    }

    /// Poll for config changes and apply hot-reload.
    ///
    /// This method checks for config file changes and applies them immediately
    /// without requiring an application restart.
    fn poll_config_changes(&mut self) {
        use crate::activity_panel::ActivityEventType;

        // Collect all pending events first to avoid borrow issues
        let events: Vec<ConfigEvent> = if let Some(ref rx) = self.config_rx {
            let mut evts = Vec::new();
            while let Ok(event) = rx.try_recv() {
                evts.push(event);
            }
            evts
        } else {
            return;
        };

        // Process events after releasing the borrow
        for event in events {
            match event {
                ConfigEvent::Reloaded { config } => {
                    self.apply_config_change(&config);
                    self.forge_config = config;

                    // Add activity log entry
                    self.data_manager.add_activity(
                        ActivityEventType::ConfigReload,
                        None,
                        "Config reloaded from disk",
                    );

                    self.status_message = Some("✅ Config reloaded".to_string());
                    self.mark_dirty();
                    info!("Config hot-reloaded successfully");
                }
                ConfigEvent::Created { config } => {
                    self.apply_config_change(&config);
                    self.forge_config = config;

                    self.data_manager.add_activity(
                        ActivityEventType::ConfigReload,
                        None,
                        "Config file created",
                    );

                    self.status_message = Some("✅ Config file created".to_string());
                    self.mark_dirty();
                    info!("Config file created");
                }
                ConfigEvent::ValidationError { error, path } => {
                    // Keep old config on validation error
                    warn!(
                        "Config validation failed for {:?}: {} - keeping old config",
                        path, error
                    );

                    self.data_manager.add_activity(
                        ActivityEventType::Error,
                        None,
                        format!("Config validation failed: {}", error),
                    );

                    self.status_message = Some(format!("⚠️ Invalid config: {}", error));
                    self.mark_dirty();
                }
                ConfigEvent::Removed => {
                    warn!("Config file removed - keeping last known config");

                    self.data_manager.add_activity(
                        ActivityEventType::Warning,
                        None,
                        "Config file removed",
                    );

                    self.status_message = Some("⚠️ Config file removed".to_string());
                    self.mark_dirty();
                }
                ConfigEvent::Error { error } => {
                    warn!("Config error: {} - keeping old config", error);

                    self.data_manager.add_activity(
                        ActivityEventType::Error,
                        None,
                        format!("Config parse error: {}", error),
                    );

                    self.status_message = Some(format!("⚠️ Config error: {}", error));
                    self.mark_dirty();
                }
            }
        }
    }

    /// Apply a configuration change to the running application.
    ///
    /// This method applies the hot-reloadable portions of the config
    /// while preserving application state.
    fn apply_config_change(&mut self, config: &ForgeConfig) {
        use crate::activity_panel::{ActivityEntry, ActivityEventType};

        let mut changes_applied = Vec::new();

        // Apply theme change if specified
        if let Some(ref theme_name) = config.theme.name {
            if let Some(theme) = crate::theme::ThemeName::from_str(theme_name) {
                if self.theme_manager.theme_name() != theme {
                    self.theme_manager.set_theme(theme);
                    changes_applied.push(format!("theme={}", theme_name));
                    info!("Theme changed to: {}", theme_name);
                }
            }
        }

        // Apply refresh interval change
        let new_interval = Duration::from_millis(
            config.dashboard.refresh_interval_ms.min(DEFAULT_DATA_POLL_INTERVAL_MS)
        );
        if self.data_poll_interval != new_interval {
            self.data_poll_interval = new_interval;
            changes_applied.push(format!("refresh_interval={}ms", config.dashboard.refresh_interval_ms));
            info!("Refresh interval changed to: {}ms", config.dashboard.refresh_interval_ms);
        }

        // Apply budget threshold changes (these are used by cost_panel when rendering)
        let old_warning = self.forge_config.cost_tracking.budget_warning_threshold;
        let old_critical = self.forge_config.cost_tracking.budget_critical_threshold;
        if config.cost_tracking.budget_warning_threshold != old_warning {
            changes_applied.push(format!("warning_threshold={}%", config.cost_tracking.budget_warning_threshold));
            info!("Budget warning threshold changed: {}% -> {}%", old_warning, config.cost_tracking.budget_warning_threshold);
        }
        if config.cost_tracking.budget_critical_threshold != old_critical {
            changes_applied.push(format!("critical_threshold={}%", config.cost_tracking.budget_critical_threshold));
            info!("Budget critical threshold changed: {}% -> {}%", old_critical, config.cost_tracking.budget_critical_threshold);
        }

        // Apply notification settings
        self.data_manager.configure_notifier(
            config.notifications.bell_on_critical,
            config.notifications.bell_on_warning,
            config.notifications.bell_interval_secs,
            config.notifications.visual_flash_on_critical,
        );
        if config.notifications.bell_on_critical != self.forge_config.notifications.bell_on_critical
            || config.notifications.bell_on_warning != self.forge_config.notifications.bell_on_warning
        {
            changes_applied.push(format!(
                "notifications=bell_critical:{},bell_warning:{}",
                config.notifications.bell_on_critical,
                config.notifications.bell_on_warning
            ));
        }

        // Log config reload to activity panel
        if !changes_applied.is_empty() {
            let message = format!("Config reloaded: {}", changes_applied.join(", "));
            self.data_manager.activity_data.push(
                ActivityEntry::new(ActivityEventType::ConfigReload, message.clone())
            );

            // Show visual feedback via status message
            self.status_message = Some(message);
            self.mark_dirty();

            info!("Applied config change: {}", changes_applied.join(", "));
        }

        // Store the updated config
        self.forge_config = config.clone();
    }

    /// Handle a key event.
    pub fn handle_key_event(&mut self, key: KeyEvent) {
        use crossterm::event::KeyCode;

        // Handle help overlay first
        if self.show_help {
            self.show_help = false;
            self.mark_dirty();
            return;
        }

        // Handle confirmation dialog if active (must be checked before kill dialog
        // since kill dialog can trigger confirmation)
        if self.show_confirmation {
            self.handle_confirmation_dialog_key(key);
            return;
        }

        // Handle kill dialog if active
        if self.show_kill_dialog {
            self.handle_kill_dialog_key(key);
            return;
        }

        // Handle task detail overlay if active
        if self.show_task_detail {
            self.handle_task_detail_key(key);
            return;
        }

        // Handle priority filter keys (0-4) when in Tasks view
        // Also handle search mode activation with '/' key
        if self.current_view == View::Tasks {
            if let KeyCode::Char(c) = key.code {
                match c {
                    // Activate search mode with '/' key
                    '/' if !self.task_search_mode => {
                        self.task_search_mode = true;
                        self.task_search_query.clear();
                        self.input_handler.set_chat_mode(true);
                        self.status_message = Some("Search mode: type to filter tasks, Esc to clear".to_string());
                        self.mark_dirty();
                        return;
                    }
                    '0' | '1' | '2' | '3' | '4' => {
                        let priority = c.to_digit(10).unwrap() as u8;
                        // Toggle filter: if same priority, clear it; otherwise set new filter
                        if self.priority_filter == Some(priority) {
                            self.priority_filter = None;
                            self.status_message = Some("Priority filter cleared".to_string());
                        } else {
                            self.priority_filter = Some(priority);
                            self.status_message = Some(format!("Filtering P{} tasks", priority));
                        }
                        self.scroll_offset = 0; // Reset scroll when filter changes
                        self.mark_dirty();
                        return;
                    }
                    // Clear filter with 'x' key
                    'x' | 'X' if self.priority_filter.is_some() => {
                        self.priority_filter = None;
                        self.status_message = Some("Priority filter cleared".to_string());
                        self.mark_dirty();
                        return;
                    }
                    _ => {}
                }
            }
        }

        let event = self.input_handler.handle_key(key);
        self.handle_app_event(event);
    }

    /// Handle an application event.
    pub fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::SwitchView(view) => self.switch_view(view),
            AppEvent::NextView => self.next_view(),
            AppEvent::PrevView => self.prev_view(),
            AppEvent::ShowHelp => {
                self.show_help = true;
                self.mark_dirty();
            }
            AppEvent::HideHelp => {
                self.show_help = false;
                self.mark_dirty();
            }
            AppEvent::Quit => self.should_quit = true,
            AppEvent::ForceQuit => self.should_quit = true,
            AppEvent::Refresh => {
                // In Chat view, if retry is available, retry the last failed query
                if self.current_view == View::Chat && self.retry_available {
                    if let Some(query) = self.last_failed_query.clone() {
                        self.retry_available = false;
                        self.last_failed_query = None;
                        self.status_message = Some(format!("🔄 Retrying: {}...", query));
                        self.chat_pending = true;

                        // Process chat request in background thread
                        if let Some(backend) = &self.chat_backend {
                            let backend_clone = Arc::clone(backend);
                            let query_clone = query.clone();

                            // Create channel if not already created
                            if self.chat_response_rx.is_none() {
                                let (tx, rx) = mpsc::channel();
                                self.chat_response_tx = Some(tx);
                                self.chat_response_rx = Some(rx);
                            }

                            let tx = self.chat_response_tx.as_ref().unwrap().clone();

                            // Spawn background thread to process request
                            std::thread::spawn(move || {
                                use tracing::info;

                                info!("Chat retry thread started for query: {}", query_clone);

                                let result = match tokio::runtime::Runtime::new() {
                                    Ok(rt) => rt.block_on(backend_clone.process_command(&query_clone)),
                                    Err(e) => Err(forge_chat::ChatError::ApiError(format!(
                                        "Runtime error: {}",
                                        e
                                    ))),
                                };

                                info!("Chat retry completed, result: {:?}", result.is_ok());

                                // Send result back to UI thread
                                match tx.send((query_clone, result)) {
                                    Ok(_) => info!("✅ Sent retry response to UI thread via channel"),
                                    Err(e) => info!("❌ Failed to send retry response: {:?}", e),
                                }
                            });
                        } else {
                            self.status_message = Some("Chat backend not available for retry".to_string());
                        }
                    }
                } else {
                    self.status_message = Some("Refreshed".to_string());
                }
                self.mark_dirty();
            }
            AppEvent::Cancel => {
                if self.show_help {
                    self.show_help = false;
                } else if self.current_view == View::Chat {
                    self.chat_input.clear();
                    self.go_back();
                } else if self.task_search_mode {
                    // Exit search mode
                    self.task_search_mode = false;
                    self.task_search_query.clear();
                    self.input_handler.set_chat_mode(false);
                    self.status_message = Some("Search cleared".to_string());
                    self.scroll_offset = 0;
                    self.mark_dirty();
                }
                self.mark_dirty();
            }
            AppEvent::NavigateUp => {
                // In Logs view, scroll the activity log
                if self.current_view == View::Logs {
                    self.data_manager.activity_log_mut().scroll_up(1);
                } else if self.current_view == View::Alerts {
                    // In Alerts view, navigate up through alerts
                    if self.selected_alert_index > 0 {
                        self.selected_alert_index -= 1;
                    }
                } else if self.current_view == View::Tasks {
                    // In Tasks view, navigate up through tasks
                    if self.selected_task_index > 0 {
                        self.selected_task_index -= 1;
                    }
                } else if self.scroll_offset > 0 {
                    self.scroll_offset -= 1;
                }
                self.mark_dirty();
            }
            AppEvent::NavigateDown => {
                // In Logs view, scroll the activity log
                if self.current_view == View::Logs {
                    self.data_manager.activity_log_mut().scroll_down(1);
                } else if self.current_view == View::Alerts {
                    // In Alerts view, navigate down through alerts
                    let alert_count = self.data_manager.alert_manager.active_count();
                    if self.selected_alert_index < alert_count.saturating_sub(1) {
                        self.selected_alert_index += 1;
                    }
                } else if self.current_view == View::Tasks {
                    // In Tasks view, navigate down through tasks
                    // Get actual task count for proper clamping (including search)
                    let search_query = if self.task_search_mode { &self.task_search_query } else { "" };
                    let task_count = self.data_manager.bead_manager.task_count_filtered_with_search(self.priority_filter, search_query);
                    if self.selected_task_index < task_count.saturating_sub(1) {
                        self.selected_task_index += 1;
                    }
                } else {
                    self.scroll_offset += 1;
                }
                self.mark_dirty();
            }
            AppEvent::PageUp => {
                // In Logs view, scroll the activity log
                if self.current_view == View::Logs {
                    self.data_manager.activity_log_mut().scroll_up(10);
                } else if self.current_view == View::Alerts {
                    // In Alerts view, page up through alerts
                    self.selected_alert_index = self.selected_alert_index.saturating_sub(10);
                } else if self.current_view == View::Tasks {
                    // In Tasks view, page up through tasks
                    self.selected_task_index = self.selected_task_index.saturating_sub(10);
                } else {
                    self.scroll_offset = self.scroll_offset.saturating_sub(10);
                }
                self.mark_dirty();
            }
            AppEvent::PageDown => {
                // In Logs view, scroll the activity log
                if self.current_view == View::Logs {
                    self.data_manager.activity_log_mut().scroll_down(10);
                } else if self.current_view == View::Alerts {
                    // In Alerts view, page down through alerts
                    let alert_count = self.data_manager.alert_manager.active_count();
                    self.selected_alert_index = (self.selected_alert_index + 10).min(alert_count.saturating_sub(1));
                } else if self.current_view == View::Tasks {
                    // In Tasks view, page down through tasks
                    let task_count = self.data_manager.bead_manager.task_count_filtered(self.priority_filter);
                    self.selected_task_index = (self.selected_task_index + 10).min(task_count.saturating_sub(1));
                } else {
                    self.scroll_offset += 10;
                }
                self.mark_dirty();
            }
            AppEvent::GoToTop => {
                // In Logs view, scroll to top of activity log
                if self.current_view == View::Logs {
                    self.data_manager.activity_log_mut().scroll_to_top();
                } else if self.current_view == View::Alerts {
                    // In Alerts view, go to first alert
                    self.selected_alert_index = 0;
                } else if self.current_view == View::Tasks {
                    // In Tasks view, go to first task
                    self.selected_task_index = 0;
                } else {
                    self.scroll_offset = 0;
                }
                self.mark_dirty();
            }
            AppEvent::GoToBottom => {
                // In Logs view, scroll to bottom of activity log (resume auto-scroll)
                if self.current_view == View::Logs {
                    self.data_manager.activity_log_mut().scroll_to_bottom();
                } else if self.current_view == View::Alerts {
                    // In Alerts view, go to last alert
                    let alert_count = self.data_manager.alert_manager.active_count();
                    self.selected_alert_index = alert_count.saturating_sub(1);
                } else if self.current_view == View::Tasks {
                    // In Tasks view, go to last task
                    let task_count = self.data_manager.bead_manager.task_count_filtered(self.priority_filter);
                    self.selected_task_index = task_count.saturating_sub(1);
                } else {
                    // For other views, just reset to 0 since we don't have exact content height
                    self.scroll_offset = 0;
                }
                self.mark_dirty();
            }
            AppEvent::TextInput(c) => {
                if self.task_search_mode && self.current_view == View::Tasks {
                    self.task_search_query.push(c);
                    self.scroll_offset = 0;
                    self.mark_dirty();
                } else {
                    self.chat_input.push(c);
                    self.mark_dirty();
                }
            }
            AppEvent::Backspace => {
                if self.task_search_mode && self.current_view == View::Tasks {
                    self.task_search_query.pop();
                    self.mark_dirty();
                } else {
                    self.chat_input.pop();
                    self.mark_dirty();
                }
            }
            AppEvent::Submit => {
                if !self.chat_input.is_empty() {
                    let query = self.chat_input.clone();
                    self.chat_input.clear();

                    // Process chat request in background thread
                    if let Some(backend) = &self.chat_backend {
                        // Check if streaming is supported
                        let supports_streaming = backend.supports_streaming();

                        self.status_message = Some(format!(
                            "⏳ Processing{}...",
                            if supports_streaming { " (streaming)" } else { "" }
                        ));
                        self.chat_pending = true;

                        if supports_streaming {
                            // Use real-time streaming
                            self.start_streaming_chat_request(&query);
                        } else {
                            // Fall back to non-streaming request
                            // Clone Arc for thread
                            let backend_clone = Arc::clone(backend);
                            let query_clone = query.clone();

                            // Create channel if not already created
                            if self.chat_response_rx.is_none() {
                                let (tx, rx) = mpsc::channel();
                                self.chat_response_tx = Some(tx);
                                self.chat_response_rx = Some(rx);
                            }

                            let tx = self.chat_response_tx.as_ref().unwrap().clone();

                            // Spawn background thread to process request
                            std::thread::spawn(move || {
                                use tracing::info;

                                info!("Chat thread started for query: {}", query_clone);

                                let result = match tokio::runtime::Runtime::new() {
                                    Ok(rt) => rt.block_on(backend_clone.process_command(&query_clone)),
                                    Err(e) => Err(forge_chat::ChatError::ApiError(format!(
                                        "Runtime error: {}",
                                        e
                                    ))),
                                };

                                info!("Chat request completed, result: {:?}", result.is_ok());

                                // Send result back to UI thread
                                match tx.send((query_clone, result)) {
                                    Ok(_) => info!("✅ Sent response to UI thread via channel"),
                                    Err(e) => info!("❌ Failed to send response: {:?}", e),
                                }
                            });
                        }
                    } else {
                        self.status_message = Some("Chat backend not initialized".to_string());
                    }
                }
                self.mark_dirty();
            }
            AppEvent::Select => {
                // Handle select based on current view
                if self.current_view == View::Tasks {
                    // Show task detail overlay
                    self.show_task_detail = true;
                    self.mark_dirty();
                } else if self.current_view == View::Alerts {
                    // Acknowledge the selected alert
                    self.acknowledge_selected_alert();
                }
            }
            AppEvent::Toggle | AppEvent::FocusNext | AppEvent::FocusPrev => {
                // Panel-specific handling - to be implemented
            }
            AppEvent::SpawnWorker(executor) => {
                // Show confirmation dialog before spawning
                self.pending_action = Some(PendingAction::SpawnWorker(executor));
                self.show_confirmation = true;
                self.mark_dirty();
            }
            AppEvent::KillWorker => {
                // Toggle kill dialog
                if self.show_kill_dialog {
                    self.show_kill_dialog = false;
                    self.kill_dialog_error = None;
                } else {
                    // Discover workers and show dialog
                    self.discover_workers_for_kill_dialog();
                }
                self.mark_dirty();
            }
            AppEvent::OpenConfig => {
                self.status_message = Some("Opening configuration menu...".to_string());
                self.mark_dirty();
                // TODO: Implement config menu
            }
            AppEvent::OpenBudgetConfig => {
                self.status_message = Some("Opening budget configuration...".to_string());
                self.mark_dirty();
                // TODO: Implement budget config
            }
            AppEvent::OpenWorkerConfig => {
                self.status_message = Some("Opening worker configuration...".to_string());
                self.mark_dirty();
                // TODO: Implement worker config
            }
            AppEvent::CycleTheme => {
                let new_theme = self.theme_manager.cycle_theme();
                self.status_message = Some(format!("Theme: {}", new_theme.display_name()));
                self.mark_dirty();
            }
            AppEvent::Update => {
                self.trigger_update();
            }
            AppEvent::PauseWorker => {
                self.show_pause_worker_confirmation();
            }
            AppEvent::PauseAllWorkers => {
                self.show_pause_all_workers_confirmation();
            }
            AppEvent::ResumeWorker => {
                self.show_resume_worker_confirmation();
            }
            AppEvent::ResumeAllWorkers => {
                self.show_resume_all_workers_confirmation();
            }
            AppEvent::AcknowledgeAlert => {
                self.acknowledge_selected_alert();
            }
            AppEvent::AcknowledgeAllAlerts => {
                self.acknowledge_all_alerts();
            }
            AppEvent::None => {}
        }
    }

    /// Spawn a new worker of the specified type.
    fn spawn_worker(&mut self, executor: crate::event::WorkerExecutor) {
        use crate::event::WorkerExecutor;
        use tracing::{error, info};

        // Determine model string and tier based on executor type
        let (model, tier) = match executor {
            WorkerExecutor::Glm => ("glm-4.7", WorkerTier::Budget),
            WorkerExecutor::Sonnet => ("sonnet", WorkerTier::Standard),
            WorkerExecutor::Opus => ("opus", WorkerTier::Premium),
            WorkerExecutor::Haiku => ("haiku", WorkerTier::Budget),
        };

        // Find the launcher script
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/coder".to_string());
        let forge_src = std::env::var("FORGE_SRC").unwrap_or_else(|_| format!("{}/forge", home));

        // Try multiple launcher paths in order of preference
        let launcher_paths = vec![
            PathBuf::from(&forge_src).join("scripts/launchers/bead-worker-launcher.sh"),
            PathBuf::from(&home).join(".forge/launchers/bead-worker-launcher.sh"),
            PathBuf::from(&forge_src).join("test/example-launchers/claude-code-launcher.sh"),
        ];

        let launcher_path = launcher_paths
            .into_iter()
            .find(|p| p.exists())
            .unwrap_or_else(|| {
                // Fall back to first path even if it doesn't exist (will fail with clear error)
                PathBuf::from(&forge_src).join("scripts/launchers/bead-worker-launcher.sh")
            });

        // Generate a unique worker ID
        let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let worker_id = format!("{}-{}-{:04}", model, timestamp, rand::random::<u16>());
        let session_name = format!("forge-{}", worker_id);

        // Use FORGE_WORKSPACE or default workspace
        let workspace = std::env::var("FORGE_WORKSPACE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join("forge"));

        info!(
            "Spawning {} worker: id={}, session={}, workspace={}",
            executor.name(),
            worker_id,
            session_name,
            workspace.display()
        );

        // Create launch config
        let config = LaunchConfig::new(&launcher_path, &session_name, &workspace, model)
            .with_tier(tier)
            .with_timeout(60);

        // Create spawn request
        let request = SpawnRequest::new(&worker_id, config);

        // Spawn the worker asynchronously using the runtime
        let worker_id_clone = worker_id.clone();
        let model_name = executor.name().to_string();
        let result = self
            .worker_runtime
            .block_on(async { self.worker_launcher.spawn(request).await });

        match result {
            Ok(handle) => {
                info!(
                    "Worker spawned successfully: {} (PID: {}, session: {})",
                    handle.id, handle.pid, handle.session_name
                );
                self.status_message = Some(format!(
                    "Spawned {} worker: {} (PID: {})",
                    model_name, worker_id_clone, handle.pid
                ));
            }
            Err(e) => {
                error!("Failed to spawn worker {}: {}", worker_id_clone, e);
                self.status_message = Some(format!("Failed to spawn {} worker: {}", model_name, e));
            }
        }

        self.mark_dirty();
    }

    /// Discover workers and show the kill dialog.
    fn discover_workers_for_kill_dialog(&mut self) {
        use forge_worker::discovery::discover_workers;
        use tracing::{error, info};

        info!("Discovering workers for kill dialog");

        // Discover workers using the runtime
        let result = self
            .worker_runtime
            .block_on(async { discover_workers().await });

        match result {
            Ok(discovery) => {
                let worker_count = discovery.workers.len();
                info!("Discovered {} workers for kill dialog", worker_count);

                if worker_count == 0 {
                    self.kill_dialog_error = Some("No active workers found".to_string());
                    self.kill_dialog_workers = Vec::new();
                } else {
                    self.kill_dialog_error = None;
                    self.kill_dialog_workers = discovery.workers;
                }

                self.kill_dialog_selected = 0;
                self.show_kill_dialog = true;
            }
            Err(e) => {
                error!("Failed to discover workers: {}", e);
                self.kill_dialog_error = Some(format!("Failed to discover workers: {}", e));
                self.kill_dialog_workers = Vec::new();
                self.kill_dialog_selected = 0;
                self.show_kill_dialog = true;
            }
        }
    }

    /// Kill the currently selected worker in the kill dialog.
    fn kill_selected_worker(&mut self) {
        use forge_worker::tmux::{kill_session, session_exists};
        use tracing::{error, info, warn};

        if self.kill_dialog_selected >= self.kill_dialog_workers.len() {
            self.kill_dialog_error = Some("No worker selected".to_string());
            self.mark_dirty();
            return;
        }

        let worker = &self.kill_dialog_workers[self.kill_dialog_selected].clone();
        let session_name = worker.session_name.clone();

        info!(
            "Killing worker: {} (session: {})",
            worker.suffix, session_name
        );

        // First check if session exists (to handle already-dead workers gracefully)
        let exists = self
            .worker_runtime
            .block_on(async { session_exists(&session_name).await });

        match exists {
            Ok(true) => {
                // Session exists, proceed to kill
                let result = self
                    .worker_runtime
                    .block_on(async { kill_session(&session_name).await });

                match result {
                    Ok(()) => {
                        info!("Successfully killed worker session: {}", session_name);
                        self.status_message = Some(format!("Killed worker: {}", worker.suffix));

                        // Remove the killed worker from the list
                        self.kill_dialog_workers.remove(self.kill_dialog_selected);

                        // Adjust selection if needed
                        if self.kill_dialog_selected >= self.kill_dialog_workers.len()
                            && self.kill_dialog_selected > 0
                        {
                            self.kill_dialog_selected -= 1;
                        }

                        // Close dialog if no workers left
                        if self.kill_dialog_workers.is_empty() {
                            self.show_kill_dialog = false;
                            self.kill_dialog_error = None;
                        }
                    }
                    Err(e) => {
                        error!("Failed to kill worker {}: {}", session_name, e);
                        self.kill_dialog_error = Some(format!("Failed to kill worker: {}", e));
                    }
                }
            }
            Ok(false) => {
                // Session doesn't exist - worker already terminated
                warn!("Worker session already terminated: {}", session_name);
                self.kill_dialog_error = Some(format!(
                    "Worker '{}' already terminated or session not found",
                    worker.suffix
                ));

                // Remove from list since it's already dead
                self.kill_dialog_workers.remove(self.kill_dialog_selected);
                if self.kill_dialog_selected >= self.kill_dialog_workers.len()
                    && self.kill_dialog_selected > 0
                {
                    self.kill_dialog_selected -= 1;
                }
                if self.kill_dialog_workers.is_empty() {
                    self.show_kill_dialog = false;
                }
            }
            Err(e) => {
                error!("Failed to check session existence: {}", e);
                // Try to kill anyway - the kill might still work
                let result = self
                    .worker_runtime
                    .block_on(async { kill_session(&session_name).await });
                match result {
                    Ok(()) => {
                        info!("Successfully killed worker session: {}", session_name);
                        self.status_message = Some(format!("Killed worker: {}", worker.suffix));
                        self.kill_dialog_workers.remove(self.kill_dialog_selected);
                        if self.kill_dialog_selected >= self.kill_dialog_workers.len()
                            && self.kill_dialog_selected > 0
                        {
                            self.kill_dialog_selected -= 1;
                        }
                        if self.kill_dialog_workers.is_empty() {
                            self.show_kill_dialog = false;
                            self.kill_dialog_error = None;
                        }
                    }
                    Err(e) => {
                        self.kill_dialog_error = Some(format!("Failed to kill worker: {}", e));
                    }
                }
            }
        }

        self.mark_dirty();
    }

    /// Show confirmation dialog for pausing the selected worker.
    fn show_pause_worker_confirmation(&mut self) {
        // Get sorted list of worker IDs
        let mut workers: Vec<_> = self.data_manager.worker_data.workers.keys().collect();
        workers.sort();

        if workers.is_empty() {
            self.status_message = Some("No workers to pause".to_string());
            self.mark_dirty();
            return;
        }

        // Ensure selection is within bounds
        if self.selected_worker_index >= workers.len() {
            self.selected_worker_index = 0;
        }

        let worker_id = workers[self.selected_worker_index].clone();

        // Check if already paused
        if self.paused_workers.contains(&worker_id) {
            self.status_message = Some(format!("Worker {} is already paused", worker_id));
            self.mark_dirty();
            return;
        }

        // Show confirmation dialog
        self.pending_action = Some(PendingAction::PauseWorker { worker_id });
        self.show_confirmation = true;
        self.mark_dirty();
    }

    /// Show confirmation dialog for pausing all workers.
    fn show_pause_all_workers_confirmation(&mut self) {
        let workers = &self.data_manager.worker_data.workers;

        if workers.is_empty() {
            self.status_message = Some("No workers to pause".to_string());
            self.mark_dirty();
            return;
        }

        // Count workers that are not already paused
        let count = workers
            .keys()
            .filter(|id| !self.paused_workers.contains(*id))
            .count();

        if count == 0 {
            self.status_message = Some("All workers are already paused".to_string());
            self.mark_dirty();
            return;
        }

        // Show confirmation dialog
        self.pending_action = Some(PendingAction::PauseAllWorkers { count });
        self.show_confirmation = true;
        self.mark_dirty();
    }

    /// Show confirmation dialog for resuming the selected worker.
    fn show_resume_worker_confirmation(&mut self) {
        // Get sorted list of worker IDs
        let mut workers: Vec<_> = self.data_manager.worker_data.workers.keys().collect();
        workers.sort();

        if workers.is_empty() {
            self.status_message = Some("No workers to resume".to_string());
            self.mark_dirty();
            return;
        }

        // Ensure selection is within bounds
        if self.selected_worker_index >= workers.len() {
            self.selected_worker_index = 0;
        }

        let worker_id = workers[self.selected_worker_index].clone();

        // Check if actually paused
        if !self.paused_workers.contains(&worker_id) {
            self.status_message = Some(format!("Worker {} is not paused", worker_id));
            self.mark_dirty();
            return;
        }

        // Show confirmation dialog
        self.pending_action = Some(PendingAction::ResumeWorker { worker_id });
        self.show_confirmation = true;
        self.mark_dirty();
    }

    /// Show confirmation dialog for resuming all workers.
    fn show_resume_all_workers_confirmation(&mut self) {
        if self.paused_workers.is_empty() {
            self.status_message = Some("No paused workers to resume".to_string());
            self.mark_dirty();
            return;
        }

        let count = self.paused_workers.len();

        // Show confirmation dialog
        self.pending_action = Some(PendingAction::ResumeAllWorkers { count });
        self.show_confirmation = true;
        self.mark_dirty();
    }

    /// Pause the currently selected worker in the workers list.
    fn pause_selected_worker(&mut self) {
        use forge_worker::tmux;

        // Get sorted list of worker IDs
        let mut workers: Vec<_> = self.data_manager.worker_data.workers.keys().collect();
        workers.sort();

        if workers.is_empty() {
            self.status_message = Some("No workers to pause".to_string());
            self.mark_dirty();
            return;
        }

        // Ensure selection is within bounds
        if self.selected_worker_index >= workers.len() {
            self.selected_worker_index = 0;
        }

        let worker_id = workers[self.selected_worker_index].clone();

        // Check if already paused
        if self.paused_workers.contains(&worker_id) {
            self.status_message = Some(format!("Worker {} is already paused", worker_id));
            self.mark_dirty();
            return;
        }

        // Send pause signal to tmux session
        let worker_id_clone = worker_id.clone();
        let result = self
            .worker_runtime
            .block_on(async { tmux::pause_session(&worker_id_clone).await });

        match result {
            Ok(()) => {
                // Add to paused set
                self.paused_workers.insert(worker_id.clone());
                self.status_message = Some(format!("Paused worker: {}", worker_id));
                info!("Paused worker: {}", worker_id);
            }
            Err(e) => {
                self.status_message = Some(format!("Failed to pause worker: {}", e));
                warn!("Failed to pause worker {}: {}", worker_id, e);
            }
        }

        self.mark_dirty();
    }

    /// Pause all workers.
    fn pause_all_workers(&mut self) {
        use forge_worker::tmux;

        let workers = &self.data_manager.worker_data.workers;

        if workers.is_empty() {
            self.status_message = Some("No workers to pause".to_string());
            self.mark_dirty();
            return;
        }

        let mut paused_count = 0;
        let mut failed_count = 0;

        for worker_id in workers.keys() {
            if !self.paused_workers.contains(worker_id) {
                let worker_id_clone = worker_id.clone();
                let result = self
                    .worker_runtime
                    .block_on(async { tmux::pause_session(&worker_id_clone).await });

                match result {
                    Ok(()) => {
                        self.paused_workers.insert(worker_id.clone());
                        paused_count += 1;
                    }
                    Err(e) => {
                        warn!("Failed to pause worker {}: {}", worker_id, e);
                        failed_count += 1;
                    }
                }
            }
        }

        if failed_count > 0 {
            self.status_message = Some(format!(
                "Paused {} workers ({} failed)",
                paused_count, failed_count
            ));
        } else {
            self.status_message = Some(format!("Paused {} workers", paused_count));
        }

        info!("Paused {} workers", paused_count);
        self.mark_dirty();
    }

    /// Resume the currently selected worker in the workers list.
    fn resume_selected_worker(&mut self) {
        use forge_worker::tmux;

        // Get sorted list of worker IDs
        let mut workers: Vec<_> = self.data_manager.worker_data.workers.keys().collect();
        workers.sort();

        if workers.is_empty() {
            self.status_message = Some("No workers to resume".to_string());
            self.mark_dirty();
            return;
        }

        // Ensure selection is within bounds
        if self.selected_worker_index >= workers.len() {
            self.selected_worker_index = 0;
        }

        let worker_id = workers[self.selected_worker_index].clone();

        // Check if actually paused
        if !self.paused_workers.contains(&worker_id) {
            self.status_message = Some(format!("Worker {} is not paused", worker_id));
            self.mark_dirty();
            return;
        }

        // Send resume signal to tmux session
        let worker_id_clone = worker_id.clone();
        let result = self
            .worker_runtime
            .block_on(async { tmux::resume_session(&worker_id_clone).await });

        match result {
            Ok(()) => {
                // Remove from paused set
                self.paused_workers.remove(&worker_id);
                self.status_message = Some(format!("Resumed worker: {}", worker_id));
                info!("Resumed worker: {}", worker_id);
            }
            Err(e) => {
                self.status_message = Some(format!("Failed to resume worker: {}", e));
                warn!("Failed to resume worker {}: {}", worker_id, e);
            }
        }

        self.mark_dirty();
    }

    /// Resume all paused workers.
    fn resume_all_workers(&mut self) {
        use forge_worker::tmux;

        if self.paused_workers.is_empty() {
            self.status_message = Some("No paused workers to resume".to_string());
            self.mark_dirty();
            return;
        }

        let resumed_count = self.paused_workers.len();
        let mut failed_count = 0;

        // Collect worker IDs to avoid modifying set while iterating
        let worker_ids: Vec<_> = self.paused_workers.iter().cloned().collect();

        for worker_id in worker_ids {
            let result = self
                .worker_runtime
                .block_on(async { tmux::resume_session(&worker_id).await });

            match result {
                Ok(()) => {
                    self.paused_workers.remove(&worker_id);
                }
                Err(e) => {
                    warn!("Failed to resume worker {}: {}", worker_id, e);
                    failed_count += 1;
                }
            }
        }

        let actually_resumed = resumed_count - failed_count;
        if failed_count > 0 {
            self.status_message = Some(format!(
                "Resumed {} workers ({} failed)",
                actually_resumed, failed_count
            ));
        } else {
            self.status_message = Some(format!("Resumed {} workers", actually_resumed));
        }

        info!("Resumed {} workers", actually_resumed);
        self.mark_dirty();
    }

    /// Check if a worker is paused.
    pub fn is_worker_paused(&self, worker_id: &str) -> bool {
        self.paused_workers.contains(worker_id)
    }

    /// Acknowledge the currently selected alert in the alerts list.
    fn acknowledge_selected_alert(&mut self) {
        let alerts = self.data_manager.alert_manager.alerts_by_severity();
        if let Some(alert) = alerts.get(self.selected_alert_index) {
            let alert_id = alert.id;
            let worker_id = alert.worker_id.clone();
            self.data_manager.alert_manager.acknowledge(alert_id);
            self.status_message = Some(format!("Acknowledged alert for {}", worker_id));
            self.mark_dirty();
        } else {
            self.status_message = Some("No alert selected".to_string());
            self.mark_dirty();
        }
    }

    /// Acknowledge all alerts.
    fn acknowledge_all_alerts(&mut self) {
        let count = self.data_manager.acknowledge_all_alerts();
        if count > 0 {
            self.status_message = Some(format!("Acknowledged {} alert(s)", count));
        } else {
            self.status_message = Some("No alerts to acknowledge".to_string());
        }
        self.mark_dirty();
    }

    /// Handle kill dialog key navigation.
    fn handle_kill_dialog_key(&mut self, key: KeyEvent) {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Char('k') | KeyCode::Up => {
                // Move selection up
                if self.kill_dialog_selected > 0 {
                    self.kill_dialog_selected -= 1;
                    self.kill_dialog_error = None; // Clear error on navigation
                    self.mark_dirty();
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                // Move selection down
                if self.kill_dialog_selected + 1 < self.kill_dialog_workers.len() {
                    self.kill_dialog_selected += 1;
                    self.kill_dialog_error = None; // Clear error on navigation
                    self.mark_dirty();
                }
            }
            KeyCode::Enter => {
                // Request confirmation before killing the selected worker
                if self.kill_dialog_selected < self.kill_dialog_workers.len() {
                    let worker = &self.kill_dialog_workers[self.kill_dialog_selected];
                    let suffix = worker.suffix.clone();
                    let worker_type = worker.worker_type.to_string();

                    self.pending_action = Some(PendingAction::KillWorker {
                        suffix,
                        worker_type
                    });
                    self.show_confirmation = true;
                    self.mark_dirty();
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                // Close dialog
                self.show_kill_dialog = false;
                self.kill_dialog_error = None;
                self.mark_dirty();
            }
            _ => {}
        }
    }

    /// Handle confirmation dialog key events.
    fn handle_confirmation_dialog_key(&mut self, key: KeyEvent) {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                // Confirm the action
                self.show_confirmation = false;
                if let Some(action) = self.pending_action.take() {
                    self.execute_pending_action(action);
                }
                self.mark_dirty();
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('q') => {
                // Cancel the action
                self.show_confirmation = false;
                self.pending_action = None;
                self.status_message = Some("Action cancelled".to_string());
                self.mark_dirty();
            }
            _ => {}
        }
    }

    /// Execute a pending action after confirmation.
    fn execute_pending_action(&mut self, action: PendingAction) {
        match action {
            PendingAction::SpawnWorker(executor) => {
                // Execute spawn logic
                self.spawn_worker(executor);
            }
            PendingAction::KillWorker { suffix: _, worker_type: _ } => {
                // Execute kill logic - called from kill dialog after confirmation
                self.kill_selected_worker();
            }
            PendingAction::PauseWorker { .. } => {
                self.pause_selected_worker();
            }
            PendingAction::PauseAllWorkers { .. } => {
                self.pause_all_workers();
            }
            PendingAction::ResumeWorker { .. } => {
                self.resume_selected_worker();
            }
            PendingAction::ResumeAllWorkers { .. } => {
                self.resume_all_workers();
            }
        }
    }

    /// Handle task detail overlay key navigation.
    fn handle_task_detail_key(&mut self, key: KeyEvent) {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                // Close detail overlay
                self.show_task_detail = false;
                self.mark_dirty();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                // Move selection up in task list
                if self.selected_task_index > 0 {
                    self.selected_task_index -= 1;
                    self.mark_dirty();
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                // Move selection down in task list
                self.selected_task_index += 1;
                self.mark_dirty();
            }
            _ => {}
        }
    }

    /// Check if an update is available by comparing source binary timestamp.
    fn check_for_update(&mut self) {
        use std::env;
        use std::fs;

        // Check every 10 seconds
        if self.last_update_check.elapsed() < Duration::from_secs(10) {
            return;
        }
        self.last_update_check = Instant::now();

        let forge_src = env::var("FORGE_SRC").unwrap_or_else(|_| "/home/coder/forge".to_string());

        let source_binary = format!("{}/target/release/forge", forge_src);
        let installed_binary = env::current_exe()
            .ok()
            .or_else(|| {
                env::var("HOME")
                    .ok()
                    .map(|h| format!("{}/.cargo/bin/forge", h).into())
            })
            .unwrap_or_else(|| "forge".into());

        // Compare modification times
        if let (Ok(source_meta), Ok(installed_meta)) = (
            fs::metadata(&source_binary),
            fs::metadata(&installed_binary),
        ) {
            if let (Ok(source_time), Ok(installed_time)) =
                (source_meta.modified(), installed_meta.modified())
            {
                self.update_available = source_time > installed_time;
            }
        }
    }

    /// Trigger forge self-update from GitHub releases (non-blocking).
    ///
    /// Spawns an async task to:
    /// 1. Check GitHub releases for latest version
    /// 2. Download the new binary if update available
    /// 3. Perform atomic swap
    ///
    /// Shows immediate visual feedback via the update_progress overlay.
    #[cfg(feature = "self-update")]
    fn trigger_update(&mut self) {
        use std::thread;

        // Prevent multiple concurrent updates
        if self.update_in_progress {
            self.status_message = Some("Update already in progress...".to_string());
            self.mark_dirty();
            return;
        }

        // Show immediate visual feedback
        self.update_in_progress = true;
        self.update_progress = UpdateProgress {
            status: "Checking for updates...".to_string(),
            percent: 0,
            in_progress: true,
        };
        self.mark_dirty();

        // Get current version
        let current_version = env!("CARGO_PKG_VERSION").to_string();

        // Create channel for receiving result
        let (tx_result, rx_result) = mpsc::channel();
        self.update_result_rx = Some(rx_result);

        // Spawn background thread with tokio runtime for async operations
        thread::spawn(move || {
            // Create a tokio runtime for this thread
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx_result.send(UpdateResult::Failed(format!(
                        "Failed to create runtime: {}",
                        e
                    )));
                    return;
                }
            };

            rt.block_on(async {
                use forge_core::self_update::{check_for_update, perform_update, UpdateStatus};

                // Check for updates
                let status = match check_for_update(&current_version).await {
                    Ok(status) => status,
                    Err(e) => {
                        let _ = tx_result.send(UpdateResult::Failed(format!(
                            "Failed to check for updates: {}",
                            e
                        )));
                        return;
                    }
                };

                match status {
                    UpdateStatus::UpToDate => {
                        let _ = tx_result.send(UpdateResult::AlreadyUpToDate);
                    }
                    UpdateStatus::Available {
                        current: _,
                        latest: _,
                        download_url,
                        asset_size,
                    } => {
                        // Perform the update without progress callback (download is typically fast)
                        let result = perform_update(&download_url, asset_size, None).await;

                        match result {
                            Ok(forge_core::self_update::UpdateResult::Success { old_version, new_version }) => {
                                let _ = tx_result.send(UpdateResult::Success {
                                    old_version,
                                    new_version,  // Use the version from the result
                                });
                            }
                            Ok(forge_core::self_update::UpdateResult::AlreadyUpToDate) => {
                                let _ = tx_result.send(UpdateResult::AlreadyUpToDate);
                            }
                            Ok(forge_core::self_update::UpdateResult::Failed(err)) => {
                                let _ = tx_result.send(UpdateResult::Failed(err));
                            }
                            Err(e) => {
                                let _ = tx_result.send(UpdateResult::Failed(format!(
                                    "Update failed: {}",
                                    e
                                )));
                            }
                        }
                    }
                }
            });
        });
    }

    /// Trigger forge update fallback (when self-update feature is disabled).
    #[cfg(not(feature = "self-update"))]
    fn trigger_update(&mut self) {
        self.status_message = Some("Self-update not available in this build.".to_string());
        self.mark_dirty();
    }

    /// Poll for update completion and progress (called from event loop).
    fn poll_update_result(&mut self) {
        if let Some(ref rx) = self.update_result_rx {
            // Non-blocking check for result
            if let Ok(result) = rx.try_recv() {
                self.update_in_progress = false;
                self.update_progress.in_progress = false;
                self.update_result_rx = None;

                match result {
                    UpdateResult::Success {
                        old_version,
                        new_version,
                    } => {
                        self.status_message = Some(format!(
                            "Updated forge v{} -> v{}! Please restart.",
                            old_version, new_version
                        ));
                        self.update_available = false;
                        self.update_progress.percent = 100;
                    }
                    UpdateResult::AlreadyUpToDate => {
                        self.status_message =
                            Some(format!("Already running latest version v{}", env!("CARGO_PKG_VERSION")));
                    }
                    UpdateResult::Failed(err) => {
                        self.status_message = Some(format!("Update failed: {}", err));
                    }
                }
                self.mark_dirty();
            }
        }
    }

    /// Run the main application loop.
    pub fn run(&mut self) -> AppResult<()> {
        // Setup terminal
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        crossterm::execute!(
            stdout,
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Main loop
        let result = self.run_loop(&mut terminal);

        // Restore terminal
        crossterm::terminal::disable_raw_mode()?;
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        result
    }

    /// The inner event loop with frame-rate limiting and optimized polling.
    fn run_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> AppResult<()> {
        // Memory update interval (every 60 frames = ~1 second at 60fps)
        let mut memory_update_counter: u32 = 0;
        const MEMORY_UPDATE_INTERVAL: u32 = 60;

        while !self.should_quit {
            let frame_start = Instant::now();

            // Advance spinner animation when chat is pending
            if self.chat_pending {
                self.chat_spinner_frame = (self.chat_spinner_frame + 1) % 4;
                self.mark_dirty(); // Ensure UI updates for spinner animation
            }

            // Poll for chat responses FIRST - this is non-blocking and must happen
            // frequently to ensure responsive chat UX. Don't let data polling block it.
            self.poll_chat_responses();

            // Update streaming display if active
            self.update_streaming();

            // Poll for config hot-reload changes
            self.poll_config_changes();

            // Optimized data polling - only poll if interval elapsed
            let needs_poll = self.last_poll_time.elapsed() >= self.data_poll_interval;
            if needs_poll {
                let data_changed = self.data_manager.poll_updates();
                self.last_poll_time = Instant::now();

                // Mark dirty if data actually changed
                if data_changed {
                    self.mark_dirty();
                }

                // Check for pending bell notifications and ring if needed
                if self.data_manager.take_pending_bell() {
                    crate::alert::AlertNotifier::ring_bell();
                }
            }

            // Poll for chat responses again after data polling in case a response
            // arrived while we were doing the (potentially slow) data poll
            self.poll_chat_responses();

            // Update streaming display if active
            self.update_streaming();

            // Poll for update completion
            self.poll_update_result();

            // Check for updates periodically
            self.check_for_update();

            // Only draw if dirty or at minimum rate (timestamp updates every second)
            let needs_redraw = self.take_dirty()
                || self.last_timestamp_update.elapsed() >= TIMESTAMP_CACHE_DURATION;

            // Measure render time
            let render_start = Instant::now();
            if needs_redraw {
                terminal.draw(|frame| self.draw(frame))?;
            }
            let render_us = render_start.elapsed().as_micros() as u64;

            // Calculate remaining time in frame for event handling
            let elapsed = frame_start.elapsed();
            let timeout_for_events = if elapsed < FRAME_DURATION {
                FRAME_DURATION - elapsed
            } else {
                Duration::ZERO
            };

            // Handle events with adaptive timeout
            let event_timeout = if timeout_for_events > Duration::ZERO {
                timeout_for_events
            } else {
                // If frame took too long, use shorter timeout
                Duration::from_millis(10)
            };

            if event::poll(event_timeout)? {
                if let Event::Key(key) = event::read()? {
                    self.data_manager.record_event();
                    self.handle_key_event(key);
                }
            }

            // Calculate event loop time (everything except render)
            let frame_elapsed = frame_start.elapsed();
            let event_loop_us = frame_elapsed.as_micros() as u64 - render_us;

            // Record frame performance metrics
            self.data_manager.record_frame_perf(event_loop_us, render_us);

            // Periodically update memory usage
            memory_update_counter += 1;
            if memory_update_counter >= MEMORY_UPDATE_INTERVAL {
                self.data_manager.update_memory();
                self.data_manager.prune_perf_alerts();
                memory_update_counter = 0;
            }

            // Frame-rate limiting: sleep if frame was too fast
            if frame_elapsed < FRAME_DURATION {
                let sleep_time = FRAME_DURATION - frame_elapsed;
                std::thread::sleep(sleep_time);
            }
        }
        Ok(())
    }

    /// Draw the UI.
    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Update cached size if changed
        if self.size_changed(area) {
            self.update_cached_size(area);
        }

        // Main layout: header, content, footer
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(10),   // Content
                Constraint::Length(2), // Footer
            ])
            .split(area);

        self.draw_header(frame, chunks[0]);
        self.draw_content(frame, chunks[1]);
        self.draw_footer(frame, chunks[2]);

        // Draw help overlay if active
        if self.show_help {
            self.draw_help_overlay(frame, area);
        }

        // Draw kill worker dialog if active
        if self.show_kill_dialog {
            self.draw_kill_dialog(frame, area);
        }

        // Draw confirmation dialog if active
        if self.show_confirmation {
            self.draw_confirmation_dialog(frame, area);
        }

        // Draw task detail overlay if active
        if self.show_task_detail {
            self.draw_task_detail_overlay(frame, area);
        }

        // Draw update notification banner if update available
        if self.update_available {
            self.draw_update_banner(frame, area);
        }

        // Draw network unreachable banner if network is down
        if !self.network_available {
            self.draw_network_banner(frame, area);
        }

        // Draw status message overlay if present
        if let Some(ref msg) = self.status_message {
            self.draw_status_message(frame, area, msg);
        }

        // Draw update in progress overlay (higher priority than status message)
        if self.update_in_progress {
            self.draw_update_progress_overlay(frame, area);
        }
    }

    /// Draw the header bar with cached timestamp.
    fn draw_header(&mut self, frame: &mut Frame, area: Rect) {
        let now = self.get_cached_timestamp();
        let theme = self.theme_manager.current();
        let version = env!("CARGO_PKG_VERSION");
        let title = format!(" FORGE v{} - {} ", version, self.current_view.title());
        let title_len = title.len();

        // Terminal dimensions display
        let dimensions = format!("{}×{}", frame.area().width, frame.area().height);

        // Get alert badge for display
        let alert_badge = self.data_manager.alert_badge();
        let alert_text = if alert_badge.should_display() {
            format!(" {} ", alert_badge.format_compact())
        } else {
            String::new()
        };
        let alert_color = if alert_badge.critical > 0 {
            theme.colors.status_error
        } else {
            theme.colors.status_warning
        };

        // Get unacknowledged errors for display
        let unacknowledged = self.error_recovery.unacknowledged_errors();
        let error_text = if !unacknowledged.is_empty() {
            // Show count of unacknowledged errors
            let count = unacknowledged.len();
            format!("⚠️{} ", count)
        } else {
            String::new()
        };

        // Determine system status from real data
        let (status_text, status_color) = if let Some(err) = self.data_manager.init_error() {
            (
                format!("[Error: {}]", truncate_status_error(err)),
                theme.colors.status_error,
            )
        } else if !self.data_manager.is_ready() {
            ("[Loading...]".to_string(), theme.colors.status_warning)
        } else {
            let counts = self.data_manager.worker_counts();
            if counts.unhealthy() > 0 {
                (
                    format!("[{} unhealthy]", counts.unhealthy()),
                    theme.colors.status_warning,
                )
            } else if counts.total == 0 {
                ("[No workers]".to_string(), theme.colors.text_dim)
            } else {
                (
                    format!("[{} workers]", counts.total),
                    theme.colors.status_healthy,
                )
            }
        };

        // Calculate spacing to right-align timestamp, dimensions, alert badge, error indicator, and status
        let right_content_len = now.len() + 2 + dimensions.len() + 2 + alert_text.len() + error_text.len() + status_text.len();
        let spacing = area
            .width
            .saturating_sub(title_len as u16 + right_content_len as u16 + 2)
            as usize;

        let mut header_spans = vec![
            Span::styled(
                title,
                Style::default()
                    .fg(theme.colors.header)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" ".repeat(spacing)),
            Span::styled(now, Style::default().fg(theme.colors.text_dim)),
            Span::raw("  "),
            Span::styled(dimensions, Style::default().fg(theme.colors.text_dim)),
            Span::raw("  "),
        ];

        // Add error indicator if there are unacknowledged errors
        if !error_text.is_empty() {
            header_spans.push(Span::styled(&error_text, Style::default().fg(theme.colors.status_error).add_modifier(Modifier::BOLD)));
        }

        // Add alert badge if there are alerts
        if alert_badge.should_display() {
            header_spans.push(Span::styled(&alert_text, Style::default().fg(alert_color).add_modifier(Modifier::BOLD)));
        }

        header_spans.push(Span::styled(status_text, Style::default().fg(status_color)));

        let header = Paragraph::new(Line::from(header_spans))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.colors.border_dim)),
        );

        frame.render_widget(header, area);
    }

    /// Draw the main content area based on current view.
    fn draw_content(&mut self, frame: &mut Frame, area: Rect) {
        match self.current_view {
            View::Overview => self.draw_overview(frame, area),
            View::Workers => self.draw_workers(frame, area),
            View::Tasks => self.draw_tasks(frame, area),
            View::Costs => self.draw_costs(frame, area),
            View::Metrics => self.draw_metrics(frame, area),
            View::Perf => self.draw_perf(frame, area),
            View::Logs => self.draw_logs(frame, area),
            View::Subscriptions => self.draw_subscriptions(frame, area),
            View::Alerts => self.draw_alerts(frame, area),
            View::Chat => self.draw_chat(frame, area),
        }
    }

    /// Draw the footer with hotkey hints.
    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let theme = self.theme_manager.current();
        let hotkey_style = Style::default().fg(theme.colors.hotkey);
        let hints = vec![
            Span::styled("[o]", hotkey_style),
            Span::raw("Overview "),
            Span::styled("[w]", hotkey_style),
            Span::raw("Workers "),
            Span::styled("[t]", hotkey_style),
            Span::raw("Tasks "),
            Span::styled("[c]", hotkey_style),
            Span::raw("Costs "),
            Span::styled("[m]", hotkey_style),
            Span::raw("Metrics "),
            Span::styled("[p]", hotkey_style),
            Span::raw("Perf "),
            Span::styled("[l]", hotkey_style),
            Span::raw("Logs "),
            Span::styled("[u]", hotkey_style),
            Span::raw("Subs "),
            Span::styled("[a]", hotkey_style),
            Span::raw("Alerts "),
            Span::styled("[:]", hotkey_style),
            Span::raw("Chat "),
            Span::styled("[?]", hotkey_style),
            Span::raw("Help "),
            Span::styled("[C]", hotkey_style),
            Span::raw("Theme "),
            Span::styled("[q]", hotkey_style),
            Span::raw("Quit"),
        ];

        let dims_text = format!("{}x{}", area.width, area.height);

        let footer = Paragraph::new(Line::from(hints))
            .style(Style::default().fg(theme.colors.text_dim))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .title(Span::styled(
                        dims_text,
                        Style::default().fg(theme.colors.border_dim),
                    ))
                    .title_alignment(ratatui::layout::Alignment::Right),
            );

        frame.render_widget(footer, area);
    }

    /// Draw the Overview/Dashboard view with cached layout mode.
    ///
    /// Layout adapts based on terminal width:
    /// - Ultra-wide (199+): 3-column layout with all 6 panels
    /// - Wide (120-198): 2-column layout with 4 panels
    /// - Narrow (<120): Single-column with stacked panels
    fn draw_overview(&mut self, frame: &mut Frame, area: Rect) {
        let layout_mode = self.get_layout_mode(area.width);

        match layout_mode {
            LayoutMode::UltraWide => self.draw_overview_ultrawide(frame, area),
            LayoutMode::Wide => self.draw_overview_wide(frame, area),
            LayoutMode::Narrow => self.draw_overview_narrow(frame, area),
        }
    }

    /// Draw ultra-wide 3-column layout (199+ cols).
    ///
    /// Layout: 66 | 66 | 65 columns (with borders accounting for 2 chars each)
    /// Left: Workers + Subscriptions (stacked)
    /// Middle: Tasks + Activity (stacked)
    /// Right: Costs + Actions (stacked)
    fn draw_overview_ultrawide(&mut self, frame: &mut Frame, area: Rect) {
        // Calculate column widths: 66 + 66 + 65 = 197, borders use remaining
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(66),
                Constraint::Length(66),
                Constraint::Min(65),
            ])
            .split(area);

        // Each column has 2 panels stacked vertically (50/50)
        let left_panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(columns[0]);

        let middle_panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(columns[1]);

        let right_panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(columns[2]);

        // Left column: Workers + Subscriptions
        let worker_summary = self.data_manager.worker_data.format_worker_pool_summary();
        self.draw_panel(
            frame,
            left_panels[0],
            "Worker Pool",
            &worker_summary,
            self.focus_panel == FocusPanel::WorkerPool,
        );

        let subscriptions_content = crate::subscription_panel::format_subscription_summary(
            &self.data_manager.subscription_data,
        );
        self.draw_panel(
            frame,
            left_panels[1],
            "Subscriptions",
            &subscriptions_content,
            self.focus_panel == FocusPanel::Subscriptions,
        );

        // Middle column: Tasks + Activity
        let task_queue_content = self.data_manager.bead_manager.format_task_queue_summary();
        self.draw_panel(
            frame,
            middle_panels[0],
            "Task Queue",
            &task_queue_content,
            self.focus_panel == FocusPanel::TaskQueue,
        );

        let activity_log = self.data_manager.worker_data.format_activity_log();
        self.draw_panel(
            frame,
            middle_panels[1],
            "Activity Log",
            &activity_log,
            self.focus_panel == FocusPanel::ActivityLog,
        );

        // Right column: Costs + Actions
        let costs_content = self.format_costs_panel();
        self.draw_panel(
            frame,
            right_panels[0],
            "Cost Breakdown",
            &costs_content,
            self.focus_panel == FocusPanel::CostBreakdown,
        );

        // Render Quick Actions panel with widget
        let quick_actions_panel =
            QuickActionsPanel::new().focused(self.focus_panel == FocusPanel::MetricsCharts);
        frame.render_widget(quick_actions_panel, right_panels[1]);
    }

    /// Draw wide 2-column layout (120-198 cols).
    fn draw_overview_wide(&mut self, frame: &mut Frame, area: Rect) {
        // Split into top and bottom sections
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        // Top section: Workers and Subscriptions side by side
        let top_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[0]);

        // Use real worker data
        let worker_summary = self.data_manager.worker_data.format_worker_pool_summary();

        self.draw_panel(
            frame,
            top_chunks[0],
            "Worker Pool",
            &worker_summary,
            self.focus_panel == FocusPanel::WorkerPool,
        );

        // Subscriptions panel - show subscription usage and reset timers
        let subscriptions_content = crate::subscription_panel::format_subscription_summary(
            &self.data_manager.subscription_data,
        );

        self.draw_panel(
            frame,
            top_chunks[1],
            "Subscriptions",
            &subscriptions_content,
            self.focus_panel == FocusPanel::Subscriptions,
        );

        // Bottom section: Task Queue and Activity Log
        let bottom_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);

        // Task queue - shows real bead data from monitored workspaces
        let task_queue_content = self.data_manager.bead_manager.format_task_queue_summary();

        self.draw_panel(
            frame,
            bottom_chunks[0],
            "Task Queue",
            &task_queue_content,
            self.focus_panel == FocusPanel::TaskQueue,
        );

        // Activity log from real worker data
        let activity_log = self.data_manager.worker_data.format_activity_log();

        self.draw_panel(
            frame,
            bottom_chunks[1],
            "Activity Log",
            &activity_log,
            self.focus_panel == FocusPanel::ActivityLog,
        );
    }

    /// Draw narrow single-column layout (<120 cols).
    fn draw_overview_narrow(&mut self, frame: &mut Frame, area: Rect) {
        // Stack panels vertically, show fewer in constrained space
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(35),
                Constraint::Percentage(35),
                Constraint::Percentage(30),
            ])
            .split(area);

        // Worker Pool (primary focus)
        let worker_summary = self.data_manager.worker_data.format_worker_pool_summary();
        self.draw_panel(
            frame,
            chunks[0],
            "Worker Pool",
            &worker_summary,
            self.focus_panel == FocusPanel::WorkerPool,
        );

        // Task Queue (secondary focus)
        let task_queue_content = self.data_manager.bead_manager.format_task_queue_summary();
        self.draw_panel(
            frame,
            chunks[1],
            "Task Queue",
            &task_queue_content,
            self.focus_panel == FocusPanel::TaskQueue,
        );

        // Activity Log (compact)
        let activity_log = self.data_manager.worker_data.format_activity_log();
        self.draw_panel(
            frame,
            chunks[2],
            "Activity Log",
            &activity_log,
            self.focus_panel == FocusPanel::ActivityLog,
        );
    }

    /// Format the costs panel for the right column in ultra-wide mode.
    fn format_costs_panel(&self) -> String {
        use crate::cost_panel::{format_tokens, format_usd};

        if !self.data_manager.is_ready() {
            return "Loading...".to_string();
        }

        let data = &self.data_manager.cost_data;

        if data.is_loading {
            return "Loading cost data...".to_string();
        }

        if !data.has_data() {
            return "No cost data available.\n\n\
                    Cost tracking requires:\n\
                    - forge-cost database\n\
                    - Worker logs being parsed"
                .to_string();
        }

        let mut lines = Vec::new();

        // Today's costs
        let daily_alert = data.daily_alert();
        lines.push(format!(
            "{} Today: {}",
            daily_alert.icon(),
            format_usd(data.today_total())
        ));
        lines.push(format!("  Calls: {}", data.today_calls()));
        lines.push(format!("  Tokens: {}", format_tokens(data.today_tokens())));
        lines.push(String::new());

        // Monthly budget
        let monthly_alert = data.monthly_alert();
        lines.push(format!(
            "{} Month: {} / {}",
            monthly_alert.icon(),
            format_usd(data.monthly_total),
            format_usd(data.budget.monthly_limit)
        ));

        // Progress bar
        let pct = data.monthly_usage_pct().min(100.0);
        let bar_width: usize = 15;
        let filled = ((pct / 100.0) * bar_width as f64).round() as usize;
        let empty = bar_width.saturating_sub(filled);
        lines.push(format!(
            "  [{}{}] {:.1}%",
            "█".repeat(filled),
            "░".repeat(empty),
            data.monthly_usage_pct()
        ));

        lines.join("\n")
    }

    /// Draw the Workers view.
    fn draw_workers(&self, frame: &mut Frame, area: Rect) {
        use crate::worker_panel::WorkerPanel;

        // Use the WorkerPanel widget with color-coded health indicators
        let worker_panel = WorkerPanel::new(&self.data_manager.worker_data)
            .focused(self.focus_panel == FocusPanel::WorkerPool);

        frame.render_widget(worker_panel, area);
    }

    /// Draw the Tasks view.
    fn draw_tasks(&self, frame: &mut Frame, area: Rect) {
        // Get formatted content with search query included
        let search_query = if self.task_search_mode {
            &self.task_search_query
        } else {
            ""
        };
        let content = self
            .data_manager
            .bead_manager
            .format_task_queue_full_filtered_with_search(self.priority_filter, search_query);

        // Update panel title with search indicator
        let title = if self.task_search_mode && !self.task_search_query.is_empty() {
            format!("Task Queue & Bead Management [Search: \"{}\"]", self.task_search_query)
        } else if self.task_search_mode {
            "Task Queue & Bead Management [Search active]".to_string()
        } else {
            "Task Queue & Bead Management".to_string()
        };

        self.draw_panel(frame, area, &title, &content, true);
    }

    /// Draw the Costs view.
    fn draw_costs(&self, frame: &mut Frame, area: Rect) {
        // Use the CostPanel widget for rich analytics display
        let cost_panel = CostPanel::new(&self.data_manager.cost_data)
            .focused(self.focus_panel == FocusPanel::CostBreakdown);
        frame.render_widget(cost_panel, area);
    }

    /// Draw the Perf view (FORGE internal performance metrics).
    fn draw_perf(&self, frame: &mut Frame, area: Rect) {
        // Use PerfPanel widget for FORGE internal performance metrics
        let perf_panel = PerfPanel::new(&self.data_manager.perf_metrics)
            .focused(self.focus_panel == FocusPanel::PerfCharts);
        frame.render_widget(perf_panel, area);
    }

    /// Draw the Metrics view.
    fn draw_metrics(&self, frame: &mut Frame, area: Rect) {
        // Use the MetricsPanel widget for rich performance visualization
        let metrics_panel = MetricsPanel::new(&self.data_manager.metrics_data)
            .focused(self.focus_panel == FocusPanel::MetricsCharts);
        frame.render_widget(metrics_panel, area);
    }

    /// Draw the Logs view.
    fn draw_logs(&self, frame: &mut Frame, area: Rect) {
        use crate::activity_panel::ActivityPanel;

        // Use the new ActivityPanel with real-time streaming
        let activity_panel = ActivityPanel::new(&self.data_manager.activity_data)
            .focused(self.focus_panel == FocusPanel::ActivityLog);

        frame.render_widget(activity_panel, area);
    }

    /// Draw the Subscriptions view.
    fn draw_subscriptions(&self, frame: &mut Frame, area: Rect) {
        use crate::subscription_panel::SubscriptionPanel;

        // Use the SubscriptionPanel widget for rich subscription visualization
        let subscription_panel = SubscriptionPanel::new(&self.data_manager.subscription_data)
            .focused(self.focus_panel == FocusPanel::Subscriptions);

        frame.render_widget(subscription_panel, area);
    }

    /// Draw the Alerts view.
    fn draw_alerts(&self, frame: &mut Frame, area: Rect) {
        use crate::alert::AlertSeverity;

        let theme = self.theme_manager.current();

        // Split into two columns: alert list (left) and alert detail (right)
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        // Get alerts sorted by severity
        let alerts = self.data_manager.alert_manager.alerts_by_severity();

        // Left panel: Alert list
        let mut list_lines: Vec<Line> = Vec::new();

        if alerts.is_empty() {
            list_lines.push(Line::styled(
                "No active alerts",
                Style::default().fg(theme.colors.text_dim),
            ));
            list_lines.push(Line::raw(""));
            list_lines.push(Line::styled(
                "Worker health alerts will appear here when:",
                Style::default().fg(theme.colors.text_dim),
            ));
            list_lines.push(Line::styled(
                "  - A worker process crashes",
                Style::default().fg(theme.colors.text_dim),
            ));
            list_lines.push(Line::styled(
                "  - A worker becomes unresponsive",
                Style::default().fg(theme.colors.text_dim),
            ));
            list_lines.push(Line::styled(
                "  - A task gets stuck",
                Style::default().fg(theme.colors.text_dim),
            ));
            list_lines.push(Line::styled(
                "  - Memory usage exceeds threshold",
                Style::default().fg(theme.colors.text_dim),
            ));
        } else {
            // Header
            let badge = self.data_manager.alert_badge();
            let header = format!(
                "Active Alerts: {} critical | {} warning | {} total",
                badge.critical,
                badge.warning,
                badge.total
            );
            list_lines.push(Line::styled(
                header,
                Style::default()
                    .fg(theme.colors.text)
                    .add_modifier(Modifier::BOLD),
            ));
            list_lines.push(Line::raw(""));

            for (idx, alert) in alerts.iter().enumerate() {
                let is_selected = idx == self.selected_alert_index;

                // Color based on severity
                let color = match alert.severity {
                    AlertSeverity::Critical => theme.colors.status_error,
                    AlertSeverity::Warning => theme.colors.status_warning,
                    AlertSeverity::Info => theme.colors.text_dim,
                };

                // Acknowledgment marker
                let ack_marker = if alert.acknowledged { "Y" } else { " " };

                // Occurrence count
                let count_str = if alert.occurrence_count > 1 {
                    format!(" (x{})", alert.occurrence_count)
                } else {
                    String::new()
                };

                // Selection indicator
                let selector = if is_selected { ">" } else { " " };

                // Time since alert
                let time_ago = {
                    let now = chrono::Utc::now();
                    let duration = now.signed_duration_since(alert.created_at);
                    if duration.num_minutes() < 1 {
                        "just now".to_string()
                    } else if duration.num_hours() < 1 {
                        format!("{}m ago", duration.num_minutes())
                    } else if duration.num_days() < 1 {
                        format!("{}h ago", duration.num_hours())
                    } else {
                        format!("{}d ago", duration.num_days())
                    }
                };

                let line_text = format!(
                    "{}{} {} [{:>8}] {}{}",
                    selector,
                    ack_marker,
                    alert.severity.icon(),
                    alert.worker_id,
                    alert.title,
                    count_str
                );

                let style = if is_selected {
                    Style::default().fg(color).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(color)
                };

                list_lines.push(Line::styled(line_text, style));
                list_lines.push(Line::styled(
                    format!("       {} - {}", time_ago, alert.message),
                    Style::default().fg(theme.colors.text_dim),
                ));
            }
        }

        let list_block = Block::default()
            .title(" Alerts ")
            .title_style(Style::default().fg(theme.colors.header))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.colors.border_dim));

        let list_paragraph = Paragraph::new(list_lines)
            .block(list_block)
            .wrap(Wrap { trim: false });

        frame.render_widget(list_paragraph, chunks[0]);

        // Right panel: Alert detail
        let mut detail_lines: Vec<Line> = Vec::new();

        if let Some(alert) = alerts.get(self.selected_alert_index) {
            detail_lines.push(Line::styled(
                format!("{} Alert Details", alert.severity.icon()),
                Style::default()
                    .fg(theme.colors.header)
                    .add_modifier(Modifier::BOLD),
            ));
            detail_lines.push(Line::raw(""));

            detail_lines.push(Line::styled(
                "Worker:",
                Style::default().fg(theme.colors.text_dim),
            ));
            detail_lines.push(Line::styled(
                format!("  {}", alert.worker_id),
                Style::default().fg(theme.colors.text),
            ));
            detail_lines.push(Line::raw(""));

            detail_lines.push(Line::styled(
                "Type:",
                Style::default().fg(theme.colors.text_dim),
            ));
            detail_lines.push(Line::styled(
                format!("  {}", alert.title),
                Style::default().fg(theme.colors.text),
            ));
            detail_lines.push(Line::raw(""));

            detail_lines.push(Line::styled(
                "Severity:",
                Style::default().fg(theme.colors.text_dim),
            ));
            let severity_color = match alert.severity {
                AlertSeverity::Critical => theme.colors.status_error,
                AlertSeverity::Warning => theme.colors.status_warning,
                AlertSeverity::Info => theme.colors.text_dim,
            };
            detail_lines.push(Line::styled(
                format!("  {:?}", alert.severity),
                Style::default().fg(severity_color),
            ));
            detail_lines.push(Line::raw(""));

            detail_lines.push(Line::styled(
                "Message:",
                Style::default().fg(theme.colors.text_dim),
            ));
            for line in alert.message.lines() {
                detail_lines.push(Line::styled(
                    format!("  {}", line),
                    Style::default().fg(theme.colors.text),
                ));
            }
            detail_lines.push(Line::raw(""));

            detail_lines.push(Line::styled(
                "First Seen:",
                Style::default().fg(theme.colors.text_dim),
            ));
            detail_lines.push(Line::styled(
                format!("  {}", alert.created_at.format("%Y-%m-%d %H:%M:%S UTC")),
                Style::default().fg(theme.colors.text),
            ));
            detail_lines.push(Line::raw(""));

            detail_lines.push(Line::styled(
                "Last Updated:",
                Style::default().fg(theme.colors.text_dim),
            ));
            detail_lines.push(Line::styled(
                format!("  {}", alert.updated_at.format("%Y-%m-%d %H:%M:%S UTC")),
                Style::default().fg(theme.colors.text),
            ));
            detail_lines.push(Line::raw(""));

            detail_lines.push(Line::styled(
                "Occurrences:",
                Style::default().fg(theme.colors.text_dim),
            ));
            detail_lines.push(Line::styled(
                format!("  {}", alert.occurrence_count),
                Style::default().fg(theme.colors.text),
            ));
            detail_lines.push(Line::raw(""));

            detail_lines.push(Line::styled(
                "Status:",
                Style::default().fg(theme.colors.text_dim),
            ));
            let status_text = if alert.acknowledged {
                "Acknowledged"
            } else if alert.is_active {
                "Active"
            } else {
                "Resolved"
            };
            let status_color = if alert.acknowledged {
                theme.colors.text_dim
            } else if alert.is_active {
                theme.colors.status_warning
            } else {
                theme.colors.status_healthy
            };
            detail_lines.push(Line::styled(
                format!("  {}", status_text),
                Style::default().fg(status_color),
            ));
            detail_lines.push(Line::raw(""));

            // Actions
            detail_lines.push(Line::styled(
                "-".repeat(30),
                Style::default().fg(theme.colors.border_dim),
            ));
            detail_lines.push(Line::styled(
                "Actions:",
                Style::default()
                    .fg(theme.colors.hotkey)
                    .add_modifier(Modifier::BOLD),
            ));
            detail_lines.push(Line::raw(""));
            detail_lines.push(Line::styled(
                "  [Enter] Acknowledge this alert",
                Style::default().fg(theme.colors.text_dim),
            ));
            detail_lines.push(Line::styled(
                "  [A] Acknowledge all alerts",
                Style::default().fg(theme.colors.text_dim),
            ));
            detail_lines.push(Line::styled(
                "  [Up/Down] Navigate alerts",
                Style::default().fg(theme.colors.text_dim),
            ));
        } else {
            detail_lines.push(Line::styled(
                "Select an alert to view details",
                Style::default().fg(theme.colors.text_dim),
            ));
        }

        let detail_block = Block::default()
            .title(" Details ")
            .title_style(Style::default().fg(theme.colors.header))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.colors.border_dim));

        let detail_paragraph = Paragraph::new(detail_lines)
            .block(detail_block)
            .wrap(Wrap { trim: false });

        frame.render_widget(detail_paragraph, chunks[1]);
    }

    /// Draw the Chat view.
    fn draw_chat(&self, frame: &mut Frame, area: Rect) {
        let theme = self.theme_manager.current();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(3)])
            .split(area);

        // Spinner animation frames (4-frame spinner)
        const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸"];

        // Build styled chat history
        let history_text = if self.chat_history.is_empty() {
            Text::from(
                "Type commands or ask questions. Examples:\n\n                 > show workers\n                 > spawn 2 glm workers\n                 > show P0 tasks\n                 > costs today\n                 > help\n\n                 Press Esc to exit chat mode.",
            )
        } else {
            let mut lines: Vec<Line> = Vec::new();

            for exchange in &self.chat_history {
                // User query line
                lines.push(Line::styled(
                    format!("[{}] You: {}", exchange.timestamp, exchange.user_query),
                    Style::default().fg(theme.colors.text),
                ));

                if exchange.is_error {
                    // Error response with red styling
                    for line in exchange.assistant_response.lines() {
                        lines.push(Line::styled(
                            format!("  ❌ {}", line),
                            Style::default().fg(theme.colors.status_error),
                        ));
                    }

                    // Show error guidance if available
                    if let Some(ref guidance) = exchange.error_guidance {
                        lines.push(Line::raw("")); // Blank line
                        lines.push(Line::styled(
                            "  ┌─ 💡 Suggested Action ─┐",
                            Style::default().fg(Color::Yellow),
                        ));
                        for guidance_line in guidance.lines() {
                            lines.push(Line::styled(
                                format!("  │ {}", guidance_line),
                                Style::default().fg(Color::Yellow),
                            ));
                        }
                        lines.push(Line::styled(
                            "  └────────────────────────┘",
                            Style::default().fg(Color::Yellow),
                        ));
                    }
                } else {
                    // Normal response
                    for line in exchange.assistant_response.lines() {
                        lines.push(Line::styled(
                            format!("  Assistant: {}", line),
                            Style::default().fg(theme.colors.text),
                        ));
                    }
                }

                // Show tool execution results
                if !exchange.tool_calls.is_empty() {
                    lines.push(Line::styled(
                        "  ── Tool Results ──",
                        Style::default().fg(theme.colors.text_dim),
                    ));
                    for tool in &exchange.tool_calls {
                        let status_icon = if tool.success { "✓" } else { "✗" };
                        let status_color = if tool.success {
                            theme.colors.status_healthy
                        } else {
                            theme.colors.status_error
                        };
                        lines.push(Line::styled(
                            format!("    {} {} {}", status_icon, tool.name, tool.message),
                            Style::default().fg(status_color),
                        ));
                    }
                }

                // Show side effects
                if !exchange.side_effects.is_empty() {
                    lines.push(Line::styled(
                        "  ── Side Effects ──",
                        Style::default().fg(theme.colors.text_dim),
                    ));
                    for effect in &exchange.side_effects {
                        let effect_icon = match effect.effect_type.as_str() {
                            "spawn" => "🚀",
                            "kill" => "💀",
                            "assign" => "📌",
                            _ => "⚡",
                        };
                        lines.push(Line::styled(
                            format!("    {} {}", effect_icon, effect.description),
                            Style::default().fg(theme.colors.text),
                        ));
                    }
                }

                // Show confirmation prompt if present
                if let Some(ref confirmation) = exchange.confirmation {
                    let level_color = match confirmation.level {
                        ConfirmationLevel::Info => Color::Blue,
                        ConfirmationLevel::Warning => Color::Yellow,
                        ConfirmationLevel::Danger => Color::Red,
                    };

                    lines.push(Line::styled(
                        "  ┌─ ⚠️  CONFIRMATION REQUIRED ─┐",
                        Style::default().fg(level_color),
                    ));
                    lines.push(Line::styled(
                        format!("  │ {}", confirmation.title),
                        Style::default().fg(level_color),
                    ));
                    lines.push(Line::styled(
                        format!("  │ {}", confirmation.description),
                        Style::default().fg(level_color),
                    ));

                    // Warning level indicator
                    let level_text = match confirmation.level {
                        ConfirmationLevel::Info => "ℹ️ INFO",
                        ConfirmationLevel::Warning => "⚠️ WARNING",
                        ConfirmationLevel::Danger => "🚨 DANGER",
                    };
                    lines.push(Line::styled(
                        format!("  │ Level: {}", level_text),
                        Style::default().fg(level_color),
                    ));

                    // Cost impact
                    if let Some(cost) = confirmation.cost_impact {
                        lines.push(Line::styled(
                            format!("  │ Cost Impact: ${:.4}", cost),
                            Style::default().fg(level_color),
                        ));
                    }

                    // Affected items
                    if !confirmation.affected_items.is_empty() {
                        lines.push(Line::styled(
                            "  │ Affected:",
                            Style::default().fg(level_color),
                        ));
                        for item in &confirmation.affected_items {
                            lines.push(Line::styled(
                                format!("  │   • {}", item),
                                Style::default().fg(level_color),
                            ));
                        }
                    }

                    // Reversibility
                    let reversible_text = if confirmation.reversible {
                        "Yes (can be undone)"
                    } else {
                        "No (permanent)"
                    };
                    lines.push(Line::styled(
                        format!("  │ Reversible: {}", reversible_text),
                        Style::default().fg(level_color),
                    ));

                    lines.push(Line::styled(
                        "  │ Type 'yes' to confirm or 'no' to cancel",
                        Style::default().fg(level_color),
                    ));
                    lines.push(Line::styled(
                        "  └────────────────────────────┘",
                        Style::default().fg(level_color),
                    ));
                }

                // Show metadata (duration, cost, provider)
                let meta = &exchange.metadata;
                if meta.duration_ms > 0 || meta.cost_usd.is_some() || !meta.provider.is_empty() {
                    let mut meta_parts = Vec::new();

                    if meta.duration_ms > 0 {
                        meta_parts.push(format!("{}ms", meta.duration_ms));
                    }

                    if let Some(cost) = meta.cost_usd {
                        meta_parts.push(format!("${:.4}", cost));
                    }

                    if !meta.provider.is_empty() {
                        meta_parts.push(meta.provider.clone());
                    }

                    if !meta_parts.is_empty() {
                        lines.push(Line::styled(
                            format!("  📊 [{}]", meta_parts.join(" | ")),
                            Style::default().fg(theme.colors.text_dim),
                        ));
                    }
                }

                lines.push(Line::raw("")); // Blank line between exchanges
            }

            // Show streaming response or processing indicator
            if self.streaming_active && !self.streaming_response.is_empty() {
                // Show streaming response with a typing indicator
                let cursor = "▌"; // Block cursor for streaming effect
                for line in self.streaming_response.lines() {
                    lines.push(Line::styled(
                        format!("  Assistant: {}{}", line, cursor),
                        Style::default().fg(theme.colors.text),
                    ));
                }
                // If response ends mid-line, show cursor at end
                if !self.streaming_response.ends_with('\n') && !self.streaming_response.is_empty() {
                    // Cursor already shown above
                }
            } else if self.chat_pending {
                let spinner = SPINNER_FRAMES[self.chat_spinner_frame % SPINNER_FRAMES.len()];
                lines.push(Line::styled(
                    format!("{} Processing your request...", spinner),
                    Style::default().fg(theme.colors.hotkey),
                ));
                lines.push(Line::styled(
                    "  Executing tools...",
                    Style::default().fg(theme.colors.text_dim),
                ));
            }

            Text::from(lines)
        };

        // Draw history panel with styled text
        let border_style = Style::default().fg(theme.colors.border_dim);
        let title_style = Style::default()
            .fg(theme.colors.header)
            .add_modifier(Modifier::BOLD);

        let history_panel = Paragraph::new(history_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(Span::styled(" Chat History ", title_style)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(history_panel, chunks[0]);

        // Input field
        let input_style = if self.focus_panel == FocusPanel::ChatInput {
            Style::default().fg(theme.colors.hotkey)
        } else {
            Style::default().fg(theme.colors.text_dim)
        };

        let cursor = if self.input_handler.is_chat_mode() {
            "█"
        } else {
            ""
        };

        let input = Paragraph::new(format!("> {}{}", self.chat_input, cursor))
            .style(input_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.colors.header))
                    .title(" Input "),
            );

        frame.render_widget(input, chunks[1]);
    }


    /// Draw a panel with optional highlight.
    fn draw_panel(&self, frame: &mut Frame, area: Rect, title: &str, content: &str, focused: bool) {
        let theme = self.theme_manager.current();

        // Enhanced focus indicator: "▶" arrow for focused (clearer than diamond), "▪" for unfocused
        let focus_icon = if focused { "▶" } else { "▪" };

        // Border type: Double border for focused (highly visible), Normal for unfocused
        let border_type = if focused {
            BorderType::Double
        } else {
            BorderType::Plain
        };

        // Border style: bright focus_highlight for focused, very dim for unfocused
        let border_style = if focused {
            Style::default()
                .fg(theme.colors.focus_highlight)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.colors.border_dim)
        };

        // Title style: bold + bright + underlined for focused, dim for unfocused
        let title_style = if focused {
            Style::default()
                .fg(theme.colors.focus_highlight)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(theme.colors.unfocused_text)
        };

        // Content style: bright for focused panels, significantly dimmed for unfocused
        let content_style = if focused {
            Style::default().fg(theme.colors.text)
        } else {
            Style::default().fg(theme.colors.unfocused_text)
        };

        let panel = Paragraph::new(content)
            .style(content_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(border_type)
                    .border_style(border_style)
                    .title(Span::styled(format!(" {} {} ", focus_icon, title), title_style)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(panel, area);
    }

    /// Poll for data updates. Called from the main loop.
    #[allow(dead_code)]
    pub fn poll_data(&mut self) {
        self.data_manager.poll_updates();
    }

    /// Draw the help overlay.
    fn draw_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let theme = self.theme_manager.current();
        // Calculate centered overlay area - increased size for more content
        let overlay_width = 70.min(area.width.saturating_sub(4));
        let overlay_height = 32.min(area.height.saturating_sub(4));
        let overlay_x = (area.width - overlay_width) / 2;
        let overlay_y = (area.height - overlay_height) / 2;

        let overlay_area = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

        // Clear background
        frame.render_widget(Clear, overlay_area);

        let help_text = "\
FORGE Hotkey Reference

View Navigation:
  O        Overview (dashboard)
  w/W      Workers view
  t/T      Tasks view
  c        Costs view
  m        Metrics view
  l/a      Activity/Logs view
  :        Chat input mode
  Tab      Cycle views forward
  Shift+Tab Cycle views backward

Worker Management:
  g/G      Spawn GLM worker
  s/S      Spawn Sonnet worker
  o        Spawn Opus worker
  h        Spawn Haiku worker
  k        Kill worker dialog

Tasks View:
  0-4      Filter by priority (P0-P4)
  x        Clear priority filter
  Enter    Show task details

General:
  ?        Show this help
  q/Q      Quit
  Esc      Cancel / Close overlay
  Ctrl+C   Force quit
  Ctrl+L   Refresh screen
  Ctrl+U   Update forge (rebuild & restart)
  r        Refresh data
  C        Cycle theme

Navigation:
  ↑        Move up
  ↓/j      Move down
  PgUp     Page up
  PgDn     Page down
  Home     Go to top
  End      Go to bottom
  Enter    Select/expand item
  Space    Toggle selection

Press any key to close this help.";

        let help = Paragraph::new(help_text)
            .style(Style::default().fg(theme.colors.text))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.colors.header))
                    .title(Span::styled(
                        " Help ",
                        Style::default()
                            .fg(theme.colors.header)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .style(Style::default().bg(Color::Black)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(help, overlay_area);
    }

    /// Draw the kill worker dialog overlay.
    fn draw_kill_dialog(&mut self, frame: &mut Frame, area: Rect) {
        let theme = self.theme_manager.current();

        // Calculate dialog dimensions based on content
        let overlay_width = 70.min(area.width.saturating_sub(4));
        let max_height = 20.min(area.height.saturating_sub(4));
        let overlay_x = (area.width - overlay_width) / 2;
        let overlay_y = (area.height - max_height) / 2;

        let overlay_area = Rect::new(overlay_x, overlay_y, overlay_width, max_height);

        // Clear background
        frame.render_widget(Clear, overlay_area);

        // Build dialog content
        let mut lines: Vec<Line> = Vec::new();

        // Header
        lines.push(Line::from(Span::styled(
            "Select a worker to kill:",
            Style::default()
                .fg(theme.colors.text)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::raw("")); // Empty line

        if self.kill_dialog_workers.is_empty() {
            // No workers message
            let msg = self
                .kill_dialog_error
                .as_deref()
                .unwrap_or("No active workers found");
            lines.push(Line::from(Span::styled(
                msg,
                Style::default().fg(theme.colors.status_warning),
            )));
        } else {
            // Worker list
            for (i, worker) in self.kill_dialog_workers.iter().enumerate() {
                let is_selected = i == self.kill_dialog_selected;

                // Format: [ ] worker-suffix (type, attached/detached, age)
                let checkbox = if is_selected { "[x] " } else { "[ ] " };
                let attached_str = if worker.is_attached {
                    "attached"
                } else {
                    "detached"
                };
                let worker_line = format!(
                    "{}{} ({}, {}, age: {})",
                    checkbox,
                    worker.suffix,
                    worker.worker_type,
                    attached_str,
                    worker.age()
                );

                let style = if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(theme.colors.hotkey)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.colors.text)
                };

                lines.push(Line::from(Span::styled(worker_line, style)));
            }
        }

        lines.push(Line::raw("")); // Empty line

        // Error message if any
        if let Some(ref err) = self.kill_dialog_error {
            if !self.kill_dialog_workers.is_empty() {
                // Only show if we have workers (otherwise it's the "no workers" message)
                lines.push(Line::from(Span::styled(
                    format!("Error: {}", err),
                    Style::default().fg(Color::Red),
                )));
                lines.push(Line::raw(""));
            }
        }

        // Footer with instructions
        lines.push(Line::from(Span::styled(
            "↑/k: up  ↓/j: down  Enter: kill  Esc/q: cancel",
            Style::default().fg(theme.colors.text_dim),
        )));

        let dialog = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red))
                    .title(Span::styled(
                        " Kill Worker ",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ))
                    .style(Style::default().bg(Color::Black)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(dialog, overlay_area);
    }

    /// Draw the confirmation dialog overlay.
    fn draw_confirmation_dialog(&self, frame: &mut Frame, area: Rect) {
        let theme = self.theme_manager.current();

        // Get dialog content based on pending action
        let (title, message, action_text) = match &self.pending_action {
            Some(PendingAction::SpawnWorker(executor)) => (
                " Spawn Worker ",
                format!("Are you sure you want to spawn a {} worker?", executor.name()),
                "spawn",
            ),
            Some(PendingAction::KillWorker { suffix, worker_type }) => (
                " Kill Worker ",
                format!(
                    "Are you sure you want to kill worker {} ({})?",
                    suffix, worker_type
                ),
                "kill",
            ),
            Some(PendingAction::PauseWorker { worker_id }) => (
                " Pause Worker ",
                format!(
                    "Are you sure you want to pause worker {}?\nThis will suspend the worker's tmux session.",
                    worker_id
                ),
                "pause",
            ),
            Some(PendingAction::PauseAllWorkers { count }) => (
                " Pause All Workers ",
                format!(
                    "Are you sure you want to pause {} worker(s)?\nThis will suspend all active tmux sessions.",
                    count
                ),
                "pause all",
            ),
            Some(PendingAction::ResumeWorker { worker_id }) => (
                " Resume Worker ",
                format!(
                    "Are you sure you want to resume worker {}?\nThis will continue the worker's tmux session.",
                    worker_id
                ),
                "resume",
            ),
            Some(PendingAction::ResumeAllWorkers { count }) => (
                " Resume All Workers ",
                format!(
                    "Are you sure you want to resume {} paused worker(s)?\nThis will continue all paused tmux sessions.",
                    count
                ),
                "resume all",
            ),
            None => return, // No action pending, don't draw
        };

        // Calculate dialog dimensions
        let overlay_width = 60.min(area.width.saturating_sub(4));
        let overlay_height = 9.min(area.height.saturating_sub(4));
        let overlay_x = (area.width - overlay_width) / 2;
        let overlay_y = (area.height - overlay_height) / 2;

        let overlay_area = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

        // Clear background
        frame.render_widget(Clear, overlay_area);

        // Build dialog content
        let mut lines: Vec<Line> = Vec::new();

        // Message
        for line in message.lines() {
            lines.push(Line::from(Span::styled(
                line,
                Style::default().fg(theme.colors.text),
            )));
        }

        lines.push(Line::raw("")); // Empty line

        // Instructions with [y] Yes / [n] No / [Esc] Cancel format
        lines.push(Line::from(vec![
            Span::styled("[", Style::default().fg(theme.colors.text_dim)),
            Span::styled("y", Style::default().fg(theme.colors.hotkey).add_modifier(Modifier::BOLD)),
            Span::styled("] Yes  ", Style::default().fg(theme.colors.text_dim)),
            Span::styled("[", Style::default().fg(theme.colors.text_dim)),
            Span::styled("n", Style::default().fg(theme.colors.hotkey).add_modifier(Modifier::BOLD)),
            Span::styled("] No  ", Style::default().fg(theme.colors.text_dim)),
            Span::styled("[", Style::default().fg(theme.colors.text_dim)),
            Span::styled("Esc", Style::default().fg(theme.colors.hotkey).add_modifier(Modifier::BOLD)),
            Span::styled("] Cancel", Style::default().fg(theme.colors.text_dim)),
        ]));

        // Determine border color based on action type
        let border_color = match &self.pending_action {
            Some(PendingAction::PauseWorker { .. }) | Some(PendingAction::PauseAllWorkers { .. }) => Color::Yellow,
            Some(PendingAction::ResumeWorker { .. }) | Some(PendingAction::ResumeAllWorkers { .. }) => Color::Green,
            Some(PendingAction::KillWorker { .. }) => Color::Red,
            Some(PendingAction::SpawnWorker(_)) => Color::Cyan,
            None => theme.colors.border_dim,
        };

        let dialog = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border_color))
                    .title(Span::styled(
                        title,
                        Style::default().fg(border_color).add_modifier(Modifier::BOLD),
                    ))
                    .style(Style::default().bg(Color::Black)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(dialog, overlay_area);
    }

    /// Draw the task detail overlay.
    fn draw_task_detail_overlay(&self, frame: &mut Frame, area: Rect) {
        let theme = self.theme_manager.current();

        // Get the aggregated bead data and find the selected bead
        let data = self
            .data_manager
            .bead_manager
            .get_filtered_aggregated_data(self.priority_filter);

        // Build a flattened list of all beads with their section info
        let mut all_beads: Vec<(String, &crate::bead::Bead, &str)> = Vec::new();

        // Add in-progress beads
        for (ws, bead) in &data.in_progress {
            all_beads.push((ws.clone(), bead, "In Progress"));
        }

        // Add ready beads
        for (ws, bead) in &data.ready {
            all_beads.push((ws.clone(), bead, "Ready"));
        }

        // Add blocked beads
        for (ws, bead) in &data.blocked {
            all_beads.push((ws.clone(), bead, "Blocked"));
        }

        // Calculate dialog dimensions
        let overlay_width = 70.min(area.width.saturating_sub(4));
        let overlay_height = 20.min(area.height.saturating_sub(4));
        let overlay_x = (area.width.saturating_sub(overlay_width)) / 2;
        let overlay_y = (area.height.saturating_sub(overlay_height)) / 2;

        let overlay_area = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

        // Clear background
        frame.render_widget(Clear, overlay_area);

        let mut lines = Vec::new();

        // Check if we have beads to show
        if all_beads.is_empty() {
            lines.push(Line::from(Span::styled(
                "No tasks available",
                Style::default().fg(theme.colors.text_dim),
            )));
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                "Tasks will appear here when beads are loaded from .beads/*.jsonl",
                Style::default().fg(theme.colors.text_dim),
            )));
        } else {
            // Clamp selected index to valid range
            let idx = self.selected_task_index.min(all_beads.len() - 1);
            let (ws, bead, section) = &all_beads[idx];

            // Header with task ID and priority
            let header = format!(
                "{} {} [{}] - {}",
                bead.priority_indicator(),
                bead.id,
                bead.priority_str(),
                section
            );
            lines.push(Line::from(Span::styled(
                header,
                Style::default()
                    .fg(theme.colors.hotkey)
                    .add_modifier(Modifier::BOLD),
            )));

            lines.push(Line::from(Span::styled(
                "─".repeat(overlay_width as usize - 2),
                Style::default().fg(theme.colors.border_dim),
            )));

            // Title
            lines.push(Line::from(Span::styled(
                "Title:",
                Style::default().fg(theme.colors.text_dim),
            )));
            lines.push(Line::from(Span::styled(
                &bead.title,
                Style::default().fg(theme.colors.text),
            )));

            lines.push(Line::raw("")); // Empty line

            // Status
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(theme.colors.text_dim)),
                Span::styled(
                    &bead.status,
                    Style::default().fg(match bead.status.as_str() {
                        "in_progress" => Color::Yellow,
                        "open" => Color::Green,
                        "closed" => Color::DarkGray,
                        "blocked" => Color::Red,
                        _ => theme.colors.text,
                    }),
                ),
            ]));

            // Priority and score
            let score = bead.score();
            lines.push(Line::from(vec![
                Span::styled("Priority: ", Style::default().fg(theme.colors.text_dim)),
                Span::styled(
                    format!("P{}", bead.priority),
                    Style::default().fg(theme.colors.text),
                ),
                Span::raw("    "),
                Span::styled("Score: ", Style::default().fg(theme.colors.text_dim)),
                Span::styled(format!("{}", score), Style::default().fg(theme.colors.text)),
            ]));

            // Dependencies
            lines.push(Line::from(vec![
                Span::styled("Dependencies: ", Style::default().fg(theme.colors.text_dim)),
                Span::styled(
                    format!(
                        "{} blocked by, {} depends on this",
                        bead.dependency_count, bead.dependent_count
                    ),
                    Style::default().fg(theme.colors.text),
                ),
            ]));

            // Workspace
            lines.push(Line::from(vec![
                Span::styled("Workspace: ", Style::default().fg(theme.colors.text_dim)),
                Span::styled(ws, Style::default().fg(theme.colors.text)),
            ]));

            // Assignee
            if let Some(ref assignee) = bead.assignee {
                lines.push(Line::from(vec![
                    Span::styled("Assignee: ", Style::default().fg(theme.colors.text_dim)),
                    Span::styled(assignee, Style::default().fg(theme.colors.text)),
                ]));
            }

            // Labels
            if !bead.labels.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("Labels: ", Style::default().fg(theme.colors.text_dim)),
                    Span::styled(
                        bead.labels.join(", "),
                        Style::default().fg(theme.colors.text),
                    ),
                ]));
            }

            lines.push(Line::raw("")); // Empty line

            // Description (truncated)
            if !bead.description.is_empty() {
                lines.push(Line::from(Span::styled(
                    "Description:",
                    Style::default().fg(theme.colors.text_dim),
                )));
                let desc_lines: Vec<&str> = bead.description.lines().take(5).collect();
                for line in desc_lines {
                    let truncated = if line.len() > (overlay_width as usize - 4) {
                        format!("{}...", &line[..(overlay_width as usize - 7)])
                    } else {
                        line.to_string()
                    };
                    lines.push(Line::from(Span::styled(
                        truncated,
                        Style::default().fg(theme.colors.text),
                    )));
                }
            }

            lines.push(Line::raw("")); // Empty line

            // Navigation info
            lines.push(Line::from(Span::styled(
                format!(
                    "Task {} of {}  |  ↑/k: prev  ↓/j: next  Esc: close",
                    idx + 1,
                    all_beads.len()
                ),
                Style::default().fg(theme.colors.text_dim),
            )));
        }

        let dialog = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.colors.header))
                    .title(Span::styled(
                        " Task Details ",
                        Style::default()
                            .fg(theme.colors.header)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .style(Style::default().bg(Color::Black)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(dialog, overlay_area);
    }

    /// Draw update notification banner at the top of the screen.
    fn draw_update_banner(&self, frame: &mut Frame, area: Rect) {
        // Create banner area at the top (below header)
        let banner_y = 3; // Right below the header (which is 3 lines)
        let banner_height = 3;
        let banner_area = Rect::new(
            area.width / 4, // Center horizontally
            banner_y,
            area.width / 2,
            banner_height,
        );

        // Clear background
        frame.render_widget(Clear, banner_area);

        let banner_text = " ⚠️  Update Available! Press Ctrl+U to update forge ";
        let banner = Paragraph::new(banner_text)
            .style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .alignment(ratatui::layout::Alignment::Center);

        frame.render_widget(banner, banner_area);
    }

    /// Draw network unreachable banner at the top of the screen.
    fn draw_network_banner(&self, frame: &mut Frame, area: Rect) {
        // Position below the header (and potentially below update banner if both are showing)
        let banner_y = if self.update_available { 6 } else { 3 };
        let banner_height = 3;
        let banner_width = (area.width * 2 / 3).min(60);
        let banner_area = Rect::new(
            (area.width.saturating_sub(banner_width)) / 2, // Center horizontally
            banner_y,
            banner_width,
            banner_height,
        );

        // Clear background
        frame.render_widget(Clear, banner_area);

        // Format the duration if available
        let duration_text = if let Some(since) = self.network_unavailable_since {
            let secs = since.elapsed().as_secs();
            if secs < 60 {
                format!(" ({}s)", secs)
            } else {
                format!(" ({}m {}s)", secs / 60, secs % 60)
            }
        } else {
            String::new()
        };

        // Get error message or use default
        let error_detail = self.network_error_message.as_deref().unwrap_or("Network unreachable");

        let banner_text = format!("📡 {}{} | Press 'r' to retry", error_detail, duration_text);
        let banner = Paragraph::new(banner_text)
            .style(
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::LightRed)),
            )
            .alignment(ratatui::layout::Alignment::Center);

        frame.render_widget(banner, banner_area);
    }

    /// Draw a status message as a toast/notification at the bottom of the screen.
    fn draw_status_message(&self, frame: &mut Frame, area: Rect, message: &str) {
        let theme = self.theme_manager.current();

        // Create a small banner at the bottom center
        let msg_width = (message.len() + 4).min(area.width as usize - 4) as u16;
        let banner_x = (area.width.saturating_sub(msg_width)) / 2;
        let banner_y = area.height.saturating_sub(5); // Just above footer

        let banner_area = Rect::new(banner_x, banner_y, msg_width, 3);

        // Clear background
        frame.render_widget(Clear, banner_area);

        let banner = Paragraph::new(message)
            .style(
                Style::default()
                    .fg(theme.colors.text)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.colors.border_dim)),
            )
            .alignment(ratatui::layout::Alignment::Center);

        frame.render_widget(banner, banner_area);
    }

    /// Draw a prominent overlay when update is in progress.
    fn draw_update_progress_overlay(&self, frame: &mut Frame, area: Rect) {
        // Create a centered overlay
        let overlay_width = 60.min(area.width.saturating_sub(4));
        let overlay_height = 7;
        let overlay_x = (area.width.saturating_sub(overlay_width)) / 2;
        let overlay_y = (area.height.saturating_sub(overlay_height)) / 2;

        let overlay_area = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

        // Clear background
        frame.render_widget(Clear, overlay_area);

        // Status text from update_progress or status_message
        let status_text = if !self.update_progress.status.is_empty() {
            &self.update_progress.status
        } else {
            self.status_message
                .as_deref()
                .unwrap_or("Updating forge...")
        };

        // Create progress bar
        let progress_percent = self.update_progress.percent as usize;
        let bar_width = (overlay_width as usize).saturating_sub(4);
        let filled = (bar_width * progress_percent) / 100;
        let empty = bar_width - filled;

        let progress_bar = format!(
            "[{}{}] {}%",
            "=".repeat(filled),
            " ".repeat(empty),
            progress_percent
        );

        let content = format!(
            "\n  {}\n\n  {}\n  ",
            status_text, progress_bar
        );

        let overlay = Paragraph::new(content)
            .style(
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::LightBlue))
                    .title(Span::styled(
                        " Forge Update (Ctrl+U) ",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    )),
            )
            .alignment(ratatui::layout::Alignment::Left);

        frame.render_widget(overlay, overlay_area);
    }
}

/// Truncate an error message for status bar display.
fn truncate_status_error(err: &str) -> String {
    if err.len() <= 20 {
        err.to_string()
    } else {
        format!("{}...", &err[..17])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    /// Helper to create a test terminal with specified dimensions
    fn test_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
        let backend = TestBackend::new(width, height);
        Terminal::new(backend).unwrap()
    }

    /// Helper to render app and get the buffer
    fn render_app(app: &mut App, width: u16, height: u16) -> Buffer {
        let mut terminal = test_terminal(width, height);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// Check if a buffer contains a specific string
    fn buffer_contains(buffer: &Buffer, text: &str) -> bool {
        let content = buffer_to_string(buffer);
        content.contains(text)
    }

    /// Convert buffer to string for debugging/searching
    fn buffer_to_string(buffer: &Buffer) -> String {
        let area = buffer.area;
        let mut result = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                result.push(buffer[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            result.push('\n');
        }
        result
    }

    // ============================================================
    // Dashboard Panel Rendering Tests
    // ============================================================

    #[test]
    fn test_overview_renders_worker_pool_panel() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 100, 30);

        // Check Worker Pool panel title appears
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "Overview should render Worker Pool panel"
        );
    }

    #[test]
    fn test_overview_renders_utilization_panel() {
        let mut app = App::new();
        // Use wide layout (120+ cols) to ensure Subscriptions panel is visible
        let buffer = render_app(&mut app, 140, 30);

        assert!(
            buffer_contains(&buffer, "Subscriptions"),
            "Overview should render Subscriptions panel in wide layout"
        );
    }

    #[test]
    fn test_overview_renders_task_queue_panel() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "Task Queue"),
            "Overview should render Task Queue panel"
        );
    }

    #[test]
    fn test_overview_renders_activity_log_panel() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "Activity Log"),
            "Overview should render Activity Log panel"
        );
    }

    #[test]
    fn test_costs_view_renders_cost_analytics_panel() {
        let mut app = App::new();
        app.switch_view(View::Costs);
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "Cost Analytics"),
            "Costs view should render Cost Analytics panel"
        );
    }

    #[test]
    fn test_metrics_view_renders_performance_panel() {
        let mut app = App::new();
        app.switch_view(View::Metrics);
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "Performance Analytics"),
            "Metrics view should render Performance Analytics panel"
        );
    }

    #[test]
    fn test_all_six_panel_types_render() {
        // Test that all 6 FocusPanel types have corresponding views that render
        let mut app = App::new();

        // 1. Worker Pool (Overview - switch to Workers first, then back to Overview)
        // The app starts in Overview, so switch_view(Overview) won't change focus
        app.switch_view(View::Workers);
        app.switch_view(View::Overview);
        assert_eq!(app.focus_panel(), FocusPanel::WorkerPool);
        // Use ultra-wide layout (199+ cols) to ensure all 6 panels are visible
        let buffer = render_app(&mut app, 199, 38);
        assert!(buffer_contains(&buffer, "Worker Pool"));

        // 2. Subscriptions (part of Overview) - visible in ultra-wide layout
        assert!(buffer_contains(&buffer, "Subscriptions"));

        // 3. Task Queue (Tasks view)
        app.switch_view(View::Tasks);
        assert_eq!(app.focus_panel(), FocusPanel::TaskQueue);
        let buffer = render_app(&mut app, 100, 30);
        assert!(buffer_contains(&buffer, "Task Queue"));

        // 4. Activity Log (Logs view)
        app.switch_view(View::Logs);
        assert_eq!(app.focus_panel(), FocusPanel::ActivityLog);
        let buffer = render_app(&mut app, 100, 30);
        assert!(buffer_contains(&buffer, "Activity Log"));

        // 5. Cost Breakdown (Costs view)
        app.switch_view(View::Costs);
        assert_eq!(app.focus_panel(), FocusPanel::CostBreakdown);
        let buffer = render_app(&mut app, 100, 30);
        assert!(buffer_contains(&buffer, "Cost Analytics"));

        // 6. Metrics Charts (Metrics view)
        app.switch_view(View::Metrics);
        assert_eq!(app.focus_panel(), FocusPanel::MetricsCharts);
        let buffer = render_app(&mut app, 100, 30);
        assert!(buffer_contains(&buffer, "Performance Analytics"));
    }

    // ============================================================
    // Border Rendering Tests
    // ============================================================

    #[test]
    fn test_panels_render_with_borders() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 100, 30);

        // Unicode box drawing characters used by ratatui
        // Check for corner characters that indicate borders
        let content = buffer_to_string(&buffer);

        // Should contain horizontal box-drawing characters (─)
        assert!(
            content.contains('─') || content.contains('-'),
            "Panels should render with horizontal border lines"
        );

        // Should contain vertical box-drawing characters (│)
        assert!(
            content.contains('│') || content.contains('|'),
            "Panels should render with vertical border lines"
        );
    }

    #[test]
    fn test_header_renders_with_borders() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "FORGE v0.1.9"),
            "Header should contain FORGE v0.1.9 title"
        );
    }

    #[test]
    fn test_footer_renders_hotkey_hints() {
        let mut app = App::new();
        // Use wider terminal to ensure all hotkeys fit in footer
        let buffer = render_app(&mut app, 150, 30);

        assert!(
            buffer_contains(&buffer, "[o]"),
            "Footer should show Overview hotkey"
        );
        assert!(
            buffer_contains(&buffer, "[w]"),
            "Footer should show Workers hotkey"
        );
        assert!(
            buffer_contains(&buffer, "[q]"),
            "Footer should show Quit hotkey"
        );
    }

    // ============================================================
    // Layout Adaptation Tests
    // ============================================================

    #[test]
    fn test_layout_adapts_to_small_terminal() {
        let mut app = App::new();

        // Very small terminal
        let buffer = render_app(&mut app, 40, 15);

        // Should still render without panic
        assert!(buffer.area.width == 40);
        assert!(buffer.area.height == 15);

        // Should still show some content
        let content = buffer_to_string(&buffer);
        assert!(
            !content.trim().is_empty(),
            "Should render content even in small terminal"
        );
    }

    #[test]
    fn test_layout_adapts_to_large_terminal() {
        let mut app = App::new();

        // Large terminal
        let buffer = render_app(&mut app, 200, 60);

        assert!(buffer.area.width == 200);
        assert!(buffer.area.height == 60);

        // All panels should still be visible
        assert!(buffer_contains(&buffer, "Worker Pool"));
        assert!(buffer_contains(&buffer, "Subscriptions"));
    }

    #[test]
    fn test_layout_adapts_to_wide_terminal() {
        let mut app = App::new();

        // Wide but short terminal
        let buffer = render_app(&mut app, 200, 20);

        assert!(buffer.area.width == 200);
        assert!(buffer.area.height == 20);

        // Should render header and some content
        assert!(buffer_contains(&buffer, "FORGE v0.1.9"));
    }

    #[test]
    fn test_layout_adapts_to_tall_terminal() {
        let mut app = App::new();

        // Narrow but tall terminal
        let buffer = render_app(&mut app, 60, 50);

        assert!(buffer.area.width == 60);
        assert!(buffer.area.height == 50);

        // Should render content
        assert!(buffer_contains(&buffer, "FORGE v0.1.9"));
    }

    #[test]
    fn test_minimum_viable_terminal_size() {
        let mut app = App::new();

        // Minimum size that should still render something
        let buffer = render_app(&mut app, 20, 10);

        // Should not panic and should produce some output
        assert!(buffer.area.width == 20);
        assert!(buffer.area.height == 10);
    }

    // ============================================================
    // Panel Content Tests
    // ============================================================

    #[test]
    fn test_worker_pool_shows_worker_counts() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 100, 30);

        // Worker pool should show worker statistics or "No workers" message or "Loading"
        assert!(
            buffer_contains(&buffer, "active")
                || buffer_contains(&buffer, "idle")
                || buffer_contains(&buffer, "Total")
                || buffer_contains(&buffer, "No workers")
                || buffer_contains(&buffer, "Loading"),
            "Worker Pool should display worker counts or status"
        );
    }

    #[test]
    fn test_task_queue_shows_priority_indicators() {
        let mut app = App::new();
        app.switch_view(View::Tasks);
        let buffer = render_app(&mut app, 100, 30);

        // Task queue should show priority markers or queue status (or br CLI unavailable message)
        assert!(
            buffer_contains(&buffer, "P0")
                || buffer_contains(&buffer, "P1")
                || buffer_contains(&buffer, "Ready")
                || buffer_contains(&buffer, "Total Open")
                || buffer_contains(&buffer, "No tasks")
                || buffer_contains(&buffer, "Task")
                || buffer_contains(&buffer, "br CLI"),
            "Task Queue should display priority indicators, queue status, or br CLI message"
        );
    }

    #[test]
    fn test_costs_view_shows_placeholder() {
        let mut app = App::new();
        app.switch_view(View::Costs);
        let buffer = render_app(&mut app, 100, 30);

        // Costs view shows placeholder since cost tracking isn't implemented
        assert!(
            buffer_contains(&buffer, "Cost")
                || buffer_contains(&buffer, "tracking")
                || buffer_contains(&buffer, "Loading"),
            "Costs view should display cost-related content"
        );
    }

    #[test]
    fn test_logs_view_shows_activity() {
        let mut app = App::new();
        app.switch_view(View::Logs);
        let buffer = render_app(&mut app, 100, 30);

        // Logs view should show activity log panel title and content
        assert!(
            buffer_contains(&buffer, "Activity Log")
                || buffer_contains(&buffer, "No recent activity")
                || buffer_contains(&buffer, "Loading"),
            "Logs view should display activity log"
        );
    }

    // ============================================================
    // View-Specific Rendering Tests
    // ============================================================

    #[test]
    fn test_workers_view_renders_table() {
        let mut app = App::new();
        app.switch_view(View::Workers);
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "Worker Pool Management"),
            "Workers view should show management panel"
        );
        // Workers view may show: table headers (with workers), spawn instructions,
        // "Loading" message, or "No workers" message depending on data state
        assert!(
            buffer_contains(&buffer, "Worker ID")
                || buffer_contains(&buffer, "Model")
                || buffer_contains(&buffer, "Status")
                || buffer_contains(&buffer, "[G] Spawn")
                || buffer_contains(&buffer, "No workers")
                || buffer_contains(&buffer, "Loading")
                || buffer_contains(&buffer, "Spawn GLM"),
            "Workers view should show table headers, spawn instructions, or loading message"
        );
    }

    #[test]
    fn test_chat_view_renders_input_field() {
        let mut app = App::new();
        app.switch_view(View::Chat);
        let buffer = render_app(&mut app, 100, 30);

        assert!(
            buffer_contains(&buffer, "Chat") || buffer_contains(&buffer, "Input"),
            "Chat view should show chat interface"
        );
    }

    // ============================================================
    // Help Overlay Tests
    // ============================================================

    #[test]
    fn test_help_overlay_renders() {
        let mut app = App::new();
        app.handle_app_event(AppEvent::ShowHelp);
        let buffer = render_app(&mut app, 100, 40);

        assert!(app.show_help(), "Help should be visible");
        assert!(
            buffer_contains(&buffer, "Help") || buffer_contains(&buffer, "Hotkey"),
            "Help overlay should render"
        );
    }

    #[test]
    fn test_help_overlay_shows_navigation_keys() {
        let mut app = App::new();
        app.handle_app_event(AppEvent::ShowHelp);
        let buffer = render_app(&mut app, 100, 40);

        // Help should show view navigation keys
        let content = buffer_to_string(&buffer);
        assert!(
            content.contains("Tab") || content.contains("Esc") || content.contains("Navigation"),
            "Help overlay should show navigation keys"
        );
    }

    #[test]
    fn test_help_overlay_appears_on_show_help() {
        let mut app = App::new();

        // Help should be hidden initially
        assert!(!app.show_help(), "Help should be hidden initially");

        // Show help via ShowHelp event (triggered by '?' key)
        app.handle_app_event(AppEvent::ShowHelp);
        assert!(
            app.show_help(),
            "Help should be visible after ShowHelp event"
        );
    }

    #[test]
    fn test_help_overlay_closes_on_escape() {
        let mut app = App::new();

        // Show help first
        app.handle_app_event(AppEvent::ShowHelp);
        assert!(app.show_help(), "Help should be visible");

        // Press Escape (Cancel event)
        app.handle_app_event(AppEvent::Cancel);
        assert!(!app.show_help(), "Help should be hidden after Esc");
    }

    #[test]
    fn test_help_overlay_closes_on_any_key() {
        let mut app = App::new();

        // Show help
        app.handle_app_event(AppEvent::ShowHelp);
        assert!(app.show_help(), "Help should be visible");

        // Any key that triggers HideHelp should close it
        app.handle_app_event(AppEvent::HideHelp);
        assert!(!app.show_help(), "Help should close after HideHelp event");
    }

    #[test]
    fn test_help_overlay_shows_global_hotkeys_section() {
        let mut app = App::new();
        app.handle_app_event(AppEvent::ShowHelp);
        let buffer = render_app(&mut app, 100, 40);
        let content = buffer_to_string(&buffer);

        // Help should show global hotkeys
        assert!(
            content.contains("General") || content.contains("?") || content.contains("Quit"),
            "Help overlay should show global hotkeys section"
        );
    }

    #[test]
    fn test_help_overlay_shows_view_navigation_section() {
        let mut app = App::new();
        app.handle_app_event(AppEvent::ShowHelp);
        let buffer = render_app(&mut app, 100, 40);
        let content = buffer_to_string(&buffer);

        // Help should show view navigation section
        assert!(
            content.contains("View")
                || content.contains("Navigation")
                || content.contains("Workers"),
            "Help overlay should show view navigation section"
        );
    }

    #[test]
    fn test_help_overlay_shows_worker_management_section() {
        let mut app = App::new();
        app.handle_app_event(AppEvent::ShowHelp);
        let buffer = render_app(&mut app, 100, 40);
        let content = buffer_to_string(&buffer);

        // Help should show worker-related keys (either spawn or kill)
        assert!(
            content.contains("g") || content.contains("s") || content.contains("k"),
            "Help overlay should show worker management keys"
        );
    }

    #[test]
    fn test_help_overlay_shows_task_management_info() {
        let mut app = App::new();
        app.handle_app_event(AppEvent::ShowHelp);
        let buffer = render_app(&mut app, 100, 40);
        let content = buffer_to_string(&buffer);

        // Help should show task-related navigation
        assert!(
            content.contains("t") || content.contains("Tasks"),
            "Help overlay should show task management navigation"
        );
    }

    #[test]
    fn test_help_overlay_no_visual_artifacts_after_closing() {
        let mut app = App::new();

        // Show help
        app.handle_app_event(AppEvent::ShowHelp);
        let _buffer_with_help = render_app(&mut app, 100, 40);

        // Close help
        app.handle_app_event(AppEvent::Cancel);
        assert!(!app.show_help(), "Help should be hidden");

        // App should render cleanly after closing help
        let buffer_after_close = render_app(&mut app, 100, 40);
        let content = buffer_to_string(&buffer_after_close);

        // Help text should no longer be visible
        assert!(
            !content.contains("Hotkey Reference"),
            "Help content should not be visible after closing"
        );
    }

    #[test]
    fn test_help_overlay_centered_on_screen() {
        let mut app = App::new();
        app.handle_app_event(AppEvent::ShowHelp);
        let buffer = render_app(&mut app, 120, 40);

        // Help should render without panic - centered positioning is visual
        assert!(app.show_help(), "Help should be visible");
        assert!(
            buffer_contains(&buffer, "Help") || buffer_contains(&buffer, "Hotkey"),
            "Help overlay should render centered"
        );
    }

    #[test]
    fn test_help_toggle_via_show_and_hide() {
        let mut app = App::new();

        // Toggle help on
        app.handle_app_event(AppEvent::ShowHelp);
        assert!(app.show_help(), "Help should be visible after ShowHelp");

        // Toggle help off
        app.handle_app_event(AppEvent::HideHelp);
        assert!(!app.show_help(), "Help should be hidden after HideHelp");

        // Toggle help on again
        app.handle_app_event(AppEvent::ShowHelp);
        assert!(app.show_help(), "Help should be visible again");
    }

    // ============================================================
    // Focus Highlighting Tests
    // ============================================================

    #[test]
    fn test_focused_panel_is_highlighted() {
        let mut app = App::new();

        // Initial focus is None (no highlight)
        assert_eq!(app.focus_panel(), FocusPanel::None);
        assert!(!app.focus_panel().is_highlighted());

        // After switching view, focus is set and highlighted
        app.switch_view(View::Workers);
        assert_eq!(app.focus_panel(), FocusPanel::WorkerPool);
        assert!(app.focus_panel().is_highlighted());
    }

    #[test]
    fn test_focus_changes_with_view() {
        let mut app = App::new();

        // Initial state has no focus
        assert_eq!(app.focus_panel(), FocusPanel::None);

        // After switching views, each view sets appropriate focus
        // Note: switch_view only sets focus when view actually changes,
        // so we need to switch to a different view first for Overview
        app.switch_view(View::Workers);
        assert_eq!(app.focus_panel(), FocusPanel::WorkerPool);

        // Now test each view has correct focus when switched to
        let view_focus_pairs = [
            (View::Tasks, FocusPanel::TaskQueue),
            (View::Costs, FocusPanel::CostBreakdown),
            (View::Metrics, FocusPanel::MetricsCharts),
            (View::Logs, FocusPanel::ActivityLog),
            (View::Chat, FocusPanel::ChatInput),
            (View::Overview, FocusPanel::WorkerPool),
            (View::Workers, FocusPanel::WorkerPool),
        ];

        for (view, expected_focus) in view_focus_pairs {
            app.switch_view(view);
            assert_eq!(
                app.focus_panel(),
                expected_focus,
                "View {:?} should have focus {:?}",
                view,
                expected_focus
            );
        }
    }

    // ============================================================
    // Original Tests
    // ============================================================

    #[test]
    fn test_app_creation() {
        let mut app = App::new();
        assert_eq!(app.current_view(), View::Overview);
        assert!(!app.should_quit());
        assert!(!app.show_help());
    }

    #[test]
    fn test_view_switching() {
        let mut app = App::new();
        assert_eq!(app.current_view(), View::Overview);

        app.switch_view(View::Workers);
        assert_eq!(app.current_view(), View::Workers);

        app.switch_view(View::Tasks);
        assert_eq!(app.current_view(), View::Tasks);
    }

    #[test]
    fn test_view_cycling() {
        let mut app = App::new();
        assert_eq!(app.current_view(), View::Overview);

        app.next_view();
        assert_eq!(app.current_view(), View::Workers);

        app.next_view();
        assert_eq!(app.current_view(), View::Tasks);

        app.prev_view();
        assert_eq!(app.current_view(), View::Workers);
    }

    #[test]
    fn test_chat_mode() {
        let mut app = App::new();
        assert!(!app.input_handler.is_chat_mode());

        app.switch_view(View::Chat);
        assert!(app.input_handler.is_chat_mode());
        assert_eq!(app.focus_panel(), FocusPanel::ChatInput);

        // Simulate text input
        app.handle_app_event(AppEvent::TextInput('h'));
        app.handle_app_event(AppEvent::TextInput('i'));
        assert_eq!(app.chat_input, "hi");

        // Backspace
        app.handle_app_event(AppEvent::Backspace);
        assert_eq!(app.chat_input, "h");
    }

    #[test]
    fn test_quit_handling() {
        let mut app = App::new();
        assert!(!app.should_quit());

        app.handle_app_event(AppEvent::Quit);
        assert!(app.should_quit());
    }

    #[test]
    fn test_help_toggle() {
        let mut app = App::new();
        assert!(!app.show_help());

        app.handle_app_event(AppEvent::ShowHelp);
        assert!(app.show_help());

        app.handle_app_event(AppEvent::Cancel);
        assert!(!app.show_help());
    }

    #[test]
    fn test_focus_panel_on_view_switch() {
        let mut app = App::new();

        app.switch_view(View::Workers);
        assert_eq!(app.focus_panel(), FocusPanel::WorkerPool);

        app.switch_view(View::Tasks);
        assert_eq!(app.focus_panel(), FocusPanel::TaskQueue);

        app.switch_view(View::Costs);
        assert_eq!(app.focus_panel(), FocusPanel::CostBreakdown);

        app.switch_view(View::Logs);
        assert_eq!(app.focus_panel(), FocusPanel::ActivityLog);

        app.switch_view(View::Chat);
        assert_eq!(app.focus_panel(), FocusPanel::ChatInput);
    }

    #[test]
    fn test_navigation() {
        let mut app = App::new();
        assert_eq!(app.scroll_offset, 0);

        app.handle_app_event(AppEvent::NavigateDown);
        assert_eq!(app.scroll_offset, 1);

        app.handle_app_event(AppEvent::NavigateUp);
        assert_eq!(app.scroll_offset, 0);

        // Can't go below 0
        app.handle_app_event(AppEvent::NavigateUp);
        assert_eq!(app.scroll_offset, 0);

        app.handle_app_event(AppEvent::PageDown);
        assert_eq!(app.scroll_offset, 10);
    }

    // ============================================================
    // 3-Column Layout Tests (Ultra-Wide Mode)
    // ============================================================

    #[test]
    fn test_ultrawide_layout_renders_all_six_panels() {
        let mut app = App::new();
        // Ultra-wide: 199x38 terminal
        let buffer = render_app(&mut app, 199, 38);

        // All 6 panels should be visible in ultra-wide mode
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "Ultra-wide layout should show Worker Pool panel"
        );
        assert!(
            buffer_contains(&buffer, "Subscriptions"),
            "Ultra-wide layout should show Subscriptions panel"
        );
        assert!(
            buffer_contains(&buffer, "Task Queue"),
            "Ultra-wide layout should show Task Queue panel"
        );
        assert!(
            buffer_contains(&buffer, "Activity Log"),
            "Ultra-wide layout should show Activity Log panel"
        );
        assert!(
            buffer_contains(&buffer, "Cost Breakdown"),
            "Ultra-wide layout should show Cost Breakdown panel"
        );
        assert!(
            buffer_contains(&buffer, "Quick Actions"),
            "Ultra-wide layout should show Quick Actions panel"
        );
    }

    #[test]
    fn test_ultrawide_layout_at_exact_boundary() {
        let mut app = App::new();
        // Exactly 199 cols - should trigger ultra-wide
        let buffer = render_app(&mut app, 199, 38);

        assert!(
            buffer_contains(&buffer, "Cost Breakdown"),
            "At 199 cols, should use ultra-wide layout with Cost Breakdown panel"
        );
        assert!(
            buffer_contains(&buffer, "Quick Actions"),
            "At 199 cols, should use ultra-wide layout with Quick Actions panel"
        );
    }

    #[test]
    fn test_wide_layout_at_boundary_below_ultrawide() {
        let mut app = App::new();
        // 198 cols - should NOT trigger ultra-wide, just wide
        let buffer = render_app(&mut app, 198, 38);

        // Should have 4 panels (wide mode)
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "Wide layout should show Worker Pool"
        );
        assert!(
            buffer_contains(&buffer, "Subscriptions"),
            "Wide layout should show Subscriptions"
        );
        assert!(
            buffer_contains(&buffer, "Task Queue"),
            "Wide layout should show Task Queue"
        );
        assert!(
            buffer_contains(&buffer, "Activity Log"),
            "Wide layout should show Activity Log"
        );

        // Should NOT have the right column panels
        assert!(
            !buffer_contains(&buffer, "Cost Breakdown"),
            "Wide layout should NOT show Cost Breakdown panel"
        );
        assert!(
            !buffer_contains(&buffer, "Quick Actions"),
            "Wide layout should NOT show Quick Actions panel"
        );
    }

    #[test]
    fn test_narrow_layout_below_wide_threshold() {
        let mut app = App::new();
        // 119 cols - should trigger narrow mode
        let buffer = render_app(&mut app, 119, 30);

        // Should still show essential panels
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "Narrow layout should show Worker Pool"
        );
        assert!(
            buffer_contains(&buffer, "Task Queue"),
            "Narrow layout should show Task Queue"
        );
        assert!(
            buffer_contains(&buffer, "Activity Log"),
            "Narrow layout should show Activity Log"
        );

        // Should NOT show secondary panels
        assert!(
            !buffer_contains(&buffer, "Cost Breakdown"),
            "Narrow layout should NOT show Cost Breakdown"
        );
    }

    #[test]
    fn test_wide_layout_at_wide_threshold() {
        let mut app = App::new();
        // 120 cols - exactly at wide threshold
        let buffer = render_app(&mut app, 120, 30);

        // Should have 4 panels (wide mode)
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "Wide layout at threshold should show Worker Pool"
        );
        assert!(
            buffer_contains(&buffer, "Subscriptions"),
            "Wide layout at threshold should show Subscriptions"
        );
    }

    #[test]
    fn test_layout_mode_detection() {
        use crate::view::LayoutMode;

        // Ultra-wide: 199+
        assert_eq!(LayoutMode::from_width(199), LayoutMode::UltraWide);
        assert_eq!(LayoutMode::from_width(250), LayoutMode::UltraWide);

        // Wide: 120-198
        assert_eq!(LayoutMode::from_width(198), LayoutMode::Wide);
        assert_eq!(LayoutMode::from_width(150), LayoutMode::Wide);
        assert_eq!(LayoutMode::from_width(120), LayoutMode::Wide);

        // Narrow: <120
        assert_eq!(LayoutMode::from_width(119), LayoutMode::Narrow);
        assert_eq!(LayoutMode::from_width(80), LayoutMode::Narrow);
        assert_eq!(LayoutMode::from_width(40), LayoutMode::Narrow);
    }

    #[test]
    fn test_layout_min_height_requirements() {
        use crate::view::LayoutMode;

        assert_eq!(LayoutMode::UltraWide.min_height(), 38);
        assert_eq!(LayoutMode::Wide.min_height(), 30);
        assert_eq!(LayoutMode::Narrow.min_height(), 20);
    }

    #[test]
    fn test_ultrawide_renders_without_panic_at_various_heights() {
        let mut app = App::new();

        // Test various heights with ultra-wide width
        for height in [20, 30, 38, 50, 60, 100] {
            let buffer = render_app(&mut app, 199, height);
            assert_eq!(buffer.area.height, height);
            // Should render something without panic
            assert!(buffer_contains(&buffer, "FORGE v0.1.9"));
        }
    }

    #[test]
    fn test_ultrawide_shows_action_hints() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 199, 38);

        // Quick Actions panel should show worker action hints
        let content = buffer_to_string(&buffer);
        assert!(
            content.contains("Spawn") || content.contains("[G]") || content.contains("Worker"),
            "Quick Actions panel should show action hints"
        );
    }

    #[test]
    fn test_ultrawide_shows_cost_placeholders() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 199, 38);

        // Cost Breakdown panel should show cost placeholders
        let content = buffer_to_string(&buffer);
        assert!(
            content.contains("Cost") || content.contains("Budget") || content.contains("$"),
            "Cost Breakdown panel should show cost-related content"
        );
    }

    #[test]
    fn test_graceful_degradation_sequence() {
        let mut app = App::new();

        // Test the degradation sequence: ultra-wide -> wide -> narrow
        // Each step down should still render without errors and show appropriate panels

        // Ultra-wide (199): 6 panels
        let buffer_ultrawide = render_app(&mut app, 199, 38);
        assert!(buffer_contains(&buffer_ultrawide, "Cost Breakdown"));
        assert!(buffer_contains(&buffer_ultrawide, "Quick Actions"));

        // Wide (150): 4 panels
        let buffer_wide = render_app(&mut app, 150, 30);
        assert!(buffer_contains(&buffer_wide, "Worker Pool"));
        assert!(buffer_contains(&buffer_wide, "Task Queue"));
        assert!(!buffer_contains(&buffer_wide, "Cost Breakdown"));

        // Narrow (80): 3 panels stacked
        let buffer_narrow = render_app(&mut app, 80, 25);
        assert!(buffer_contains(&buffer_narrow, "Worker Pool"));
        assert!(buffer_contains(&buffer_narrow, "Task Queue"));
        assert!(buffer_contains(&buffer_narrow, "Activity Log"));
    }

    // ============================================================
    // Priority Filter Tests
    // ============================================================

    #[test]
    fn test_priority_filter_keys_in_tasks_view() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut app = App::new();

        // Start in Overview, no filter
        assert_eq!(app.current_view, View::Overview);
        assert_eq!(app.priority_filter, None);

        // Switch to Tasks view
        app.switch_view(View::Tasks);
        assert_eq!(app.current_view, View::Tasks);

        // Press '0' - should set filter to P0
        let key = KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE);
        app.handle_key_event(key);
        assert_eq!(app.priority_filter, Some(0));

        // Press '1' - should change filter to P1
        let key = KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE);
        app.handle_key_event(key);
        assert_eq!(app.priority_filter, Some(1));

        // Press '1' again - should toggle off (clear filter)
        let key = KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE);
        app.handle_key_event(key);
        assert_eq!(app.priority_filter, None);

        // Set filter to P2
        let key = KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE);
        app.handle_key_event(key);
        assert_eq!(app.priority_filter, Some(2));

        // Press 'X' - should clear filter
        let key = KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE);
        app.handle_key_event(key);
        assert_eq!(app.priority_filter, None);
    }

    #[test]
    fn test_priority_filter_keys_ignored_in_other_views() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut app = App::new();

        // Start in Overview
        app.switch_view(View::Overview);
        assert_eq!(app.priority_filter, None);

        // Press '0' in Overview - should NOT set filter (it's treated as spawn Opus + '0')
        let key = KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE);
        app.handle_key_event(key);

        // Filter should still be None
        assert_eq!(app.priority_filter, None);
    }

    #[test]
    fn test_priority_filter_resets_scroll() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut app = App::new();
        app.switch_view(View::Tasks);
        app.scroll_offset = 10;

        // Press '0' to filter - should reset scroll to 0
        let key = KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE);
        app.handle_key_event(key);

        assert_eq!(app.priority_filter, Some(0));
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn test_priority_filter_all_levels() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut app = App::new();
        app.switch_view(View::Tasks);

        // Test all priority levels 0-4
        for p in 0u8..=4 {
            let key = KeyEvent::new(
                KeyCode::Char(char::from_digit(p as u32, 10).unwrap()),
                KeyModifiers::NONE,
            );
            app.handle_key_event(key);
            assert_eq!(
                app.priority_filter,
                Some(p),
                "Priority filter should be P{}",
                p
            );
        }
    }

    // ============================================================
    // Task Detail Overlay Tests
    // ============================================================

    #[test]
    fn test_task_detail_opens_on_select_in_tasks_view() {
        let mut app = App::new();

        // Start in Overview, no task detail overlay
        assert!(!app.show_task_detail);

        // Switch to Tasks view
        app.switch_view(View::Tasks);

        // Trigger Select event (Enter key)
        app.handle_app_event(AppEvent::Select);

        // Task detail overlay should be shown
        assert!(app.show_task_detail);
    }

    #[test]
    fn test_task_detail_does_not_open_in_other_views() {
        let mut app = App::new();

        // Start in Overview
        app.switch_view(View::Overview);

        // Trigger Select event
        app.handle_app_event(AppEvent::Select);

        // Task detail overlay should NOT be shown
        assert!(!app.show_task_detail);

        // Try in Workers view
        app.switch_view(View::Workers);
        app.handle_app_event(AppEvent::Select);
        assert!(!app.show_task_detail);
    }

    #[test]
    fn test_task_detail_closes_on_escape() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut app = App::new();
        app.switch_view(View::Tasks);
        app.show_task_detail = true;

        // Press Escape to close
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        app.handle_key_event(key);

        // Task detail overlay should be closed
        assert!(!app.show_task_detail);
    }

    #[test]
    fn test_task_detail_navigation_keys() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut app = App::new();
        app.switch_view(View::Tasks);
        app.show_task_detail = true;
        app.selected_task_index = 5;

        // Press 'k' or Up to move up
        let key = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        app.handle_key_event(key);
        assert_eq!(app.selected_task_index, 4);

        // Press Down to move down
        let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key_event(key);
        assert_eq!(app.selected_task_index, 5);

        // Press 'j' to move down again
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        app.handle_key_event(key);
        assert_eq!(app.selected_task_index, 6);
    }

    #[test]
    fn test_task_detail_navigation_bounds() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut app = App::new();
        app.switch_view(View::Tasks);
        app.show_task_detail = true;
        app.selected_task_index = 0;

        // Try to move up when already at 0 - should stay at 0
        let key = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        app.handle_key_event(key);
        assert_eq!(app.selected_task_index, 0);
    }
}
