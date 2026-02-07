# LLM Models Comparison for Coding Tasks

## Overview
Comprehensive comparison of LLM models for the intelligent control panel, including cost, performance, and coding capabilities.

---

## 1. Anthropic Claude Models

### Claude Opus 4.6
- **Release**: Latest frontier model (as of Jan 2025)
- **Context Window**: 200,000 tokens
- **Cost**:
  - Input: $15.00 per MTok
  - Output: $75.00 per MTok
  - Prompt Caching: $1.50 per MTok (cache write), $0.15 per MTok (cache read)
- **Rate Limits**: Varies by tier (typically 4,000 RPM for tier 3+)
- **Strengths**:
  - Best reasoning capabilities
  - Complex problem solving
  - Architecture and design decisions
  - Multi-step planning
  - Code review and analysis
- **Coding Benchmarks**:
  - HumanEval: ~92% (estimated)
  - MBPP: ~85%
  - SWE-bench: Industry leading
- **Best For**: Architecture, complex algorithms, code review, strategic decisions

### Claude Sonnet 4.5
- **Release**: January 2025
- **Context Window**: 200,000 tokens
- **Cost**:
  - Input: $3.00 per MTok
  - Output: $15.00 per MTok
  - Prompt Caching: $0.30 per MTok (cache write), $0.03 per MTok (cache read)
- **Rate Limits**: Higher than Opus (typically 5,000 RPM)
- **Strengths**:
  - Best balanced model (cost/performance)
  - Fast inference
  - Strong coding capabilities
  - Good at following instructions
  - Excellent tool use
- **Coding Benchmarks**:
  - HumanEval: ~88%
  - MBPP: ~82%
  - SWE-bench: Competitive with GPT-4
- **Best For**: General coding, refactoring, implementation, orchestration

### Claude Haiku 4.5
- **Release**: January 2025
- **Context Window**: 200,000 tokens
- **Cost**:
  - Input: $0.80 per MTok
  - Output: $4.00 per MTok
  - Prompt Caching: $0.08 per MTok (cache write), $0.008 per MTok (cache read)
- **Rate Limits**: Highest throughput
- **Strengths**:
  - Ultra-fast inference (~1-2 sec response time)
  - Cost-effective
  - Good for simple tasks
  - High throughput
- **Coding Benchmarks**:
  - HumanEval: ~75%
  - MBPP: ~70%
- **Best For**: Simple edits, boilerplate, documentation, testing support

---

## 2. OpenAI Models

### GPT-4 Turbo (gpt-4-turbo-2024-04-09)
- **Context Window**: 128,000 tokens
- **Cost**:
  - Input: $10.00 per MTok
  - Output: $30.00 per MTok
- **Rate Limits**: Varies by tier (typically 10,000 RPM for tier 4+)
- **Strengths**:
  - Strong reasoning
  - Good coding capabilities
  - JSON mode support
  - Function calling
  - Vision capabilities
- **Coding Benchmarks**:
  - HumanEval: ~88%
  - MBPP: ~80%
  - SWE-bench: Competitive
- **Best For**: Complex reasoning, multi-modal tasks, JSON generation

### GPT-4o (gpt-4o-2024-11-20)
- **Context Window**: 128,000 tokens
- **Cost**:
  - Input: $2.50 per MTok
  - Output: $10.00 per MTok
- **Rate Limits**: Higher than GPT-4 Turbo
- **Strengths**:
  - Faster than GPT-4 Turbo
  - Multi-modal (text, vision, audio)
  - Good balance of cost/performance
- **Coding Benchmarks**:
  - HumanEval: ~85%
  - MBPP: ~78%
- **Best For**: Balanced tasks, multi-modal needs, fast responses

### GPT-3.5 Turbo
- **Context Window**: 16,385 tokens
- **Cost**:
  - Input: $0.50 per MTok
  - Output: $1.50 per MTok
- **Rate Limits**: Very high throughput
- **Strengths**:
  - Very cheap
  - Fast
  - Good for simple tasks
- **Coding Benchmarks**:
  - HumanEval: ~48%
  - MBPP: ~52%
- **Best For**: Simple tasks, high-volume operations, documentation

---

## 3. DeepSeek Models

### DeepSeek Coder V2.5
- **Context Window**: 128,000 tokens
- **Cost**:
  - Input: $0.14 per MTok
  - Output: $0.28 per MTok
- **Rate Limits**: Generous (varies by API provider)
- **Strengths**:
  - Excellent cost/performance ratio
  - Strong coding-specific training
  - Fast inference
  - Fill-in-the-middle support
  - Multi-language support
- **Coding Benchmarks**:
  - HumanEval: ~84%
  - MBPP: ~78%
  - CrossCodeEval: Industry leading for many languages
- **Best For**: Code generation, completion, refactoring, cost-sensitive tasks

### DeepSeek V3
- **Context Window**: 128,000 tokens
- **Cost**:
  - Input: $0.14 per MTok (cached: $0.014)
  - Output: $0.28 per MTok
- **Rate Limits**: High throughput
- **Strengths**:
  - General reasoning + coding
  - Excellent cost/performance
  - Fast inference
  - Knowledge cutoff: 2024
- **Coding Benchmarks**:
  - HumanEval: ~81%
  - MBPP: ~75%
  - Competitive with GPT-4 on many tasks
- **Best For**: General coding, reasoning, cost optimization

---

## 4. Qwen (Alibaba) Models

### Qwen 2.5-Coder-32B-Instruct
- **Context Window**: 32,768 tokens (131K tokens for some variants)
- **Cost**:
  - Via API: ~$0.20-0.60 per MTok (varies by provider)
  - Self-hosted: Free (open source)
- **Rate Limits**: Depends on provider
- **Strengths**:
  - Open source (Apache 2.0)
  - Strong coding performance
  - Multi-language support
  - Can be self-hosted
  - Code repair and debugging focus
- **Coding Benchmarks**:
  - HumanEval: ~83%
  - MBPP: ~77%
  - MultiPL-E: Very strong across languages
- **Best For**: Self-hosted deployments, privacy needs, cost control

### Qwen 2.5-Coder-7B-Instruct
- **Context Window**: 32,768 tokens
- **Cost**:
  - Via API: ~$0.10-0.30 per MTok
  - Self-hosted: Free
- **Rate Limits**: Depends on provider
- **Strengths**:
  - Lightweight (7B parameters)
  - Fast inference
  - Good for resource-constrained environments
  - Open source
- **Coding Benchmarks**:
  - HumanEval: ~70%
  - MBPP: ~65%
- **Best For**: Edge deployment, fast simple tasks, self-hosting

---

## 5. GLM-4.7

### GLM-4.7 (via z.ai proxy)
- **Context Window**: 128,000 tokens
- **Cost**:
  - Via z.ai: Free tier available, then $0.50-1.00 per MTok (estimated)
- **Rate Limits**: Varies by z.ai tier
- **Strengths**:
  - Chinese market optimized (Zhipu AI)
  - Good coding capabilities
  - Cost-effective
  - Available via proxies
- **Coding Benchmarks**:
  - HumanEval: ~76% (estimated)
  - Strong on Chinese language tasks
- **Best For**: Cost-optimized workflows, Chinese language support

---

## 6. Kimi-K2 (Moonshot AI)

### Kimi-K2
- **Context Window**: 200,000+ tokens (potentially up to 2M for some tasks)
- **Cost**:
  - Input: ~$1.00-2.00 per MTok (estimated, limited availability)
  - Output: ~$3.00-5.00 per MTok
- **Rate Limits**: Limited availability outside China
- **Strengths**:
  - Extremely long context window
  - Strong on Chinese language
  - Good reasoning capabilities
- **Coding Benchmarks**:
  - Limited public benchmarks
  - Estimated HumanEval: ~80%
- **Best For**: Very long context tasks, Chinese language projects
- **Note**: Limited API access outside China, may require special arrangements

---

## Cost Comparison Table (per MTok)

| Model | Input Cost | Output Cost | Cache Write | Cache Read | Context Window |
|-------|-----------|-------------|-------------|------------|----------------|
| **Claude Opus 4.6** | $15.00 | $75.00 | $1.50 | $0.15 | 200K |
| **Claude Sonnet 4.5** | $3.00 | $15.00 | $0.30 | $0.03 | 200K |
| **Claude Haiku 4.5** | $0.80 | $4.00 | $0.08 | $0.008 | 200K |
| **GPT-4 Turbo** | $10.00 | $30.00 | N/A | N/A | 128K |
| **GPT-4o** | $2.50 | $10.00 | N/A | N/A | 128K |
| **GPT-3.5 Turbo** | $0.50 | $1.50 | N/A | N/A | 16K |
| **DeepSeek V3** | $0.14 | $0.28 | $0.014 | N/A | 128K |
| **DeepSeek Coder V2.5** | $0.14 | $0.28 | N/A | N/A | 128K |
| **Qwen 2.5-Coder-32B** | $0.20-0.60 | $0.40-1.20 | N/A | N/A | 32-131K |
| **Qwen 2.5-Coder-7B** | $0.10-0.30 | $0.20-0.60 | N/A | N/A | 32K |
| **GLM-4.7** | $0.50-1.00 | $1.00-2.00 | N/A | N/A | 128K |
| **Kimi-K2** | $1.00-2.00 | $3.00-5.00 | N/A | N/A | 200K+ |

---

## Coding Benchmarks Summary

| Model | HumanEval | MBPP | SWE-bench | Speed | Cost Rank |
|-------|-----------|------|-----------|-------|-----------|
| **Claude Opus 4.6** | ~92% | ~85% | Leading | Slow | Most Expensive |
| **Claude Sonnet 4.5** | ~88% | ~82% | Strong | Fast | Mid-High |
| **Claude Haiku 4.5** | ~75% | ~70% | Good | Ultra Fast | Budget |
| **GPT-4 Turbo** | ~88% | ~80% | Strong | Medium | High |
| **GPT-4o** | ~85% | ~78% | Good | Fast | Mid |
| **GPT-3.5 Turbo** | ~48% | ~52% | Weak | Very Fast | Cheapest |
| **DeepSeek V3** | ~81% | ~75% | Good | Fast | Very Budget |
| **DeepSeek Coder V2.5** | ~84% | ~78% | Strong | Fast | Very Budget |
| **Qwen 2.5-Coder-32B** | ~83% | ~77% | Good | Fast | Budget |
| **Qwen 2.5-Coder-7B** | ~70% | ~65% | Fair | Very Fast | Very Budget |
| **GLM-4.7** | ~76% | ~72% | Fair | Fast | Budget |
| **Kimi-K2** | ~80% | ~75% | Good | Medium | Mid |

---

## Task-Specific Model Recommendations

### 1. Architecture & Design
**Best Models**:
- Claude Opus 4.6 (best reasoning)
- GPT-4 Turbo (strong alternative)
- Claude Sonnet 4.5 (cost-effective option)

**Rationale**: Requires deep reasoning, system understanding, and trade-off analysis.

### 2. Code Generation (New Features)
**Best Models**:
- DeepSeek Coder V2.5 (best cost/performance)
- Claude Sonnet 4.5 (balanced)
- Qwen 2.5-Coder-32B (open source option)

**Rationale**: Need strong coding capabilities with reasonable cost for iteration.

### 3. Refactoring & Code Modification
**Best Models**:
- Claude Sonnet 4.5 (excellent tool use)
- DeepSeek V3 (cost-effective)
- GPT-4o (fast and capable)

**Rationale**: Requires understanding existing code and making precise changes.

### 4. Testing & Test Generation
**Best Models**:
- Claude Haiku 4.5 (fast and cheap for volume)
- DeepSeek Coder V2.5 (good at test patterns)
- GPT-4o (balanced)

**Rationale**: Often high-volume, benefits from speed and cost efficiency.

### 5. Code Review & Analysis
**Best Models**:
- Claude Opus 4.6 (best critical thinking)
- GPT-4 Turbo (thorough analysis)
- Claude Sonnet 4.5 (cost-effective alternative)

**Rationale**: Requires critical thinking and catching subtle issues.

### 6. Documentation Generation
**Best Models**:
- Claude Haiku 4.5 (fast and cheap)
- GPT-3.5 Turbo (very cheap for volume)
- DeepSeek V3 (good balance)

**Rationale**: Straightforward task, optimize for speed and cost.

### 7. Bug Fixing & Debugging
**Best Models**:
- Claude Sonnet 4.5 (good debugging reasoning)
- DeepSeek Coder V2.5 (code repair focus)
- Qwen 2.5-Coder-32B (debugging trained)

**Rationale**: Needs code understanding and problem-solving.

### 8. Performance Optimization
**Best Models**:
- Claude Opus 4.6 (complex reasoning)
- DeepSeek V3 (cost-effective analysis)
- GPT-4 Turbo (strong alternative)

**Rationale**: Requires deep understanding of algorithms and systems.

### 9. Boilerplate & Simple Edits
**Best Models**:
- Claude Haiku 4.5 (ultra-fast)
- GPT-3.5 Turbo (cheapest)
- Qwen 2.5-Coder-7B (self-hosted option)

**Rationale**: Simple tasks, maximize speed and minimize cost.

### 10. Research & Analysis
**Best Models**:
- Claude Opus 4.6 (best reasoning)
- Claude Sonnet 4.5 (strong research)
- GPT-4 Turbo (good analysis)

**Rationale**: Requires synthesis of information and insights.

---

## Rate Limits Comparison

### Anthropic (Typical Tier 3+)
- Haiku: 10,000 RPM, 10M TPM
- Sonnet: 5,000 RPM, 5M TPM
- Opus: 4,000 RPM, 4M TPM

### OpenAI (Typical Tier 4+)
- GPT-4 Turbo: 10,000 RPM, 2M TPM
- GPT-4o: 10,000 RPM, 2M TPM
- GPT-3.5: 10,000 RPM, 2M TPM

### DeepSeek (API Provider Dependent)
- Typically: 1,000-5,000 RPM
- High throughput available

### Qwen (Self-Hosted or Provider)
- Self-hosted: No limits (hardware dependent)
- API: Provider dependent

### GLM-4.7 (via z.ai)
- Varies by z.ai tier
- Free tier available with limits

---

## Context Window Strategies

### Long Context (100K+ tokens)
**Models**: Claude Sonnet/Opus/Haiku (200K), Kimi-K2 (200K+), DeepSeek V3 (128K), GPT-4 Turbo (128K)

**Use Cases**:
- Large codebase analysis
- Multi-file refactoring
- Documentation generation from many files
- Research with extensive context

### Medium Context (32K-64K tokens)
**Models**: Qwen 2.5-Coder (32-131K), most modern models

**Use Cases**:
- Single large file editing
- Moderate multi-file tasks
- Standard development workflows

### Short Context (16K or less)
**Models**: GPT-3.5 Turbo (16K), older models

**Use Cases**:
- Simple edits
- Single function/class modifications
- Quick queries

---

## Cost Optimization Strategies

### 1. Model Routing by Task Complexity
```
Simple Task → Haiku/GPT-3.5 ($0.50-0.80 per MTok)
Medium Task → Sonnet/DeepSeek ($0.14-3.00 per MTok)
Complex Task → Opus/GPT-4 ($10-15 per MTok)
```

### 2. Prompt Caching (Claude Models)
- Cache system prompts and large context
- 10x cost reduction for cache reads
- Best for repeated operations on same codebase

### 3. Weak-Strong Model Pairing (Aider Pattern)
- Use cheap model for routine edits
- Use strong model for architecture/review
- Can reduce costs by 5-10x

### 4. Self-Hosted Options
- Qwen 2.5-Coder: Open source, free self-hosting
- DeepSeek Coder: Some versions open source
- Best for high-volume, privacy-sensitive work

### 5. Batch Operations
- Group similar tasks together
- Maximize cache hit rates
- Reduce API overhead

---

## Recommended Model Pool for Optimizer

### Tier 1: Premium (Complex Reasoning)
- **Claude Opus 4.6**: Architecture, design, complex algorithms
- **GPT-4 Turbo**: Alternative for complex reasoning
- **Usage**: 5-10% of tasks

### Tier 2: Workhorse (Balanced)
- **Claude Sonnet 4.5**: General coding, orchestration, refactoring
- **DeepSeek Coder V2.5**: Cost-effective code generation
- **GPT-4o**: Multi-modal needs, fast responses
- **Usage**: 60-70% of tasks

### Tier 3: Budget (High Volume)
- **Claude Haiku 4.5**: Testing, documentation, simple edits
- **DeepSeek V3**: Cost-optimized general tasks
- **GLM-4.7**: Additional budget option
- **Usage**: 20-30% of tasks

### Tier 4: Specialized
- **Qwen 2.5-Coder-32B**: Self-hosted option for privacy
- **Kimi-K2**: Very long context tasks (if accessible)
- **Usage**: As needed

---

## Implementation Example

### Control Panel Task Allocation
```python
task_model_mapping = {
    "architecture": ["claude-opus-4.6", "gpt-4-turbo"],
    "code_generation": ["deepseek-coder-v2.5", "claude-sonnet-4.5", "qwen-2.5-coder-32b"],
    "refactoring": ["claude-sonnet-4.5", "deepseek-v3", "gpt-4o"],
    "testing": ["claude-haiku-4.5", "deepseek-coder-v2.5", "gpt-4o"],
    "code_review": ["claude-opus-4.6", "gpt-4-turbo", "claude-sonnet-4.5"],
    "documentation": ["claude-haiku-4.5", "gpt-3.5-turbo", "deepseek-v3"],
    "debugging": ["claude-sonnet-4.5", "deepseek-coder-v2.5", "qwen-2.5-coder-32b"],
    "optimization": ["claude-opus-4.6", "deepseek-v3", "gpt-4-turbo"],
    "boilerplate": ["claude-haiku-4.5", "gpt-3.5-turbo", "qwen-2.5-coder-7b"],
    "research": ["claude-opus-4.6", "claude-sonnet-4.5", "gpt-4-turbo"],
}

# Cost-aware routing
def select_model(task_type, complexity="medium", budget_mode=False):
    candidates = task_model_mapping.get(task_type, ["claude-sonnet-4.5"])

    if budget_mode:
        # Prefer cheaper models
        return candidates[-1] if len(candidates) > 1 else candidates[0]

    if complexity == "high":
        return candidates[0]  # Best model
    elif complexity == "medium":
        return candidates[1] if len(candidates) > 1 else candidates[0]
    else:  # low
        return candidates[-1]  # Cheapest capable model
```

---

## Updated: 2026-02-07
