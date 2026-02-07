# Coding Orchestrators Comparison

## Overview
This document compares coding orchestrators for intelligent pool optimization and task allocation.

---

## 1. Claude Code

### Overview
Official Anthropic CLI for Claude with agent-based coding capabilities.

### Features
- **Model Support**: Claude Sonnet 4.5, Opus 4.6, Haiku 4.5 (primary), other Anthropic models
- **API Compatibility**: Anthropic API native, can work with OpenAI-compatible proxies
- **CLI Capabilities**: Full terminal integration, tmux session management, autonomous execution
- **Concurrent Execution**: Built-in task system for spawning parallel agents
- **Strengths**:
  - Native Claude integration with latest models
  - Strong agent orchestration with Task tools
  - Excellent file operations (Read, Write, Edit, Glob, Grep)
  - Web search and fetch capabilities
  - Jupyter notebook support
  - Git workflow automation
  - MCP (Model Context Protocol) support
  - Skills system for reusable capabilities

### Limitations
- Primarily designed for Anthropic models
- Requires API key management
- May have rate limits based on API tier

### Use Cases
- Complex multi-file refactoring
- Agent-based task decomposition
- Research and analysis workflows
- Full-stack development with git integration

---

## 2. Aider

### Overview
AI pair programming tool with strong git integration and multi-model support.

### Features
- **Model Support**:
  - Claude (Sonnet, Opus, Haiku)
  - GPT-4, GPT-4 Turbo, GPT-3.5
  - DeepSeek Coder
  - Qwen models
  - Open source models via APIs
- **API Compatibility**: OpenAI API, Anthropic API, OpenRouter, custom endpoints
- **CLI Capabilities**:
  - Terminal-based chat interface
  - Git integration (auto-commit, branch management)
  - File editing with semantic understanding
  - Multiple edit formats (whole, diff, udiff)
- **Concurrent Execution**: Limited (single agent per session)
- **Strengths**:
  - Excellent git integration
  - Universal model connector (supports 100+ models)
  - Cost optimization features (weak/strong model pairing)
  - Repo map for large codebase understanding
  - Streaming responses
  - Voice input support

### Limitations
- Single agent per session (no native parallelization)
- Focused on file editing vs orchestration
- Less suited for complex multi-agent workflows

### Use Cases
- Quick code changes and refactoring
- Git-centric development workflows
- Cost-optimized coding with model switching
- Codebase exploration and modification

---

## 3. Cursor

### Overview
AI-first code editor (fork of VS Code) with deep model integration.

### Features
- **Model Support**:
  - GPT-4, GPT-4 Turbo
  - Claude Sonnet, Opus
  - Custom model endpoints
- **API Compatibility**: OpenAI API, Anthropic API, custom providers
- **CLI Capabilities**: Limited (primarily GUI-based)
- **Concurrent Execution**: Multiple agent contexts in tabs
- **Strengths**:
  - Seamless IDE integration
  - Inline code completion (Copilot++)
  - Chat interface with codebase context
  - Multi-file editing
  - Fast response times
  - Terminal integration within IDE
  - Privacy mode (SOC2 compliant)

### Limitations
- Requires GUI (not pure CLI)
- Subscription-based pricing
- Less suitable for headless/automated workflows
- Not designed for orchestration

### Use Cases
- Interactive development
- Real-time code completion
- IDE-integrated AI assistance
- Visual coding workflows

---

## 4. Continue.dev

### Overview
Open-source AI code assistant with IDE integration and customization focus.

### Features
- **Model Support**:
  - OpenAI (GPT-4, GPT-3.5)
  - Claude (all models)
  - Llama, Mistral, CodeLlama
  - Custom/local models
  - DeepSeek, Qwen
- **API Compatibility**: OpenAI API, Anthropic API, Ollama, LM Studio, custom endpoints
- **CLI Capabilities**: Limited (VS Code extension focus)
- **Concurrent Execution**: Single agent per chat
- **Strengths**:
  - Open source and customizable
  - Supports local models
  - Context providers (files, git, docs)
  - Multiple IDE support (VS Code, JetBrains)
  - Slash commands for workflows
  - Model provider flexibility

### Limitations
- Requires IDE (not standalone CLI)
- No native orchestration features
- Single agent interaction model
- Community-driven (slower updates)

### Use Cases
- IDE-integrated assistance
- Local/private model deployment
- Customizable AI workflows
- Multi-IDE support needs

---

## 5. Goose

### Overview
Developer agent by Block (Square) with toolkit extensibility focus.

### Features
- **Model Support**:
  - OpenAI models
  - Anthropic Claude
  - Open source models via providers
- **API Compatibility**: OpenAI API, Anthropic API, extensible provider system
- **CLI Capabilities**:
  - Terminal-based interface
  - Session management
  - Toolkit system (extensible tools)
- **Concurrent Execution**: Single agent with tool execution
- **Strengths**:
  - Extensible toolkit architecture
  - Developer-focused workflows
  - Session persistence
  - Shell command execution
  - File operations
  - Browser automation support

### Limitations
- Relatively new project
- Smaller community than alternatives
- Limited documentation
- No native multi-agent orchestration

### Use Cases
- Developer automation workflows
- Custom tool integration
- Shell-based agent tasks
- Research and exploration

---

## 6. OpenCode

### Overview
(Limited public information available - appears to be internal/unreleased tool)

### Features
- **Status**: Not widely documented in public sources
- Likely similar capabilities to Claude Code given naming similarity
- May be internal/beta Anthropic tooling

### Note
Requires further investigation or internal documentation access.

---

## 7. KiloCode

### Overview
(Limited public information available - possible confusion with Replit's Agent or similar)

### Features
- **Status**: Not clearly documented as standalone orchestrator
- May refer to:
  - Replit's Agent with multi-file context
  - Internal tooling at specific organizations
  - Code name for unreleased product

### Note
Requires clarification on exact product/tool being referenced.

---

## Comparison Matrix

| Feature | Claude Code | Aider | Cursor | Continue.dev | Goose | OpenCode | KiloCode |
|---------|------------|-------|---------|--------------|-------|----------|----------|
| **CLI Native** | Yes | Yes | No (GUI) | No (IDE) | Yes | Unknown | Unknown |
| **Multi-Model** | Limited | Excellent | Good | Excellent | Good | Unknown | Unknown |
| **Concurrent Agents** | Yes | No | Tabs | No | No | Unknown | Unknown |
| **Git Integration** | Excellent | Excellent | Good | Good | Good | Unknown | Unknown |
| **Orchestration** | Strong | Weak | Weak | Weak | Moderate | Unknown | Unknown |
| **Open Source** | No | Yes | No | Yes | Yes | Unknown | Unknown |
| **Cost** | API usage | API usage | Subscription | Free | API usage | Unknown | Unknown |
| **Extensibility** | MCP/Skills | Moderate | Limited | High | High | Unknown | Unknown |
| **Context Window** | Model-dependent | Model-dependent | Model-dependent | Model-dependent | Model-dependent | Unknown | Unknown |
| **Task Decomposition** | Built-in | Manual | Manual | Manual | Manual | Unknown | Unknown |

---

## Orchestration Capabilities Ranking

### 1. Claude Code (Best for Orchestration)
- Native task spawning and agent coordination
- Built-in parallel execution
- Strong autonomous workflows
- Best for: Complex multi-agent workflows, research, full-stack projects

### 2. Goose (Good for Single-Agent Automation)
- Extensible toolkit system
- Developer-focused automation
- Best for: Custom tool workflows, shell automation

### 3. Aider (Best for Git-Centric Development)
- Excellent file editing
- Universal model support
- Best for: Quick code changes, multi-model cost optimization

### 4. Cursor (Best for Interactive Development)
- Seamless IDE integration
- Real-time assistance
- Best for: Interactive coding, visual development

### 5. Continue.dev (Best for Customization)
- Open source flexibility
- Local model support
- Best for: Custom workflows, privacy-focused development

---

## Recommendations for Control Panel

### Primary Orchestrator: Claude Code
**Rationale**:
- Native support for task decomposition and parallel agents
- Strong file operations and research capabilities
- Git workflow automation
- Can spawn multiple specialized agents for different tasks

### Secondary/Complementary: Aider
**Rationale**:
- Universal model support for cost optimization
- Quick code edits and refactoring
- Can be invoked by Claude Code for specific model access
- Excellent for targeted changes

### Development Environment: Cursor or Continue.dev
**Rationale**:
- Interactive development and debugging
- Real-time code completion
- Not for orchestration, but for human developer interaction

---

## Architecture Proposal

```
Control Panel (Main Orchestrator: Claude Code)
├── Research Agent (Claude Sonnet 4.5) - Market analysis, strategy research
├── Code Generation Agent (DeepSeek V3/Qwen Coder) - Implementation tasks
├── Testing Agent (GPT-4 Turbo) - Test generation and validation
├── Review Agent (Claude Opus 4.6) - Code review, architecture decisions
└── Optimization Agent (GLM-4.7) - Performance tuning, cost optimization

Tool Chain:
- Claude Code: Orchestration, task distribution, git management
- Aider: Model-specific code edits (via Claude Code invocation)
- MCP Servers: Data access, API integration
```

---

## Integration Notes

### Model Selection Strategy
1. **Complex reasoning**: Claude Opus 4.6, GPT-4
2. **Fast coding**: DeepSeek V3, Qwen 2.5-Coder
3. **Cost-optimized**: GLM-4.7, Haiku 4.5
4. **Balanced**: Claude Sonnet 4.5, GPT-4 Turbo

### Concurrent Execution Pattern
```bash
# Claude Code native task spawning
Task("Research agent", "Analyze market data...", "researcher")
Task("Coder agent", "Implement strategy...", "coder")
Task("Tester agent", "Generate tests...", "tester")

# Or via tmux + spawn-workers script
./spawn-workers.sh --executor=claude-code-glm-47 --workspace=/control-panel
```

### Cost Optimization
- Use Aider's weak/strong model pairing
- Route simple tasks to cheaper models (GLM-4.7, Haiku)
- Reserve Opus/GPT-4 for architecture and complex reasoning
- Implement caching for repeated queries

---

## Updated: 2026-02-07
