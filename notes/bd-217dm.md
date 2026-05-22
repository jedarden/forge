# bd-217dm: Forge-Server Integration Complete

## Task
Wire forge-server integration: server/client CLI modes not connected to TUI app loop

## Verification Summary

The forge-server integration was verified as complete and functional. All CLI modes are properly wired to the TUI app loop.

## Integration Points Verified

### 1. CLI Mode Detection (src/main.rs:310-321)
```rust
if cli.server {
    return run_server_mode(cli.server_bind, cli.server_port);
}
if let Some(server_url) = &cli.connect {
    return run_client_mode(server_url.clone(), ...);
}
```

### 2. Server Mode (src/main.rs:1143-1225)
- Spawns WebSocket server in background thread with separate Tokio runtime
- TUI connects as client to local server via `App::run_with_client()`
- Default users: admin/admin123, operator/operator123, viewer/viewer123

### 3. Client Mode (src/main.rs:1228-1254)
- Connects to remote FORGE server via WebSocket
- Uses same `App::run_with_client()` entry point

### 4. TUI Integration (crates/forge-tui/src/app.rs)
- `App::run_with_client()` (line 4144)
  - Creates Tokio runtime for async WebSocket client
  - Spawns `run_client_background_task()` for WebSocket communication
  - Runs `run_loop_with_client()` main event loop

- `run_loop_with_client()` (line 4338)
  - Polls `poll_server_client_messages()` each frame for server updates

- `run_inner()` (line 4544) - normal mode
  - Also polls server messages when `server_client_rx.is_some()`
  - Added in commit a6ef940

### 5. Message Flow
```
Server → WebSocket → ForgeClient → state_rx → ServerClientMessage → TUI
TUI → ServerClientRequest → ForgeClient.send_direct() → WebSocket → Server
```

## Status
✅ Complete - Integration verified working. Build succeeds. No additional wiring required.
