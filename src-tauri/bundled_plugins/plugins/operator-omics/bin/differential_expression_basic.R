#!/usr/bin/env Rscript
script_path <- sub("^--file=", "", commandArgs(trailingOnly = FALSE)[grep("^--file=", commandArgs(trailingOnly = FALSE))[1]][[1]])
source(file.path(dirname(normalizePath(script_path)), "omics_common.R"))

values <- args_named(
  required = c("matrix", "metadata", "outdir", "group_column", "case_group", "control_group"),
  optional = list(sample_column = "sample", delimiter = "auto", row_names = "true", pseudocount = "1", log2fc_threshold = "1", pvalue_threshold = "0.05")
)
outdir <- ensure_outdir(values$outdir)
mat <- read_matrix_file(values$matrix, values$delimiter, parse_bool(values$row_names, TRUE))
meta_sep <- choose_sep(values$metadata, values$delimiter)
meta <- read.table(values$metadata, header = TRUE, sep = meta_sep, quote = "", comment.char = "", check.names = FALSE, stringsAsFactors = FALSE)
if (!values$sample_column %in% colnames(meta)) stop(sprintf("metadata lacks sample column '%s'", values$sample_column), call. = FALSE)
if (!values$group_column %in% colnames(meta)) stop(sprintf("metadata lacks group column '%s'", values$group_column), call. = FALSE)
case_samples <- meta[[values$sample_column]][meta[[values$group_column]] == values$case_group]
control_samples <- meta[[values$sample_column]][meta[[values$group_column]] == values$control_group]
case_samples <- intersect(case_samples, colnames(mat))
control_samples <- intersect(control_samples, colnames(mat))
if (length(case_samples) < 2 || length(control_samples) < 2) stop("differential expression requires at least two samples per group after matching metadata to matrix columns", call. = FALSE)
pseudocount <- parse_num(values$pseudocount, 1)
case_mat <- mat[, case_samples, drop = FALSE]
control_mat <- mat[, control_samples, drop = FALSE]
case_mean <- rowMeans(case_mat)
control_mean <- rowMeans(control_mat)
log2fc <- log2((case_mean + pseudocount) / (control_mean + pseudocount))
pvalues <- apply(mat, 1, function(row) {
  res <- try(t.test(row[case_samples], row[control_samples]), silent = TRUE)
  if (inherits(res, "try-error")) return(NA_real_)
  res$p.value
})
padj <- p.adjust(pvalues, method = "BH")
results <- data.frame(
  feature = rownames(mat),
  caseMean = case_mean,
  controlMean = control_mean,
  log2FoldChange = log2fc,
  pvalue = pvalues,
  padj = padj,
  stringsAsFactors = FALSE
)
results <- results[order(results$padj, results$pvalue, decreasing = FALSE, na.last = TRUE), ]
lfc_thr <- parse_num(values$log2fc_threshold, 1)
p_thr <- parse_num(values$pvalue_threshold, 0.05)
results$direction <- ifelse(results$padj <= p_thr & results$log2FoldChange >= lfc_thr, "up", ifelse(results$padj <= p_thr & results$log2FoldChange <= -lfc_thr, "down", "none"))
sig <- results[results$direction != "none", , drop = FALSE]
write_tsv(results, file.path(outdir, "de-results.tsv"))
write_tsv(sig, file.path(outdir, "de-significant.tsv"))

svg(file.path(outdir, "de-volcano.svg"), width = 7, height = 5)
y <- -log10(pmax(results$padj, .Machine$double.xmin))
cols <- ifelse(results$direction == "up", "#DC2626", ifelse(results$direction == "down", "#2563EB", "#9CA3AF"))
plot(results$log2FoldChange, y, pch = 19, cex = 0.55, col = cols, xlab = "log2 fold-change", ylab = "-log10 adjusted p-value", main = "Differential expression")
abline(v = c(-lfc_thr, lfc_thr), lty = 2, col = "#6B7280")
abline(h = -log10(p_thr), lty = 2, col = "#6B7280")
dev.off()

write_outputs_json(outdir, list(
  featuresTested = nrow(results),
  significant = nrow(sig),
  up = sum(results$direction == "up"),
  down = sum(results$direction == "down"),
  caseSamples = length(case_samples),
  controlSamples = length(control_samples),
  results = "de-results.tsv",
  significantTable = "de-significant.tsv",
  plot = "de-volcano.svg"
))
cat(sprintf("DE complete: %d features, %d significant\n", nrow(results), nrow(sig)))
