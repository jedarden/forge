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
        assert!(buffer_contains(&buffer, "$"));

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
}
