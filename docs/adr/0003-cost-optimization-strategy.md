# ADR 0003: Cost Optimization Strategy - Subscription-First Routing

**Date**: 2026-02-07
**Status**: Accepted
**Deciders**: Jed Arden, Claude Sonnet 4.5

---

## Context

AI coding tools have two pricing models:
1. **Subscriptions**: Fixed monthly cost, unlimited* usage (use-or-lose)
   - Claude Code Pro: $20/month
   - GitHub Copilot: $10/month
   - Cursor Pro: $20/month
   - Kimi-K2 subscription: ¥500/month (~$70)

2. **Pay-per-token APIs**: Variable cost based on usage
   - Claude Sonnet via API: $3/million tokens (input), $15/million (output)
   - GPT-4: $10-30/million tokens
   - Qwen: $0.40-2.00/million tokens

**Problem**: Users often have subscriptions with unused capacity while simultaneously paying for API tokens. This results in paying twice for AI usage.

**Opportunity**: Route work strategically to maximize subscription usage before falling back to pay-per-token APIs.

---

## Decision

Implement **subscription-first routing** with intelligent overflow to APIs:

1. **Prioritize subscriptions**: Route tasks to subscription-based workers first
2. **Track usage limits**: Monitor subscription quotas and rate limits
3. **Intelligent overflow**: Route to pay-per-token APIs when subscriptions are saturated or rate-limited
4. **Task-model matching**: Route based on task complexity and model capabilities

---

## Rationale

### Cost Savings Analysis

Research shows **87-94% cost reduction** is achievable with subscription optimization:

**Example: 100 tasks/day**
- **Without optimization**:
  - Mix of subscription + API usage
  - Average cost: $150-200/month

- **With subscription-first routing**:
  - Maximize $20 Claude Code Pro subscription
  - API usage only for overflow
  - Average cost: $20-30/month
  - **Savings: 85-90%**

### Subscription Optimization Patterns

**Pattern 1: Use-or-Lose Maximization**
```
Priority 1: Subscription workers (fixed cost, maximize usage)
Priority 2: Budget API models (Qwen, Haiku)
Priority 3: Premium API models (Opus, GPT-4)
```

**Pattern 2: Task-Complexity Matching**
```
P0 (Critical): Premium models (Opus via API if needed)
P1 (High): Standard models (Sonnet subscription)
P2 (Medium): Budget models (Haiku subscription, Qwen API)
P3-P4 (Low): Free/cheap models (free tier APIs)
```

**Pattern 3: Temporal Optimization**
```
Start of month: Aggressive subscription usage (fresh quota)
Mid-month: Balanced (monitor quota remaining)
End of month: Maximize remaining quota (use-or-lose)
```

---

## Design

### 4-Tier Model Pool

**Tier 1: Premium (Reserve for P0 tasks)**
- Claude Opus (API: $15/million tokens out)
- GPT-4 (API: $30/million tokens)
- Cost: High, use sparingly

**Tier 2: Standard (Primary workhorse)**
- Claude Sonnet (Subscription: $20/month unlimited* or API: $3/$15)
- GPT-4o (API: $2.50/$10)
- Cost: Medium, optimize subscription first

**Tier 3: Budget (High-volume tasks)**
- Claude Haiku (API: $0.25/$1.25)
- Qwen Turbo (API: $0.40/$1.20)
- Kimi-K2 (Subscription: $70/month)
- Cost: Low, good for bulk work

**Tier 4: Free/Minimal (Experimental)**
- Free tier APIs
- Local models (if available)
- Cost: Minimal, use for non-critical tasks

### Routing Algorithm

```python
def route_task(task):
    # Evaluate task complexity
    score = calculate_task_value(task)  # 0-100

    # Route based on score and availability
    if score >= 90:  # P0 - Critical
        return get_available_worker([Tier1, Tier2_subscription, Tier2_api])
    elif score >= 70:  # P1 - High
        return get_available_worker([Tier2_subscription, Tier2_api, Tier1])
    elif score >= 40:  # P2 - Medium
        return get_available_worker([Tier2_subscription, Tier3, Tier2_api])
    else:  # P3-P4 - Low
        return get_available_worker([Tier3, Tier4, Tier2_subscription])

def get_available_worker(tier_priority):
    for tier in tier_priority:
        # Check subscription quota
        if is_subscription(tier) and has_quota(tier):
            return get_subscription_worker(tier)
        # Check API rate limits
        if is_api(tier) and within_rate_limit(tier):
            return get_api_worker(tier)
    # Fallback: queue or wait
    return None
```

### Cost Tracking

Real-time metrics:
- Subscription usage (% of quota used)
- API token consumption (input/output)
- Cost per task
- Daily/weekly/monthly totals
- Projected end-of-month cost
- Savings vs unoptimized routing

---

## Consequences

### Positive
- **Massive cost savings**: 85-95% reduction achievable
- **Maximize subscriptions**: Use-or-lose capacity fully utilized
- **Transparent costs**: Real-time tracking and forecasting
- **Flexible scaling**: Overflow to APIs when needed
- **Risk management**: Never blocked by quota (API fallback)

### Negative
- **Quota monitoring complexity**: Need to track multiple subscription limits
- **Rate limit handling**: API tier fallback adds latency
- **Initial setup**: Users must configure subscription credentials
- **Vendor lock-in risk**: Heavy subscription usage increases switching cost

### Neutral
- **Task scoring accuracy**: Routing quality depends on task value algorithm
- **Subscription changes**: Pricing model changes require strategy updates
- **User behavior**: Some users prefer API-only for predictability

---

## Implementation Plan

### Phase 1: Basic Routing
- [ ] Implement 4-tier model pool
- [ ] Basic subscription-first routing
- [ ] Simple cost tracking (API tokens only)

### Phase 2: Intelligent Routing
- [ ] Task value scoring (0-100)
- [ ] Subscription quota tracking
- [ ] API rate limit handling
- [ ] Dynamic tier selection based on availability

### Phase 3: Advanced Optimization
- [ ] Temporal optimization (start/mid/end of month)
- [ ] Learning-based routing (improve over time)
- [ ] Cost forecasting and alerts
- [ ] Subscription utilization recommendations

### Phase 4: Enterprise Features
- [ ] Multi-user subscription sharing
- [ ] Budget caps and alerts
- [ ] Cost allocation by project/team
- [ ] Detailed audit logs for billing

---

## Metrics

Success criteria:
- **Cost reduction**: ≥80% vs unoptimized baseline
- **Subscription utilization**: ≥90% by end of billing period
- **Task routing accuracy**: ≥85% tasks routed to optimal tier
- **User satisfaction**: Cost transparency and control

Monitoring:
- Daily cost reports
- Subscription quota utilization
- Task-model distribution
- Performance vs cost tradeoff

---

## Alternatives Considered

### Round-Robin Load Balancing
- **Pros**: Simple, equal distribution
- **Cons**: Doesn't optimize cost, wastes subscription capacity
- **Verdict**: Rejected - no cost awareness

### API-Only (No Subscriptions)
- **Pros**: Predictable per-token pricing, no quota tracking
- **Cons**: 5-10x more expensive, no use-or-lose optimization
- **Verdict**: Rejected - cost prohibitive for high usage

### Subscription-Only (No API Fallback)
- **Pros**: Fixed cost ceiling, simple billing
- **Cons**: Hard blocking when quota exhausted, poor scaling
- **Verdict**: Rejected - too inflexible

### Manual User Selection
- **Pros**: User has full control
- **Cons**: Requires constant user decisions, error-prone, no optimization
- **Verdict**: Rejected - defeats purpose of orchestration

---

## References

- [Pricing Analysis](../notes/pricing-analysis.md)
- [Pricing Research Summary](../notes/PRICING_RESEARCH_SUMMARY.md)
- [Model Cost Matrix](../notes/model-cost-matrix.csv)
- [LLM Models Comparison](../notes/llm-models-comparison.md)
- [Task Value Scoring System](../notes/docs/task-value-scoring-system.md)
