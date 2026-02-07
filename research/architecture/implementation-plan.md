# Control Panel Implementation Plan

## Overview

Phased implementation plan for building a production-ready multi-worker control panel with advanced coordination and health monitoring capabilities.

## Phase 1: Foundation (Weeks 1-3)

### Week 1: Core Infrastructure

#### Objectives
- Set up project structure
- Implement basic worker abstraction
- Create beads integration
- Build simple scheduler

#### Tasks

**1.1 Project Setup**
```bash
control-panel/
├── src/
│   ├── workers/
│   │   ├── __init__.py
│   │   ├── base.py           # Abstract Worker class
│   │   ├── claude_worker.py  # Claude Code worker
│   │   └── glm_worker.py     # GLM worker
│   ├── scheduler/
│   │   ├── __init__.py
│   │   ├── scheduler.py      # Task scheduler
│   │   └── dependency.py     # Dependency resolver
│   ├── locks/
│   │   ├── __init__.py
│   │   └── manager.py        # Lock manager
│   └── beads/
│       ├── __init__.py
│       └── client.py         # Beads CLI wrapper
├── tests/
├── config/
│   └── workers.yml           # Worker configurations
└── requirements.txt
```

**1.2 Base Worker Implementation**
- Abstract Worker class with standard interface
- Worker lifecycle management (start, stop, restart)
- Process management and signal handling
- Basic logging and stdout/stderr capture

**Deliverables:**
- [ ] Worker base class with methods: `start()`, `stop()`, `assign_bead()`, `get_status()`
- [ ] Claude Code worker implementation
- [ ] GLM worker implementation
- [ ] Unit tests for worker lifecycle

**Success Criteria:**
- Can spawn 3+ workers concurrently
- Workers can execute simple beads end-to-end
- Clean shutdown without orphaned processes

---

### Week 2: Basic Locking and Scheduling

#### Objectives
- Implement SQLite-based bead locking
- Build dependency-aware scheduler
- Create file conflict predictor

#### Tasks

**2.1 Lock Manager (SQLite)**
```python
# src/locks/manager.py

class LockManager:
    def __init__(self, db_path: str):
        self.db = sqlite3.connect(db_path)
        self._create_tables()

    def acquire_bead_lock(
        self,
        bead_id: str,
        worker_id: str,
        timeout: int = 1800
    ) -> bool:
        """Acquire exclusive lock on bead."""

    def release_bead_lock(self, bead_id: str, worker_id: str):
        """Release bead lock."""

    def cleanup_expired_locks(self):
        """Remove stale locks."""
```

**2.2 Dependency Scheduler**
```python
# src/scheduler/scheduler.py

class DependencyScheduler:
    def __init__(self, beads: List[Bead]):
        self.beads = beads
        self.graph = self._build_dependency_graph()

    def get_ready_beads(self, worker_count: int) -> List[Bead]:
        """Return beads ready for execution."""

    def mark_completed(self, bead_id: str):
        """Update dependency graph on completion."""

    def detect_cycles(self) -> List[List[str]]:
        """Detect dependency cycles."""
```

**2.3 File Conflict Predictor**
```python
# src/scheduler/file_predictor.py

class FileConflictPredictor:
    def predict_files(self, bead: Bead) -> Set[str]:
        """Predict which files bead will modify."""

    def check_conflicts(
        self,
        file_set_1: Set[str],
        file_set_2: Set[str]
    ) -> Set[str]:
        """Find overlapping files."""
```

**Deliverables:**
- [ ] SQLite lock manager with bead + file locks
- [ ] Dependency graph builder and topological sort
- [ ] File prediction using regex and patterns
- [ ] Integration tests for concurrent workers

**Success Criteria:**
- 5 workers can run independent beads in parallel
- Dependent beads execute in correct order
- File conflicts detected and prevent lock acquisition
- No deadlocks in 100+ test runs

---

### Week 3: Basic Health Monitoring

#### Objectives
- Implement liveness and activity monitoring
- Build resource usage tracker
- Create simple alerting system

#### Tasks

**3.1 Liveness Monitor**
```python
# src/monitoring/liveness.py

class LivenessMonitor:
    def check_liveness(self, worker: Worker) -> LivenessStatus:
        """Check if worker is alive and responsive."""

    def start_heartbeat_monitor(self, worker: Worker):
        """Start background heartbeat checker."""
```

**3.2 Activity Monitor**
```python
# src/monitoring/activity.py

class ActivityMonitor:
    def track_activity(self, worker: Worker) -> ActivityMetrics:
        """Monitor worker progress indicators."""

    def detect_stuck(self, worker: Worker) -> float:
        """Calculate stuck probability (0-1)."""
```

**3.3 Resource Monitor**
```python
# src/monitoring/resources.py

class ResourceMonitor:
    def track_resources(self, worker: Worker) -> ResourceMetrics:
        """Monitor CPU, memory, disk, network."""

    def detect_leaks(self, worker: Worker) -> List[str]:
        """Detect resource leaks."""
```

**Deliverables:**
- [ ] Liveness checker with heartbeat validation
- [ ] Activity tracker monitoring logs and file changes
- [ ] Resource tracker using psutil
- [ ] SQLite metrics storage
- [ ] Basic alert rules (worker dead, stuck, resource leak)

**Success Criteria:**
- Detect worker failure within 30 seconds
- Identify stuck workers with 90%+ accuracy
- Track resource usage with <1% overhead
- Generate alerts for critical conditions

---

## Phase 2: Recovery and Reliability (Weeks 4-6)

### Week 4: Auto-Recovery Mechanisms

#### Objectives
- Implement graceful restart with state preservation
- Build exponential backoff system
- Create lock cleanup on failure

#### Tasks

**4.1 Recovery Manager**
```python
# src/recovery/manager.py

class RecoveryManager:
    def restart_worker(
        self,
        worker: Worker,
        reason: str,
        preserve_state: bool = True
    ) -> Worker:
        """Restart worker with state preservation."""

    def cleanup_worker(self, worker: Worker):
        """Clean up resources after failure."""
```

**4.2 State Preservation**
```python
# src/recovery/state.py

class StateManager:
    def capture_state(self, worker: Worker) -> dict:
        """Save worker state for recovery."""

    def restore_state(self, worker: Worker, state: dict):
        """Restore worker from saved state."""
```

**4.3 Backoff Manager**
```python
# src/recovery/backoff.py

class BackoffManager:
    def should_restart(
        self,
        worker_id: str,
        failure_type: str
    ) -> tuple[bool, int]:
        """Determine if should restart and wait time."""

    def reset_failure_count(self, worker_id: str):
        """Reset after successful operation."""
```

**Deliverables:**
- [ ] Graceful restart preserving bead progress
- [ ] Exponential backoff (capped at 1 hour)
- [ ] Lock cleanup on worker failure
- [ ] State serialization and restoration
- [ ] Recovery success metrics

**Success Criteria:**
- 95%+ recovery success rate for transient failures
- Average recovery time < 2 minutes
- No lock leaks after 100 worker failures
- Preserved state allows continuation from 80%+ completion point

---

### Week 5: Model Fallback and Circuit Breaker

#### Objectives
- Implement model fallback chains
- Build circuit breaker for API endpoints
- Add rate limit compliance monitoring

#### Tasks

**5.1 Fallback Manager**
```python
# src/recovery/fallback.py

class ModelFallbackManager:
    def get_next_model(
        self,
        worker: Worker,
        failure_reason: str
    ) -> dict:
        """Get next model in fallback chain."""

    def create_fallback_worker(
        self,
        worker: Worker,
        next_model: dict
    ) -> Worker:
        """Create worker with fallback model."""
```

**5.2 Circuit Breaker**
```python
# src/api/circuit_breaker.py

class CircuitBreaker:
    def call(self, func, *args, **kwargs):
        """Execute function through circuit breaker."""

    def get_state(self) -> dict:
        """Get circuit breaker state."""
```

**5.3 Rate Limit Monitor**
```python
# src/monitoring/rate_limits.py

class RateLimitMonitor:
    def track_api_call(self, worker: Worker, request: dict):
        """Track API call against rate limits."""

    def get_throttle_delay(self, worker: Worker) -> int:
        """Calculate delay to stay under limits."""
```

**Deliverables:**
- [ ] Fallback chains: Opus → Sonnet → GLM
- [ ] Circuit breaker with CLOSED/OPEN/HALF_OPEN states
- [ ] Rate limit tracking from API response headers
- [ ] Automatic throttling when approaching limits
- [ ] Cost tracking per worker and model

**Success Criteria:**
- Automatic fallback on model unavailability
- Circuit opens after 5 consecutive failures
- Stay under rate limits 99%+ of time
- Cost tracking accurate within 5%

---

### Week 6: Advanced Deadlock Detection

#### Objectives
- Implement wait-for graph cycle detection
- Build resource ordering prevention
- Create deadlock recovery with victim selection

#### Tasks

**6.1 Deadlock Detector**
```python
# src/locks/deadlock.py

class DeadlockDetector:
    def detect_cycle(self) -> List[List[str]]:
        """Detect cycles in wait-for graph."""

    def is_deadlocked(self) -> bool:
        """Check if system is deadlocked."""
```

**6.2 Resource Ordering**
```python
# src/locks/ordering.py

class ResourceOrderingPreventor:
    def acquire_locks_ordered(
        self,
        worker_id: str,
        resources: List[str]
    ) -> bool:
        """Acquire locks in global order."""
```

**6.3 Deadlock Recovery**
```python
# src/locks/recovery.py

class DeadlockRecovery:
    def select_victim(self, cycle: List[str]) -> str:
        """Choose worker to abort."""

    def abort_worker(self, worker_id: str):
        """Abort worker to break deadlock."""
```

**Deliverables:**
- [ ] Wait-for graph with DFS cycle detection
- [ ] Resource ordering to prevent circular wait
- [ ] Victim selection algorithm (least work lost)
- [ ] Automatic deadlock resolution
- [ ] Deadlock occurrence metrics

**Success Criteria:**
- Detect deadlocks within 10 seconds
- Deadlock occurrence rate < 1% of runs
- 100% deadlock resolution without manual intervention
- Minimal work lost (victim has <20% progress)

---

## Phase 3: Dashboard and Optimization (Weeks 7-9)

### Week 7: TUI Dashboard

#### Objectives
- Build real-time monitoring dashboard
- Implement worker distribution visualization
- Add interactive controls

#### Tasks

**7.1 Dashboard Layout**
```
┌─────────────────────────────────────────────────────────────┐
│ Control Panel Dashboard                    [P0: 3] [P1: 5] │
├─────────────────────────────────────────────────────────────┤
│ Workers [8/12 active]                                       │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ worker-1 [Opus]   │ po-2ug │ 45% │████████░░ │ $2.34    │ │
│ │ worker-2 [Sonnet] │ po-3h3 │ 78% │███████████ │ $0.89   │ │
│ │ worker-3 [GLM]    │ IDLE   │  -  │           │ $0.00    │ │
│ └─────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│ Metrics (Last Hour)                                         │
│ Throughput: 12 beads/hr  │  Success: 95%  │  Errors: 3     │
│ Avg Time: 245s           │  Cost: $8.45   │  Tokens: 125K  │
├─────────────────────────────────────────────────────────────┤
│ Ready Beads [5]                                             │
│ • po-1to - Compare API pricing [P0]                         │
│ • po-3pv - Design task scoring [P1]                         │
│ • po-1x9 - Prototype TUI [P2]                               │
├─────────────────────────────────────────────────────────────┤
│ Recent Alerts [2]                                           │
│ ⚠ worker-5 high memory usage (92%)          10m ago         │
│ ℹ worker-3 idle for 15 minutes              15m ago         │
└─────────────────────────────────────────────────────────────┘
```

**7.2 Implementation**
- Use `rich` library for TUI
- Real-time updates every 2 seconds
- Interactive mode (pause/resume workers, view logs)
- Export metrics to CSV/JSON

**Deliverables:**
- [ ] TUI dashboard with worker overview
- [ ] Real-time metrics display
- [ ] Progress bars for active beads
- [ ] Alert notifications in UI
- [ ] Keyboard shortcuts (p=pause, r=resume, q=quit)

**Success Criteria:**
- Dashboard updates with <500ms latency
- Handles 20+ workers without performance degradation
- Interactive controls work reliably
- Visually appealing and informative

---

### Week 8: Worker Count Optimization

#### Objectives
- Build benchmark framework
- Run experiments with varying worker counts
- Generate optimization recommendations

#### Tasks

**8.1 Benchmark Framework**
```python
# src/benchmarks/runner.py

class WorkerBenchmark:
    def run_benchmark(
        self,
        worker_counts: List[int],
        iterations: int = 3
    ) -> dict:
        """Run benchmark with different worker counts."""

    def analyze_results(self) -> dict:
        """Analyze and recommend optimal count."""
```

**8.2 Experiments**
- Small workspace (50 beads, 100 files)
- Medium workspace (200 beads, 500 files)
- Large workspace (500 beads, 1000+ files)
- Vary dependency density (10%, 50%, 80%)

**8.3 Analysis**
- Throughput vs worker count
- Lock contention vs worker count
- Cost efficiency vs worker count
- Optimal count by workspace size

**Deliverables:**
- [ ] Automated benchmark suite
- [ ] Results for 3 workspace sizes
- [ ] Analysis report with recommendations
- [ ] Optimization heuristics integrated into scheduler

**Success Criteria:**
- Identify optimal worker count for each workspace size
- Provide 5x throughput improvement for large workspaces
- Lock contention stays below 10% at optimal count
- Cost per bead reduced by 20%+ through optimization

---

### Week 9: Production Hardening

#### Objectives
- Add comprehensive error handling
- Implement crash recovery
- Build operational runbooks
- Performance tuning

#### Tasks

**9.1 Error Handling**
- Catch all exception types
- Graceful degradation
- Detailed error context
- Automatic bug reports

**9.2 Crash Recovery**
- Persist worker state to disk every 60s
- Resume from last checkpoint on restart
- Rollback incomplete changes
- Notify on data loss risk

**9.3 Operational Runbooks**
- Setup guide
- Troubleshooting guide
- Performance tuning guide
- Backup and recovery procedures

**9.4 Performance Tuning**
- Profile critical paths
- Optimize database queries
- Cache dependency graphs
- Batch metrics collection

**Deliverables:**
- [ ] Exception handling in all worker operations
- [ ] Checkpoint/restore mechanism
- [ ] 3 operational runbooks (setup, troubleshooting, tuning)
- [ ] Performance improvements (20%+ faster scheduler)
- [ ] Load testing results (100 workers, 1000 beads)

**Success Criteria:**
- Zero unhandled exceptions in 24-hour test
- Recover from crash with <1% data loss
- Complete documentation for operations
- Handle 100+ workers on single machine
- Scheduler latency <100ms for 1000 beads

---

## Phase 4: Distributed Scaling (Weeks 10-12)

### Week 10: Redis Migration

#### Objectives
- Migrate from SQLite to Redis for locks
- Implement Redlock algorithm
- Support cross-machine workers

#### Tasks

**10.1 Redis Lock Manager**
```python
# src/locks/redis_manager.py

class RedisLockManager:
    def acquire_lock(
        self,
        bead_id: str,
        worker_id: str,
        ttl: int = 1800
    ) -> bool:
        """Acquire lock using Redlock."""
```

**10.2 Migration Script**
- Export existing locks from SQLite
- Import into Redis
- Validation and verification

**10.3 Multi-Machine Support**
- Network-based worker communication
- Shared workspace via NFS/S3
- Distributed metrics collection

**Deliverables:**
- [ ] Redis lock manager with Redlock
- [ ] Migration tool (SQLite → Redis)
- [ ] Multi-machine worker support
- [ ] Network partition handling
- [ ] Benchmark comparison (SQLite vs Redis)

**Success Criteria:**
- Redis lock ops 10x faster than SQLite
- Support workers across 3+ machines
- Handle network partitions gracefully
- Zero lock corruption in failover scenarios

---

### Week 11: Metrics and Observability

#### Objectives
- Migrate to TimescaleDB for metrics
- Build Grafana dashboards
- Set up Prometheus exporters

#### Tasks

**11.1 TimescaleDB Integration**
```python
# src/metrics/timescale.py

class TimeScaleMetrics:
    def store_metrics(self, metrics: dict):
        """Store metrics in TimescaleDB."""

    def query_metrics(
        self,
        worker_id: str,
        metric_name: str,
        since: datetime
    ) -> List[dict]:
        """Query historical metrics."""
```

**11.2 Prometheus Exporter**
```python
# src/metrics/prometheus.py

class PoolOptimizerExporter:
    def export_metrics(self) -> str:
        """Export metrics in Prometheus format."""
```

**11.3 Grafana Dashboards**
- Worker overview dashboard
- Health metrics dashboard
- Cost and usage dashboard
- Alert history dashboard

**Deliverables:**
- [ ] TimescaleDB schema and integration
- [ ] Prometheus exporter endpoint
- [ ] 4 Grafana dashboards
- [ ] Historical metric retention (30 days)
- [ ] Query API for metrics

**Success Criteria:**
- Store 1M+ metric points without performance degradation
- Prometheus scrape completes in <1s
- Grafana dashboards load in <2s
- Historical queries complete in <500ms

---

### Week 12: Cloud Deployment

#### Objectives
- Package for Kubernetes deployment
- Build Helm charts
- Deploy to production cluster

#### Tasks

**12.1 Docker Images**
```dockerfile
# Dockerfile
FROM python:3.11-slim
WORKDIR /app
COPY requirements.txt .
RUN pip install -r requirements.txt
COPY src/ ./src/
CMD ["python", "-m", "src.main"]
```

**12.2 Helm Chart**
```yaml
# helm/control-panel/values.yaml
replicaCount: 1
workers:
  maxCount: 20
  models:
    - name: opus
      count: 5
    - name: sonnet
      count: 10
    - name: glm
      count: 5
redis:
  enabled: true
  cluster:
    nodes: 3
timescaledb:
  enabled: true
```

**12.3 Kubernetes Manifests**
- Deployment for orchestrator
- StatefulSet for workers
- Services for Redis and TimescaleDB
- ConfigMaps for configuration
- Secrets for API keys

**Deliverables:**
- [ ] Docker images for orchestrator and workers
- [ ] Helm chart with all components
- [ ] Kubernetes deployment tested on cluster
- [ ] Auto-scaling policies (HPA)
- [ ] Production deployment guide

**Success Criteria:**
- Deploy to Kubernetes in <10 minutes
- Auto-scale from 5 to 50 workers based on queue depth
- Rolling updates with zero downtime
- Monitor with Prometheus + Grafana
- Production-ready with HA configuration

---

## Testing Strategy

### Unit Tests (Throughout)
- Target: 80%+ code coverage
- Focus: Individual components (lock manager, scheduler, monitors)
- Framework: pytest
- Run: On every commit

### Integration Tests (Phase 1-2)
- Target: Critical paths covered
- Focus: Multi-component interactions
- Scenarios: Concurrent workers, lock contention, failure recovery
- Run: On PR merge

### Load Tests (Phase 3)
- Target: Performance benchmarks
- Focus: Scalability limits
- Scenarios: 10, 50, 100 workers; 100, 500, 1000 beads
- Run: Weekly

### Chaos Tests (Phase 4)
- Target: Resilience validation
- Focus: Failure scenarios
- Scenarios: Worker crashes, network partitions, Redis failures
- Run: Before production deployment

---

## Risk Mitigation

### High-Risk Areas

**1. Deadlock Bugs**
- Mitigation: Extensive testing with random workloads
- Fallback: Timeout-based detection and recovery
- Monitoring: Alert on deadlock detection

**2. Lock Corruption**
- Mitigation: Use proven libraries (Redis, etcd)
- Fallback: Periodic lock audits and cleanup
- Monitoring: Track lock count vs expected

**3. State Loss on Crash**
- Mitigation: Frequent checkpointing (every 60s)
- Fallback: Replay from git history
- Monitoring: Alert on incomplete state recovery

**4. Cost Overruns**
- Mitigation: Hard limits on daily spend
- Fallback: Auto-pause workers at threshold
- Monitoring: Real-time cost tracking

**5. Network Partitions**
- Mitigation: Use distributed consensus (etcd)
- Fallback: Split-brain detection and resolution
- Monitoring: Track network health

---

## Success Metrics

### Phase 1 (Foundation)
- ✓ 3+ workers running concurrently
- ✓ 90%+ lock acquisition success
- ✓ Zero deadlocks in 100 test runs
- ✓ Worker failure detection <30s

### Phase 2 (Recovery)
- ✓ 95%+ auto-recovery success
- ✓ Average recovery time <2 minutes
- ✓ Zero lock leaks after failures
- ✓ Circuit breaker prevents 99%+ cascading failures

### Phase 3 (Dashboard)
- ✓ Dashboard updates <500ms latency
- ✓ 5x throughput for large workspaces
- ✓ 20%+ cost reduction through optimization
- ✓ Handle 100+ workers on single machine

### Phase 4 (Distributed)
- ✓ Support workers across 3+ machines
- ✓ Redis lock ops 10x faster than SQLite
- ✓ Zero downtime deployments
- ✓ Auto-scale 5 to 50 workers based on load

---

## Dependencies

### External Libraries
```
# Core
python >= 3.11
sqlite3 (stdlib)
psutil >= 5.9
redis >= 5.0
etcd3 >= 0.12

# Monitoring
prometheus-client >= 0.19
psycopg2 >= 2.9  # TimescaleDB

# Dashboard
rich >= 13.0
textual >= 0.50

# Testing
pytest >= 7.4
pytest-asyncio >= 0.21
pytest-timeout >= 2.2

# Utilities
pyyaml >= 6.0
jsonschema >= 4.20
click >= 8.1
```

### External Services
- Redis (for distributed locks)
- PostgreSQL + TimescaleDB (for metrics)
- Prometheus (for monitoring)
- Grafana (for dashboards)

---

## Maintenance Plan

### Weekly
- Review error logs
- Check health metrics
- Rotate API keys if needed
- Update cost estimates

### Monthly
- Analyze performance trends
- Optimize slow queries
- Update dependencies
- Review security patches

### Quarterly
- Benchmark new LLM models
- Re-evaluate fallback chains
- Capacity planning
- Disaster recovery drills

---

## Rollout Strategy

### Stage 1: Internal Alpha (Week 3)
- Deploy to single workspace
- 3 workers (Opus, Sonnet, GLM)
- Manual testing and iteration

### Stage 2: Beta (Week 6)
- Deploy to 3 workspaces
- 10 workers total
- Gather feedback and metrics

### Stage 3: Limited Production (Week 9)
- Deploy to 10 workspaces
- 30 workers total
- Monitor for 1 week

### Stage 4: Full Production (Week 12)
- Deploy to all workspaces
- Auto-scaling enabled
- 24/7 monitoring active

---

## Team and Resources

### Required Roles
- **Lead Engineer (1):** Overall architecture and implementation
- **Backend Engineer (1):** Lock manager, scheduler, recovery
- **DevOps Engineer (0.5):** Kubernetes, monitoring, deployment
- **QA Engineer (0.5):** Testing, benchmarks, chaos engineering

### Total Effort
- 12 weeks × (1 + 1 + 0.5 + 0.5) FTE = 36 person-weeks
- Roughly 3 engineers full-time for 3 months

### Budget Estimate
- Engineering: $200K (3 months × 3 engineers)
- Infrastructure: $5K/month (Redis, TimescaleDB, monitoring)
- API costs: $1K/month (testing and benchmarking)
- **Total:** ~$220K

---

## Conclusion

This implementation plan provides a clear roadmap from basic multi-worker coordination to a production-ready distributed system. By following this phased approach, we can:

1. **Deliver value early** (Phase 1: basic parallelism)
2. **Build reliability** (Phase 2: auto-recovery)
3. **Optimize performance** (Phase 3: benchmarking and tuning)
4. **Scale to production** (Phase 4: distributed deployment)

The plan balances ambition with pragmatism, starting with SQLite and progressing to Redis/etcd only when needed. Each phase has clear success criteria and deliverables, making progress measurable and risks manageable.
