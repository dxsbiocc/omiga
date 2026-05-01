---
name: notebook-helper
description: Create, initialize, repair, and explain Jupyter notebooks in Omiga.
tags:
  - notebook
  - jupyter
  - python
---

# Notebook Helper

Use this workflow when the user asks to create, initialize, repair, or explain a Jupyter notebook.

1. Inspect the target `.ipynb` file before editing.
2. If it is empty, initialize it as valid nbformat 4 JSON with at least one code or markdown cell that matches the user's intent.
3. Preserve existing cells and outputs unless the user explicitly asks to clear or rewrite them.
4. Prefer small, executable Python cells with short markdown explanations.
5. After editing, validate that the notebook is valid JSON and still has `nbformat` / `nbformat_minor` fields.
