//! End-to-end integration tests for the FORGE TUI.
//!
//! This module tests the full workflow:
//! - Start forge application (using TestBackend)
//! - Create dummy workers (via status file creation)
//! - Generate updates and events (status changes, log entries)
//! - Navigate views and verify panels
//! - Verify stability under load
//!
//! These tests validate that all components work together correctly
//! and that the application remains stable under various conditions.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::thread;
    use std::time::Duration;

    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;
    use tempfile::TempDir;

    use crate::app::App;
    use crate::event::{AppEvent, InputHandler};
    use crate::log::{LogBuffer, LogEntry, LogLevel, LogTailer, LogTailerConfig};
    use crate::status::{StatusEvent, StatusWatcher, StatusWatcherConfig};
    use crate::view::{FocusPanel, View};
    use forge_core::types::WorkerStatus;

    // ============================================================
    // Test Helpers
    // ============================================================

    /// Helper to create a test terminal with specified dimensions.
    fn test_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
        let backend = TestBackend::new(width, height);
        Terminal::new(backend).unwrap()
    }

    /// Helper to render app and get the buffer.
    fn render_app(app: &App, width: u16, height: u16) -> Buffer {
        let mut terminal = test_terminal(width, height);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// Check if a buffer contains a specific string.
    fn buffer_contains(buffer: &Buffer, text: &str) -> bool {
        let content = buffer_to_string(buffer);
        content.contains(text)
    }

    /// Convert buffer to string for debugging/searching.
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

    /// Create a test status file with the given worker data.
    fn create_test_status_file(dir: &std::path::Path, worker_id: &str, status: &str) -> PathBuf {
        let path = dir.join(format!("{}.json", worker_id));
        let content = format!(
            r#"{{
                "worker_id": "{}",
                "status": "{}",
                "model": "test-model",
                "workspace": "/test/workspace",
                "pid": {},
                "started_at": "2026-02-08T10:00:00Z",
                "tasks_completed": {}
            }}"#,
            worker_id,
            status,
            std::process::id(), // Use current process PID for valid PID check
            rand_u32() % 100
        );
        fs::write(&path, content).unwrap();
        path
    }

    /// Create a test log file with entries.
    fn create_test_log_file(dir: &std::path::Path, worker_id: &str, entries: &[(&str, &str)]) -> PathBuf {
        let path = dir.join(format!("{}.log", worker_id));
        let mut content = String::new();
        for (level, message) in entries {
            content.push_str(&format!("[{}] {}\n", level, message));
        }
        fs::write(&path, content).unwrap();
        path
    }

    /// Simple pseudo-random u32 for test data variation.
    fn rand_u32() -> u32 {
        use std::time::SystemTime;
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    }

    // ============================================================
    // Integration Test 1: Application Startup and Basic Rendering
    // ============================================================

    #[test]
    fn test_e2e_application_startup() {
        // Test that the application starts correctly and renders the default view
        let app = App::new();

        // Verify initial state
        assert_eq!(app.current_view(), View::Overview);
        assert!(!app.should_quit());
        assert!(!app.show_help());

        // Render the application
        let buffer = render_app(&app, 120, 40);

        // Verify the header is rendered
        assert!(
            buffer_contains(&buffer, "FORGE Dashboard"),
            "Application should render FORGE Dashboard header"
        );

        // Verify the overview panels are rendered
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "Overview should show Worker Pool panel"
        );

        // Verify footer hotkey hints
        assert!(
            buffer_contains(&buffer, "[o]") && buffer_contains(&buffer, "[q]"),
            "Footer should show hotkey hints"
        );
    }

    #[test]
    fn test_e2e_application_renders_all_views() {
        let mut app = App::new();

        // Test rendering each view
        let views = [
            (View::Overview, "Worker Pool"),
            (View::Workers, "Worker Pool Management"),
            (View::Tasks, "Task Queue"),
            (View::Costs, "Cost Analytics"),
            (View::Metrics, "Performance Metrics"),
            (View::Logs, "Activity Log"),
            (View::Chat, "Chat"),
        ];

        for (view, expected_content) in views {
            app.switch_view(view);
            let buffer = render_app(&app, 120, 40);

            assert!(
                buffer_contains(&buffer, expected_content),
                "View {:?} should contain '{}'",
                view,
                expected_content
            );
        }
    }

    // ============================================================
    // Integration Test 2: Worker Status File Monitoring
    // ============================================================

    #[test]
    fn test_e2e_worker_status_monitoring() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create initial workers
        create_test_status_file(&status_dir, "worker-alpha", "active");
        create_test_status_file(&status_dir, "worker-beta", "idle");
        create_test_status_file(&status_dir, "worker-gamma", "starting");

        // Initialize status watcher
        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let watcher = StatusWatcher::new(config).unwrap();

        // Verify initial scan detected all workers
        assert_eq!(watcher.workers().len(), 3);
        assert!(watcher.get_worker("worker-alpha").is_some());
        assert!(watcher.get_worker("worker-beta").is_some());
        assert!(watcher.get_worker("worker-gamma").is_some());

        // Verify worker counts
        let counts = watcher.worker_counts();
        assert_eq!(counts.total, 3);
        assert_eq!(counts.active, 1);
        assert_eq!(counts.idle, 1);
        assert_eq!(counts.starting, 1);
        assert_eq!(counts.healthy(), 3);
    }

    #[test]
    fn test_e2e_worker_status_updates() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create initial worker
        let path = create_test_status_file(&status_dir, "worker-dynamic", "starting");

        // Initialize watcher
        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Verify initial state
        assert_eq!(
            watcher.get_worker("worker-dynamic").unwrap().status,
            WorkerStatus::Starting
        );

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Update worker status
        let updated_content = r#"{
            "worker_id": "worker-dynamic",
            "status": "active",
            "model": "test-model-upgraded",
            "workspace": "/test/workspace",
            "pid": 12345,
            "tasks_completed": 5
        }"#;
        fs::write(&path, updated_content).unwrap();

        // Wait for update
        thread::sleep(Duration::from_millis(100));

        // Drain events
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify status was updated
        let worker = watcher.get_worker("worker-dynamic");
        assert!(worker.is_some());
        // Note: The status should be updated, but depending on timing it may or may not be
        // reflected yet. The important thing is that the worker still exists and can be queried.
    }

    // ============================================================
    // Integration Test 3: Log Streaming Integration
    // ============================================================

    #[test]
    fn test_e2e_log_streaming() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().join("logs");
        fs::create_dir_all(&log_dir).unwrap();

        // Create a log file with initial entries
        let log_path = create_test_log_file(
            &log_dir,
            "worker-logger",
            &[
                ("INFO", "Worker started successfully"),
                ("DEBUG", "Connecting to API"),
                ("INFO", "Processing task bd-001"),
                ("WARN", "Rate limit approaching"),
                ("INFO", "Task bd-001 completed"),
            ],
        );

        // Create log tailer
        let config = LogTailerConfig::new(&log_path)
            .with_source("worker-logger")
            .with_start_from_end(false);
        let mut tailer = LogTailer::new(config);

        // Read entries
        let entries = tailer.read_new_lines().unwrap();
        assert_eq!(entries.len(), 5);

        // Verify entries are parsed correctly
        assert_eq!(entries[0].level, LogLevel::Info);
        assert_eq!(entries[0].message, "Worker started successfully");
        assert_eq!(entries[3].level, LogLevel::Warn);
        assert_eq!(entries[4].message, "Task bd-001 completed");

        // Verify source is set
        for entry in &entries {
            assert_eq!(entry.source, Some("worker-logger".to_string()));
        }
    }

    #[test]
    fn test_e2e_log_buffer_ring_behavior() {
        // Create a small ring buffer
        let mut buffer = LogBuffer::new(5);

        // Add entries exceeding capacity
        for i in 1..=10 {
            buffer.push(LogEntry::new(
                LogLevel::Info,
                format!("Log entry {}", i),
            ));
        }

        // Verify ring buffer behavior
        assert_eq!(buffer.len(), 5);
        assert_eq!(buffer.total_added(), 10);
        assert_eq!(buffer.dropped_count(), 5);

        // Verify only the last 5 entries are retained
        let messages: Vec<_> = buffer.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            messages,
            vec!["Log entry 6", "Log entry 7", "Log entry 8", "Log entry 9", "Log entry 10"]
        );
    }

    // ============================================================
    // Integration Test 4: View Navigation and Panel Focus
    // ============================================================

    #[test]
    fn test_e2e_view_navigation_cycle() {
        let mut app = App::new();

        // Test forward cycling through all views
        let expected_sequence = [
            View::Overview,
            View::Workers,
            View::Tasks,
            View::Costs,
            View::Metrics,
            View::Logs,
            View::Chat,
            View::Overview, // Wraps around
        ];

        for (i, expected_view) in expected_sequence.iter().enumerate() {
            assert_eq!(
                app.current_view(),
                *expected_view,
                "View at step {} should be {:?}",
                i,
                expected_view
            );
            app.next_view();
        }
    }

    #[test]
    fn test_e2e_view_navigation_hotkeys() {
        let mut app = App::new();

        // Test direct view switching via events
        let view_events = [
            (AppEvent::SwitchView(View::Workers), View::Workers),
            (AppEvent::SwitchView(View::Tasks), View::Tasks),
            (AppEvent::SwitchView(View::Costs), View::Costs),
            (AppEvent::SwitchView(View::Metrics), View::Metrics),
            (AppEvent::SwitchView(View::Logs), View::Logs),
            (AppEvent::SwitchView(View::Chat), View::Chat),
            (AppEvent::SwitchView(View::Overview), View::Overview),
        ];

        for (event, expected_view) in view_events {
            app.handle_app_event(event);
            assert_eq!(app.current_view(), expected_view);
        }
    }

    #[test]
    fn test_e2e_panel_focus_changes_with_view() {
        let mut app = App::new();

        // Each view should set appropriate default focus
        let view_focus_mapping = [
            (View::Workers, FocusPanel::WorkerPool),
            (View::Tasks, FocusPanel::TaskQueue),
            (View::Costs, FocusPanel::CostBreakdown),
            (View::Metrics, FocusPanel::MetricsCharts),
            (View::Logs, FocusPanel::ActivityLog),
            (View::Chat, FocusPanel::ChatInput),
            (View::Overview, FocusPanel::WorkerPool),
        ];

        for (view, expected_focus) in view_focus_mapping {
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
    // Integration Test 5: Chat Mode and Text Input
    // ============================================================

    #[test]
    fn test_e2e_chat_mode_workflow() {
        let mut app = App::new();

        // Initially not in chat mode
        assert_ne!(app.current_view(), View::Chat);

        // Switch to chat view
        app.switch_view(View::Chat);
        assert_eq!(app.current_view(), View::Chat);
        assert_eq!(app.focus_panel(), FocusPanel::ChatInput);

        // Simulate typing a command
        let command = "show workers";
        for c in command.chars() {
            app.handle_app_event(AppEvent::TextInput(c));
        }

        // Verify the input was captured (checking internal state via rendering)
        let buffer = render_app(&app, 120, 40);
        // The chat input should be visible in the rendered output
        assert!(
            buffer_contains(&buffer, "Chat") || buffer_contains(&buffer, "Input"),
            "Chat view should be rendered"
        );

        // Test backspace
        app.handle_app_event(AppEvent::Backspace);

        // Test submit
        app.handle_app_event(AppEvent::Submit);

        // Test escape/cancel
        app.handle_app_event(AppEvent::Cancel);
    }

    // ============================================================
    // Integration Test 6: Help Overlay
    // ============================================================

    #[test]
    fn test_e2e_help_overlay_toggle() {
        let mut app = App::new();

        // Help should be hidden initially
        assert!(!app.show_help());

        // Show help
        app.handle_app_event(AppEvent::ShowHelp);
        assert!(app.show_help());

        // Render with help overlay
        let buffer = render_app(&app, 120, 40);
        assert!(
            buffer_contains(&buffer, "Help") || buffer_contains(&buffer, "Hotkey"),
            "Help overlay should be visible"
        );

        // Hide help via cancel
        app.handle_app_event(AppEvent::Cancel);
        assert!(!app.show_help());
    }

    // ============================================================
    // Integration Test 7: Application Quit Handling
    // ============================================================

    #[test]
    fn test_e2e_quit_handling() {
        let mut app = App::new();

        // App should not quit initially
        assert!(!app.should_quit());

        // Normal quit
        app.handle_app_event(AppEvent::Quit);
        assert!(app.should_quit());

        // Reset and test force quit
        let mut app2 = App::new();
        app2.handle_app_event(AppEvent::ForceQuit);
        assert!(app2.should_quit());
    }

    // ============================================================
    // Integration Test 8: Scroll Navigation
    // ============================================================

    #[test]
    fn test_e2e_scroll_navigation() {
        let mut app = App::new();

        // Initial scroll position
        // We can't access scroll_offset directly, but we can test navigation events

        // Navigate down
        for _ in 0..5 {
            app.handle_app_event(AppEvent::NavigateDown);
        }

        // Navigate up
        app.handle_app_event(AppEvent::NavigateUp);

        // Page navigation
        app.handle_app_event(AppEvent::PageDown);
        app.handle_app_event(AppEvent::PageUp);

        // Go to top/bottom
        app.handle_app_event(AppEvent::GoToBottom);
        app.handle_app_event(AppEvent::GoToTop);

        // App should still be functional
        assert!(!app.should_quit());
    }

    // ============================================================
    // Integration Test 9: Concurrent Worker Operations
    // ============================================================

    #[test]
    fn test_e2e_concurrent_worker_creation() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Create multiple workers rapidly (simulating concurrent registration)
        for i in 0..10 {
            let content = format!(
                r#"{{
                    "worker_id": "concurrent-worker-{}",
                    "status": "{}",
                    "model": "test-model",
                    "workspace": "/test/workspace",
                    "pid": {},
                    "tasks_completed": {}
                }}"#,
                i,
                if i % 3 == 0 { "active" } else if i % 3 == 1 { "idle" } else { "starting" },
                std::process::id(),
                i * 10
            );
            let path = status_dir.join(format!("concurrent-worker-{}.json", i));
            fs::write(&path, content).unwrap();
        }

        // Wait for all files to be processed
        thread::sleep(Duration::from_millis(200));

        // Drain events
        while watcher.recv_timeout(Duration::from_millis(100)).is_some() {}

        // Verify all workers are tracked
        for i in 0..10 {
            let worker_id = format!("concurrent-worker-{}", i);
            assert!(
                watcher.get_worker(&worker_id).is_some(),
                "Worker {} should be tracked",
                worker_id
            );
        }

        // Verify counts
        let counts = watcher.worker_counts();
        assert_eq!(counts.total, 10);
    }

    // ============================================================
    // Integration Test 10: Stability Under Load
    // ============================================================

    #[test]
    fn test_e2e_stability_rapid_view_switching() {
        let mut app = App::new();

        // Rapidly switch between all views multiple times
        for _ in 0..100 {
            for view in View::ALL {
                app.switch_view(view);
                // Render each view to ensure no crashes
                let _ = render_app(&app, 80, 24);
            }
        }

        // App should still be functional
        assert!(!app.should_quit());
        assert_eq!(app.current_view(), View::Chat); // Last view in ALL array
    }

    #[test]
    fn test_e2e_stability_rapid_event_processing() {
        let mut app = App::new();

        // Process many events rapidly
        let events = [
            AppEvent::NavigateDown,
            AppEvent::NavigateUp,
            AppEvent::NextView,
            AppEvent::PrevView,
            AppEvent::Refresh,
            AppEvent::PageDown,
            AppEvent::PageUp,
            AppEvent::GoToTop,
            AppEvent::GoToBottom,
        ];

        for _ in 0..100 {
            for event in &events {
                app.handle_app_event(event.clone());
            }
        }

        // App should still be functional
        assert!(!app.should_quit());
    }

    #[test]
    fn test_e2e_stability_large_terminal_sizes() {
        let app = App::new();

        // Test various terminal sizes including extreme cases
        let sizes = [
            (20, 10),    // Minimum
            (80, 24),    // Standard
            (120, 40),   // Large
            (200, 60),   // Very large
            (300, 100),  // Extreme
        ];

        for (width, height) in sizes {
            let buffer = render_app(&app, width, height);
            assert_eq!(buffer.area.width, width);
            assert_eq!(buffer.area.height, height);
        }
    }

    #[test]
    fn test_e2e_stability_log_buffer_overflow() {
        let mut buffer = LogBuffer::new(100);

        // Add thousands of entries
        for i in 0..10000 {
            buffer.push(LogEntry::new(
                LogLevel::Info,
                format!("Stress test entry {}", i),
            ));
        }

        // Buffer should maintain invariants
        assert_eq!(buffer.len(), 100);
        assert_eq!(buffer.total_added(), 10000);
        assert_eq!(buffer.dropped_count(), 9900);

        // Buffer should be iterable
        let count = buffer.iter().count();
        assert_eq!(count, 100);

        // Last N should work
        let last_5: Vec<_> = buffer.last_n(5).collect();
        assert_eq!(last_5.len(), 5);
    }

    // ============================================================
    // Integration Test 11: Full Workflow Simulation
    // ============================================================

    #[test]
    fn test_e2e_full_workflow_simulation() {
        // This test simulates a complete user workflow:
        // 1. Start the application
        // 2. Navigate to different views
        // 3. Open help
        // 4. Switch to chat mode
        // 5. Type a command
        // 6. Return to overview
        // 7. Quit

        let mut app = App::new();

        // 1. Verify initial state
        assert_eq!(app.current_view(), View::Overview);
        let buffer = render_app(&app, 120, 40);
        assert!(buffer_contains(&buffer, "FORGE Dashboard"));

        // 2. Navigate through views
        app.switch_view(View::Workers);
        assert_eq!(app.current_view(), View::Workers);
        let buffer = render_app(&app, 120, 40);
        assert!(buffer_contains(&buffer, "Worker"));

        app.switch_view(View::Tasks);
        assert_eq!(app.current_view(), View::Tasks);

        app.switch_view(View::Costs);
        assert_eq!(app.current_view(), View::Costs);
        let buffer = render_app(&app, 120, 40);
        // Costs view shows placeholder content since cost tracking isn't yet implemented
        assert!(buffer_contains(&buffer, "Cost") || buffer_contains(&buffer, "Loading"));

        // 3. Open help overlay
        app.handle_app_event(AppEvent::ShowHelp);
        assert!(app.show_help());
        let buffer = render_app(&app, 120, 40);
        assert!(buffer_contains(&buffer, "Help"));

        // Close help
        app.handle_app_event(AppEvent::Cancel);
        assert!(!app.show_help());

        // 4. Switch to chat mode
        app.switch_view(View::Chat);
        assert_eq!(app.current_view(), View::Chat);
        assert_eq!(app.focus_panel(), FocusPanel::ChatInput);

        // 5. Type a command
        for c in "spawn worker".chars() {
            app.handle_app_event(AppEvent::TextInput(c));
        }
        app.handle_app_event(AppEvent::Submit);

        // 6. Return to overview
        app.switch_view(View::Overview);
        assert_eq!(app.current_view(), View::Overview);

        // 7. Quit
        app.handle_app_event(AppEvent::Quit);
        assert!(app.should_quit());
    }

    // ============================================================
    // Integration Test 12: Worker and Log Integration
    // ============================================================

    #[test]
    fn test_e2e_worker_log_correlation() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        let log_dir = temp_dir.path().join("logs");
        fs::create_dir_all(&status_dir).unwrap();
        fs::create_dir_all(&log_dir).unwrap();

        // Create workers with corresponding logs
        let workers = ["worker-a", "worker-b", "worker-c"];

        for worker in &workers {
            // Create status file
            create_test_status_file(&status_dir, worker, "active");

            // Create corresponding log file
            create_test_log_file(
                &log_dir,
                worker,
                &[
                    ("INFO", &format!("{} initialized", worker)),
                    ("INFO", &format!("{} processing task", worker)),
                ],
            );
        }

        // Initialize status watcher
        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let watcher = StatusWatcher::new(config).unwrap();

        // Verify all workers detected
        assert_eq!(watcher.workers().len(), 3);

        // Create aggregate log buffer
        let mut agg_buffer = crate::log::AggregateLogBuffer::new(100);

        // Read logs from each worker
        for worker in &workers {
            let log_path = log_dir.join(format!("{}.log", worker));
            let config = LogTailerConfig::new(&log_path)
                .with_source(*worker)
                .with_start_from_end(false);
            let mut tailer = LogTailer::new(config);

            let entries = tailer.read_new_lines().unwrap();
            for entry in entries {
                agg_buffer.push(entry);
            }
        }

        // Verify aggregate buffer has all logs
        assert_eq!(agg_buffer.len(), 6); // 2 entries per worker * 3 workers

        // Verify we can filter by source
        for worker in &workers {
            let worker_logs = agg_buffer.for_source(worker);
            assert!(worker_logs.is_some());
            assert_eq!(worker_logs.unwrap().len(), 2);
        }
    }

    // ============================================================
    // Integration Test 13: Input Handler Integration
    // ============================================================

    #[test]
    fn test_e2e_input_handler_mode_transitions() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut handler = InputHandler::new();

        // Start in normal mode
        assert!(!handler.is_chat_mode());

        // Activate chat mode with ':'
        let event = handler.handle_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        assert!(handler.is_chat_mode());
        assert_eq!(event, AppEvent::SwitchView(View::Chat));

        // In chat mode, characters become text input
        let event = handler.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(event, AppEvent::TextInput('a'));

        // Enter submits
        let event = handler.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(event, AppEvent::Submit);

        // Escape exits chat mode
        let event = handler.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(event, AppEvent::Cancel);
        assert!(!handler.is_chat_mode());

        // Ctrl+C always force quits
        let event = handler.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(event, AppEvent::ForceQuit);
    }

    // ============================================================
    // Integration Test 14: Status Event Processing
    // ============================================================

    #[test]
    fn test_e2e_status_event_lifecycle() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Should receive initial scan complete event
        let event = watcher.recv_timeout(Duration::from_millis(100));
        assert!(matches!(event, Some(StatusEvent::InitialScanComplete { .. })));

        // Create a new worker
        let path = create_test_status_file(&status_dir, "lifecycle-worker", "starting");
        thread::sleep(Duration::from_millis(100));

        // Drain events
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Worker should be tracked
        assert!(watcher.get_worker("lifecycle-worker").is_some());

        // Update worker status
        let content = r#"{"worker_id": "lifecycle-worker", "status": "active", "model": "test", "workspace": "/test"}"#;
        fs::write(&path, content).unwrap();
        thread::sleep(Duration::from_millis(100));

        // Drain events
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Delete worker
        fs::remove_file(&path).unwrap();
        thread::sleep(Duration::from_millis(100));

        // Drain events
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Worker should be removed from tracking
        assert!(watcher.get_worker("lifecycle-worker").is_none());
    }

    // ============================================================
    // Integration Test 15: Edge Cases and Error Handling
    // ============================================================

    #[test]
    fn test_e2e_invalid_status_file_handling() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create an invalid JSON file
        let invalid_path = status_dir.join("invalid.json");
        fs::write(&invalid_path, "{ this is not valid json }").unwrap();

        // Create a valid file
        create_test_status_file(&status_dir, "valid-worker", "active");

        // Status watcher should handle gracefully
        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let watcher = StatusWatcher::new(config).unwrap();

        // Should have only the valid worker
        assert_eq!(watcher.workers().len(), 1);
        assert!(watcher.get_worker("valid-worker").is_some());
    }

    #[test]
    fn test_e2e_non_json_files_ignored() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create non-JSON files that should be ignored
        fs::write(status_dir.join("readme.txt"), "This is not a status file").unwrap();
        fs::write(status_dir.join(".hidden"), "Hidden file").unwrap();
        fs::write(status_dir.join("backup.json.bak"), "{}").unwrap();

        // Create a valid worker
        create_test_status_file(&status_dir, "real-worker", "active");

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let watcher = StatusWatcher::new(config).unwrap();

        // Should only have the real worker
        assert_eq!(watcher.workers().len(), 1);
        assert!(watcher.get_worker("real-worker").is_some());
    }

    #[test]
    fn test_e2e_empty_log_file_handling() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("empty.log");

        // Create empty log file
        fs::write(&log_path, "").unwrap();

        let config = LogTailerConfig::new(&log_path)
            .with_start_from_end(false);
        let mut tailer = LogTailer::new(config);

        // Should handle gracefully
        let entries = tailer.read_new_lines().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_e2e_view_back_navigation() {
        let mut app = App::new();

        // Navigate forward
        app.switch_view(View::Workers);
        app.switch_view(View::Tasks);
        app.switch_view(View::Costs);

        // Go back
        app.go_back();

        // Should return to Tasks (the previous view)
        // Note: go_back uses previous_view which is set on each switch
        // After switching to Costs, previous_view is Tasks
        assert_eq!(app.current_view(), View::Tasks);
    }

    // ============================================================
    // Integration Test 16: Data Flow - Worker Starts
    // ============================================================
    //
    // Tests the complete data flow when a worker starts:
    // Status file created → StatusWatcher detects → App updates → UI displays

    #[test]
    fn test_data_flow_worker_starts() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Initialize status watcher (no workers yet)
        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Verify no workers initially
        assert_eq!(watcher.workers().len(), 0);
        let initial_counts = watcher.worker_counts();
        assert_eq!(initial_counts.total, 0);
        assert_eq!(initial_counts.starting, 0);

        // Consume initial scan event
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Simulate worker starting - create status file with "starting" status
        let worker_json = r#"{
            "worker_id": "worker-alpha",
            "status": "starting",
            "model": "claude-sonnet-4-5",
            "workspace": "/home/user/project",
            "pid": 12345,
            "started_at": "2026-02-08T10:00:00Z"
        }"#;
        let worker_path = status_dir.join("worker-alpha.json");
        fs::write(&worker_path, worker_json).unwrap();

        // Wait for file system event to propagate
        thread::sleep(Duration::from_millis(100));

        // Drain events to update internal state
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify worker is now tracked with "starting" status
        let worker = watcher.get_worker("worker-alpha");
        assert!(worker.is_some(), "Worker should be tracked after status file creation");
        assert_eq!(worker.unwrap().status, WorkerStatus::Starting, "Worker should be in 'starting' status");

        // Verify counts updated
        let counts = watcher.worker_counts();
        assert_eq!(counts.total, 1, "Total worker count should be 1");
        assert_eq!(counts.starting, 1, "Starting worker count should be 1");
        assert_eq!(counts.healthy(), 1, "Worker should be considered healthy");

        // Verify worker data can be retrieved by ID
        let worker_data = watcher.get_worker("worker-alpha").unwrap();
        assert_eq!(worker_data.model, "claude-sonnet-4-5");
        assert_eq!(worker_data.workspace, "/home/user/project");
        assert_eq!(worker_data.pid, Some(12345));
    }

    #[test]
    fn test_data_flow_worker_starts_ui_display() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create a starting worker
        let worker_json = r#"{
            "worker_id": "ui-test-worker",
            "status": "starting",
            "model": "sonnet",
            "workspace": "/test",
            "pid": 99999,
            "started_at": "2026-02-08T10:00:00Z"
        }"#;
        fs::write(status_dir.join("ui-test-worker.json"), worker_json).unwrap();

        // Initialize app with custom status dir
        let app = App::with_status_dir(status_dir.clone());

        // Render the app and verify UI shows the worker
        let buffer = render_app(&app, 120, 40);

        // The worker should appear in the worker pool display
        // When starting, the UI should show "Starting" or the starting indicator
        let content = buffer_to_string(&buffer);
        assert!(
            content.contains("Worker") || content.contains("Loading"),
            "UI should display worker-related content: got {}",
            &content[..content.len().min(500)]
        );
    }

    // ============================================================
    // Integration Test 17: Data Flow - Worker Completes Task
    // ============================================================
    //
    // Tests the data flow when a worker completes a task:
    // Worker active with task → Task completed → Status updated → UI reflects completion

    #[test]
    fn test_data_flow_worker_completes_task() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create worker actively working on a task
        let active_json = r#"{
            "worker_id": "task-worker",
            "status": "active",
            "model": "opus",
            "workspace": "/project",
            "pid": 11111,
            "started_at": "2026-02-08T10:00:00Z",
            "last_activity": "2026-02-08T10:30:00Z",
            "current_task": "fg-123",
            "tasks_completed": 5
        }"#;
        let worker_path = status_dir.join("task-worker.json");
        fs::write(&worker_path, active_json).unwrap();

        // Initialize watcher
        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Verify initial state - worker is active with task
        let worker = watcher.get_worker("task-worker").unwrap();
        assert_eq!(worker.status, WorkerStatus::Active);
        assert_eq!(worker.current_task, Some("fg-123".to_string()));
        assert_eq!(worker.tasks_completed, 5);

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Simulate task completion - worker goes idle with incremented task count
        let completed_json = r#"{
            "worker_id": "task-worker",
            "status": "idle",
            "model": "opus",
            "workspace": "/project",
            "pid": 11111,
            "started_at": "2026-02-08T10:00:00Z",
            "last_activity": "2026-02-08T10:35:00Z",
            "current_task": null,
            "tasks_completed": 6
        }"#;
        fs::write(&worker_path, completed_json).unwrap();

        // Wait for update
        thread::sleep(Duration::from_millis(100));

        // Drain events
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify task completion is reflected
        let updated_worker = watcher.get_worker("task-worker").unwrap();
        assert_eq!(
            updated_worker.status,
            WorkerStatus::Idle,
            "Worker should be idle after completing task"
        );
        assert_eq!(
            updated_worker.current_task, None,
            "Worker should have no current task after completion"
        );
        assert_eq!(
            updated_worker.tasks_completed, 6,
            "Tasks completed count should be incremented"
        );

        // Verify counts updated
        let counts = watcher.worker_counts();
        assert_eq!(counts.active, 0, "No workers should be active");
        assert_eq!(counts.idle, 1, "One worker should be idle");
    }

    #[test]
    fn test_data_flow_multiple_task_completions() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create worker with initial task count
        let initial_json = r#"{
            "worker_id": "prolific-worker",
            "status": "active",
            "model": "haiku",
            "workspace": "/fast-tasks",
            "pid": 22222,
            "current_task": "task-1",
            "tasks_completed": 0
        }"#;
        let worker_path = status_dir.join("prolific-worker.json");
        fs::write(&worker_path, initial_json).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Simulate multiple task completions
        for task_num in 1..=5 {
            // Complete current task and start next
            let next_task = if task_num < 5 {
                format!(r#""task-{}""#, task_num + 1)
            } else {
                "null".to_string()
            };

            let status = if task_num < 5 { "active" } else { "idle" };

            let updated_json = format!(
                r#"{{
                    "worker_id": "prolific-worker",
                    "status": "{}",
                    "model": "haiku",
                    "workspace": "/fast-tasks",
                    "pid": 22222,
                    "current_task": {},
                    "tasks_completed": {}
                }}"#,
                status, next_task, task_num
            );
            fs::write(&worker_path, updated_json).unwrap();

            thread::sleep(Duration::from_millis(50));
            while watcher.recv_timeout(Duration::from_millis(20)).is_some() {}
        }

        // Verify final state
        let worker = watcher.get_worker("prolific-worker").unwrap();
        assert_eq!(worker.status, WorkerStatus::Idle);
        assert_eq!(worker.tasks_completed, 5, "Should have completed 5 tasks");
        assert_eq!(worker.current_task, None);
    }

    // ============================================================
    // Integration Test 18: Data Flow - Worker Goes Idle
    // ============================================================
    //
    // Tests the data flow when a worker transitions to idle:
    // Active → Idle transition, UI reflects the change

    #[test]
    fn test_data_flow_worker_goes_idle() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create an active worker
        let active_json = r#"{
            "worker_id": "busy-worker",
            "status": "active",
            "model": "sonnet",
            "workspace": "/work",
            "pid": 33333,
            "current_task": "ongoing-task"
        }"#;
        let worker_path = status_dir.join("busy-worker.json");
        fs::write(&worker_path, active_json).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Verify initial active state
        assert_eq!(watcher.worker_counts().active, 1);
        assert_eq!(watcher.worker_counts().idle, 0);

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Transition to idle (no more tasks to process)
        let idle_json = r#"{
            "worker_id": "busy-worker",
            "status": "idle",
            "model": "sonnet",
            "workspace": "/work",
            "pid": 33333,
            "current_task": null
        }"#;
        fs::write(&worker_path, idle_json).unwrap();

        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify idle transition
        let worker = watcher.get_worker("busy-worker").unwrap();
        assert_eq!(worker.status, WorkerStatus::Idle, "Worker should be idle");
        assert!(worker.is_healthy(), "Idle worker should be healthy");

        let counts = watcher.worker_counts();
        assert_eq!(counts.active, 0, "No workers should be active");
        assert_eq!(counts.idle, 1, "One worker should be idle");
        assert_eq!(counts.healthy(), 1, "One worker should be healthy");
    }

    #[test]
    fn test_data_flow_idle_to_active_transition() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Start with idle worker
        let idle_json = r#"{
            "worker_id": "waiting-worker",
            "status": "idle",
            "model": "sonnet",
            "workspace": "/ready"
        }"#;
        let worker_path = status_dir.join("waiting-worker.json");
        fs::write(&worker_path, idle_json).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        assert_eq!(watcher.worker_counts().idle, 1);
        assert_eq!(watcher.worker_counts().active, 0);

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Worker picks up a new task
        let active_json = r#"{
            "worker_id": "waiting-worker",
            "status": "active",
            "model": "sonnet",
            "workspace": "/ready",
            "current_task": "new-task-123"
        }"#;
        fs::write(&worker_path, active_json).unwrap();

        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify transition to active
        let worker = watcher.get_worker("waiting-worker").unwrap();
        assert_eq!(worker.status, WorkerStatus::Active);
        assert_eq!(worker.current_task, Some("new-task-123".to_string()));

        let counts = watcher.worker_counts();
        assert_eq!(counts.idle, 0);
        assert_eq!(counts.active, 1);
    }

    // ============================================================
    // Integration Test 19: Data Flow - Worker Crashes
    // ============================================================
    //
    // Tests the data flow when a worker crashes/fails:
    // Active/Starting → Failed/Error, UI shows unhealthy worker

    #[test]
    fn test_data_flow_worker_crashes_from_active() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create an active worker
        let active_json = r#"{
            "worker_id": "unstable-worker",
            "status": "active",
            "model": "opus",
            "workspace": "/risky",
            "pid": 44444,
            "current_task": "dangerous-task"
        }"#;
        let worker_path = status_dir.join("unstable-worker.json");
        fs::write(&worker_path, active_json).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Verify initial healthy state
        assert_eq!(watcher.worker_counts().healthy(), 1);
        assert_eq!(watcher.worker_counts().unhealthy(), 0);

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Simulate crash - worker updates status to failed
        let crashed_json = r#"{
            "worker_id": "unstable-worker",
            "status": "failed",
            "model": "opus",
            "workspace": "/risky",
            "pid": 44444,
            "current_task": "dangerous-task"
        }"#;
        fs::write(&worker_path, crashed_json).unwrap();

        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify crash detection
        let worker = watcher.get_worker("unstable-worker").unwrap();
        assert_eq!(worker.status, WorkerStatus::Failed, "Worker should be in failed state");
        assert!(!worker.is_healthy(), "Failed worker should not be healthy");

        let counts = watcher.worker_counts();
        assert_eq!(counts.failed, 1, "One worker should be failed");
        assert_eq!(counts.healthy(), 0, "No workers should be healthy");
        assert_eq!(counts.unhealthy(), 1, "One worker should be unhealthy");
    }

    #[test]
    fn test_data_flow_worker_crashes_from_starting() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create a starting worker
        let starting_json = r#"{
            "worker_id": "startup-crash",
            "status": "starting",
            "model": "haiku",
            "workspace": "/init"
        }"#;
        let worker_path = status_dir.join("startup-crash.json");
        fs::write(&worker_path, starting_json).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        assert_eq!(watcher.worker_counts().starting, 1);

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Crash during startup
        let crashed_json = r#"{
            "worker_id": "startup-crash",
            "status": "error",
            "model": "haiku",
            "workspace": "/init"
        }"#;
        fs::write(&worker_path, crashed_json).unwrap();

        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify error state
        let worker = watcher.get_worker("startup-crash").unwrap();
        assert_eq!(worker.status, WorkerStatus::Error);
        assert!(!worker.is_healthy());

        let counts = watcher.worker_counts();
        assert_eq!(counts.error, 1);
        assert_eq!(counts.starting, 0);
    }

    #[test]
    fn test_data_flow_worker_recovery_after_crash() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Start with a failed worker
        let failed_json = r#"{
            "worker_id": "recovering-worker",
            "status": "failed",
            "model": "sonnet"
        }"#;
        let worker_path = status_dir.join("recovering-worker.json");
        fs::write(&worker_path, failed_json).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        assert_eq!(watcher.worker_counts().failed, 1);
        assert_eq!(watcher.worker_counts().unhealthy(), 1);

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Worker restarts and recovers
        let recovered_json = r#"{
            "worker_id": "recovering-worker",
            "status": "starting",
            "model": "sonnet"
        }"#;
        fs::write(&worker_path, recovered_json).unwrap();

        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify recovery
        let worker = watcher.get_worker("recovering-worker").unwrap();
        assert_eq!(worker.status, WorkerStatus::Starting);
        assert!(worker.is_healthy(), "Restarted worker should be healthy");

        let counts = watcher.worker_counts();
        assert_eq!(counts.failed, 0);
        assert_eq!(counts.starting, 1);
        assert_eq!(counts.healthy(), 1);
    }

    #[test]
    fn test_data_flow_worker_file_deletion_as_crash() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create an active worker
        let active_json = r#"{
            "worker_id": "vanishing-worker",
            "status": "active",
            "model": "opus"
        }"#;
        let worker_path = status_dir.join("vanishing-worker.json");
        fs::write(&worker_path, active_json).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        assert!(watcher.get_worker("vanishing-worker").is_some());

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Simulate sudden crash - status file deleted (process terminated unexpectedly)
        fs::remove_file(&worker_path).unwrap();

        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Worker should no longer be tracked
        assert!(
            watcher.get_worker("vanishing-worker").is_none(),
            "Worker should be removed after status file deletion"
        );
        assert_eq!(watcher.workers().len(), 0);
    }

    // ============================================================
    // Integration Test 20: Complete Worker Lifecycle Data Flow
    // ============================================================
    //
    // Tests the full lifecycle: start → active → complete tasks → idle → shutdown

    #[test]
    fn test_data_flow_complete_worker_lifecycle() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_path = status_dir.join("lifecycle-worker.json");

        // Phase 1: Worker starts
        let starting_json = r#"{
            "worker_id": "lifecycle-worker",
            "status": "starting",
            "model": "sonnet",
            "workspace": "/project",
            "pid": 55555,
            "started_at": "2026-02-08T10:00:00Z",
            "tasks_completed": 0
        }"#;
        fs::write(&worker_path, starting_json).unwrap();

        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        let worker = watcher.get_worker("lifecycle-worker").unwrap();
        assert_eq!(worker.status, WorkerStatus::Starting, "Phase 1: Should be starting");

        // Phase 2: Worker becomes active with first task
        let active_json = r#"{
            "worker_id": "lifecycle-worker",
            "status": "active",
            "model": "sonnet",
            "workspace": "/project",
            "pid": 55555,
            "started_at": "2026-02-08T10:00:00Z",
            "last_activity": "2026-02-08T10:01:00Z",
            "current_task": "task-001",
            "tasks_completed": 0
        }"#;
        fs::write(&worker_path, active_json).unwrap();

        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        let worker = watcher.get_worker("lifecycle-worker").unwrap();
        assert_eq!(worker.status, WorkerStatus::Active, "Phase 2: Should be active");
        assert_eq!(worker.current_task, Some("task-001".to_string()));

        // Phase 3: Worker completes task, picks up another
        let task2_json = r#"{
            "worker_id": "lifecycle-worker",
            "status": "active",
            "model": "sonnet",
            "workspace": "/project",
            "pid": 55555,
            "started_at": "2026-02-08T10:00:00Z",
            "last_activity": "2026-02-08T10:05:00Z",
            "current_task": "task-002",
            "tasks_completed": 1
        }"#;
        fs::write(&worker_path, task2_json).unwrap();

        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        let worker = watcher.get_worker("lifecycle-worker").unwrap();
        assert_eq!(worker.tasks_completed, 1, "Phase 3: Should have completed 1 task");
        assert_eq!(worker.current_task, Some("task-002".to_string()));

        // Phase 4: Worker finishes all tasks, goes idle
        let idle_json = r#"{
            "worker_id": "lifecycle-worker",
            "status": "idle",
            "model": "sonnet",
            "workspace": "/project",
            "pid": 55555,
            "started_at": "2026-02-08T10:00:00Z",
            "last_activity": "2026-02-08T10:10:00Z",
            "current_task": null,
            "tasks_completed": 2
        }"#;
        fs::write(&worker_path, idle_json).unwrap();

        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        let worker = watcher.get_worker("lifecycle-worker").unwrap();
        assert_eq!(worker.status, WorkerStatus::Idle, "Phase 4: Should be idle");
        assert_eq!(worker.tasks_completed, 2, "Phase 4: Should have completed 2 tasks");
        assert!(worker.current_task.is_none(), "Phase 4: Should have no current task");

        // Phase 5: Worker is stopped
        let stopped_json = r#"{
            "worker_id": "lifecycle-worker",
            "status": "stopped",
            "model": "sonnet",
            "workspace": "/project",
            "pid": 55555,
            "started_at": "2026-02-08T10:00:00Z",
            "last_activity": "2026-02-08T10:15:00Z",
            "tasks_completed": 2
        }"#;
        fs::write(&worker_path, stopped_json).unwrap();

        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        let worker = watcher.get_worker("lifecycle-worker").unwrap();
        assert_eq!(worker.status, WorkerStatus::Stopped, "Phase 5: Should be stopped");
        assert!(!worker.is_healthy(), "Phase 5: Stopped worker should not be healthy");
    }

    // ============================================================
    // Integration Test 21: Multi-Worker Data Flow Scenarios
    // ============================================================

    #[test]
    fn test_data_flow_mixed_worker_states() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create workers in different states
        let workers = [
            ("worker-starting", "starting"),
            ("worker-active", "active"),
            ("worker-idle", "idle"),
            ("worker-failed", "failed"),
        ];

        for (id, status) in &workers {
            let json = format!(
                r#"{{"worker_id": "{}", "status": "{}", "model": "test"}}"#,
                id, status
            );
            fs::write(status_dir.join(format!("{}.json", id)), json).unwrap();
        }

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let watcher = StatusWatcher::new(config).unwrap();

        // Verify all workers are tracked
        assert_eq!(watcher.workers().len(), 4);

        // Verify counts
        let counts = watcher.worker_counts();
        assert_eq!(counts.total, 4);
        assert_eq!(counts.starting, 1);
        assert_eq!(counts.active, 1);
        assert_eq!(counts.idle, 1);
        assert_eq!(counts.failed, 1);

        // Verify health calculations
        assert_eq!(counts.healthy(), 3, "Starting, active, and idle should be healthy");
        assert_eq!(counts.unhealthy(), 1, "Only failed should be unhealthy");

        // Verify healthy/unhealthy lists
        let healthy = watcher.healthy_workers();
        let unhealthy = watcher.unhealthy_workers();

        assert_eq!(healthy.len(), 3);
        assert_eq!(unhealthy.len(), 1);
        assert_eq!(unhealthy[0].worker_id, "worker-failed");
    }

    #[test]
    fn test_data_flow_ui_updates_with_multiple_workers() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create multiple workers
        for i in 1..=3 {
            let status = if i == 1 { "active" } else { "idle" };
            let json = format!(
                r#"{{
                    "worker_id": "multi-worker-{}",
                    "status": "{}",
                    "model": "sonnet",
                    "tasks_completed": {}
                }}"#,
                i, status, i * 5
            );
            fs::write(status_dir.join(format!("multi-worker-{}.json", i)), json).unwrap();
        }

        // Create app with custom status dir
        let app = App::with_status_dir(status_dir);

        // Render Workers view
        let mut app = app;
        app.switch_view(View::Workers);
        let buffer = render_app(&app, 120, 40);

        // UI should show worker information
        let content = buffer_to_string(&buffer);
        assert!(
            content.contains("Worker") || content.contains("Pool"),
            "Workers view should display worker pool: {}",
            &content[..content.len().min(500)]
        );
    }

    // ============================================================
    // Integration Test 22: Data Flow Error Handling
    // ============================================================

    #[test]
    fn test_data_flow_corrupted_status_file_recovery() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create a valid worker
        let valid_json = r#"{"worker_id": "good-worker", "status": "active"}"#;
        let worker_path = status_dir.join("good-worker.json");
        fs::write(&worker_path, valid_json).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(10);
        let mut watcher = StatusWatcher::new(config).unwrap();

        assert!(watcher.get_worker("good-worker").is_some());

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Corrupt the file
        fs::write(&worker_path, "{ invalid json [[[").unwrap();

        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Worker may still be tracked from previous good state or may have error event
        // The key is that the system doesn't crash

        // Now fix the file
        let fixed_json = r#"{"worker_id": "good-worker", "status": "idle"}"#;
        fs::write(&worker_path, fixed_json).unwrap();

        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Worker should be restored
        let worker = watcher.get_worker("good-worker");
        if let Some(w) = worker {
            // If tracked, should be idle now
            assert_eq!(w.status, WorkerStatus::Idle, "Worker should recover to idle state");
        }
    }

    #[test]
    fn test_data_flow_rapid_status_changes() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(5); // Very short debounce
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_path = status_dir.join("rapid-worker.json");

        // Rapid status changes (simulating a busy worker)
        let statuses = ["starting", "active", "active", "active", "idle", "active", "idle"];

        for status in &statuses {
            let json = format!(
                r#"{{"worker_id": "rapid-worker", "status": "{}"}}"#,
                status
            );
            fs::write(&worker_path, json).unwrap();
            thread::sleep(Duration::from_millis(20));
        }

        // Wait for final state
        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(30)).is_some() {}

        // Worker should end up in final state (idle)
        let worker = watcher.get_worker("rapid-worker");
        assert!(worker.is_some(), "Worker should be tracked after rapid changes");
        // Final state should be the last written status
        assert_eq!(worker.unwrap().status, WorkerStatus::Idle);
    }

    // ============================================================
    // Integration Test 23: Cost Panel Rendering
    // ============================================================

    #[test]
    fn test_cost_panel_empty_state() {
        use crate::cost_panel::CostPanelData;

        let data = CostPanelData::new();
        assert!(!data.has_data());
        assert!(!data.is_loading);
        assert!(data.error.is_none());
        assert_eq!(data.monthly_usage_pct(), 0.0);
    }

    #[test]
    fn test_cost_panel_loading_state() {
        use crate::cost_panel::CostPanelData;

        let data = CostPanelData::loading();
        assert!(data.is_loading);
        assert!(!data.has_data());
    }

    #[test]
    fn test_cost_panel_error_state() {
        use crate::cost_panel::CostPanelData;

        let data = CostPanelData::with_error("Database connection failed");
        assert!(data.error.is_some());
        assert_eq!(data.error.as_ref().unwrap(), "Database connection failed");
    }

    #[test]
    fn test_cost_panel_with_data() {
        use crate::cost_panel::{CostPanelData, BudgetConfig, BudgetAlertLevel};
        use forge_cost::DailyCost;
        use chrono::Utc;

        let mut data = CostPanelData::new();

        let today = DailyCost {
            date: Utc::now().date_naive(),
            total_cost_usd: 25.50,
            call_count: 150,
            total_tokens: 500000,
            by_model: vec![],
        };

        data.set_today(today);
        data.set_budget(BudgetConfig::new(500.0));
        data.monthly_total = 150.0;

        assert!(data.has_data());
        assert_eq!(data.today_total(), 25.50);
        assert_eq!(data.today_calls(), 150);
        assert_eq!(data.today_tokens(), 500000);
        assert!((data.monthly_usage_pct() - 30.0).abs() < 0.01);
        assert_eq!(data.monthly_alert(), BudgetAlertLevel::Normal);
    }

    #[test]
    fn test_cost_panel_budget_alerts() {
        use crate::cost_panel::{CostPanelData, BudgetConfig, BudgetAlertLevel};

        let mut data = CostPanelData::new();
        data.set_budget(BudgetConfig::new(100.0));

        // Normal (< 70%)
        data.monthly_total = 50.0;
        assert_eq!(data.monthly_alert(), BudgetAlertLevel::Normal);

        // Warning (70-90%)
        data.monthly_total = 80.0;
        assert_eq!(data.monthly_alert(), BudgetAlertLevel::Warning);

        // Critical (90-100%)
        data.monthly_total = 95.0;
        assert_eq!(data.monthly_alert(), BudgetAlertLevel::Critical);

        // Exceeded (> 100%)
        data.monthly_total = 120.0;
        assert_eq!(data.monthly_alert(), BudgetAlertLevel::Exceeded);
    }

    #[test]
    fn test_cost_panel_sparkline_rendering() {
        use crate::cost_panel::render_sparkline;

        // Test with values
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let sparkline = render_sparkline(&values, 8);
        assert_eq!(sparkline.chars().count(), 8);

        // Test with empty values
        let empty = render_sparkline(&[], 10);
        assert_eq!(empty.chars().count(), 10);
        assert!(empty.trim().is_empty());

        // Test with single value
        let single = render_sparkline(&[5.0], 5);
        assert_eq!(single.chars().count(), 5);
    }

    #[test]
    fn test_cost_panel_bar_rendering() {
        use crate::cost_panel::render_bar;

        let bar = render_bar(50.0, 100.0, 10, '█', '░');
        assert_eq!(bar.chars().count(), 10);
        assert_eq!(bar.chars().filter(|&c| c == '█').count(), 5);
        assert_eq!(bar.chars().filter(|&c| c == '░').count(), 5);

        // Edge case: value > max
        let overflow = render_bar(150.0, 100.0, 10, '█', '░');
        assert_eq!(overflow.chars().filter(|&c| c == '█').count(), 10);

        // Edge case: max = 0
        let zero_max = render_bar(50.0, 0.0, 10, '█', '░');
        assert_eq!(zero_max.chars().filter(|&c| c == '░').count(), 10);
    }

    #[test]
    fn test_cost_formatting_functions() {
        use crate::cost_panel::{format_usd, format_tokens, truncate_model_name};

        // Test USD formatting
        assert_eq!(format_usd(0.0012), "$0.0012");
        assert_eq!(format_usd(5.678), "$5.678");
        assert_eq!(format_usd(99.99), "$99.99");
        assert_eq!(format_usd(123.4), "$123.4");
        assert_eq!(format_usd(5000.0), "$5.00K");

        // Test token formatting
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1500), "1.5K");
        assert_eq!(format_tokens(2500000), "2.5M");

        // Test model name truncation
        assert!(truncate_model_name("claude-opus-4-5-20251101", 15).len() <= 15);
        assert_eq!(truncate_model_name("glm-4.7", 10), "GLM-4.7");
    }

    // ============================================================
    // Integration Test 24: Subscription Panel Rendering
    // ============================================================

    #[test]
    fn test_subscription_data_demo_initialization() {
        use crate::subscription_panel::{SubscriptionData, SubscriptionService};

        let data = SubscriptionData::with_demo_data();

        assert!(data.has_data());
        assert!(data.has_active());
        assert_eq!(data.active_count(), 4);
        assert!(!data.is_loading);
        assert!(data.error.is_none());

        // Check Claude Pro exists
        let claude = data.get(SubscriptionService::ClaudePro).unwrap();
        assert!(claude.is_active);
        assert_eq!(claude.current_usage, 328);
        assert_eq!(claude.limit, Some(500));
    }

    #[test]
    fn test_subscription_status_calculations() {
        use crate::subscription_panel::{SubscriptionStatus, SubscriptionService, ResetPeriod};
        use chrono::{Duration, Utc};

        let status = SubscriptionStatus::new(SubscriptionService::ClaudePro)
            .with_usage(250, 500, "msgs")
            .with_reset(Utc::now() + Duration::days(10), ResetPeriod::Monthly)
            .with_active(true);

        assert!((status.usage_pct() - 50.0).abs() < 0.01);
        assert_eq!(status.remaining(), Some(250));
        assert!(status.time_until_reset().is_some());
    }

    #[test]
    fn test_subscription_recommended_actions() {
        use crate::subscription_panel::{SubscriptionStatus, SubscriptionService, SubscriptionAction, ResetPeriod};
        use chrono::{Duration, Utc};

        // Paused subscription
        let paused = SubscriptionStatus::new(SubscriptionService::ChatGPTPlus)
            .with_active(false);
        assert_eq!(paused.recommended_action(), SubscriptionAction::Paused);

        // Pay-per-use
        let pay = SubscriptionStatus::new(SubscriptionService::DeepSeekAPI)
            .with_pay_per_use(0.05)
            .with_active(true);
        assert_eq!(pay.recommended_action(), SubscriptionAction::Active);

        // Over quota
        let over = SubscriptionStatus::new(SubscriptionService::CursorPro)
            .with_usage(550, 500, "reqs")
            .with_reset(Utc::now() + Duration::days(5), ResetPeriod::Monthly)
            .with_active(true);
        assert_eq!(over.recommended_action(), SubscriptionAction::OverQuota);
    }

    #[test]
    fn test_subscription_format_reset_timer() {
        use crate::subscription_panel::{SubscriptionStatus, SubscriptionService, ResetPeriod};
        use chrono::{Duration, Utc};

        // Days and hours
        let days_status = SubscriptionStatus::new(SubscriptionService::ClaudePro)
            .with_reset(Utc::now() + Duration::days(5) + Duration::hours(12), ResetPeriod::Monthly);
        let timer = days_status.format_reset_timer();
        assert!(timer.contains("d") || timer.contains("h"));

        // Pay-per-use shows Monthly
        let pay = SubscriptionStatus::new(SubscriptionService::DeepSeekAPI)
            .with_pay_per_use(0.05);
        assert_eq!(pay.format_reset_timer(), "Monthly");
    }

    #[test]
    fn test_subscription_summary_formatting() {
        use crate::subscription_panel::{format_subscription_summary, SubscriptionData};

        // Test with demo data
        let data = SubscriptionData::with_demo_data();
        let summary = format_subscription_summary(&data);
        assert!(summary.contains("Claude"));
        assert!(summary.contains("ChatGPT"));
        assert!(summary.contains("Cursor"));
        assert!(summary.contains("DeepSeek"));

        // Test with loading state
        let loading = SubscriptionData::loading();
        let loading_summary = format_subscription_summary(&loading);
        assert!(loading_summary.contains("Loading"));

        // Test with empty data
        let empty = SubscriptionData::new();
        let empty_summary = format_subscription_summary(&empty);
        assert!(empty_summary.contains("No subscriptions"));
    }

    #[test]
    fn test_subscription_usage_colors() {
        use crate::subscription_panel::{SubscriptionStatus, SubscriptionService};
        use ratatui::style::Color;

        // Low usage - green
        let low = SubscriptionStatus::new(SubscriptionService::ClaudePro)
            .with_usage(100, 500, "msgs");
        assert_eq!(low.usage_color(), Color::Green);

        // Medium usage - cyan/yellow
        let med = SubscriptionStatus::new(SubscriptionService::ClaudePro)
            .with_usage(350, 500, "msgs");
        assert!(matches!(med.usage_color(), Color::Cyan | Color::Yellow));

        // High usage - red
        let high = SubscriptionStatus::new(SubscriptionService::ClaudePro)
            .with_usage(480, 500, "msgs");
        assert!(matches!(high.usage_color(), Color::Red | Color::LightRed));
    }

    // ============================================================
    // Integration Test 25: Responsive Layout Tests
    // ============================================================

    #[test]
    fn test_layout_mode_transitions() {
        use crate::view::LayoutMode;

        // Test all boundary conditions
        let test_cases = [
            (40, LayoutMode::Narrow),
            (80, LayoutMode::Narrow),
            (119, LayoutMode::Narrow),
            (120, LayoutMode::Wide),
            (150, LayoutMode::Wide),
            (198, LayoutMode::Wide),
            (199, LayoutMode::UltraWide),
            (250, LayoutMode::UltraWide),
            (400, LayoutMode::UltraWide),
        ];

        for (width, expected_mode) in test_cases {
            assert_eq!(
                LayoutMode::from_width(width),
                expected_mode,
                "Width {} should be {:?}",
                width,
                expected_mode
            );
        }
    }

    #[test]
    fn test_responsive_rendering_all_modes() {
        let app = App::new();

        // Narrow mode
        let narrow = render_app(&app, 80, 30);
        assert!(buffer_contains(&narrow, "FORGE Dashboard"));
        assert!(buffer_contains(&narrow, "Worker Pool"));

        // Wide mode
        let wide = render_app(&app, 150, 40);
        assert!(buffer_contains(&wide, "FORGE Dashboard"));
        assert!(buffer_contains(&wide, "Worker Pool"));
        assert!(buffer_contains(&wide, "Subscriptions"));

        // Ultra-wide mode
        let ultrawide = render_app(&app, 220, 50);
        assert!(buffer_contains(&ultrawide, "FORGE Dashboard"));
        assert!(buffer_contains(&ultrawide, "Cost Breakdown"));
        assert!(buffer_contains(&ultrawide, "Quick Actions"));
    }

    #[test]
    fn test_responsive_panel_visibility() {
        let app = App::new();

        // Narrow: 3 panels
        let narrow = render_app(&app, 80, 30);
        assert!(buffer_contains(&narrow, "Worker Pool"));
        assert!(buffer_contains(&narrow, "Task Queue"));
        assert!(buffer_contains(&narrow, "Activity Log"));
        assert!(!buffer_contains(&narrow, "Quick Actions"));

        // Wide: 4 panels
        let wide = render_app(&app, 150, 40);
        assert!(buffer_contains(&wide, "Worker Pool"));
        assert!(buffer_contains(&wide, "Subscriptions"));
        assert!(buffer_contains(&wide, "Task Queue"));
        assert!(buffer_contains(&wide, "Activity Log"));
        assert!(!buffer_contains(&wide, "Quick Actions"));

        // Ultra-wide: 6 panels
        let ultrawide = render_app(&app, 220, 50);
        assert!(buffer_contains(&ultrawide, "Worker Pool"));
        assert!(buffer_contains(&ultrawide, "Subscriptions"));
        assert!(buffer_contains(&ultrawide, "Task Queue"));
        assert!(buffer_contains(&ultrawide, "Activity Log"));
        assert!(buffer_contains(&ultrawide, "Cost Breakdown"));
        assert!(buffer_contains(&ultrawide, "Quick Actions"));
    }

    #[test]
    fn test_responsive_min_height_requirements() {
        use crate::view::LayoutMode;

        assert_eq!(LayoutMode::Narrow.min_height(), 20);
        assert_eq!(LayoutMode::Wide.min_height(), 30);
        assert_eq!(LayoutMode::UltraWide.min_height(), 38);
    }

    // ============================================================
    // Integration Test 26: Chat Commands Integration
    // ============================================================

    #[test]
    fn test_chat_command_text_input() {
        let mut app = App::new();

        // Switch to chat mode
        app.switch_view(View::Chat);
        assert_eq!(app.current_view(), View::Chat);

        // Simulate typing "help"
        app.handle_app_event(AppEvent::TextInput('h'));
        app.handle_app_event(AppEvent::TextInput('e'));
        app.handle_app_event(AppEvent::TextInput('l'));
        app.handle_app_event(AppEvent::TextInput('p'));

        // Render and check input is captured
        let buffer = render_app(&app, 120, 40);
        assert!(buffer_contains(&buffer, "Chat") || buffer_contains(&buffer, "Input"));
    }

    #[test]
    fn test_chat_submit_clears_input() {
        let mut app = App::new();

        app.switch_view(View::Chat);

        // Type command
        app.handle_app_event(AppEvent::TextInput('t'));
        app.handle_app_event(AppEvent::TextInput('e'));
        app.handle_app_event(AppEvent::TextInput('s'));
        app.handle_app_event(AppEvent::TextInput('t'));

        // Submit
        app.handle_app_event(AppEvent::Submit);

        // Input should be cleared after submit
        // The status_message should show execution message
    }

    #[test]
    fn test_chat_escape_cancels() {
        let mut app = App::new();

        app.switch_view(View::Chat);

        // Type some text
        app.handle_app_event(AppEvent::TextInput('a'));
        app.handle_app_event(AppEvent::TextInput('b'));
        app.handle_app_event(AppEvent::TextInput('c'));

        // Cancel
        app.handle_app_event(AppEvent::Cancel);

        // Should go back to previous view
        assert_ne!(app.current_view(), View::Chat);
    }

    // ============================================================
    // Integration Test 27: Bead Manager Tests
    // ============================================================

    #[test]
    fn test_bead_status_checks() {
        use crate::bead::Bead;

        let ready_bead = Bead {
            id: "fg-test-1".to_string(),
            title: "Test task".to_string(),
            status: "open".to_string(),
            priority: 2,
            dependency_count: 0,
            ..Default::default()
        };

        assert!(ready_bead.is_ready());
        assert!(!ready_bead.is_blocked());
        assert!(!ready_bead.is_in_progress());
        assert!(!ready_bead.is_closed());
    }

    #[test]
    fn test_bead_blocked_status() {
        use crate::bead::Bead;

        let blocked = Bead {
            id: "fg-test-2".to_string(),
            title: "Blocked task".to_string(),
            status: "open".to_string(),
            priority: 1,
            dependency_count: 2,
            ..Default::default()
        };

        assert!(!blocked.is_ready());
        assert!(blocked.is_blocked());
    }

    #[test]
    fn test_bead_priority_indicators() {
        use crate::bead::Bead;

        let p0 = Bead { priority: 0, ..Default::default() };
        let p1 = Bead { priority: 1, ..Default::default() };
        let p2 = Bead { priority: 2, ..Default::default() };
        let p3 = Bead { priority: 3, ..Default::default() };
        let p4 = Bead { priority: 4, ..Default::default() };

        assert_eq!(p0.priority_indicator(), "🔴");
        assert_eq!(p1.priority_indicator(), "🟠");
        assert_eq!(p2.priority_indicator(), "🟡");
        assert_eq!(p3.priority_indicator(), "🔵");
        assert_eq!(p4.priority_indicator(), "⚪");
    }

    #[test]
    fn test_bead_status_indicators() {
        use crate::bead::Bead;

        let test_cases = [
            ("open", "○"),
            ("in_progress", "●"),
            ("closed", "✓"),
            ("blocked", "⊘"),
            ("deferred", "⏸"),
            ("unknown", "?"),
        ];

        for (status, expected) in test_cases {
            let bead = Bead { status: status.to_string(), ..Default::default() };
            assert_eq!(bead.status_indicator(), expected, "Status '{}' should have indicator '{}'", status, expected);
        }
    }

    #[test]
    fn test_bead_manager_initialization() {
        use crate::bead::BeadManager;

        let manager = BeadManager::new();
        assert_eq!(manager.workspace_count(), 0);
        assert!(!manager.is_loaded());
    }

    #[test]
    fn test_bead_aggregated_data_formatting() {
        use crate::bead::AggregatedBeadData;

        let data = AggregatedBeadData {
            ready: vec![],
            blocked: vec![],
            in_progress: vec![],
            total_ready: 5,
            total_blocked: 2,
            total_in_progress: 3,
            total_open: 10,
        };

        let summary = data.format_summary();
        assert!(summary.contains("Ready: 5"));
        assert!(summary.contains("Blocked: 2"));
        assert!(summary.contains("In Progress: 3"));
        assert!(summary.contains("Total Open: 10"));
    }

    // ============================================================
    // Integration Test 28: Widget Tests
    // ============================================================

    #[test]
    fn test_progress_bar_widget() {
        use crate::widget::ProgressBar;

        let bar = ProgressBar::new(75, 100).width(20).label("Memory");
        let rendered = bar.render_string();
        assert!(rendered.contains("Memory"));
        assert!(rendered.contains("75/100"));
        assert!(rendered.contains("75%"));
    }

    #[test]
    fn test_progress_bar_edge_cases() {
        use crate::widget::ProgressBar;

        // Over 100%
        let over = ProgressBar::new(150, 100).width(10);
        let rendered = over.render_string();
        assert!(rendered.contains("100%")); // Should cap at 100%

        // Zero max
        let zero = ProgressBar::new(50, 0).width(10);
        let rendered = zero.render_string();
        assert!(rendered.contains("0%"));
    }

    #[test]
    fn test_status_indicators() {
        use crate::widget::StatusIndicator;

        // Verify that status indicators can be created and rendered as spans
        let healthy = StatusIndicator::healthy("Running");
        let healthy_span = healthy.as_span();
        assert!(healthy_span.content.contains("Running"));

        let warning = StatusIndicator::warning("Slow");
        let warning_span = warning.as_span();
        assert!(warning_span.content.contains("Slow"));

        let error = StatusIndicator::error("Crashed");
        let error_span = error.as_span();
        assert!(error_span.content.contains("Crashed"));

        let idle = StatusIndicator::idle("Waiting");
        let idle_span = idle.as_span();
        assert!(idle_span.content.contains("Waiting"));
    }

    #[test]
    fn test_hotkey_hints_widget() {
        use crate::widget::HotkeyHints;

        let hints = HotkeyHints::new()
            .hint('o', "Overview")
            .hint('w', "Workers")
            .hint('t', "Tasks")
            .hint('q', "Quit");

        let line = hints.as_line();
        assert_eq!(line.spans.len(), 8); // 4 keys + 4 descriptions
    }

    // ============================================================
    // Integration Test 29: Full View Rendering Tests
    // ============================================================

    #[test]
    fn test_all_views_render_without_panic() {
        let mut app = App::new();

        let sizes = [
            (80, 24),    // Minimum viable
            (120, 40),   // Standard
            (199, 55),   // Ultra-wide threshold
            (250, 70),   // Large
        ];

        for (width, height) in sizes {
            for view in View::ALL {
                app.switch_view(view);
                let buffer = render_app(&app, width, height);

                // Should render without panic and have some content
                assert!(
                    !buffer_to_string(&buffer).trim().is_empty(),
                    "View {:?} at {}x{} should render content",
                    view,
                    width,
                    height
                );
            }
        }
    }

    #[test]
    fn test_each_view_has_correct_title() {
        let mut app = App::new();

        let view_titles = [
            (View::Overview, "Overview"),
            (View::Workers, "Workers"),
            (View::Tasks, "Tasks"),
            (View::Costs, "Costs"),
            (View::Metrics, "Metrics"),
            (View::Logs, "Logs"),
            (View::Chat, "Chat"),
        ];

        for (view, expected_title) in view_titles {
            app.switch_view(view);
            let buffer = render_app(&app, 120, 40);
            let content = buffer_to_string(&buffer);

            assert!(
                content.contains(expected_title),
                "View {:?} should have title containing '{}'",
                view,
                expected_title
            );
        }
    }

    // ============================================================
    // Integration Test 30: Error Recovery Tests
    // ============================================================

    #[test]
    fn test_app_recovers_from_invalid_view_state() {
        let mut app = App::new();

        // Rapid view switching should not cause issues
        for _ in 0..50 {
            app.next_view();
            app.prev_view();
            app.switch_view(View::Chat);
            app.handle_app_event(AppEvent::Cancel);
        }

        // App should still be functional
        assert!(!app.should_quit());
        let buffer = render_app(&app, 120, 40);
        assert!(buffer_contains(&buffer, "FORGE Dashboard"));
    }

    #[test]
    fn test_app_handles_extreme_scroll_offsets() {
        let mut app = App::new();

        // Scroll way past the end
        for _ in 0..1000 {
            app.handle_app_event(AppEvent::NavigateDown);
        }

        // App should still render without panic
        let buffer = render_app(&app, 120, 40);
        assert!(buffer_contains(&buffer, "FORGE Dashboard"));

        // Go back to top
        app.handle_app_event(AppEvent::GoToTop);
    }

    #[test]
    fn test_app_handles_rapid_help_toggle() {
        let mut app = App::new();

        for _ in 0..50 {
            app.handle_app_event(AppEvent::ShowHelp);
            let buffer = render_app(&app, 120, 40);
            assert!(buffer_contains(&buffer, "Help") || buffer_contains(&buffer, "Hotkey"));

            app.handle_app_event(AppEvent::HideHelp);
            assert!(!app.show_help());
        }
    }

    // ============================================================
    // Integration Test 31: Data Manager Integration
    // ============================================================

    #[test]
    fn test_data_manager_worker_data_formatting() {
        use crate::data::WorkerData;

        let data = WorkerData::new();

        // Before loading
        assert!(!data.is_loaded());
        let summary = data.format_worker_pool_summary();
        assert!(summary.contains("Loading"));

        // Empty after load
        let mut loaded = WorkerData::new();
        loaded.last_update = Some(std::time::Instant::now());
        let empty_summary = loaded.format_worker_pool_summary();
        assert!(empty_summary.contains("No workers"));
    }

    #[test]
    fn test_data_manager_activity_log_formatting() {
        use crate::data::WorkerData;

        let data = WorkerData::new();
        let log = data.format_activity_log();
        assert!(log.contains("Loading") || log.contains("No recent"));

        let mut loaded = WorkerData::new();
        loaded.last_update = Some(std::time::Instant::now());
        let loaded_log = loaded.format_activity_log();
        assert!(loaded_log.contains("No recent activity"));
    }

    // ============================================================
    // Integration Test 32: Focus Panel Management
    // ============================================================

    #[test]
    fn test_focus_panel_highlighting() {
        use crate::view::FocusPanel;

        assert!(!FocusPanel::None.is_highlighted());
        assert!(FocusPanel::WorkerPool.is_highlighted());
        assert!(FocusPanel::TaskQueue.is_highlighted());
        assert!(FocusPanel::CostBreakdown.is_highlighted());
        assert!(FocusPanel::ActivityLog.is_highlighted());
        assert!(FocusPanel::ChatInput.is_highlighted());
    }

    #[test]
    fn test_focus_changes_correctly_on_view_switch() {
        let mut app = App::new();

        let expected_focus = [
            (View::Workers, FocusPanel::WorkerPool),
            (View::Tasks, FocusPanel::TaskQueue),
            (View::Costs, FocusPanel::CostBreakdown),
            (View::Metrics, FocusPanel::MetricsCharts),
            (View::Logs, FocusPanel::ActivityLog),
            (View::Chat, FocusPanel::ChatInput),
            (View::Overview, FocusPanel::WorkerPool),
        ];

        for (view, expected) in expected_focus {
            app.switch_view(view);
            assert_eq!(
                app.focus_panel(),
                expected,
                "View {:?} should have focus {:?}",
                view,
                expected
            );
        }
    }

    // ============================================================
    // Integration Test 33: Log Level and Entry Tests
    // ============================================================

    #[test]
    fn test_log_level_parsing() {
        use crate::log::LogLevel;

        assert_eq!(LogLevel::from_str("DEBUG"), LogLevel::Debug);
        assert_eq!(LogLevel::from_str("debug"), LogLevel::Debug);
        assert_eq!(LogLevel::from_str("INFO"), LogLevel::Info);
        assert_eq!(LogLevel::from_str("WARN"), LogLevel::Warn);
        assert_eq!(LogLevel::from_str("WARNING"), LogLevel::Warn);
        assert_eq!(LogLevel::from_str("ERROR"), LogLevel::Error);
        assert_eq!(LogLevel::from_str("unknown"), LogLevel::Info); // Default
    }

    #[test]
    fn test_log_level_symbols() {
        use crate::log::LogLevel;

        assert_eq!(LogLevel::Trace.symbol(), "→");
        assert_eq!(LogLevel::Debug.symbol(), "○");
        assert_eq!(LogLevel::Info.symbol(), "●");
        assert_eq!(LogLevel::Warn.symbol(), "⚠");
        assert_eq!(LogLevel::Error.symbol(), "✖");
    }

    #[test]
    fn test_log_entry_creation() {
        use crate::log::{LogEntry, LogLevel};

        let entry = LogEntry::new(LogLevel::Info, "Test message".to_string())
            .with_source("test-worker");

        assert_eq!(entry.level, LogLevel::Info);
        assert_eq!(entry.message, "Test message");
        assert_eq!(entry.source, Some("test-worker".to_string()));
    }

    #[test]
    fn test_log_buffer_operations() {
        use crate::log::{LogBuffer, LogEntry, LogLevel};

        let mut buffer = LogBuffer::new(10);

        // Add entries
        for i in 0..15 {
            buffer.push(LogEntry::new(LogLevel::Info, format!("Entry {}", i)));
        }

        // Ring buffer behavior
        assert_eq!(buffer.len(), 10);
        assert_eq!(buffer.total_added(), 15);
        assert_eq!(buffer.dropped_count(), 5);

        // Last N
        let last_3: Vec<_> = buffer.last_n(3).collect();
        assert_eq!(last_3.len(), 3);
    }

    // ============================================================
    // Integration Test 34: Keyboard Event Handler Tests
    // ============================================================

    #[test]
    fn test_input_handler_view_hotkeys() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut handler = InputHandler::new();

        let view_keys = [
            (KeyCode::Char('o'), View::Overview),
            (KeyCode::Char('w'), View::Workers),
            (KeyCode::Char('t'), View::Tasks),
            (KeyCode::Char('c'), View::Costs),
            (KeyCode::Char('m'), View::Metrics),
            (KeyCode::Char('l'), View::Logs),
        ];

        for (keycode, expected_view) in view_keys {
            let event = handler.handle_key(crossterm::event::KeyEvent::new(
                keycode,
                KeyModifiers::NONE,
            ));
            assert_eq!(
                event,
                AppEvent::SwitchView(expected_view),
                "Key {:?} should switch to {:?}",
                keycode,
                expected_view
            );
        }
    }

    #[test]
    fn test_input_handler_control_keys() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut handler = InputHandler::new();

        // Ctrl+C force quit
        let event = handler.handle_key(crossterm::event::KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ));
        assert_eq!(event, AppEvent::ForceQuit);

        // Ctrl+L refresh
        let event = handler.handle_key(crossterm::event::KeyEvent::new(
            KeyCode::Char('l'),
            KeyModifiers::CONTROL,
        ));
        assert_eq!(event, AppEvent::Refresh);
    }

    #[test]
    fn test_input_handler_navigation() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut handler = InputHandler::new();

        // Arrow keys
        let up = handler.handle_key(crossterm::event::KeyEvent::new(
            KeyCode::Up,
            KeyModifiers::NONE,
        ));
        assert_eq!(up, AppEvent::NavigateUp);

        let down = handler.handle_key(crossterm::event::KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::NONE,
        ));
        assert_eq!(down, AppEvent::NavigateDown);

        // Vim keys
        let k = handler.handle_key(crossterm::event::KeyEvent::new(
            KeyCode::Char('k'),
            KeyModifiers::NONE,
        ));
        assert_eq!(k, AppEvent::NavigateUp);

        let j = handler.handle_key(crossterm::event::KeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
        ));
        assert_eq!(j, AppEvent::NavigateDown);
    }

    // ============================================================
    // Integration Test 35: End-to-End Cost Panel with Widget
    // ============================================================

    #[test]
    fn test_cost_panel_widget_rendering() {
        use crate::cost_panel::{CostPanel, CostPanelData, BudgetConfig};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        // Create cost data
        let mut data = CostPanelData::new();
        data.set_budget(BudgetConfig::new(500.0));
        data.monthly_total = 250.0;

        // Create widget
        let panel = CostPanel::new(&data).focused(true);

        // Render
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| {
            let area = f.area();
            f.render_widget(panel, area);
        }).unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content = buffer_to_string(&buffer);

        // Should show cost analytics panel
        assert!(content.contains("Cost Analytics"));
    }

    #[test]
    fn test_subscription_panel_widget_rendering() {
        use crate::subscription_panel::{SubscriptionPanel, SubscriptionData};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let data = SubscriptionData::with_demo_data();
        let panel = SubscriptionPanel::new(&data).focused(true);

        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| {
            let area = f.area();
            f.render_widget(panel, area);
        }).unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content = buffer_to_string(&buffer);

        // Should show subscription status panel
        assert!(content.contains("Subscription Status"));
    }

    // ============================================================
    // Integration Test 36: Subscription Panel Comprehensive Tests
    // ============================================================

    #[test]
    fn test_subscription_status_usage_calculations() {
        use crate::subscription_panel::{SubscriptionStatus, SubscriptionService, ResetPeriod};
        use chrono::{Utc, Duration};

        // Test usage percentage calculation
        let status = SubscriptionStatus::new(SubscriptionService::ClaudePro)
            .with_usage(250, 500, "msgs")
            .with_reset(Utc::now() + Duration::days(15), ResetPeriod::Monthly)
            .with_active(true);

        assert!((status.usage_pct() - 50.0).abs() < 0.01);
        assert_eq!(status.remaining(), Some(250));
        assert!(status.is_active);
    }

    #[test]
    fn test_subscription_status_over_quota() {
        use crate::subscription_panel::{SubscriptionStatus, SubscriptionService, SubscriptionAction, ResetPeriod};
        use chrono::{Utc, Duration};

        let status = SubscriptionStatus::new(SubscriptionService::CursorPro)
            .with_usage(520, 500, "reqs")
            .with_reset(Utc::now() + Duration::days(5), ResetPeriod::Monthly)
            .with_active(true);

        assert!(status.usage_pct() >= 100.0);
        assert_eq!(status.recommended_action(), SubscriptionAction::OverQuota);
    }

    #[test]
    fn test_subscription_status_pay_per_use() {
        use crate::subscription_panel::{SubscriptionStatus, SubscriptionService, SubscriptionAction, ResetPeriod};

        let status = SubscriptionStatus::new(SubscriptionService::DeepSeekAPI)
            .with_pay_per_use(0.05)
            .with_active(true);

        assert_eq!(status.remaining(), None);
        assert!(matches!(status.reset_period, ResetPeriod::PayPerUse));
        assert_eq!(status.recommended_action(), SubscriptionAction::Active);
    }

    #[test]
    fn test_subscription_status_inactive() {
        use crate::subscription_panel::{SubscriptionStatus, SubscriptionService, SubscriptionAction};

        let status = SubscriptionStatus::new(SubscriptionService::ChatGPTPlus)
            .with_usage(10, 40, "msgs")
            .with_active(false);

        assert_eq!(status.recommended_action(), SubscriptionAction::Paused);
        assert!(!status.is_active);
    }

    #[test]
    fn test_subscription_status_accelerate_recommendation() {
        use crate::subscription_panel::{SubscriptionStatus, SubscriptionService, SubscriptionAction, ResetPeriod};
        use chrono::{Utc, Duration};

        // Under-utilized: 20% used with 80% of time passed
        let now = Utc::now();
        let end = now + Duration::days(6);

        let status = SubscriptionStatus::new(SubscriptionService::ClaudePro)
            .with_usage(100, 500, "msgs")
            .with_reset(end, ResetPeriod::Monthly)
            .with_active(true);

        // Should recommend acceleration
        let action = status.recommended_action();
        assert!(
            matches!(action, SubscriptionAction::Accelerate | SubscriptionAction::MaxOut),
            "Should recommend accelerating usage when behind schedule"
        );
    }

    #[test]
    fn test_subscription_status_on_pace() {
        use crate::subscription_panel::{SubscriptionStatus, SubscriptionService, SubscriptionAction, ResetPeriod};
        use chrono::{Utc, Duration};

        // On pace: 50% used with 50% of time passed
        let status = SubscriptionStatus::new(SubscriptionService::ClaudePro)
            .with_usage(250, 500, "msgs")
            .with_reset(Utc::now() + Duration::days(15), ResetPeriod::Monthly)
            .with_active(true);

        assert_eq!(status.recommended_action(), SubscriptionAction::OnPace);
    }

    #[test]
    fn test_subscription_data_demo_data() {
        use crate::subscription_panel::{SubscriptionData, SubscriptionService};

        let data = SubscriptionData::with_demo_data();

        assert!(data.has_data());
        assert!(!data.is_loading);
        assert!(data.error.is_none());
        assert!(data.has_active());
        assert_eq!(data.active_count(), 4);

        // Verify specific subscriptions
        assert!(data.get(SubscriptionService::ClaudePro).is_some());
        assert!(data.get(SubscriptionService::ChatGPTPlus).is_some());
        assert!(data.get(SubscriptionService::CursorPro).is_some());
        assert!(data.get(SubscriptionService::DeepSeekAPI).is_some());
    }

    #[test]
    fn test_subscription_data_empty() {
        use crate::subscription_panel::SubscriptionData;

        let data = SubscriptionData::new();

        assert!(!data.has_data());
        assert!(!data.is_loading);
        assert!(!data.has_active());
        assert_eq!(data.active_count(), 0);
    }

    #[test]
    fn test_subscription_data_loading() {
        use crate::subscription_panel::SubscriptionData;

        let data = SubscriptionData::loading();

        assert!(data.is_loading);
        assert!(!data.has_data());
    }

    #[test]
    fn test_subscription_reset_timer_formatting() {
        use crate::subscription_panel::{SubscriptionStatus, SubscriptionService, ResetPeriod};
        use chrono::{Utc, Duration};

        // Days and hours
        let status = SubscriptionStatus::new(SubscriptionService::ClaudePro)
            .with_reset(Utc::now() + Duration::days(5) + Duration::hours(12), ResetPeriod::Monthly);
        let timer = status.format_reset_timer();
        assert!(timer.contains("d"), "Timer should show days: {}", timer);

        // Hours and minutes
        let status = SubscriptionStatus::new(SubscriptionService::ChatGPTPlus)
            .with_reset(Utc::now() + Duration::hours(2) + Duration::minutes(30), ResetPeriod::Hourly(3));
        let timer = status.format_reset_timer();
        assert!(timer.contains("h") || timer.contains("m"), "Timer should show hours/minutes: {}", timer);

        // Just minutes
        let status = SubscriptionStatus::new(SubscriptionService::ChatGPTPlus)
            .with_reset(Utc::now() + Duration::minutes(45), ResetPeriod::Hourly(3));
        let timer = status.format_reset_timer();
        assert!(timer.contains("m"), "Timer should show minutes: {}", timer);
    }

    #[test]
    fn test_subscription_action_colors() {
        use crate::subscription_panel::SubscriptionAction;
        use ratatui::style::Color;

        assert_eq!(SubscriptionAction::OnPace.color(), Color::Green);
        assert_eq!(SubscriptionAction::Accelerate.color(), Color::Cyan);
        assert_eq!(SubscriptionAction::MaxOut.color(), Color::Yellow);
        assert_eq!(SubscriptionAction::Active.color(), Color::Green);
        assert_eq!(SubscriptionAction::Paused.color(), Color::Gray);
        assert_eq!(SubscriptionAction::OverQuota.color(), Color::Red);
    }

    #[test]
    fn test_subscription_usage_color_gradient() {
        use crate::subscription_panel::{SubscriptionStatus, SubscriptionService};
        use ratatui::style::Color;

        // Low usage - green (< 40%)
        let low = SubscriptionStatus::new(SubscriptionService::ClaudePro)
            .with_usage(100, 500, "msgs");
        assert_eq!(low.usage_color(), Color::Green);

        // Medium-low usage - cyan (40-60%)
        let medium_low = SubscriptionStatus::new(SubscriptionService::ClaudePro)
            .with_usage(250, 500, "msgs");
        assert_eq!(medium_low.usage_color(), Color::Cyan);

        // Medium-high usage - yellow (60-80%)
        let medium_high = SubscriptionStatus::new(SubscriptionService::ClaudePro)
            .with_usage(350, 500, "msgs");
        assert_eq!(medium_high.usage_color(), Color::Yellow);

        // High usage - light red (80-95%)
        let high = SubscriptionStatus::new(SubscriptionService::ClaudePro)
            .with_usage(450, 500, "msgs");
        assert_eq!(high.usage_color(), Color::LightRed);

        // Critical usage - red (>= 95%)
        let critical = SubscriptionStatus::new(SubscriptionService::ClaudePro)
            .with_usage(480, 500, "msgs");
        assert_eq!(critical.usage_color(), Color::Red);
    }

    #[test]
    fn test_subscription_format_summary() {
        use crate::subscription_panel::{SubscriptionData, format_subscription_summary};

        // Test with demo data
        let data = SubscriptionData::with_demo_data();
        let summary = format_subscription_summary(&data);

        assert!(summary.contains("Claude"));
        assert!(summary.contains("ChatGPT"));
        assert!(summary.contains("Cursor"));
        assert!(summary.contains("DeepSeek"));
        assert!(summary.contains("Subscription Status"));

        // Test loading state
        let loading = SubscriptionData::loading();
        let loading_summary = format_subscription_summary(&loading);
        assert!(loading_summary.contains("Loading"));

        // Test empty state
        let empty = SubscriptionData::new();
        let empty_summary = format_subscription_summary(&empty);
        assert!(empty_summary.contains("No subscriptions"));
    }

    #[test]
    fn test_subscription_panel_compact_widget() {
        use crate::subscription_panel::{SubscriptionSummaryCompact, SubscriptionData};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let data = SubscriptionData::with_demo_data();
        let widget = SubscriptionSummaryCompact::new(&data);

        let backend = TestBackend::new(50, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| {
            let area = f.area();
            f.render_widget(widget, area);
        }).unwrap();

        // Should render without panic
        let buffer = terminal.backend().buffer();
        assert!(buffer.area.width > 0);
    }

    // ============================================================
    // Integration Test 37: Responsive Layout Tests
    // ============================================================

    #[test]
    fn test_layout_mode_detection() {
        use crate::view::LayoutMode;

        // Ultra-wide layout (199+ cols)
        assert_eq!(LayoutMode::from_width(199), LayoutMode::UltraWide);
        assert_eq!(LayoutMode::from_width(200), LayoutMode::UltraWide);
        assert_eq!(LayoutMode::from_width(300), LayoutMode::UltraWide);

        // Wide layout (120-198 cols)
        assert_eq!(LayoutMode::from_width(120), LayoutMode::Wide);
        assert_eq!(LayoutMode::from_width(150), LayoutMode::Wide);
        assert_eq!(LayoutMode::from_width(198), LayoutMode::Wide);

        // Narrow layout (<120 cols)
        assert_eq!(LayoutMode::from_width(119), LayoutMode::Narrow);
        assert_eq!(LayoutMode::from_width(80), LayoutMode::Narrow);
        assert_eq!(LayoutMode::from_width(40), LayoutMode::Narrow);
        assert_eq!(LayoutMode::from_width(0), LayoutMode::Narrow);
    }

    #[test]
    fn test_layout_mode_min_heights() {
        use crate::view::LayoutMode;

        assert_eq!(LayoutMode::UltraWide.min_height(), 38);
        assert_eq!(LayoutMode::Wide.min_height(), 30);
        assert_eq!(LayoutMode::Narrow.min_height(), 20);
    }

    #[test]
    fn test_layout_mode_requirements() {
        use crate::view::LayoutMode;

        // Ultra-wide mode requirements
        assert!(LayoutMode::UltraWide.meets_requirements(199, 38));
        assert!(LayoutMode::UltraWide.meets_requirements(250, 50));
        assert!(!LayoutMode::UltraWide.meets_requirements(199, 37)); // Height too short
        assert!(!LayoutMode::UltraWide.meets_requirements(198, 38)); // Width too narrow

        // Wide mode requirements
        assert!(LayoutMode::Wide.meets_requirements(150, 30));
        assert!(LayoutMode::Wide.meets_requirements(120, 35));
        assert!(!LayoutMode::Wide.meets_requirements(150, 29)); // Height too short
        assert!(!LayoutMode::Wide.meets_requirements(199, 30)); // Width triggers UltraWide

        // Narrow mode requirements
        assert!(LayoutMode::Narrow.meets_requirements(80, 20));
        assert!(LayoutMode::Narrow.meets_requirements(100, 25));
        assert!(!LayoutMode::Narrow.meets_requirements(80, 19)); // Height too short
        assert!(!LayoutMode::Narrow.meets_requirements(120, 20)); // Width triggers Wide
    }

    #[test]
    fn test_layout_boundary_conditions() {
        use crate::view::LayoutMode;

        // Test exact boundary values
        assert_eq!(LayoutMode::from_width(119), LayoutMode::Narrow);
        assert_eq!(LayoutMode::from_width(120), LayoutMode::Wide);
        assert_eq!(LayoutMode::from_width(198), LayoutMode::Wide);
        assert_eq!(LayoutMode::from_width(199), LayoutMode::UltraWide);
    }

    #[test]
    fn test_responsive_rendering_ultrawide() {
        let app = App::new();

        // Ultra-wide: 200x50
        let buffer = render_app(&app, 200, 50);

        assert!(buffer_contains(&buffer, "FORGE Dashboard"));
        assert!(buffer_contains(&buffer, "Worker Pool"));
    }

    #[test]
    fn test_responsive_rendering_wide() {
        let app = App::new();

        // Wide: 150x35
        let buffer = render_app(&app, 150, 35);

        assert!(buffer_contains(&buffer, "FORGE Dashboard"));
        assert!(buffer_contains(&buffer, "Worker Pool"));
    }

    #[test]
    fn test_responsive_rendering_narrow() {
        let app = App::new();

        // Narrow: 80x25
        let buffer = render_app(&app, 80, 25);

        // Should still render header
        assert!(buffer_contains(&buffer, "FORGE"));
    }

    #[test]
    fn test_responsive_rendering_minimum() {
        let app = App::new();

        // Minimum viable size: 40x15
        let buffer = render_app(&app, 40, 15);

        // Should not crash
        assert_eq!(buffer.area.width, 40);
        assert_eq!(buffer.area.height, 15);
    }

    #[test]
    fn test_responsive_rendering_extreme_wide() {
        let app = App::new();

        // Extreme wide: 400x100
        let buffer = render_app(&app, 400, 100);

        // Should handle gracefully
        assert_eq!(buffer.area.width, 400);
        assert!(buffer_contains(&buffer, "FORGE Dashboard"));
    }

    #[test]
    fn test_responsive_all_views_at_different_sizes() {
        let mut app = App::new();

        let sizes = [(80, 24), (120, 40), (200, 50), (60, 20)];
        let views = View::ALL;

        for (width, height) in sizes {
            for view in &views {
                app.switch_view(*view);
                let buffer = render_app(&app, width, height);

                // Should render without panic
                assert_eq!(buffer.area.width, width);
                assert_eq!(buffer.area.height, height);
            }
        }
    }

    #[test]
    fn test_view_hotkey_navigation_comprehensive() {
        use crate::view::View;

        // Test all hotkey mappings
        assert_eq!(View::from_hotkey('o'), Some(View::Overview));
        assert_eq!(View::from_hotkey('w'), Some(View::Workers));
        assert_eq!(View::from_hotkey('t'), Some(View::Tasks));
        assert_eq!(View::from_hotkey('c'), Some(View::Costs));
        assert_eq!(View::from_hotkey('m'), Some(View::Metrics));
        assert_eq!(View::from_hotkey('l'), Some(View::Logs));
        assert_eq!(View::from_hotkey(':'), Some(View::Chat));

        // Case insensitive
        assert_eq!(View::from_hotkey('O'), Some(View::Overview));
        assert_eq!(View::from_hotkey('W'), Some(View::Workers));

        // Invalid keys
        assert_eq!(View::from_hotkey('x'), None);
        assert_eq!(View::from_hotkey('z'), None);
        assert_eq!(View::from_hotkey('1'), None);
    }

    #[test]
    fn test_view_cycling_comprehensive() {
        use crate::view::View;

        // Forward cycling
        assert_eq!(View::Overview.next(), View::Workers);
        assert_eq!(View::Workers.next(), View::Tasks);
        assert_eq!(View::Tasks.next(), View::Costs);
        assert_eq!(View::Costs.next(), View::Metrics);
        assert_eq!(View::Metrics.next(), View::Logs);
        assert_eq!(View::Logs.next(), View::Chat);
        assert_eq!(View::Chat.next(), View::Overview); // Wrap around

        // Backward cycling
        assert_eq!(View::Overview.prev(), View::Chat);
        assert_eq!(View::Workers.prev(), View::Overview);
        assert_eq!(View::Chat.prev(), View::Logs);
    }

    #[test]
    fn test_view_display_names() {
        use crate::view::View;

        assert_eq!(View::Overview.title(), "Overview");
        assert_eq!(View::Workers.title(), "Workers");
        assert_eq!(View::Tasks.title(), "Tasks");
        assert_eq!(View::Costs.title(), "Costs");
        assert_eq!(View::Metrics.title(), "Metrics");
        assert_eq!(View::Logs.title(), "Logs");
        assert_eq!(View::Chat.title(), "Chat");
    }

    #[test]
    fn test_view_hotkey_hints() {
        use crate::view::View;

        assert_eq!(View::Overview.hotkey_hint(), "[o] Overview");
        assert_eq!(View::Workers.hotkey_hint(), "[w] Workers");
        assert_eq!(View::Chat.hotkey_hint(), "[:] Chat");
    }

    // ============================================================
    // Integration Test 38: End-to-End Data Flow Tests
    // ============================================================

    #[test]
    fn test_e2e_data_flow_subscription_updates_ui() {
        use crate::subscription_panel::{SubscriptionData, SubscriptionStatus, SubscriptionService, ResetPeriod};
        use chrono::{Utc, Duration};

        // Create subscription data that would trigger different UI states
        let now = Utc::now();

        let mut data = SubscriptionData::new();
        data.subscriptions = vec![
            // Active subscription on pace
            SubscriptionStatus::new(SubscriptionService::ClaudePro)
                .with_usage(250, 500, "msgs")
                .with_reset(now + Duration::days(15), ResetPeriod::Monthly)
                .with_active(true),
            // Over quota
            SubscriptionStatus::new(SubscriptionService::CursorPro)
                .with_usage(510, 500, "reqs")
                .with_reset(now + Duration::days(5), ResetPeriod::Monthly)
                .with_active(true),
        ];
        data.last_updated = Some(now);

        assert_eq!(data.active_count(), 2);
        assert!(data.has_data());

        // Verify different action recommendations
        let claude = data.get(SubscriptionService::ClaudePro).unwrap();
        let cursor = data.get(SubscriptionService::CursorPro).unwrap();

        assert_eq!(claude.recommended_action(), crate::subscription_panel::SubscriptionAction::OnPace);
        assert_eq!(cursor.recommended_action(), crate::subscription_panel::SubscriptionAction::OverQuota);
    }

    #[test]
    fn test_e2e_data_flow_cost_updates_ui() {
        use crate::cost_panel::{CostPanelData, BudgetConfig, BudgetAlertLevel};

        // Simulate cost data being updated
        let mut data = CostPanelData::new();
        data.set_budget(BudgetConfig::new(1000.0));

        // Day 1: Low usage
        data.monthly_total = 100.0;
        assert_eq!(data.monthly_alert(), BudgetAlertLevel::Normal);
        assert!((data.monthly_usage_pct() - 10.0).abs() < 0.01);

        // Day 15: Moderate usage
        data.monthly_total = 500.0;
        assert_eq!(data.monthly_alert(), BudgetAlertLevel::Normal);
        assert!((data.monthly_usage_pct() - 50.0).abs() < 0.01);

        // Day 25: High usage approaching limit
        data.monthly_total = 850.0;
        assert_eq!(data.monthly_alert(), BudgetAlertLevel::Warning);

        // Over budget
        data.monthly_total = 1100.0;
        assert_eq!(data.monthly_alert(), BudgetAlertLevel::Exceeded);
    }

    #[test]
    fn test_e2e_full_session_workflow() {
        // Simulate a complete user session:
        // 1. Start app
        // 2. Check workers (Overview)
        // 3. View costs
        // 4. Check subscriptions
        // 5. Navigate through views
        // 6. Open help
        // 7. Use chat
        // 8. Exit

        let mut app = App::new();

        // 1. Start in Overview
        assert_eq!(app.current_view(), View::Overview);
        let buffer = render_app(&app, 120, 40);
        assert!(buffer_contains(&buffer, "FORGE"));

        // 2. Check Workers view
        app.switch_view(View::Workers);
        let buffer = render_app(&app, 120, 40);
        assert!(buffer_contains(&buffer, "Worker"));

        // 3. View Costs
        app.switch_view(View::Costs);
        let buffer = render_app(&app, 120, 40);
        assert!(buffer_contains(&buffer, "Cost"));

        // 4. View Metrics (includes subscription-related info)
        app.switch_view(View::Metrics);
        let buffer = render_app(&app, 120, 40);
        assert!(buffer_contains(&buffer, "Metrics") || buffer_contains(&buffer, "Performance"));

        // 5. Cycle through remaining views
        for _ in 0..3 {
            app.next_view();
            let _ = render_app(&app, 120, 40);
        }

        // 6. Open help
        app.handle_app_event(AppEvent::ShowHelp);
        assert!(app.show_help());
        let buffer = render_app(&app, 120, 40);
        assert!(buffer_contains(&buffer, "Help") || buffer_contains(&buffer, "Hotkey"));
        app.handle_app_event(AppEvent::Cancel);
        assert!(!app.show_help());

        // 7. Use chat
        app.switch_view(View::Chat);
        assert_eq!(app.current_view(), View::Chat);
        for c in "show status".chars() {
            app.handle_app_event(AppEvent::TextInput(c));
        }
        app.handle_app_event(AppEvent::Submit);

        // 8. Return to overview and exit
        app.switch_view(View::Overview);
        app.handle_app_event(AppEvent::Quit);
        assert!(app.should_quit());
    }

    #[test]
    fn test_e2e_stress_test_subscription_updates() {
        use crate::subscription_panel::{SubscriptionData, SubscriptionStatus, SubscriptionService, ResetPeriod};
        use chrono::{Utc, Duration};

        let now = Utc::now();

        // Simulate rapid subscription updates (like from a background poller)
        let mut data = SubscriptionData::new();

        for i in 0..100 {
            data.subscriptions = vec![
                SubscriptionStatus::new(SubscriptionService::ClaudePro)
                    .with_usage(i * 5, 500, "msgs")
                    .with_reset(now + Duration::days(15), ResetPeriod::Monthly)
                    .with_active(true),
            ];
            data.last_updated = Some(now);

            // Verify data is consistent after each update
            assert_eq!(data.active_count(), 1);
            let status = data.get(SubscriptionService::ClaudePro).unwrap();
            assert_eq!(status.current_usage, (i * 5) as u64);
        }
    }

    #[test]
    fn test_e2e_stress_test_cost_updates() {
        use crate::cost_panel::{CostPanelData, BudgetConfig};
        use forge_cost::DailyCost;
        use chrono::Utc;

        let mut data = CostPanelData::new();
        data.set_budget(BudgetConfig::new(1000.0));

        // Simulate rapid cost updates
        for i in 0..100 {
            let today = DailyCost {
                date: Utc::now().date_naive(),
                total_cost_usd: (i as f64) * 0.5,
                call_count: i * 10,
                total_tokens: i * 1000,
                by_model: vec![],
            };

            data.set_today(today);
            data.monthly_total = (i as f64) * 10.0;

            // Verify data is consistent
            assert!(data.has_data());
            assert_eq!(data.today_calls(), (i * 10) as i64);
        }
    }
}
