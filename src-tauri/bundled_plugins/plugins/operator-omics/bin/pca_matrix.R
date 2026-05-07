#!/usr/bin/env Rscript
script_path <- sub("^--file=", "", commandArgs(trailingOnly = FALSE)[grep("^--file=", commandArgs(trailingOnly = FALSE))[1]][[1]])
source(file.path(dirname(normalizePath(script_path)), "omics_common.R"))

values <- args_named(
  required = c("matrix", "outdir"),
  optional = list(delimiter = "auto", row_names = "true", features_by_rows = "true", center = "true", scale = "true", top_variable_features = "5000")
)
outdir <- ensure_outdir(values$outdir)
mat <- read_matrix_file(values$matrix, values$delimiter, parse_bool(values$row_names, TRUE))
features_by_rows <- parse_bool(values$features_by_rows, TRUE)
if (features_by_rows) {
  feature_names <- rownames(mat)
  sample_names <- colnames(mat)
  feature_matrix <- mat
  pca_input <- t(mat)
} else {
  sample_names <- rownames(mat)
  feature_names <- colnames(mat)
  feature_matrix <- t(mat)
  pca_input <- mat
}
if (nrow(pca_input) < 2 || ncol(pca_input) < 2) stop("PCA requires at least two samples and two features", call. = FALSE)
vars <- apply(feature_matrix, 1, var)
vars[is.na(vars)] <- 0
top_n <- min(max(parse_int(values$top_variable_features, 5000), 2), nrow(feature_matrix))
keep <- order(vars, decreasing = TRUE)[seq_len(top_n)]
pca_input <- if (features_by_rows) t(feature_matrix[keep, , drop = FALSE]) else t(feature_matrix[keep, , drop = FALSE])
zero_var <- apply(pca_input, 2, var) == 0
if (all(zero_var)) stop("all selected features have zero variance", call. = FALSE)
pca_input <- pca_input[, !zero_var, drop = FALSE]
fit <- prcomp(pca_input, center = parse_bool(values$center, TRUE), scale. = parse_bool(values$scale, TRUE))
variance <- fit$sdev^2
variance_fraction <- variance / sum(variance)

scores <- data.frame(sample = rownames(fit$x), fit$x, check.names = FALSE)
loadings <- data.frame(feature = colnames(pca_input), fit$rotation, check.names = FALSE)
variance_df <- data.frame(component = paste0("PC", seq_along(variance)), variance = variance, varianceFraction = variance_fraction)
write_tsv(scores, file.path(outdir, "pca-scores.tsv"))
write_tsv(loadings, file.path(outdir, "pca-loadings.tsv"))
write_tsv(variance_df, file.path(outdir, "pca-variance.tsv"))

svg(file.path(outdir, "pca-plot.svg"), width = 7, height = 5)
plot(fit$x[, 1], fit$x[, 2], xlab = sprintf("PC1 (%.1f%%)", 100 * variance_fraction[1]), ylab = sprintf("PC2 (%.1f%%)", 100 * variance_fraction[2]), pch = 19, col = "#2563EB", main = "PCA")
text(fit$x[, 1], fit$x[, 2], labels = rownames(fit$x), pos = 3, cex = 0.7)
dev.off()

write_outputs_json(outdir, list(
  samples = nrow(fit$x),
  featuresUsed = ncol(pca_input),
  pc1VarianceFraction = variance_fraction[1],
  pc2VarianceFraction = variance_fraction[2],
  scores = "pca-scores.tsv",
  loadings = "pca-loadings.tsv",
  plot = "pca-plot.svg"
))
cat(sprintf("PCA complete: %d samples, %d features\n", nrow(fit$x), ncol(pca_input)))
