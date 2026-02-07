# Implementation Plan: Task Value Scoring & Model Assignment System

## Overview

This document outlines the implementation plan for the intelligent task value scoring and model assignment system. The implementation is divided into phases, with each phase building on the previous one.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                     Control Panel System                        │
└─────────────────────────────────────────────────────────────────┘
                               │
        ┌──────────────────────┼──────────────────────┐
        │                      │                      │
        ▼                      ▼                      ▼
┌──────────────┐    ┌──────────────────┐    ┌─────────────────┐
│ Task Queue   │    │ Scoring Engine   │    │ Learning Engine │
│ Manager      │    │                  │    │                 │
└──────┬───────┘    └────────┬─────────┘    └────────┬────────┘
       │                     │                       │
       │ Task Metadata       │ Value Score           │ Performance Data
       │                     │ Token Estimate        │ Adjustments
       ▼                     ▼                       ▼
┌─────────────────────────────────────────────────────────────────┐
│                   Assignment Orchestrator                        │
│  - Quota Tracking                                                │
│  - Model Selection Logic                                         │
│  - A/B Test Assignment                                           │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             │ Model + Worker Assignment
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Worker Pool                               │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐           │
│  │ Opus    │  │ Sonnet  │  │ DeepSeek│  │ GLM-4.7 │  ...      │
│  │ Workers │  │ Workers │  │ Workers │  │ Workers │           │
│  └─────────┘  └─────────┘  └─────────┘  └─────────┘           │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             │ Execution Results
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                   Execution Tracker                              │
│  - Record outcomes                                               │
│  - Calculate quality scores                                      │
│  - Update quota usage                                            │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             │ Feedback Loop
                             ▼
                    ┌─────────────────┐
                    │ Learning Engine │
                    │ (Continuous)    │
                    └─────────────────┘
```

## Technology Stack

### Core Components
- **Language**: Python 3.11+
- **Database**: PostgreSQL 15+ (task executions, performance metrics)
- **Cache**: Redis (quota tracking, real-time state)
- **Task Queue**: Celery or RQ (async task processing)
- **ML**: scikit-learn, numpy, scipy (learning algorithms)
- **API**: FastAPI (REST API for integrations)
- **CLI**: Click or Typer (command-line interface)

### Optional Components
- **Monitoring**: Prometheus + Grafana
- **Logging**: Structured logging with Loguru
- **Config**: YAML + Pydantic for validation

## Phase 1: Core Scoring Engine (Week 1-2)

### Goals
- Implement task value scoring
- Implement token estimation
- Create basic model assignment logic

### Tasks

#### 1.1 Set Up Project Structure
```bash
control-panel/
├── src/
│   ├── scoring/
│   │   ├── __init__.py
│   │   ├── value_scorer.py
│   │   ├── token_estimator.py
│   │   └── complexity_analyzer.py
│   ├── assignment/
│   │   ├── __init__.py
│   │   ├── model_selector.py
│   │   └── quota_tracker.py
│   ├── models/
│   │   ├── __init__.py
│   │   ├── task.py
│   │   ├── model.py
│   │   └── execution.py
│   ├── config/
│   │   ├── __init__.py
│   │   ├── scoring_config.yaml
│   │   └── models_config.yaml
│   └── utils/
│       ├── __init__.py
│       └── domain_classifier.py
├── tests/
│   ├── test_scoring.py
│   ├── test_assignment.py
│   └── test_integration.py
├── config/
│   ├── scoring.yaml
│   └── models.yaml
├── requirements.txt
└── README.md
```

#### 1.2 Implement Data Models
```python
# src/models/task.py
from dataclasses import dataclass
from typing import List, Optional
from datetime import datetime

@dataclass
class Task:
    id: str
    title: str
    description: str
    priority: str  # P0-P4
    labels: List[str]
    workspace_path: str
    estimated_files: int = 0
    estimated_loc: int = 0
    dependencies: List[str] = None
    deadline: Optional[datetime] = None
    value_score: Optional[float] = None
    assigned_model: Optional[str] = None

    def __post_init__(self):
        if self.dependencies is None:
            self.dependencies = []
```

#### 1.3 Implement Value Scorer
```python
# src/scoring/value_scorer.py
class TaskValueScorer:
    def __init__(self, config: dict):
        self.priority_weights = config['priority_weights']
        self.complexity_multipliers = config['complexity_multipliers']
        self.domain_modifiers = config['domain_modifiers']
        self.risk_modifiers = config['risk_modifiers']
        self.time_bonuses = config['time_sensitivity_bonuses']

    def calculate_value(self, task: Task) -> float:
        """Calculate task value score (0-100)"""
        # Implementation from algorithm doc
        pass
```

#### 1.4 Implement Token Estimator
```python
# src/scoring/token_estimator.py
class TokenEstimator:
    def estimate(self, task: Task) -> Tuple[int, int]:
        """Estimate input and output tokens"""
        # Implementation from algorithm doc
        pass
```

#### 1.5 Create Configuration Files
```yaml
# config/scoring.yaml
priority_weights:
  P0: 40
  P1: 30
  P2: 20
  P3: 10
  P4: 5

complexity_multipliers:
  simple: 0.5
  moderate: 1.0
  complex: 1.5
  highly_complex: 2.0

# ... (rest of config)
```

#### 1.6 Unit Tests
```python
# tests/test_scoring.py
def test_p0_production_hotfix():
    """Test high-value scoring for critical production tasks"""
    task = Task(
        id='test-1',
        title='Fix auth vulnerability',
        priority='P0',
        labels=['security', 'backend', 'hotfix'],
        # ...
    )
    scorer = TaskValueScorer(load_config())
    score = scorer.calculate_value(task)
    assert score >= 75  # Should be high-value
```

### Deliverables
- [ ] Working value scoring system
- [ ] Token estimation system
- [ ] Configuration management
- [ ] Unit tests (>80% coverage)
- [ ] Documentation

## Phase 2: Model Assignment System (Week 3-4)

### Goals
- Implement model selection logic
- Add subscription quota tracking
- Create assignment orchestrator

### Tasks

#### 2.1 Implement Model Registry
```python
# src/models/model.py
@dataclass
class Model:
    name: str
    tier: str
    cost_per_mtok_input: float
    cost_per_mtok_output: float
    context_window: int
    capabilities: Dict[str, float]

class ModelRegistry:
    def __init__(self, config_path: str):
        self.models = self._load_models(config_path)

    def get_models_by_tier(self, tiers: List[str]) -> List[Model]:
        """Get all models in specified tiers"""
        pass

    def get_model(self, name: str) -> Optional[Model]:
        """Get model by name"""
        pass
```

#### 2.2 Implement Quota Tracker
```python
# src/assignment/quota_tracker.py
class QuotaTracker:
    def __init__(self, redis_client):
        self.redis = redis_client

    def check_quota(self, subscription: str, tokens: int) -> bool:
        """Check if subscription has enough quota"""
        pass

    def reserve_quota(self, subscription: str, tokens: int) -> bool:
        """Reserve tokens from quota"""
        pass

    def release_quota(self, subscription: str, tokens: int):
        """Release reserved tokens"""
        pass

    def get_remaining_quota(self, subscription: str) -> dict:
        """Get remaining daily/monthly quota"""
        pass
```

#### 2.3 Implement Model Selector
```python
# src/assignment/model_selector.py
class ModelSelector:
    def __init__(self, scorer, estimator, quota_tracker, registry):
        self.scorer = scorer
        self.estimator = estimator
        self.quota_tracker = quota_tracker
        self.registry = registry

    def select_model(
        self,
        task: Task,
        strategy: str = 'maximize_value'
    ) -> Optional[Model]:
        """Select optimal model for task"""
        # Implementation from algorithm doc
        pass
```

#### 2.4 Create Assignment Orchestrator
```python
# src/assignment/orchestrator.py
class AssignmentOrchestrator:
    def assign_tasks(
        self,
        tasks: List[Task],
        strategy: str = 'maximize_subscription_usage'
    ) -> Dict[str, Model]:
        """Batch assign models to tasks"""
        pass
```

#### 2.5 Integration Tests
```python
# tests/test_assignment.py
def test_quota_enforcement():
    """Test that quota limits are enforced"""
    pass

def test_high_value_gets_premium_model():
    """Test that high-value tasks get premium models"""
    pass

def test_batch_optimization():
    """Test batch assignment optimization"""
    pass
```

### Deliverables
- [ ] Model selection system
- [ ] Quota tracking with Redis
- [ ] Assignment orchestrator
- [ ] Integration tests
- [ ] API documentation

## Phase 3: Execution Tracking (Week 5-6)

### Goals
- Track task execution outcomes
- Calculate quality scores
- Store performance data

### Tasks

#### 3.1 Set Up Database
```sql
-- migrations/001_create_tables.sql
CREATE TABLE task_executions (
    id UUID PRIMARY KEY,
    task_id VARCHAR(50) NOT NULL,
    -- ... (schema from adaptive learning doc)
);

CREATE INDEX idx_task_executions_model ON task_executions(assigned_model);
CREATE INDEX idx_task_executions_timestamp ON task_executions(timestamp);
```

#### 3.2 Implement Execution Tracker
```python
# src/tracking/execution_tracker.py
class ExecutionTracker:
    def __init__(self, db_connection):
        self.db = db_connection

    def start_execution(self, task: Task, model: Model) -> str:
        """Record task execution start"""
        pass

    def record_completion(
        self,
        execution_id: str,
        success: bool,
        actual_tokens: Tuple[int, int],
        quality_score: float
    ):
        """Record task execution completion"""
        pass

    def get_execution_history(
        self,
        task_id: Optional[str] = None,
        model: Optional[str] = None,
        days: int = 30
    ) -> List[TaskExecution]:
        """Retrieve execution history"""
        pass
```

#### 3.3 Implement Quality Scorer
```python
# src/tracking/quality_scorer.py
class QualityScorer:
    def calculate_quality(
        self,
        task: Task,
        execution_result: dict
    ) -> float:
        """
        Calculate quality score (0-10) based on:
        - Tests passed/failed
        - Bugs introduced
        - Revision needed
        - Code review feedback
        """
        pass
```

#### 3.4 Create Worker Integration
```python
# src/workers/worker_wrapper.py
class WorkerWrapper:
    def __init__(self, tracker, model):
        self.tracker = tracker
        self.model = model

    def execute_task(self, task: Task):
        """Execute task and track outcomes"""
        execution_id = self.tracker.start_execution(task, self.model)

        try:
            # Execute task with worker
            result = self._run_worker(task)

            # Calculate quality
            quality = self._assess_quality(result)

            # Record completion
            self.tracker.record_completion(
                execution_id,
                success=result['success'],
                actual_tokens=(result['input_tokens'], result['output_tokens']),
                quality_score=quality
            )

            return result

        except Exception as e:
            self.tracker.record_completion(
                execution_id,
                success=False,
                actual_tokens=(0, 0),
                quality_score=0.0
            )
            raise
```

### Deliverables
- [ ] PostgreSQL database setup
- [ ] Execution tracking system
- [ ] Quality scoring system
- [ ] Worker integration
- [ ] Database migration scripts

## Phase 4: Learning Engine (Week 7-8)

### Goals
- Implement performance tracking
- Build learning algorithms
- Create A/B testing framework

### Tasks

#### 4.1 Implement Performance Analyzer
```python
# src/learning/performance_analyzer.py
class PerformanceAnalyzer:
    def update_model_performance(self, execution: TaskExecution):
        """Update model performance metrics"""
        # Implementation from adaptive learning doc
        pass

    def get_model_ranking(
        self,
        task: Task,
        models: List[Model]
    ) -> List[Tuple[Model, float]]:
        """Rank models by predicted performance"""
        pass
```

#### 4.2 Implement Calibration Engine
```python
# src/learning/calibration.py
class CalibrationEngine:
    def calibrate_token_estimation(self, window_days: int = 30):
        """Calibrate token estimation formulas"""
        pass

    def calibrate_value_scoring(self, window_days: int = 30):
        """Calibrate value scoring parameters"""
        pass

    def apply_adjustments(self):
        """Apply learned adjustments to production config"""
        pass
```

#### 4.3 Implement A/B Testing
```python
# src/learning/ab_testing.py
class ABTestFramework:
    def create_test(self, test_config: dict) -> ABTest:
        """Create new A/B test"""
        pass

    def assign_to_test(self, task: Task) -> Tuple[str, Optional[str]]:
        """Assign task to test variant"""
        pass

    def analyze_test(self, test_id: str) -> dict:
        """Analyze test results"""
        pass
```

#### 4.4 Create Learning Pipeline
```python
# src/learning/pipeline.py
class LearningPipeline:
    def run_daily_learning(self):
        """Daily learning batch job"""
        # 1. Update model performance
        # 2. Calibrate token estimation
        # 3. Calibrate value scoring
        # 4. Analyze cost effectiveness
        # 5. Check A/B tests
        # 6. Generate report
        pass
```

#### 4.5 Set Up Batch Jobs
```python
# src/jobs/learning_jobs.py
from celery import Celery

app = Celery('control-panel')

@app.task
def daily_learning_job():
    pipeline = LearningPipeline()
    pipeline.run_daily_learning()

# Schedule: daily at 2 AM
app.conf.beat_schedule = {
    'daily-learning': {
        'task': 'jobs.learning_jobs.daily_learning_job',
        'schedule': crontab(hour=2, minute=0),
    },
}
```

### Deliverables
- [ ] Performance tracking system
- [ ] Calibration engine
- [ ] A/B testing framework
- [ ] Learning pipeline
- [ ] Batch job scheduler

## Phase 5: API & CLI (Week 9-10)

### Goals
- Create REST API
- Build CLI tool
- Add monitoring

### Tasks

#### 5.1 Implement REST API
```python
# src/api/main.py
from fastapi import FastAPI, HTTPException

app = FastAPI(title="Control Panel API")

@app.post("/tasks/assign")
async def assign_task(task: TaskCreate):
    """Assign model to task"""
    pass

@app.get("/tasks/{task_id}/execution")
async def get_execution(task_id: str):
    """Get task execution details"""
    pass

@app.get("/models/performance")
async def get_model_performance(model: Optional[str] = None):
    """Get model performance metrics"""
    pass

@app.get("/quota/remaining")
async def get_remaining_quota():
    """Get remaining subscription quotas"""
    pass
```

#### 5.2 Implement CLI
```python
# src/cli/main.py
import click

@click.group()
def cli():
    """Control Panel CLI"""
    pass

@cli.command()
@click.argument('task_id')
def assign(task_id):
    """Assign model to task"""
    pass

@cli.command()
def quota():
    """Show remaining quotas"""
    pass

@cli.command()
@click.option('--days', default=7)
def performance(days):
    """Show model performance"""
    pass

@cli.command()
def calibrate():
    """Run calibration"""
    pass
```

#### 5.3 Add Monitoring
```python
# src/monitoring/metrics.py
from prometheus_client import Counter, Histogram, Gauge

# Metrics
task_assignments = Counter(
    'task_assignments_total',
    'Total task assignments',
    ['model', 'priority']
)

task_quality = Histogram(
    'task_quality_score',
    'Task quality scores',
    ['model', 'domain']
)

quota_remaining = Gauge(
    'subscription_quota_remaining',
    'Remaining subscription quota',
    ['subscription']
)
```

#### 5.4 Create Dashboard
```python
# src/api/dashboard.py
@app.get("/dashboard/stats")
async def get_dashboard_stats():
    """Get dashboard statistics"""
    return {
        'total_tasks_today': ...,
        'avg_quality_score': ...,
        'total_cost_today': ...,
        'quota_utilization': ...,
        'active_ab_tests': ...,
    }
```

### Deliverables
- [ ] REST API with OpenAPI docs
- [ ] CLI tool
- [ ] Prometheus metrics
- [ ] Grafana dashboards
- [ ] API documentation

## Phase 6: Production Deployment (Week 11-12)

### Goals
- Deploy to production
- Set up monitoring
- Create runbooks

### Tasks

#### 6.1 Containerization
```dockerfile
# Dockerfile
FROM python:3.11-slim

WORKDIR /app

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY src/ ./src/
COPY config/ ./config/

CMD ["uvicorn", "src.api.main:app", "--host", "0.0.0.0", "--port", "8000"]
```

#### 6.2 Kubernetes Deployment
```yaml
# k8s/deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: control-panel
  namespace: control-panel
spec:
  replicas: 2
  selector:
    matchLabels:
      app: control-panel
  template:
    metadata:
      labels:
        app: control-panel
    spec:
      containers:
      - name: api
        image: control-panel:latest
        ports:
        - containerPort: 8000
        env:
        - name: DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: control-panel-secrets
              key: database-url
        - name: REDIS_URL
          valueFrom:
            secretKeyRef:
              name: control-panel-secrets
              key: redis-url
```

#### 6.3 Create Runbooks
```markdown
# runbooks/incident-response.md

## High Error Rate Alert

### Symptoms
- Model assignment failures > 5%
- 500 errors in API

### Investigation
1. Check database connectivity
2. Check Redis connectivity
3. Review recent config changes
4. Check model API availability

### Resolution
1. Rollback recent config changes
2. Restart API pods
3. Clear Redis cache if corrupted
```

#### 6.4 Set Up Alerting
```yaml
# monitoring/alerts.yaml
groups:
- name: control-panel
  rules:
  - alert: HighAssignmentFailureRate
    expr: rate(task_assignment_failures_total[5m]) > 0.05
    for: 5m
    annotations:
      summary: High task assignment failure rate

  - alert: ModelPerformanceDegradation
    expr: avg_over_time(task_quality_score[1h]) < 6.0
    for: 1h
    annotations:
      summary: Model performance below threshold
```

### Deliverables
- [ ] Docker images
- [ ] Kubernetes manifests
- [ ] Monitoring setup
- [ ] Runbooks
- [ ] Deployment documentation

## Testing Strategy

### Unit Tests
- Individual components (scorer, estimator, selector)
- Target: >80% coverage

### Integration Tests
- End-to-end assignment flow
- Database operations
- API endpoints

### Performance Tests
- Assignment latency < 100ms
- Handle 1000 tasks/minute
- Database query optimization

### A/B Tests
- New scoring parameters
- Model selection strategies
- Quota optimization approaches

## Success Metrics

### Phase 1-2
- [ ] Assign correct model tier 95%+ of time
- [ ] Token estimation within 30% of actual
- [ ] Assignment latency < 100ms

### Phase 3-4
- [ ] Track 100% of executions
- [ ] Quality score correlation > 0.7
- [ ] Learning adjustments improve performance

### Phase 5-6
- [ ] API uptime > 99.9%
- [ ] Dashboard load time < 2s
- [ ] Zero data loss in production

## Risks & Mitigation

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Token estimation inaccurate | High costs | Medium | Conservative estimates, calibration |
| Model API unavailable | Task failures | Low | Fallback models, retry logic |
| Database performance | Slow assignments | Medium | Indexing, caching, query optimization |
| Learning overfitting | Poor decisions | Medium | A/B testing, confidence thresholds |
| Quota tracking errors | Budget overruns | Low | Double-entry accounting, alerts |

## Timeline Summary

- **Week 1-2**: Core scoring engine
- **Week 3-4**: Model assignment system
- **Week 5-6**: Execution tracking
- **Week 7-8**: Learning engine
- **Week 9-10**: API & CLI
- **Week 11-12**: Production deployment

**Total**: 12 weeks to production-ready system

## Post-Launch

### Week 13-16: Optimization
- Tune learning parameters
- Optimize database queries
- Improve token estimation

### Week 17-20: Enhancement
- Add more model providers
- Advanced cost optimization
- Multi-workspace support

### Ongoing
- Monthly model capability reviews
- Quarterly cost/benefit analysis
- Continuous A/B testing
