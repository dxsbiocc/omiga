---
name: notebook-helper
description: Create, initialize, repair, render-aware edit, and explain Jupyter notebooks in Omiga.
tags:
  - notebook
  - jupyter
  - python
  - ipynb
---

# Jupyter Notebook Helper

Use this workflow when the user asks to create, initialize, repair, render, or
explain a Jupyter notebook.

## Architecture alignment

Omiga follows the same high-level separation used by VS Code Jupyter:

1. **Serializer/model:** parse `.ipynb` JSON into cells, normalize source arrays,
   preserve metadata, and write valid nbformat JSON back.
2. **Cell language:** prefer `metadata.language_info.name`, then
   `metadata.kernelspec.language`, then notebook defaults.
3. **Renderer:** choose outputs by MIME priority. Preserve unsupported rich outputs
   such as ipywidgets instead of deleting them.
4. **Controller/execution:** execute supported local Python/R code through Omiga's
   notebook command. Do not assume a persistent Jupyter kernel exists unless the UI
   explicitly adds one.

## Editing rules

1. Inspect the target `.ipynb` file before editing.
2. If it is empty, initialize it as valid nbformat 4.5 JSON.
3. Preserve existing cells, cell `id`s, metadata, and outputs unless the user
   explicitly asks to clear or rewrite them.
4. Prefer small executable cells with short markdown explanations.
5. Use `notebook_edit` rather than raw JSON `file_edit` for cell edits.
6. For malformed notebooks, repair the smallest valid structure:
   - top-level `cells` array
   - `metadata`
   - `nbformat`
   - `nbformat_minor`
   - valid `cell_type`, `source`, and code-cell `outputs` / `execution_count`
7. After editing, validate the file is valid JSON and still has
   `nbformat` / `nbformat_minor` fields.

## Rendering notes

- HTML output is sandboxed by the UI setting.
- SVG output should be treated as an image resource, not injected as live DOM.
- Widget MIME (`application/vnd.jupyter.widget-view+json`) is preserved but not
  executed by Omiga's local renderer.
- If the notebook needs packages, explain the local Python/R environment
  requirement instead of silently installing packages.
