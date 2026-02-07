# Contributing to FORGE

Thank you for your interest in contributing to FORGE!

## Development Setup

```bash
# Clone the repository
git clone https://github.com/jedarden/forge.git
cd forge

# Install dependencies (TBD based on implementation choice)
# For Python/Textual:
pip install -r requirements.txt

# For Rust/Ratatui:
cargo build
```

## Project Structure

```
forge/
├── src/              # Source code (TBD: Rust or Python)
├── research/         # Design documentation and research
├── docs/             # User documentation
├── tests/            # Test suite
└── examples/         # Usage examples
```

## Development Workflow

1. **Fork** the repository
2. **Create a branch** for your feature (`git checkout -b feature/amazing-feature`)
3. **Make your changes** with clear, atomic commits
4. **Add tests** for new functionality
5. **Update documentation** as needed
6. **Submit a pull request** with a clear description

## Commit Message Guidelines

Use conventional commits:
- `feat: Add new feature`
- `fix: Fix bug`
- `docs: Update documentation`
- `refactor: Refactor code`
- `test: Add tests`
- `chore: Maintenance tasks`

## Code Style

- **Rust**: Follow `rustfmt` and `clippy` recommendations
- **Python**: Follow PEP 8, use `black` for formatting
- **Documentation**: Clear, concise, with examples

## Testing

```bash
# Run tests (TBD based on implementation)
cargo test      # Rust
pytest          # Python
```

## Research & Design

Before implementing major features, review the research documentation in `research/`:
- Understand the architecture and design decisions
- Check if your feature aligns with the project goals
- Open an issue to discuss before large changes

## Questions?

Open an issue or start a discussion. We're here to help!

---

**FORGE** - Federated Orchestration & Resource Generation Engine
