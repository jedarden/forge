# ADR 0007: Bead Integration Strategy

**Status**: Accepted
**Date**: 2026-02-07
**Deciders**: FORGE Architecture Team

---

## Context

FORGE needs to integrate with the `br` CLI (bead management system) to:
1. Display task queues from `.beads/*.jsonl` files
2. Assign tasks to workers
3. Track task progress and dependencies
4. Calculate task value scores for intelligent routing

The design gap analysis identified four missing specifications:
- How FORGE reads bead data
- Task assignment algorithm
- Task value scoring implementation
- Dependency resolution handling

---

## Decision

**FORGE reads `.beads/*.jsonl` files directly and provides task visibility, but delegates task management to `br` CLI and workers.**

### Integration Model

```
┌─────────────────────────────────────────────────┐
│  FORGE (Read-Only Bead Consumer)                │
│  - Reads .beads/*.jsonl files                   │
│  - Displays task queue in TUI                   │
│  - Shows dependency graph                       │
│  - Calculates task value scores                 │
│  - Suggests worker assignments                  │
│  └─────────────────────────────────────────────┘
           │                            ↑
           │ Reads                      │ Writes
           ↓                            │
┌─────────────────────┐    ┌───────────────────────┐
│  .beads/bd.jsonl    │    │  br CLI               │
│  (Source of Truth)  │←───│  (Write Authority)    │
└─────────────────────┘    └───────────────────────┘
                                      ↑
                                      │ Uses
                           ┌──────────┴──────────┐
                           │  Workers            │
                           │  (Update Status)    │
                           └─────────────────────┘
```

### Key Principles

1. **FORGE is read-only** - Never writes to `.beads/` files directly
2. **br CLI is write authority** - All bead mutations via `br` commands
3. **Workers are autonomous** - Decide what to work on, update via `br`
4. **FORGE provides visibility** - Dashboard shows task state, suggests actions

---

## Implementation Details

### 1. Bead Data Reading

**File Discovery:**
```python
def discover_bead_workspaces(workspace_root: Path) -> list[BeadWorkspace]:
    """Find all .beads/ directories in workspace"""
    workspaces = []

    # Check workspace root
    if (workspace_root / ".beads").exists():
        workspaces.append(parse_bead_workspace(workspace_root))

    # Check subdirectories (common pattern)
    for subdir in workspace_root.iterdir():
        if subdir.is_dir() and (subdir / ".beads").exists():
            workspaces.append(parse_bead_workspace(subdir))

    return workspaces
```

**JSONL Parsing:**
```python
import orjson  # Fast JSON parsing

def parse_bead_workspace(path: Path) -> BeadWorkspace:
    """Parse .beads/*.jsonl files"""
    beads_dir = path / ".beads"
    beads = []

    # Find JSONL file (usually matches prefix, e.g., bd.jsonl)
    jsonl_files = list(beads_dir.glob("*.jsonl"))

    for jsonl_file in jsonl_files:
        with open(jsonl_file, 'rb') as f:
            for line in f:
                if line.strip():
                    bead = orjson.loads(line)
                    beads.append(Bead.from_dict(bead))

    return BeadWorkspace(path=path, beads=beads)
```

**Bead Data Model:**
```python
@dataclass
class Bead:
    """Single bead/task"""
    id: str                    # e.g., "bd-abc"
    title: str                 # Short description
    type: str                  # task, bug, feature, epic
    status: str                # open, in_progress, closed
    priority: str              # P0-P4
    description: str           # Detailed explanation
    assignee: str | None       # Worker ID or None
    labels: list[str]          # Tags
    created_at: datetime
    updated_at: datetime
    closed_at: datetime | None

    # Dependencies
    depends_on: list[str]      # Blocks this bead
    blocks: list[str]          # This bead blocks these
    parent: str | None         # Parent epic/feature

    # Metadata
    workspace: Path            # Where .beads/ lives

    @property
    def is_ready(self) -> bool:
        """Can this bead be worked on?"""
        return (
            self.status == "open" and
            all(dep.status == "closed" for dep in self.dependencies)
        )

    @property
    def value_score(self) -> int:
        """Calculate 0-100 task value score"""
        # See Task Value Scoring section
        pass
```

**File Watching:**
```python
from watchdog.observers import Observer
from watchdog.events import FileSystemEventHandler

class BeadFileHandler(FileSystemEventHandler):
    """Watch .beads/*.jsonl for changes"""

    def on_modified(self, event):
        if event.src_path.endswith('.jsonl'):
            # Reload bead workspace
            workspace = parse_bead_workspace(event.src_path.parent.parent)
            # Trigger TUI update
            app.refresh_task_view(workspace)
```

### 2. Task Assignment Algorithm

**FORGE suggests, workers decide:**

```python
def suggest_task_assignment(
    ready_beads: list[Bead],
    workers: list[Worker]
) -> list[tuple[Bead, Worker]]:
    """
    Suggest optimal bead-to-worker assignments.
    Workers are free to ignore suggestions.
    """
    assignments = []

    # Sort beads by value score (highest first)
    sorted_beads = sorted(ready_beads, key=lambda b: b.value_score, reverse=True)

    # Sort workers by availability (idle > active with capacity)
    idle_workers = [w for w in workers if w.status == "idle"]
    active_workers = [w for w in workers if w.status == "active" and w.can_accept_tasks]

    available_workers = idle_workers + active_workers

    # Greedy assignment
    for bead in sorted_beads:
        if not available_workers:
            break

        # Match worker tier to bead priority
        best_worker = find_best_worker_for_bead(bead, available_workers)

        if best_worker:
            assignments.append((bead, best_worker))
            # Don't assign multiple tasks to same worker in one pass
            available_workers.remove(best_worker)

    return assignments


def find_best_worker_for_bead(bead: Bead, workers: list[Worker]) -> Worker | None:
    """
    Match worker tier to bead priority for cost optimization.

    P0 (critical) → Premium tier (Sonnet, GPT-4)
    P1 (high)     → Standard tier (Haiku, GPT-3.5)
    P2 (medium)   → Budget tier (Qwen, Llama)
    P3-P4 (low)   → Free tier (local models)
    """
    priority_to_tier = {
        "P0": "premium",
        "P1": "standard",
        "P2": "budget",
        "P3": "free",
        "P4": "free",
    }

    preferred_tier = priority_to_tier.get(bead.priority, "standard")

    # First, try exact tier match
    for worker in workers:
        if worker.tier == preferred_tier:
            return worker

    # Fallback: any available worker
    return workers[0] if workers else None
```

**Worker Autonomy:**
Workers decide what to work on via:
1. **Explicit assignment**: User/FORGE suggests via chat: "Assign bd-abc to sonnet-alpha"
2. **Worker pulls from queue**: Worker runs `br ready` and picks highest value task
3. **Manual selection**: User attaches to worker tmux and runs `br show <id>; br update <id> --status in_progress`

FORGE **never** automatically assigns tasks. It only:
- Shows ready tasks in dashboard
- Calculates value scores
- Suggests optimal assignments when asked

### 3. Task Value Scoring

**Algorithm:**
```python
def calculate_value_score(bead: Bead) -> int:
    """
    Calculate 0-100 task value score.

    Factors:
    - Priority (40 points)
    - Blockers (30 points) - how many other tasks depend on this
    - Age (20 points) - older tasks prioritized
    - Labels (10 points) - critical/urgent tags
    """
    score = 0

    # Priority contribution (0-40 points)
    priority_scores = {
        "P0": 40,
        "P1": 30,
        "P2": 20,
        "P3": 10,
        "P4": 5,
    }
    score += priority_scores.get(bead.priority, 15)

    # Blocker contribution (0-30 points)
    # More tasks blocked = higher value
    blocked_count = len(bead.blocks)
    score += min(blocked_count * 10, 30)

    # Age contribution (0-20 points)
    # Older than 7 days = full points, linear scale before
    age_days = (datetime.now() - bead.created_at).days
    score += min(age_days * 3, 20)

    # Label contribution (0-10 points)
    urgent_labels = {"critical", "urgent", "blocker", "hotfix"}
    if any(label in urgent_labels for label in bead.labels):
        score += 10

    return min(score, 100)
```

**Example Scores:**
- `P0 task, blocks 3 others, 5 days old, labeled "critical"` → 40 + 30 + 15 + 10 = **95**
- `P1 task, blocks 1 other, 2 days old, no urgent labels` → 30 + 10 + 6 + 0 = **46**
- `P3 task, blocks nothing, created today, no labels` → 10 + 0 + 0 + 0 = **10**

**Customization:**
Users can override scoring via config:
```yaml
# ~/.forge/config.yaml
task_value_scoring:
  weights:
    priority: 40
    blockers: 30
    age: 20
    labels: 10

  priority_values:
    P0: 40
    P1: 30
    P2: 20
    P3: 10
    P4: 5

  urgent_labels:
    - critical
    - urgent
    - blocker
    - hotfix
    - p0-equivalent
```

### 4. Dependency Resolution

**FORGE visualizes, br CLI enforces:**

```python
def build_dependency_graph(beads: list[Bead]) -> DependencyGraph:
    """Build dependency graph for visualization"""
    graph = DependencyGraph()

    for bead in beads:
        graph.add_node(bead)

        for dep_id in bead.depends_on:
            dep_bead = find_bead_by_id(beads, dep_id)
            if dep_bead:
                graph.add_edge(dep_bead, bead)  # dep_bead must complete first

        for blocked_id in bead.blocks:
            blocked_bead = find_bead_by_id(beads, blocked_id)
            if blocked_bead:
                graph.add_edge(bead, blocked_bead)  # bead must complete first

    return graph


def get_blocked_beads(beads: list[Bead]) -> list[Bead]:
    """Find beads blocked by open dependencies"""
    blocked = []

    for bead in beads:
        if bead.status != "open":
            continue

        # Check if any dependencies are still open
        for dep_id in bead.depends_on:
            dep_bead = find_bead_by_id(beads, dep_id)
            if dep_bead and dep_bead.status != "closed":
                blocked.append(bead)
                break

    return blocked
```

**Dependency Enforcement:**
- `br` CLI handles enforcement when closing beads
- Workers check `br ready` to see only unblocked tasks
- FORGE shows dependency graph, highlights blocked tasks in red
- FORGE **never** overrides dependency rules

**Critical Path Highlighting:**
```python
def find_critical_path(graph: DependencyGraph) -> list[Bead]:
    """
    Find longest path through dependency graph.
    These tasks block the most downstream work.
    """
    # Topological sort + longest path algorithm
    # Highlight critical path beads in orange in TUI
    pass
```

---

## TUI Integration

### Task View Panel

```
┌─ TASKS ────────────────────────────────────────────────┐
│ Ready (3)  Blocked (5)  In Progress (2)  Completed (12)│
├────────────────────────────────────────────────────────┤
│ ID       │ Priority │ Title               │ Score │ Dep│
├──────────┼──────────┼─────────────────────┼───────┼────┤
│ bd-abc ● │ P0       │ Fetch order history │  95   │ 0  │
│ bd-def ● │ P1       │ Analyze failures    │  46   │ 1  │
│ bd-ghi ● │ P2       │ Test risk gate      │  38   │ 0  │
│ bd-jkl ○ │ P0       │ Shadow trading      │  78   │ 2  │← Blocked
│ bd-mno ○ │ P1       │ Implement API       │  52   │ 1  │← Blocked
└────────────────────────────────────────────────────────┘
 ● = Ready    ○ = Blocked    ◆ = In Progress    ✓ = Done

Filters: [P0] [P1] [P2] [P3] [P4] [Ready] [Blocked] [All]
Sort by: [Score ↓] [Priority] [Age] [Blockers]
```

### Dependency Graph Panel

```
┌─ DEPENDENCY GRAPH ─────────────────────────────────────┐
│                                                         │
│   bd-abc (P0) ──┬──→ bd-def (P1) ──→ bd-xyz (P2)       │
│                 │                                       │
│                 └──→ bd-ghi (P2) ──→ bd-xyz (P2)       │
│                                                         │
│   bd-jkl (P0) ──→ bd-mno (P1) ──→ bd-pqr (P3)          │
│        ↑                                                │
│        └─── BLOCKED (waiting on bd-stu)                │
│                                                         │
│ Legend: ─→ depends_on    [ORANGE] = critical path      │
└─────────────────────────────────────────────────────────┘
```

### Chat Commands

```
User: "Show me ready tasks sorted by value"
FORGE: → filter_tasks(status="ready", sort="value_score")

User: "What's blocking bd-jkl?"
FORGE: bd-jkl depends on bd-stu (P0, in_progress, sonnet-alpha)

User: "Suggest task for idle worker"
FORGE: Highest value ready task: bd-abc (score 95, P0)
       Recommended worker: sonnet-alpha (premium tier, idle)
       Run: br update bd-abc --assignee sonnet-alpha --status in_progress

User: "Assign bd-abc to sonnet-alpha"
FORGE: → suggest_assignment(bead="bd-abc", worker="sonnet-alpha")
       [Opens confirmation dialog]
       "This will run: br update bd-abc --assignee sonnet-alpha --status in_progress"
       [Execute] [Cancel]
```

---

## Consequences

### Positive

1. **Clean Separation**: FORGE reads, `br` writes - no conflicts
2. **Worker Autonomy**: Workers decide what to work on, FORGE provides visibility
3. **Simple Integration**: Just parse JSONL files, no database access needed
4. **No Lock Conflicts**: `br` CLI handles SQLite locking, FORGE reads exported JSONL
5. **Intelligent Routing**: Value scoring optimizes cost (87-94% savings potential)
6. **Dependency Safety**: `br` enforces rules, FORGE visualizes them

### Negative

1. **Read-Only Limitation**: FORGE can't update beads directly (must shell out to `br`)
   - Mitigation: Provide "Execute br command" tool in chat interface
2. **Stale Data Risk**: JSONL might be out of sync with SQLite database
   - Mitigation: Watch for file changes, auto-refresh on updates
3. **No Real-Time Updates**: Must poll JSONL files or watch for changes
   - Mitigation: watchdog library, 1-second poll interval acceptable
4. **Value Score Subjectivity**: Algorithm may not match user priorities
   - Mitigation: Make scoring weights configurable

### Alternatives Considered

#### Direct SQLite Access
**Rejected**: Adds locking complexity, violates `br` as write authority

#### BR CLI Subprocess Calls
**Rejected for reads**: Slower than parsing JSONL, unnecessary overhead
**Accepted for writes**: Only way to mutate bead state safely

#### FORGE as Bead Manager
**Rejected**: Violates single responsibility, duplicates `br` functionality

---

## Future Enhancements

### Phase 2: Machine Learning Scoring (Optional)
- Train model on completed beads to predict value scores
- Features: priority, description embeddings, historical completion time, blocker count
- Use lightweight model (scikit-learn, not deep learning)

### Phase 3: Worker Performance Tracking
- Track which workers complete which bead types fastest
- Suggest assignments based on worker specialty (e.g., "sonnet-alpha is 2x faster on P0 analysis tasks")

### Phase 4: Multi-Workspace Bead Aggregation
- Show beads across all workspaces in single dashboard
- Filter by workspace, cross-workspace dependency tracking

---

## References

- Bead CLI Documentation: https://github.com/jedarden/beads (assumed)
- ADR 0005: Dumb Orchestrator Architecture
- ADR 0006: Technology Stack Selection (Python/orjson for JSONL parsing)
- CLAUDE.md: Bead workflow patterns and `br` CLI usage

---

## Notes

- **JSONL Parsing Performance**: orjson can parse 1MB JSONL in ~10ms
- **File Watch Latency**: watchdog typically detects changes in <100ms
- **Value Score Caching**: Cache scores for 60 seconds, recalculate on bead update
- **Dependency Cycle Detection**: `br` handles this, FORGE just visualizes

**Decision Confidence**: High - Clean read-only integration with clear boundaries

---

**FORGE** - Federated Orchestration & Resource Generation Engine
