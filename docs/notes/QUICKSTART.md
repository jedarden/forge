# Control Panel Quick Start Guide

## Overview

The Control Panel Cost Management System implements two complementary strategies:

1. **Use-or-Lose Optimization**: Maximizes value from fixed-quota subscriptions
2. **Pay-Per-Token Optimization**: Routes API tasks to cost-effective models based on value

## Installation

```bash
# Install required dependencies
pip install pyyaml

# Copy configuration template
cp optimizer-config.yaml my-config.yaml

# Edit configuration with your settings
nano my-config.yaml
```

## Configuration

### 1. Set Your Budget

Edit `my-config.yaml`:

```yaml
budget:
  monthly_total: 100.00        # Your total monthly budget
  subscription_fixed: 40.00    # Fixed subscription costs
  api_variable: 60.00          # Variable API budget
```

### 2. Enable Your Subscriptions

```yaml
subscriptions:
  claude_pro:
    enabled: true              # Set to true if you have subscription
    monthly_cost: 20.00
    billing_day: 15            # Day of month quota resets
    estimated_quota_tokens: 10_000_000

  cursor_pro:
    enabled: true
    monthly_cost: 20.00
    billing_day: 15
    estimated_quota_tokens: 5_000_000
```

### 3. Configure API Services

```yaml
api_services:
  deepseek:
    enabled: true              # Enable ultra-cheap overflow

  anthropic:
    enabled: true              # Enable Claude API models

  openai:
    enabled: true              # Enable GPT models
```

## Usage

### Basic Usage

```python
from cost_optimizer import PoolOptimizer, Model, Subscription, Task
from datetime import datetime, timedelta

# Define your models
models = {
    'deepseek': Model('DeepSeek V3', 'deepseek', ServiceType.API, 0.14, 0.28, 64000, 65),
    'sonnet': Model('Claude 3.5 Sonnet', 'anthropic', ServiceType.API, 3.0, 15.0, 200000, 90),
}

# Define your subscriptions
subscriptions = [
    Subscription(
        name='Claude Pro',
        model=models['sonnet'],
        monthly_cost=20.0,
        billing_period_start=datetime(2026, 2, 1),
        billing_period_end=datetime(2026, 3, 1),
        estimated_quota_tokens=10_000_000,
        used_tokens=3_200_000,
    ),
]

# Initialize optimizer
optimizer = PoolOptimizer(
    subscriptions=subscriptions,
    available_models=models,
    monthly_budget=100.0,
)

# Create a task
task = Task(
    id='T1',
    description='Generate test suite',
    estimated_tokens=50_000,
    affects_revenue=False,
    improves_efficiency=True,
    deadline_hours=48,
)

# Allocate task to optimal service
model, subscription, metadata = optimizer.allocate_task(task)

print(f"Allocated to: {metadata['service']}")
print(f"Cost: ${metadata['cost']:.2f}")
```

### Running the Demo

```bash
python cost_optimizer.py
```

This will run a demonstration showing:
- Task allocation decisions
- Subscription vs API routing
- Cost-benefit analysis
- Optimization report
- Monthly cost simulation

## Key Features

### 1. Automatic Quota Tracking

The system automatically tracks subscription usage and calculates urgency scores:

```
Claude Pro: 6.2M / 10M tokens (62%) - 8 days left
Status: ON TRACK - Continue normal usage
Cost Savings vs API: $74.40
```

### 2. Intelligent Task Routing

Tasks are routed based on multiple factors:

- **Task Value Score** (0-100): Business impact, urgency, complexity
- **Quota Urgency** (0.0-1.0): How aggressively to use subscriptions
- **Budget Constraints**: Available API budget
- **Quality Requirements**: Minimum model quality needed

### 3. Cost-Benefit Analysis

Every API task undergoes cost-benefit analysis:

```
Expected Value: $47.50
Cost: $0.60
ROI: 79.2x
Decision: APPROVED
```

### 4. Acceleration Strategies

When subscriptions are under-utilized, the system suggests tasks to maximize value:

- Generate comprehensive documentation
- Create extensive test suites
- Perform security audits
- Run code reviews
- Create architecture diagrams

### 5. Real-Time Monitoring

Track costs and utilization in real-time:

```
Month: February 2026
Total Budget: $100.00
Spent: $47.32 (47%)
Projected: $84.50 (15% under budget ✓)
```

## Decision Tree

```
Task arrives
    │
    ├─> Check subscriptions with quota
    │   │
    │   ├─> High urgency (>0.5) or high value (>50)?
    │   │   └─> Use subscription (cost: $0)
    │   │
    │   └─> Low urgency and low value?
    │       └─> Continue to API routing
    │
    └─> API Selection based on task value:
        │
        ├─> Value 0-30: DeepSeek ($0.245/MTok)
        ├─> Value 30-50: Haiku ($1/MTok)
        ├─> Value 50-75: Sonnet ($12/MTok)
        └─> Value 75-100: Opus ($60/MTok)
```

## Monthly Optimization Cycle

### Week 1 (Days 22-30)
- **Target:** 20-40% quota utilization
- **Strategy:** Normal allocation
- **Focus:** High-quality tasks only

### Week 2 (Days 15-21)
- **Target:** 40-60% quota utilization
- **Strategy:** Normal allocation
- **Focus:** Establish baseline patterns

### Week 3 (Days 8-14)
- **Target:** 60-80% quota utilization
- **Strategy:** Begin acceleration if under-utilized
- **Focus:** Route medium-value tasks to subscription

### Week 4 (Days 0-7)
- **Target:** 80-100% quota utilization
- **Strategy:** Aggressive acceleration
- **Focus:** Use any remaining quota productively

## Cost Savings Examples

### Scenario 1: Medium Volume (10M tokens/month)

**Without Optimizer:**
- All tasks via Sonnet API: $120/month

**With Optimizer:**
- Claude Pro subscription: $20/month
- DeepSeek overflow: $2.45/month
- **Total: $22.45/month**
- **Savings: $97.55 (81%)**

### Scenario 2: High Volume (100M tokens/month)

**Without Optimizer:**
- All tasks via Sonnet API: $1,200/month

**With Optimizer:**
- Subscriptions: $40/month
- DeepSeek primary: $24.50/month
- Haiku overflow: $10/month
- **Total: $74.50/month**
- **Savings: $1,125.50 (94%)**

### Scenario 3: Mixed Quality Needs (30M tokens/month)

**Without Optimizer:**
- Mixed Sonnet/Opus: $500/month

**With Optimizer:**
- Subscriptions: $40/month
- Tiered API routing: $45/month
- **Total: $85/month**
- **Savings: $415 (83%)**

## Monitoring Alerts

The system generates alerts for:

1. **Quota Depletion** (>80% used with >30% time remaining)
   - Action: Switch to API mode

2. **Under-Utilization** (<40% used with <5 days remaining)
   - Action: Accelerate usage, suggest batch tasks

3. **Budget Overrun** (>90% spent with >7 days remaining)
   - Action: Throttle API usage

4. **Quality Degradation** (Success rate <85%)
   - Action: Review model selection, upgrade tiers

## Best Practices

### 1. Calibrate Task Value Scoring

Adjust weights in `optimizer-config.yaml` to match your business:

```yaml
task_scoring:
  weights:
    business_impact: 0.30      # Increase if revenue focus
    time_sensitivity: 0.25     # Increase if deadline critical
    complexity: 0.20
    quality_requirement: 0.15
    visibility: 0.10
```

### 2. Monitor and Adjust

Review monthly reports to identify:
- Which tasks used expensive models unnecessarily
- Which subscriptions were under-utilized
- Optimal task-to-model mappings

### 3. Set Appropriate Billing Days

Ensure `billing_day` in config matches your actual subscription renewal dates for accurate quota tracking.

### 4. Start Conservative

Begin with conservative budgets and adjust upward based on actual usage patterns:

```yaml
budget:
  monthly_total: 50.00        # Start low
  # Increase after 1-2 months of data
```

### 5. Enable Logging

Track all decisions for continuous improvement:

```yaml
logging:
  enabled: true
  log_executions: true
  storage:
    type: "sqlite"
    path: "./optimizer_data.db"
```

## Troubleshooting

### Issue: Tasks Rejected Due to Budget

**Solution:** Increase API variable budget or adjust task value scoring to better reflect business priorities.

### Issue: Subscriptions Under-Utilized

**Solution:** Lower urgency thresholds to accelerate usage earlier in billing cycle:

```yaml
quota_optimization:
  urgency_thresholds:
    medium: 0.20              # Lower from 0.30
```

### Issue: Too Many High-Cost API Calls

**Solution:** Review task value scoring weights or adjust tier thresholds:

```yaml
model_selection:
  tiers:
    balanced:
      value_range: [50, 80]   # Increase upper bound
```

### Issue: Quality Issues with Cheap Models

**Solution:** Increase quality thresholds or adjust success probability calculation:

```yaml
model_selection:
  min_success_probability: 0.80  # Increase from 0.70
```

## Advanced Usage

### Custom Task Scoring

Implement custom scoring logic:

```python
class CustomTaskScorer(TaskValueScorer):
    def score_task(self, task: Task) -> int:
        # Your custom logic
        score = super().score_task(task)

        # Add custom factors
        if task.is_ml_training:
            score += 20

        return min(100, score)

optimizer.value_scorer = CustomTaskScorer()
```

### Monthly Cost Simulation

Forecast costs before deployment:

```python
simulation = optimizer.simulate_monthly_costs(
    expected_tokens=25_000_000,
    task_distribution={
        'low_value': 0.60,
        'medium_value': 0.25,
        'high_value': 0.15,
    }
)

print(f"Projected cost: ${simulation['total_cost']:.2f}")
print(f"Cost per MTok: ${simulation['cost_per_mtok']:.2f}")
```

### Optimization Report

Generate comprehensive reports:

```python
report = optimizer.get_optimization_report()

for sub in report['subscriptions']:
    print(f"{sub['name']}: {sub['utilization_pct']:.1f}% utilized")
    print(f"Savings: ${sub['cost_savings']:.2f}")

    if 'acceleration_suggestions' in sub:
        print("Suggestions:")
        for suggestion in sub['acceleration_suggestions']:
            print(f"  - {suggestion}")
```

## Next Steps

1. **Deploy in pilot mode** for one month
2. **Collect data** on actual usage patterns
3. **Analyze results** and tune configuration
4. **Iterate** on model selection and scoring algorithms
5. **Scale** to full production usage

## Support

For issues or questions:
1. Review the detailed `pricing-analysis.md` document
2. Check the `pricing-spreadsheet.csv` for cost projections
3. Examine the source code in `cost_optimizer.py`
4. Run the demo to understand behavior

## Summary

The Control Panel delivers:
- **60-98% cost savings** vs pure API usage
- **Intelligent task routing** based on value and urgency
- **Automatic quota optimization** to prevent waste
- **Real-time monitoring** and alerting
- **Continuous improvement** through data collection

Start optimizing your LLM costs today!
