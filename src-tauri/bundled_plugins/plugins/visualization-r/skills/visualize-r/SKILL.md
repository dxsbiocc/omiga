---
name: visualize-r
description: Create editable static R figures with visualization-r Template units; use for ggplot2/ComplexHeatmap-style table-to-figure work.
tags: [visualization, r, template, figure]
---

# Visualize R

Use `visualization-r` Templates for static R figures.

## Rules

- Use Template units, not Operators, for plot styling.
- `template.R.j2` is R source, not JSON-to-R DSL.
- Keep params for stable inputs only; customize the emitted source when needed.
- Source artifact form follows the user's intent: script, document, or code block are all valid when appropriate.
- Promote a reusable style only on explicit user request.
- Template priority: project preference > user preference > bundled.

## Workflow

1. Select a template by visual grammar/tags. Read `../../TEMPLATE_INDEX.md` only when a template list is needed.
2. Run it with the user's table and params.
3. Verify declared figure outputs are non-empty.
4. For tweaks, edit the emitted source.
5. If asked to save style, promote the source to a user/project preference template.

$ARGUMENTS
