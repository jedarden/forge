# bd-217dm: Forge-Server Integration Verification

## Summary

Verified that the forge-server integration is complete and wired correctly.

## Implementation

### Server Mode (`--server`)
- Located in: `src/main.rs::run_server_mode()`
- Spawns WebSocket server in background thread with separate Tokio runtime
- TUI runs as client connected to local server
- Avoids nested runtime issues between server and TUI

### Client Mode (`--connect URL`)
- Located in: `src/main.rs::run_client_mode()`
- TUI connects to remote FORGE server
- Uses same `App::run_with_client()` as server mode

### TUI Integration
- `App::run_with_client()` (app.rs:4144)
  - Creates Tokio runtime for WebSocket client
  - Spawns background task `run_client_background_task()`
  - Runs `run_loop_with_client()` main loop

- `run_loop_with_client()` (app.rs:4338)
  - Calls `poll_server_client_messages()` each frame
  - Handles server state updates in real-time

- Message handling:
  - `poll_server_client_messages()` (app.rs:4453)
  - `handle_server_client_message()` (app.rs:4477)
  - Handles: Connected, StateUpdate, UserJoined, UserLeft, BeadAssigned, ChatMessage, Error

## Status

Integration is complete and functional. No additional wiring required.
