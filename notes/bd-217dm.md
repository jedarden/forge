# Bead bd-217dm: Wire forge-server integration

## Task
Wire forge-server integration: server/client CLI modes not connected to TUI app loop

## Implementation Summary

The forge-server integration wiring was completed in two commits:

### 1. Server Mode (commit ca88007)
- `run_server_mode()` spawns server in separate thread with own Tokio runtime
- TUI connects as client to local server via `App::run_with_client()`
- Server runs in background while TUI polls for messages via `poll_server_client_messages()`

### 2. Client Mode (commit 10b803d)
- `run_client_mode()` connects to remote server via `App::run_with_client()`
- WebSocket client runs in background task
- TUI main loop polls for incoming messages via mpsc channels
- State updates forwarded to DataManager for UI synchronization

## Wiring Path

**Server Mode:**
```
--server flag → run_server_mode() → App::run_with_client() → run_loop_with_client() → poll_server_client_messages()
```

**Client Mode:**
```
--connect flag → run_client_mode() → App::run_with_client() → run_loop_with_client() → poll_server_client_messages()
```

## Key Integration Points

1. **main.rs (lines 310-321)**: CLI mode detection and routing
2. **main.rs (lines 1143-1225)**: `run_server_mode()` implementation
3. **main.rs (lines 1228-1254)**: `run_client_mode()` implementation
4. **app.rs (line 4144)**: `run_with_client()` entry point
5. **app.rs (line 4338)**: `run_loop_with_client()` event loop
6. **app.rs (line 4353)**: `poll_server_client_messages()` call
7. **app.rs (line 4453)**: `poll_server_client_messages()` implementation
8. **app.rs (line 4477)**: `handle_server_client_message()` implementation

## Verification

- Build succeeds: `cargo build --release`
- CLI flags available: `--server`, `--connect`, `--user`, `--password`
- TUI app loop properly polls server messages in both modes
- Background WebSocket client task communicates via mpsc channels
- Server state updates flow to DataManager for UI sync

## Status

✅ Complete - Server and client CLI modes are fully wired to TUI app loop with message polling and state synchronization.
