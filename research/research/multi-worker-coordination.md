# Multi-Worker Coordination Research

## Executive Summary

Current limitation: One worker per workspace creates bottlenecks for large projects with independent beads. This research proposes bead-level locking, dependency-aware scheduling, and file-level conflict detection to enable parallel worker execution.

## 1. Current State Analysis

### Limitations of Workspace-Level Locking

**Problem:**
- Entire workspace locked by single worker
- Independent beads cannot execute in parallel
- Worker utilization inefficient for large workspaces
- Long-running tasks block all other work

**Impact:**
- Reduced throughput for multi-bead projects
- Poor resource utilization
- Increased time-to-completion for complex tasks

## 2. Bead-Level Locking Architecture

### 2.1 Lock Granularity Strategy

**Hierarchy of Locks:**
```
Workspace (optional read/write mode)
  ├── Bead (exclusive lock per worker)
  │     ├── File Set (tracked per bead)
  │     └── Dependency Chain (topological ordering)
  └── Global Resources
        ├── Git Repository (serialized operations)
        └── External APIs (rate-limited pool)
```

### 2.2 Lock Types

#### Bead Execution Lock
- **Type:** Exclusive per bead ID
- **Scope:** Single bead + declared file dependencies
- **Duration:** Task lifecycle (claim → complete/fail)
- **Timeout:** Configurable (default: 30 minutes)

#### File Access Lock
- **Type:** Reader-writer lock per file path
- **Scope:** Individual files or directories
- **Duration:** File operation duration
- **Conflict Resolution:** Automatic merge for compatible changes

#### Git Operations Lock
- **Type:** Serialized queue
- **Scope:** Workspace-wide git commands
- **Duration:** Single git operation
- **Coordination:** Sequential commits with automatic rebase

### 2.3 Lock Implementation Options

#### Option A: File-Based Locking (SQLite)

**Pros:**
- No external dependencies
- ACID guarantees via SQLite
- Simple to implement and debug
- Works with existing beads database

**Cons:**
- Network filesystem challenges (NFS, CIFS)
- Limited to single machine without distributed FS
- Lock contention on high-frequency operations

**Implementation:**
```sql
-- Lock table schema
CREATE TABLE bead_locks (
    bead_id TEXT PRIMARY KEY,
    worker_id TEXT NOT NULL,
    acquired_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP NOT NULL,
    file_patterns TEXT, -- JSON array of file globs
    UNIQUE(bead_id)
);

CREATE INDEX idx_worker_locks ON bead_locks(worker_id);
CREATE INDEX idx_expiry ON bead_locks(expires_at);

-- File lock tracking
CREATE TABLE file_locks (
    file_path TEXT,
    bead_id TEXT,
    lock_mode TEXT CHECK(lock_mode IN ('read', 'write')),
    acquired_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (file_path, bead_id),
    FOREIGN KEY (bead_id) REFERENCES bead_locks(bead_id)
);
```

**Lock Acquisition Algorithm:**
```python
def acquire_bead_lock(bead_id: str, worker_id: str, file_patterns: list[str]) -> bool:
    """
    Acquire exclusive lock on bead and check file conflicts.
    """
    with sqlite_transaction():
        # Clean expired locks
        execute("DELETE FROM bead_locks WHERE expires_at < ?", [now()])

        # Check if bead already locked
        existing = query_one("SELECT worker_id FROM bead_locks WHERE bead_id = ?", [bead_id])
        if existing and existing['worker_id'] != worker_id:
            return False

        # Check file conflicts
        conflicting_files = query_all("""
            SELECT DISTINCT fl.file_path, bl.worker_id
            FROM file_locks fl
            JOIN bead_locks bl ON fl.bead_id = bl.bead_id
            WHERE fl.file_path IN (?) AND fl.lock_mode = 'write'
        """, [expand_globs(file_patterns)])

        if conflicting_files:
            return False

        # Acquire locks
        expires = now() + timedelta(minutes=30)
        execute("""
            INSERT INTO bead_locks (bead_id, worker_id, expires_at, file_patterns)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(bead_id) DO UPDATE SET
                worker_id = excluded.worker_id,
                acquired_at = CURRENT_TIMESTAMP,
                expires_at = excluded.expires_at
        """, [bead_id, worker_id, expires, json.dumps(file_patterns)])

        # Register file locks
        for pattern in file_patterns:
            for file_path in expand_glob(pattern):
                execute("""
                    INSERT INTO file_locks (file_path, bead_id, lock_mode)
                    VALUES (?, ?, 'write')
                """, [file_path, bead_id])

        return True
```

#### Option B: Redis Distributed Locking (Redlock Algorithm)

**Pros:**
- True distributed locking across machines
- High performance (in-memory)
- Built-in TTL expiration
- Atomic operations with Lua scripting

**Cons:**
- External dependency (Redis cluster)
- Network latency for lock operations
- Requires Redis cluster for fault tolerance
- More complex operational overhead

**Implementation:**
```python
import redis
from datetime import timedelta

class RedisLockManager:
    def __init__(self, redis_nodes: list[str]):
        self.clients = [redis.Redis.from_url(url) for url in redis_nodes]
        self.quorum = len(self.clients) // 2 + 1

    def acquire_bead_lock(self, bead_id: str, worker_id: str, ttl: int = 1800) -> bool:
        """
        Redlock algorithm implementation for bead locking.
        """
        lock_key = f"bead:lock:{bead_id}"
        lock_value = f"{worker_id}:{uuid4()}"

        # Try to acquire lock on majority of nodes
        acquired = 0
        start_time = time.time()

        for client in self.clients:
            try:
                # SET NX EX: atomic set-if-not-exists with expiry
                if client.set(lock_key, lock_value, nx=True, ex=ttl):
                    acquired += 1
            except redis.RedisError:
                continue

        # Check if we got quorum
        validity_time = ttl - (time.time() - start_time) - 100  # drift compensation

        if acquired >= self.quorum and validity_time > 0:
            return True
        else:
            # Failed to acquire, release partial locks
            self._release_lock(lock_key, lock_value)
            return False

    def check_file_conflicts(self, file_patterns: list[str]) -> list[str]:
        """
        Check if any files are locked by other beads.
        """
        conflicts = []
        for pattern in file_patterns:
            for file_path in expand_glob(pattern):
                lock_key = f"file:lock:{file_path}"
                for client in self.clients:
                    if client.exists(lock_key):
                        owner = client.get(lock_key)
                        if owner:
                            conflicts.append(f"{file_path} locked by {owner.decode()}")
                        break
        return conflicts
```

#### Option C: etcd Distributed Coordination

**Pros:**
- Strong consistency guarantees (Raft consensus)
- Built-in distributed locking with leases
- Watch API for real-time lock monitoring
- Production-grade reliability (used by Kubernetes)

**Cons:**
- Heavyweight for simple use cases
- Requires etcd cluster deployment
- Higher operational complexity
- Slower than Redis for simple locks

**Implementation:**
```python
import etcd3

class EtcdLockManager:
    def __init__(self, endpoints: list[str]):
        self.client = etcd3.client(host=endpoints[0].split(':')[0],
                                   port=int(endpoints[0].split(':')[1]))

    def acquire_bead_lock(self, bead_id: str, worker_id: str, ttl: int = 1800) -> tuple[bool, object]:
        """
        Acquire distributed lock using etcd lease and transactions.
        """
        lock_key = f"/beads/locks/{bead_id}"

        # Create lease with TTL
        lease = self.client.lease(ttl)

        # Atomic compare-and-set transaction
        success = self.client.transaction(
            compare=[
                self.client.transactions.create(lock_key) == 0  # Key doesn't exist
            ],
            success=[
                self.client.transactions.put(lock_key, worker_id, lease=lease)
            ],
            failure=[]
        )

        if success.succeeded:
            return True, lease

        return False, None

    def watch_lock_changes(self, callback):
        """
        Watch for lock changes in real-time.
        """
        watch_id = self.client.add_watch_prefix_callback("/beads/locks/", callback)
        return watch_id
```

### 2.4 Recommended Approach: Hybrid Model

**Strategy:** Start with SQLite, migrate to Redis/etcd when scaling

**Phase 1: Local Development (SQLite)**
- Use SQLite for bead and file locks
- Implement full locking logic and deadlock detection
- Optimize for single-machine multi-worker scenarios

**Phase 2: Distributed Teams (Redis)**
- Migrate to Redis Redlock for cross-machine locking
- Keep SQLite as fallback for offline work
- Add network partition handling

**Phase 3: Production Scale (etcd)**
- Deploy etcd cluster for enterprise deployments
- Leverage watch API for real-time coordination
- Integrate with Kubernetes for cloud-native deployments

## 3. Dependency-Aware Scheduling

### 3.1 Task Scheduling Algorithm

**Goal:** Maximize parallelism while respecting dependencies

**Algorithm: Priority Queue with Topological Sort**

```python
from collections import defaultdict, deque
from typing import Dict, List, Set
import heapq

class DependencyScheduler:
    def __init__(self, beads: List[Bead]):
        self.beads = {b.id: b for b in beads}
        self.dependency_graph = self._build_graph()
        self.in_degree = self._compute_in_degree()

    def _build_graph(self) -> Dict[str, List[str]]:
        """Build dependency graph from bead relationships."""
        graph = defaultdict(list)
        for bead in self.beads.values():
            for dep in bead.depends_on:
                graph[dep].append(bead.id)
        return graph

    def _compute_in_degree(self) -> Dict[str, int]:
        """Count incoming edges for each bead."""
        in_degree = {bid: 0 for bid in self.beads}
        for dependents in self.dependency_graph.values():
            for dependent in dependents:
                in_degree[dependent] += 1
        return in_degree

    def get_ready_beads(self, worker_count: int) -> List[Bead]:
        """
        Get beads ready for execution (no unresolved dependencies).
        Returns up to worker_count beads prioritized by:
        1. Priority (P0 > P1 > P2)
        2. Estimated duration (shorter first for quick wins)
        3. Creation time (older first)
        """
        ready = []

        for bead_id, degree in self.in_degree.items():
            if degree == 0 and self.beads[bead_id].status == 'open':
                bead = self.beads[bead_id]
                # Priority tuple: (priority, estimated_duration, -creation_timestamp)
                priority_score = (
                    bead.priority_value(),
                    bead.estimated_duration or 600,
                    -bead.created_at.timestamp()
                )
                heapq.heappush(ready, (priority_score, bead))

        # Return top N beads
        return [heapq.heappop(ready)[1] for _ in range(min(worker_count, len(ready)))]

    def mark_completed(self, bead_id: str):
        """Update graph when bead completes."""
        if bead_id in self.dependency_graph:
            for dependent_id in self.dependency_graph[bead_id]:
                self.in_degree[dependent_id] -= 1

    def detect_cycles(self) -> List[List[str]]:
        """Detect dependency cycles using DFS."""
        cycles = []
        visited = set()
        rec_stack = set()

        def dfs(node: str, path: List[str]):
            visited.add(node)
            rec_stack.add(node)
            path.append(node)

            for neighbor in self.dependency_graph.get(node, []):
                if neighbor not in visited:
                    dfs(neighbor, path[:])
                elif neighbor in rec_stack:
                    # Found cycle
                    cycle_start = path.index(neighbor)
                    cycles.append(path[cycle_start:] + [neighbor])

            rec_stack.remove(node)

        for bead_id in self.beads:
            if bead_id not in visited:
                dfs(bead_id, [])

        return cycles
```

### 3.2 Worker Assignment Strategy

**Factors for Assignment:**
1. **Bead characteristics:**
   - Estimated duration (short vs. long)
   - File scope (localized vs. broad)
   - Resource requirements (CPU, memory)

2. **Worker capabilities:**
   - Model type (GPT-4, Claude, GLM)
   - Cost per token
   - Rate limits and quotas

3. **Load balancing:**
   - Current worker utilization
   - Queue depth per worker
   - Historical completion rates

**Assignment Algorithm:**

```python
class WorkerAssigner:
    def assign_bead_to_worker(self, bead: Bead, workers: List[Worker]) -> Worker:
        """
        Assign bead to optimal worker based on multiple factors.
        """
        scores = []

        for worker in workers:
            if not worker.is_available():
                continue

            score = 0

            # Factor 1: Model capability match
            if bead.requires_high_capability() and worker.is_premium_model():
                score += 50
            elif not bead.requires_high_capability() and worker.is_economy_model():
                score += 30

            # Factor 2: Current load (prefer less loaded workers)
            utilization = worker.current_load() / worker.capacity()
            score += (1 - utilization) * 30

            # Factor 3: Cost efficiency
            estimated_tokens = bead.estimated_tokens or 10000
            cost = worker.cost_per_token * estimated_tokens
            score -= cost * 10  # Penalize expensive workers

            # Factor 4: Historical success rate
            success_rate = worker.get_success_rate_for_type(bead.type)
            score += success_rate * 20

            # Factor 5: Avoid context switching (same workspace)
            if worker.current_workspace == bead.workspace:
                score += 15

            scores.append((score, worker))

        if not scores:
            return None

        # Return highest scoring worker
        scores.sort(key=lambda x: x[0], reverse=True)
        return scores[0][1]
```

## 4. File-Level Conflict Detection

### 4.1 Static Analysis Approach

**Goal:** Predict file modifications before execution

**Method 1: Pattern-Based Prediction**

```python
class FileConflictPredictor:
    def predict_file_changes(self, bead: Bead) -> Set[str]:
        """
        Predict which files a bead will modify based on:
        - Bead description keywords
        - Historical patterns
        - Code analysis
        """
        predicted_files = set()

        # Extract file mentions from description
        file_pattern = r'`([^`]+\.(py|js|ts|yml|yaml|json))`'
        matches = re.findall(file_pattern, bead.description)
        predicted_files.update(m[0] for m in matches)

        # Keyword-based prediction
        keywords_map = {
            'test': ['test_*.py', '*_test.py', 'tests/**/*.py'],
            'config': ['*.yml', '*.yaml', '*.json', 'config/**'],
            'api': ['api/**/*.py', 'routes/*.py'],
            'database': ['models/*.py', 'migrations/*.py', 'schema.sql'],
        }

        description_lower = bead.description.lower()
        for keyword, patterns in keywords_map.items():
            if keyword in description_lower:
                for pattern in patterns:
                    predicted_files.update(glob.glob(pattern, recursive=True))

        # Historical pattern matching
        similar_beads = self._find_similar_beads(bead)
        for similar in similar_beads[:5]:  # Top 5 similar
            if similar.actual_files_changed:
                predicted_files.update(similar.actual_files_changed)

        return predicted_files

    def check_conflicts(self, bead1_files: Set[str], bead2_files: Set[str]) -> Set[str]:
        """
        Check if two beads have overlapping file modifications.
        """
        return bead1_files.intersection(bead2_files)
```

**Method 2: Git-Based Conflict Detection**

```python
class GitConflictDetector:
    def detect_merge_conflicts(self, branch1: str, branch2: str) -> List[str]:
        """
        Detect potential merge conflicts between branches.
        """
        # Dry-run merge to detect conflicts
        result = subprocess.run(
            ['git', 'merge-tree',
             subprocess.check_output(['git', 'merge-base', branch1, branch2]).strip(),
             branch1, branch2],
            capture_output=True, text=True
        )

        conflicts = []
        if result.returncode != 0 or '<<<<<<< ' in result.stdout:
            # Parse conflict markers
            for line in result.stdout.split('\n'):
                if line.startswith('CONFLICT'):
                    file_path = line.split()[-1]
                    conflicts.append(file_path)

        return conflicts

    def compute_file_edit_distance(self, file_path: str, branch1: str, branch2: str) -> int:
        """
        Compute edit distance between same file in two branches.
        Low distance = likely compatible changes.
        """
        content1 = self._get_file_at_ref(file_path, branch1)
        content2 = self._get_file_at_ref(file_path, branch2)

        # Use difflib for line-level distance
        matcher = difflib.SequenceMatcher(None, content1.splitlines(), content2.splitlines())
        return int((1 - matcher.ratio()) * 100)
```

### 4.2 Dynamic Conflict Resolution

**Strategy:** Detect conflicts during execution and auto-resolve when possible

```python
class ConflictResolver:
    def resolve_git_conflict(self, file_path: str, strategy: str = 'auto') -> bool:
        """
        Attempt automatic conflict resolution.

        Strategies:
        - auto: Use AI to merge intelligently
        - ours: Keep current branch changes
        - theirs: Accept incoming changes
        - manual: Flag for human review
        """
        if strategy == 'auto':
            return self._ai_merge(file_path)
        elif strategy == 'ours':
            subprocess.run(['git', 'checkout', '--ours', file_path], check=True)
            return True
        elif strategy == 'theirs':
            subprocess.run(['git', 'checkout', '--theirs', file_path], check=True)
            return True
        else:
            return False

    def _ai_merge(self, file_path: str) -> bool:
        """
        Use AI to intelligently merge conflicting changes.
        """
        # Read conflict markers
        with open(file_path) as f:
            content = f.read()

        if '<<<<<<< ' not in content:
            return True  # No conflict

        # Extract conflict sections
        conflicts = self._parse_conflict_markers(content)

        # Ask AI to resolve
        prompt = f"""
        Merge these conflicting changes intelligently:

        File: {file_path}

        Conflicts:
        {conflicts}

        Provide the merged result without conflict markers.
        """

        resolved = call_llm(prompt)

        # Write resolved content
        with open(file_path, 'w') as f:
            f.write(resolved)

        subprocess.run(['git', 'add', file_path], check=True)
        return True
```

## 5. Git Branch Isolation Strategies

### 5.1 Branch-Per-Worker Model

**Pattern:** Each worker operates on dedicated branch

```
main
  ├── worker-1/bead-abc
  ├── worker-2/bead-def
  └── worker-3/bead-ghi
```

**Pros:**
- Complete isolation between workers
- Easy to track changes per bead
- Can merge independently

**Cons:**
- Many branches to manage
- Potential merge conflicts at integration
- Git history becomes complex

**Implementation:**

```python
class BranchManager:
    def create_worker_branch(self, worker_id: str, bead_id: str) -> str:
        """
        Create isolated branch for worker.
        """
        branch_name = f"worker-{worker_id}/bead-{bead_id}"

        subprocess.run(['git', 'checkout', '-b', branch_name, 'main'], check=True)
        return branch_name

    def merge_worker_branch(self, branch_name: str, strategy: str = 'rebase') -> bool:
        """
        Merge worker branch back to main.
        """
        subprocess.run(['git', 'checkout', 'main'], check=True)
        subprocess.run(['git', 'pull', '--rebase'], check=True)

        if strategy == 'rebase':
            # Rebase worker branch onto latest main
            subprocess.run(['git', 'checkout', branch_name], check=True)
            result = subprocess.run(['git', 'rebase', 'main'], capture_output=True)

            if result.returncode != 0:
                # Conflict during rebase
                return False

            subprocess.run(['git', 'checkout', 'main'], check=True)
            subprocess.run(['git', 'merge', '--ff-only', branch_name], check=True)
        else:
            # Traditional merge
            result = subprocess.run(['git', 'merge', branch_name], capture_output=True)
            if result.returncode != 0:
                return False

        # Cleanup
        subprocess.run(['git', 'branch', '-d', branch_name], check=True)
        return True
```

### 5.2 Worktree-Per-Worker Model

**Pattern:** Use git worktrees for parallel checkouts

```
project/
  ├── .git/
  ├── main/          (main worktree)
  ├── worker-1/      (worktree for worker 1)
  ├── worker-2/      (worktree for worker 2)
  └── worker-3/      (worktree for worker 3)
```

**Pros:**
- True parallel work on same repo
- No workspace switching overhead
- Share .git database (efficient)

**Cons:**
- Disk space for multiple checkouts
- Complexity in managing worktrees
- Not suitable for cloud workers

**Implementation:**

```python
class WorktreeManager:
    def create_worktree(self, worker_id: str, bead_id: str, base_path: str) -> str:
        """
        Create git worktree for isolated worker environment.
        """
        branch_name = f"worker-{worker_id}/bead-{bead_id}"
        worktree_path = os.path.join(base_path, f"worker-{worker_id}")

        subprocess.run([
            'git', 'worktree', 'add',
            '-b', branch_name,
            worktree_path,
            'main'
        ], check=True)

        return worktree_path

    def remove_worktree(self, worktree_path: str):
        """
        Clean up worktree after worker completes.
        """
        subprocess.run(['git', 'worktree', 'remove', worktree_path], check=True)
```

### 5.3 Recommended Strategy: Hybrid Approach

**For small workspaces (<100 files):**
- Use branch-per-worker with aggressive rebasing
- Merge frequently to avoid divergence

**For large workspaces (100+ files):**
- Use worktrees for local workers
- Use branches for remote/cloud workers
- Implement file-level locking to reduce conflicts

## 6. Deadlock Detection and Resolution

### 6.1 Deadlock Detection Algorithm

**Method: Wait-For Graph Analysis**

```python
class DeadlockDetector:
    def __init__(self):
        self.wait_for_graph = defaultdict(set)  # worker -> set of workers it waits for
        self.lock_ownership = {}  # resource -> worker
        self.lock_requests = defaultdict(list)  # resource -> [workers waiting]

    def add_lock_request(self, worker_id: str, resource_id: str):
        """
        Record that worker is waiting for resource.
        """
        current_owner = self.lock_ownership.get(resource_id)

        if current_owner and current_owner != worker_id:
            # Worker must wait for current owner
            self.wait_for_graph[worker_id].add(current_owner)
            self.lock_requests[resource_id].append(worker_id)

    def detect_cycle(self) -> List[List[str]]:
        """
        Detect cycles in wait-for graph (indicates deadlock).
        Uses DFS with cycle detection.
        """
        visited = set()
        rec_stack = set()
        cycles = []

        def dfs(node: str, path: List[str]):
            visited.add(node)
            rec_stack.add(node)
            path.append(node)

            for neighbor in self.wait_for_graph[node]:
                if neighbor not in visited:
                    dfs(neighbor, path[:])
                elif neighbor in rec_stack:
                    # Cycle detected
                    cycle_start = path.index(neighbor)
                    cycles.append(path[cycle_start:] + [neighbor])

            rec_stack.remove(node)

        for worker in self.wait_for_graph:
            if worker not in visited:
                dfs(worker, [])

        return cycles

    def is_deadlocked(self) -> bool:
        """
        Check if system is currently deadlocked.
        """
        return len(self.detect_cycle()) > 0
```

### 6.2 Deadlock Resolution Strategies

**Strategy 1: Timeout-Based Prevention**

```python
class TimeoutBasedPreventor:
    def __init__(self, max_wait_time: int = 300):
        self.max_wait_time = max_wait_time
        self.lock_wait_times = {}  # (worker, resource) -> timestamp

    def wait_for_lock(self, worker_id: str, resource_id: str) -> bool:
        """
        Wait for lock with timeout. Return False if timeout exceeded.
        """
        wait_key = (worker_id, resource_id)
        start_time = self.lock_wait_times.get(wait_key, time.time())
        self.lock_wait_times[wait_key] = start_time

        while time.time() - start_time < self.max_wait_time:
            if self.try_acquire_lock(worker_id, resource_id):
                del self.lock_wait_times[wait_key]
                return True
            time.sleep(1)

        # Timeout exceeded
        del self.lock_wait_times[wait_key]
        return False
```

**Strategy 2: Resource Ordering (Prevention)**

```python
class ResourceOrderingPreventor:
    def __init__(self):
        self.resource_order = {}  # resource -> priority
        self.next_priority = 0

    def assign_resource_priority(self, resource_id: str) -> int:
        """
        Assign global ordering to resources.
        """
        if resource_id not in self.resource_order:
            self.resource_order[resource_id] = self.next_priority
            self.next_priority += 1
        return self.resource_order[resource_id]

    def acquire_locks_ordered(self, worker_id: str, resources: List[str]) -> bool:
        """
        Acquire multiple locks in consistent global order.
        Prevents circular wait condition.
        """
        # Sort resources by priority
        sorted_resources = sorted(resources,
                                 key=lambda r: self.assign_resource_priority(r))

        acquired = []
        try:
            for resource in sorted_resources:
                if not self.acquire_lock(worker_id, resource):
                    raise LockAcquisitionError(f"Failed to acquire {resource}")
                acquired.append(resource)
            return True
        except LockAcquisitionError:
            # Release all acquired locks in reverse order
            for resource in reversed(acquired):
                self.release_lock(worker_id, resource)
            return False
```

**Strategy 3: Deadlock Recovery (Victim Selection)**

```python
class DeadlockRecovery:
    def recover_from_deadlock(self, cycle: List[str]) -> str:
        """
        Select victim worker to abort and break deadlock.

        Selection criteria:
        1. Least amount of work completed
        2. Lowest priority bead
        3. Most recently started
        """
        victim_scores = []

        for worker_id in cycle:
            worker = self.workers[worker_id]
            score = 0

            # Factor 1: Work completed (prefer to kill workers who just started)
            score += worker.work_completed * -100

            # Factor 2: Bead priority (prefer to kill low-priority beads)
            priority_value = {'P0': 1000, 'P1': 500, 'P2': 100, 'P3': 10, 'P4': 1}
            score += priority_value.get(worker.current_bead.priority, 0)

            # Factor 3: Start time (prefer to kill recently started)
            time_running = time.time() - worker.start_time
            score += time_running * -1

            victim_scores.append((score, worker_id))

        # Select victim (lowest score)
        victim_scores.sort()
        victim_id = victim_scores[0][1]

        # Abort victim worker
        self.abort_worker(victim_id)

        return victim_id

    def abort_worker(self, worker_id: str):
        """
        Forcefully abort worker and release all its locks.
        """
        worker = self.workers[worker_id]

        # Release all locks
        for resource in worker.held_locks:
            self.release_lock(worker_id, resource)

        # Mark bead as failed/retry
        if worker.current_bead:
            worker.current_bead.status = 'open'
            worker.current_bead.retry_count += 1

        # Stop worker
        worker.stop()
```

## 7. Optimal Worker Count Benchmarking

### 7.1 Benchmark Methodology

**Variables to Test:**
1. Workspace size (files, lines of code)
2. Bead count and complexity
3. Dependency graph density
4. Worker model types
5. Hardware resources (CPU, memory, I/O)

**Metrics to Measure:**
1. Throughput (beads completed per hour)
2. Average bead completion time
3. Worker utilization (% time actively working)
4. Lock contention rate
5. Merge conflict rate
6. Total cost (API tokens consumed)

### 7.2 Benchmark Framework

```python
class WorkerBenchmark:
    def __init__(self, workspace_path: str):
        self.workspace_path = workspace_path
        self.results = []

    def run_benchmark(self, worker_counts: List[int], iterations: int = 3):
        """
        Run benchmark with different worker counts.
        """
        workspace_size = self._analyze_workspace()

        for worker_count in worker_counts:
            for iteration in range(iterations):
                result = self._run_iteration(worker_count, workspace_size)
                result['iteration'] = iteration
                self.results.append(result)

        return self._analyze_results()

    def _run_iteration(self, worker_count: int, workspace_size: dict) -> dict:
        """
        Single benchmark iteration.
        """
        start_time = time.time()

        # Create beads
        beads = self._create_test_beads(count=50)

        # Launch workers
        workers = [self._launch_worker(i) for i in range(worker_count)]

        # Monitor execution
        metrics = {
            'worker_count': worker_count,
            'workspace_size': workspace_size,
            'beads_completed': 0,
            'lock_contentions': 0,
            'merge_conflicts': 0,
            'total_tokens': 0,
        }

        completed_beads = []
        while len(completed_beads) < len(beads):
            for worker in workers:
                if worker.is_idle() and worker.current_bead:
                    completed_beads.append(worker.current_bead)
                    metrics['beads_completed'] += 1
                    metrics['total_tokens'] += worker.tokens_used

            metrics['lock_contentions'] += self._count_lock_contentions()
            time.sleep(1)

        metrics['duration'] = time.time() - start_time
        metrics['throughput'] = metrics['beads_completed'] / (metrics['duration'] / 3600)
        metrics['avg_completion_time'] = metrics['duration'] / metrics['beads_completed']
        metrics['worker_utilization'] = self._compute_utilization(workers)

        return metrics

    def _analyze_results(self) -> dict:
        """
        Analyze benchmark results and provide recommendations.
        """
        df = pd.DataFrame(self.results)

        # Group by worker count
        grouped = df.groupby('worker_count').agg({
            'throughput': 'mean',
            'avg_completion_time': 'mean',
            'worker_utilization': 'mean',
            'lock_contentions': 'mean',
            'merge_conflicts': 'mean',
        })

        # Find optimal worker count (maximize throughput, minimize conflicts)
        grouped['efficiency_score'] = (
            grouped['throughput'] * 100 -
            grouped['lock_contentions'] * 10 -
            grouped['merge_conflicts'] * 20
        )

        optimal_count = grouped['efficiency_score'].idxmax()

        return {
            'optimal_worker_count': optimal_count,
            'detailed_results': grouped.to_dict(),
            'recommendation': self._generate_recommendation(grouped, optimal_count)
        }

    def _generate_recommendation(self, results: pd.DataFrame, optimal: int) -> str:
        """
        Generate human-readable recommendation.
        """
        optimal_data = results.loc[optimal]

        return f"""
        Recommended Worker Count: {optimal}

        Expected Performance:
        - Throughput: {optimal_data['throughput']:.1f} beads/hour
        - Avg completion time: {optimal_data['avg_completion_time']:.1f} seconds
        - Worker utilization: {optimal_data['worker_utilization']:.1%}
        - Lock contention rate: {optimal_data['lock_contentions']:.1f} per hour
        - Merge conflict rate: {optimal_data['merge_conflicts']:.1f} per hour

        Scaling Guidelines:
        - Below {optimal} workers: Underutilized, add more workers
        - At {optimal} workers: Optimal balance
        - Above {optimal} workers: Diminishing returns, lock contention increases
        """
```

### 7.3 Expected Results by Workspace Size

**Small Workspace (< 100 files, < 10K LOC):**
- Optimal: 2-3 workers
- Reasoning: Limited parallelism, high lock contention with more workers

**Medium Workspace (100-500 files, 10K-50K LOC):**
- Optimal: 4-6 workers
- Reasoning: Good parallelism, manageable lock contention

**Large Workspace (500+ files, 50K+ LOC):**
- Optimal: 8-12 workers
- Reasoning: High parallelism, file conflicts are rare

**Factors that Increase Optimal Count:**
- Low dependency density (independent beads)
- File changes localized to modules
- Fast worker models (quick turnaround)

**Factors that Decrease Optimal Count:**
- High dependency density (serial work)
- Broad file changes (high conflict rate)
- Slow worker models (long-running tasks)

## 8. Implementation Roadmap

### Phase 1: Foundation (Weeks 1-2)
- [ ] Implement SQLite-based bead locking
- [ ] Build dependency graph and scheduler
- [ ] Create file conflict predictor
- [ ] Add deadlock detection

### Phase 2: Git Integration (Weeks 3-4)
- [ ] Implement branch-per-worker strategy
- [ ] Add automatic merge/rebase logic
- [ ] Build conflict resolver with AI merge
- [ ] Test with multiple workers

### Phase 3: Optimization (Weeks 5-6)
- [ ] Run worker count benchmarks
- [ ] Optimize lock acquisition performance
- [ ] Implement resource ordering
- [ ] Add metrics and monitoring

### Phase 4: Distributed Scaling (Weeks 7-8)
- [ ] Migrate to Redis/etcd for distributed locking
- [ ] Add cross-machine coordination
- [ ] Implement advanced deadlock recovery
- [ ] Load test with 10+ concurrent workers

## 9. Success Metrics

**Performance:**
- 5x throughput increase for independent beads
- < 1% deadlock occurrence rate
- < 5% merge conflict rate
- 90%+ worker utilization

**Reliability:**
- 99.9% uptime for locking service
- < 1s lock acquisition latency
- Zero data corruption incidents
- Automatic recovery from 95% of failures

**Cost:**
- 30% reduction in idle worker time
- 20% reduction in total API costs
- No increase in infrastructure costs

## 10. References and Prior Art

**Distributed Locking:**
- Redis Redlock Algorithm: https://redis.io/topics/distlock
- etcd distributed locks: https://etcd.io/docs/v3.5/dev-guide/api_concurrency_reference_v3/
- Google Chubby: Distributed lock service design

**Deadlock Detection:**
- Banker's Algorithm for deadlock avoidance
- Wait-For Graph cycle detection
- Database deadlock handling (PostgreSQL, MySQL)

**Git Workflows:**
- Git worktrees: https://git-scm.com/docs/git-worktree
- GitHub Flow branching model
- GitLab merge strategies

**Scheduling:**
- Kubernetes scheduler design
- Apache Airflow DAG execution
- Celery distributed task queue
