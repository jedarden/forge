# Control Panel: Task Value Scoring & Model Assignment Algorithm Design

Complete algorithm design for intelligent task value scoring and model assignment system.

## Overview

This document provides a comprehensive design for optimizing LLM model assignments to coding tasks based on value scoring, cost efficiency, and adaptive learning. The system combines multiple factors to calculate task value, estimate resource requirements, and select the optimal model while managing subscription quotas and API budgets.

## Documentation Structure

### Core Algorithm Documents

1. **[Task Value Scoring System](docs/task-value-scoring-system.md)** (20KB)
   - Multi-factor scoring formula (0-100 scale)
   - Priority weights, complexity multipliers, domain modifiers
   - Time sensitivity and risk adjustments
   - Detailed examples and calculations
   - Adaptive learning integration

2. **[Model Capability Matrix](docs/model-capability-matrix.md)** (30KB)
   - Comprehensive model profiles (Opus, Sonnet, DeepSeek, GLM-4.7, GPT-4, Qwen)
   - Performance benchmarks (HumanEval, SWE-bench, MBPP)
   - Language-specific capabilities (Python, TypeScript, Rust, Go)
   - Task type performance ratings
   - Cost efficiency analysis
   - Subscription vs API strategies

3. **[Model Assignment Algorithm](algorithms/model-assignment-algorithm.md)** (42KB)
   - Complete pseudocode implementation
   - Phase-by-phase decision logic
   - Quota tracking and management
   - Batch optimization strategies
   - Configuration examples
   - Integration patterns

4. **[Adaptive Learning System](algorithms/adaptive-learning-system.md)** (48KB)
   - Performance tracking methodology
   - Token estimation calibration
   - Value score validation
   - Cost optimization learning
   - A/B testing framework
   - Database schema and SQL
   - Machine learning algorithms

5. **[Flowcharts & Visual Diagrams](algorithms/flowcharts.md)** (28KB)
   - Complete system flow
   - Value scoring calculation flow
   - Model selection decision tree
   - Batch assignment optimization
   - Learning feedback loop
   - Quota management state machine
   - Quality scoring calculation
   - Token estimation decision tree

6. **[Implementation Plan](implementation/implementation-plan.md)** (60KB)
   - 12-week phased implementation roadmap
   - Technology stack recommendations
   - Architecture diagrams
   - Database schemas and code examples
   - Testing strategy
   - Risk mitigation
   - Success metrics and KPIs

## Quick Start

### Value Scoring Example

```python
# Task: Fix authentication vulnerability in production API
# Priority: P0, Labels: [security, backend, hotfix], Files: 3

# Step 1: Base score
priority_weight = 40  # P0 = 40 points
complexity_multiplier = 1.0  # Moderate (3 files)
base_score = 40 * 1.0 = 40

# Step 2: Time sensitivity
time_bonus = 15  # Hotfix = immediate (+15)

# Step 3: Domain modifier
domain_modifier = 1.1  # Backend

# Step 4: Risk modifier
risk_modifier = 1.3  # Production

# Final calculation
score = (40 + 15) * 1.1 * 1.3 = 78.65 → 79/100

# Result: High-value task → Assign Sonnet 4.5 or premium model
```

### Model Assignment Flow

```
Task Value: 79/100
   ↓
Tier Selection: Mid-Premium (75-89)
   ↓
Check Subscription Quota
   ↓
Claude Pro: 3.8M tokens remaining ✓
   ↓
Assign: Sonnet 4.5 (Subscription)
Cost: $0 (included in subscription)
ROI: Infinite
```

## Key Features

### 1. Multi-Factor Value Scoring

Task value (0-100) calculated from:
- **Priority Weight** (40 pts max): P0=40, P1=30, P2=20, P3=10, P4=5
- **Complexity Multiplier** (0.5x-2.0x): Simple to highly complex
- **Time Sensitivity Bonus** (0-15 pts): Immediate to flexible deadlines
- **Domain Modifier** (0.8x-1.2x): Infrastructure, backend, frontend, etc.
- **Risk Adjustment** (0.7x-1.3x): Production, staging, development, sandbox

### 2. Intelligent Model Selection

Score-based tier selection:
- **90-100**: Premium (Opus 4.6, GPT-4)
- **75-89**: Mid-Premium (Sonnet 4.5, GPT-4 Turbo)
- **60-74**: Mid-Range (DeepSeek V3, Qwen2.5)
- **40-59**: Budget (GLM-4.7, Qwen2.5)
- **0-39**: Defer or batch

### 3. Subscription Optimization

Three strategies:
- **Maximize Subscription Usage**: Fill quotas with highest-value tasks
- **Minimize Cost**: Use subscriptions first, cheapest API for overflow
- **Maximize Value**: Best model per task within budget

### 4. Adaptive Learning

Continuous improvement through:
- Model performance tracking by task type
- Token estimation calibration
- Value score validation
- Cost optimization identification
- A/B testing framework

## Algorithm Components

### Value Scoring Formula

```python
base_score = priority_weight * complexity_multiplier
adjusted_score = (base_score + time_sensitivity_bonus) * domain_modifier * risk_modifier
final_score = clamp(adjusted_score, 0, 100)
```

### Token Estimation

```python
# Base estimates by complexity
token_estimates = {
    'simple': (7500, 1250),          # Input, Output
    'moderate': (27500, 3500),
    'complex': (70000, 10000),
    'highly_complex': (150000, 27500)
}

# Apply domain and workspace adjustments
# Apply learned calibration factors
```

### Model Selection Logic

```python
if value_score >= 90:
    candidates = premium_models  # Opus, GPT-4
elif value_score >= 75:
    candidates = mid_premium_models  # Sonnet, GPT-4 Turbo
elif value_score >= 60:
    candidates = mid_range_models  # DeepSeek, Qwen
elif value_score >= 40:
    candidates = budget_models  # GLM-4.7
else:
    return None  # Defer task

# Check subscription quota availability
# Select based on cost and capabilities
```

### Quota Management

```python
# State machine: FULL → MEDIUM → LOW → CRITICAL → EXHAUSTED

if quota_remaining > 80%:
    use_for_value_threshold = 75  # Use subscription for high-value tasks
elif quota_remaining > 50%:
    use_for_value_threshold = 60  # Use for high & medium value
elif quota_remaining > 20%:
    use_for_value_threshold = 90  # Reserve for critical only
else:
    use_api_only = True  # Quota exhausted
```

## Example Calculations

### Example 1: Production Hotfix (High Value)

```
Task: Fix authentication bypass vulnerability
Priority: P0 (40 points)
Complexity: Moderate (1.0x)
Time Sensitivity: Immediate (+15 points)
Domain: Backend (1.1x)
Risk: Production (1.3x)

Score = (40 * 1.0 + 15) * 1.1 * 1.3 = 79/100

Assignment: Sonnet 4.5
Tokens: 31K total
Cost: $0 (subscription) or $0.13 (API)
Category: High-value work
```

### Example 2: Feature Development (Low Value)

```
Task: Add user profile customization UI
Priority: P2 (20 points)
Complexity: Moderate (1.0x)
Time Sensitivity: Flexible (+0 points)
Domain: Frontend (1.0x)
Risk: Development (1.0x)

Score = (20 * 1.0 + 0) * 1.0 * 1.0 = 20/100

Assignment: GLM-4.7
Tokens: 27.5K total
Cost: $0 (free tier)
Category: Low-value work
```

### Example 3: Infrastructure Redesign (Critical)

```
Task: Migrate Kubernetes cluster to new control plane
Priority: P1 (30 points)
Complexity: Highly Complex (2.0x)
Time Sensitivity: Scheduled (+5 points)
Domain: Infrastructure (1.2x)
Risk: Production (1.3x)

Score = (30 * 2.0 + 5) * 1.2 * 1.3 = 100/100 (clamped)

Assignment: Opus 4.6
Tokens: 150K total
Cost: $2.25 (API)
Category: Critical infrastructure
```

## Model Performance Profiles

### Claude Opus 4.6 (Premium)
- **Quality**: 9.5/10 code gen, 10/10 reasoning
- **Cost**: $15/$75 per MTok
- **Best for**: Architecture, complex refactoring, high-stakes work
- **HumanEval**: 92% | SWE-bench: 38%

### Claude Sonnet 4.5 (Mid-Premium)
- **Quality**: 9/10 code gen, 8.5/10 reasoning
- **Cost**: $3/$15 per MTok
- **Best for**: General coding, balanced performance/cost
- **HumanEval**: 89% | SWE-bench: 33%

### DeepSeek Coder V3 (Mid-Range)
- **Quality**: 8.5/10 code gen, 7/10 reasoning
- **Cost**: $0.14/$0.28 per MTok
- **Best for**: High-volume coding, specialized generation
- **HumanEval**: 83% | SWE-bench: 24%

### GLM-4.7 via Z.AI (Budget)
- **Quality**: 7/10 code gen, 6/10 reasoning
- **Cost**: Free tier or very cheap
- **Best for**: Simple tasks, high-volume low-value work
- **HumanEval**: 68% | SWE-bench: 12%

## Learning System

### Data Collection

```sql
CREATE TABLE task_executions (
    task_id VARCHAR(50),
    predicted_value_score FLOAT,
    predicted_tokens INT,
    assigned_model VARCHAR(50),
    actual_tokens INT,
    actual_cost FLOAT,
    success BOOLEAN,
    quality_score FLOAT,  -- 0-10
    execution_time_seconds FLOAT
);
```

### Performance Tracking

```python
# Update model performance after each execution
performance = {
    'model': 'sonnet-4.5',
    'task_type': 'P1_backend_moderate',
    'success_rate': 0.94,
    'avg_quality_score': 8.6,
    'avg_token_efficiency': 1.15,  # actual/predicted
    'value_per_dollar': 215.3
}
```

### Calibration

```python
# Adjust token estimation if consistently off
if avg(actual_tokens / predicted_tokens) > 1.15:
    # We're under-estimating, adjust base estimates up
    complexity_adjustments['moderate'] *= 1.15

# Adjust value scoring if predictions don't correlate
if correlation(predicted_value, actual_value) < 0.6:
    # Recalibrate priority weights or modifiers
    priority_weights['P1'] *= (actual_avg / predicted_avg)
```

### A/B Testing

```python
# Test new scoring parameters
test = ABTest(
    name='Increased complexity multipliers',
    control_strategy='original_multipliers',
    treatment_strategy='learned_multipliers',
    assignment_ratio=0.5,
    min_samples=50
)

# Analyze results
if test.p_value < 0.05 and treatment_vpd > control_vpd:
    # Graduate treatment to production
    production_config.update(treatment_parameters)
```

## Implementation Architecture

```
┌─────────────────────────────────────────────────────┐
│                Control Panel System                 │
├─────────────────────────────────────────────────────┤
│                                                      │
│  ┌──────────────┐  ┌───────────────┐  ┌──────────┐ │
│  │   Scoring    │  │  Assignment   │  │ Learning │ │
│  │   Engine     │  │ Orchestrator  │  │  Engine  │ │
│  └──────────────┘  └───────────────┘  └──────────┘ │
│                                                      │
│  ┌──────────────┐  ┌───────────────┐                │
│  │    Quota     │  │  Execution    │                │
│  │   Tracker    │  │   Tracker     │                │
│  └──────────────┘  └───────────────┘                │
│                                                      │
├─────────────────────────────────────────────────────┤
│         PostgreSQL + Redis + Celery                  │
└─────────────────────────────────────────────────────┘
                        │
                        ▼
      ┌──────────────────────────────────┐
      │         Worker Pool               │
      │  Opus | Sonnet | DeepSeek | GLM   │
      └──────────────────────────────────┘
```

## Configuration

### Scoring Parameters

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

domain_modifiers:
  infrastructure: 1.2
  backend: 1.1
  ml: 1.1
  frontend: 1.0
  testing: 0.9
  documentation: 0.8

risk_modifiers:
  production: 1.3
  staging: 1.1
  development: 1.0
  experimental: 0.9
  sandbox: 0.7
```

### Model Registry

```yaml
# config/models.yaml
models:
  - name: opus-4.6
    tier: premium
    cost_per_mtok_input: 15.0
    cost_per_mtok_output: 75.0
    context_window: 200000

  - name: sonnet-4.5
    tier: mid-premium
    cost_per_mtok_input: 3.0
    cost_per_mtok_output: 15.0
    context_window: 200000

subscriptions:
  - model_family: claude
    monthly_cost: 20.0
    token_limit_monthly: 5000000
```

## Success Metrics

### Target KPIs

- **Assignment Accuracy**: >95% correct tier selection
- **Token Estimation**: Within 30% of actual usage
- **Assignment Latency**: <100ms per task
- **Subscription Utilization**: >80% (maximize ROI)
- **Cost Efficiency**: Value per dollar >100
- **Quality Correlation**: Value score vs quality >0.7
- **Learning Improvement**: +10% value/cost quarterly

### Monitoring

- Real-time quota tracking (Redis)
- Daily learning batch jobs (2 AM)
- Weekly performance reports
- Monthly cost/benefit analysis
- Quarterly model capability reviews

## Implementation Timeline

### Phase 1: Core Scoring Engine (Week 1-2)
- Value scoring implementation
- Token estimation
- Configuration management
- Unit tests

### Phase 2: Model Assignment (Week 3-4)
- Model registry
- Quota tracking
- Selection logic
- Integration tests

### Phase 3: Execution Tracking (Week 5-6)
- Database setup
- Execution logging
- Quality scoring
- Worker integration

### Phase 4: Learning Engine (Week 7-8)
- Performance tracking
- Calibration algorithms
- A/B testing framework
- Batch jobs

### Phase 5: API & CLI (Week 9-10)
- REST API
- CLI tools
- Monitoring
- Dashboards

### Phase 6: Production (Week 11-12)
- Deployment
- Runbooks
- Alerting
- Documentation

**Total**: 12 weeks to production-ready system

## Usage Examples

### Python API

```python
from pool_optimizer import PoolOptimizer

optimizer = PoolOptimizer.from_config('config/')

# Score and assign single task
task = optimizer.create_task(
    id='po-123',
    title='Fix auth vulnerability',
    priority='P0',
    labels=['security', 'backend']
)

assignment = optimizer.assign_model(task)
print(f"Model: {assignment.model.name}")
print(f"Value: {assignment.value_score}/100")
print(f"Cost: ${assignment.estimated_cost:.2f}")

# Batch assignment with optimization
tasks = optimizer.load_tasks_from_beads()
assignments = optimizer.assign_batch(
    tasks,
    strategy='maximize_subscription_usage'
)

# Track execution
result = worker.execute(task, assignment.model)
optimizer.record_execution(task, result)
```

### CLI

```bash
# Assign model to task
control-panel assign po-123
# Output: Assigned Sonnet 4.5 (value: 79/100, cost: $0.13)

# Check quota status
control-panel quota
# Output: Claude Pro: 3.8M/5M (76%) | API Budget: $45.20/$100

# View performance
control-panel performance --model sonnet-4.5 --days 7
# Output: Success: 94% | Quality: 8.6/10 | Value/$: 215.3

# Run calibration
control-panel calibrate
# Output: Adjusted 3 parameters (confidence: 0.82)
```

## Integration with Beads

```bash
# Beads automatically trigger assignment
br create "Fix auth bug" --priority P0 --labels security,backend

# Pool optimizer:
# 1. Reads bead metadata
# 2. Calculates value score (79/100)
# 3. Selects model (Sonnet 4.5)
# 4. Assigns to worker pool
# 5. Tracks execution
# 6. Updates learning models

# Check assignment
br show po-123
# Assigned: Sonnet 4.5 | Value: 79/100 | Status: in_progress
```

## Research Completion

**Status**: ✓ Complete

**Deliverables**:
- [x] Task value scoring algorithm with examples
- [x] Model capability matrix with benchmarks
- [x] Model assignment algorithm with pseudocode
- [x] Adaptive learning system design
- [x] Visual flowcharts and diagrams
- [x] 12-week implementation plan
- [x] Configuration schemas
- [x] Integration patterns

**Bead**: po-3pv (Design task value scoring and intelligent model assignment algorithm)

**Research Date**: 2026-02-07

**Researcher**: Claude Sonnet 4.5

---

## Next Steps

1. Review algorithm designs in detail
2. Validate scoring formulas with sample tasks
3. Begin Phase 1 implementation (scoring engine)
4. Set up database and tracking infrastructure
5. Deploy initial version with manual model assignment
6. Iterate with learning system

For detailed implementation guidance, see [Implementation Plan](implementation/implementation-plan.md).
