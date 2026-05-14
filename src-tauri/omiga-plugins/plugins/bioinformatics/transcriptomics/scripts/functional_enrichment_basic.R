#!/usr/bin/env Rscript
script_path <- sub("^--file=", "", commandArgs(trailingOnly = FALSE)[grep("^--file=", commandArgs(trailingOnly = FALSE))[1]][[1]])
source(file.path(dirname(normalizePath(script_path)), "omics_common.R"))

values <- args_named(
  required = c("genes", "gene_sets", "outdir"),
  optional = list(
    gene_sets_format = "auto",
    min_size = "5",
    max_size = "500",
    pvalue_threshold = "0.05",
    analysis_mode = "ora",
    gene_column = "auto",
    score_column = "auto",
    display_top_n = "20",
    plot_style = "auto",
    gsea_weight = "1"
  )
)
outdir <- ensure_outdir(values$outdir)

require_bioc_package <- function(pkg, mode) {
  if (!requireNamespace(pkg, quietly = TRUE)) {
    stop(sprintf(
      "%s enrichment requires the Bioconductor R package '%s'. Install it into the shared Omiga analysis environment with: if (!requireNamespace('BiocManager', quietly=TRUE)) install.packages('BiocManager'); BiocManager::install('%s')",
      toupper(mode), pkg, pkg
    ), call. = FALSE)
  }
}

require_r_package <- function(pkg, purpose) {
  if (!requireNamespace(pkg, quietly = TRUE)) {
    stop(sprintf(
      "%s requires the R package '%s' in the shared Omiga analysis environment.",
      purpose, pkg
    ), call. = FALSE)
  }
}

read_gene_vector <- function(path) {
  lines <- readLines(path, warn = FALSE)
  lines <- trimws(lines)
  lines <- lines[nzchar(lines) & !grepl("^#", lines)]
  genes <- unique(vapply(strsplit(lines, "[\t, ]+"), `[`, character(1), 1))
  if (length(genes) && tolower(genes[[1]]) %in% c("gene", "genes", "symbol", "id", "feature")) genes <- genes[-1]
  unique(toupper(genes[nzchar(genes)]))
}

read_ranked_genes <- function(path, delimiter = "auto", gene_column = "auto", score_column = "auto") {
  tab <- read_table_file(path, delimiter, header = TRUE)
  if (nrow(tab) == 0) stop("ranked gene table is empty", call. = FALSE)
  gene_col <- if (gene_column != "auto" && gene_column %in% colnames(tab)) gene_column else first_existing_col(tab, c("gene", "Gene", "symbol", "feature", "id"), colnames(tab)[1])
  score_candidates <- c("score", "stat", "rank", "log2_fold_change", "log2FoldChange", "log2FC", "NES")
  score_col <- if (score_column != "auto" && score_column %in% colnames(tab)) score_column else first_existing_col(tab, score_candidates, if (ncol(tab) >= 2) colnames(tab)[2] else colnames(tab)[1])
  scores <- suppressWarnings(as.numeric(tab[[score_col]]))
  if (all(is.na(scores))) stop("ranked GSEA input requires a numeric score/stat/log2FC column", call. = FALSE)
  ranked <- data.frame(gene = toupper(as.character(tab[[gene_col]])), score = scores, stringsAsFactors = FALSE)
  ranked <- ranked[nzchar(ranked$gene) & is.finite(ranked$score), , drop = FALSE]
  ranked <- ranked[!duplicated(ranked$gene), , drop = FALSE]
  ranked <- ranked[order(ranked$score, decreasing = TRUE), , drop = FALSE]
  ranked
}

read_gene_sets <- function(path, format) {
  format <- tolower(format)
  first <- readLines(path, n = 1, warn = FALSE)
  if (format == "auto") format <- if (length(first) && length(strsplit(first, "\t", fixed = TRUE)[[1]]) > 2) "gmt" else "tsv"
  sets <- list()
  if (format == "gmt") {
    for (line in readLines(path, warn = FALSE)) {
      parts <- strsplit(line, "\t", fixed = TRUE)[[1]]
      if (length(parts) >= 3) sets[[parts[[1]]]] <- unique(parts[-c(1, 2)])
    }
  } else {
    tab <- read_table_file(path, "auto", header = TRUE)
    if (ncol(tab) < 2) stop("gene set TSV requires at least two columns", call. = FALSE)
    term_col <- if ("term" %in% colnames(tab)) "term" else colnames(tab)[1]
    gene_col <- if ("gene" %in% colnames(tab)) "gene" else colnames(tab)[2]
    for (term in unique(tab[[term_col]])) sets[[as.character(term)]] <- unique(as.character(tab[[gene_col]][tab[[term_col]] == term]))
  }
  sets <- lapply(sets, function(x) {
    genes <- trimws(as.character(x))
    genes <- genes[!is.na(genes) & nzchar(genes)]
    unique(toupper(genes))
  })
  sets[lengths(sets) > 0]
}

sets_to_term2gene <- function(sets) {
  data.frame(
    term = rep(names(sets), lengths(sets)),
    gene = unlist(sets, use.names = FALSE),
    stringsAsFactors = FALSE
  )
}

empty_result <- function(mode) {
  if (mode == "gsea") {
    return(data.frame(term = character(), termId = character(), setSize = integer(), overlapSize = integer(), pvalue = numeric(), padj = numeric(), qvalue = numeric(), enrichmentScore = numeric(), NES = numeric(), leadingEdge = character(), direction = character(), analysisMode = character(), method = character(), stringsAsFactors = FALSE))
  }
  data.frame(term = character(), termId = character(), setSize = integer(), overlapSize = integer(), querySize = integer(), universeSize = integer(), pvalue = numeric(), padj = numeric(), qvalue = numeric(), overlapGenes = character(), geneRatio = numeric(), negLog10Padj = numeric(), analysisMode = character(), method = character(), stringsAsFactors = FALSE)
}

parse_ratio_parts <- function(value) {
  parts <- strsplit(as.character(value), "/", fixed = TRUE)[[1]]
  nums <- suppressWarnings(as.numeric(parts))
  if (length(nums) < 2 || any(is.na(nums[1:2]))) return(c(NA_real_, NA_real_))
  nums[1:2]
}

clusterprofiler_ora <- function(sets) {
  require_bioc_package("clusterProfiler", "ora")
  query <- read_gene_vector(values$genes)
  if (!length(query)) stop("gene list is empty", call. = FALSE)
  term2gene <- sets_to_term2gene(sets)
  universe <- unique(term2gene$gene)
  query <- intersect(query, universe)
  if (!length(query)) return(empty_result("ora"))

  enrich <- clusterProfiler::enricher(
    gene = query,
    pvalueCutoff = 1,
    pAdjustMethod = "BH",
    universe = universe,
    minGSSize = min_size,
    maxGSSize = max_size,
    qvalueCutoff = 1,
    TERM2GENE = term2gene
  )
  if (is.null(enrich)) return(empty_result("ora"))
  raw <- as.data.frame(enrich)
  if (!nrow(raw)) return(empty_result("ora"))

  gene_ratio <- t(vapply(raw$GeneRatio, parse_ratio_parts, numeric(2)))
  bg_ratio <- t(vapply(raw$BgRatio, parse_ratio_parts, numeric(2)))
  res <- data.frame(
    term = if ("Description" %in% colnames(raw)) raw$Description else raw$ID,
    termId = raw$ID,
    setSize = as.integer(bg_ratio[, 1]),
    overlapSize = as.integer(raw$Count),
    querySize = as.integer(gene_ratio[, 2]),
    universeSize = as.integer(bg_ratio[, 2]),
    pvalue = as.numeric(raw$pvalue),
    padj = as.numeric(raw$p.adjust),
    qvalue = if ("qvalue" %in% colnames(raw)) as.numeric(raw$qvalue) else NA_real_,
    overlapGenes = gsub("/", ",", raw$geneID, fixed = TRUE),
    geneRatio = gene_ratio[, 1] / pmax(gene_ratio[, 2], 1),
    negLog10Padj = safe_neg_log10(raw$p.adjust),
    analysisMode = "ora",
    method = "clusterProfiler::enricher",
    stringsAsFactors = FALSE
  )
  res[order(res$padj, res$pvalue, decreasing = FALSE), ]
}

fgsea_ranked <- function(sets) {
  require_bioc_package("fgsea", "gsea")
  ranked <- read_ranked_genes(values$genes, "auto", values$gene_column, values$score_column)
  if (nrow(ranked) < 2) stop("ranked gene table requires at least two scored genes", call. = FALSE)
  stats <- ranked$score
  names(stats) <- ranked$gene
  stats <- sort(stats, decreasing = TRUE)
  weight <- max(0, parse_num(values$gsea_weight, 1))
  fgsea_fn_name <- if (exists("fgseaMultilevel", envir = asNamespace("fgsea"), mode = "function")) "fgseaMultilevel" else "fgsea"
  fgsea_fn <- getExportedValue("fgsea", fgsea_fn_name)
  raw <- as.data.frame(fgsea_fn(
    pathways = sets,
    stats = stats,
    minSize = min_size,
    maxSize = max_size,
    gseaParam = weight
  ))
  if (!nrow(raw)) return(empty_result("gsea"))
  leading <- if ("leadingEdge" %in% colnames(raw)) {
    vapply(raw$leadingEdge, function(x) paste(as.character(x), collapse = ","), character(1))
  } else {
    rep("", nrow(raw))
  }
  res <- data.frame(
    term = raw$pathway,
    termId = raw$pathway,
    setSize = as.integer(raw$size),
    overlapSize = as.integer(raw$size),
    pvalue = as.numeric(raw$pval),
    padj = as.numeric(raw$padj),
    qvalue = NA_real_,
    enrichmentScore = as.numeric(raw$ES),
    NES = as.numeric(raw$NES),
    leadingEdge = leading,
    direction = ifelse(as.numeric(raw$NES) >= 0, "up", "down"),
    analysisMode = "gsea",
    method = paste0("fgsea::", fgsea_fn_name),
    stringsAsFactors = FALSE
  )
  res[order(res$padj, -abs(res$NES), decreasing = FALSE), ]
}

min_size <- parse_int(values$min_size, 5)
max_size <- parse_int(values$max_size, 500)
p_thr <- parse_num(values$pvalue_threshold, 0.05)
display_top_n <- max(1, parse_int(values$display_top_n, 20))
mode <- tolower(values$analysis_mode)
if (!mode %in% c("ora", "gsea", "auto")) stop("analysis_mode must be ora, gsea, or auto", call. = FALSE)
sets <- read_gene_sets(values$gene_sets, values$gene_sets_format)
if (!length(sets)) stop("gene set file produced no sets", call. = FALSE)
if (mode == "auto") {
  mode <- if (ncol(read_table_file(values$genes, "auto", header = TRUE)) >= 2) "gsea" else "ora"
}

res <- if (mode == "gsea") fgsea_ranked(sets) else clusterprofiler_ora(sets)
top <- head(res, display_top_n)
sig <- res[res$padj <= p_thr, , drop = FALSE]
write_tsv(res, file.path(outdir, "enrichment-results.tsv"))
write_tsv(top, file.path(outdir, "enrichment-top.tsv"))

plot_ora_bar <- function(plot_df, path) {
  svg(path, width = 8.6, height = 5.4)
  plot_top <- head(plot_df[plot_df$overlapSize > 0, , drop = FALSE], display_top_n)
  if (nrow(plot_top) > 0) {
    scores <- plot_top$negLog10Padj
    names(scores) <- plot_top$term
    par(mar = c(5, 11, 3, 1))
    barplot(rev(scores), horiz = TRUE, las = 1, col = "#0F766E", xlab = "-log10 adjusted p-value", main = "Functional enrichment ORA (clusterProfiler)")
  } else {
    plot.new(); text(0.5, 0.5, "No enriched terms returned by clusterProfiler")
  }
  invisible(dev.off())
}

plot_ora_dot <- function(plot_df, path) {
  svg(path, width = 8.6, height = 5.4)
  plot_top <- head(plot_df[plot_df$overlapSize > 0, , drop = FALSE], display_top_n)
  if (nrow(plot_top) > 0) {
    plot_top <- plot_top[rev(seq_len(nrow(plot_top))), , drop = FALSE]
    size <- 1.4 + 5 * plot_top$geneRatio / max(plot_top$geneRatio)
    cols <- colorRampPalette(c("#DBEAFE", "#2563EB", "#7F1D1D"))(100)
    z <- plot_top$negLog10Padj
    col_idx <- pmax(1, pmin(100, round(1 + 99 * (z - min(z)) / max(max(z) - min(z), 1e-9))))
    par(mar = c(5, 12, 3, 1))
    plot(z, seq_len(nrow(plot_top)), pch = 21, bg = cols[col_idx], col = "#1F2937", cex = size,
      yaxt = "n", xlab = "-log10 adjusted p-value", ylab = "", main = "Functional enrichment dot plot")
    axis(2, at = seq_len(nrow(plot_top)), labels = plot_top$term, las = 1, cex.axis = 0.78)
    grid(nx = NA, ny = NULL, col = "#E5E7EB")
  } else {
    plot.new(); text(0.5, 0.5, "No enriched terms returned by clusterProfiler")
  }
  invisible(dev.off())
}

plot_gsea_bar <- function(plot_df, path) {
  svg(path, width = 8.6, height = 5.4)
  plot_top <- head(plot_df[order(abs(plot_df$NES), decreasing = TRUE), , drop = FALSE], display_top_n)
  if (nrow(plot_top) > 0) {
    scores <- plot_top$NES
    names(scores) <- plot_top$term
    par(mar = c(5, 11, 3, 1))
    barplot(rev(scores), horiz = TRUE, las = 1, col = ifelse(rev(scores) >= 0, "#DC2626", "#2563EB"), xlab = "Normalized enrichment score", main = "GSEA (fgsea)")
    abline(v = 0, col = "#111827")
  } else {
    plot.new(); text(0.5, 0.5, "No pathways returned by fgsea")
  }
  invisible(dev.off())
}

plot_gsea_dot <- function(plot_df, path) {
  svg(path, width = 8.6, height = 5.4)
  plot_top <- head(plot_df[order(abs(plot_df$NES), decreasing = TRUE), , drop = FALSE], display_top_n)
  if (nrow(plot_top) > 0) {
    plot_top <- plot_top[rev(seq_len(nrow(plot_top))), , drop = FALSE]
    size <- 1.2 + 5 * plot_top$setSize / max(plot_top$setSize)
    col <- ifelse(plot_top$NES >= 0, "#DC2626", "#2563EB")
    par(mar = c(5, 12, 3, 1))
    plot(plot_top$NES, seq_len(nrow(plot_top)), pch = 21, bg = col, col = "#1F2937", cex = size,
      yaxt = "n", xlab = "Normalized enrichment score", ylab = "", main = "GSEA dot plot (fgsea)")
    axis(2, at = seq_len(nrow(plot_top)), labels = plot_top$term, las = 1, cex.axis = 0.78)
    abline(v = 0, col = "#6B7280", lty = 2)
  } else {
    plot.new(); text(0.5, 0.5, "No pathways returned by fgsea")
  }
  invisible(dev.off())
}

plot_gsea_curve <- function(plot_df, path) {
  svg(path, width = 8.2, height = 5.2)
  if (!nrow(plot_df)) {
    plot.new(); text(0.5, 0.5, "No GSEA terms")
  } else {
    ranked <- read_ranked_genes(values$genes, "auto", values$gene_column, values$score_column)
    stats <- ranked$score
    names(stats) <- ranked$gene
    stats <- sort(stats, decreasing = TRUE)
    term <- plot_df$termId[[1]]
    print(fgsea::plotEnrichment(sets[[term]], stats) + ggplot2::ggtitle(paste("GSEA curve:", term)))
  }
  invisible(dev.off())
}

if (mode == "gsea") {
  require_r_package("ggplot2", "GSEA enrichment curve plotting")
  plot_gsea_bar(res, file.path(outdir, "enrichment-barplot.svg"))
  plot_gsea_dot(res, file.path(outdir, "enrichment-dotplot.svg"))
  plot_gsea_curve(res, file.path(outdir, "enrichment-gsea-curve.svg"))
  default_plot <- if (tolower(values$plot_style) == "bar") "enrichment-barplot.svg" else if (tolower(values$plot_style) == "dot") "enrichment-dotplot.svg" else "enrichment-gsea-curve.svg"
} else {
  plot_ora_bar(res, file.path(outdir, "enrichment-barplot.svg"))
  plot_ora_dot(res, file.path(outdir, "enrichment-dotplot.svg"))
  default_plot <- if (tolower(values$plot_style) == "bar") "enrichment-barplot.svg" else "enrichment-dotplot.svg"
}
invisible(file.copy(file.path(outdir, default_plot), file.path(outdir, "enrichment-plot.svg"), overwrite = TRUE))

method_name <- if (mode == "gsea") {
  methods <- unique(as.character(res$method))
  if (length(methods) && nzchar(methods[[1]])) methods[[1]] else "fgsea"
} else {
  "clusterProfiler::enricher"
}

write_outputs_json(outdir, list(
  analysisMode = mode,
  method = method_name,
  queryGenes = if (mode == "ora") length(read_gene_vector(values$genes)) else nrow(read_ranked_genes(values$genes, "auto", values$gene_column, values$score_column)),
  geneSetsTested = nrow(res),
  significant = nrow(sig),
  displayTopN = display_top_n,
  results = "enrichment-results.tsv",
  topTable = "enrichment-top.tsv",
  plot = "enrichment-plot.svg",
  barplot = "enrichment-barplot.svg",
  dotplot = "enrichment-dotplot.svg",
  gseaCurve = if (mode == "gsea") "enrichment-gsea-curve.svg" else ""
))
cat(sprintf("Enrichment complete: %s via %s, %d sets, %d significant\n", mode, method_name, nrow(res), nrow(sig)))
