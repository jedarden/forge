# ADR 0001: Use "FORGE" as Project Name

**Date**: 2026-02-07
**Status**: Accepted
**Deciders**: Jed Arden, Claude Sonnet 4.5

---

## Context

We needed a name for the AI agent orchestration control panel that is:
- Memorable and pronounceable
- Available across key namespaces (GitHub, domains, package registries)
- Works well as an acronym
- Conveys the purpose of the project

Multiple naming options were evaluated:
1. Initial consideration: "LLM-Forge" (hyphenated)
2. Non-hyphenated alternatives: ForgeLM, AgentForge, CodeForge
3. Creative neologisms: Forgenix, Forgent, Llamarx, Agrix, Korgex
4. Simple naming: ForgeAI, Forge

---

## Decision

We chose **FORGE** as the project name with the acronym:

**F**ederated **O**rchestration & **R**esource **G**eneration **E**ngine

---

## Rationale

### Strengths
1. **Simple and memorable**: Single word, no hyphens or numbers
2. **Strong metaphor**: Blacksmith's forge transforms raw materials into refined tools; FORGE transforms AI resources into coordinated intelligence
3. **Clear acronym**: Federated, Orchestration, Resource, Generation, Engine - all key concepts
4. **Professional yet approachable**: Works for indie developers and enterprises
5. **Namespace strategy**: Use qualifiers where needed (llm-forge, forge-cli) while branding as "FORGE"

### Namespace Availability
- GitHub: `jedarden/forge` (owned by project)
- Domain: `llm-forge.dev` available as fallback
- PyPI: `llmforge` or `llm-forge` available
- npm: `llm-forge` available
- CLI command: `forge` (via Python entry point or PATH)

### Alternatives Considered

**LLM-Forge**:
- ✅ Clear LLM reference
- ✅ Perfect availability
- ❌ Hyphen adds friction
- ❌ Less memorable

**Forgenix**:
- ✅ Perfect availability
- ✅ Unique, distinctive
- ❌ Requires explanation (phoenix/unix reference not obvious)
- ❌ More complex

**Forgent**:
- ✅ Perfect availability
- ✅ Clear "forge + agent" portmanteau
- ❌ Less distinctive than FORGE
- ❌ Portmanteau needs explanation

**ForgeAI**:
- ✅ Crystal clear meaning
- ❌ Major namespace conflicts (GitHub, PyPI, .com, .dev all taken)
- ❌ Expensive .ai domain ($100+/year)

---

## Consequences

### Positive
- Strong, memorable brand identity
- Acronym works well for enterprise positioning
- Simple to type and say
- Metaphor is self-explanatory
- Package names can use qualifiers while brand stays clean

### Negative
- "forge" alone is taken in many namespaces (expected for common word)
- Requires namespace qualifiers for some packages (llm-forge, forge-cli)
- Need to educate users on acronym meaning (but acronym is optional)

### Neutral
- Acronym emphasis in README makes it feel more "enterprise"
- Can be positioned as simple tool (just "forge") or sophisticated system (FORGE acronym)

---

## Implementation

1. **Brand name**: FORGE (all caps in headers, title case elsewhere)
2. **GitHub**: `github.com/jedarden/forge`
3. **README**: Emphasize acronym at top
4. **CLI command**: `forge` (via entry point)
5. **Package names**: Use qualifiers as needed (llm-forge, forgeai, etc.)
6. **Domain**: Consider `forge.tools`, `forge.sh`, or `llm-forge.dev`

---

## References

- [Naming Analysis](../notes/naming-options.md)
- [Pronounceability Analysis](../notes/naming-pronounceability-analysis.md)
- [Creative Neologisms Research](../notes/naming-creative-neologisms.md)
- [LLM-Forge Branding Guide](../notes/LLM-FORGE-BRANDING.md)
