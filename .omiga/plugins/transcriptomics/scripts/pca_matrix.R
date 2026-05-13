#!/usr/bin/env Rscript
script_path <- sub("^--file=", "", commandArgs(trailingOnly = FALSE)[grep("^--file=", commandArgs(trailingOnly = FALSE))[1]][[1]])
source(file.path(dirname(normalizePath(script_path)), "omics_common.R"))

values <- args_named(
  required = c("matrix", "outdir"),
  optional = list(
    delimiter = "auto",
    row_names = "true",
    features_by_rows = "true",
    center = "true",
    scale = "true",
    top_variable_features = "5000",
    metadata = "",
    sample_column = "sample",
    group_column = "group",
    plot_labels = "true",
    confidence_hulls = "true"
  )
)
outdir <- ensure_outdir(values$outdir)
mat <- read_matrix_file(values$matrix, values$delimiter, parse_bool(values$row_names, TRUE))
features_by_rows <- parse_bool(values$features_by_rows, TRUE)
if (features_by_rows) {
  feature_matrix <- mat
} else {
  feature_matrix <- t(mat)
}
sample_names <- colnames(feature_matrix)
feature_names <- rownames(feature_matrix)
if (length(sample_names) < 2 || length(feature_names) < 2) stop("PCA requires at least two samples and two features", call. = FALSE)

vars <- apply(feature_matrix, 1, var)
vars[is.na(vars)] <- 0
top_n <- min(max(parse_int(values$top_variable_features, 5000), 2), nrow(feature_matrix))
keep <- order(vars, decreasing = TRUE)[seq_len(top_n)]
pca_input <- t(feature_matrix[keep, , drop = FALSE])
zero_var <- apply(pca_input, 2, var) == 0
if (all(zero_var)) stop("all selected features have zero variance", call. = FALSE)
pca_input <- pca_input[, !zero_var, drop = FALSE]
fit <- prcomp(pca_input, center = parse_bool(values$center, TRUE), scale. = parse_bool(values$scale, TRUE))
variance <- fit$sdev^2
variance_fraction <- if (sum(variance) > 0) variance / sum(variance) else variance
metadata <- read_sample_metadata(values$metadata, rownames(fit$x), values$delimiter, values$sample_column, values$group_column)
metadata <- metadata[match(rownames(fit$x), metadata$sample), , drop = FALSE]
metadata$group[is.na(metadata$group) | !nzchar(metadata$group)] <- "Unmatched"

scores <- data.frame(sample = rownames(fit$x), group = metadata$group, fit$x, check.names = FALSE)
loadings <- data.frame(feature = colnames(pca_input), fit$rotation, check.names = FALSE)
variance_df <- data.frame(component = paste0("PC", seq_along(variance)), variance = variance, varianceFraction = variance_fraction)
group_summary <- aggregate(sample ~ group, data = scores, FUN = length)
colnames(group_summary) <- c("group", "sampleCount")
write_tsv(scores, file.path(outdir, "pca-scores.tsv"))
write_tsv(loadings, file.path(outdir, "pca-loadings.tsv"))
write_tsv(variance_df, file.path(outdir, "pca-variance.tsv"))
write_tsv(group_summary, file.path(outdir, "pca-group-summary.tsv"))

pc_percent <- function(i) {
  if (length(variance_fraction) < i || is.na(variance_fraction[[i]])) return("0.0%")
  sprintf("%.1f%%", 100 * variance_fraction[[i]])
}

svg(file.path(outdir, "pca-plot.svg"), width = 7.5, height = 5.6)
groups <- unique(scores$group)
pal <- setNames(operator_palette(length(groups)), groups)
plot(scores$PC1, scores$PC2,
  xlab = sprintf("PC1 (%s)", pc_percent(1)),
  ylab = sprintf("PC2 (%s)", pc_percent(2)),
  pch = 21,
  bg = pal[scores$group],
  col = "#1F2937",
  cex = 1.35,
  main = "PCA sample overview"
)
abline(h = 0, v = 0, col = "#CBD5E1", lty = 2)
if (parse_bool(values$confidence_hulls, TRUE)) {
  for (group in groups) {
    idx <- which(scores$group == group)
    if (length(idx) >= 3) {
      hull <- chull(scores$PC1[idx], scores$PC2[idx])
      polygon(scores$PC1[idx][hull], scores$PC2[idx][hull], border = pal[[group]], col = adjustcolor(pal[[group]], alpha.f = 0.16))
      points(scores$PC1[idx], scores$PC2[idx], pch = 21, bg = pal[[group]], col = "#1F2937", cex = 1.35)
    }
  }
}
if (parse_bool(values$plot_labels, TRUE)) text(scores$PC1, scores$PC2, labels = scores$sample, pos = 3, cex = 0.68)
legend("topright", legend = groups, pt.bg = pal[groups], pch = 21, col = "#1F2937", bty = "n", title = "Group", cex = 0.8)
invisible(dev.off())

svg(file.path(outdir, "pca-scree.svg"), width = 7, height = 4.6)
barplot(100 * head(variance_fraction, 12), names.arg = head(variance_df$component, 12), col = "#2563EB", border = NA,
  ylab = "Explained variance (%)", xlab = "Principal component", main = "PCA variance explained")
invisible(dev.off())

write_outputs_json(outdir, list(
  samples = nrow(fit$x),
  groups = length(groups),
  featuresUsed = ncol(pca_input),
  pc1VarianceFraction = if (length(variance_fraction) >= 1) variance_fraction[1] else 0,
  pc2VarianceFraction = if (length(variance_fraction) >= 2) variance_fraction[2] else 0,
  scores = "pca-scores.tsv",
  loadings = "pca-loadings.tsv",
  variance = "pca-variance.tsv",
  plot = "pca-plot.svg",
  screePlot = "pca-scree.svg"
))
cat(sprintf("PCA complete: %d samples, %d groups, %d features\n", nrow(fit$x), length(groups), ncol(pca_input)))
