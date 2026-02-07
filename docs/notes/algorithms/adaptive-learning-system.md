# Adaptive Learning System

## Overview

The adaptive learning system tracks task outcomes, model performance, and assignment effectiveness to continuously improve model selection decisions. It learns from historical data to optimize value delivery and cost efficiency.

## Learning Objectives

1. **Model Performance by Task Type**: Which models excel at which types of tasks?
2. **Token Estimation Accuracy**: Are we over/under-estimating token usage?
3. **Value Score Calibration**: Do high-value scores correlate with high-value outcomes?
4. **Cost Optimization**: Are we maximizing value per dollar spent?
5. **Subscription Utilization**: Are we efficiently using quota-limited subscriptions?

## Data Collection

### Task Execution Record

```python
@dataclass
class TaskExecution:
    task_id: str
    task_title: str
    priority: str
    domain: str
    complexity: str

    # Predictions
    predicted_value_score: float
    predicted_input_tokens: int
    predicted_output_tokens: int
    predicted_cost: float

    # Assignment
    assigned_model: str
    assignment_reason: str  # e.g., "high_value_premium", "subscription_quota"

    # Actual execution
    actual_input_tokens: int
    actual_output_tokens: int
    actual_cost: float
    execution_time_seconds: float

    # Outcomes
    success: bool
    quality_score: float  # 0-10, determined by validation
    tests_passed: int
    tests_failed: int
    bugs_introduced: int
    revision_needed: bool

    # Stakeholder feedback
    stakeholder_satisfaction: Optional[float]  # 1-5 scale
    value_delivered: Optional[float]  # Subjective value assessment

    # Metadata
    timestamp: datetime
    worker_id: str
    workspace: str


@dataclass
class ModelPerformance:
    model_name: str
    task_type: str  # e.g., "P0_infrastructure_complex"

    # Aggregated metrics
    total_tasks: int
    success_rate: float
    avg_quality_score: float
    avg_execution_time: float
    avg_token_efficiency: float  # actual vs predicted tokens
    avg_cost_efficiency: float   # value delivered per dollar

    # Historical tracking
    last_updated: datetime
    confidence_interval: float  # Statistical confidence in metrics
```

### Database Schema

```sql
-- Task executions
CREATE TABLE task_executions (
    id UUID PRIMARY KEY,
    task_id VARCHAR(50) NOT NULL,
    task_title TEXT,
    priority VARCHAR(10),
    domain VARCHAR(50),
    complexity VARCHAR(50),

    predicted_value_score FLOAT,
    predicted_input_tokens INT,
    predicted_output_tokens INT,
    predicted_cost FLOAT,

    assigned_model VARCHAR(50),
    assignment_reason VARCHAR(100),

    actual_input_tokens INT,
    actual_output_tokens INT,
    actual_cost FLOAT,
    execution_time_seconds FLOAT,

    success BOOLEAN,
    quality_score FLOAT,
    tests_passed INT,
    tests_failed INT,
    bugs_introduced INT,
    revision_needed BOOLEAN,

    stakeholder_satisfaction FLOAT,
    value_delivered FLOAT,

    timestamp TIMESTAMP DEFAULT NOW(),
    worker_id VARCHAR(100),
    workspace TEXT
);

-- Model performance aggregates (materialized view)
CREATE MATERIALIZED VIEW model_performance AS
SELECT
    assigned_model,
    priority,
    domain,
    complexity,
    COUNT(*) as total_tasks,
    AVG(CASE WHEN success THEN 1.0 ELSE 0.0 END) as success_rate,
    AVG(quality_score) as avg_quality_score,
    AVG(execution_time_seconds) as avg_execution_time,
    AVG(actual_input_tokens::float / NULLIF(predicted_input_tokens, 0)) as input_token_accuracy,
    AVG(actual_output_tokens::float / NULLIF(predicted_output_tokens, 0)) as output_token_accuracy,
    AVG(value_delivered / NULLIF(actual_cost, 0)) as value_per_dollar,
    STDDEV(quality_score) as quality_stddev,
    MAX(timestamp) as last_updated
FROM task_executions
WHERE success = true
GROUP BY assigned_model, priority, domain, complexity;

-- Learning adjustments (stores learned parameters)
CREATE TABLE learning_adjustments (
    id UUID PRIMARY KEY,
    parameter_name VARCHAR(100),
    parameter_type VARCHAR(50),  -- 'complexity_multiplier', 'domain_modifier', etc.
    original_value FLOAT,
    adjusted_value FLOAT,
    adjustment_reason TEXT,
    confidence_score FLOAT,
    effective_date TIMESTAMP,
    created_at TIMESTAMP DEFAULT NOW()
);
```

## Learning Algorithms

### 1. Model Performance Learning

```python
def update_model_performance(execution: TaskExecution):
    """
    Update model performance metrics after task completion
    """
    # Create task type signature
    task_type = f"{execution.priority}_{execution.domain}_{execution.complexity}"

    # Retrieve or create performance record
    perf = get_or_create_performance_record(
        model=execution.assigned_model,
        task_type=task_type
    )

    # Update running averages (using exponential moving average)
    alpha = 0.1  # Learning rate

    perf.success_rate = (1 - alpha) * perf.success_rate + alpha * (1.0 if execution.success else 0.0)
    perf.avg_quality_score = (1 - alpha) * perf.avg_quality_score + alpha * execution.quality_score
    perf.avg_execution_time = (1 - alpha) * perf.avg_execution_time + alpha * execution.execution_time_seconds

    # Token efficiency
    input_efficiency = execution.actual_input_tokens / max(execution.predicted_input_tokens, 1)
    output_efficiency = execution.actual_output_tokens / max(execution.predicted_output_tokens, 1)
    token_efficiency = (input_efficiency + output_efficiency) / 2
    perf.avg_token_efficiency = (1 - alpha) * perf.avg_token_efficiency + alpha * token_efficiency

    # Cost efficiency (value per dollar)
    if execution.value_delivered and execution.actual_cost > 0:
        cost_efficiency = execution.value_delivered / execution.actual_cost
        perf.avg_cost_efficiency = (1 - alpha) * perf.avg_cost_efficiency + alpha * cost_efficiency

    perf.total_tasks += 1
    perf.last_updated = datetime.now()

    # Calculate confidence interval (more tasks = higher confidence)
    perf.confidence_interval = min(0.95, 0.5 + (perf.total_tasks / 100) * 0.45)

    save_performance_record(perf)


def get_model_ranking(task: Task, models: List[Model]) -> List[Tuple[Model, float]]:
    """
    Rank models by predicted performance for a given task
    Uses learned performance data
    """
    task_type = f"{task.priority}_{classify_domain(task)}_{estimate_complexity(task)}"

    rankings = []
    for model in models:
        perf = get_performance_record(model.name, task_type)

        if perf and perf.confidence_interval > 0.3:
            # Use learned performance
            expected_quality = perf.avg_quality_score
            expected_success_rate = perf.success_rate
            expected_cost_efficiency = perf.avg_cost_efficiency

            # Combined score: quality * success_rate * cost_efficiency
            performance_score = expected_quality * expected_success_rate * expected_cost_efficiency

        else:
            # Use default capability matrix (cold start)
            performance_score = get_default_capability_score(model, task)

        rankings.append((model, performance_score))

    # Sort by performance score (descending)
    rankings.sort(key=lambda x: x[1], reverse=True)
    return rankings
```

### 2. Token Estimation Calibration

```python
def calibrate_token_estimation(window_days: int = 30):
    """
    Analyze recent task executions and adjust token estimation formulas
    """
    # Retrieve recent executions
    executions = get_recent_executions(days=window_days)

    # Group by complexity
    complexity_groups = {}
    for exec in executions:
        complexity = exec.complexity
        if complexity not in complexity_groups:
            complexity_groups[complexity] = []
        complexity_groups[complexity].append(exec)

    adjustments = {}

    for complexity, execs in complexity_groups.items():
        # Calculate actual vs predicted ratios
        input_ratios = [e.actual_input_tokens / max(e.predicted_input_tokens, 1) for e in execs]
        output_ratios = [e.actual_output_tokens / max(e.predicted_output_tokens, 1) for e in execs]

        avg_input_ratio = np.mean(input_ratios)
        avg_output_ratio = np.mean(output_ratios)

        # If consistently over/under estimating, adjust base estimates
        if abs(avg_input_ratio - 1.0) > 0.15:  # More than 15% off
            adjustment_factor = avg_input_ratio
            adjustments[f"input_tokens_{complexity}"] = adjustment_factor

            log_adjustment(
                parameter_name=f"token_estimate_input_{complexity}",
                parameter_type="token_estimation",
                adjustment_factor=adjustment_factor,
                reason=f"Avg actual/predicted ratio: {avg_input_ratio:.2f}",
                confidence=min(len(execs) / 50, 1.0)
            )

        if abs(avg_output_ratio - 1.0) > 0.15:
            adjustment_factor = avg_output_ratio
            adjustments[f"output_tokens_{complexity}"] = adjustment_factor

            log_adjustment(
                parameter_name=f"token_estimate_output_{complexity}",
                parameter_type="token_estimation",
                adjustment_factor=adjustment_factor,
                reason=f"Avg actual/predicted ratio: {avg_output_ratio:.2f}",
                confidence=min(len(execs) / 50, 1.0)
            )

    return adjustments


def apply_learned_token_adjustments(complexity: str, base_input: int, base_output: int) -> Tuple[int, int]:
    """
    Apply learned adjustments to token estimates
    """
    input_adj = get_latest_adjustment(f"token_estimate_input_{complexity}")
    output_adj = get_latest_adjustment(f"token_estimate_output_{complexity}")

    adjusted_input = int(base_input * input_adj) if input_adj else base_input
    adjusted_output = int(base_output * output_adj) if output_adj else base_output

    return (adjusted_input, adjusted_output)
```

### 3. Value Score Calibration

```python
def calibrate_value_scoring(window_days: int = 30):
    """
    Validate that predicted value scores align with actual value delivered
    """
    executions = get_recent_executions(days=window_days)

    # Filter to tasks with stakeholder feedback
    executions = [e for e in executions if e.value_delivered is not None]

    if len(executions) < 20:
        # Not enough data yet
        return

    # Analyze correlation between predicted value score and actual value
    predicted_scores = [e.predicted_value_score for e in executions]
    actual_values = [e.value_delivered for e in executions]

    correlation = np.corrcoef(predicted_scores, actual_values)[0, 1]

    if correlation < 0.6:
        # Low correlation - investigate which components are off

        # Test priority weight correlation
        priority_groups = {}
        for e in executions:
            if e.priority not in priority_groups:
                priority_groups[e.priority] = []
            priority_groups[e.priority].append(e)

        for priority, group in priority_groups.items():
            avg_predicted = np.mean([e.predicted_value_score for e in group])
            avg_actual = np.mean([e.value_delivered for e in group])

            ratio = avg_actual / avg_predicted

            if abs(ratio - 1.0) > 0.2:
                # Suggest priority weight adjustment
                current_weight = get_priority_weight(priority)
                suggested_weight = current_weight * ratio

                log_adjustment(
                    parameter_name=f"priority_weight_{priority}",
                    parameter_type="value_scoring",
                    adjustment_factor=ratio,
                    reason=f"Priority {priority}: predicted {avg_predicted:.1f}, actual {avg_actual:.1f}",
                    confidence=min(len(group) / 30, 1.0)
                )

    # Test domain modifier correlation
    domain_groups = {}
    for e in executions:
        if e.domain not in domain_groups:
            domain_groups[e.domain] = []
        domain_groups[e.domain].append(e)

    for domain, group in domain_groups.items():
        if len(group) < 5:
            continue

        avg_predicted = np.mean([e.predicted_value_score for e in group])
        avg_actual = np.mean([e.value_delivered for e in group])

        ratio = avg_actual / avg_predicted

        if abs(ratio - 1.0) > 0.2:
            current_modifier = get_domain_modifier(domain)
            suggested_modifier = current_modifier * ratio

            log_adjustment(
                parameter_name=f"domain_modifier_{domain}",
                parameter_type="value_scoring",
                adjustment_factor=ratio,
                reason=f"Domain {domain}: predicted {avg_predicted:.1f}, actual {avg_actual:.1f}",
                confidence=min(len(group) / 20, 1.0)
            )
```

### 4. Cost Optimization Learning

```python
def analyze_cost_effectiveness():
    """
    Identify opportunities to reduce costs while maintaining quality
    """
    executions = get_recent_executions(days=60)

    # Find cases where expensive models were used for tasks that could have been handled cheaper
    downgrade_opportunities = []

    for exec in executions:
        if exec.assigned_model in ['opus-4.6', 'gpt-4']:
            # Check if cheaper models perform similarly on this task type
            task_type = f"{exec.priority}_{exec.domain}_{exec.complexity}"

            # Get performance of cheaper models on similar tasks
            cheaper_models = ['sonnet-4.5', 'deepseek-v3', 'qwen2.5-coder']

            for cheaper_model in cheaper_models:
                perf = get_performance_record(cheaper_model, task_type)
                exec_model_perf = get_performance_record(exec.assigned_model, task_type)

                if perf and exec_model_perf:
                    # Quality difference
                    quality_diff = exec_model_perf.avg_quality_score - perf.avg_quality_score

                    # Cost difference
                    cheaper_cost = estimate_task_cost(cheaper_model, exec)
                    cost_savings = exec.actual_cost - cheaper_cost

                    # If quality difference is small but cost savings is large
                    if quality_diff < 0.5 and cost_savings > 1.0:
                        downgrade_opportunities.append({
                            'task_type': task_type,
                            'current_model': exec.assigned_model,
                            'suggested_model': cheaper_model,
                            'quality_diff': quality_diff,
                            'cost_savings': cost_savings,
                            'confidence': min(perf.total_tasks / 10, 1.0)
                        })

    # Log findings
    for opp in downgrade_opportunities:
        if opp['confidence'] > 0.5:
            log_adjustment(
                parameter_name=f"model_preference_{opp['task_type']}",
                parameter_type="cost_optimization",
                adjustment_factor=0,  # Not a numeric adjustment
                reason=f"Can downgrade from {opp['current_model']} to {opp['suggested_model']} "
                       f"with minimal quality loss ({opp['quality_diff']:.2f}) "
                       f"and save ${opp['cost_savings']:.2f} per task",
                confidence=opp['confidence']
            )

    return downgrade_opportunities
```

### 5. A/B Testing Framework

```python
@dataclass
class ABTest:
    test_id: str
    name: str
    description: str

    # Test parameters
    control_strategy: str  # e.g., "current_algorithm"
    treatment_strategy: str  # e.g., "learned_adjustments"

    # Assignment
    assignment_ratio: float  # 0.5 = 50/50 split

    # Tracking
    start_date: datetime
    end_date: Optional[datetime]
    min_samples: int  # Minimum tasks per group

    # Results
    control_metrics: Dict[str, float]
    treatment_metrics: Dict[str, float]
    statistical_significance: Optional[float]
    winner: Optional[str]


def assign_to_ab_test(task: Task, active_tests: List[ABTest]) -> Tuple[str, Optional[str]]:
    """
    Determine if task should be part of A/B test and which variant
    """
    for test in active_tests:
        if test.end_date and datetime.now() > test.end_date:
            continue

        # Check if task matches test criteria
        if task_matches_test_criteria(task, test):
            # Randomly assign based on ratio
            if random.random() < test.assignment_ratio:
                return ('control', test.test_id)
            else:
                return ('treatment', test.test_id)

    return ('production', None)


def analyze_ab_test_results(test: ABTest):
    """
    Analyze A/B test results and determine winner
    """
    # Retrieve executions for both groups
    control_execs = get_test_executions(test.test_id, 'control')
    treatment_execs = get_test_executions(test.test_id, 'treatment')

    if len(control_execs) < test.min_samples or len(treatment_execs) < test.min_samples:
        # Not enough data yet
        return None

    # Calculate metrics for each group
    control_metrics = calculate_group_metrics(control_execs)
    treatment_metrics = calculate_group_metrics(treatment_execs)

    # Statistical significance test
    # Primary metric: value per dollar
    control_vpd = [e.value_delivered / e.actual_cost for e in control_execs if e.value_delivered and e.actual_cost > 0]
    treatment_vpd = [e.value_delivered / e.actual_cost for e in treatment_execs if e.value_delivered and e.actual_cost > 0]

    # Two-sample t-test
    from scipy import stats
    t_stat, p_value = stats.ttest_ind(control_vpd, treatment_vpd)

    test.control_metrics = control_metrics
    test.treatment_metrics = treatment_metrics
    test.statistical_significance = p_value

    # Determine winner (p < 0.05 for significance)
    if p_value < 0.05:
        if np.mean(treatment_vpd) > np.mean(control_vpd):
            test.winner = 'treatment'
        else:
            test.winner = 'control'
    else:
        test.winner = None  # No significant difference

    return test


def calculate_group_metrics(executions: List[TaskExecution]) -> Dict[str, float]:
    """
    Calculate aggregate metrics for an A/B test group
    """
    return {
        'avg_quality_score': np.mean([e.quality_score for e in executions]),
        'success_rate': np.mean([1.0 if e.success else 0.0 for e in executions]),
        'avg_cost': np.mean([e.actual_cost for e in executions]),
        'avg_value_delivered': np.mean([e.value_delivered for e in executions if e.value_delivered]),
        'value_per_dollar': np.mean([e.value_delivered / e.actual_cost for e in executions
                                      if e.value_delivered and e.actual_cost > 0]),
        'avg_execution_time': np.mean([e.execution_time_seconds for e in executions]),
        'total_tasks': len(executions)
    }
```

## Learning Workflow

```
┌─────────────────────────────────────┐
│  Task Execution Completes           │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│  Record Execution Data              │
│  - Predicted vs actual tokens       │
│  - Predicted vs actual cost         │
│  - Quality score                    │
│  - Success/failure                  │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│  Update Model Performance           │
│  - Success rate by task type        │
│  - Quality score by task type       │
│  - Token efficiency                 │
│  - Cost efficiency                  │
└──────────────┬──────────────────────┘
               │
               ▼
        ┌──────┴──────────┐
        │   Daily Batch   │
        │   Learning      │
        └──────┬──────────┘
               │
        ┌──────┴──────────────────────────────┐
        │                                     │
        ▼                                     ▼
┌──────────────────────┐          ┌──────────────────────┐
│ Calibrate Token      │          │ Calibrate Value      │
│ Estimation           │          │ Scoring              │
│ - Adjust complexity  │          │ - Adjust priority    │
│   multipliers        │          │   weights            │
│ - Adjust domain      │          │ - Adjust domain      │
│   factors            │          │   modifiers          │
└──────────┬───────────┘          └──────────┬───────────┘
           │                                 │
           └────────────┬────────────────────┘
                        │
                        ▼
           ┌────────────────────────┐
           │ Analyze Cost           │
           │ Effectiveness          │
           │ - Find downgrade opps  │
           │ - Optimize assignments │
           └────────────┬───────────┘
                        │
                        ▼
           ┌────────────────────────┐
           │ Update A/B Tests       │
           │ - Check significance   │
           │ - Declare winners      │
           │ - Graduate learnings   │
           └────────────┬───────────┘
                        │
                        ▼
           ┌────────────────────────┐
           │ Generate Learning      │
           │ Report                 │
           │ - Adjustments made     │
           │ - Confidence scores    │
           │ - Recommendations      │
           └────────────────────────┘
```

## Example A/B Tests

### Test 1: Adjusted Complexity Multipliers

```python
test_complexity = ABTest(
    test_id='test-001',
    name='Adjusted Complexity Multipliers',
    description='Test if learned complexity adjustments improve value delivery',
    control_strategy='original_multipliers',
    treatment_strategy='learned_multipliers',
    assignment_ratio=0.5,
    start_date=datetime.now(),
    min_samples=50
)

# Control: Use original multipliers (0.5, 1.0, 1.5, 2.0)
# Treatment: Use learned multipliers (0.6, 1.1, 1.7, 2.3)
# Measure: Value per dollar, quality score, success rate
```

### Test 2: Domain-Specific Model Preferences

```python
test_domain_models = ABTest(
    test_id='test-002',
    name='Infrastructure Model Preference',
    description='Test if DeepSeek can handle P2 infrastructure tasks vs Sonnet',
    control_strategy='sonnet_for_p2_infra',
    treatment_strategy='deepseek_for_p2_infra',
    assignment_ratio=0.5,
    start_date=datetime.now(),
    min_samples=30
)

# Control: Assign Sonnet 4.5 to P2 infrastructure tasks
# Treatment: Assign DeepSeek V3 to P2 infrastructure tasks
# Measure: Quality score, success rate, cost savings
```

### Test 3: Aggressive Subscription Usage

```python
test_subscription = ABTest(
    test_id='test-003',
    name='Maximize Subscription ROI',
    description='Test aggressive subscription filling strategy',
    control_strategy='conservative_subscription_usage',
    treatment_strategy='aggressive_subscription_filling',
    assignment_ratio=0.5,
    start_date=datetime.now(),
    min_samples=100
)

# Control: Use subscription for tasks >= 70 value score
# Treatment: Use subscription for tasks >= 50 value score (fill quota faster)
# Measure: Total value delivered, subscription utilization, overflow API costs
```

## Implementation Recommendations

### Phase 1: Data Collection (Week 1-2)
1. Implement task execution tracking
2. Set up database schema
3. Log all task assignments and outcomes
4. Collect baseline metrics

### Phase 2: Basic Learning (Week 3-4)
1. Implement model performance tracking
2. Build token estimation calibration
3. Create daily learning batch jobs
4. Generate weekly learning reports

### Phase 3: Advanced Learning (Week 5-6)
1. Implement value score calibration
2. Build cost optimization analysis
3. Create A/B testing framework
4. Set up confidence scoring

### Phase 4: Production Learning (Week 7+)
1. Graduate high-confidence learnings to production
2. Run continuous A/B tests
3. Monitor for drift and degradation
4. Quarterly model capability reviews

## Monitoring and Alerting

```python
# Alert if learned adjustments are drifting
if abs(adjustment_factor - 1.0) > 0.5:
    alert("Large adjustment detected", parameter_name, adjustment_factor)

# Alert if model performance degrades
if current_success_rate < historical_avg - 2 * stddev:
    alert("Model performance degradation", model_name, task_type)

# Alert if A/B test reaches significance
if p_value < 0.05 and samples >= min_samples:
    alert("A/B test significant result", test_id, winner)

# Alert if cost efficiency drops
if current_value_per_dollar < target_value_per_dollar * 0.8:
    alert("Cost efficiency below threshold", current_vpd, target_vpd)
```
