# Control Panel Architecture Diagrams

## 1. System Component Diagram

```
┌────────────────────────────────────────────────────────────────────────────┐
│                          CONTROL PANEL SYSTEM                             │
└────────────────────────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────────────────────────┐
│                                USER LAYER                                  │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│    ┌────────────────────────────────────────────────────────────┐         │
│    │               TUI Dashboard (rich/textual)                 │         │
│    │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐    │         │
│    │  │ Worker View  │  │ Metrics View │  │  Alerts View │    │         │
│    │  └──────────────┘  └──────────────┘  └──────────────┘    │         │
│    └────────────────────────────────────────────────────────────┘         │
│                                   │                                        │
└───────────────────────────────────┼────────────────────────────────────────┘
                                    │ REST API / RPC
                                    ▼
┌────────────────────────────────────────────────────────────────────────────┐
│                            ORCHESTRATOR LAYER                              │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  ┌──────────────────┐  ┌──────────────────┐  ┌──────────────────┐        │
│  │  Task Scheduler  │  │  Worker Manager  │  │  Lock Manager    │        │
│  │                  │  │                  │  │                  │        │
│  │ • Dependency     │  │ • Pool mgmt      │  │ • Bead locks     │        │
│  │   resolution     │  │ • Lifecycle      │  │ • File locks     │        │
│  │ • Priority queue │  │ • Assignment     │  │ • Deadlock det.  │        │
│  │ • Deadlock det.  │  │ • Health checks  │  │ • Cleanup        │        │
│  └────────┬─────────┘  └────────┬─────────┘  └────────┬─────────┘        │
│           │                     │                      │                  │
│           └─────────────────────┼──────────────────────┘                  │
│                                 │                                         │
└─────────────────────────────────┼─────────────────────────────────────────┘
                                  │
          ┌───────────────────────┼───────────────────────┐
          │                       │                       │
          ▼                       ▼                       ▼
┌────────────────────────────────────────────────────────────────────────────┐
│                               WORKER LAYER                                 │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  ┌────────────────┐    ┌────────────────┐    ┌────────────────┐          │
│  │  Worker 1      │    │  Worker 2      │    │  Worker N      │          │
│  │  ┌──────────┐  │    │  ┌──────────┐  │    │  ┌──────────┐  │          │
│  │  │  Claude  │  │    │  │   GLM-4  │  │    │  │  Sonnet  │  │          │
│  │  │  Opus 4  │  │    │  │          │  │    │  │   3.7    │  │          │
│  │  └──────────┘  │    │  └──────────┘  │    │  └──────────┘  │          │
│  │                │    │                │    │                │          │
│  │ • Bead exec    │    │ • Bead exec    │    │ • Bead exec    │          │
│  │ • File ops     │    │ • File ops     │    │ • File ops     │          │
│  │ • Git commits  │    │ • Git commits  │    │ • Git commits  │          │
│  │ • Heartbeat    │    │ • Heartbeat    │    │ • Heartbeat    │          │
│  └────────┬───────┘    └────────┬───────┘    └────────┬───────┘          │
│           │                     │                      │                  │
└───────────┼─────────────────────┼──────────────────────┼──────────────────┘
            │                     │                      │
            └─────────────────────┼──────────────────────┘
                                  │
                                  ▼
┌────────────────────────────────────────────────────────────────────────────┐
│                           MONITORING LAYER                                 │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐                 │
│  │   Liveness    │  │   Activity    │  │   Resource    │                 │
│  │   Monitor     │  │   Monitor     │  │   Monitor     │                 │
│  └───────┬───────┘  └───────┬───────┘  └───────┬───────┘                 │
│          │                  │                  │                          │
│          └──────────────────┼──────────────────┘                          │
│                             ▼                                              │
│               ┌──────────────────────────┐                                │
│               │   Health Aggregator      │                                │
│               │  • Compute health score  │                                │
│               │  • Detect anomalies      │                                │
│               └────────────┬─────────────┘                                │
│                            │                                               │
│          ┌─────────────────┼─────────────────┐                            │
│          ▼                 ▼                 ▼                            │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                    │
│  │   Alert      │  │   Recovery   │  │   Circuit    │                    │
│  │   Manager    │  │   Manager    │  │   Breaker    │                    │
│  └──────────────┘  └──────────────┘  └──────────────┘                    │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
                                  │
                                  ▼
┌────────────────────────────────────────────────────────────────────────────┐
│                          PERSISTENCE LAYER                                 │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐  ┌────────────┐          │
│  │  Beads DB  │  │  Locks DB  │  │ Metrics DB │  │ Event Log  │          │
│  │  (SQLite/  │  │ (SQLite/   │  │(TimescaleDB│  │(Structured │          │
│  │   JSONL)   │  │Redis/etcd) │  │/InfluxDB)  │  │  Logging)  │          │
│  └────────────┘  └────────────┘  └────────────┘  └────────────┘          │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
```

## 2. Lock Manager Flow Diagram

```
                         LOCK ACQUISITION FLOW

User/Scheduler
    │
    │ assign_bead(worker_1, bead_abc)
    ▼
┌───────────────────────────────────────────────────────────────┐
│                    Lock Manager                               │
└───────────────────────────────────────────────────────────────┘
    │
    │ Step 1: Check bead availability
    ▼
┌─────────────────────────────────────────────────────────────────┐
│  Query: SELECT * FROM bead_locks WHERE bead_id = 'bead_abc'    │
└─────────────────────────────────────────────────────────────────┘
    │
    ├─→ EXISTS ──→ Return LOCKED ──→ Add to wait queue
    │                                       │
    │                                       ▼
    │                              ┌────────────────┐
    │                              │  Wait Queue    │
    │                              │  • worker_1    │
    │                              │  • worker_3    │
    │                              └────────────────┘
    │
    └─→ NOT EXISTS
         │
         │ Step 2: Predict file modifications
         ▼
    ┌───────────────────────────────────────┐
    │      File Conflict Predictor          │
    │                                       │
    │  analyze(bead.description)            │
    │  ├─ Extract file mentions            │
    │  ├─ Pattern matching                 │
    │  └─ Historical analysis               │
    │                                       │
    │  Returns: ['src/api.py',             │
    │            'tests/test_api.py']       │
    └─────────────────┬─────────────────────┘
                      │
                      │ Step 3: Check file conflicts
                      ▼
    ┌───────────────────────────────────────────────────────┐
    │  Query: SELECT bl.worker_id, fl.file_path             │
    │         FROM file_locks fl                            │
    │         JOIN bead_locks bl ON fl.bead_id = bl.bead_id │
    │         WHERE fl.file_path IN ('src/api.py', ...)     │
    │         AND fl.lock_mode = 'write'                    │
    └───────────────────────────────────────────────────────┘
                      │
                      ├─→ CONFLICTS FOUND
                      │       │
                      │       └─→ Return CONFLICT with details
                      │                │
                      │                ▼
                      │           ┌──────────────────────────┐
                      │           │ Conflict: src/api.py     │
                      │           │ Locked by: worker_2      │
                      │           │ For bead: bead_def       │
                      │           └──────────────────────────┘
                      │
                      └─→ NO CONFLICTS
                               │
                               │ Step 4: Acquire locks atomically
                               ▼
                      ┌──────────────────────────────────────┐
                      │  BEGIN TRANSACTION                   │
                      │                                      │
                      │  INSERT INTO bead_locks              │
                      │    (bead_id, worker_id, expires_at)  │
                      │  VALUES ('bead_abc', 'worker_1',     │
                      │          NOW() + 30min)              │
                      │                                      │
                      │  INSERT INTO file_locks              │
                      │    (file_path, bead_id, lock_mode)   │
                      │  VALUES                              │
                      │    ('src/api.py', 'bead_abc', 'w'),  │
                      │    ('tests/test_api.py','bead_abc','w')│
                      │                                      │
                      │  COMMIT                              │
                      └──────────────────┬───────────────────┘
                                         │
                                         ▼
                                  ┌──────────────┐
                                  │ Lock Token   │
                                  │ bead_abc:... │
                                  └──────────────┘
                                         │
                                         │ Return SUCCESS
                                         ▼
                                    Worker 1 starts
                                    executing bead_abc
```

## 3. Worker Lifecycle State Machine

```
                        WORKER LIFECYCLE STATES

    ┌─────────────────────────────────────────────────────────────┐
    │                           IDLE                              │
    │                                                             │
    │  • No bead assigned                                         │
    │  • Process alive, waiting for work                          │
    │  • Consuming minimal resources                              │
    └──────────────────┬──────────────────────────────────────────┘
                       │
                       │ assign_bead(bead_id)
                       │
                       ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                        STARTING                             │
    │                                                             │
    │  • Bead assigned                                            │
    │  • Acquiring locks                                          │
    │  • Initializing workspace                                   │
    └──────────────────┬──────────────────────────────────────────┘
                       │
                       ├─→ lock_failed ──────────────────┐
                       │                                 │
                       │ locks_acquired                  │
                       ▼                                 ▼
    ┌─────────────────────────────────────────────┐  ┌──────────┐
    │              RUNNING                        │  │  BLOCKED │
    │                                             │  │          │
    │  • Executing bead                           │  │ Waiting  │
    │  • Making LLM API calls                     │  │ for lock │
    │  • Modifying files                          │  └────┬─────┘
    │  • Committing changes                       │       │
    │  • Sending heartbeats                       │       │
    └──┬───────────────────┬──────────────────┬───┘       │
       │                   │                  │           │
       │ heartbeat()       │                  │           │
       │ (every 10s)       │                  │           │
       └───────────────────┘                  │           │
                           │                  │           │
              ┌────────────┼──────────────────┤           │
              │            │                  │           │
              │            │                  │           │
      complete│    timeout │          error   │           │
              │            │                  │           │
              ▼            ▼                  ▼           │
    ┌──────────────┐  ┌──────────┐    ┌────────────┐    │
    │ COMPLETING   │  │ TIMEOUT  │    │   ERROR    │◄───┘
    │              │  │          │    │            │
    │ • Releasing  │  │ Stuck >  │    │ Exception  │
    │   locks      │  │ 30min    │    │ occurred   │
    │ • Updating   │  │          │    │            │
    │   metrics    │  └────┬─────┘    └─────┬──────┘
    │              │       │                 │
    └──────┬───────┘       │                 │
           │               │                 │
           │               ▼                 ▼
           │          ┌────────────────────────────┐
           │          │    RECOVERY LOGIC          │
           │          │                            │
           │          │  • Analyze failure         │
           │          │  • Check retry count       │
           │          │  • Calculate backoff       │
           │          │  • Preserve state          │
           │          │  • Clean up locks          │
           │          └────┬───────────────────────┘
           │               │
           │               ├─→ can_recover ──→ BACKOFF ──┐
           │               │                             │
           │               │                    ┌────────▼──────┐
           │               │                    │   BACKOFF     │
           │               │                    │               │
           │               │                    │  Wait time =  │
           │               │                    │  2^failures   │
           │               │                    │  (max 1 hour) │
           │               │                    └───────┬───────┘
           │               │                            │
           │               │                            │ wait_complete
           │               │                            │
           │               │         ┌──────────────────┘
           │               │         │
           │               │         ├─→ retry_same_model ──┐
           │               │         │                      │
           │               │         └─→ fallback_model ────┤
           │               │                                │
           │               │                                ▼
           │               │                          ┌──────────┐
           │               │                          │ STARTING │
           │               │                          └──────────┘
           │               │
           │               └─→ !can_recover
           │                        │
           ▼                        ▼
    ┌──────────────┐        ┌─────────────┐
    │     IDLE     │        │   FAILED    │
    │              │        │             │
    │ Ready for    │        │ Manual      │
    │ next bead    │        │ intervention│
    └──────────────┘        │ required    │
                            └─────────────┘
```

## 4. Health Monitoring Architecture

```
                    HEALTH MONITORING SYSTEM

┌─────────────────────────────────────────────────────────────────┐
│                          WORKERS                                │
│                                                                 │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐                  │
│  │ Worker 1 │    │ Worker 2 │    │ Worker N │                  │
│  └────┬─────┘    └────┬─────┘    └────┬─────┘                  │
│       │               │               │                         │
└───────┼───────────────┼───────────────┼─────────────────────────┘
        │               │               │
        │ (1) Heartbeat │               │
        │ (2) Log writes│               │
        │ (3) File mods │               │
        │ (4) API calls │               │
        │               │               │
        ▼               ▼               ▼
┌─────────────────────────────────────────────────────────────────┐
│                   METRICS COLLECTORS                            │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                  Liveness Monitor                        │   │
│  │  • Check process exists (os.kill(pid, 0))               │   │
│  │  • Validate heartbeat freshness (<30s)                   │   │
│  │  • Ping-pong test (5s timeout)                          │   │
│  │  → is_alive: bool, response_time_ms: int                │   │
│  └──────────────────────────────────────────────────────────┘   │
│                             ↓                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                 Activity Monitor                         │   │
│  │  • Count log entries (last 1min)                        │   │
│  │  • Track file modifications                             │   │
│  │  • Monitor API call frequency                           │   │
│  │  • Calculate idle time                                  │   │
│  │  • Compute stuck_score (0-1)                            │   │
│  │  → activity_metrics: ActivityMetrics                    │   │
│  └──────────────────────────────────────────────────────────┘   │
│                             ↓                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                Resource Monitor                          │   │
│  │  • CPU usage (psutil.cpu_percent)                       │   │
│  │  • Memory usage (psutil.memory_info)                    │   │
│  │  • Disk I/O (psutil.io_counters)                        │   │
│  │  • File descriptors (psutil.open_files)                 │   │
│  │  • Detect leaks (monotonic growth)                      │   │
│  │  → resource_metrics: ResourceMetrics                    │   │
│  └──────────────────────────────────────────────────────────┘   │
│                             ↓                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │              Rate Limit Monitor                          │   │
│  │  • Track requests per minute/day                        │   │
│  │  • Parse rate limit headers                             │   │
│  │  • Calculate throttle delay                             │   │
│  │  → rate_limit_metrics: RateLimitMetrics                 │   │
│  └──────────────────────────────────────────────────────────┘   │
│                             ↓                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                 Error Monitor                            │   │
│  │  • Record all exceptions                                │   │
│  │  • Count consecutive errors                             │   │
│  │  • Detect error spirals                                 │   │
│  │  → error_metrics: ErrorMetrics                          │   │
│  └──────────────────────────────────────────────────────────┘   │
│                             ↓                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │               Velocity Monitor                           │   │
│  │  • Track beads completed/failed                         │   │
│  │  • Calculate success rate                               │   │
│  │  • Measure completion times                             │   │
│  │  • Compute productivity score                           │   │
│  │  → velocity_metrics: VelocityMetrics                    │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              │ Every 15-30s (adaptive)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                   HEALTH AGGREGATOR                             │
│                                                                 │
│  health_score = weighted_average([                              │
│      (liveness_check,     0.30),  # 30% - Alive?               │
│      (activity_score,     0.25),  # 25% - Progress?            │
│      (1 - error_rate,     0.20),  # 20% - Error-free?          │
│      (resource_efficiency,0.15),  # 15% - Resource usage?      │
│      (velocity_score,     0.10)   # 10% - Productive?          │
│  ])                                                             │
│                                                                 │
│  • Compute overall health (0-1)                                │
│  • Detect anomalies (statistical outliers)                     │
│  • Update health trends                                        │
│  • Predict future health                                       │
└─────────────────┬───────────────────────────────────────────────┘
                  │
                  ├──→ Store in TimescaleDB
                  │
                  └──→ Alert Evaluator
                         ↓
┌─────────────────────────────────────────────────────────────────┐
│                      ALERT RULES                                │
│                                                                 │
│  Rule: worker_dead                                              │
│  ├─ Condition: !worker.is_alive()                              │
│  ├─ Level: CRITICAL                                            │
│  └─ Action: restart_worker()                                   │
│                                                                 │
│  Rule: error_spiral                                             │
│  ├─ Condition: consecutive_errors >= 5                         │
│  ├─ Level: CRITICAL                                            │
│  └─ Action: fallback_to_next_model()                           │
│                                                                 │
│  Rule: high_memory_usage                                        │
│  ├─ Condition: memory_percent > 90                             │
│  ├─ Level: WARNING                                             │
│  └─ Action: restart_worker()                                   │
│                                                                 │
│  Rule: rate_limit_approaching                                   │
│  ├─ Condition: requests_per_minute > max_rpm * 0.9             │
│  ├─ Level: WARNING                                             │
│  └─ Action: throttle_worker()                                  │
└─────────────────┬───────────────────────────────────────────────┘
                  │
                  │ Alert triggered
                  ▼
┌─────────────────────────────────────────────────────────────────┐
│                    ALERT CHANNELS                               │
│                                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐          │
│  │   Slack      │  │    Email     │  │  PagerDuty   │          │
│  │              │  │              │  │              │          │
│  │ • CRITICAL   │  │ • CRITICAL   │  │ • CRITICAL   │          │
│  │ • WARNING    │  │ • WARNING    │  │   only       │          │
│  └──────────────┘  └──────────────┘  └──────────────┘          │
│                                                                 │
│  ┌──────────────────────────────────────────────────┐           │
│  │              Structured Logs                     │           │
│  │  • All alerts logged                             │           │
│  │  • Searchable and auditable                      │           │
│  └──────────────────────────────────────────────────┘           │
└─────────────────────────────────────────────────────────────────┘
```

## 5. Dependency Scheduling Flow

```
              DEPENDENCY-AWARE SCHEDULING

┌─────────────────────────────────────────────────────────────┐
│                      BEAD GRAPH                             │
│                                                             │
│         po-1oh (P0)          po-3h3 (P0)                    │
│         "API pricing"        "LLM models"                   │
│              │                    │                         │
│              │                    │                         │
│              └────────┬───────────┘                         │
│                       │                                     │
│                       ▼                                     │
│                  po-4gr (P0)                                │
│            "Subscription optimization"                      │
│                       │                                     │
│              ┌────────┴────────┐                            │
│              │                 │                            │
│              ▼                 ▼                            │
│         po-3pv (P1)       po-2ug (P1)                       │
│      "Task scoring"    "Multi-worker coord"                 │
│              │                 │                            │
│              └────────┬────────┘                            │
│                       │                                     │
│                       ▼                                     │
│                  po-7jb (P1)                                │
│               "TUI framework"                               │
│                       │                                     │
│                       ▼                                     │
│                  po-3j1 (P1)                                │
│               "TUI dashboard"                               │
│                       │                                     │
│                       ▼                                     │
│                  po-1x9 (P2)                                │
│              "TUI prototype"                                │
└─────────────────────────────────────────────────────────────┘

                        ↓ Scheduler analyzes

┌─────────────────────────────────────────────────────────────┐
│              TOPOLOGICAL SORT + PRIORITY                    │
│                                                             │
│  Step 1: Calculate in-degree for each bead                 │
│  ┌────────┬────────────┬───────────┬──────────┐            │
│  │ Bead   │ Depends On │ In-Degree │ Priority │            │
│  ├────────┼────────────┼───────────┼──────────┤            │
│  │po-1oh  │ []         │ 0         │ P0       │            │
│  │po-3h3  │ []         │ 0         │ P0       │            │
│  │po-4gr  │ [1oh,3h3]  │ 2         │ P0       │            │
│  │po-3pv  │ [4gr]      │ 1         │ P1       │            │
│  │po-2ug  │ [4gr]      │ 1         │ P1       │            │
│  │po-7jb  │ [3pv,2ug]  │ 2         │ P1       │            │
│  │po-3j1  │ [7jb]      │ 1         │ P1       │            │
│  │po-1x9  │ [3j1]      │ 1         │ P2       │            │
│  └────────┴────────────┴───────────┴──────────┘            │
│                                                             │
│  Step 2: Extract ready beads (in-degree = 0)               │
│  Ready: [po-1oh, po-3h3]  (both P0)                        │
│                                                             │
│  Step 3: Assign to workers                                 │
│  ┌──────────────────────────────────────────────┐          │
│  │ Worker 1 ← po-1oh (API pricing)              │          │
│  │ Worker 2 ← po-3h3 (LLM models)               │          │
│  │ Worker 3 ← IDLE (waiting for po-4gr)         │          │
│  └──────────────────────────────────────────────┘          │
└─────────────────────────────────────────────────────────────┘

                        ↓ Time passes

┌─────────────────────────────────────────────────────────────┐
│              COMPLETION UPDATES GRAPH                       │
│                                                             │
│  Worker 1 completes po-1oh                                  │
│  ├─→ Mark po-1oh as complete                               │
│  └─→ Decrement in-degree of po-4gr (2 → 1)                 │
│                                                             │
│  Worker 2 completes po-3h3                                  │
│  ├─→ Mark po-3h3 as complete                               │
│  └─→ Decrement in-degree of po-4gr (1 → 0)                 │
│                                                             │
│  Updated ready beads: [po-4gr]                              │
│  ├─→ Assign po-4gr to Worker 1                             │
│  └─→ Workers 2 and 3 still idle                            │
└─────────────────────────────────────────────────────────────┘

                        ↓ Time passes

┌─────────────────────────────────────────────────────────────┐
│             PARALLEL EXECUTION UNLOCKED                     │
│                                                             │
│  Worker 1 completes po-4gr                                  │
│  ├─→ Decrement in-degree of po-3pv (1 → 0)                 │
│  └─→ Decrement in-degree of po-2ug (1 → 0)                 │
│                                                             │
│  Updated ready beads: [po-3pv, po-2ug]                      │
│  ├─→ Assign po-3pv to Worker 1                             │
│  └─→ Assign po-2ug to Worker 2                             │
│                                                             │
│  ✓ Now 2 workers executing in parallel!                    │
└─────────────────────────────────────────────────────────────┘
```

## 6. Recovery Decision Tree

```
                        WORKER FAILURE RECOVERY

                    ┌──────────────────┐
                    │  Worker Failed   │
                    └────────┬─────────┘
                             │
                             ▼
                ┌────────────────────────┐
                │  Classify Failure Type │
                └────────┬───────────────┘
                         │
         ┌───────────────┼───────────────┬───────────────┐
         │               │               │               │
         ▼               ▼               ▼               ▼
    ┌────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
    │Process │    │ Timeout  │    │API Error │    │Code Bug  │
    │ Crash  │    │(>30 min) │    │(rate lmt)│    │(syntax)  │
    └───┬────┘    └────┬─────┘    └────┬─────┘    └────┬─────┘
        │              │               │               │
        │              │               │               │
        ▼              ▼               ▼               ▼
    Can auto-     Is stuck?      Rate limited?    Syntax error?
    recover?           │               │               │
        │              │               │               │
        │ YES          │ YES           │ YES           │ NO
        │              │               │               │
        ▼              ▼               ▼               ▼
    ┌────────┐    ┌────────┐    ┌────────┐    ┌────────────┐
    │Restart │    │Restart │    │Wait for│    │Manual Fix  │
    │Worker  │    │+Reset  │    │Reset   │    │Required    │
    │        │    │Context │    │Time    │    │            │
    └───┬────┘    └───┬────┘    └───┬────┘    └────────────┘
        │             │              │
        │             │              │
        ▼             ▼              ▼
    Check retry  Check retry    Check retry
    count < 10   count < 5      count < 3
        │             │              │
        ├─ YES        ├─ YES         ├─ YES
        │             │              │
        │             │              │
        ▼             ▼              ▼
┌──────────────────────────────────────────────────────┐
│           Calculate Backoff Wait Time                │
│                                                      │
│   wait_time = min(2^retry_count, 3600)              │
│                                                      │
│   retry_count  wait_time                            │
│   ──────────  ──────────                            │
│       1          2s                                  │
│       2          4s                                  │
│       3          8s                                  │
│       4         16s                                  │
│       5         32s                                  │
│       6         64s (1 min)                          │
│       7        128s (2 min)                          │
│       8        256s (4 min)                          │
│       9        512s (8 min)                          │
│      10+      3600s (60 min, capped)                 │
└──────────────────┬───────────────────────────────────┘
                   │
                   │ Wait...
                   │
                   ▼
         ┌────────────────────┐
         │  Should Fallback?  │
         └────────┬───────────┘
                  │
          ┌───────┴───────┐
          │               │
          ▼               ▼
      Multiple        First retry
      failures?
          │               │
          │ YES           │ NO
          │               │
          ▼               ▼
  ┌──────────────┐  ┌──────────────┐
  │   Fallback   │  │  Retry Same  │
  │   to Next    │  │    Model     │
  │   Model      │  │              │
  └──────┬───────┘  └──────┬───────┘
         │                 │
         │                 │
         ▼                 ▼
  ┌──────────────────────────────────┐
  │  Fallback Chain:                 │
  │                                  │
  │  Claude Opus 4                   │
  │       ↓ (failed)                 │
  │  Claude Sonnet 3.7               │
  │       ↓ (failed)                 │
  │  GLM-4                           │
  │       ↓ (failed)                 │
  │  [EXHAUSTED] → Manual escalation │
  └──────┬───────────────────────────┘
         │
         │
         ▼
  ┌──────────────────────┐
  │ Preserve State:      │
  │ • Bead progress      │
  │ • Files modified     │
  │ • Context summary    │
  │ • Partial outputs    │
  └──────┬───────────────┘
         │
         │
         ▼
  ┌──────────────────────┐
  │  Create New Worker   │
  │  with Restored State │
  └──────┬───────────────┘
         │
         │
         ▼
  ┌──────────────────────┐
  │   Resume Execution   │
  └──────────────────────┘
```

## 7. Cost Optimization Flow

```
                    COST OPTIMIZATION SYSTEM

┌─────────────────────────────────────────────────────────────┐
│                    INCOMING BEAD                            │
│                                                             │
│  Bead: po-3pv                                               │
│  Title: "Design task scoring algorithm"                    │
│  Description: "Create algorithm to score task complexity   │
│                and assign optimal LLM model..."            │
│  Priority: P1                                               │
│  Estimated complexity: ?                                    │
└──────────────────────────┬──────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│               COMPLEXITY ANALYZER                           │
│                                                             │
│  Analyze bead characteristics:                              │
│  ├─ Text length: 500 words → Medium                        │
│  ├─ Keywords: "algorithm", "design" → High complexity      │
│  ├─ Requires: Architecture, planning → High complexity     │
│  ├─ Historical similar beads: Avg 800s, Opus preferred     │
│  └─ File scope: Multiple files → Medium                    │
│                                                             │
│  Complexity Score: 7.5/10 → HIGH                            │
└──────────────────────────┬──────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                 MODEL COST MATRIX                           │
│                                                             │
│  ┌────────────┬──────────┬──────────┬──────────┐           │
│  │ Model      │ $/1K tok │ Capability│ Speed    │           │
│  ├────────────┼──────────┼──────────┼──────────┤           │
│  │ Opus 4     │  $15.00  │  10/10   │  Slow    │           │
│  │ Sonnet 3.7 │   $3.00  │   8/10   │  Medium  │           │
│  │ GLM-4      │   $0.50  │   6/10   │  Fast    │           │
│  └────────────┴──────────┴──────────┴──────────┘           │
│                                                             │
│  Estimated tokens for po-3pv:                               │
│  • Input: 2K (context + bead description)                  │
│  • Output: 8K (algorithm design + code)                    │
│  • Total: 10K tokens                                       │
└──────────────────────────┬──────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│              COST vs CAPABILITY ANALYSIS                    │
│                                                             │
│  Option 1: Opus 4                                           │
│  ├─ Cost: 10K × $15/1K = $150.00                           │
│  ├─ Success probability: 95%                               │
│  ├─ Expected time: 600s                                    │
│  └─ Quality score: 9.5/10                                  │
│                                                             │
│  Option 2: Sonnet 3.7                                       │
│  ├─ Cost: 10K × $3/1K = $30.00                             │
│  ├─ Success probability: 85%                               │
│  ├─ Expected time: 400s                                    │
│  └─ Quality score: 8/10                                    │
│                                                             │
│  Option 3: GLM-4                                            │
│  ├─ Cost: 10K × $0.50/1K = $5.00                           │
│  ├─ Success probability: 60%                               │
│  ├─ Expected time: 300s                                    │
│  └─ Quality score: 6/10                                    │
│                                                             │
│  Factoring in retry costs:                                 │
│  • GLM-4: $5 × (1 + 0.4 retry) = $7.00 expected           │
│  • Sonnet: $30 × (1 + 0.15 retry) = $34.50 expected       │
│  • Opus: $150 × (1 + 0.05 retry) = $157.50 expected       │
└──────────────────────────┬──────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                 DECISION LOGIC                              │
│                                                             │
│  IF complexity >= 8 AND priority = P0:                     │
│      → Use Opus (quality matters most)                     │
│                                                             │
│  ELIF complexity >= 6 AND budget_remaining > $500:         │
│      → Use Sonnet (good balance)                           │
│                                                             │
│  ELIF complexity < 5:                                       │
│      → Use GLM-4 (cost-effective for simple tasks)         │
│                                                             │
│  ELSE:                                                      │
│      → Use Sonnet with fallback to Opus if failed         │
│                                                             │
│  Decision: Use Sonnet 3.7                                  │
│  Reason: complexity=7.5, P1, budget=$1200                  │
└──────────────────────────┬──────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│              ASSIGN TO WORKER                               │
│                                                             │
│  ┌──────────────────────────────────────────────┐          │
│  │ Worker 2 (Sonnet 3.7)                        │          │
│  │ ├─ Current load: 0 beads                     │          │
│  │ ├─ Cost today: $45.00                        │          │
│  │ └─ Estimated cost: +$30.00 → $75.00          │          │
│  └──────────────────────────────────────────────┘          │
│                                                             │
│  Update budget tracker:                                     │
│  ├─ Budget remaining: $1200 → $1170                        │
│  ├─ Projected spend today: $450                            │
│  └─ Alert threshold (80%): Not exceeded                    │
└─────────────────────────────────────────────────────────────┘

                           ↓

┌─────────────────────────────────────────────────────────────┐
│               EXECUTION & MONITORING                        │
│                                                             │
│  Track actual usage:                                        │
│  ├─ Actual tokens: 9.5K (vs 10K estimated)                 │
│  ├─ Actual cost: $28.50 (vs $30 estimated)                 │
│  ├─ Completion time: 420s (vs 400s estimated)              │
│  └─ Success: ✓                                             │
│                                                             │
│  Update ML model for future estimates                       │
└─────────────────────────────────────────────────────────────┘
```

## 8. Database Schema Diagram

```
┌──────────────────────────────────────────────────────────────┐
│                    DATABASE SCHEMA                           │
└──────────────────────────────────────────────────────────────┘

┌────────────────────────────────┐
│         bead_locks             │
├────────────────────────────────┤
│ PK  bead_id         TEXT       │
│     worker_id       TEXT       │
│     acquired_at     TIMESTAMP  │
│     expires_at      TIMESTAMP  │
│     file_patterns   TEXT (JSON)│
│     workspace_path  TEXT       │
├────────────────────────────────┤
│ INDEX: worker_id               │
│ INDEX: expires_at              │
└────────────┬───────────────────┘
             │
             │ 1:N relationship
             │
             ▼
┌────────────────────────────────┐
│         file_locks             │
├────────────────────────────────┤
│ PK  file_path       TEXT       │
│ PK  bead_id         TEXT       │◄─── FK to bead_locks
│     lock_mode       TEXT       │
│     acquired_at     TIMESTAMP  │
└────────────────────────────────┘


┌────────────────────────────────┐
│      worker_metrics            │
├────────────────────────────────┤
│ PK  id              SERIAL     │
│     worker_id       TEXT       │
│     timestamp       TIMESTAMP  │
│                                │
│     -- Liveness                │
│     is_alive        BOOLEAN    │
│     heartbeat_age_s INT        │
│                                │
│     -- Activity                │
│     log_entries_1m  INT        │
│     files_modified  INT        │
│     api_calls       INT        │
│     idle_time_s     INT        │
│     stuck_score     FLOAT      │
│                                │
│     -- Resources               │
│     cpu_percent     FLOAT      │
│     memory_mb       FLOAT      │
│                                │
│     -- Errors                  │
│     errors_1h       INT        │
│     consec_errors   INT        │
│                                │
│     -- Velocity                │
│     beads_completed INT        │
│     success_rate    FLOAT      │
│     productivity    FLOAT      │
│                                │
│     -- Aggregated              │
│     health_score    FLOAT      │
├────────────────────────────────┤
│ INDEX: (worker_id, timestamp)  │
└────────────────────────────────┘


┌────────────────────────────────┐
│       alert_events             │
├────────────────────────────────┤
│ PK  id              SERIAL     │
│     timestamp       TIMESTAMP  │
│     alert_name      TEXT       │
│     alert_level     TEXT       │
│     worker_id       TEXT       │
│     message         TEXT       │
│     metadata        JSONB      │
│     acknowledged    BOOLEAN    │
├────────────────────────────────┤
│ INDEX: worker_id               │
│ INDEX: (alert_level, timestamp)│
└────────────────────────────────┘


┌────────────────────────────────┐
│       bead_history             │
├────────────────────────────────┤
│ PK  id              SERIAL     │
│     bead_id         TEXT       │
│     worker_id       TEXT       │
│     started_at      TIMESTAMP  │
│     completed_at    TIMESTAMP  │
│     status          TEXT       │
│     tokens_used     INT        │
│     cost_usd        FLOAT      │
│     model_used      TEXT       │
│     error_message   TEXT       │
│     retry_count     INT        │
└────────────────────────────────┘
```

This comprehensive set of diagrams provides visual representation of the entire control panel architecture, from high-level system design to detailed component interactions and data flows.