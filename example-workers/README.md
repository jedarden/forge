# Example Worker Configurations

This directory contains example worker configuration files for various AI models that can be used with FORGE.

## Available Workers

| Model | Tier | Input Cost/1M tokens | Output Cost/1M tokens | Best For |
|-------|------|---------------------|----------------------|----------|
| **claude-sonnet** | Premium | $3.00 | $15.00 | Balanced performance for most tasks |
| **claude-haiku** | Budget | $0.25 | $1.25 | Quick tasks, simple operations |
| **claude-opus** | Premium | $15.00 | $75.00 | Complex reasoning, architecture |
| **gpt-4-turbo** | Premium | $10.00 | $30.00 | Diverse tasks, data analysis |
| **qwen-coder** | Free | $0.00 | $0.00 | Basic coding, completions |

## Using Example Workers

### Option 1: Copy to ~/.forge/workers

```bash
# Copy individual worker
cp example-workers/claude-sonnet.yaml ~/.forge/workers/

# Copy all workers
cp example-workers/*.yaml ~/.forge/workers/
```

### Option 2: Symlink for Easy Updates

```bash
ln -s $(pwd)/example-workers/claude-sonnet.yaml ~/.forge/workers/claude-sonnet.yaml
```

### Option 3: Reference Directly in Config

Add to `~/.forge/config.yaml`:

```yaml
worker_repos:
  - url: "file:///home/coder/forge"
    branch: "main"
    path: "example-workers/"
```

## Configuration Schema

All worker configs follow this schema:

```yaml
# Required fields
name: "worker-name"
description: "Human-readable description"
launcher: "launcher-name"  # References launcher in config.yaml
model: "model-id"
tier: "premium"  # premium, standard, budget, free

# Cost information (required)
cost_per_million_tokens:
  input: 0.0
  output: 0.0

# Subscription (optional)
subscription:
  enabled: false
  monthly_cost: 0
  quota_type: "unlimited"  # or "tokens", "requests"
  quota_limit: null

# Environment variables (optional)
environment:
  API_KEY: "${API_KEY}"  # Use ${VAR} syntax

# Spawn arguments (optional)
spawn_args:
  - "--flag=${variable}"

# File paths (optional, with placeholders)
log_path: "~/.forge/logs/${worker_id}.log"
status_path: "~/.forge/status/${worker_id}.json"

# Health check (optional)
health_check:
  enabled: true
  interval_seconds: 60
  timeout_seconds: 10
  command: "tmux has-session -t ${session_name}"

# Capabilities (optional)
capabilities:
  - "code_generation"
  - "code_review"
max_context_tokens: 200000
supports_tools: true
supports_vision: false
```

## Validation

Validate any worker config before using:

```bash
python3 test/worker-config-validator.py example-workers/claude-sonnet.yaml
```

## Customizing Workers

1. Copy an example config
2. Modify fields for your needs
3. Validate with the validator script
4. Copy to `~/.forge/workers/`

## Adding New Workers

When adding new workers, include:
- Accurate cost information
- Appropriate tier classification
- Relevant capabilities
- Clear description of best use cases

See `test/worker-config-validator.py` for complete validation rules.
