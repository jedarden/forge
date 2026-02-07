# Worker Health Monitoring and Auto-Recovery Research

## Executive Summary

Comprehensive health monitoring and auto-recovery system for LLM workers to ensure reliability, prevent resource exhaustion, and minimize manual intervention. This research covers metrics collection, anomaly detection, recovery strategies, and alerting systems.

## 1. Health Metrics Taxonomy

### 1.1 Liveness Metrics

**Definition:** Is the worker process alive and responsive?

**Metrics:**
```python
class LivenessMetrics:
    """Core metrics to determine if worker is alive."""

    heartbeat_timestamp: datetime      # Last successful heartbeat
    heartbeat_interval: int            # Expected interval (seconds)
    process_id: int                    # OS process ID
    is_responsive: bool                # Responded to health check in <5s
    uptime_seconds: int                # Time since worker started
```

**Collection Method:**
```python
class LivenessMonitor:
    def check_liveness(self, worker: Worker) -> LivenessStatus:
        """
        Check if worker is alive and responsive.
        """
        status = LivenessStatus()

        # Check 1: Process exists
        try:
            os.kill(worker.pid, 0)  # Signal 0: check existence
            status.process_exists = True
        except OSError:
            status.process_exists = False
            status.healthy = False
            return status

        # Check 2: Heartbeat freshness
        time_since_heartbeat = (datetime.now() - worker.last_heartbeat).total_seconds()
        status.heartbeat_age = time_since_heartbeat

        if time_since_heartbeat > worker.heartbeat_interval * 3:
            status.heartbeat_stale = True
            status.healthy = False
            return status

        # Check 3: Ping-pong test
        start = time.time()
        try:
            response = worker.ping(timeout=5)
            status.response_time = time.time() - start
            status.is_responsive = response == 'pong'
        except TimeoutError:
            status.is_responsive = False
            status.healthy = False
            return status

        status.healthy = True
        return status
```

### 1.2 Activity Metrics

**Definition:** Is the worker making progress?

**Metrics:**
```python
class ActivityMetrics:
    """Metrics tracking worker activity and progress."""

    log_entries_last_minute: int       # Lines written to log
    files_modified_count: int          # Files changed in current bead
    git_commits_count: int             # Commits made
    api_calls_count: int               # LLM API calls made
    tokens_consumed: int               # Total tokens used
    tool_invocations: int              # Tool/function calls
    idle_time_seconds: int             # Time with no activity
    stuck_detection_score: float       # 0-1, 1=likely stuck
```

**Collection Method:**
```python
class ActivityMonitor:
    def __init__(self):
        self.activity_history = defaultdict(list)

    def track_activity(self, worker: Worker) -> ActivityMetrics:
        """
        Monitor worker activity across multiple signals.
        """
        metrics = ActivityMetrics()
        now = time.time()

        # Analyze log output
        recent_logs = self._get_logs_since(worker.log_file, now - 60)
        metrics.log_entries_last_minute = len(recent_logs)

        # Track file system changes
        metrics.files_modified_count = self._count_recent_file_changes(
            worker.workspace, since=worker.bead_start_time
        )

        # Monitor git activity
        git_log = subprocess.check_output(
            ['git', 'log', '--since=1 hour ago', '--oneline'],
            cwd=worker.workspace
        ).decode()
        metrics.git_commits_count = len(git_log.strip().split('\n')) if git_log.strip() else 0

        # Track API usage
        metrics.api_calls_count = worker.api_call_count
        metrics.tokens_consumed = worker.total_tokens

        # Calculate idle time
        last_activity = max([
            worker.last_log_write,
            worker.last_file_modification,
            worker.last_api_call
        ])
        metrics.idle_time_seconds = (now - last_activity)

        # Compute stuck detection score
        metrics.stuck_detection_score = self._compute_stuck_score(metrics, worker)

        self.activity_history[worker.id].append((now, metrics))

        return metrics

    def _compute_stuck_score(self, metrics: ActivityMetrics, worker: Worker) -> float:
        """
        Calculate probability that worker is stuck (0=active, 1=stuck).

        Factors:
        - Long idle time
        - No progress indicators
        - Repeated same errors
        - Context length approaching limit
        """
        score = 0.0

        # Idle time factor (max 0.4)
        if metrics.idle_time_seconds > 600:  # 10+ minutes idle
            score += 0.4
        elif metrics.idle_time_seconds > 300:  # 5+ minutes idle
            score += 0.2

        # No progress factor (max 0.3)
        progress_indicators = sum([
            metrics.log_entries_last_minute > 0,
            metrics.files_modified_count > 0,
            metrics.api_calls_count > 0,
            metrics.git_commits_count > 0
        ])
        if progress_indicators == 0:
            score += 0.3
        elif progress_indicators == 1:
            score += 0.15

        # Error repetition factor (max 0.2)
        recent_errors = self._get_recent_errors(worker, count=10)
        unique_errors = len(set(recent_errors))
        if len(recent_errors) > 5 and unique_errors <= 2:
            score += 0.2  # Same error repeatedly

        # Context overflow risk (max 0.1)
        if worker.current_context_tokens > worker.max_context_tokens * 0.9:
            score += 0.1

        return min(score, 1.0)
```

### 1.3 Resource Usage Metrics

**Definition:** System resources consumed by worker

**Metrics:**
```python
class ResourceMetrics:
    """System resource consumption metrics."""

    cpu_percent: float                 # CPU usage (0-100%)
    memory_mb: float                   # RAM usage in MB
    memory_percent: float              # RAM usage as % of total
    disk_io_read_mb: float             # Disk read in MB
    disk_io_write_mb: float            # Disk write in MB
    network_sent_mb: float             # Network sent in MB
    network_recv_mb: float             # Network received in MB
    open_files_count: int              # Number of open file descriptors
    thread_count: int                  # Number of threads
```

**Collection Method:**
```python
import psutil

class ResourceMonitor:
    def track_resources(self, worker: Worker) -> ResourceMetrics:
        """
        Monitor system resource usage for worker process.
        """
        try:
            process = psutil.Process(worker.pid)
        except psutil.NoSuchProcess:
            return None

        metrics = ResourceMetrics()

        # CPU and Memory
        metrics.cpu_percent = process.cpu_percent(interval=1.0)
        mem_info = process.memory_info()
        metrics.memory_mb = mem_info.rss / (1024 * 1024)
        metrics.memory_percent = process.memory_percent()

        # Disk I/O
        io_counters = process.io_counters()
        metrics.disk_io_read_mb = io_counters.read_bytes / (1024 * 1024)
        metrics.disk_io_write_mb = io_counters.write_bytes / (1024 * 1024)

        # Network I/O (if available)
        try:
            net_io = process.connections()
            # Approximate network usage from connection count
            metrics.network_connections = len(net_io)
        except psutil.AccessDenied:
            pass

        # File descriptors and threads
        metrics.open_files_count = len(process.open_files())
        metrics.thread_count = process.num_threads()

        return metrics

    def detect_resource_leaks(self, worker: Worker) -> List[str]:
        """
        Detect potential resource leaks.
        """
        issues = []

        history = self.get_resource_history(worker.id, duration=3600)  # 1 hour

        # Check for memory leak (monotonic increase)
        memory_trend = [m.memory_mb for m in history]
        if len(memory_trend) > 10:
            correlation = np.corrcoef(range(len(memory_trend)), memory_trend)[0, 1]
            if correlation > 0.9:  # Strong positive correlation
                issues.append("Memory leak detected: monotonic increase over 1 hour")

        # Check for file descriptor leak
        fd_counts = [m.open_files_count for m in history]
        if fd_counts and max(fd_counts) > 1000:
            issues.append(f"File descriptor leak: {max(fd_counts)} open files")

        # Check for thread leak
        thread_counts = [m.thread_count for m in history]
        if thread_counts and max(thread_counts) > 100:
            issues.append(f"Thread leak: {max(thread_counts)} threads")

        return issues
```

### 1.4 API Rate Limit Compliance Metrics

**Definition:** Track API usage against provider limits

**Metrics:**
```python
class RateLimitMetrics:
    """API rate limit tracking and compliance."""

    requests_per_minute: int           # API requests in last minute
    tokens_per_minute: int             # Tokens consumed in last minute
    requests_per_day: int              # API requests in last 24h
    tokens_per_day: int                # Tokens consumed in last 24h
    rate_limit_remaining: int          # Requests remaining (from headers)
    rate_limit_reset_time: datetime    # When limit resets
    throttle_delay_ms: int             # Current throttle delay
    rate_limit_hit_count: int          # Times hit rate limit today
```

**Collection Method:**
```python
class RateLimitMonitor:
    def __init__(self):
        self.request_history = defaultdict(deque)
        self.token_history = defaultdict(deque)

    def track_api_call(self, worker: Worker, request_data: dict):
        """
        Track API call and update rate limit metrics.
        """
        now = time.time()

        # Record request
        self.request_history[worker.id].append(now)
        self.token_history[worker.id].append((now, request_data.get('tokens', 0)))

        # Clean old data (keep 24h)
        cutoff = now - 86400
        while self.request_history[worker.id] and self.request_history[worker.id][0] < cutoff:
            self.request_history[worker.id].popleft()

        while self.token_history[worker.id] and self.token_history[worker.id][0][0] < cutoff:
            self.token_history[worker.id].popleft()

        # Parse rate limit headers
        if 'response_headers' in request_data:
            headers = request_data['response_headers']
            worker.rate_limit_remaining = int(headers.get('x-ratelimit-remaining', 0))
            worker.rate_limit_reset_time = datetime.fromtimestamp(
                int(headers.get('x-ratelimit-reset', 0))
            )

    def get_rate_limit_metrics(self, worker: Worker) -> RateLimitMetrics:
        """
        Calculate current rate limit metrics.
        """
        metrics = RateLimitMetrics()
        now = time.time()

        # Calculate requests per minute
        one_min_ago = now - 60
        metrics.requests_per_minute = sum(
            1 for ts in self.request_history[worker.id] if ts > one_min_ago
        )

        # Calculate tokens per minute
        metrics.tokens_per_minute = sum(
            tokens for ts, tokens in self.token_history[worker.id] if ts > one_min_ago
        )

        # Calculate daily totals
        one_day_ago = now - 86400
        metrics.requests_per_day = len(self.request_history[worker.id])
        metrics.tokens_per_day = sum(
            tokens for ts, tokens in self.token_history[worker.id]
        )

        # Current limit status
        metrics.rate_limit_remaining = worker.rate_limit_remaining
        metrics.rate_limit_reset_time = worker.rate_limit_reset_time

        # Calculate recommended throttle delay
        metrics.throttle_delay_ms = self._calculate_throttle_delay(metrics, worker)

        return metrics

    def _calculate_throttle_delay(self, metrics: RateLimitMetrics, worker: Worker) -> int:
        """
        Calculate delay needed to stay under rate limits.
        """
        # Get worker's rate limits
        max_rpm = worker.model_config.get('max_requests_per_minute', 60)
        max_tpm = worker.model_config.get('max_tokens_per_minute', 100000)

        # Calculate delay needed for request limit
        if metrics.requests_per_minute >= max_rpm * 0.9:  # 90% threshold
            request_delay = (60 / max_rpm) * 1000  # Convert to ms
        else:
            request_delay = 0

        # Calculate delay needed for token limit
        if metrics.tokens_per_minute >= max_tpm * 0.9:  # 90% threshold
            token_delay = ((60 / max_tpm) * metrics.tokens_per_minute) * 1000
        else:
            token_delay = 0

        return int(max(request_delay, token_delay))
```

### 1.5 Error Rate Metrics

**Definition:** Track failures and error patterns

**Metrics:**
```python
class ErrorMetrics:
    """Error tracking and analysis metrics."""

    total_errors: int                  # All errors since start
    errors_last_hour: int              # Errors in last hour
    errors_last_minute: int            # Errors in last minute
    error_rate: float                  # Errors per hour
    most_common_error: str             # Most frequent error type
    consecutive_errors: int            # Errors in a row without success
    error_types: Dict[str, int]        # Count by error type
    is_error_spiral: bool              # Stuck in error loop
```

**Collection Method:**
```python
class ErrorMonitor:
    def __init__(self):
        self.error_history = defaultdict(list)

    def record_error(self, worker: Worker, error: Exception, context: dict):
        """
        Record error with context for analysis.
        """
        error_record = {
            'timestamp': time.time(),
            'type': type(error).__name__,
            'message': str(error),
            'traceback': traceback.format_exc(),
            'context': context,
            'bead_id': worker.current_bead.id if worker.current_bead else None
        }

        self.error_history[worker.id].append(error_record)

    def get_error_metrics(self, worker: Worker) -> ErrorMetrics:
        """
        Analyze error patterns.
        """
        metrics = ErrorMetrics()
        now = time.time()

        errors = self.error_history[worker.id]
        metrics.total_errors = len(errors)

        # Time-based counts
        one_hour_ago = now - 3600
        one_min_ago = now - 60

        recent_errors = [e for e in errors if e['timestamp'] > one_hour_ago]
        metrics.errors_last_hour = len(recent_errors)
        metrics.errors_last_minute = len([e for e in errors if e['timestamp'] > one_min_ago])

        # Error rate (per hour)
        if errors:
            duration_hours = (now - errors[0]['timestamp']) / 3600
            metrics.error_rate = len(errors) / max(duration_hours, 1)

        # Error type distribution
        error_types = defaultdict(int)
        for error in errors:
            error_types[error['type']] += 1

        metrics.error_types = dict(error_types)

        if error_types:
            metrics.most_common_error = max(error_types.items(), key=lambda x: x[1])[0]

        # Consecutive errors
        metrics.consecutive_errors = self._count_consecutive_errors(worker)

        # Error spiral detection
        metrics.is_error_spiral = (
            metrics.consecutive_errors >= 5 and
            len(set([e['type'] for e in errors[-5:]])) <= 2  # Same errors
        )

        return metrics

    def _count_consecutive_errors(self, worker: Worker) -> int:
        """
        Count consecutive errors without successful operations.
        """
        # Get recent operations (errors and successes)
        operations = self._get_recent_operations(worker, limit=50)

        consecutive = 0
        for op in reversed(operations):
            if op['type'] == 'error':
                consecutive += 1
            else:
                break

        return consecutive
```

### 1.6 Task Completion Velocity Metrics

**Definition:** Measure worker productivity and efficiency

**Metrics:**
```python
class VelocityMetrics:
    """Productivity and completion metrics."""

    beads_completed: int               # Total beads finished
    beads_failed: int                  # Total beads failed
    success_rate: float                # Completion rate (0-1)
    avg_completion_time_seconds: int   # Mean time to complete bead
    median_completion_time_seconds: int # Median completion time
    current_bead_duration_seconds: int # Time on current bead
    estimated_time_remaining: int      # Predicted time to finish current bead
    productivity_score: float          # 0-100, normalized productivity
```

**Collection Method:**
```python
class VelocityMonitor:
    def __init__(self):
        self.completion_history = defaultdict(list)

    def record_completion(self, worker: Worker, bead: Bead, success: bool, duration: int):
        """
        Record bead completion event.
        """
        record = {
            'timestamp': time.time(),
            'bead_id': bead.id,
            'success': success,
            'duration': duration,
            'priority': bead.priority,
            'type': bead.type,
            'tokens_used': worker.current_bead_tokens
        }

        self.completion_history[worker.id].append(record)

    def get_velocity_metrics(self, worker: Worker) -> VelocityMetrics:
        """
        Calculate productivity metrics.
        """
        metrics = VelocityMetrics()

        history = self.completion_history[worker.id]
        if not history:
            return metrics

        # Counts
        metrics.beads_completed = len([h for h in history if h['success']])
        metrics.beads_failed = len([h for h in history if not h['success']])

        # Success rate
        total = len(history)
        metrics.success_rate = metrics.beads_completed / total if total > 0 else 0

        # Completion times (only successful beads)
        successful = [h for h in history if h['success']]
        if successful:
            durations = [h['duration'] for h in successful]
            metrics.avg_completion_time_seconds = int(np.mean(durations))
            metrics.median_completion_time_seconds = int(np.median(durations))

        # Current bead progress
        if worker.current_bead:
            metrics.current_bead_duration_seconds = int(
                time.time() - worker.bead_start_time
            )

            # Estimate time remaining based on historical data
            if successful:
                similar_beads = [
                    h for h in successful
                    if h['type'] == worker.current_bead.type
                ]
                if similar_beads:
                    avg_duration = np.mean([h['duration'] for h in similar_beads])
                    metrics.estimated_time_remaining = int(
                        max(0, avg_duration - metrics.current_bead_duration_seconds)
                    )

        # Productivity score (normalized)
        metrics.productivity_score = self._calculate_productivity_score(worker, history)

        return metrics

    def _calculate_productivity_score(self, worker: Worker, history: list) -> float:
        """
        Calculate normalized productivity score (0-100).

        Factors:
        - Success rate (40%)
        - Speed vs average (30%)
        - Consistency (20%)
        - Quality (10% - fewer retries needed)
        """
        if not history:
            return 0.0

        score = 0.0

        # Success rate component (40 points)
        successes = len([h for h in history if h['success']])
        success_rate = successes / len(history)
        score += success_rate * 40

        # Speed component (30 points)
        successful = [h for h in history if h['success']]
        if successful:
            avg_duration = np.mean([h['duration'] for h in successful])
            # Compare to global average (assume 600s baseline)
            baseline = 600
            speed_ratio = baseline / avg_duration if avg_duration > 0 else 1
            speed_ratio = min(speed_ratio, 2)  # Cap at 2x baseline
            score += (speed_ratio - 0.5) * 30  # 0.5x = 0pts, 2x = 45pts

        # Consistency component (20 points)
        if len(successful) >= 3:
            durations = [h['duration'] for h in successful]
            cv = np.std(durations) / np.mean(durations)  # Coefficient of variation
            consistency_score = max(0, 1 - cv)  # Lower CV = higher consistency
            score += consistency_score * 20

        # Quality component (10 points)
        # Measure by retry rate (assumes retries are tracked)
        retry_rate = len([h for h in history if h.get('is_retry', False)]) / len(history)
        quality_score = max(0, 1 - retry_rate)
        score += quality_score * 10

        return min(100, max(0, score))
```

### 1.7 Model-Specific Monitoring

**Definition:** Track issues specific to LLM models

**Metrics:**
```python
class ModelSpecificMetrics:
    """Metrics specific to LLM model characteristics."""

    # Context management
    current_context_tokens: int        # Tokens in current context
    max_context_tokens: int            # Model's context limit
    context_utilization: float         # % of context used
    context_overflow_count: int        # Times hit context limit

    # Output quality
    avg_output_tokens: int             # Mean output length
    truncated_response_count: int      # Incomplete responses
    refusal_count: int                 # "I cannot do that" responses

    # Model-specific errors
    timeout_count: int                 # Request timeouts
    content_filter_triggers: int       # Safety filter activations
    invalid_json_count: int            # Failed tool calls
    hallucination_indicators: int      # Detected hallucinations

    # Performance
    avg_first_token_latency_ms: int    # Time to first token
    avg_tokens_per_second: float       # Generation speed
```

**Collection Method:**
```python
class ModelSpecificMonitor:
    def track_llm_call(self, worker: Worker, request: dict, response: dict):
        """
        Track LLM-specific metrics from API call.
        """
        # Context tracking
        if 'usage' in response:
            worker.current_context_tokens = response['usage'].get('total_tokens', 0)
            worker.max_context_tokens = worker.model_config['context_window']

            utilization = worker.current_context_tokens / worker.max_context_tokens
            if utilization > 0.95:
                worker.context_overflow_count += 1
                self._handle_context_overflow(worker)

        # Output quality
        output_tokens = response.get('usage', {}).get('completion_tokens', 0)
        worker.output_token_history.append(output_tokens)

        # Check for truncation
        if response.get('finish_reason') == 'length':
            worker.truncated_response_count += 1

        # Check for refusals
        content = response.get('content', '')
        if self._is_refusal(content):
            worker.refusal_count += 1

        # Latency tracking
        if 'timing' in response:
            worker.first_token_latency_history.append(
                response['timing']['first_token_ms']
            )
            worker.tokens_per_second_history.append(
                response['timing']['tokens_per_second']
            )

    def _handle_context_overflow(self, worker: Worker):
        """
        Handle context window overflow.
        """
        # Strategy 1: Truncate older messages
        if worker.context_truncation_enabled:
            worker.truncate_context(keep_recent=10)

        # Strategy 2: Summarize conversation
        elif worker.context_summarization_enabled:
            worker.summarize_and_compress_context()

        # Strategy 3: Restart with fresh context
        else:
            worker.restart_with_summary()

    def detect_hallucination_indicators(self, worker: Worker, response: str) -> int:
        """
        Detect potential hallucination indicators.

        Heuristics:
        - Mentions files that don't exist
        - Invents function names not in codebase
        - Contradicts previous statements
        - Over-confident assertions without evidence
        """
        indicators = 0

        # Check for non-existent files
        file_pattern = r'`([^`]+\.(?:py|js|ts|yml))`'
        mentioned_files = re.findall(file_pattern, response)
        for file in mentioned_files:
            if not os.path.exists(os.path.join(worker.workspace, file)):
                indicators += 1

        # Check for invented API calls
        # (Would need codebase index)

        # Check for contradiction with conversation history
        # (Would need semantic similarity check)

        return indicators
```

## 2. Auto-Recovery Strategies

### 2.1 Graceful Restart with State Preservation

**Goal:** Restart worker without losing progress

**Implementation:**
```python
class GracefulRestartManager:
    def restart_worker(self, worker: Worker, reason: str) -> Worker:
        """
        Restart worker preserving as much state as possible.
        """
        # Save current state
        state = self._capture_worker_state(worker)

        # Gracefully stop worker
        self._graceful_shutdown(worker, timeout=30)

        # Launch new worker with restored state
        new_worker = self._create_worker_from_state(state)

        self._log_restart_event(worker.id, reason, state)

        return new_worker

    def _capture_worker_state(self, worker: Worker) -> dict:
        """
        Capture all recoverable state.
        """
        state = {
            'worker_id': worker.id,
            'workspace': worker.workspace,
            'current_bead': worker.current_bead.id if worker.current_bead else None,
            'bead_progress': self._estimate_bead_progress(worker),
            'context_summary': worker.get_context_summary(),
            'files_modified': worker.get_modified_files(),
            'partial_outputs': worker.get_partial_outputs(),
            'environment_vars': worker.get_env_vars(),
            'model_config': worker.model_config,
            'timestamp': datetime.now().isoformat()
        }

        # Save to disk for recovery
        state_file = f"/tmp/worker-{worker.id}-state.json"
        with open(state_file, 'w') as f:
            json.dump(state, f)

        return state

    def _estimate_bead_progress(self, worker: Worker) -> dict:
        """
        Estimate how far along the bead is.
        """
        return {
            'elapsed_time': time.time() - worker.bead_start_time,
            'files_modified_count': len(worker.get_modified_files()),
            'commits_made': worker.git_commit_count,
            'estimated_completion': worker.estimated_completion_percent
        }

    def _graceful_shutdown(self, worker: Worker, timeout: int):
        """
        Attempt graceful shutdown before force kill.
        """
        # Send SIGTERM
        try:
            os.kill(worker.pid, signal.SIGTERM)

            # Wait for graceful exit
            start = time.time()
            while time.time() - start < timeout:
                try:
                    os.kill(worker.pid, 0)
                    time.sleep(1)
                except OSError:
                    # Process exited
                    return
        except OSError:
            return  # Already dead

        # Force kill if still alive
        try:
            os.kill(worker.pid, signal.SIGKILL)
        except OSError:
            pass

    def _create_worker_from_state(self, state: dict) -> Worker:
        """
        Create new worker and restore state.
        """
        worker = Worker(
            workspace=state['workspace'],
            model_config=state['model_config']
        )

        # Restore bead assignment
        if state['current_bead']:
            bead = Bead.get(state['current_bead'])
            worker.assign_bead(bead)

            # Provide context from previous attempt
            worker.set_initial_context(f"""
            You are resuming work on this bead after a worker restart.

            Previous progress:
            - Time spent: {state['bead_progress']['elapsed_time']:.0f} seconds
            - Files modified: {state['bead_progress']['files_modified_count']}
            - Commits made: {state['bead_progress']['commits_made']}
            - Estimated {state['bead_progress']['estimated_completion']}% complete

            Context summary from previous attempt:
            {state['context_summary']}

            Modified files:
            {', '.join(state['files_modified'])}

            Please continue where you left off.
            """)

        return worker
```

### 2.2 Workspace Lock Cleanup

**Goal:** Release locks when worker fails

**Implementation:**
```python
class LockCleanupManager:
    def cleanup_worker_locks(self, worker: Worker):
        """
        Release all locks held by failed worker.
        """
        # Get all locks held by worker
        locks = self.lock_manager.get_worker_locks(worker.id)

        for lock in locks:
            try:
                # Release lock
                self.lock_manager.release_lock(lock.resource_id, worker.id)

                # Log cleanup
                self._log_lock_release(worker.id, lock.resource_id, reason="worker_failure")

                # Notify waiting workers
                self._notify_waiters(lock.resource_id)

            except Exception as e:
                # Log but don't fail cleanup
                self._log_lock_cleanup_error(worker.id, lock.resource_id, e)

        # Clean up stale locks in database
        self._cleanup_stale_locks(worker.id)

    def _cleanup_stale_locks(self, worker_id: str):
        """
        Remove any orphaned locks in database.
        """
        # SQLite cleanup
        with sqlite_transaction():
            execute("""
                DELETE FROM bead_locks WHERE worker_id = ?
            """, [worker_id])

            execute("""
                DELETE FROM file_locks
                WHERE bead_id IN (
                    SELECT bead_id FROM bead_locks WHERE worker_id = ?
                )
            """, [worker_id])

        # Redis cleanup (if using)
        if self.redis_client:
            pattern = f"*:lock:*:{worker_id}"
            for key in self.redis_client.scan_iter(match=pattern):
                self.redis_client.delete(key)
```

### 2.3 Exponential Backoff for Failing Workers

**Goal:** Prevent rapid restart loops

**Implementation:**
```python
class BackoffManager:
    def __init__(self):
        self.failure_counts = defaultdict(int)
        self.last_failure_times = {}

    def should_restart(self, worker_id: str, failure_type: str) -> tuple[bool, int]:
        """
        Determine if worker should restart and how long to wait.

        Returns: (should_restart, wait_seconds)
        """
        # Increment failure count
        self.failure_counts[worker_id] += 1
        self.last_failure_times[worker_id] = time.time()

        failure_count = self.failure_counts[worker_id]

        # Exponential backoff: 2^n seconds, capped at 1 hour
        wait_time = min(2 ** failure_count, 3600)

        # Stop restarting after 10 consecutive failures
        if failure_count >= 10:
            return False, 0

        # Special handling for different failure types
        if failure_type == 'context_overflow':
            # Fast retry with context reset
            return True, 5

        elif failure_type == 'rate_limit':
            # Wait until rate limit resets
            return True, wait_time

        elif failure_type == 'authentication':
            # Don't auto-retry auth failures
            return False, 0

        elif failure_type == 'unrecoverable_error':
            # Critical error, don't restart
            return False, 0

        else:
            # Default exponential backoff
            return True, wait_time

    def reset_failure_count(self, worker_id: str):
        """
        Reset failure count after successful operation.
        """
        self.failure_counts[worker_id] = 0

    def get_backoff_stats(self, worker_id: str) -> dict:
        """
        Get backoff statistics for monitoring.
        """
        return {
            'failure_count': self.failure_counts[worker_id],
            'last_failure': self.last_failure_times.get(worker_id),
            'next_retry_in': self._calculate_next_retry_time(worker_id)
        }
```

### 2.4 Model Fallback Chains

**Goal:** Automatically switch to backup models when primary fails

**Implementation:**
```python
class ModelFallbackManager:
    """
    Manage fallback chain: Opus 4 → Sonnet 3.7 → GLM-4
    """

    def __init__(self):
        self.fallback_chains = {
            'default': [
                {'model': 'claude-opus-4', 'cost_per_1k': 15.0, 'max_context': 200000},
                {'model': 'claude-sonnet-3-7', 'cost_per_1k': 3.0, 'max_context': 200000},
                {'model': 'glm-4', 'cost_per_1k': 0.5, 'max_context': 128000},
            ],
            'budget': [
                {'model': 'glm-4', 'cost_per_1k': 0.5, 'max_context': 128000},
                {'model': 'claude-haiku-3', 'cost_per_1k': 0.25, 'max_context': 200000},
            ],
            'premium': [
                {'model': 'claude-opus-4', 'cost_per_1k': 15.0, 'max_context': 200000},
                {'model': 'gpt-4-turbo', 'cost_per_1k': 10.0, 'max_context': 128000},
            ]
        }

        self.fallback_attempts = defaultdict(lambda: defaultdict(int))

    def get_next_model(self, worker: Worker, failure_reason: str) -> dict:
        """
        Get next model in fallback chain.
        """
        chain_type = worker.fallback_chain_type or 'default'
        chain = self.fallback_chains[chain_type]

        current_model = worker.current_model
        current_index = next(
            (i for i, m in enumerate(chain) if m['model'] == current_model),
            0
        )

        # Check if we've exhausted the chain
        if current_index >= len(chain) - 1:
            return None  # No more fallbacks

        # Get next model in chain
        next_model = chain[current_index + 1]

        # Record fallback attempt
        self.fallback_attempts[worker.id][next_model['model']] += 1

        self._log_fallback(worker.id, current_model, next_model['model'], failure_reason)

        return next_model

    def should_fallback(self, worker: Worker, failure_reason: str) -> bool:
        """
        Determine if we should fallback to next model.
        """
        # Always fallback for these reasons
        fallback_triggers = {
            'rate_limit_exceeded',
            'model_unavailable',
            'authentication_failed',
            'context_window_exceeded'
        }

        if failure_reason in fallback_triggers:
            return True

        # Fallback after multiple consecutive failures
        if worker.consecutive_failures >= 3:
            return True

        # Don't fallback for user errors or code bugs
        no_fallback_reasons = {
            'invalid_request',
            'syntax_error',
            'file_not_found'
        }

        if failure_reason in no_fallback_reasons:
            return False

        return False

    def create_fallback_worker(self, worker: Worker, next_model: dict) -> Worker:
        """
        Create new worker with fallback model.
        """
        # Preserve worker state
        state = worker.get_state()

        # Create new worker with different model
        new_worker = Worker(
            workspace=worker.workspace,
            model_config=next_model
        )

        # Restore state
        new_worker.restore_state(state)

        # Add context about model change
        new_worker.add_system_message(f"""
        Note: This task was originally assigned to {worker.current_model} but has been
        reassigned to {next_model['model']} due to: {worker.last_failure_reason}.

        Please continue the task with the same goals and context.
        """)

        return new_worker
```

### 2.5 Circuit Breaker Pattern

**Goal:** Prevent cascading failures by stopping requests to failing services

**Implementation:**
```python
class CircuitBreaker:
    """
    Circuit breaker for API endpoints.

    States:
    - CLOSED: Normal operation, requests pass through
    - OPEN: Too many failures, block all requests
    - HALF_OPEN: Testing if service recovered
    """

    def __init__(self, failure_threshold: int = 5, timeout: int = 60):
        self.failure_threshold = failure_threshold
        self.timeout = timeout  # Time before trying again (seconds)

        self.state = 'CLOSED'
        self.failure_count = 0
        self.last_failure_time = None
        self.success_count = 0

    def call(self, func, *args, **kwargs):
        """
        Execute function through circuit breaker.
        """
        if self.state == 'OPEN':
            # Check if we should try again
            if time.time() - self.last_failure_time > self.timeout:
                self.state = 'HALF_OPEN'
                self.success_count = 0
            else:
                raise CircuitBreakerOpen("Service unavailable, circuit breaker is open")

        try:
            result = func(*args, **kwargs)
            self._on_success()
            return result

        except Exception as e:
            self._on_failure()
            raise

    def _on_success(self):
        """Handle successful call."""
        if self.state == 'HALF_OPEN':
            self.success_count += 1
            # Require multiple successes before closing
            if self.success_count >= 3:
                self.state = 'CLOSED'
                self.failure_count = 0
        else:
            self.failure_count = 0

    def _on_failure(self):
        """Handle failed call."""
        self.failure_count += 1
        self.last_failure_time = time.time()

        if self.failure_count >= self.failure_threshold:
            self.state = 'OPEN'

        # If we were testing (HALF_OPEN) and failed, go back to OPEN
        if self.state == 'HALF_OPEN':
            self.state = 'OPEN'

    def get_state(self) -> dict:
        """Get current circuit breaker state."""
        return {
            'state': self.state,
            'failure_count': self.failure_count,
            'last_failure': self.last_failure_time,
            'next_retry': self.last_failure_time + self.timeout if self.state == 'OPEN' else None
        }


class APIClientWithCircuitBreaker:
    """
    API client that uses circuit breakers per endpoint.
    """

    def __init__(self):
        self.circuit_breakers = {}

    def _get_circuit_breaker(self, endpoint: str) -> CircuitBreaker:
        """Get or create circuit breaker for endpoint."""
        if endpoint not in self.circuit_breakers:
            self.circuit_breakers[endpoint] = CircuitBreaker(
                failure_threshold=5,
                timeout=60
            )
        return self.circuit_breakers[endpoint]

    def call_api(self, endpoint: str, **kwargs):
        """Make API call through circuit breaker."""
        breaker = self._get_circuit_breaker(endpoint)

        try:
            return breaker.call(self._make_request, endpoint, **kwargs)
        except CircuitBreakerOpen:
            # Try fallback endpoint if available
            if fallback := self._get_fallback_endpoint(endpoint):
                return self.call_api(fallback, **kwargs)
            raise
```

### 2.6 Health Check Intervals by Worker Type

**Goal:** Optimize monitoring frequency based on worker characteristics

**Implementation:**
```python
class AdaptiveHealthCheckScheduler:
    """
    Adjust health check frequency based on worker type and health history.
    """

    def __init__(self):
        self.base_intervals = {
            'claude-opus-4': 30,      # Expensive, check less often
            'claude-sonnet-3-7': 20,  # Balanced
            'glm-4': 10,              # Cheap, check more often
            'default': 15
        }

    def get_check_interval(self, worker: Worker) -> int:
        """
        Calculate optimal health check interval for worker.
        """
        # Start with base interval for model type
        base = self.base_intervals.get(worker.model_type, self.base_intervals['default'])

        # Adjust based on worker health history
        if worker.health_score > 0.9:
            # Healthy worker, check less often
            interval = base * 2
        elif worker.health_score < 0.5:
            # Unhealthy worker, check more often
            interval = base // 2
        else:
            interval = base

        # Adjust based on current task
        if worker.current_bead:
            # Long-running tasks need less frequent checks
            if worker.estimated_duration > 1800:  # 30+ minutes
                interval *= 1.5

            # Critical priority beads need more frequent monitoring
            if worker.current_bead.priority == 'P0':
                interval *= 0.7

        return int(interval)

    def schedule_health_checks(self, workers: List[Worker]):
        """
        Create optimized health check schedule.
        """
        schedule = {}

        for worker in workers:
            interval = self.get_check_interval(worker)
            next_check = time.time() + interval

            schedule[worker.id] = {
                'interval': interval,
                'next_check': next_check,
                'checks_per_hour': 3600 / interval
            }

        return schedule
```

## 3. Alerting System

### 3.1 Alert Definitions

**Alert Levels:**
- CRITICAL: Immediate action required
- WARNING: Attention needed soon
- INFO: Informational, no action needed

**Alert Types:**

```python
class AlertManager:
    def __init__(self):
        self.alert_rules = self._define_alert_rules()
        self.alert_channels = self._setup_channels()

    def _define_alert_rules(self) -> List[AlertRule]:
        """
        Define all alert rules.
        """
        return [
            # Critical failures
            AlertRule(
                name="worker_dead",
                level="CRITICAL",
                condition=lambda w: not w.is_alive(),
                message="Worker {worker_id} is not responding",
                action=self._restart_worker
            ),

            AlertRule(
                name="error_spiral",
                level="CRITICAL",
                condition=lambda w: w.error_metrics.consecutive_errors >= 5,
                message="Worker {worker_id} in error spiral: {consecutive_errors} errors",
                action=self._escalate_to_fallback_model
            ),

            AlertRule(
                name="workspace_lock_held_too_long",
                level="CRITICAL",
                condition=lambda w: w.lock_duration > 3600,  # 1 hour
                message="Worker {worker_id} holding lock for {lock_duration}s",
                action=self._force_release_lock
            ),

            # Resource warnings
            AlertRule(
                name="high_memory_usage",
                level="WARNING",
                condition=lambda w: w.resource_metrics.memory_percent > 90,
                message="Worker {worker_id} using {memory_percent}% memory",
                action=self._restart_worker
            ),

            AlertRule(
                name="context_overflow_imminent",
                level="WARNING",
                condition=lambda w: w.context_utilization > 0.95,
                message="Worker {worker_id} at {context_utilization}% context capacity",
                action=self._trigger_context_compression
            ),

            # API limit warnings
            AlertRule(
                name="rate_limit_approaching",
                level="WARNING",
                condition=lambda w: w.rate_limit_metrics.requests_per_minute > w.max_rpm * 0.9,
                message="Worker {worker_id} approaching rate limit: {requests_per_minute} RPM",
                action=self._throttle_worker
            ),

            AlertRule(
                name="daily_token_limit_approaching",
                level="WARNING",
                condition=lambda w: w.tokens_today > w.daily_token_limit * 0.9,
                message="Worker {worker_id} used {tokens_today} tokens today (limit: {daily_token_limit})",
                action=self._pause_worker
            ),

            # Cost alerts
            AlertRule(
                name="cost_threshold_exceeded",
                level="WARNING",
                condition=lambda w: w.cost_today > w.daily_cost_limit,
                message="Worker {worker_id} cost ${cost_today} exceeded daily limit ${daily_cost_limit}",
                action=self._switch_to_cheaper_model
            ),

            # Pattern detection
            AlertRule(
                name="unusual_activity",
                level="INFO",
                condition=lambda w: self._detect_unusual_pattern(w),
                message="Worker {worker_id} showing unusual activity pattern",
                action=self._log_for_investigation
            ),
        ]

    def evaluate_alerts(self, worker: Worker) -> List[Alert]:
        """
        Evaluate all alert rules for worker.
        """
        triggered_alerts = []

        for rule in self.alert_rules:
            try:
                if rule.condition(worker):
                    alert = self._create_alert(rule, worker)
                    triggered_alerts.append(alert)

                    # Execute alert action
                    if rule.action:
                        rule.action(worker)

            except Exception as e:
                # Don't let alert evaluation break monitoring
                self._log_alert_error(rule.name, worker.id, e)

        return triggered_alerts

    def _create_alert(self, rule: AlertRule, worker: Worker) -> Alert:
        """
        Create alert from rule and worker state.
        """
        # Format message with worker data
        message = rule.message.format(**worker.__dict__)

        alert = Alert(
            name=rule.name,
            level=rule.level,
            message=message,
            worker_id=worker.id,
            timestamp=datetime.now(),
            metadata=self._gather_alert_context(worker)
        )

        return alert

    def _gather_alert_context(self, worker: Worker) -> dict:
        """
        Gather context for alert debugging.
        """
        return {
            'worker_id': worker.id,
            'model': worker.model_type,
            'current_bead': worker.current_bead.id if worker.current_bead else None,
            'uptime': worker.uptime_seconds,
            'health_score': worker.health_score,
            'recent_errors': worker.error_metrics.error_types,
            'resource_usage': {
                'cpu': worker.resource_metrics.cpu_percent,
                'memory': worker.resource_metrics.memory_percent
            },
            'api_usage': {
                'requests_per_minute': worker.rate_limit_metrics.requests_per_minute,
                'tokens_today': worker.tokens_today
            }
        }
```

### 3.2 Alert Channels

**Implementation:**

```python
class AlertChannels:
    """
    Multiple channels for delivering alerts.
    """

    def __init__(self, config: dict):
        self.config = config
        self.setup_channels()

    def setup_channels(self):
        """Initialize alert delivery channels."""
        self.channels = {}

        # Slack
        if self.config.get('slack_webhook'):
            self.channels['slack'] = SlackChannel(
                webhook_url=self.config['slack_webhook']
            )

        # Email
        if self.config.get('smtp_config'):
            self.channels['email'] = EmailChannel(
                smtp_config=self.config['smtp_config']
            )

        # PagerDuty (for critical alerts)
        if self.config.get('pagerduty_key'):
            self.channels['pagerduty'] = PagerDutyChannel(
                api_key=self.config['pagerduty_key']
            )

        # Logging (always enabled)
        self.channels['log'] = LogChannel()

    def send_alert(self, alert: Alert):
        """
        Send alert to appropriate channels based on level.
        """
        # All alerts go to logs
        self.channels['log'].send(alert)

        # Route by severity
        if alert.level == 'CRITICAL':
            # Critical alerts to all channels
            for channel in self.channels.values():
                channel.send(alert)

        elif alert.level == 'WARNING':
            # Warnings to Slack and email
            if 'slack' in self.channels:
                self.channels['slack'].send(alert)
            if 'email' in self.channels:
                self.channels['email'].send(alert)

        else:
            # Info to logs only
            pass


class SlackChannel:
    """Send alerts to Slack."""

    def __init__(self, webhook_url: str):
        self.webhook_url = webhook_url

    def send(self, alert: Alert):
        """Send alert to Slack."""
        color = {
            'CRITICAL': 'danger',
            'WARNING': 'warning',
            'INFO': 'good'
        }[alert.level]

        payload = {
            'attachments': [{
                'color': color,
                'title': f"{alert.level}: {alert.name}",
                'text': alert.message,
                'fields': [
                    {'title': 'Worker ID', 'value': alert.worker_id, 'short': True},
                    {'title': 'Time', 'value': alert.timestamp.isoformat(), 'short': True},
                ],
                'footer': 'Control Panel Monitoring'
            }]
        }

        requests.post(self.webhook_url, json=payload)
```

## 4. Implementation Roadmap

### Phase 1: Core Monitoring (Week 1-2)
- [ ] Implement liveness and activity monitoring
- [ ] Set up resource tracking with psutil
- [ ] Build error tracking system
- [ ] Create health score calculation

### Phase 2: Recovery Mechanisms (Week 3-4)
- [ ] Implement graceful restart with state preservation
- [ ] Build lock cleanup on worker failure
- [ ] Add exponential backoff logic
- [ ] Create circuit breaker for API calls

### Phase 3: Advanced Features (Week 5-6)
- [ ] Model fallback chain implementation
- [ ] Adaptive health check scheduling
- [ ] Hallucination detection heuristics
- [ ] Context overflow handling

### Phase 4: Alerting & Dashboards (Week 7-8)
- [ ] Set up alert rules and channels
- [ ] Build real-time monitoring dashboard
- [ ] Add historical metrics analysis
- [ ] Create automated reports

## 5. Success Metrics

**Reliability:**
- 99.5% worker uptime
- < 1% unplanned restarts
- Mean time to recovery (MTTR) < 2 minutes
- Zero undetected worker failures

**Efficiency:**
- < 5% false positive health alerts
- 95% of failures auto-recovered
- Average health check overhead < 1% CPU

**Cost:**
- 20% reduction in wasted API calls
- Early detection prevents 90% of rate limit violations
- Automated recovery saves 10+ hours/week of manual intervention

## 6. References

- **Circuit Breaker Pattern:** Martin Fowler's "Release It!"
- **Health Checks:** Kubernetes liveness/readiness probes
- **Error Budgets:** Google SRE Book
- **Exponential Backoff:** AWS SDK retry strategies
- **Metrics:** Prometheus monitoring best practices
