## Memory System

The omiga agent uses a persistent memory system. When starting a conversation or when context about past work is needed, always check the relevant memory files.

### Memory Registry

The global registry maps project paths to their memory locations:
```
~/.omiga/memory/registry.json
```

Look up the current project path in `registry.json` to find:
- `memory_root` — root of this project's memory
- `wiki_path` — wiki pages built by pageindex (structured knowledge)
- `implicit_path` — implicit memories (observed patterns, preferences)
- `permanent_wiki_path` — global/permanent wiki shared across projects (`~/.omiga/memory/permanent/wiki/`)

### Memory Layout

```
~/.omiga/memory/
├── registry.json              # Project → memory path mapping
├── permanent/
│   └── wiki/                  # Global wiki (cross-project knowledge)
└── projects/
    └── <hash>/                # Centralized storage for projects without local write access
        ├── wiki/
        └── implicit/

<project-root>/.omiga/memory/  # Local project memory (preferred when writable)
├── wiki/                      # Pageindex-built wiki articles
└── implicit/                  # Implicit memory files
```

### How to Retrieve Memory

1. **Start of conversation**: Read `~/.omiga/memory/registry.json` to find the memory paths for the current working directory.
2. **Wiki knowledge**: List and read files under `wiki_path` for structured project knowledge.
3. **Implicit memories**: Read files under `implicit_path` for observed patterns and preferences.
4. **Global knowledge**: Check `~/.omiga/memory/permanent/wiki/` for cross-project facts.

When the user asks you to "remember", "recall", or "check memory", always resolve the current project's paths via the registry first, then read the relevant files.

---

## Skill routing

When the user's request matches an available skill, ALWAYS invoke it using the Skill
tool as your FIRST action. Do NOT answer directly, do NOT use other tools first.
The skill has specialized workflows that produce better results than ad-hoc answers.

Key routing rules:

- Product ideas, "is this worth building", brainstorming → invoke office-hours
- Bugs, errors, "why is this broken", 500 errors → invoke investigate
- Ship, deploy, push, create PR → invoke ship
- QA, test the site, find bugs → invoke qa
- Code review, check my diff → invoke review
- Update docs after shipping → invoke document-release
- Weekly retro → invoke retro
- Design system, brand → invoke design-consultation
- Visual audit, design polish → invoke design-review
- Architecture review → invoke plan-eng-review
- Save progress, checkpoint, resume → invoke checkpoint
- Code quality, health check → invoke health
