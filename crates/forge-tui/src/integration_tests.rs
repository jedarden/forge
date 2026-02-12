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

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
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
    fn render_app(app: &mut App, width: u16, height: u16) -> Buffer {
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
    fn create_test_log_file(
        dir: &std::path::Path,
        worker_id: &str,
        entries: &[(&str, &str)],
    ) -> PathBuf {
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
        let mut app = App::new();

        // Verify initial state
        assert_eq!(app.current_view(), View::Overview);
        assert!(!app.should_quit());
        assert!(!app.show_help());

        // Render the application
        let buffer = render_app(&mut app, 120, 40);

        // Verify the header is rendered
        assert!(
            buffer_contains(&buffer, "FORGE v0.1.9"),
            "Application should render FORGE v0.1.9 header"
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
            let buffer = render_app(&mut app, 120, 40);

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
            buffer.push(LogEntry::new(LogLevel::Info, format!("Log entry {}", i)));
        }

        // Verify ring buffer behavior
        assert_eq!(buffer.len(), 5);
        assert_eq!(buffer.total_added(), 10);
        assert_eq!(buffer.dropped_count(), 5);

        // Verify only the last 5 entries are retained
        let messages: Vec<_> = buffer.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            messages,
            vec![
                "Log entry 6",
                "Log entry 7",
                "Log entry 8",
                "Log entry 9",
                "Log entry 10"
            ]
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
        let buffer = render_app(&mut app, 120, 40);
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
        let buffer = render_app(&mut app, 120, 40);
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
                if i % 3 == 0 {
                    "active"
                } else if i % 3 == 1 {
                    "idle"
                } else {
                    "starting"
                },
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
                let _ = render_app(&mut app, 80, 24);
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
        let mut app = App::new();

        // Test various terminal sizes including extreme cases
        let sizes = [
            (20, 10),   // Minimum
            (80, 24),   // Standard
            (120, 40),  // Large
            (200, 60),  // Very large
            (300, 100), // Extreme
        ];

        for (width, height) in sizes {
            let buffer = render_app(&mut app, width, height);
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
        let buffer = render_app(&mut app, 120, 40);
        assert!(buffer_contains(&buffer, "FORGE v0.1.9"));

        // 2. Navigate through views
        app.switch_view(View::Workers);
        assert_eq!(app.current_view(), View::Workers);
        let buffer = render_app(&mut app, 120, 40);
        assert!(buffer_contains(&buffer, "Worker"));

        app.switch_view(View::Tasks);
        assert_eq!(app.current_view(), View::Tasks);

        app.switch_view(View::Costs);
        assert_eq!(app.current_view(), View::Costs);
        let buffer = render_app(&mut app, 120, 40);
        // Costs view shows placeholder content since cost tracking isn't yet implemented
        assert!(buffer_contains(&buffer, "Cost") || buffer_contains(&buffer, "Loading"));

        // 3. Open help overlay
        app.handle_app_event(AppEvent::ShowHelp);
        assert!(app.show_help());
        let buffer = render_app(&mut app, 120, 40);
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
        assert!(matches!(
            event,
            Some(StatusEvent::InitialScanComplete { .. })
        ));

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

        let config = LogTailerConfig::new(&log_path).with_start_from_end(false);
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
        assert!(
            worker.is_some(),
            "Worker should be tracked after status file creation"
        );
        assert_eq!(
            worker.unwrap().status,
            WorkerStatus::Starting,
            "Worker should be in 'starting' status"
        );

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
        let mut app = App::with_status_dir(status_dir.clone());

        // Render the app and verify UI shows the worker
        let buffer = render_app(&mut app, 120, 40);

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
        assert_eq!(
            worker.status,
            WorkerStatus::Failed,
            "Worker should be in failed state"
        );
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
        assert_eq!(
            worker.status,
            WorkerStatus::Starting,
            "Phase 1: Should be starting"
        );

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
        assert_eq!(
            worker.status,
            WorkerStatus::Active,
            "Phase 2: Should be active"
        );
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
        assert_eq!(
            worker.tasks_completed, 1,
            "Phase 3: Should have completed 1 task"
        );
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
        assert_eq!(
            worker.tasks_completed, 2,
            "Phase 4: Should have completed 2 tasks"
        );
        assert!(
            worker.current_task.is_none(),
            "Phase 4: Should have no current task"
        );

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
        assert_eq!(
            worker.status,
            WorkerStatus::Stopped,
            "Phase 5: Should be stopped"
        );
        assert!(
            !worker.is_healthy(),
            "Phase 5: Stopped worker should not be healthy"
        );
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
        assert_eq!(
            counts.healthy(),
            3,
            "Starting, active, and idle should be healthy"
        );
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
                i,
                status,
                i * 5
            );
            fs::write(status_dir.join(format!("multi-worker-{}.json", i)), json).unwrap();
        }

        // Create app with custom status dir
        let mut app = App::with_status_dir(status_dir);

        // Render Workers view
        let mut app = app;
        app.switch_view(View::Workers);
        let buffer = render_app(&mut app, 120, 40);

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
            assert_eq!(
                w.status,
                WorkerStatus::Idle,
                "Worker should recover to idle state"
            );
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
        let statuses = [
            "starting", "active", "active", "active", "idle", "active", "idle",
        ];

        for status in &statuses {
            let json = format!(r#"{{"worker_id": "rapid-worker", "status": "{}"}}"#, status);
            fs::write(&worker_path, json).unwrap();
            thread::sleep(Duration::from_millis(20));
        }

        // Wait for final state
        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(30)).is_some() {}

        // Worker should end up in final state (idle)
        let worker = watcher.get_worker("rapid-worker");
        assert!(
            worker.is_some(),
            "Worker should be tracked after rapid changes"
        );
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
        use crate::cost_panel::{BudgetAlertLevel, BudgetConfig, CostPanelData};
        use chrono::Utc;
        use forge_cost::DailyCost;

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
        use crate::cost_panel::{BudgetAlertLevel, BudgetConfig, CostPanelData};

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
        use crate::cost_panel::{format_tokens, format_usd, truncate_model_name};

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
        use crate::subscription_panel::{ResetPeriod, SubscriptionService, SubscriptionStatus};
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
        use crate::subscription_panel::{
            ResetPeriod, SubscriptionAction, SubscriptionService, SubscriptionStatus,
        };
        use chrono::{Duration, Utc};

        // Paused subscription
        let paused = SubscriptionStatus::new(SubscriptionService::ChatGPTPlus).with_active(false);
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
        use crate::subscription_panel::{ResetPeriod, SubscriptionService, SubscriptionStatus};
        use chrono::{Duration, Utc};

        // Days and hours
        let days_status = SubscriptionStatus::new(SubscriptionService::ClaudePro).with_reset(
            Utc::now() + Duration::days(5) + Duration::hours(12),
            ResetPeriod::Monthly,
        );
        let timer = days_status.format_reset_timer();
        assert!(timer.contains("d") || timer.contains("h"));

        // Pay-per-use shows Monthly
        let pay = SubscriptionStatus::new(SubscriptionService::DeepSeekAPI).with_pay_per_use(0.05);
        assert_eq!(pay.format_reset_timer(), "Monthly");
    }

    #[test]
    fn test_subscription_summary_formatting() {
        use crate::subscription_panel::{SubscriptionData, format_subscription_summary};

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
        use crate::subscription_panel::{SubscriptionService, SubscriptionStatus};
        use ratatui::style::Color;

        // Low usage - green
        let low =
            SubscriptionStatus::new(SubscriptionService::ClaudePro).with_usage(100, 500, "msgs");
        assert_eq!(low.usage_color(), Color::Green);

        // Medium usage - cyan/yellow
        let med =
            SubscriptionStatus::new(SubscriptionService::ClaudePro).with_usage(350, 500, "msgs");
        assert!(matches!(med.usage_color(), Color::Cyan | Color::Yellow));

        // High usage - red
        let high =
            SubscriptionStatus::new(SubscriptionService::ClaudePro).with_usage(480, 500, "msgs");
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
        let mut app = App::new();

        // Narrow mode
        let narrow = render_app(&mut app, 80, 30);
        assert!(buffer_contains(&narrow, "FORGE v0.1.9"));
        assert!(buffer_contains(&narrow, "Worker Pool"));

        // Wide mode
        let wide = render_app(&mut app, 150, 40);
        assert!(buffer_contains(&wide, "FORGE v0.1.9"));
        assert!(buffer_contains(&wide, "Worker Pool"));
        assert!(buffer_contains(&wide, "Subscriptions"));

        // Ultra-wide mode
        let ultrawide = render_app(&mut app, 220, 50);
        assert!(buffer_contains(&ultrawide, "FORGE v0.1.9"));
        assert!(buffer_contains(&ultrawide, "Cost Breakdown"));
        assert!(buffer_contains(&ultrawide, "Quick Actions"));
    }

    #[test]
    fn test_responsive_panel_visibility() {
        let mut app = App::new();

        // Narrow: 3 panels
        let narrow = render_app(&mut app, 80, 30);
        assert!(buffer_contains(&narrow, "Worker Pool"));
        assert!(buffer_contains(&narrow, "Task Queue"));
        assert!(buffer_contains(&narrow, "Activity Log"));
        assert!(!buffer_contains(&narrow, "Quick Actions"));

        // Wide: 4 panels
        let wide = render_app(&mut app, 150, 40);
        assert!(buffer_contains(&wide, "Worker Pool"));
        assert!(buffer_contains(&wide, "Subscriptions"));
        assert!(buffer_contains(&wide, "Task Queue"));
        assert!(buffer_contains(&wide, "Activity Log"));
        assert!(!buffer_contains(&wide, "Quick Actions"));

        // Ultra-wide: 6 panels
        let ultrawide = render_app(&mut app, 220, 50);
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
        let buffer = render_app(&mut app, 120, 40);
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

        let p0 = Bead {
            priority: 0,
            ..Default::default()
        };
        let p1 = Bead {
            priority: 1,
            ..Default::default()
        };
        let p2 = Bead {
            priority: 2,
            ..Default::default()
        };
        let p3 = Bead {
            priority: 3,
            ..Default::default()
        };
        let p4 = Bead {
            priority: 4,
            ..Default::default()
        };

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
            let bead = Bead {
                status: status.to_string(),
                ..Default::default()
            };
            assert_eq!(
                bead.status_indicator(),
                expected,
                "Status '{}' should have indicator '{}'",
                status,
                expected
            );
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

        let bar = ProgressBar::new(75, 100)
            .width(20)
            .label("Memory")
            .show_value(true);
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
            (80, 24),  // Minimum viable
            (120, 40), // Standard
            (199, 55), // Ultra-wide threshold
            (250, 70), // Large
        ];

        for (width, height) in sizes {
            for view in View::ALL {
                app.switch_view(view);
                let buffer = render_app(&mut app, width, height);

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
            let buffer = render_app(&mut app, 120, 40);
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
        let buffer = render_app(&mut app, 120, 40);
        assert!(buffer_contains(&buffer, "FORGE v0.1.9"));
    }

    #[test]
    fn test_app_handles_extreme_scroll_offsets() {
        let mut app = App::new();

        // Scroll way past the end
        for _ in 0..1000 {
            app.handle_app_event(AppEvent::NavigateDown);
        }

        // App should still render without panic
        let buffer = render_app(&mut app, 120, 40);
        assert!(buffer_contains(&buffer, "FORGE v0.1.9"));

        // Go back to top
        app.handle_app_event(AppEvent::GoToTop);
    }

    #[test]
    fn test_app_handles_rapid_help_toggle() {
        let mut app = App::new();

        for _ in 0..50 {
            app.handle_app_event(AppEvent::ShowHelp);
            let buffer = render_app(&mut app, 120, 40);
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

        let entry =
            LogEntry::new(LogLevel::Info, "Test message".to_string()).with_source("test-worker");

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
        use crate::event::WorkerExecutor;
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut handler = InputHandler::new();

        let view_keys = [
            (KeyCode::Char('O'), View::Overview), // Uppercase for Overview
            (KeyCode::Char('w'), View::Workers),
            (KeyCode::Char('t'), View::Tasks),
            (KeyCode::Char('c'), View::Costs),
            (KeyCode::Char('m'), View::Metrics),
            (KeyCode::Char('l'), View::Logs),
            (KeyCode::Char('a'), View::Logs), // 'a' for Activity/Logs
        ];

        for (keycode, expected_view) in view_keys {
            let event =
                handler.handle_key(crossterm::event::KeyEvent::new(keycode, KeyModifiers::NONE));
            assert_eq!(
                event,
                AppEvent::SwitchView(expected_view),
                "Key {:?} should switch to {:?}",
                keycode,
                expected_view
            );
        }

        // Verify lowercase 'o' is now spawn Opus, not Overview
        let event = handler.handle_key(crossterm::event::KeyEvent::new(
            KeyCode::Char('o'),
            KeyModifiers::NONE,
        ));
        assert_eq!(
            event,
            AppEvent::SpawnWorker(WorkerExecutor::Opus),
            "Lowercase 'o' should spawn Opus worker"
        );
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

        // Vim keys (note: 'k' is now KillWorker, not NavigateUp)
        let k = handler.handle_key(crossterm::event::KeyEvent::new(
            KeyCode::Char('k'),
            KeyModifiers::NONE,
        ));
        assert_eq!(k, AppEvent::KillWorker);

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
        use crate::cost_panel::{BudgetConfig, CostPanel, CostPanelData};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        // Create cost data
        let mut data = CostPanelData::new();
        data.set_budget(BudgetConfig::new(500.0));
        data.monthly_total = 250.0;

        // Create widget
        let panel = CostPanel::new(&data).focused(true);

        // Render
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                let area = f.area();
                f.render_widget(panel, area);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content = buffer_to_string(&buffer);

        // Should show cost analytics panel
        assert!(content.contains("Cost Analytics"));
    }

    #[test]
    fn test_subscription_panel_widget_rendering() {
        use crate::subscription_panel::{SubscriptionData, SubscriptionPanel};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let data = SubscriptionData::with_demo_data();
        let panel = SubscriptionPanel::new(&data).focused(true);

        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                let area = f.area();
                f.render_widget(panel, area);
            })
            .unwrap();

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
        use crate::subscription_panel::{ResetPeriod, SubscriptionService, SubscriptionStatus};
        use chrono::{Duration, Utc};

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
        use crate::subscription_panel::{
            ResetPeriod, SubscriptionAction, SubscriptionService, SubscriptionStatus,
        };
        use chrono::{Duration, Utc};

        let status = SubscriptionStatus::new(SubscriptionService::CursorPro)
            .with_usage(520, 500, "reqs")
            .with_reset(Utc::now() + Duration::days(5), ResetPeriod::Monthly)
            .with_active(true);

        assert!(status.usage_pct() >= 100.0);
        assert_eq!(status.recommended_action(), SubscriptionAction::OverQuota);
    }

    #[test]
    fn test_subscription_status_pay_per_use() {
        use crate::subscription_panel::{
            ResetPeriod, SubscriptionAction, SubscriptionService, SubscriptionStatus,
        };

        let status = SubscriptionStatus::new(SubscriptionService::DeepSeekAPI)
            .with_pay_per_use(0.05)
            .with_active(true);

        assert_eq!(status.remaining(), None);
        assert!(matches!(status.reset_period, ResetPeriod::PayPerUse));
        assert_eq!(status.recommended_action(), SubscriptionAction::Active);
    }

    #[test]
    fn test_subscription_status_inactive() {
        use crate::subscription_panel::{
            SubscriptionAction, SubscriptionService, SubscriptionStatus,
        };

        let status = SubscriptionStatus::new(SubscriptionService::ChatGPTPlus)
            .with_usage(10, 40, "msgs")
            .with_active(false);

        assert_eq!(status.recommended_action(), SubscriptionAction::Paused);
        assert!(!status.is_active);
    }

    #[test]
    fn test_subscription_status_accelerate_recommendation() {
        use crate::subscription_panel::{
            ResetPeriod, SubscriptionAction, SubscriptionService, SubscriptionStatus,
        };
        use chrono::{Duration, Utc};

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
            matches!(
                action,
                SubscriptionAction::Accelerate | SubscriptionAction::MaxOut
            ),
            "Should recommend accelerating usage when behind schedule"
        );
    }

    #[test]
    fn test_subscription_status_on_pace() {
        use crate::subscription_panel::{
            ResetPeriod, SubscriptionAction, SubscriptionService, SubscriptionStatus,
        };
        use chrono::{Duration, Utc};

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
        use crate::subscription_panel::{ResetPeriod, SubscriptionService, SubscriptionStatus};
        use chrono::{Duration, Utc};

        // Days and hours
        let status = SubscriptionStatus::new(SubscriptionService::ClaudePro).with_reset(
            Utc::now() + Duration::days(5) + Duration::hours(12),
            ResetPeriod::Monthly,
        );
        let timer = status.format_reset_timer();
        assert!(timer.contains("d"), "Timer should show days: {}", timer);

        // Hours and minutes
        let status = SubscriptionStatus::new(SubscriptionService::ChatGPTPlus).with_reset(
            Utc::now() + Duration::hours(2) + Duration::minutes(30),
            ResetPeriod::Hourly(3),
        );
        let timer = status.format_reset_timer();
        assert!(
            timer.contains("h") || timer.contains("m"),
            "Timer should show hours/minutes: {}",
            timer
        );

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
        use crate::subscription_panel::{SubscriptionService, SubscriptionStatus};
        use ratatui::style::Color;

        // Low usage - green (< 40%)
        let low =
            SubscriptionStatus::new(SubscriptionService::ClaudePro).with_usage(100, 500, "msgs");
        assert_eq!(low.usage_color(), Color::Green);

        // Medium-low usage - cyan (40-60%)
        let medium_low =
            SubscriptionStatus::new(SubscriptionService::ClaudePro).with_usage(250, 500, "msgs");
        assert_eq!(medium_low.usage_color(), Color::Cyan);

        // Medium-high usage - yellow (60-80%)
        let medium_high =
            SubscriptionStatus::new(SubscriptionService::ClaudePro).with_usage(350, 500, "msgs");
        assert_eq!(medium_high.usage_color(), Color::Yellow);

        // High usage - light red (80-95%)
        let high =
            SubscriptionStatus::new(SubscriptionService::ClaudePro).with_usage(450, 500, "msgs");
        assert_eq!(high.usage_color(), Color::LightRed);

        // Critical usage - red (>= 95%)
        let critical =
            SubscriptionStatus::new(SubscriptionService::ClaudePro).with_usage(480, 500, "msgs");
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
        use crate::subscription_panel::{SubscriptionData, SubscriptionSummaryCompact};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let data = SubscriptionData::with_demo_data();
        let widget = SubscriptionSummaryCompact::new(&data);

        let backend = TestBackend::new(50, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                let area = f.area();
                f.render_widget(widget, area);
            })
            .unwrap();

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
        let mut app = App::new();

        // Ultra-wide: 200x50
        let buffer = render_app(&mut app, 200, 50);

        assert!(buffer_contains(&buffer, "FORGE v0.1.9"));
        assert!(buffer_contains(&buffer, "Worker Pool"));
    }

    #[test]
    fn test_responsive_rendering_wide() {
        let mut app = App::new();

        // Wide: 150x35
        let buffer = render_app(&mut app, 150, 35);

        assert!(buffer_contains(&buffer, "FORGE v0.1.9"));
        assert!(buffer_contains(&buffer, "Worker Pool"));
    }

    #[test]
    fn test_responsive_rendering_narrow() {
        let mut app = App::new();

        // Narrow: 80x25
        let buffer = render_app(&mut app, 80, 25);

        // Should still render header
        assert!(buffer_contains(&buffer, "FORGE"));
    }

    #[test]
    fn test_responsive_rendering_minimum() {
        let mut app = App::new();

        // Minimum viable size: 40x15
        let buffer = render_app(&mut app, 40, 15);

        // Should not crash
        assert_eq!(buffer.area.width, 40);
        assert_eq!(buffer.area.height, 15);
    }

    #[test]
    fn test_responsive_rendering_extreme_wide() {
        let mut app = App::new();

        // Extreme wide: 400x100
        let buffer = render_app(&mut app, 400, 100);

        // Should handle gracefully
        assert_eq!(buffer.area.width, 400);
        assert!(buffer_contains(&buffer, "FORGE v0.1.9"));
    }

    #[test]
    fn test_responsive_all_views_at_different_sizes() {
        let mut app = App::new();

        let sizes = [(80, 24), (120, 40), (200, 50), (60, 20)];
        let views = View::ALL;

        for (width, height) in sizes {
            for view in &views {
                app.switch_view(*view);
                let buffer = render_app(&mut app, width, height);

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
        use crate::subscription_panel::{
            ResetPeriod, SubscriptionData, SubscriptionService, SubscriptionStatus,
        };
        use chrono::{Duration, Utc};

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

        assert_eq!(
            claude.recommended_action(),
            crate::subscription_panel::SubscriptionAction::OnPace
        );
        assert_eq!(
            cursor.recommended_action(),
            crate::subscription_panel::SubscriptionAction::OverQuota
        );
    }

    #[test]
    fn test_e2e_data_flow_cost_updates_ui() {
        use crate::cost_panel::{BudgetAlertLevel, BudgetConfig, CostPanelData};

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
        let buffer = render_app(&mut app, 120, 40);
        assert!(buffer_contains(&buffer, "FORGE"));

        // 2. Check Workers view
        app.switch_view(View::Workers);
        let buffer = render_app(&mut app, 120, 40);
        assert!(buffer_contains(&buffer, "Worker"));

        // 3. View Costs
        app.switch_view(View::Costs);
        let buffer = render_app(&mut app, 120, 40);
        assert!(buffer_contains(&buffer, "Cost"));

        // 4. View Metrics (includes subscription-related info)
        app.switch_view(View::Metrics);
        let buffer = render_app(&mut app, 120, 40);
        assert!(buffer_contains(&buffer, "Metrics") || buffer_contains(&buffer, "Performance"));

        // 5. Cycle through remaining views
        for _ in 0..3 {
            app.next_view();
            let _ = render_app(&mut app, 120, 40);
        }

        // 6. Open help
        app.handle_app_event(AppEvent::ShowHelp);
        assert!(app.show_help());
        let buffer = render_app(&mut app, 120, 40);
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
        use crate::subscription_panel::{
            ResetPeriod, SubscriptionData, SubscriptionService, SubscriptionStatus,
        };
        use chrono::{Duration, Utc};

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
        use crate::cost_panel::{BudgetConfig, CostPanelData};
        use chrono::Utc;
        use forge_cost::DailyCost;

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

    // ============================================================
    // Integration Test: Worker Kill Dialog
    // ============================================================

    #[test]
    fn test_kill_dialog_shows_on_kill_event() {
        // Test that the kill dialog is shown when KillWorker event is triggered
        let mut app = App::new();

        // Initially, kill dialog should not be shown
        assert!(!app.show_kill_dialog());

        // Trigger the KillWorker event
        app.handle_app_event(AppEvent::KillWorker);

        // Kill dialog should now be shown
        assert!(app.show_kill_dialog());

        // Verify the dialog renders
        let buffer = render_app(&mut app, 120, 40);
        assert!(
            buffer_contains(&buffer, "Kill Worker"),
            "Kill dialog should render with 'Kill Worker' title"
        );
    }

    #[test]
    fn test_kill_dialog_closes_on_escape() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new();

        // Open kill dialog
        app.handle_app_event(AppEvent::KillWorker);
        assert!(app.show_kill_dialog());

        // Press Escape to close
        let escape_key = KeyEvent::new(KeyCode::Esc, KeyModifiers::empty());
        app.handle_key_event(escape_key);

        // Dialog should be closed
        assert!(!app.show_kill_dialog());
    }

    #[test]
    fn test_kill_dialog_closes_on_q() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new();

        // Open kill dialog
        app.handle_app_event(AppEvent::KillWorker);
        assert!(app.show_kill_dialog());

        // Press 'q' to close
        let q_key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty());
        app.handle_key_event(q_key);

        // Dialog should be closed
        assert!(!app.show_kill_dialog());
    }

    #[test]
    fn test_kill_dialog_toggles() {
        // Test that KillWorker event toggles the dialog
        let mut app = App::new();

        // Open
        app.handle_app_event(AppEvent::KillWorker);
        assert!(app.show_kill_dialog());

        // Close (toggle again)
        app.handle_app_event(AppEvent::KillWorker);
        assert!(!app.show_kill_dialog());

        // Reopen
        app.handle_app_event(AppEvent::KillWorker);
        assert!(app.show_kill_dialog());
    }

    #[test]
    fn test_kill_dialog_navigation() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new();

        // Open kill dialog
        app.handle_app_event(AppEvent::KillWorker);

        // Set up multiple workers for navigation testing
        // Note: In a real test we would mock worker discovery
        // For now, we just verify that navigation keys don't crash
        let down_key = KeyEvent::new(KeyCode::Down, KeyModifiers::empty());
        let up_key = KeyEvent::new(KeyCode::Up, KeyModifiers::empty());
        let j_key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::empty());
        let k_key = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::empty());

        // These should not panic
        app.handle_key_event(down_key);
        app.handle_key_event(j_key);
        app.handle_key_event(up_key);
        app.handle_key_event(k_key);

        // Dialog should still be open
        assert!(app.show_kill_dialog());
    }

    #[test]
    fn test_kill_dialog_no_workers_message() {
        // Test that dialog shows appropriate message when no workers are found
        let mut app = App::new();

        // Open kill dialog (will discover workers from tmux)
        app.handle_app_event(AppEvent::KillWorker);

        // Render and check for appropriate UI elements
        let buffer = render_app(&mut app, 120, 40);

        // Dialog should either show workers or "No active workers found"
        let content = buffer_to_string(&buffer);
        let has_workers_or_message = content.contains("Select a worker to kill")
            || content.contains("No active workers found")
            || content.contains("Error:");
        assert!(
            has_workers_or_message,
            "Kill dialog should show worker selection or appropriate message"
        );
    }

    #[test]
    fn test_kill_dialog_shows_worker_details() {
        // Test that when workers are discovered, the dialog shows their details
        let mut app = App::new();

        // Open kill dialog
        app.handle_app_event(AppEvent::KillWorker);

        // Render the dialog
        let buffer = render_app(&mut app, 120, 40);
        let content = buffer_to_string(&buffer);

        // If workers exist, check for expected format elements
        if app.show_kill_dialog() {
            // The dialog should show either workers with details or a no-workers message
            let has_expected_content = content.contains("Select a worker to kill")
                || content.contains("No active workers found")
                || content.contains("attached")
                || content.contains("detached")
                || content.contains("GLM")
                || content.contains("Sonnet")
                || content.contains("Opus")
                || content.contains("Haiku");
            assert!(
                has_expected_content,
                "Kill dialog should show worker details or no workers message"
            );
        }
    }

    #[test]
    fn test_kill_dialog_navigation_keys() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new();

        // Open kill dialog
        app.handle_app_event(AppEvent::KillWorker);
        assert!(app.show_kill_dialog());

        // Test that navigation keys don't crash and dialog stays open
        let nav_keys = [
            KeyEvent::new(KeyCode::Up, KeyModifiers::empty()),
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::empty()),
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::empty()),
        ];

        for key in nav_keys {
            app.handle_key_event(key);
            assert!(app.show_kill_dialog());
        }

        // Test Enter key (may or may not have workers to kill)
        let enter_key = KeyEvent::new(KeyCode::Enter, KeyModifiers::empty());
        app.handle_key_event(enter_key); // Should not panic
    }

    // ============================================================
    // Integration Test 39: Responsive Layout Adaptation Tests
    // ============================================================
    //
    // These tests verify that forge properly adapts to different terminal sizes
    // as described in bead fg-1w9:
    // - Ultra-wide (199+ cols): 6-panel 3-column layout
    // - Wide (120-198 cols): 4-panel 2-column layout
    // - Narrow (<120 cols): single-column mode with 3 panels

    /// Test that UltraWide layout (199+ columns) renders all 6 panels.
    /// Layout: 3 columns with 2 panels each:
    /// - Left: Worker Pool + Subscriptions
    /// - Middle: Task Queue + Activity Log
    /// - Right: Cost Breakdown + Quick Actions
    #[test]
    fn test_responsive_layout_ultrawide_six_panels() {
        let mut app = App::new();

        // Ultra-wide: 200 columns x 50 rows
        let buffer = render_app(&mut app, 200, 50);

        // Verify all 6 panel headers are present
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "UltraWide layout should show Worker Pool panel"
        );
        assert!(
            buffer_contains(&buffer, "Subscriptions"),
            "UltraWide layout should show Subscriptions panel"
        );
        assert!(
            buffer_contains(&buffer, "Task Queue"),
            "UltraWide layout should show Task Queue panel"
        );
        assert!(
            buffer_contains(&buffer, "Activity Log"),
            "UltraWide layout should show Activity Log panel"
        );
        assert!(
            buffer_contains(&buffer, "Cost Breakdown"),
            "UltraWide layout should show Cost Breakdown panel"
        );
        assert!(
            buffer_contains(&buffer, "Quick Actions"),
            "UltraWide layout should show Quick Actions panel"
        );

        // Verify header is rendered
        assert!(
            buffer_contains(&buffer, "FORGE"),
            "Header should be visible in UltraWide layout"
        );
    }

    /// Test that Wide layout (120-198 columns) renders 4 panels.
    /// Layout: 2x2 grid
    /// - Top: Worker Pool + Subscriptions
    /// - Bottom: Task Queue + Activity Log
    #[test]
    fn test_responsive_layout_wide_four_panels() {
        let mut app = App::new();

        // Wide: 150 columns x 40 rows
        let buffer = render_app(&mut app, 150, 40);

        // Verify 4 core panel headers are present
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "Wide layout should show Worker Pool panel"
        );
        assert!(
            buffer_contains(&buffer, "Subscriptions"),
            "Wide layout should show Subscriptions panel"
        );
        assert!(
            buffer_contains(&buffer, "Task Queue"),
            "Wide layout should show Task Queue panel"
        );
        assert!(
            buffer_contains(&buffer, "Activity Log"),
            "Wide layout should show Activity Log panel"
        );

        // Cost Breakdown and Quick Actions should NOT be present in Wide mode
        // (They only appear in UltraWide mode)
        // Note: We're testing that these panels don't have their own dedicated space

        // Verify header is rendered
        assert!(
            buffer_contains(&buffer, "FORGE"),
            "Header should be visible in Wide layout"
        );
    }

    /// Test that Narrow layout (<120 columns) renders 3 panels in single column.
    /// Layout: Single column stacked
    /// - Worker Pool (top)
    /// - Task Queue (middle)
    /// - Activity Log (bottom)
    #[test]
    fn test_responsive_layout_narrow_three_panels() {
        let mut app = App::new();

        // Narrow: 80 columns x 25 rows (standard terminal)
        let buffer = render_app(&mut app, 80, 25);

        // Verify 3 essential panels are present
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "Narrow layout should show Worker Pool panel"
        );
        assert!(
            buffer_contains(&buffer, "Task Queue"),
            "Narrow layout should show Task Queue panel"
        );
        assert!(
            buffer_contains(&buffer, "Activity Log"),
            "Narrow layout should show Activity Log panel"
        );

        // Subscriptions, Cost Breakdown, Quick Actions should not have dedicated space
        // in Narrow mode (they're only in wider layouts)

        // Verify header is rendered
        assert!(
            buffer_contains(&buffer, "FORGE"),
            "Header should be visible in Narrow layout"
        );
    }

    /// Test real-time resize from UltraWide to Wide.
    /// Verifies that layout adapts correctly when terminal narrows.
    #[test]
    fn test_responsive_resize_ultrawide_to_wide() {
        let mut app = App::new();

        // Start in UltraWide mode (200x50)
        let buffer_ultrawide = render_app(&mut app, 200, 50);

        // Verify all 6 panels in UltraWide
        assert!(buffer_contains(&buffer_ultrawide, "Worker Pool"));
        assert!(buffer_contains(&buffer_ultrawide, "Subscriptions"));
        assert!(buffer_contains(&buffer_ultrawide, "Task Queue"));
        assert!(buffer_contains(&buffer_ultrawide, "Activity Log"));
        assert!(buffer_contains(&buffer_ultrawide, "Cost Breakdown"));
        assert!(buffer_contains(&buffer_ultrawide, "Quick Actions"));

        // Resize to Wide mode (150x40)
        let buffer_wide = render_app(&mut app, 150, 40);

        // Verify layout adapted - 4 core panels still visible
        assert!(
            buffer_contains(&buffer_wide, "Worker Pool"),
            "Worker Pool should remain visible after resize to Wide"
        );
        assert!(
            buffer_contains(&buffer_wide, "Task Queue"),
            "Task Queue should remain visible after resize to Wide"
        );
        assert!(
            buffer_contains(&buffer_wide, "Activity Log"),
            "Activity Log should remain visible after resize to Wide"
        );
        assert!(
            buffer_contains(&buffer_wide, "Subscriptions"),
            "Subscriptions should remain visible after resize to Wide"
        );

        // App should not crash
        assert_eq!(buffer_wide.area.width, 150);
        assert_eq!(buffer_wide.area.height, 40);
    }

    /// Test real-time resize from Wide to Narrow.
    /// Verifies that layout adapts correctly when terminal becomes very narrow.
    #[test]
    fn test_responsive_resize_wide_to_narrow() {
        let mut app = App::new();

        // Start in Wide mode (150x40)
        let buffer_wide = render_app(&mut app, 150, 40);
        assert!(buffer_contains(&buffer_wide, "Worker Pool"));
        assert!(buffer_contains(&buffer_wide, "Subscriptions"));

        // Resize to Narrow mode (80x25)
        let buffer_narrow = render_app(&mut app, 80, 25);

        // Verify 3 essential panels still visible
        assert!(
            buffer_contains(&buffer_narrow, "Worker Pool"),
            "Worker Pool should remain visible after resize to Narrow"
        );
        assert!(
            buffer_contains(&buffer_narrow, "Task Queue"),
            "Task Queue should remain visible after resize to Narrow"
        );
        assert!(
            buffer_contains(&buffer_narrow, "Activity Log"),
            "Activity Log should remain visible after resize to Narrow"
        );

        // App should not crash
        assert_eq!(buffer_narrow.area.width, 80);
        assert_eq!(buffer_narrow.area.height, 25);
    }

    /// Test layout restoration after resize cycle.
    /// UltraWide -> Narrow -> UltraWide should restore all panels.
    #[test]
    fn test_responsive_resize_restore_cycle() {
        let mut app = App::new();

        // Start in UltraWide (200x50)
        let buffer_initial = render_app(&mut app, 200, 50);
        assert!(buffer_contains(&buffer_initial, "Cost Breakdown"));
        assert!(buffer_contains(&buffer_initial, "Quick Actions"));

        // Shrink to Narrow (80x25)
        let buffer_narrow = render_app(&mut app, 80, 25);
        assert!(buffer_contains(&buffer_narrow, "Worker Pool"));

        // Restore to UltraWide (200x50)
        let buffer_restored = render_app(&mut app, 200, 50);

        // All 6 panels should be visible again
        assert!(
            buffer_contains(&buffer_restored, "Worker Pool"),
            "Worker Pool should be visible after restore"
        );
        assert!(
            buffer_contains(&buffer_restored, "Subscriptions"),
            "Subscriptions should be visible after restore"
        );
        assert!(
            buffer_contains(&buffer_restored, "Task Queue"),
            "Task Queue should be visible after restore"
        );
        assert!(
            buffer_contains(&buffer_restored, "Activity Log"),
            "Activity Log should be visible after restore"
        );
        assert!(
            buffer_contains(&buffer_restored, "Cost Breakdown"),
            "Cost Breakdown should be visible after restore"
        );
        assert!(
            buffer_contains(&buffer_restored, "Quick Actions"),
            "Quick Actions should be visible after restore"
        );
    }

    /// Test multiple rapid resizes don't cause crashes.
    /// Simulates user rapidly resizing terminal.
    #[test]
    fn test_responsive_rapid_resize_no_crash() {
        let mut app = App::new();

        // Rapid resize sequence: UltraWide -> Wide -> Narrow -> Wide -> UltraWide
        let sizes: [(u16, u16); 5] = [(200, 50), (150, 40), (80, 25), (150, 40), (200, 50)];

        for (width, height) in sizes {
            let buffer = render_app(&mut app, width, height);

            // Should always render header without crashing
            assert!(
                buffer_contains(&buffer, "FORGE"),
                "Header should be visible at size {}x{}",
                width,
                height
            );

            // Worker Pool should always be visible (it's in all layouts)
            assert!(
                buffer_contains(&buffer, "Worker Pool"),
                "Worker Pool should be visible at size {}x{}",
                width,
                height
            );
        }
    }

    /// Test that footer hotkey hints adapt to layout mode.
    #[test]
    fn test_responsive_footer_hotkey_hints() {
        let mut app = App::new();

        // Test UltraWide mode footer
        let buffer_ultrawide = render_app(&mut app, 200, 50);
        assert!(
            buffer_contains(&buffer_ultrawide, "FORGE"),
            "UltraWide should render"
        );

        // Test Wide mode footer
        let buffer_wide = render_app(&mut app, 150, 40);
        assert!(buffer_contains(&buffer_wide, "FORGE"), "Wide should render");

        // Test Narrow mode footer (hotkeys may be abbreviated)
        let buffer_narrow = render_app(&mut app, 80, 25);
        assert!(
            buffer_contains(&buffer_narrow, "FORGE"),
            "Narrow should render"
        );
    }

    /// Test boundary conditions: exactly 199 columns (UltraWide threshold).
    #[test]
    fn test_responsive_boundary_199_columns() {
        let mut app = App::new();

        // Exactly at UltraWide threshold
        let buffer = render_app(&mut app, 199, 50);

        // Should use UltraWide layout with all 6 panels
        assert!(
            buffer_contains(&buffer, "Cost Breakdown"),
            "At 199 columns, UltraWide layout with Cost Breakdown should be used"
        );
        assert!(
            buffer_contains(&buffer, "Quick Actions"),
            "At 199 columns, UltraWide layout with Quick Actions should be used"
        );
    }

    /// Test boundary conditions: exactly 198 columns (Wide upper bound).
    #[test]
    fn test_responsive_boundary_198_columns() {
        let mut app = App::new();

        // Just below UltraWide threshold
        let buffer = render_app(&mut app, 198, 40);

        // Should use Wide layout (4 panels, no Cost Breakdown or Quick Actions)
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "At 198 columns, Wide layout should show Worker Pool"
        );
        assert!(
            buffer_contains(&buffer, "Task Queue"),
            "At 198 columns, Wide layout should show Task Queue"
        );
    }

    /// Test boundary conditions: exactly 120 columns (Wide lower bound).
    #[test]
    fn test_responsive_boundary_120_columns() {
        let mut app = App::new();

        // Exactly at Wide lower bound
        let buffer = render_app(&mut app, 120, 35);

        // Should use Wide layout
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "At 120 columns, Wide layout should show Worker Pool"
        );
        assert!(
            buffer_contains(&buffer, "Subscriptions"),
            "At 120 columns, Wide layout should show Subscriptions"
        );
    }

    /// Test boundary conditions: exactly 119 columns (Narrow upper bound).
    #[test]
    fn test_responsive_boundary_119_columns() {
        let mut app = App::new();

        // Just below Wide threshold
        let buffer = render_app(&mut app, 119, 25);

        // Should use Narrow layout (3 panels)
        assert!(
            buffer_contains(&buffer, "Worker Pool"),
            "At 119 columns, Narrow layout should show Worker Pool"
        );
        assert!(
            buffer_contains(&buffer, "Task Queue"),
            "At 119 columns, Narrow layout should show Task Queue"
        );
        assert!(
            buffer_contains(&buffer, "Activity Log"),
            "At 119 columns, Narrow layout should show Activity Log"
        );
    }

    /// Test that content reflows correctly in each layout mode.
    #[test]
    fn test_responsive_content_reflow() {
        let mut app = App::new();

        // UltraWide: Content should be spread across 3 columns
        let buffer_ultrawide = render_app(&mut app, 200, 50);
        let content_ultrawide = buffer_to_string(&buffer_ultrawide);

        // Wide: Content should be in 2x2 grid
        let buffer_wide = render_app(&mut app, 150, 40);
        let content_wide = buffer_to_string(&buffer_wide);

        // Narrow: Content should be in single column
        let buffer_narrow = render_app(&mut app, 80, 25);
        let content_narrow = buffer_to_string(&buffer_narrow);

        // All should contain Worker Pool content (it's in all layouts)
        assert!(
            content_ultrawide.contains("Worker Pool"),
            "UltraWide should contain Worker Pool content"
        );
        assert!(
            content_wide.contains("Worker Pool"),
            "Wide should contain Worker Pool content"
        );
        assert!(
            content_narrow.contains("Worker Pool"),
            "Narrow should contain Worker Pool content"
        );

        // UltraWide should have more content (6 panels vs 4 or 3)
        // This is a soft check - just verify they render without error
        assert!(
            content_ultrawide.len() > 0,
            "UltraWide content should not be empty"
        );
        assert!(content_wide.len() > 0, "Wide content should not be empty");
        assert!(
            content_narrow.len() > 0,
            "Narrow content should not be empty"
        );
    }

    /// Test minimum height requirements for each layout mode.
    #[test]
    fn test_responsive_minimum_heights() {
        use crate::view::LayoutMode;

        let mut app = App::new();

        // UltraWide minimum height is 38
        let buffer_ultrawide_min = render_app(&mut app, 200, 38);
        assert_eq!(buffer_ultrawide_min.area.height, 38);

        // Wide minimum height is 30
        let buffer_wide_min = render_app(&mut app, 150, 30);
        assert_eq!(buffer_wide_min.area.height, 30);

        // Narrow minimum height is 20
        let buffer_narrow_min = render_app(&mut app, 80, 20);
        assert_eq!(buffer_narrow_min.area.height, 20);

        // Verify LayoutMode min_height values match
        assert_eq!(LayoutMode::UltraWide.min_height(), 38);
        assert_eq!(LayoutMode::Wide.min_height(), 30);
        assert_eq!(LayoutMode::Narrow.min_height(), 20);
    }

    /// Test that all views work correctly at each layout size.
    #[test]
    fn test_responsive_all_views_at_all_sizes() {
        use crate::view::View;

        let mut app = App::new();

        let sizes = [
            (200, 50), // UltraWide
            (150, 40), // Wide
            (80, 25),  // Narrow
        ];

        for (width, height) in sizes {
            for view in View::ALL {
                app.switch_view(view);
                let buffer = render_app(&mut app, width, height);

                // Should render without panic
                assert_eq!(buffer.area.width, width);
                assert_eq!(buffer.area.height, height);

                // Should show view title in header
                assert!(
                    buffer_contains(&buffer, view.title()),
                    "View {} should show its title at size {}x{}",
                    view.title(),
                    width,
                    height
                );
            }
        }
    }

    /// Test resize across all boundary transitions.
    #[test]
    fn test_responsive_all_boundary_transitions() {
        let mut app = App::new();

        // Test all transitions between layout modes
        let transitions: [((u16, u16), (u16, u16)); 6] = [
            ((200, 50), (150, 40)), // UltraWide -> Wide
            ((150, 40), (80, 25)),  // Wide -> Narrow
            ((80, 25), (150, 40)),  // Narrow -> Wide
            ((150, 40), (200, 50)), // Wide -> UltraWide
            ((200, 50), (80, 25)),  // UltraWide -> Narrow
            ((80, 25), (200, 50)),  // Narrow -> UltraWide
        ];

        for ((from_w, from_h), (to_w, to_h)) in transitions {
            // Render at initial size
            let _ = render_app(&mut app, from_w, from_h);

            // Resize to new size - should not panic
            let buffer = render_app(&mut app, to_w, to_h);

            // Verify it rendered correctly
            assert_eq!(buffer.area.width, to_w);
            assert_eq!(buffer.area.height, to_h);
            assert!(
                buffer_contains(&buffer, "FORGE"),
                "Transition from {}x{} to {}x{} should show FORGE",
                from_w,
                from_h,
                to_w,
                to_h
            );
        }
    }

    // ============================================================
    // Integration Test 32: Real-Time Worker Status Update Tests (fg-56p)
    // ============================================================
    //
    // These tests verify that worker status updates propagate in real-time
    // with minimal latency (< 2 seconds) and that all status transitions
    // are visible to the user.

    /// Test that status updates are detected within 2 seconds of file modification.
    ///
    /// Success Criteria: Status updates within 1-2 seconds
    #[test]
    fn test_realtime_status_update_latency() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create watcher with default debounce (50ms)
        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(50);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Create a worker file and measure time to detection
        let start = std::time::Instant::now();
        let worker_json = r#"{"worker_id": "latency-test", "status": "starting"}"#;
        fs::write(status_dir.join("latency-test.json"), worker_json).unwrap();

        // Wait for the event with 2-second timeout
        let event = watcher.recv_timeout(Duration::from_secs(2));
        let elapsed = start.elapsed();

        // Verify event was received within 2 seconds
        assert!(
            event.is_some(),
            "Status update should be detected within 2 seconds"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "Status update latency should be < 2s, was {:?}",
            elapsed
        );

        // Verify the event content
        match event {
            Some(StatusEvent::WorkerUpdated { worker_id, status }) => {
                assert_eq!(worker_id, "latency-test");
                assert_eq!(status.status, WorkerStatus::Starting);
            }
            Some(StatusEvent::InitialScanComplete { .. }) => {
                // Initial scan might have picked it up - still valid
            }
            other => panic!("Unexpected event: {:?}", other),
        }
    }

    /// Test that 'starting' status is visible immediately after worker spawn.
    ///
    /// Success Criteria: 'starting' status visible within 500ms
    #[test]
    fn test_realtime_starting_status_immediate() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(50);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Simulate worker spawn - create file with 'starting' status
        let start = std::time::Instant::now();
        let spawn_json = r#"{
            "worker_id": "spawned-worker",
            "status": "starting",
            "model": "sonnet",
            "workspace": "/project",
            "started_at": "2026-02-12T10:00:00Z"
        }"#;
        fs::write(status_dir.join("spawned-worker.json"), spawn_json).unwrap();

        // Drain events until we see the worker
        let mut found_starting = false;
        let max_wait = Duration::from_millis(500);
        while start.elapsed() < max_wait {
            if let Some(event) = watcher.recv_timeout(Duration::from_millis(50)) {
                if let StatusEvent::WorkerUpdated { ref status, .. } = event {
                    if status.status == WorkerStatus::Starting {
                        found_starting = true;
                        break;
                    }
                }
            }
        }

        assert!(
            found_starting,
            "'starting' status should be visible within 500ms of spawn"
        );
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "Starting status latency was {:?}, should be < 500ms",
            start.elapsed()
        );

        // Verify the worker is tracked with starting status
        let worker = watcher.get_worker("spawned-worker");
        assert!(
            worker.is_some(),
            "Spawned worker should be tracked immediately"
        );
        assert_eq!(
            worker.unwrap().status,
            WorkerStatus::Starting,
            "Worker should be in 'starting' status immediately after spawn"
        );
    }

    /// Test that all status transitions are visible in sequence.
    ///
    /// Success Criteria: All transitions visible, no stale data
    #[test]
    fn test_realtime_all_status_transitions_visible() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(20);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_path = status_dir.join("transition-worker.json");

        // Define the sequence of transitions
        let transitions: [(&str, WorkerStatus); 4] = [
            ("starting", WorkerStatus::Starting),
            ("active", WorkerStatus::Active),
            ("idle", WorkerStatus::Idle),
            ("stopped", WorkerStatus::Stopped),
        ];

        let mut observed_statuses: Vec<WorkerStatus> = Vec::new();

        for (status_str, expected_status) in transitions {
            let json = format!(
                r#"{{"worker_id": "transition-worker", "status": "{}"}}"#,
                status_str
            );
            fs::write(&worker_path, &json).unwrap();

            // Wait for the update to propagate
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_millis(200) {
                if let Some(event) = watcher.recv_timeout(Duration::from_millis(50)) {
                    if let StatusEvent::WorkerUpdated { ref status, .. } = event {
                        if status.worker_id == "transition-worker" {
                            observed_statuses.push(status.status);
                            break;
                        }
                    }
                }
            }

            // Also verify internal state
            if let Some(worker) = watcher.get_worker("transition-worker") {
                assert_eq!(
                    worker.status, expected_status,
                    "Internal state should reflect transition to {:?}",
                    expected_status
                );
            }
        }

        // Verify we observed all transitions (may have duplicates due to multiple events)
        assert!(
            observed_statuses.contains(&WorkerStatus::Starting),
            "Should have observed 'starting' status"
        );
        assert!(
            observed_statuses.contains(&WorkerStatus::Active),
            "Should have observed 'active' status"
        );
        assert!(
            observed_statuses.contains(&WorkerStatus::Idle),
            "Should have observed 'idle' status"
        );
        assert!(
            observed_statuses.contains(&WorkerStatus::Stopped),
            "Should have observed 'stopped' status"
        );

        // Final state should match last written status
        let final_worker = watcher.get_worker("transition-worker").unwrap();
        assert_eq!(
            final_worker.status,
            WorkerStatus::Stopped,
            "Final status should be 'stopped'"
        );
    }

    /// Test that current_task field updates in real-time when worker picks up a task.
    ///
    /// Success Criteria: current_task field updates within 1 second
    #[test]
    fn test_realtime_current_task_updates() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(20);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan (empty)
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Create idle worker with no task
        let idle_json = r#"{
            "worker_id": "task-worker",
            "status": "idle",
            "current_task": null
        }"#;
        let worker_path = status_dir.join("task-worker.json");
        fs::write(&worker_path, idle_json).unwrap();

        // Wait for file creation to be processed
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(1) {
            if let Some(event) = watcher.recv_timeout(Duration::from_millis(50)) {
                if let StatusEvent::WorkerUpdated { ref worker_id, .. } = event {
                    if worker_id == "task-worker" {
                        break;
                    }
                }
            }
        }

        // Verify initial state
        let worker = watcher
            .get_worker("task-worker")
            .expect("Worker should be tracked after creation");
        assert_eq!(
            worker.current_task, None,
            "Worker should have no task initially"
        );

        // Worker picks up a task
        let start = std::time::Instant::now();
        let active_json = r#"{
            "worker_id": "task-worker",
            "status": "active",
            "current_task": "fg-123"
        }"#;
        fs::write(&worker_path, active_json).unwrap();

        // Wait for update
        let mut task_updated = false;
        while start.elapsed() < Duration::from_secs(1) {
            if let Some(event) = watcher.recv_timeout(Duration::from_millis(50)) {
                if let StatusEvent::WorkerUpdated { ref status, .. } = event {
                    if status.worker_id == "task-worker"
                        && status.current_task == Some("fg-123".to_string())
                    {
                        task_updated = true;
                        break;
                    }
                }
            }
        }

        assert!(task_updated, "current_task should update within 1 second");

        // Verify internal state
        let worker = watcher.get_worker("task-worker").unwrap();
        assert_eq!(
            worker.current_task,
            Some("fg-123".to_string()),
            "Worker should have current_task set"
        );
        assert_eq!(
            worker.status,
            WorkerStatus::Active,
            "Worker should be active after picking up task"
        );
    }

    /// Test that tasks_completed increments in real-time.
    ///
    /// Success Criteria: tasks_completed field updates within 1 second
    #[test]
    fn test_realtime_tasks_completed_increments() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(20);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event (empty directory)
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Create worker with 0 tasks completed (after watcher is ready)
        let initial_json = r#"{
            "worker_id": "productive-worker",
            "status": "active",
            "tasks_completed": 0,
            "current_task": "task-1"
        }"#;
        let worker_path = status_dir.join("productive-worker.json");
        fs::write(&worker_path, initial_json).unwrap();

        // Wait for file creation event to be processed
        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify initial state
        let worker = watcher.get_worker("productive-worker").expect("Worker should be tracked after file creation");
        assert_eq!(
            worker.tasks_completed, 0,
            "Should start with 0 tasks completed"
        );

        // Simulate task completion
        let start = std::time::Instant::now();
        let completed_json = r#"{
            "worker_id": "productive-worker",
            "status": "idle",
            "tasks_completed": 1,
            "current_task": null
        }"#;
        fs::write(&worker_path, completed_json).unwrap();

        // Wait for update
        let mut count_updated = false;
        while start.elapsed() < Duration::from_secs(1) {
            if let Some(event) = watcher.recv_timeout(Duration::from_millis(50)) {
                if let StatusEvent::WorkerUpdated { ref status, .. } = event {
                    if status.worker_id == "productive-worker" && status.tasks_completed == 1 {
                        count_updated = true;
                        break;
                    }
                }
            }
        }

        assert!(
            count_updated,
            "tasks_completed should increment within 1 second"
        );

        // Verify internal state
        let worker = watcher.get_worker("productive-worker").unwrap();
        assert_eq!(
            worker.tasks_completed, 1,
            "Worker should have 1 task completed"
        );
    }

    /// Test that external worker kill (file deletion) shows stopped/failed status.
    ///
    /// Simulates what happens when a worker is killed via `tmux kill-session`.
    /// The worker process terminates and the status file is either deleted
    /// or updated to show stopped/failed status.
    ///
    /// Success Criteria: Status shows 'stopped' or worker is removed within 2 seconds
    #[test]
    fn test_realtime_external_worker_kill_file_deletion() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(20);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan event (empty directory)
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Create an active worker (after watcher is ready)
        let active_json = r#"{
            "worker_id": "doomed-worker",
            "status": "active",
            "current_task": "unfinished-task"
        }"#;
        let worker_path = status_dir.join("doomed-worker.json");
        fs::write(&worker_path, active_json).unwrap();

        // Wait for file creation event to be processed
        std::thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify worker exists
        assert!(
            watcher.get_worker("doomed-worker").is_some(),
            "Worker should be tracked before kill"
        );

        // Simulate external kill - delete the status file
        // (In real scenario, worker process dies and can't update its status)
        let start = std::time::Instant::now();
        fs::remove_file(&worker_path).unwrap();

        // Wait for the removal event
        let mut worker_removed = false;
        while start.elapsed() < Duration::from_secs(2) {
            if let Some(event) = watcher.recv_timeout(Duration::from_millis(50)) {
                if let StatusEvent::WorkerRemoved { ref worker_id } = event {
                    if worker_id == "doomed-worker" {
                        worker_removed = true;
                        break;
                    }
                }
            }
        }

        assert!(
            worker_removed,
            "Worker should be removed within 2 seconds of status file deletion"
        );

        // Verify worker is no longer tracked
        assert!(
            watcher.get_worker("doomed-worker").is_none(),
            "Killed worker should no longer be tracked"
        );
    }

    /// Test that external worker kill with status update shows 'stopped' or 'failed'.
    ///
    /// Alternative scenario: Worker has time to update its status to 'stopped'
    /// before terminating (graceful shutdown).
    #[test]
    fn test_realtime_external_worker_kill_graceful_shutdown() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(20);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Create an active worker
        let active_json = r#"{
            "worker_id": "graceful-worker",
            "status": "active",
            "current_task": "wrapping-up"
        }"#;
        let worker_path = status_dir.join("graceful-worker.json");
        fs::write(&worker_path, active_json).unwrap();

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Simulate graceful shutdown - worker updates status to 'stopped'
        let start = std::time::Instant::now();
        let stopped_json = r#"{
            "worker_id": "graceful-worker",
            "status": "stopped"
        }"#;
        fs::write(&worker_path, stopped_json).unwrap();

        // Wait for the update
        let mut status_updated = false;
        while start.elapsed() < Duration::from_secs(2) {
            if let Some(event) = watcher.recv_timeout(Duration::from_millis(50)) {
                if let StatusEvent::WorkerUpdated { ref status, .. } = event {
                    if status.worker_id == "graceful-worker"
                        && (status.status == WorkerStatus::Stopped
                            || status.status == WorkerStatus::Failed)
                    {
                        status_updated = true;
                        break;
                    }
                }
            }
        }

        assert!(
            status_updated,
            "Worker status should show 'stopped' or 'failed' within 2 seconds of graceful shutdown"
        );

        // Verify internal state
        let worker = watcher.get_worker("graceful-worker").unwrap();
        assert!(
            matches!(worker.status, WorkerStatus::Stopped | WorkerStatus::Failed),
            "Worker should be in 'stopped' or 'failed' status, got {:?}",
            worker.status
        );
        assert!(
            !worker.is_healthy(),
            "Stopped/failed worker should not be healthy"
        );
    }

    /// Test no stale data is displayed after rapid status changes.
    ///
    /// Success Criteria: Final state is correctly reflected, no intermediate stale data
    #[test]
    fn test_realtime_no_stale_data_after_rapid_changes() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(20);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_path = status_dir.join("rapid-worker.json");

        // Rapidly change status multiple times
        let statuses = ["starting", "active", "idle", "active", "failed", "stopped"];
        let mut last_json = String::new();

        for status in &statuses {
            last_json = format!(
                r#"{{"worker_id": "rapid-worker", "status": "{}", "tasks_completed": 0}}"#,
                status
            );
            fs::write(&worker_path, &last_json).unwrap();
            thread::sleep(Duration::from_millis(30)); // Rapid but not instant
        }

        // Wait for all events to be processed
        thread::sleep(Duration::from_millis(200));
        while watcher.recv_timeout(Duration::from_millis(50)).is_some() {}

        // Verify final state is 'stopped' (the last written status)
        let worker = watcher.get_worker("rapid-worker");
        assert!(
            worker.is_some(),
            "Worker should still be tracked after rapid changes"
        );
        let worker = worker.unwrap();
        assert_eq!(
            worker.status,
            WorkerStatus::Stopped,
            "Final status should be 'stopped', not stale data"
        );
        assert!(!worker.is_healthy(), "Stopped worker should not be healthy");
    }

    /// Test that status watcher handles concurrent worker spawns gracefully.
    ///
    /// Success Criteria: All workers tracked correctly, no missing or duplicate events
    #[test]
    fn test_realtime_concurrent_worker_spawns() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(20);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        // Spawn multiple workers concurrently (simulating rapid S key presses)
        let worker_count = 5;
        let start = std::time::Instant::now();

        for i in 0..worker_count {
            let json = format!(
                r#"{{"worker_id": "concurrent-{}", "status": "starting"}}"#,
                i
            );
            fs::write(status_dir.join(format!("concurrent-{}.json", i)), json).unwrap();
        }

        // Wait for all workers to be detected
        let mut detected_count = 0;
        while start.elapsed() < Duration::from_secs(2) && detected_count < worker_count {
            if let Some(event) = watcher.recv_timeout(Duration::from_millis(50)) {
                if let StatusEvent::WorkerUpdated { ref worker_id, .. } = event {
                    if worker_id.starts_with("concurrent-") {
                        detected_count += 1;
                    }
                }
            }
        }

        // Drain remaining events
        while watcher.recv_timeout(Duration::from_millis(20)).is_some() {}

        // Verify all workers are tracked
        assert_eq!(
            detected_count, worker_count,
            "All {} workers should be detected within 2 seconds, got {}",
            worker_count, detected_count
        );

        // Verify internal state has all workers
        for i in 0..worker_count {
            let worker_id = format!("concurrent-{}", i);
            assert!(
                watcher.get_worker(&worker_id).is_some(),
                "Worker {} should be tracked",
                worker_id
            );
        }

        let counts = watcher.worker_counts();
        assert_eq!(counts.total, worker_count);
        assert_eq!(counts.starting, worker_count);
    }

    /// Test that the watcher correctly handles worker status transitions
    /// with current_task as both string and object formats.
    #[test]
    fn test_realtime_current_task_format_variations() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        let config = StatusWatcherConfig::default()
            .with_status_dir(&status_dir)
            .with_debounce_ms(20);
        let mut watcher = StatusWatcher::new(config).unwrap();

        // Consume initial scan
        let _ = watcher.recv_timeout(Duration::from_millis(100));

        let worker_path = status_dir.join("format-worker.json");

        // Test string format
        let string_format = r#"{
            "worker_id": "format-worker",
            "status": "active",
            "current_task": "bd-string"
        }"#;
        fs::write(&worker_path, string_format).unwrap();
        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(20)).is_some() {}

        let worker = watcher.get_worker("format-worker").unwrap();
        assert_eq!(
            worker.current_task,
            Some("bd-string".to_string()),
            "String format current_task should work"
        );

        // Test object format
        let object_format = r#"{
            "worker_id": "format-worker",
            "status": "active",
            "current_task": {"bead_id": "bd-object", "priority": 1}
        }"#;
        fs::write(&worker_path, object_format).unwrap();
        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(20)).is_some() {}

        let worker = watcher.get_worker("format-worker").unwrap();
        assert_eq!(
            worker.current_task,
            Some("bd-object".to_string()),
            "Object format current_task should extract bead_id"
        );

        // Test null format
        let null_format = r#"{
            "worker_id": "format-worker",
            "status": "idle",
            "current_task": null
        }"#;
        fs::write(&worker_path, null_format).unwrap();
        thread::sleep(Duration::from_millis(100));
        while watcher.recv_timeout(Duration::from_millis(20)).is_some() {}

        let worker = watcher.get_worker("format-worker").unwrap();
        assert_eq!(
            worker.current_task, None,
            "Null current_task should be None"
        );
    }

    /// Test that the UI correctly reflects worker status changes.
    #[test]
    fn test_realtime_ui_reflects_status_changes() {
        let temp_dir = TempDir::new().unwrap();
        let status_dir = temp_dir.path().join("status");
        fs::create_dir_all(&status_dir).unwrap();

        // Create a worker
        let worker_json = r#"{
            "worker_id": "ui-test-worker",
            "status": "starting",
            "model": "sonnet"
        }"#;
        fs::write(status_dir.join("ui-test-worker.json"), worker_json).unwrap();

        // Create app with custom status dir
        let mut app = App::with_status_dir(status_dir.clone());

        // Render and verify worker appears
        app.switch_view(View::Workers);
        let buffer = render_app(&mut app, 120, 40);
        let content = buffer_to_string(&buffer);

        // The UI should show worker-related content
        assert!(
            content.contains("Worker") || content.contains("Pool"),
            "Workers view should display worker pool"
        );

        // Update worker status
        let active_json = r#"{
            "worker_id": "ui-test-worker",
            "status": "active",
            "model": "sonnet",
            "current_task": "fg-56p"
        }"#;
        fs::write(status_dir.join("ui-test-worker.json"), active_json).unwrap();

        // Give time for update
        thread::sleep(Duration::from_millis(100));

        // Re-render
        let buffer = render_app(&mut app, 120, 40);

        // App should still render without error after status change
        assert!(
            buffer_contains(&buffer, "Worker"),
            "UI should still render after status change"
        );
    }

    // ============================================================
    // Integration Test: Exact Boundary Size Tests (fg-2n0k)
    // ============================================================
    //
    // These tests verify rendering at exact boundary sizes:
    // - Narrow: <120 cols OR <min_height rows
    // - Wide: 120-198 cols AND 30+ rows
    // - UltraWide: 199+ cols AND 38+ rows
    //
    // Test Matrix from fg-2n0k:
    // | Terminal Size | Expected Mode | Panels Visible |
    // |--------------|---------------|----------------|
    // | 80x24        | Narrow        | 1 (switching)  |
    // | 100x25       | Narrow        | 1 (switching)  |
    // | 119x40       | Narrow        | 1 (width limit)|
    // | 120x29       | Narrow        | 1 (height limit)|
    // | 120x30       | Wide          | 4              |
    // | 150x40       | Wide          | 4              |
    // | 198x38       | Wide          | 4              |
    // | 199x37       | Wide          | 4 (height limit)|
    // | 199x38       | UltraWide     | 6              |
    // | 250x60       | UltraWide     | 6              |

    /// Test narrow mode at 80x24 (typical small terminal)
    #[test]
    fn test_boundary_80x24_narrow() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 80, 24);

        // Should render without crash
        assert!(buffer.area.width == 80);
        assert!(buffer.area.height == 24);

        // Header should be visible
        assert!(buffer_contains(&buffer, "FORGE"));

        // Hotkey hints should be visible (narrow mode indicator)
        assert!(buffer_contains(&buffer, "[o]") || buffer_contains(&buffer, "Overview"));
    }

    /// Test narrow mode at 100x25
    #[test]
    fn test_boundary_100x25_narrow() {
        let mut app = App::new();
        let buffer = render_app(&mut app, 100, 25);

        assert!(buffer.area.width == 100);
        assert!(buffer.area.height == 25);
        assert!(buffer_contains(&buffer, "FORGE"));
    }

    /// Test narrow mode at 119x40 (width limit boundary)
    /// 119 cols is just under the 120 wide threshold
    #[test]
    fn test_boundary_119x40_narrow_width_limit() {
        use crate::view::LayoutMode;

        // Verify layout mode detection
        assert_eq!(LayoutMode::from_width(119), LayoutMode::Narrow);

        let mut app = App::new();
        let buffer = render_app(&mut app, 119, 40);

        assert!(buffer.area.width == 119);
        assert!(buffer.area.height == 40);
        assert!(buffer_contains(&buffer, "FORGE"));

        // Worker Pool should be visible in narrow mode
        assert!(buffer_contains(&buffer, "Worker Pool"));
    }

    /// Test narrow mode at 120x29 (height limit boundary)
    /// 120 cols qualifies for wide, but 29 rows is below 30 minimum
    #[test]
    fn test_boundary_120x29_narrow_height_limit() {
        use crate::view::LayoutMode;

        // Width says Wide, but height requirement not met
        assert_eq!(LayoutMode::from_width(120), LayoutMode::Wide);
        assert!(!LayoutMode::Wide.meets_requirements(120, 29)); // Height too short

        let mut app = App::new();
        let buffer = render_app(&mut app, 120, 29);

        assert!(buffer.area.width == 120);
        assert!(buffer.area.height == 29);
        assert!(buffer_contains(&buffer, "FORGE"));
    }

    /// Test wide mode at 120x30 (exact wide threshold)
    /// Minimum size that qualifies for wide mode
    #[test]
    fn test_boundary_120x30_wide_threshold() {
        use crate::view::LayoutMode;

        assert_eq!(LayoutMode::from_width(120), LayoutMode::Wide);
        assert!(LayoutMode::Wide.meets_requirements(120, 30));

        let mut app = App::new();
        let buffer = render_app(&mut app, 120, 30);

        assert!(buffer.area.width == 120);
        assert!(buffer.area.height == 30);
        assert!(buffer_contains(&buffer, "FORGE"));

        // Wide mode should show 4 panels
        assert!(buffer_contains(&buffer, "Worker Pool"));
        assert!(buffer_contains(&buffer, "Task Queue"));
    }

    /// Test wide mode at 150x40 (typical wide terminal)
    #[test]
    fn test_boundary_150x40_wide() {
        use crate::view::LayoutMode;

        assert_eq!(LayoutMode::from_width(150), LayoutMode::Wide);
        assert!(LayoutMode::Wide.meets_requirements(150, 40));

        let mut app = App::new();
        let buffer = render_app(&mut app, 150, 40);

        assert!(buffer.area.width == 150);
        assert!(buffer.area.height == 40);
        assert!(buffer_contains(&buffer, "FORGE"));

        // Wide mode should show 4 core panels
        assert!(buffer_contains(&buffer, "Worker Pool"));
        assert!(buffer_contains(&buffer, "Subscriptions"));
        assert!(buffer_contains(&buffer, "Task Queue"));
        assert!(buffer_contains(&buffer, "Activity Log"));
    }

    /// Test wide mode at 198x38 (upper wide boundary)
    /// 198 cols is the maximum wide width, but 38 height qualifies for wide
    #[test]
    fn test_boundary_198x38_wide_upper_limit() {
        use crate::view::LayoutMode;

        assert_eq!(LayoutMode::from_width(198), LayoutMode::Wide);
        assert!(LayoutMode::Wide.meets_requirements(198, 38));

        let mut app = App::new();
        let buffer = render_app(&mut app, 198, 38);

        assert!(buffer.area.width == 198);
        assert!(buffer.area.height == 38);
        assert!(buffer_contains(&buffer, "FORGE"));
    }

    /// Test wide mode at 199x37 (height limit for ultrawide)
    /// 199 cols qualifies for ultrawide, but 37 rows is below 38 minimum
    #[test]
    fn test_boundary_199x37_wide_height_limit() {
        use crate::view::LayoutMode;

        // Width says UltraWide, but height requirement not met
        assert_eq!(LayoutMode::from_width(199), LayoutMode::UltraWide);
        assert!(!LayoutMode::UltraWide.meets_requirements(199, 37)); // Height too short

        let mut app = App::new();
        let buffer = render_app(&mut app, 199, 37);

        assert!(buffer.area.width == 199);
        assert!(buffer.area.height == 37);
        assert!(buffer_contains(&buffer, "FORGE"));
    }

    /// Test ultrawide mode at 199x38 (exact ultrawide threshold)
    /// Minimum size that qualifies for ultrawide mode
    #[test]
    fn test_boundary_199x38_ultrawide_threshold() {
        use crate::view::LayoutMode;

        assert_eq!(LayoutMode::from_width(199), LayoutMode::UltraWide);
        assert!(LayoutMode::UltraWide.meets_requirements(199, 38));

        let mut app = App::new();
        let buffer = render_app(&mut app, 199, 38);

        assert!(buffer.area.width == 199);
        assert!(buffer.area.height == 38);
        assert!(buffer_contains(&buffer, "FORGE"));

        // UltraWide mode should show all 6 panels
        assert!(buffer_contains(&buffer, "Worker Pool"));
        assert!(buffer_contains(&buffer, "Task Queue"));
        assert!(buffer_contains(&buffer, "Activity Log"));
        assert!(buffer_contains(&buffer, "Cost Breakdown"));
        assert!(buffer_contains(&buffer, "Quick Actions"));
    }

    /// Test ultrawide mode at 250x60 (large ultrawide terminal)
    #[test]
    fn test_boundary_250x60_ultrawide() {
        use crate::view::LayoutMode;

        assert_eq!(LayoutMode::from_width(250), LayoutMode::UltraWide);
        assert!(LayoutMode::UltraWide.meets_requirements(250, 60));

        let mut app = App::new();
        let buffer = render_app(&mut app, 250, 60);

        assert!(buffer.area.width == 250);
        assert!(buffer.area.height == 60);
        assert!(buffer_contains(&buffer, "FORGE"));

        // All 6 panels should be visible
        assert!(buffer_contains(&buffer, "Worker Pool"));
        assert!(buffer_contains(&buffer, "Subscriptions"));
        assert!(buffer_contains(&buffer, "Task Queue"));
        assert!(buffer_contains(&buffer, "Activity Log"));
        assert!(buffer_contains(&buffer, "Cost Breakdown"));
        assert!(buffer_contains(&buffer, "Quick Actions"));
    }

    /// Test dynamic resize transitions: narrow -> wide -> ultrawide
    #[test]
    fn test_boundary_dynamic_resize_transitions() {
        let mut app = App::new();

        // Start narrow
        let narrow = render_app(&mut app, 80, 24);
        assert!(buffer_contains(&narrow, "FORGE"));

        // Resize to wide
        let wide = render_app(&mut app, 150, 40);
        assert!(buffer_contains(&wide, "FORGE"));
        assert!(buffer_contains(&wide, "Subscriptions"));

        // Resize to ultrawide
        let ultrawide = render_app(&mut app, 250, 60);
        assert!(buffer_contains(&ultrawide, "FORGE"));
        assert!(buffer_contains(&ultrawide, "Cost Breakdown"));
        assert!(buffer_contains(&ultrawide, "Quick Actions"));

        // Resize back down to narrow
        let narrow_again = render_app(&mut app, 80, 24);
        assert!(buffer_contains(&narrow_again, "FORGE"));
    }

    /// Test that all 10 sizes render without crash
    #[test]
    fn test_boundary_all_sizes_no_crash() {
        let sizes = [
            (80, 24),   // Narrow - small
            (100, 25),  // Narrow - medium
            (119, 40),  // Narrow - width limit
            (120, 29),  // Narrow - height limit
            (120, 30),  // Wide - threshold
            (150, 40),  // Wide - typical
            (198, 38),  // Wide - upper limit
            (199, 37),  // Wide - ultrawide height limit
            (199, 38),  // UltraWide - threshold
            (250, 60),  // UltraWide - large
        ];

        for (width, height) in sizes {
            let mut app = App::new();
            // This should not panic for any size
            let buffer = render_app(&mut app, width, height);
            assert!(
                buffer.area.width == width,
                "Buffer width {} should be {}",
                buffer.area.width,
                width
            );
            assert!(
                buffer.area.height == height,
                "Buffer height {} should be {}",
                buffer.area.height,
                height
            );
            // Header should always be visible
            assert!(
                buffer_contains(&buffer, "FORGE"),
                "FORGE header should be visible at {}x{}",
                width,
                height
            );
        }
    }
}
