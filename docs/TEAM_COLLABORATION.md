# Team Collaboration Features

FORGE supports multi-user collaboration through server mode, allowing multiple users to observe and interact with the same FORGE instance.

## Overview

Team collaboration features enable:
- Multiple users can observe the same FORGE instance
- Role-based access (viewer vs operator vs admin)
- Named user sessions with attribution on actions
- Shared bead queue with assignment

## Architecture

### Server Mode

When FORGE runs in server mode, it hosts a WebSocket server that clients can connect to for real-time state synchronization.

```
┌─────────────────────────────────────────────────────────┐
│  FORGE Server (Host)                                     │
│  - WebSocket server (ws://host:8080/ws)                 │
│  - Session management                                   │
│  - Bead assignment tracking                            │
│  - State broadcast to all clients                      │
└──────────────┬──────────────────────────────────────────┘
               │
               ↓ WebSocket
┌─────────────────────────────────────────────────────────┐
│  Clients (Multiple)                                      │
│  - Viewer: Read-only observation                        │
│  - Operator: Can spawn/kill workers, assign tasks       │
│  - Admin: Full access including config changes          │
└─────────────────────────────────────────────────────────┘
```

## Usage

### Starting the Server

Start FORGE in server mode:

```bash
forge --server
```

The server will start on `127.0.0.1:8080` by default. You can configure the bind address and port in `~/.forge/config.yaml`:

```yaml
server:
  enabled: true
  bind_address: "0.0.0.0"
  port: 8080
```

### Connecting as a Client

Connect to a running FORGE server:

```bash
forge --connect ws://server-host:8080/ws
```

You can also configure connection settings in `~/.forge/config.yaml`:

```yaml
server:
  mode: "client"
  remote_url: "ws://server-host:8080/ws"
  auth_user: "username"
  auth_password: "password"
```

## Default Users

The server comes with three default users for development:

| Username | Password | Role    | Permissions                         |
|----------|----------|---------|-------------------------------------|
| admin    | admin123 | Admin   | Full access including user management |
| operator | operator123 | Operator | Spawn/kill workers, assign tasks  |
| viewer   | viewer123 | Viewer  | Read-only observation               |

## Roles and Permissions

### Viewer
- View all workers, tasks, and metrics
- No ability to modify state
- Ideal for observation-only access

### Operator
- All Viewer permissions
- Spawn and kill workers
- Assign and unassign tasks
- Modify task status
- Cannot modify configuration or manage users

### Admin
- All Operator permissions
- Modify configuration
- Manage users
- Full system access

## Features

### Sessions View

Press `s` to view the Sessions panel, which shows:
- Connected users
- User roles (color-coded)
- Current view being observed by each user
- Connection status

### Bead Assignment

Operators and Admins can assign beads to specific users:
- Assign bead from Tasks view
- Attribution tracked (who assigned to whom)
- Unassign beads to return to pool

### Real-time Updates

All clients receive real-time updates for:
- Worker status changes
- Bead status changes
- Bead assignments
- User join/leave events
- Cost updates

### Action Attribution

Actions performed by users are attributed in:
- Audit logs (who did what when)
- Bead assignments (assigned by/to tracking)
- Worker spawn/kill operations

## Configuration

### Server Configuration

```yaml
server:
  # Enable server mode
  enabled: true

  # Bind address (0.0.0.0 for all interfaces)
  bind_address: "127.0.0.1"

  # Port for WebSocket server
  port: 8080
```

### Client Configuration

```yaml
server:
  # Client mode settings
  mode: "client"
  remote_url: "ws://server-host:8080/ws"

  # Authentication credentials
  auth_user: "username"
  auth_password: "password"
```

## Security Considerations

### Production Deployment

For production use:
1. **Replace SimpleAuth**: The default auth provider uses hardcoded passwords. Integrate with your existing auth system (OAuth, LDAP, etc.)
2. **Use TLS/WSS**: Encrypt WebSocket connections in production
3. **Network Security**: Use firewall rules to restrict access
4. **Audit Logging**: Ensure audit logs are enabled for compliance

### Authentication Provider

The `SimpleAuth` provider is for development only. Implement the `AuthProvider` trait for production:

```rust
#[async_trait]
pub trait AuthProvider: Send + Sync {
    async fn authenticate(&self, user_id: &str, credentials: &str)
        -> Result<AuthResult, ServerError>;
}
```

## API Reference

### Server Messages

Messages sent from server to clients:

- `Welcome`: Session info and server state
- `StateUpdate`: Full state snapshot
- `UserJoined`: New user connected
- `UserLeft`: User disconnected
- `BeadAssigned`: Bead assigned to user
- `WorkerChanged`: Worker status changed
- `BeadChanged`: Bead status changed
- `ChatMessage`: Chat message from another user
- `Error`: Error occurred
- `Ping`: Keep-alive ping

### Client Messages

Messages sent from client to server:

- `Authenticate`: Authentication credentials
- `SyncState`: Request full state sync
- `AssignBead`: Assign bead to user
- `UnassignBead`: Unassign bead
- `SpawnWorker`: Spawn new worker
- `KillWorker`: Kill a worker
- `ChangeBeadStatus`: Modify bead status
- `ChatMessage`: Send chat message
- `UpdateView`: Update current view
- `Pong`: Ping response

## Troubleshooting

### Connection Refused
- Verify server is running: `forge --server`
- Check firewall rules
- Verify correct host/port

### Authentication Failed
- Verify username/password
- Check user role permissions
- Ensure user exists in auth provider

### State Not Syncing
- Check WebSocket connection is active
- Verify client is subscribed to state updates
- Check server logs for errors

## Development

### Running Tests

```bash
cargo test --package forge-server --test team_collaboration_test
```

### Code Organization

- `crates/forge-server/src/lib.rs` - Main server exports
- `crates/forge-server/src/auth.rs` - Authentication and authorization
- `crates/forge-server/src/session.rs` - Session management
- `crates/forge-server/src/assignment.rs` - Bead assignment tracking
- `crates/forge-server/src/protocol.rs` - WebSocket protocol messages
- `crates/forge-server/src/websocket.rs` - WebSocket server implementation
- `crates/forge-server/src/client.rs` - WebSocket client implementation
- `crates/forge-tui/src/sessions_panel.rs` - TUI Sessions view

## Future Enhancements

Potential improvements for team collaboration:

1. **Persistent Auth**: Integration with OAuth, SAML, LDAP
2. **Encrypted Connections**: TLS/WSS support
3. **Chat System**: Built-in team chat within FORGE
4. **Presence Indicators**: Show what each user is viewing/working on
5. **Session Recording**: Record and replay sessions for training
6. **Multi-Server Federation**: Connect multiple FORGE instances

---

**FORGE** - Federated Orchestration & Resource Generation Engine
