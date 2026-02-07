# Task Value Scoring System

## Overview

The task value scoring system assigns a numerical score (0-100) to each task based on multiple factors that represent the task's business value, urgency, and impact. This score is used to optimize model assignment and resource allocation.

## Scoring Components

### 1. Priority Weight (40 points max)

Priority is the primary driver of task value, mapped from P0-P4 labels:

```
P0 (Critical):  40 points - Production outages, security issues, blocking dependencies
P1 (High):      30 points - Important features, significant bugs, time-sensitive work
P2 (Medium):    20 points - Standard features, minor bugs, routine improvements
P3 (Low):       10 points - Nice-to-have features, technical debt, optimizations
P4 (Backlog):    5 points - Future work, experiments, research tasks
```

**Rationale**: Priority directly reflects business impact. A 2:1 ratio between adjacent priority levels provides clear differentiation while allowing other factors to influence the final score.

### 2. Complexity Multiplier (0.5x - 2.0x)

Complexity affects the absolute value delivered. More complex tasks that are executed well provide greater value:

```
Simple (1-2 files, <100 LOC):           0.5x - Low complexity, quick wins
Moderate (3-5 files, 100-500 LOC):      1.0x - Standard task complexity
Complex (6-10 files, 500-1500 LOC):     1.5x - Significant implementation effort
Highly Complex (10+ files, >1500 LOC):  2.0x - Architecture-level changes
```

**Estimation Heuristics**:
- Count files mentioned in task description
- Analyze dependency graph depth
- Check if task involves multiple domains (frontend + backend + infra)
- Look for keywords: "refactor", "migrate", "redesign" (high complexity)

### 3. Time Sensitivity Bonus (0-15 points)

Tasks with deadlines or that block other work receive additional value:

```
Immediate (blocking production):        +15 points
Urgent (deadline within 24 hours):      +12 points
Time-sensitive (deadline within week):   +8 points
Scheduled (deadline within month):       +5 points
Flexible (no deadline):                  +0 points
```

**Detection**:
- Parse task description for deadline keywords
- Check dependency graph for tasks blocked by this one
- Look for labels: "urgent", "blocking", "deadline"

### 4. Domain Classification Modifier (0.8x - 1.2x)

Different domains have different value densities and risk profiles:

```
Infrastructure/DevOps:    1.2x - High leverage, affects entire system
Backend/API:              1.1x - Core business logic, high impact
ML/Data:                  1.1x - Complex, specialized knowledge required
Frontend:                 1.0x - User-facing, important but more isolated
Testing/QA:               0.9x - Important but supporting role
Documentation:            0.8x - Necessary but lower immediate impact
```

**Classification**:
- Parse labels and description for domain keywords
- Analyze file paths mentioned (src/api/, infra/, frontend/, etc.)
- Use workspace structure to infer domain

### 5. Risk Level Adjustment (0.7x - 1.3x)

Higher risk tasks require more careful execution and provide more value when done correctly:

```
Production/Critical Systems:  1.3x - Errors have severe consequences
Staging/Integration:          1.1x - Important testing environment
Development/Experimental:     1.0x - Standard development work
Prototypes/Research:          0.9x - Exploratory, lower risk
Sandbox/Learning:             0.7x - No production impact
```

**Risk Indicators**:
- Labels: "production", "hotfix", "security"
- Workspace paths containing: "prod", "main", "master"
- Keywords: "migration", "breaking change", "deployment"

## Value Score Formula

```python
base_score = priority_weight * complexity_multiplier

adjusted_score = (base_score + time_sensitivity_bonus) * domain_modifier * risk_modifier

final_score = clamp(adjusted_score, 0, 100)
```

## Score Interpretation

```
90-100: Critical high-value work - Assign best available model (Opus, GPT-4)
75-89:  High-value work - Assign premium model (Sonnet 4.5, GPT-4 Turbo)
60-74:  Medium-high value - Assign capable model (DeepSeek, Qwen2.5)
40-59:  Standard value - Assign efficient model (GLM-4.7, Sonnet 3.5)
20-39:  Low value - Assign cheapest capable model
0-19:   Minimal value - Defer or batch with other tasks
```

## Example Calculations

### Example 1: Production Hotfix
```
Task: Fix authentication bypass vulnerability in production API
Priority: P0 (40 points)
Complexity: Moderate (1.0x) - 3 files, security patch
Time Sensitivity: Immediate (+15 points)
Domain: Backend (1.1x)
Risk: Production (1.3x)

Score = (40 * 1.0 + 15) * 1.1 * 1.3 = 78.65 → 79/100
Category: High-value work → Assign Sonnet 4.5 or GPT-4 Turbo
```

### Example 2: Feature Development
```
Task: Add user profile customization UI
Priority: P2 (20 points)
Complexity: Moderate (1.0x) - 4 files, standard CRUD
Time Sensitivity: Flexible (+0 points)
Domain: Frontend (1.0x)
Risk: Development (1.0x)

Score = (20 * 1.0 + 0) * 1.0 * 1.0 = 20/100
Category: Low value → Assign GLM-4.7 or cheapest capable model
```

### Example 3: Infrastructure Redesign
```
Task: Migrate Kubernetes cluster to new control plane
Priority: P1 (30 points)
Complexity: Highly Complex (2.0x) - 15+ manifests, multiple namespaces
Time Sensitivity: Scheduled (+5 points)
Domain: Infrastructure (1.2x)
Risk: Production (1.3x)

Score = (30 * 2.0 + 5) * 1.2 * 1.3 = 109.2 → 100/100 (clamped)
Category: Critical high-value → Assign Opus 4.6 or best available
```

### Example 4: Documentation Update
```
Task: Update API documentation for new endpoints
Priority: P3 (10 points)
Complexity: Simple (0.5x) - 1 markdown file
Time Sensitivity: Flexible (+0 points)
Domain: Documentation (0.8x)
Risk: Development (1.0x)

Score = (10 * 0.5 + 0) * 0.8 * 1.0 = 4/100
Category: Minimal value → Defer or batch, use cheapest model
```

## Adaptive Learning

The scoring system should track actual task outcomes and adjust weights over time:

1. **Outcome Tracking**: Record completion time, quality (tests passed, bugs found), and stakeholder satisfaction
2. **Weight Adjustment**: If high-complexity tasks consistently deliver less value than predicted, reduce complexity multiplier
3. **Domain Calibration**: Learn which domains actually provide more value in your specific context
4. **Priority Validation**: Verify that P0 tasks are actually 4x more valuable than P2 tasks

## Implementation Notes

- Store scoring parameters in configuration file for easy tuning
- Log all score calculations with breakdown for transparency
- Allow manual score overrides with justification
- Periodically review score distribution (should be roughly normal, centered around 50)
- A/B test scoring changes by comparing task outcomes before/after adjustments
