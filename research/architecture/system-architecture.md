# Control Panel System Architecture

## 1. High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Control Panel Dashboard                      │
│                              (TUI Interface)                         │
└─────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────┐
│                          Orchestrator Layer                          │
├─────────────────────────────────────────────────────────────────────┤
│  • Task Scheduler (Dependency-Aware)                                │
│  • Worker Pool Manager                                              │
│  • Lock Manager (Bead + File Level)                                 │
│  • Model Assignment Engine                                          │
└─────────────────────────────────────────────────────────────────────┘
                                    │
                  ┌─────────────────┼─────────────────┐
                  ▼                 ▼                 ▼
        ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
        │   Worker 1   │  │   Worker 2   │  │   Worker N   │
        │  (Claude     │  │  (GLM-4)     │  │  (Sonnet)    │
        │   Opus)      │  │              │  │              │
        └──────────────┘  └──────────────┘  └──────────────┘
                  │                 │                 │
                  └─────────────────┼─────────────────┘
                                    ▼
        ┌───────────────────────────────────────────────────┐
        │            Health Monitoring System                │
        ├───────────────────────────────────────────────────┤
        │  • Liveness Checker                               │
        │  • Activity Monitor                               │
        │  • Resource Tracker                               │
        │  • Error Rate Monitor                             │
        │  • Alert Manager                                  │
        └───────────────────────────────────────────────────┘
                                    │
                  ┌─────────────────┼─────────────────┐
                  ▼                 ▼                 ▼
        ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
        │   Recovery   │  │   Circuit    │  │   Fallback   │
        │   Manager    │  │   Breaker    │  │   Chain      │
        └──────────────┘  └──────────────┘  └──────────────┘
                                    │
                                    ▼
        ┌───────────────────────────────────────────────────┐
        │              Data Persistence Layer               │
        ├───────────────────────────────────────────────────┤
        │  • Beads DB (SQLite/JSONL)                        │
        │  • Lock DB (SQLite/Redis/etcd)                    │
        │  • Metrics DB (TimescaleDB/InfluxDB)              │
        │  • Event Log (Structured Logging)                 │
        └───────────────────────────────────────────────────┘
```

## 2. Component Interaction Flow

### 2.1 Normal Operation Flow

```
User Request
    │
    ▼
[Dashboard] Displays available beads
    │
    ▼
[Scheduler] Analyzes dependencies and priorities
    │
    ├─→ [Deadlock Detector] Check for cycles
    ├─→ [File Predictor] Predict file conflicts
    └─→ [Resource Estimator] Calculate requirements
    │
    ▼
[Worker Assignment] Match beads to workers
    │
    ├─→ Model capability matching
    ├─→ Cost optimization
    ├─→ Load balancing
    └─→ Historical performance
    │
    ▼
[Lock Manager] Acquire bead + file locks
    │
    ├─→ Check existing locks
    ├─→ Detect conflicts
    └─→ Grant lock if available
    │
    ▼
[Worker] Execute bead
    │
    ├─→ Make LLM API calls
    ├─→ Modify files
    ├─→ Commit changes
    └─→ Report progress
    │
    ▼
[Health Monitor] Track worker health
    │
    ├─→ Heartbeat checks
    ├─→ Activity monitoring
    ├─→ Resource tracking
    └─→ Error rate analysis
    │
    ▼
[Completion] Release locks, update metrics
    │
    ▼
[Scheduler] Assign next bead
```

### 2.2 Failure Recovery Flow

```
[Health Monitor] Detects worker failure
    │
    ▼
[Alert Manager] Trigger alert
    │
    ├─→ Log event
    ├─→ Notify via Slack/Email
    └─→ Update dashboard
    │
    ▼
[Recovery Manager] Analyze failure
    │
    ├─→ Classify failure type
    ├─→ Check retry count
    └─→ Determine recovery strategy
    │
    ▼
Decision: Can auto-recover?
    │
    ├─ YES ─→ [Backoff Manager] Calculate wait time
    │             │
    │             ▼
    │         [Lock Cleanup] Release held locks
    │             │
    │             ▼
    │         [State Capture] Save worker state
    │             │
    │             ▼
    │         Wait (exponential backoff)
    │             │
    │             ▼
    │         Decision: Should fallback?
    │             │
    │             ├─ YES ─→ [Fallback Manager] Switch model
    │             │             │
    │             │             ▼
    │             │         Create worker with fallback model
    │             │             │
    │             │             └─→ [Worker] Resume with state
    │             │
    │             └─ NO ──→ [Worker] Restart same model
    │
    └─ NO ──→ [Human Escalation] Manual intervention required
                  │
                  └─→ Mark bead as blocked
```

## 3. Lock Management Architecture

### 3.1 Lock Hierarchy

```
Workspace
    │
    ├─ Bead Locks (Exclusive)
    │   │
    │   ├─ bead-abc [Worker-1]
    │   │   │
    │   │   └─ File Locks
    │   │       ├─ src/api.py (write)
    │   │       └─ tests/test_api.py (write)
    │   │
    │   └─ bead-def [Worker-2]
    │       │
    │       └─ File Locks
    │           ├─ src/database.py (write)
    │           └─ models/*.py (write)
    │
    ├─ Git Operations Lock (Sequential)
    │   │
    │   └─ commit, push, merge queue
    │
    └─ Global Resources
        │
        ├─ API Rate Limit Pool
        └─ Shared Configuration Files
```

### 3.2 Lock Acquisition Protocol

```
Worker requests bead-abc
    │
    ▼
[Lock Manager] Check bead availability
    │
    ├─→ Query: SELECT * FROM bead_locks WHERE bead_id = 'bead-abc'
    │
    ▼
Is bead locked?
    │
    ├─ YES ─→ Return failure, add to wait queue
    │
    └─ NO ──→ Predict files to be modified
              │
              ▼
          [File Predictor] Analyze bead description
              │
              ├─→ Extract file mentions
              ├─→ Pattern matching (test → test_*.py)
              ├─→ Historical analysis
              └─→ Returns: ['src/api.py', 'tests/test_api.py']
              │
              ▼
          [Lock Manager] Check file conflicts
              │
              ├─→ Query: SELECT * FROM file_locks WHERE file_path IN (...)
              │
              ▼
          Any file conflicts?
              │
              ├─ YES ─→ Return failure with conflict details
              │
              └─ NO ──→ Acquire locks atomically
                        │
                        ├─→ INSERT INTO bead_locks (...)
                        ├─→ INSERT INTO file_locks (...)
                        └─→ COMMIT transaction
                        │
                        ▼
                    Return success + lock token
```

## 4. Health Monitoring Architecture

### 4.1 Metrics Collection Pipeline

```
┌─────────────┐
│   Worker    │
└──────┬──────┘
       │
       │ (1) Heartbeat every 10s
       │ (2) Log writes
       │ (3) File modifications
       │ (4) API calls
       │
       ▼
┌─────────────────────────┐
│  Metrics Collector      │
├─────────────────────────┤
│  • LivenessMonitor      │
│  • ActivityMonitor      │
│  • ResourceMonitor      │
│  • RateLimitMonitor     │
│  • ErrorMonitor         │
│  • VelocityMonitor      │
└──────┬──────────────────┘
       │
       │ Every 15-30s (adaptive)
       │
       ▼
┌─────────────────────────┐
│   Health Aggregator     │
├─────────────────────────┤
│  • Compute health score │
│  • Detect anomalies     │
│  • Update trends        │
└──────┬──────────────────┘
       │
       ├──→ [Time Series DB] Store metrics
       │
       ├──→ [Alert Evaluator] Check alert rules
       │       │
       │       └──→ [Alert Manager] Send notifications
       │
       └──→ [Dashboard] Update UI
```

### 4.2 Health Score Calculation

```python
health_score = weighted_average([
    (liveness_check, 0.30),      # 30% - Is alive and responsive?
    (activity_score, 0.25),       # 25% - Making progress?
    (error_rate, 0.20),           # 20% - Error-free operation?
    (resource_usage, 0.15),       # 15% - Resource efficiency?
    (velocity_score, 0.10)        # 10% - Productivity?
])

where:
    liveness_check = 1.0 if responsive else 0.0

    activity_score = 1.0 - stuck_detection_score

    error_rate = 1.0 - min(errors_per_hour / 10, 1.0)

    resource_usage = 1.0 - max(
        cpu_percent / 100,
        memory_percent / 100
    )

    velocity_score = productivity_score / 100
```

## 5. Worker Lifecycle State Machine

```
                    ┌──────────┐
                    │  IDLE    │
                    └────┬─────┘
                         │
                         │ assign_bead()
                         ▼
                    ┌──────────┐
              ┌─────┤ STARTING │
              │     └────┬─────┘
              │          │
              │          │ acquire_lock()
              │          ▼
              │     ┌──────────┐
              │     │ RUNNING  │◀─────────┐
              │     └────┬─────┘          │
              │          │                │
              │          ├─ heartbeat ────┘
              │          │
              │          ├─ complete ────→ COMPLETING
              │          │
              │          ├─ error ───────→ ERROR
              │          │
              │          └─ timeout ─────→ TIMEOUT
              │
              │ (restart)
              │
         ┌────┴─────┐
         │  ERROR   │
         └────┬─────┘
              │
              ├─ can_recover? ──→ BACKOFF ──→ STARTING
              │
              └─ !can_recover ──→ FAILED
```

## 6. Data Models

### 6.1 Bead Lock Table

```sql
CREATE TABLE bead_locks (
    bead_id TEXT PRIMARY KEY,
    worker_id TEXT NOT NULL,
    acquired_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP NOT NULL,
    file_patterns TEXT,  -- JSON array
    workspace_path TEXT,

    INDEX idx_worker (worker_id),
    INDEX idx_expiry (expires_at)
);
```

### 6.2 File Lock Table

```sql
CREATE TABLE file_locks (
    file_path TEXT,
    bead_id TEXT,
    lock_mode TEXT CHECK(lock_mode IN ('read', 'write')),
    acquired_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,

    PRIMARY KEY (file_path, bead_id),
    FOREIGN KEY (bead_id) REFERENCES bead_locks(bead_id) ON DELETE CASCADE
);
```

### 6.3 Worker Metrics Table

```sql
CREATE TABLE worker_metrics (
    id SERIAL PRIMARY KEY,
    worker_id TEXT NOT NULL,
    timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,

    -- Liveness
    is_alive BOOLEAN,
    heartbeat_age_seconds INT,

    -- Activity
    log_entries_last_minute INT,
    files_modified INT,
    api_calls_count INT,
    idle_time_seconds INT,
    stuck_score FLOAT,

    -- Resources
    cpu_percent FLOAT,
    memory_mb FLOAT,

    -- Errors
    errors_last_hour INT,
    consecutive_errors INT,

    -- Velocity
    beads_completed INT,
    success_rate FLOAT,
    productivity_score FLOAT,

    -- Aggregated
    health_score FLOAT,

    INDEX idx_worker_time (worker_id, timestamp)
);
```

### 6.4 Alert Events Table

```sql
CREATE TABLE alert_events (
    id SERIAL PRIMARY KEY,
    timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    alert_name TEXT NOT NULL,
    alert_level TEXT CHECK(alert_level IN ('CRITICAL', 'WARNING', 'INFO')),
    worker_id TEXT,
    message TEXT,
    metadata JSONB,
    acknowledged BOOLEAN DEFAULT FALSE,

    INDEX idx_worker (worker_id),
    INDEX idx_level_time (alert_level, timestamp)
);
```

## 7. API Interfaces

### 7.1 Worker Manager API

```python
class WorkerManager:
    def create_worker(
        self,
        model_config: dict,
        workspace: str
    ) -> Worker:
        """Create and initialize a worker."""

    def assign_bead(
        self,
        worker_id: str,
        bead_id: str
    ) -> bool:
        """Assign bead to worker."""

    def pause_worker(self, worker_id: str):
        """Temporarily pause worker."""

    def resume_worker(self, worker_id: str):
        """Resume paused worker."""

    def terminate_worker(
        self,
        worker_id: str,
        graceful: bool = True,
        timeout: int = 30
    ):
        """Stop worker."""

    def get_worker_status(self, worker_id: str) -> dict:
        """Get current worker status and metrics."""
```

### 7.2 Lock Manager API

```python
class LockManager:
    def acquire_bead_lock(
        self,
        bead_id: str,
        worker_id: str,
        file_patterns: List[str],
        timeout: int = 1800
    ) -> LockResult:
        """Acquire exclusive lock on bead."""

    def release_bead_lock(
        self,
        bead_id: str,
        worker_id: str
    ):
        """Release bead lock."""

    def check_file_conflicts(
        self,
        file_patterns: List[str]
    ) -> List[Conflict]:
        """Check if files are locked."""

    def cleanup_expired_locks(self):
        """Remove expired locks."""

    def get_lock_status(self, bead_id: str) -> dict:
        """Get current lock status."""
```

### 7.3 Health Monitor API

```python
class HealthMonitor:
    def register_worker(self, worker: Worker):
        """Register worker for monitoring."""

    def check_health(self, worker_id: str) -> HealthStatus:
        """Perform health check."""

    def get_metrics(
        self,
        worker_id: str,
        since: datetime = None
    ) -> List[Metrics]:
        """Retrieve historical metrics."""

    def get_health_score(self, worker_id: str) -> float:
        """Get current health score (0-1)."""

    def set_alert_rule(self, rule: AlertRule):
        """Configure alert rule."""
```

### 7.4 Recovery Manager API

```python
class RecoveryManager:
    def handle_failure(
        self,
        worker: Worker,
        failure_type: str,
        error: Exception
    ) -> RecoveryAction:
        """Determine and execute recovery action."""

    def restart_worker(
        self,
        worker: Worker,
        preserve_state: bool = True
    ) -> Worker:
        """Restart worker with optional state preservation."""

    def fallback_to_model(
        self,
        worker: Worker,
        target_model: str
    ) -> Worker:
        """Create new worker with fallback model."""

    def cleanup_worker(self, worker_id: str):
        """Clean up failed worker resources."""
```

## 8. Deployment Architecture

### 8.1 Single-Machine Deployment

```
┌────────────────────────────────────────┐
│         Host Machine                   │
│                                        │
│  ┌──────────────────────────────────┐ │
│  │  Control Panel Process          │ │
│  │                                  │ │
│  │  ├─ Orchestrator                │ │
│  │  ├─ Worker Pool (N workers)     │ │
│  │  ├─ Health Monitor               │ │
│  │  ├─ Lock Manager (SQLite)        │ │
│  │  └─ TUI Dashboard                │ │
│  └──────────────────────────────────┘ │
│                                        │
│  ┌──────────────────────────────────┐ │
│  │  Data Storage                    │ │
│  │  ├─ beads.db (SQLite)            │ │
│  │  ├─ locks.db (SQLite)            │ │
│  │  └─ metrics.db (SQLite)          │ │
│  └──────────────────────────────────┘ │
└────────────────────────────────────────┘
```

### 8.2 Distributed Deployment (Kubernetes)

```
┌──────────────────────────────────────────────────────────┐
│                    Kubernetes Cluster                     │
│                                                           │
│  ┌────────────────────────────────────────────────────┐  │
│  │  Control Panel Orchestrator (Deployment)          │  │
│  │  ├─ Scheduler                                      │  │
│  │  ├─ Worker Manager                                 │  │
│  │  └─ Health Monitor                                 │  │
│  └────────────────────────────────────────────────────┘  │
│                          │                                │
│                          ▼                                │
│  ┌────────────────────────────────────────────────────┐  │
│  │  Worker Pods (StatefulSet)                         │  │
│  │  ├─ worker-0 (Claude Opus)                         │  │
│  │  ├─ worker-1 (GLM-4)                               │  │
│  │  └─ worker-2 (Sonnet)                              │  │
│  └────────────────────────────────────────────────────┘  │
│                          │                                │
│                          ▼                                │
│  ┌────────────────────────────────────────────────────┐  │
│  │  Distributed Services                              │  │
│  │  ├─ Redis Cluster (Lock Manager)                   │  │
│  │  ├─ PostgreSQL (Beads DB)                          │  │
│  │  └─ TimescaleDB (Metrics)                          │  │
│  └────────────────────────────────────────────────────┘  │
│                          │                                │
│                          ▼                                │
│  ┌────────────────────────────────────────────────────┐  │
│  │  Monitoring Stack                                  │  │
│  │  ├─ Prometheus (Metrics collection)                │  │
│  │  ├─ Grafana (Dashboards)                           │  │
│  │  └─ AlertManager (Notifications)                   │  │
│  └────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

## 9. Security Considerations

### 9.1 Worker Isolation

```
Each worker runs in isolated environment:
├─ Separate process (no shared memory)
├─ Dedicated workspace directory
├─ API key rotation (if compromised)
└─ Resource limits (CPU, memory, disk)
```

### 9.2 Lock Security

```
Lock tampering prevention:
├─ Cryptographic signatures on lock tokens
├─ Expiry timestamps (prevent indefinite holds)
├─ Owner validation (only owner can release)
└─ Audit log (all lock operations logged)
```

### 9.3 API Key Management

```
Secure credential handling:
├─ Environment variables (never in code)
├─ Kubernetes Secrets (encrypted at rest)
├─ Rotation policy (rotate every 90 days)
└─ Per-worker keys (limit blast radius)
```

## 10. Scalability Considerations

### 10.1 Horizontal Scaling

**Current limitations:**
- SQLite locks (single-machine)
- File system workspace (local disk)

**Scaling strategy:**
- Phase 1 (1-10 workers): SQLite + local FS
- Phase 2 (10-50 workers): Redis locks + NFS
- Phase 3 (50+ workers): etcd + distributed FS (Ceph, GlusterFS)

### 10.2 Performance Optimization

**Bottleneck analysis:**

1. **Lock contention:** Use finer-grained locks (file-level vs workspace-level)
2. **Scheduler overhead:** Cache dependency graphs, incremental updates
3. **Health checks:** Adaptive intervals, batch metric collection
4. **Database I/O:** Connection pooling, read replicas for metrics

**Expected performance:**

| Workers | Lock Ops/sec | Health Checks/sec | Scheduler Latency |
|---------|-------------|-------------------|-------------------|
| 1-5     | 100+        | 20+               | <10ms            |
| 10-20   | 500+        | 50+               | <50ms            |
| 50-100  | 1000+       | 100+              | <200ms           |

## 11. Observability

### 11.1 Metrics to Expose

**System metrics:**
- Worker count (by state: idle, running, error)
- Active bead count
- Lock contention rate
- Queue depth

**Performance metrics:**
- Beads completed per hour
- Average completion time
- Worker utilization
- API call rate

**Health metrics:**
- Worker health scores
- Error rate
- Recovery success rate
- Alert frequency

### 11.2 Logging Strategy

**Structured logging format:**

```json
{
  "timestamp": "2026-02-07T14:30:00Z",
  "level": "INFO",
  "component": "worker_manager",
  "worker_id": "worker-abc123",
  "event": "bead_completed",
  "bead_id": "po-2ug",
  "duration_seconds": 245,
  "tokens_used": 15234,
  "cost_usd": 0.45,
  "metadata": {
    "model": "claude-opus-4",
    "files_modified": 3,
    "commits": 1
  }
}
```

## 12. Future Enhancements

### 12.1 Machine Learning Integration

- Predict bead completion time using historical data
- Detect anomalies in worker behavior
- Optimize worker-to-bead assignment
- Auto-tune health check intervals

### 12.2 Advanced Scheduling

- Critical path analysis (prioritize bottleneck beads)
- Resource-aware scheduling (CPU/memory constraints)
- Time-of-day scheduling (cheaper API rates)
- Preemptive scheduling (interrupt low-priority beads)

### 12.3 Cost Optimization

- Automatic model selection based on bead complexity
- Batch API calls to reduce overhead
- Cache LLM responses for similar queries
- Spot instance usage for cloud workers
