# Control Panel: Quick Reference Guide

## Model Selection Cheat Sheet

### By Task Type (Default Choices)

| Task Type | Primary Model | Cost/MTok | When to Use |
|-----------|--------------|-----------|-------------|
| **Architecture & Design** | Claude Opus 4.6 | $15/$75 | Complex system decisions |
| **Code Generation** | DeepSeek Coder V2.5 | $0.14/$0.28 | New features, implementations |
| **Refactoring** | Claude Sonnet 4.5 | $3/$15 | Code modifications, cleanup |
| **Testing** | Claude Haiku 4.5 | $0.80/$4 | Test generation, high volume |
| **Code Review** | Claude Opus 4.6 | $15/$75 | Critical review, security |
| **Documentation** | Claude Haiku 4.5 | $0.80/$4 | Docs, comments, README |
| **Debugging** | Claude Sonnet 4.5 | $3/$15 | Bug fixes, troubleshooting |
| **Optimization** | Claude Opus 4.6 | $15/$75 | Performance tuning |
| **Boilerplate** | Claude Haiku 4.5 | $0.80/$4 | Simple edits, scaffolding |
| **Research** | Claude Opus 4.6 | $15/$75 | Analysis, investigation |

---

## Cost Tiers (Input/Output per MTok)

### Premium ($10-75/MTok)
- **Claude Opus 4.6**: $15/$75 - Best reasoning
- **GPT-4 Turbo**: $10/$30 - Strong alternative

### Mid-Tier ($2.50-15/MTok)
- **Claude Sonnet 4.5**: $3/$15 - Best balanced
- **GPT-4o**: $2.50/$10 - Fast responses

### Budget ($0.14-4/MTok)
- **Claude Haiku 4.5**: $0.80/$4 - Ultra-fast
- **DeepSeek V3**: $0.14/$0.28 - Cost leader
- **DeepSeek Coder V2.5**: $0.14/$0.28 - Code specialist
- **GLM-4.7**: $0.75/$1.50 - z.ai option

### Ultra-Budget ($0.10-1.50/MTok)
- **GPT-3.5 Turbo**: $0.50/$1.50 - Simple tasks
- **Qwen 2.5-Coder (API)**: $0.40/$0.80 - Code tasks
- **Qwen 2.5-Coder (Self-hosted)**: Free - Privacy option

---

## Command Quick Reference

### Claude Code (Primary Orchestrator)

```bash
# Start Claude Code in workspace
cd /home/coder/research/control-panel
claude-code --workspace .

# Spawn parallel agents
Task("Research agent", "Analyze...", "researcher")
Task("Coder agent", "Implement...", "coder")
Task("Tester agent", "Test...", "tester")

# Use skills
Skill("commit", "-m 'Fix bug'")
Skill("review-pr", "123")
```

### Aider (Multi-Model Tool)

```bash
# Quick edit with DeepSeek Coder (cheapest)
aider --model deepseek/deepseek-coder --message "Fix bug in main.py"

# Review with Claude Sonnet (balanced)
aider --model claude-3-5-sonnet-20241022 --message "Review code quality"

# Architecture with Claude Opus (best reasoning)
aider --model claude-opus-4 --message "Design new module architecture"

# Use weak-strong pairing for cost optimization
aider --model deepseek/deepseek-coder --weak-model gpt-3.5-turbo

# List available models
aider --models

# Work with specific files
aider --file src/main.py --file tests/test_main.py
```

### Worker Management

```bash
# Launch workers (must be from /home/coder/claude-config)
cd /home/coder/claude-config
./scripts/spawn-workers.sh --workspace=/control-panel --executor=claude-code-glm-47

# List active workers
tlist

# Attach to worker
tw <session-name>

# Kill worker
tkill <session-name>

# Check worker status
./scripts/worker-status.sh --workspace=/control-panel
```

---

## Cost Optimization Patterns

### 1. Prompt Caching (Claude Models Only)

```python
# Cache system prompt and large context
system = """
You are a control panel agent.
[Include large unchanging codebase context here]
"""

# First call: $3.00/MTok input
# Subsequent calls: $0.03/MTok input (10x cheaper!)
```

**Savings**: 90% on repeated operations with same context

### 2. Model Tiering

```python
# Route by complexity
if complexity == "high":
    model = "claude-opus-4.6"  # $15/MTok
elif complexity == "medium":
    model = "claude-sonnet-4.5"  # $3/MTok
else:  # low
    model = "claude-haiku-4.5"  # $0.80/MTok
```

**Savings**: 5-20x depending on task mix

### 3. Weak-Strong Pairing

```bash
# Step 1: Generate with cheap model
aider --model deepseek/deepseek-coder --message "Implement feature"

# Step 2: Review with strong model
aider --model claude-3-5-sonnet --message "Review and improve"
```

**Savings**: 5-10x vs using premium model for everything

### 4. Batch Operations

```python
# Bad: Individual API calls
for file in files:
    edit_file(file)  # N API calls

# Good: Batch in one context
edit_all_files(files)  # 1 API call with caching
```

**Savings**: Reduces API overhead, increases cache hits

---

## Model Comparison at a Glance

### Best for Reasoning
1. Claude Opus 4.6 (92% HumanEval)
2. GPT-4 Turbo (88% HumanEval)
3. Claude Sonnet 4.5 (88% HumanEval)

### Best for Code Generation
1. DeepSeek Coder V2.5 (84% HumanEval, $0.14/MTok)
2. Qwen 2.5-Coder-32B (83% HumanEval, $0.40/MTok)
3. Claude Sonnet 4.5 (88% HumanEval, $3/MTok)

### Best for Speed
1. Claude Haiku 4.5 (1-2 sec, ultra-fast)
2. GPT-3.5 Turbo (fast)
3. DeepSeek V3 (fast)

### Best for Cost
1. DeepSeek V3/Coder ($0.14/MTok)
2. Qwen 2.5-Coder (Free self-hosted or $0.20-0.60/MTok)
3. GPT-3.5 Turbo ($0.50/MTok)

### Best for Long Context (100K+ tokens)
1. Claude models (200K tokens)
2. Kimi-K2 (200K+ tokens, potentially 2M)
3. DeepSeek V3 (128K tokens)
4. GPT-4 Turbo (128K tokens)

---

## Typical Task Costs

### Simple Edit (1K input + 500 output)
- **Claude Opus**: $22.50 per 1000 edits
- **Claude Sonnet**: $4.50 per 1000 edits
- **Claude Haiku**: $1.20 per 1000 edits
- **DeepSeek Coder**: $0.21 per 1000 edits
- **GPT-3.5**: $0.75 per 1000 edits

### Code Generation (2K input + 2K output)
- **Claude Opus**: $180 per 1000 tasks
- **Claude Sonnet**: $36 per 1000 tasks
- **Claude Haiku**: $9.60 per 1000 tasks
- **DeepSeek Coder**: $0.84 per 1000 tasks
- **GPT-4o**: $20 per 1000 tasks

### Large Refactor (10K input + 5K output)
- **Claude Opus**: $525 per 1000 tasks
- **Claude Sonnet**: $105 per 1000 tasks
- **Claude Haiku**: $28 per 1000 tasks
- **DeepSeek Coder**: $3.50 per 1000 tasks
- **GPT-4 Turbo**: $250 per 1000 tasks

---

## Budget Planning

### Daily Cost by Usage Level

| Usage Level | Tasks/Day | Task Mix | Estimated Cost |
|-------------|-----------|----------|----------------|
| **Low** | 100 | 10% complex, 60% medium, 30% simple | $3/day |
| **Medium** | 500 | 5% complex, 70% medium, 25% simple | $10/day |
| **High** | 2000 | 5% complex, 65% medium, 30% simple | $35/day |

### Monthly Budget Estimates
- **Low usage**: ~$90/month
- **Medium usage**: ~$300/month
- **High usage**: ~$1,050/month

---

## Model APIs and Setup

### Anthropic (Claude Models)

```bash
export ANTHROPIC_API_KEY="sk-ant-..."

# Test connection
curl https://api.anthropic.com/v1/messages \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "content-type: application/json" \
  -d '{"model": "claude-3-5-sonnet-20241022", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI (GPT Models)

```bash
export OPENAI_API_KEY="sk-..."

# Test connection
curl https://api.openai.com/v1/chat/completions \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4-turbo", "messages": [{"role": "user", "content": "Hello"}]}'
```

### DeepSeek

```bash
export DEEPSEEK_API_KEY="..."

# Via OpenAI-compatible endpoint
curl https://api.deepseek.com/v1/chat/completions \
  -H "Authorization: Bearer $DEEPSEEK_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model": "deepseek-coder", "messages": [{"role": "user", "content": "Hello"}]}'
```

### Qwen (Self-Hosted)

```bash
# Deploy with vLLM
docker run -d --gpus all \
  -p 8000:8000 \
  vllm/vllm-openai:latest \
  --model Qwen/Qwen2.5-Coder-32B-Instruct

# Test local endpoint
curl http://localhost:8000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "Qwen2.5-Coder-32B-Instruct", "messages": [{"role": "user", "content": "Hello"}]}'
```

### GLM-4.7 (via z.ai)

```bash
export ZAI_API_KEY="..."

# Via z.ai proxy (available in apexalgo-iad cluster)
# Proxy: http://zai-proxy.mcp.svc.cluster.local:8080
```

---

## Decision Tree

```
Start
  │
  ├─ Task Type?
  │   │
  │   ├─ Architecture/Design → High Complexity → Claude Opus 4.6
  │   │
  │   ├─ Code Generation
  │   │   ├─ Budget Mode? → DeepSeek Coder V2.5
  │   │   └─ Balanced → Claude Sonnet 4.5
  │   │
  │   ├─ Refactoring
  │   │   ├─ Complex? → Claude Sonnet 4.5
  │   │   └─ Simple? → DeepSeek V3
  │   │
  │   ├─ Testing → Claude Haiku 4.5 (high volume)
  │   │
  │   ├─ Code Review
  │   │   ├─ Critical? → Claude Opus 4.6
  │   │   └─ Routine? → Claude Sonnet 4.5
  │   │
  │   ├─ Documentation → Claude Haiku 4.5 or GPT-3.5
  │   │
  │   ├─ Debugging → Claude Sonnet 4.5 or DeepSeek Coder
  │   │
  │   ├─ Optimization → Claude Opus 4.6 (requires deep reasoning)
  │   │
  │   └─ Boilerplate → Claude Haiku 4.5 (ultra-fast)
  │
  └─ Budget Constraint?
      ├─ Unlimited → Use best model for task type
      ├─ High ($100/day) → Mix premium + budget models
      ├─ Medium ($30/day) → Favor Sonnet + DeepSeek + Haiku
      ├─ Low ($10/day) → DeepSeek + Haiku + GPT-3.5
      └─ Very Low ($3/day) → DeepSeek + Haiku only
```

---

## Common Pitfalls to Avoid

### 1. Using Premium Models for Simple Tasks
**Mistake**: Using Claude Opus for documentation
**Fix**: Route simple tasks to Haiku or GPT-3.5
**Savings**: 20-50x

### 2. Not Using Prompt Caching
**Mistake**: Sending full context on every request
**Fix**: Cache system prompts and codebase context
**Savings**: 10x on repeated operations

### 3. Single Model for Everything
**Mistake**: Only using Claude Sonnet
**Fix**: Use model pool with task routing
**Savings**: 5-10x overall

### 4. Ignoring Rate Limits
**Mistake**: No retry logic when hitting limits
**Fix**: Implement exponential backoff and fallback models
**Impact**: Prevents workflow interruptions

### 5. No Cost Monitoring
**Mistake**: Running blind without tracking spend
**Fix**: Log all API calls with costs, set budget alerts
**Impact**: Prevents cost overruns

---

## Performance Benchmarks

### Response Time (Average)
- **Claude Haiku 4.5**: 1-2 seconds (ultra-fast)
- **GPT-3.5 Turbo**: 2-3 seconds (very fast)
- **DeepSeek V3/Coder**: 2-4 seconds (fast)
- **Claude Sonnet 4.5**: 3-5 seconds (fast)
- **GPT-4o**: 4-6 seconds (medium-fast)
- **GPT-4 Turbo**: 5-8 seconds (medium)
- **Claude Opus 4.6**: 8-15 seconds (slower, deep reasoning)

### Context Processing Speed
- **Short context (<4K tokens)**: All models fast
- **Medium context (4K-32K)**: Modern models handle well
- **Long context (32K-128K)**: Claude/DeepSeek/GPT-4 optimized
- **Very long (128K-200K)**: Claude models excel

---

## Integration Examples

### Claude Code + Aider

```bash
# In Claude Code session, invoke Aider for specific model
invoke_tool("Bash", {
  "command": "aider --model deepseek/deepseek-coder --message 'Implement feature X' --yes"
})
```

### Parallel Agent Execution

```python
# Claude Code native
Task("Agent 1", "Generate code with DeepSeek", "coder1", executor="aider", model="deepseek-coder")
Task("Agent 2", "Review with Sonnet", "reviewer1", executor="aider", model="claude-sonnet")
Task("Agent 3", "Test with Haiku", "tester1", executor="aider", model="claude-haiku")
```

### Cost-Aware Routing

```python
# Check budget before task assignment
if daily_spend < 10:
    model = select_model(task_type, complexity="high")
elif daily_spend < 30:
    model = select_model(task_type, complexity="medium")
else:  # Approaching limit
    model = select_model(task_type, budget_mode=True)
```

---

## Emergency Fallbacks

### Primary Model Down
1. **Claude Opus down** → Use GPT-4 Turbo
2. **Claude Sonnet down** → Use GPT-4o or DeepSeek Coder
3. **Claude Haiku down** → Use GPT-3.5 or DeepSeek V3

### Rate Limit Hit
1. Wait with exponential backoff (2^n seconds)
2. Switch to alternative model in same tier
3. If critical, escalate to human

### Budget Exhausted
1. Switch all tasks to budget models (DeepSeek, Haiku, GPT-3.5)
2. Defer non-critical tasks
3. Use self-hosted Qwen if available
4. Request budget increase

---

## Success Metrics

### Track These KPIs
- **Cost per task** (target: <$0.05 average)
- **Success rate** (target: >95%)
- **Cache hit rate** (target: >60%)
- **Average response time** (target: <5 seconds)
- **Daily budget adherence** (target: 100%)

### Weekly Review Questions
1. Are we using the right model for each task type?
2. Is prompt caching optimized?
3. Are we hitting rate limits?
4. Is cost trending up or down?
5. Are budget models meeting quality standards?

---

## Updated: 2026-02-07
