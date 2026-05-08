#!/usr/bin/env Rscript
script_path <- sub("^--file=", "", commandArgs(trailingOnly = FALSE)[grep("^--file=", commandArgs(trailingOnly = FALSE))[1]][[1]])
source(file.path(dirname(normalizePath(script_path)), "omics_common.R"))

values <- args_named(
  required = c("matrix", "metadata", "outdir"),
  optional = list(
    group_column = "group",
    case_group = "",
    control_group = "",
    sample_column = "sample",
    delimiter = "auto",
    row_names = "true",
    pseudocount = "1",
    log2fc_threshold = "1",
    pvalue_threshold = "0.05",
    comparisons = "",
    stat_test = "welch",
    input_data_type = "auto",
    transform_counts = "true",
    de_method = "auto"
  )
)

outdir <- ensure_outdir(values$outdir)
mat <- read_matrix_file(values$matrix, values$delimiter, parse_bool(values$row_names, TRUE))
metadata <- read_sample_metadata(values$metadata, colnames(mat), values$delimiter, values$sample_column, values$group_column)
metadata <- metadata[metadata$group != "Unmatched", , drop = FALSE]
unique_groups <- unique(as.character(metadata$group))
if (length(unique_groups) < 2) stop("differential expression requires at least two detected groups", call. = FALSE)

requested_pairs <- parse_comparisons(values$comparisons)
if (!length(requested_pairs) && nzchar(values$case_group) && nzchar(values$control_group)) {
  requested_pairs <- list(c(values$case_group, values$control_group))
}
if (!length(requested_pairs)) requested_pairs <- build_default_pairs(unique_groups)
valid_pairs <- list()
for (pair in requested_pairs) {
  if (pair[[1]] %in% unique_groups && pair[[2]] %in% unique_groups) valid_pairs[[length(valid_pairs) + 1]] <- pair
}
if (!length(valid_pairs)) stop("at least one valid comparison is required", call. = FALSE)

pseudocount <- parse_num(values$pseudocount, 1)
lfc_thr <- parse_num(values$log2fc_threshold, 1)
p_thr <- parse_num(values$pvalue_threshold, 0.05)

normalize_method <- function(value) {
  value <- tolower(trimws(as.character(value)))
  aliases <- c(
    deseq = "deseq2",
    edge = "edger",
    edge_r = "edger",
    t = "t_test",
    ttest = "t_test",
    student = "t_test",
    student_t = "t_test",
    wilcox = "wilcoxon",
    mannwhitney = "wilcoxon",
    mann_whitney = "wilcoxon",
    chisq = "chi_square",
    chisquare = "chi_square",
    chi2 = "chi_square"
  )
  if (value %in% names(aliases)) value <- aliases[[value]]
  value
}

stat_test <- normalize_method(values$stat_test)
if (!stat_test %in% c("welch", "t_test", "wilcoxon", "chi_square")) {
  stop("stat_test must be one of welch, t_test, wilcoxon, or chi_square", call. = FALSE)
}
requested_method <- normalize_method(values$de_method)
if (!requested_method %in% c("auto", "deseq2", "edger", "limma", "welch", "t_test", "wilcoxon", "chi_square")) {
  stop("de_method must be one of auto, deseq2, edger, limma, welch, t_test, wilcoxon, chi_square", call. = FALSE)
}

input_type <- tolower(values$input_data_type)
if (!input_type %in% c("auto", "counts", "quantitative")) stop("input_data_type must be auto, counts, or quantitative", call. = FALSE)
is_integerish_counts <- all(is.finite(mat)) && all(mat >= 0) && all(abs(mat - round(mat)) < 1e-8)
if (input_type == "auto") input_type <- if (is_integerish_counts) "counts" else "quantitative"
if (input_type == "counts" && !is_integerish_counts) stop("count-based methods require non-negative integer count matrices", call. = FALSE)

package_available <- function(method) {
  switch(method,
    deseq2 = requireNamespace("DESeq2", quietly = TRUE),
    edger = requireNamespace("edgeR", quietly = TRUE),
    limma = requireNamespace("limma", quietly = TRUE),
    TRUE
  )
}

require_method_package <- function(method) {
  pkg <- switch(method, deseq2 = "DESeq2", edger = "edgeR", limma = "limma", NA_character_)
  if (!is.na(pkg) && !requireNamespace(pkg, quietly = TRUE)) {
    stop(sprintf(
      "Differential expression method '%s' requires Bioconductor package '%s' in the shared Omiga analysis environment. Install with: if (!requireNamespace('BiocManager', quietly=TRUE)) install.packages('BiocManager'); BiocManager::install('%s')",
      method, pkg, pkg
    ), call. = FALSE)
  }
}

method_candidates <- function() {
  if (requested_method != "auto") return(requested_method)
  if (input_type == "counts") return(c("deseq2", "edger", "limma", stat_test))
  c("limma", stat_test)
}

analysis_matrix_for_simple <- function() {
  if (input_type == "counts" && parse_bool(values$transform_counts, TRUE)) return(log2(mat + pseudocount))
  mat
}

safe_pvalue <- function(a, b, method) {
  a <- as.numeric(a); b <- as.numeric(b)
  a <- a[is.finite(a)]; b <- b[is.finite(b)]
  if (length(a) < 2 || length(b) < 2) return(1)
  if (length(a) == length(b) && isTRUE(all.equal(a, b, tolerance = 1e-12))) return(1)
  p <- tryCatch({
    if (method == "wilcoxon") {
      wilcox.test(a, b, exact = FALSE)$p.value
    } else if (method == "t_test") {
      t.test(a, b, var.equal = TRUE)$p.value
    } else if (method == "chi_square") {
      threshold <- median(c(a, b), na.rm = TRUE)
      tab <- rbind(
        A = c(sum(a > threshold), sum(a <= threshold)),
        B = c(sum(b > threshold), sum(b <= threshold))
      )
      if (any(rowSums(tab) == 0) || any(colSums(tab) == 0)) 1 else suppressWarnings(chisq.test(tab, correct = FALSE)$p.value)
    } else {
      t.test(a, b, var.equal = FALSE)$p.value
    }
  }, error = function(e) 1, warning = function(w) suppressWarnings({
    if (method == "wilcoxon") wilcox.test(a, b, exact = FALSE)$p.value else if (method == "t_test") t.test(a, b, var.equal = TRUE)$p.value else 1
  }))
  if (!is.finite(p)) 1 else max(0, min(1, p))
}

make_result_frame <- function(group_a, group_b, mean_a, mean_b, log2fc, pvalues, padj, method, extra = list()) {
  direction <- ifelse(padj <= p_thr & log2fc >= lfc_thr, "up", ifelse(padj <= p_thr & log2fc <= -lfc_thr, "down", "none"))
  frame <- data.frame(
    groupA = group_a,
    groupB = group_b,
    feature = rownames(mat),
    gene = rownames(mat),
    caseMean = mean_a,
    controlMean = mean_b,
    MeanA = mean_a,
    MeanB = mean_b,
    log2FoldChange = log2fc,
    log2_fold_change = log2fc,
    pvalue = pvalues,
    p_value = pvalues,
    padj = padj,
    adjusted_p_value = padj,
    direction = direction,
    regulation = direction,
    method = method,
    inputDataType = input_type,
    stringsAsFactors = FALSE
  )
  for (name in names(extra)) frame[[name]] <- extra[[name]]
  frame
}

finalize_pvalues <- function(pvalues) {
  pvalues <- as.numeric(pvalues)
  pvalues[!is.finite(pvalues) | is.na(pvalues)] <- 1
  padj <- p.adjust(pvalues, method = "BH")
  padj[!is.finite(padj) | is.na(padj)] <- 1
  list(pvalue = pvalues, padj = padj)
}

samples_for_pair <- function(group_a, group_b) {
  samples_a <- intersect(metadata$sample[metadata$group == group_a], colnames(mat))
  samples_b <- intersect(metadata$sample[metadata$group == group_b], colnames(mat))
  list(a = samples_a, b = samples_b)
}

run_simple_pair <- function(group_a, group_b, samples_a, samples_b, method) {
  analysis_mat <- analysis_matrix_for_simple()
  mean_a <- rowMeans(mat[, samples_a, drop = FALSE])
  mean_b <- rowMeans(mat[, samples_b, drop = FALSE])
  log2fc <- log2((mean_a + pseudocount) / (mean_b + pseudocount))
  pvalues <- vapply(seq_len(nrow(mat)), function(i) safe_pvalue(analysis_mat[i, samples_a], analysis_mat[i, samples_b], method), numeric(1))
  pv <- finalize_pvalues(pvalues)
  make_result_frame(group_a, group_b, mean_a, mean_b, log2fc, pv$pvalue, pv$padj, method)
}

run_deseq2_pair <- function(group_a, group_b, samples_a, samples_b) {
  require_method_package("deseq2")
  if (input_type != "counts") stop("DESeq2 requires input_data_type=counts or an auto-detected integer count matrix", call. = FALSE)
  if (length(samples_a) < 2 || length(samples_b) < 2) stop("DESeq2 requires at least two samples per group", call. = FALSE)
  suppressPackageStartupMessages(library(DESeq2))
  sample_order <- c(samples_a, samples_b)
  count_data <- round(mat[, sample_order, drop = FALSE])
  col_data <- data.frame(group = factor(c(rep(group_a, length(samples_a)), rep(group_b, length(samples_b))), levels = c(group_b, group_a)), row.names = sample_order)
  dds <- DESeq2::DESeqDataSetFromMatrix(countData = count_data, colData = col_data, design = ~ group)
  keep <- rowSums(DESeq2::counts(dds)) > 0
  dds <- dds[keep, ]
  dds <- tryCatch(
    DESeq2::DESeq(dds, quiet = TRUE, sfType = "poscounts"),
    error = function(e) {
      dds_gene <- DESeq2::estimateSizeFactors(dds, type = "poscounts")
      dds_gene <- DESeq2::estimateDispersionsGeneEst(dds_gene, quiet = TRUE)
      dispersion <- S4Vectors::mcols(dds_gene)$dispGeneEst
      fallback <- median(dispersion[is.finite(dispersion) & !is.na(dispersion)], na.rm = TRUE)
      if (!is.finite(fallback) || is.na(fallback)) fallback <- 0.1
      dispersion[!is.finite(dispersion) | is.na(dispersion)] <- fallback
      DESeq2::dispersions(dds_gene) <- pmax(dispersion, 1e-8)
      DESeq2::nbinomWaldTest(dds_gene, quiet = TRUE)
    }
  )
  res <- as.data.frame(DESeq2::results(dds, contrast = c("group", group_a, group_b), independentFiltering = FALSE))
  rownames(res) <- rownames(dds)
  pvalues <- rep(1, nrow(mat)); names(pvalues) <- rownames(mat)
  padj <- rep(1, nrow(mat)); names(padj) <- rownames(mat)
  lfc <- log2((rowMeans(mat[, samples_a, drop = FALSE]) + pseudocount) / (rowMeans(mat[, samples_b, drop = FALSE]) + pseudocount))
  base_mean <- rep(NA_real_, nrow(mat)); names(base_mean) <- rownames(mat)
  pvalues[rownames(res)] <- res$pvalue
  padj[rownames(res)] <- res$padj
  lfc[rownames(res)] <- res$log2FoldChange
  base_mean[rownames(res)] <- res$baseMean
  pv <- finalize_pvalues(pvalues)
  pv$padj <- padj
  pv$padj[!is.finite(pv$padj) | is.na(pv$padj)] <- 1
  pv$pvalue[!is.finite(pv$pvalue) | is.na(pv$pvalue)] <- 1
  lfc[!is.finite(lfc) | is.na(lfc)] <- 0
  mean_a <- rowMeans(mat[, samples_a, drop = FALSE])
  mean_b <- rowMeans(mat[, samples_b, drop = FALSE])
  make_result_frame(group_a, group_b, mean_a, mean_b, lfc, pv$pvalue, pv$padj, "DESeq2", list(baseMean = base_mean))
}

run_edger_pair <- function(group_a, group_b, samples_a, samples_b) {
  require_method_package("edger")
  if (input_type != "counts") stop("edgeR requires input_data_type=counts or an auto-detected integer count matrix", call. = FALSE)
  if (length(samples_a) < 2 || length(samples_b) < 2) stop("edgeR requires at least two samples per group", call. = FALSE)
  suppressPackageStartupMessages(library(edgeR))
  sample_order <- c(samples_a, samples_b)
  group <- factor(c(rep(group_a, length(samples_a)), rep(group_b, length(samples_b))), levels = c(group_b, group_a))
  y <- edgeR::DGEList(counts = round(mat[, sample_order, drop = FALSE]), group = group)
  keep <- rowSums(y$counts) > 0
  y <- y[keep, , keep.lib.sizes = FALSE]
  y <- edgeR::calcNormFactors(y)
  y <- edgeR::estimateDisp(y)
  test <- edgeR::exactTest(y, pair = c(group_b, group_a))
  tab <- edgeR::topTags(test, n = Inf, sort.by = "none")$table
  pvalues <- rep(1, nrow(mat)); names(pvalues) <- rownames(mat)
  padj <- rep(1, nrow(mat)); names(padj) <- rownames(mat)
  lfc <- log2((rowMeans(mat[, samples_a, drop = FALSE]) + pseudocount) / (rowMeans(mat[, samples_b, drop = FALSE]) + pseudocount))
  log_cpm <- rep(NA_real_, nrow(mat)); names(log_cpm) <- rownames(mat)
  pvalues[rownames(tab)] <- tab$PValue
  padj[rownames(tab)] <- tab$FDR
  lfc[rownames(tab)] <- tab$logFC
  log_cpm[rownames(tab)] <- tab$logCPM
  pv <- finalize_pvalues(pvalues); pv$padj <- padj; pv$padj[!is.finite(pv$padj) | is.na(pv$padj)] <- 1
  lfc[!is.finite(lfc) | is.na(lfc)] <- 0
  mean_a <- rowMeans(mat[, samples_a, drop = FALSE])
  mean_b <- rowMeans(mat[, samples_b, drop = FALSE])
  make_result_frame(group_a, group_b, mean_a, mean_b, lfc, pv$pvalue, pv$padj, "edgeR", list(logCPM = log_cpm))
}

run_limma_pair <- function(group_a, group_b, samples_a, samples_b) {
  require_method_package("limma")
  if (length(samples_a) < 2 || length(samples_b) < 2) stop("limma requires at least two samples per group", call. = FALSE)
  suppressPackageStartupMessages(library(limma))
  sample_order <- c(samples_a, samples_b)
  group <- factor(c(rep(group_a, length(samples_a)), rep(group_b, length(samples_b))))
  design <- model.matrix(~ 0 + group)
  level_names <- make.names(levels(group), unique = TRUE)
  colnames(design) <- level_names
  expr <- mat[, sample_order, drop = FALSE]
  if (input_type == "counts") {
    expr <- limma::voom(round(expr), design, plot = FALSE)$E
  }
  contrast <- paste0(make.names(group_a), "-", make.names(group_b))
  fit <- limma::lmFit(expr, design)
  fit <- limma::contrasts.fit(fit, limma::makeContrasts(contrasts = contrast, levels = design))
  fit <- limma::eBayes(fit)
  tab <- limma::topTable(fit, number = Inf, sort.by = "none")
  pvalues <- tab$P.Value
  padj <- tab$adj.P.Val
  lfc <- tab$logFC
  pvalues[!is.finite(pvalues) | is.na(pvalues)] <- 1
  padj[!is.finite(padj) | is.na(padj)] <- 1
  lfc[!is.finite(lfc) | is.na(lfc)] <- 0
  mean_a <- rowMeans(mat[, samples_a, drop = FALSE])
  mean_b <- rowMeans(mat[, samples_b, drop = FALSE])
  make_result_frame(group_a, group_b, mean_a, mean_b, lfc, pvalues, padj, if (input_type == "counts") "limma-voom" else "limma", list(t = tab$t, B = tab$B))
}

run_pair_with_method <- function(method, group_a, group_b, samples_a, samples_b) {
  if (method == "deseq2") return(run_deseq2_pair(group_a, group_b, samples_a, samples_b))
  if (method == "edger") return(run_edger_pair(group_a, group_b, samples_a, samples_b))
  if (method == "limma") return(run_limma_pair(group_a, group_b, samples_a, samples_b))
  run_simple_pair(group_a, group_b, samples_a, samples_b, method)
}

run_pair <- function(group_a, group_b, samples_a, samples_b) {
  candidates <- method_candidates()
  errors <- character()
  for (method in candidates) {
    if (requested_method == "auto" && !package_available(method)) {
      errors <- c(errors, sprintf("%s unavailable", method))
      next
    }
    result <- tryCatch(run_pair_with_method(method, group_a, group_b, samples_a, samples_b), error = function(e) e)
    if (!inherits(result, "error")) return(result)
    errors <- c(errors, sprintf("%s: %s", method, conditionMessage(result)))
    if (requested_method != "auto") stop(conditionMessage(result), call. = FALSE)
  }
  stop(sprintf("No differential-expression method succeeded for %s vs %s: %s", group_a, group_b, paste(errors, collapse = "; ")), call. = FALSE)
}

all_results <- list()
for (pair_index in seq_along(valid_pairs)) {
  pair <- valid_pairs[[pair_index]]
  group_a <- pair[[1]]
  group_b <- pair[[2]]
  samples <- samples_for_pair(group_a, group_b)
  if (!length(samples$a) || !length(samples$b)) next
  pair_df <- run_pair(group_a, group_b, samples$a, samples$b)
  pair_df$comparison_index <- pair_index
  pair_df$comparison <- paste(group_a, "vs", group_b)
  all_results[[length(all_results) + 1]] <- pair_df
}
if (!length(all_results)) stop("no comparisons had matched samples", call. = FALSE)
results <- do.call(rbind, all_results)
results$absLog2FoldChange <- abs(results$log2FoldChange)
results <- results[order(results$comparison_index, results$padj, results$pvalue, -results$absLog2FoldChange, na.last = TRUE), ]
sig <- results[results$direction != "none", , drop = FALSE]
write_tsv(results[, setdiff(colnames(results), c("comparison_index", "absLog2FoldChange")), drop = FALSE], file.path(outdir, "de-results.tsv"))
write_tsv(sig[, setdiff(colnames(sig), c("comparison_index", "absLog2FoldChange")), drop = FALSE], file.path(outdir, "de-significant.tsv"))

plot_volcano <- function(df, path) {
  y <- safe_neg_log10(df$padj)
  cols <- ifelse(df$direction == "up", "#DC2626", ifelse(df$direction == "down", "#2563EB", "#9CA3AF"))
  svg(path, width = 7.5, height = 5.5)
  plot(df$log2FoldChange, y, pch = 19, cex = 0.62, col = cols,
    xlab = "log2 fold-change", ylab = "-log10 adjusted p-value", main = paste("Differential expression:", unique(df$comparison)[[1]]))
  abline(v = c(-lfc_thr, lfc_thr), lty = 2, col = "#6B7280")
  abline(h = -log10(max(p_thr, .Machine$double.xmin)), lty = 2, col = "#6B7280")
  top <- head(df[order(df$padj, -abs(df$log2FoldChange)), , drop = FALSE], 8)
  if (nrow(top)) text(top$log2FoldChange, safe_neg_log10(top$padj), labels = top$gene, pos = 3, cex = 0.62)
  legend("topright", legend = c("Up", "Down", "Not significant"), col = c("#DC2626", "#2563EB", "#9CA3AF"), pch = 19, bty = "n", cex = 0.8)
  invisible(dev.off())
}

plot_quadrant <- function(df, comparisons, path) {
  wide <- reshape(df[, c("gene", "comparison", "log2FoldChange")], idvar = "gene", timevar = "comparison", direction = "wide")
  xcol <- paste0("log2FoldChange.", comparisons[[1]])
  ycol <- paste0("log2FoldChange.", comparisons[[2]])
  if (!all(c(xcol, ycol) %in% colnames(wide))) return(plot_empty_svg(path, "Differential-expression quadrant", "Insufficient paired comparison data"))
  x <- wide[[xcol]]; y <- wide[[ycol]]
  group <- ifelse(x <= -lfc_thr & y >= lfc_thr, "L-H", ifelse(x >= lfc_thr & y >= lfc_thr, "H-H", ifelse(x <= -lfc_thr & y <= -lfc_thr, "L-L", ifelse(x >= lfc_thr & y <= -lfc_thr, "H-L", "none"))))
  pal <- c("L-H" = "#3288BD", "H-H" = "#D53E4F", "L-L" = "#66C2A5", "H-L" = "#F46D43", "none" = "#9CA3AF")
  svg(path, width = 6.4, height = 6)
  plot(x, y, pch = 19, cex = 0.7, col = pal[group], xlab = comparisons[[1]], ylab = comparisons[[2]], main = "Two-comparison log2FC quadrant")
  abline(v = c(-lfc_thr, lfc_thr), h = c(-lfc_thr, lfc_thr), lty = 2, col = "#6B7280")
  top_idx <- head(order(abs(x) + abs(y), decreasing = TRUE), 8)
  text(x[top_idx], y[top_idx], labels = wide$gene[top_idx], pos = 3, cex = 0.62)
  legend("topright", legend = names(pal), col = pal, pch = 19, bty = "n", cex = 0.75)
  invisible(dev.off())
}

plot_beeswarm <- function(df, comparisons, path) {
  plot_df <- df[df$direction != "none", , drop = FALSE]
  if (!nrow(plot_df)) return(plot_empty_svg(path, "Multi-comparison differential expression", "No genes passed the current thresholds"))
  set.seed(1)
  xbase <- match(plot_df$comparison, comparisons)
  x <- xbase + runif(nrow(plot_df), -0.22, 0.22)
  cols <- ifelse(plot_df$direction == "up", "#DC2626", "#2563EB")
  svg(path, width = max(7, length(comparisons) * 1.45), height = 5.5)
  plot(x, plot_df$log2FoldChange, xaxt = "n", pch = 19, cex = 0.68, col = adjustcolor(cols, alpha.f = 0.72),
    xlab = "Comparison", ylab = "log2 fold-change", main = "Differential genes across comparisons")
  axis(1, at = seq_along(comparisons), labels = comparisons, las = 2, cex.axis = 0.72)
  abline(h = c(-lfc_thr, 0, lfc_thr), lty = c(2, 1, 2), col = c("#6B7280", "#CBD5E1", "#6B7280"))
  legend("topright", legend = c("Up", "Down"), col = c("#DC2626", "#2563EB"), pch = 19, bty = "n", cex = 0.8)
  invisible(dev.off())
}

comparison_order <- unique(results$comparison)
plot_volcano(results[results$comparison == comparison_order[[1]], , drop = FALSE], file.path(outdir, "de-volcano.svg"))
default_plot <- "de-volcano.svg"
if (length(comparison_order) == 2) {
  plot_quadrant(results, comparison_order, file.path(outdir, "de-quadrant.svg"))
  default_plot <- "de-quadrant.svg"
} else if (length(comparison_order) > 2) {
  plot_beeswarm(results, comparison_order, file.path(outdir, "de-beeswarm.svg"))
  default_plot <- "de-beeswarm.svg"
}
invisible(file.copy(file.path(outdir, default_plot), file.path(outdir, "de-plot.svg"), overwrite = TRUE))

methods_used <- paste(unique(results$method), collapse = ",")
write_outputs_json(outdir, list(
  featuresTested = length(unique(results$gene)),
  comparisons = length(comparison_order),
  significantRows = nrow(sig),
  up = sum(results$direction == "up"),
  down = sum(results$direction == "down"),
  inputDataType = input_type,
  method = methods_used,
  requestedMethod = requested_method,
  fallbackStatTest = stat_test,
  results = "de-results.tsv",
  significantTable = "de-significant.tsv",
  plot = "de-plot.svg",
  volcanoPlot = "de-volcano.svg",
  defaultPlotType = if (length(comparison_order) == 1) "volcano" else if (length(comparison_order) == 2) "quadrant" else "beeswarm"
))
cat(sprintf("DE complete: %d features, %d comparisons, methods=%s, %d significant rows\n", length(unique(results$gene)), length(comparison_order), methods_used, nrow(sig)))
