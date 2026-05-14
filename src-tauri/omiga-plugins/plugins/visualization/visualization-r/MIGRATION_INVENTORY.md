# Visualization-R Migration Inventory

Source reviewed: `~/.codex/skills/omics-visualization-template`.

## First-phase migrated templates

| Source slug | Target template id | Target path | Notes |
| --- | --- | --- | --- |
| `scatter/basic` | `viz_scatter_basic` | `templates/scatter/basic` | General scatter template. |
| `scatter/beeswarm` | `viz_scatter_beeswarm` | `templates/scatter/beeswarm` | Migrated from ggbeeswarm JSON DSL to direct ggplot2 beeswarm-style grouped expression scatter with deterministic packed jitter, quartile bars, and median markers; `data.json` converted to TSV. |
| `scatter/beeswarm_group` | `viz_scatter_group_beeswarm` | `templates/scatter/group-beeswarm` | Migrated from ggbeeswarm JSON DSL to direct ggplot2 grouped differential beeswarm with category tiles, p-value point sizes, and top labels; `data.json` converted to TSV. |
| `scatter/bezier` | `viz_scatter_bezier` | `templates/scatter/bezier` | Migrated from ggforce/ggsignif JSON DSL to direct ggplot2 paired scatter with Bezier-style curves and paired statistics; `data.json` converted to TSV. |
| `scatter/cleveland` | `viz_scatter_cleveland` | `templates/scatter/cleveland` | Migrated from JSON DSL to direct ggplot2 Cleveland dot plot with guide segments and emphasized endpoint dots; `data.json` converted to TSV. |
| `scatter/correlation` | `viz_scatter_correlation` | `templates/scatter/correlation` | Correlation/fitted-line scatter. |
| `scatter/volcano` | `viz_scatter_volcano` | `templates/scatter/volcano` | Kept as `omics-preset` tag, not top-level omics category. |
| `scatter/diagonal` | `viz_scatter_diagonal` | `templates/scatter/diagonal` | Migrated from JSON DSL to direct ggplot2 log2 diagonal comparison; `data.json` converted to TSV. |
| `scatter/dumbbell_horizontal` | `viz_scatter_dumbbell_horizontal` | `templates/scatter/dumbbell-horizontal` | Migrated from JSON DSL to direct ggplot2 horizontal dumbbell; `data.json` converted to TSV. |
| `scatter/dumbbell_rect` | `viz_scatter_rect_dumbbell` | `templates/scatter/rect-dumbbell` | Migrated from JSON DSL to direct ggplot2 rectangular dumbbell plot with two-group range bands and endpoint dots; `data.json` converted to TSV. |
| `boxplot/basic` | `viz_distribution_boxplot` | `templates/distribution/boxplot` | Distribution grammar naming. |
| `boxplot/bezier_point` | `viz_distribution_bezier_point_boxplot` | `templates/distribution/bezier-point-boxplot` | Migrated from JSON DSL to direct ggplot2 faceted boxplot with points and Bezier sample-connection curves; `data.json` converted to TSV. |
| `boxplot/differential_bg` | `viz_distribution_background_band_boxplot` | `templates/distribution/background-band-boxplot` | Migrated from JSON DSL to direct ggplot2 grouped differential boxplot with per-feature background bands, jittered points, and significance labels; `data.json` converted to TSV. |
| `boxplot/differential_expression` | `viz_distribution_differential_expression_boxplot` | `templates/distribution/differential-expression-boxplot` | Migrated from JSON DSL to direct ggplot2 differential-expression boxplot with statistical brackets; `data.json` converted to TSV. |
| `boxplot/differential_facet` | `viz_distribution_differential_facet_boxplot` | `templates/distribution/differential-facet-boxplot` | Migrated from JSON DSL to direct ggplot2 faceted differential boxplot with jittered points and per-facet significance labels; `data.json` converted to TSV. |
| `boxplot/differential_two` | `viz_distribution_differential_two_boxplot` | `templates/distribution/differential-two-boxplot` | Migrated from JSON DSL to direct ggplot2 two-group differential-expression boxplot with per-feature significance labels; `data.json` converted to TSV. |
| `boxplot/paired` | `viz_distribution_paired_boxplot` | `templates/distribution/paired-boxplot` | Migrated from JSON DSL to direct ggplot2 paired boxplot with patient linking curves and paired statistics; `data.json` converted to TSV. |
| `boxplot/polar` | `viz_distribution_polar_violin` | `templates/distribution/polar-violin` | Migrated from JSON DSL to direct ggplot2 polar/radial violin plot; `data.json` converted to TSV. |
| `boxplot/polar_heatmap` | `viz_distribution_polar_heatmap` | `templates/distribution/polar-heatmap` | Migrated from JSON DSL to direct ggplot2 polar fan plot with outer violins and inner heatmap; `data.json` converted to TSV. |
| `boxplot/violin` | `viz_distribution_violin` | `templates/distribution/violin` | Distribution grammar naming. |
| `boxplot/raincloud` | `viz_distribution_raincloud` | `templates/distribution/raincloud` | Migrated from generic placeholder to direct ggplot2 horizontal raincloud with stacked sample dots and half-density clouds; `data.json` converted to TSV. |
| `boxplot/raincloud_differential` | `viz_distribution_differential_raincloud` | `templates/distribution/differential-raincloud` | Migrated from JSON DSL to direct ggplot2 code; `data.json` converted to TSV. |
| `boxplot/raincloud_vertical` | `viz_distribution_vertical_raincloud` | `templates/distribution/vertical-raincloud` | Migrated from JSON DSL to direct ggplot2 vertical raincloud plot with half-density clouds, compact boxplots, and jittered samples; `data.json` converted to TSV. |
| `bar/basic` | `viz_bar_basic` | `templates/bar/basic` | General bar template. |
| `bar/butterfly` | `viz_bar_butterfly` | `templates/bar/butterfly` | Migrated from JSON DSL to direct ggplot2 two-sided butterfly/population-pyramid bar chart; `data.json` converted to TSV. |
| `bar/enrichment_groups` | `viz_bar_enrichment_groups` | `templates/bar/enrichment-groups` | Migrated from JSON DSL to direct ggplot2 multi-group enrichment bar chart with category strips, count bubbles, and gene labels; `data.json` converted to TSV. |
| `bar/enrichment_expand` | `viz_bar_pathway_expand` | `templates/bar/pathway-expand` | Migrated from JSON DSL to direct ggplot2 category-expanded enrichment bar chart; `data.json` converted to TSV. |
| `bar/enrichment_genes` | `viz_bar_gene_terms` | `templates/bar/gene-terms` | Migrated from JSON DSL to direct ggplot2 enrichment term bars with grouped direction and gene labels; `data.json` converted to TSV. |
| `bar/grouped` | `viz_bar_grouped` | `templates/bar/grouped` | Optional SE column. |
| `bar/opposite` | `viz_bar_diverging` | `templates/bar/diverging` | Migrated from JSON DSL to direct ggplot2 two-sided diverging enrichment bar chart; `data.json` converted to TSV. |
| `bar/percent` | `viz_bar_stacked_percent` | `templates/bar/stacked-percent` | Migrated from JSON DSL to direct ggplot2 faceted percent/stacked bar chart; `data.json` converted to TSV. |
| `bar/radial` | `viz_bar_radial` | `templates/bar/radial` | Migrated from JSON DSL to direct ggplot2 polar/radial bar chart; `data.json` converted to TSV. |
| `bar/radial_groups` | `viz_bar_radial_groups` | `templates/bar/radial-groups` | Migrated from JSON DSL to direct ggplot2 grouped radial bar chart with outer labels and inner group bands; `data.json` converted to TSV. |
| `bar/waffle` | `viz_bar_waffle` | `templates/bar/waffle` | Migrated from JSON DSL to direct ggplot2 faceted waffle chart; `data.json` converted to TSV. |
| `bar/waterfall` | `viz_bar_waterfall` | `templates/bar/waterfall` | Migrated from JSON DSL to direct ggplot2 waterfall chart; `data.json` converted to TSV. |
| `heatmap/basic` | `viz_heatmap_basic` | `templates/heatmap/basic` | Long-form tile heatmap. |
| `heatmap/cluster_basic` | `viz_heatmap_clustered` | `templates/heatmap/clustered` | Wide matrix clustered with base R ordering + ggplot tile rendering. |
| `heatmap/mantel` | `viz_heatmap_mantel` | `templates/heatmap/mantel` | Migrated from linkET JSON DSL to direct ggplot2 linked correlation heatmap; `data.json` converted to TSV. |
| `heatmap/mantel_size` | `viz_heatmap_mantel_size` | `templates/heatmap/mantel-size` | Migrated from ggcor/linkET JSON DSL to direct ggplot2 Mantel-style linked heatmap with target-target squares and anchor-target links sized by absolute correlation and colored by p-value bins; `data.json` converted to TSV. |
| `heatmap/shape` | `viz_heatmap_shape_overlay` | `templates/heatmap/shape-overlay` | Migrated from linkET JSON DSL to direct ggplot2 correlation heatmap with shape markers; `data.json` converted to TSV. |
| `heatmap/signif` | `viz_heatmap_signif` | `templates/heatmap/signif` | Migrated from ggcor JSON DSL to direct ggplot2 significance-marked correlation heatmap with lower tiles, upper circles, stars, and weak-correlation crosses; `data.json` converted to TSV. |
| `heatmap/two` | `viz_heatmap_split` | `templates/heatmap/split` | Migrated from ggcor JSON DSL to direct ggplot2 two-set correlation heatmap with anchor-target tiles, significance stars, and non-significant crosses; `data.json` converted to TSV. |
| `heatmap/two_shape` | `viz_heatmap_split_shape` | `templates/heatmap/split-shape` | Migrated from ggcor JSON DSL to direct ggplot2 two-group correlation heatmap with lower-triangle rings and upper-triangle stars; `data.json` converted to TSV. |
| `line/group` | `viz_line_group` | `templates/line/group` | Grouped line plot. |

## Later optional candidates

- `scatter/pca_scores`
- `scatter/embedding`
- `scatter/quadrant`
- `scatter/bubble`
- `line/gsea_curve`
- `line/time_series`
- `heatmap/correlation`

## Excluded from core migration

- `references/omicsagent-*`
- `scripts/audit_omicsagent_visual_coverage.R`
- handwritten `templates/catalog.csv` as a source of truth
- generated gallery PNGs
- template-local output directories

## Dependency policy

The first-phase templates use `ggplot2` plus base R only. Missing packages are reported by helper code; templates do not auto-install dependencies.
