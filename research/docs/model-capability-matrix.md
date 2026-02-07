# Model Capability Matrix

## Overview

This matrix defines the capabilities, strengths, and cost efficiency of different LLM models across various task dimensions. It enables intelligent model selection based on task requirements.

## Model Profiles

### Claude Opus 4.6
**Tier**: Premium
**Best For**: Architecture design, complex refactoring, high-stakes production work

| Capability | Rating | Notes |
|------------|--------|-------|
| Code Generation Quality | 9.5/10 | Excellent across all languages |
| Reasoning Depth | 10/10 | Best for architecture and system design |
| Context Handling | 9.5/10 | 200K context, excellent long-term coherence |
| Speed | 6/10 | Slower time-to-first-token, thoughtful responses |
| Reliability | 9.5/10 | Very consistent, rarely hallucinates |
| Cost Efficiency | 4/10 | Most expensive ($15/$75 per MTok input/output) |

**Language Strengths**:
- Python: 9.5/10 (excellent for complex algorithms, ML code)
- TypeScript/JavaScript: 9/10 (strong React, Node.js understanding)
- Rust: 9/10 (excellent with borrow checker, async)
- Go: 9/10 (great for concurrent systems)
- YAML/Config: 9.5/10 (Kubernetes, CI/CD manifests)

**Task Type Performance**:
- Architecture Design: 10/10 - Best choice for system design
- Complex Refactoring: 9.5/10 - Handles multi-file changes excellently
- API Design: 9.5/10 - Creates well-structured, RESTful APIs
- Bug Fixing: 8.5/10 - Good, but may be overkill for simple bugs
- Testing: 9/10 - Writes comprehensive test suites
- Documentation: 9/10 - Excellent technical writing

**Optimal Use Cases**:
- Production-critical infrastructure changes
- Large-scale refactoring (10+ files)
- Complex algorithm implementation
- System architecture design
- High-risk deployments requiring careful analysis

---

### Claude Sonnet 4.5
**Tier**: Mid-Premium
**Best For**: General-purpose coding, balanced performance and cost

| Capability | Rating | Notes |
|------------|--------|-------|
| Code Generation Quality | 9/10 | Very high quality, slightly below Opus |
| Reasoning Depth | 8.5/10 | Strong reasoning, handles most complex tasks |
| Context Handling | 9/10 | 200K context, good long-term coherence |
| Speed | 8/10 | Fast time-to-first-token, responsive |
| Reliability | 9/10 | Highly consistent and reliable |
| Cost Efficiency | 8/10 | Good value ($3/$15 per MTok input/output) |

**Language Strengths**:
- Python: 9/10 (excellent general-purpose coding)
- TypeScript/JavaScript: 9/10 (strong full-stack development)
- Rust: 8.5/10 (very capable, occasionally needs guidance)
- Go: 8.5/10 (solid concurrent programming)
- YAML/Config: 9/10 (Kubernetes, IaC)

**Task Type Performance**:
- Architecture Design: 8.5/10 - Very capable for most designs
- Complex Refactoring: 9/10 - Excellent multi-file handling
- API Design: 9/10 - Creates clean, maintainable APIs
- Bug Fixing: 9/10 - Great at debugging and fixing issues
- Testing: 9/10 - Comprehensive test coverage
- Documentation: 8.5/10 - Clear, concise documentation

**Optimal Use Cases**:
- Standard feature development
- Multi-file refactoring (3-10 files)
- API implementation and integration
- Test suite creation
- Code review and optimization
- Most P0-P1 tasks that aren't architecture-level

---

### DeepSeek Coder V3
**Tier**: Mid-Range
**Best For**: High-volume coding tasks, specialized code generation

| Capability | Rating | Notes |
|------------|--------|-------|
| Code Generation Quality | 8.5/10 | Excellent pure coding, less context-aware |
| Reasoning Depth | 7/10 | Good for implementation, weaker on design |
| Context Handling | 8/10 | 64K context, handles medium complexity |
| Speed | 9/10 | Very fast generation |
| Reliability | 8/10 | Consistent for coding, occasional hallucinations |
| Cost Efficiency | 9/10 | Excellent value ($0.14/$0.28 per MTok) |

**Language Strengths**:
- Python: 9/10 (trained heavily on Python codebases)
- TypeScript/JavaScript: 8/10 (good, but less idiomatic)
- Rust: 7.5/10 (capable but may need iteration)
- Go: 8/10 (solid performance)
- YAML/Config: 7/10 (functional but less polished)

**Task Type Performance**:
- Architecture Design: 6/10 - Better suited for implementation
- Complex Refactoring: 7.5/10 - Can handle with clear instructions
- API Design: 7.5/10 - Creates functional APIs, may lack polish
- Bug Fixing: 8.5/10 - Excellent at targeted fixes
- Testing: 8/10 - Good test coverage, less edge case handling
- Documentation: 7/10 - Functional but basic

**Optimal Use Cases**:
- High-volume P2-P3 tasks
- Straightforward implementations from specs
- Algorithm optimization
- Data processing scripts
- Quick bug fixes in familiar codebases
- Batch processing of similar tasks

---

### GLM-4.7 (via Z.AI)
**Tier**: Budget
**Best For**: Simple tasks, high-volume low-value work

| Capability | Rating | Notes |
|------------|--------|-------|
| Code Generation Quality | 7/10 | Good for simple tasks, struggles with complexity |
| Reasoning Depth | 6/10 | Limited architectural reasoning |
| Context Handling | 7/10 | 128K context, but coherence degrades |
| Speed | 9.5/10 | Very fast, low latency |
| Reliability | 7/10 | Inconsistent on complex tasks |
| Cost Efficiency | 10/10 | Free tier available, very cheap |

**Language Strengths**:
- Python: 8/10 (good for simple scripts)
- TypeScript/JavaScript: 7/10 (functional implementations)
- Rust: 6/10 (struggles with advanced features)
- Go: 7/10 (decent for simple services)
- YAML/Config: 7.5/10 (handles configs well)

**Task Type Performance**:
- Architecture Design: 4/10 - Not recommended
- Complex Refactoring: 5/10 - Struggles with multi-file changes
- API Design: 6/10 - Can create basic endpoints
- Bug Fixing: 7.5/10 - Good for simple, isolated bugs
- Testing: 6.5/10 - Basic test coverage
- Documentation: 7.5/10 - Good at templated docs

**Optimal Use Cases**:
- P3-P4 low-priority tasks
- Simple CRUD implementations
- Config file generation
- Boilerplate code creation
- Low-risk experimental work
- Documentation updates

---

### GPT-4 Turbo
**Tier**: Premium Alternative
**Best For**: Broad knowledge tasks, when Claude unavailable

| Capability | Rating | Notes |
|------------|--------|-------|
| Code Generation Quality | 8.5/10 | Very good, slightly less consistent than Claude |
| Reasoning Depth | 8.5/10 | Strong reasoning capabilities |
| Context Handling | 8/10 | 128K context, good coherence |
| Speed | 7/10 | Moderate speed |
| Reliability | 8/10 | Generally reliable, occasional errors |
| Cost Efficiency | 6/10 | $10/$30 per MTok input/output |

**Language Strengths**:
- Python: 8.5/10 (excellent for data science, web)
- TypeScript/JavaScript: 9/10 (very strong in modern JS/TS)
- Rust: 7.5/10 (capable but less specialized)
- Go: 8/10 (good for web services)
- YAML/Config: 8/10 (solid configuration handling)

**Task Type Performance**:
- Architecture Design: 8/10 - Good for most designs
- Complex Refactoring: 8/10 - Capable with clear guidance
- API Design: 8.5/10 - Strong API design skills
- Bug Fixing: 8/10 - Good debugging abilities
- Testing: 8.5/10 - Comprehensive testing
- Documentation: 8.5/10 - Excellent technical writing

**Optimal Use Cases**:
- When Claude quota exhausted
- JavaScript/TypeScript heavy projects
- Tasks requiring broad general knowledge
- Integration with OpenAI-specific tools
- Customer-facing documentation

---

### Qwen2.5-Coder (32B)
**Tier**: Mid-Range
**Best For**: Open-source alternative, self-hosted scenarios

| Capability | Rating | Notes |
|------------|--------|-------|
| Code Generation Quality | 8/10 | Strong coding abilities |
| Reasoning Depth | 7/10 | Good implementation reasoning |
| Context Handling | 7.5/10 | 32K context, decent coherence |
| Speed | 8.5/10 | Fast with local deployment |
| Reliability | 7.5/10 | Consistent for defined tasks |
| Cost Efficiency | 9.5/10 | Free if self-hosted, cheap API |

**Language Strengths**:
- Python: 8.5/10 (excellent for ML/data tasks)
- TypeScript/JavaScript: 7.5/10 (functional implementations)
- Rust: 7/10 (capable with guidance)
- Go: 7.5/10 (good for services)
- YAML/Config: 7/10 (handles configs)

**Task Type Performance**:
- Architecture Design: 6.5/10 - Limited design capabilities
- Complex Refactoring: 7/10 - Can handle with clear specs
- API Design: 7/10 - Creates functional APIs
- Bug Fixing: 8/10 - Good debugging skills
- Testing: 7.5/10 - Solid test coverage
- Documentation: 7/10 - Basic documentation

**Optimal Use Cases**:
- Self-hosted deployments (data privacy)
- High-volume tasks with cost constraints
- China-based deployments (local API)
- Python/ML-focused projects
- P2-P3 tasks requiring decent quality

---

## Model Selection Decision Tree

```
Task Value Score >= 90?
├─ YES → Opus 4.6 (if quota available) or GPT-4
└─ NO
   ├─ Task Value Score >= 75?
   │  ├─ YES → Sonnet 4.5 (primary) or GPT-4 Turbo
   │  └─ NO
   │     ├─ Task Value Score >= 60?
   │     │  ├─ YES → DeepSeek V3 or Qwen2.5
   │     │  └─ NO
   │     │     ├─ Task Value Score >= 40?
   │     │     │  ├─ YES → GLM-4.7 or Qwen2.5
   │     │     │  └─ NO → Defer or batch tasks

Domain-Specific Overrides:
- Infrastructure/K8s → Prefer Claude models (better YAML understanding)
- JavaScript/React → GPT-4 Turbo competitive with Sonnet
- Python/ML → DeepSeek or Qwen competitive for implementation
- Complex Architecture → Always Opus regardless of score
```

## Token Consumption Estimates

| Task Complexity | Estimated Tokens (Input) | Estimated Tokens (Output) | Total Cost Range |
|----------------|-------------------------|--------------------------|-----------------|
| Simple (1-2 files) | 5K - 15K | 500 - 2K | $0.02 - $1.50 |
| Moderate (3-5 files) | 15K - 40K | 2K - 5K | $0.10 - $5.00 |
| Complex (6-10 files) | 40K - 100K | 5K - 15K | $0.50 - $15.00 |
| Highly Complex (10+ files) | 100K - 200K | 15K - 40K | $2.00 - $40.00 |

**Note**: Costs calculated using Opus pricing as upper bound. Actual costs vary by model selection.

## Performance Benchmarks

### HumanEval (Code Generation)
- Opus 4.6: ~92%
- Sonnet 4.5: ~89%
- GPT-4 Turbo: ~86%
- DeepSeek V3: ~83%
- Qwen2.5-Coder: ~79%
- GLM-4.7: ~68%

### SWE-bench (Real-World Engineering)
- Opus 4.6: ~38% (estimated)
- Sonnet 4.5: ~33%
- GPT-4 Turbo: ~28%
- DeepSeek V3: ~24%
- Qwen2.5-Coder: ~20%
- GLM-4.7: ~12%

### MBPP (Multi-Turn Programming)
- Opus 4.6: ~88%
- Sonnet 4.5: ~84%
- GPT-4 Turbo: ~81%
- DeepSeek V3: ~77%
- Qwen2.5-Coder: ~73%
- GLM-4.7: ~62%

## Subscription vs API Strategy

### Claude Pro ($20/month)
- Effective for: 100-200 moderate tasks/month
- Break-even: ~1.3M input + 300K output tokens
- Best strategy: Fill quota with high-value Sonnet tasks
- Overflow: Use API for Opus on critical tasks

### ChatGPT Plus ($20/month)
- Effective for: Limited by message caps, not tokens
- Best strategy: Use for exploratory work, prototyping
- Overflow: API for production tasks

### API-Only Strategy
- Viable for: <50 tasks/month or highly variable workload
- Optimize by: Dynamic model selection based on task value
- Cost control: Set monthly budget, track spend per task
