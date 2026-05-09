# Visualization-R Migration Inventory

Source reviewed: `~/.codex/skills/omics-visualization-template`.

## First-phase migrated templates

| Source slug | Target template id | Target path | Notes |
| --- | --- | --- | --- |
| `scatter/basic` | `viz_scatter_basic` | `templates/scatter/basic` | General scatter template. |
| `scatter/correlation` | `viz_scatter_correlation` | `templates/scatter/correlation` | Correlation/fitted-line scatter. |
| `scatter/volcano` | `viz_scatter_volcano` | `templates/scatter/volcano` | Kept as `omics-preset` tag, not top-level omics category. |
| `boxplot/basic` | `viz_distribution_boxplot` | `templates/distribution/boxplot` | Distribution grammar naming. |
| `boxplot/violin` | `viz_distribution_violin` | `templates/distribution/violin` | Distribution grammar naming. |
| `bar/basic` | `viz_bar_basic` | `templates/bar/basic` | General bar template. |
| `bar/grouped` | `viz_bar_grouped` | `templates/bar/grouped` | Optional SE column. |
| `heatmap/basic` | `viz_heatmap_basic` | `templates/heatmap/basic` | Long-form tile heatmap. |
| `heatmap/cluster_basic` | `viz_heatmap_clustered` | `templates/heatmap/clustered` | Wide matrix clustered with base R ordering + ggplot tile rendering. |
| `line/group` | `viz_line_group` | `templates/line/group` | Grouped line plot. |

## Later optional candidates

- `scatter/pca_scores`
- `scatter/embedding`
- `scatter/quadrant`
- `scatter/bubble`
- `bar/enrichment_points`
- `line/gsea_curve`
- `line/time_series`
- `boxplot/paired`
- `boxplot/raincloud`
- `heatmap/correlation`

## Excluded from core migration

- `references/omicsagent-*`
- `scripts/audit_omicsagent_visual_coverage.R`
- handwritten `templates/catalog.csv` as a source of truth
- generated gallery PNGs
- template-local output directories

## Dependency policy

The first-phase templates use `ggplot2` plus base R only. Missing packages are reported by helper code; templates do not auto-install dependencies.
