---
name: visualize
description: Route visualization requests to the right backend skill/template library.
triggers:
  - make plot
  - create plot
  - generate figure
  - visualize
  - 画图
  - 生成图表
  - 可视化
  - 出图
  - 绘图
---

# Visualize

Route the request; do not implement a plotting DSL here.

## Route

- Static R/table/publication figures -> `$visualize-r`.
- Python/notebook plotting -> `$visualize-python` when available; otherwise use project Python conventions.
- Interactive web/dashboard charts -> `$visualize-js` when available; otherwise use project frontend conventions.

## Rules

- Prefer existing Template units over one-off plotting code.
- Do not invent JSON-to-plot specifications.
- Source artifact form follows the user's intent; do not hard-code script/document names or formats.
- Save/promote reusable styles only when the user explicitly asks.

## Verify

- Figure artifacts exist and are non-empty.
- Generated outputs stay outside template source directories.

$ARGUMENTS
