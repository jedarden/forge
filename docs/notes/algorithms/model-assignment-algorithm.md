# Model Assignment Algorithm

## Algorithm Overview

The model assignment algorithm combines task value scoring, token estimation, subscription quota tracking, and model capabilities to select the optimal LLM for each task.

## Input Data Structures

### Task Metadata
```python
@dataclass
class Task:
    id: str
    title: str
    description: str
    priority: str  # P0, P1, P2, P3, P4
    labels: List[str]
    estimated_files: int
    estimated_loc: int
    dependencies: List[str]
    workspace_path: str
    deadline: Optional[datetime]
    assigned_model: Optional[str] = None
    value_score: Optional[float] = None
```

### Model Metadata
```python
@dataclass
class Model:
    name: str
    tier: str  # premium, mid-premium, mid-range, budget
    cost_per_mtok_input: float
    cost_per_mtok_output: float
    context_window: int
    capabilities: Dict[str, float]  # domain -> rating
    benchmarks: Dict[str, float]  # test -> score

@dataclass
class Subscription:
    model_family: str  # claude, openai, deepseek, etc.
    monthly_cost: float
    token_limit_daily: Optional[int]
    token_limit_monthly: Optional[int]
    tokens_used_today: int
    tokens_used_month: int
    reset_date: datetime
```

## Algorithm Pseudocode

### Phase 1: Task Value Scoring

```python
def calculate_task_value(task: Task) -> float:
    """
    Calculate task value score (0-100)
    """
    # 1. Base priority weight
    priority_map = {
        'P0': 40, 'P1': 30, 'P2': 20, 'P3': 10, 'P4': 5
    }
    base_score = priority_map.get(task.priority, 20)

    # 2. Complexity multiplier
    complexity = estimate_complexity(task)
    complexity_multiplier = {
        'simple': 0.5,
        'moderate': 1.0,
        'complex': 1.5,
        'highly_complex': 2.0
    }[complexity]

    # 3. Time sensitivity bonus
    time_bonus = 0
    if task.deadline:
        hours_until_deadline = (task.deadline - now()).total_seconds() / 3600
        if hours_until_deadline <= 4:
            time_bonus = 15  # Immediate
        elif hours_until_deadline <= 24:
            time_bonus = 12  # Urgent
        elif hours_until_deadline <= 168:
            time_bonus = 8   # This week
        elif hours_until_deadline <= 720:
            time_bonus = 5   # This month

    # Check if blocking other tasks
    if is_blocking_tasks(task):
        time_bonus = max(time_bonus, 12)

    # 4. Domain modifier
    domain = classify_domain(task)
    domain_modifiers = {
        'infrastructure': 1.2,
        'backend': 1.1,
        'ml': 1.1,
        'frontend': 1.0,
        'testing': 0.9,
        'documentation': 0.8
    }
    domain_mod = domain_modifiers.get(domain, 1.0)

    # 5. Risk adjustment
    risk_level = assess_risk(task)
    risk_modifiers = {
        'production': 1.3,
        'staging': 1.1,
        'development': 1.0,
        'experimental': 0.9,
        'sandbox': 0.7
    }
    risk_mod = risk_modifiers.get(risk_level, 1.0)

    # Calculate final score
    score = (base_score * complexity_multiplier + time_bonus) * domain_mod * risk_mod

    return clamp(score, 0, 100)


def estimate_complexity(task: Task) -> str:
    """
    Estimate task complexity based on multiple signals
    """
    # File count estimation
    file_count = task.estimated_files
    if file_count == 0:
        # Estimate from description
        file_count = estimate_files_from_description(task.description)

    # LOC estimation
    loc = task.estimated_loc
    if loc == 0:
        loc = estimate_loc_from_description(task.description)

    # Dependency depth
    dep_depth = len(task.dependencies)

    # Keyword analysis
    complex_keywords = ['refactor', 'migrate', 'redesign', 'architecture', 'breaking']
    has_complex_keywords = any(kw in task.description.lower() for kw in complex_keywords)

    # Scoring
    if file_count >= 10 or loc > 1500 or has_complex_keywords:
        return 'highly_complex'
    elif file_count >= 6 or loc > 500:
        return 'complex'
    elif file_count >= 3 or loc > 100:
        return 'moderate'
    else:
        return 'simple'


def classify_domain(task: Task) -> str:
    """
    Classify task domain from labels and content
    """
    label_set = set(label.lower() for label in task.labels)

    # Infrastructure keywords
    if label_set & {'kubernetes', 'k8s', 'infra', 'devops', 'deployment', 'ci/cd'}:
        return 'infrastructure'

    # Backend keywords
    if label_set & {'api', 'backend', 'database', 'server', 'auth'}:
        return 'backend'

    # ML keywords
    if label_set & {'ml', 'machine-learning', 'data', 'model', 'training'}:
        return 'ml'

    # Frontend keywords
    if label_set & {'frontend', 'ui', 'react', 'vue', 'css', 'html'}:
        return 'frontend'

    # Testing keywords
    if label_set & {'test', 'testing', 'qa', 'e2e', 'unit-test'}:
        return 'testing'

    # Documentation keywords
    if label_set & {'docs', 'documentation', 'readme', 'guide'}:
        return 'documentation'

    # Fallback: analyze workspace path
    if 'infra' in task.workspace_path or 'k8s' in task.workspace_path:
        return 'infrastructure'
    elif 'api' in task.workspace_path or 'server' in task.workspace_path:
        return 'backend'
    elif 'frontend' in task.workspace_path or 'ui' in task.workspace_path:
        return 'frontend'

    return 'backend'  # Default


def assess_risk(task: Task) -> str:
    """
    Assess risk level of task
    """
    desc_lower = task.description.lower()
    label_set = set(label.lower() for label in task.labels)

    # Production indicators
    if 'production' in label_set or 'hotfix' in label_set or 'security' in label_set:
        return 'production'
    if 'prod' in task.workspace_path or 'main' in task.workspace_path:
        return 'production'
    if any(kw in desc_lower for kw in ['production', 'breaking change', 'migration']):
        return 'production'

    # Staging indicators
    if 'staging' in label_set or 'integration' in label_set:
        return 'staging'

    # Experimental indicators
    if 'experimental' in label_set or 'prototype' in label_set:
        return 'experimental'
    if 'experiment' in desc_lower or 'prototype' in desc_lower:
        return 'experimental'

    # Sandbox indicators
    if 'sandbox' in label_set or 'learning' in label_set:
        return 'sandbox'

    return 'development'  # Default
```

### Phase 2: Token Estimation

```python
def estimate_token_usage(task: Task) -> Tuple[int, int]:
    """
    Estimate input and output tokens for task completion
    Returns: (input_tokens, output_tokens)
    """
    complexity = estimate_complexity(task)

    # Base estimates by complexity
    token_estimates = {
        'simple': (7500, 1250),          # 5-15K input, 500-2K output
        'moderate': (27500, 3500),        # 15-40K input, 2-5K output
        'complex': (70000, 10000),        # 40-100K input, 5-15K output
        'highly_complex': (150000, 27500) # 100-200K input, 15-40K output
    }

    input_est, output_est = token_estimates[complexity]

    # Adjust for domain
    domain = classify_domain(task)
    if domain == 'infrastructure':
        # K8s YAML files are verbose
        input_est *= 1.2
    elif domain == 'documentation':
        # Less code, more text
        output_est *= 1.3
    elif domain == 'testing':
        # Test files add significant output
        output_est *= 1.4

    # Adjust for workspace size
    if task.workspace_path:
        workspace_files = count_workspace_files(task.workspace_path)
        if workspace_files > 100:
            input_est *= 1.3  # Need more context
        elif workspace_files > 50:
            input_est *= 1.15

    return (int(input_est), int(output_est))
```

### Phase 3: Model Selection

```python
def select_model(
    task: Task,
    subscriptions: List[Subscription],
    models: List[Model],
    quota_strategy: str = 'maximize_value'
) -> Model:
    """
    Select optimal model for task based on value, cost, and quotas
    """
    # Calculate task value
    value_score = calculate_task_value(task)
    task.value_score = value_score

    # Estimate token usage
    input_tokens, output_tokens = estimate_token_usage(task)

    # Calculate value density (value per expected token)
    total_tokens = input_tokens + output_tokens
    value_density = value_score / (total_tokens / 1000)  # Value per K tokens

    # Get domain for capability matching
    domain = classify_domain(task)

    # Check subscription quotas
    available_subscriptions = []
    for sub in subscriptions:
        remaining_daily = (sub.token_limit_daily or float('inf')) - sub.tokens_used_today
        remaining_monthly = (sub.token_limit_monthly or float('inf')) - sub.tokens_used_month

        if total_tokens <= remaining_daily and total_tokens <= remaining_monthly:
            available_subscriptions.append(sub)

    # Model selection logic based on value score
    if value_score >= 90:
        # Critical high-value work
        candidates = filter_models(models, tier=['premium'])

        # Prefer Opus, fallback to GPT-4
        preferred_order = ['opus-4.6', 'gpt-4', 'sonnet-4.5']

    elif value_score >= 75:
        # High-value work
        candidates = filter_models(models, tier=['premium', 'mid-premium'])

        # Check if we have subscription quota for Sonnet
        if has_quota(available_subscriptions, 'claude'):
            preferred_order = ['sonnet-4.5', 'opus-4.6', 'gpt-4-turbo']
        else:
            preferred_order = ['gpt-4-turbo', 'sonnet-4.5', 'deepseek-v3']

    elif value_score >= 60:
        # Medium-high value
        candidates = filter_models(models, tier=['mid-premium', 'mid-range'])

        # Prefer cost-efficient mid-range models
        preferred_order = ['deepseek-v3', 'qwen2.5-coder', 'sonnet-4.5']

    elif value_score >= 40:
        # Standard value
        candidates = filter_models(models, tier=['mid-range', 'budget'])

        # Use cheapest capable models
        preferred_order = ['glm-4.7', 'qwen2.5-coder', 'deepseek-v3']

    else:
        # Low value - defer or use cheapest
        if quota_strategy == 'maximize_value':
            # Don't spend API tokens on low-value work
            return None  # Signal to defer task
        else:
            candidates = filter_models(models, tier=['budget'])
            preferred_order = ['glm-4.7', 'qwen2.5-coder']

    # Domain-specific overrides
    if domain == 'infrastructure' and value_score >= 60:
        # Claude excels at K8s YAML
        preferred_order = [m for m in preferred_order if 'sonnet' in m or 'opus' in m] + \
                         [m for m in preferred_order if 'sonnet' not in m and 'opus' not in m]

    # Select best available model from preferred order
    for model_name in preferred_order:
        model = find_model(models, model_name)
        if model and model in candidates:
            # Check if we can afford it
            if can_use_model(model, subscriptions, input_tokens, output_tokens):
                return model

    # Fallback to cheapest available
    candidates.sort(key=lambda m: m.cost_per_mtok_input + m.cost_per_mtok_output)
    return candidates[0] if candidates else None


def can_use_model(
    model: Model,
    subscriptions: List[Subscription],
    input_tokens: int,
    output_tokens: int
) -> bool:
    """
    Check if we can use this model based on quota and budget
    """
    # Check subscription quota
    for sub in subscriptions:
        if model.name.startswith(sub.model_family):
            total_needed = input_tokens + output_tokens
            remaining_daily = (sub.token_limit_daily or float('inf')) - sub.tokens_used_today
            remaining_monthly = (sub.token_limit_monthly or float('inf')) - sub.tokens_used_month

            if total_needed <= remaining_daily and total_needed <= remaining_monthly:
                return True

    # Check API budget
    cost = calculate_cost(model, input_tokens, output_tokens)
    remaining_budget = get_remaining_monthly_budget()

    return cost <= remaining_budget


def calculate_cost(model: Model, input_tokens: int, output_tokens: int) -> float:
    """
    Calculate task cost for given model
    """
    input_cost = (input_tokens / 1_000_000) * model.cost_per_mtok_input
    output_cost = (output_tokens / 1_000_000) * model.cost_per_mtok_output
    return input_cost + output_cost
```

### Phase 4: Quota Optimization

```python
def optimize_task_assignments(
    tasks: List[Task],
    subscriptions: List[Subscription],
    models: List[Model],
    strategy: str = 'maximize_subscription_usage'
) -> Dict[str, Model]:
    """
    Optimize task assignments across a batch of tasks
    """
    # Sort tasks by value density (value per token)
    scored_tasks = []
    for task in tasks:
        value = calculate_task_value(task)
        input_tok, output_tok = estimate_token_usage(task)
        total_tok = input_tok + output_tok
        density = value / (total_tok / 1000)
        scored_tasks.append((task, value, density, total_tok))

    assignments = {}

    if strategy == 'maximize_subscription_usage':
        # Priority 1: Fill subscription quotas with highest-value tasks that fit
        # Priority 2: Use API for remaining high-value tasks
        # Priority 3: Defer low-value tasks

        # Sort by value (descending)
        scored_tasks.sort(key=lambda x: x[1], reverse=True)

        for task, value, density, tokens in scored_tasks:
            model = select_model(task, subscriptions, models, quota_strategy='maximize_value')

            if model:
                assignments[task.id] = model
                # Update subscription usage
                update_quota_tracking(subscriptions, model, tokens)

    elif strategy == 'minimize_cost':
        # Priority 1: Use subscriptions for everything that fits
        # Priority 2: Use cheapest API models for overflow
        # Priority 3: Defer if over budget

        # Sort by token count (ascending) to pack subscriptions efficiently
        scored_tasks.sort(key=lambda x: x[3])

        for task, value, density, tokens in scored_tasks:
            # Try subscription models first
            model = try_subscription_models(task, subscriptions, models)

            if not model and value >= 40:
                # Overflow to cheapest API model
                model = get_cheapest_capable_model(task, models)

            if model:
                assignments[task.id] = model
                update_quota_tracking(subscriptions, model, tokens)

    elif strategy == 'maximize_value':
        # Priority 1: High-value tasks get best models
        # Priority 2: Medium-value tasks get good models if quota available
        # Priority 3: Low-value tasks deferred or batched

        # Sort by value (descending)
        scored_tasks.sort(key=lambda x: x[1], reverse=True)

        for task, value, density, tokens in scored_tasks:
            if value >= 75:
                # Always assign high-value tasks
                model = select_model(task, subscriptions, models, 'maximize_value')
                if model:
                    assignments[task.id] = model
                    update_quota_tracking(subscriptions, model, tokens)
            elif value >= 40:
                # Assign if we have quota/budget
                model = select_model(task, subscriptions, models, 'maximize_value')
                if model and can_use_model(model, subscriptions, *estimate_token_usage(task)):
                    assignments[task.id] = model
                    update_quota_tracking(subscriptions, model, tokens)
            # else: defer low-value tasks

    return assignments
```

## Decision Flow Chart

```
┌─────────────────────────────────────┐
│  Incoming Task                      │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│  Calculate Task Value Score         │
│  - Priority weight                  │
│  - Complexity multiplier            │
│  - Time sensitivity bonus           │
│  - Domain modifier                  │
│  - Risk adjustment                  │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│  Estimate Token Usage               │
│  - Base estimate from complexity    │
│  - Domain adjustment                │
│  - Workspace size adjustment        │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│  Calculate Value Density            │
│  value_density = score / (tokens/1K)│
└──────────────┬──────────────────────┘
               │
               ▼
        ┌──────┴──────┐
        │ Value >= 90? │
        └──────┬──────┘
         YES   │   NO
        ┌──────┴──────┐
        │             │
        ▼             ▼
┌──────────────┐  ┌───────────────┐
│ Tier:        │  │ Value >= 75?  │
│ Premium      │  └───────┬───────┘
│ (Opus/GPT-4) │    YES   │   NO
└──────────────┘   ┌──────┴──────┐
                   │             │
                   ▼             ▼
            ┌─────────────┐  ┌──────────────┐
            │ Tier:       │  │ Value >= 60? │
            │ Mid-Premium │  └──────┬───────┘
            │ (Sonnet)    │   YES   │   NO
            └─────────────┘  ┌──────┴──────┐
                             │             │
                             ▼             ▼
                      ┌────────────┐  ┌──────────────┐
                      │ Tier:      │  │ Value >= 40? │
                      │ Mid-Range  │  └──────┬───────┘
                      │ (DeepSeek) │   YES   │   NO
                      └────────────┘  ┌──────┴──────┐
                                      │             │
                                      ▼             ▼
                               ┌────────────┐  ┌──────────┐
                               │ Tier:      │  │ Defer or │
                               │ Budget     │  │ Batch    │
                               │ (GLM-4.7)  │  └──────────┘
                               └────────────┘
                                      │
                                      ▼
                           ┌────────────────────────┐
                           │ Check Subscription     │
                           │ Quota Availability     │
                           └──────────┬─────────────┘
                                      │
                              ┌───────┴────────┐
                              │ Quota Available?│
                              └───────┬────────┘
                               YES    │    NO
                              ┌───────┴────────┐
                              │                │
                              ▼                ▼
                     ┌──────────────┐   ┌────────────┐
                     │ Use          │   │ Check API  │
                     │ Subscription │   │ Budget     │
                     └──────────────┘   └─────┬──────┘
                                              │
                                      ┌───────┴────────┐
                                      │ Budget OK?     │
                                      └───────┬────────┘
                                       YES    │    NO
                                      ┌───────┴────────┐
                                      │                │
                                      ▼                ▼
                               ┌────────────┐   ┌──────────┐
                               │ Use API    │   │ Downgrade│
                               │ with model │   │ or Defer │
                               └────────────┘   └──────────┘
                                      │
                                      ▼
                           ┌────────────────────────┐
                           │ Assign Model to Task   │
                           │ Update Quota Tracking  │
                           │ Log Assignment Decision│
                           └────────────────────────┘
```

## Implementation Example

```python
# Example usage
task = Task(
    id='po-123',
    title='Fix authentication vulnerability',
    description='Critical security fix in JWT validation',
    priority='P0',
    labels=['security', 'backend', 'hotfix'],
    estimated_files=3,
    estimated_loc=150,
    dependencies=[],
    workspace_path='/home/coder/api-server',
    deadline=datetime.now() + timedelta(hours=2)
)

subscriptions = [
    Subscription(
        model_family='claude',
        monthly_cost=20.0,
        token_limit_daily=None,
        token_limit_monthly=5_000_000,  # Estimated
        tokens_used_today=0,
        tokens_used_month=1_200_000,
        reset_date=datetime(2026, 3, 1)
    )
]

models = load_models_from_config()

# Assign model
selected_model = select_model(task, subscriptions, models)
print(f"Selected {selected_model.name} for task {task.id}")
print(f"Task value: {task.value_score:.1f}/100")
```

## Configuration

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

time_sensitivity_bonuses:
  immediate: 15  # < 4 hours
  urgent: 12     # < 24 hours
  week: 8        # < 1 week
  month: 5       # < 1 month
  flexible: 0

quota_strategy: maximize_subscription_usage  # or minimize_cost or maximize_value
```
