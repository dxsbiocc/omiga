# Builtin Agent Prompts

This directory contains the default system prompts for all built-in Omiga agents.

## How to customize

**Project-level override** (this directory):  
Edit any `.md` file here. Changes take effect on the next agent call — no restart needed.

**User-global override** (`~/.omiga/agents/builtins/<type>.md`):  
Same format, applies across all projects.

**Full agent replacement** (`.omiga/agents/<type>.md`):  
Drop a full agent definition (with YAML frontmatter) into `.omiga/agents/`.  
This replaces everything — prompt, tools, model, color — and is hot-reloaded instantly.

## File format

```markdown
---
description: Human-readable description (optional)
model: standard | frontier | fast   # optional — overrides model tier
tools: [web_search, web_fetch, ...]  # optional — overrides allowed tools
color: "#hex"                         # optional — overrides UI color
---

Your custom system prompt here.
Supports {cwd} placeholder — replaced with the current project root at runtime.
```

The frontmatter block is optional. If omitted, the entire file is used as the system prompt.

## Available agents

| File | Agent type | Role |
|------|-----------|------|
| `executor.md` | `executor` | End-to-end task execution |
| `architect.md` | `architect` | Verification authority and design review |
| `debugger.md` | `debugger` | Root-cause bug investigation |
| `explore.md` | `Explore` | Read-only codebase search |
| `plan.md` | `Plan` | Read-only planning and design |
| `verification.md` | `verification` | Adversarial testing |
| `general-purpose.md` | `general-purpose` | Main assistant / commander |
| `literature-search.md` | `literature-search` | Academic database search (PubMed, arXiv, bioRxiv) |
| `deep-research.md` | `deep-research` | Comprehensive domain survey with citations |
| `data-analysis.md` | `data-analysis` | Scientific data analysis (Python/R) |
| `data-viz.md` | `data-viz` | Publication-ready figure generation |

> **Note**: Your edits to these files are never overwritten by Omiga.
