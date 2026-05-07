#!/usr/bin/env Rscript
script_path <- sub("^--file=", "", commandArgs(trailingOnly = FALSE)[grep("^--file=", commandArgs(trailingOnly = FALSE))[1]][[1]])
source(file.path(dirname(normalizePath(script_path)), "omics_common.R"))

values <- args_named(
  required = c("genes", "gene_sets", "outdir"),
  optional = list(gene_sets_format = "auto", min_size = "5", max_size = "500", pvalue_threshold = "0.05")
)
outdir <- ensure_outdir(values$outdir)
read_gene_vector <- function(path) {
  lines <- readLines(path, warn = FALSE)
  lines <- trimws(lines)
  lines <- lines[nzchar(lines) & !grepl("^#", lines)]
  unique(vapply(strsplit(lines, "[\t, ]+"), `[`, character(1), 1))
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
    tab <- read.table(path, header = TRUE, sep = choose_sep(path, "auto"), quote = "", comment.char = "", check.names = FALSE, stringsAsFactors = FALSE)
    term_col <- if ("term" %in% colnames(tab)) "term" else colnames(tab)[1]
    gene_col <- if ("gene" %in% colnames(tab)) "gene" else colnames(tab)[2]
    for (term in unique(tab[[term_col]])) sets[[as.character(term)]] <- unique(as.character(tab[[gene_col]][tab[[term_col]] == term]))
  }
  sets[lengths(sets) > 0]
}
query <- read_gene_vector(values$genes)
sets <- read_gene_sets(values$gene_sets, values$gene_sets_format)
if (!length(query)) stop("gene list is empty", call. = FALSE)
if (!length(sets)) stop("gene set file produced no sets", call. = FALSE)
universe <- unique(c(query, unlist(sets, use.names = FALSE)))
query <- intersect(query, universe)
min_size <- parse_int(values$min_size, 5)
max_size <- parse_int(values$max_size, 500)
sets <- lapply(sets, intersect, universe)
sets <- sets[lengths(sets) >= min_size & lengths(sets) <= max_size]
if (!length(sets)) stop("no gene sets passed size filters", call. = FALSE)
N <- length(universe)
n <- length(query)
rows <- lapply(names(sets), function(term) {
  genes <- sets[[term]]
  k <- length(genes)
  overlap <- intersect(query, genes)
  x <- length(overlap)
  p <- phyper(x - 1, k, N - k, n, lower.tail = FALSE)
  data.frame(term = term, setSize = k, overlapSize = x, querySize = n, universeSize = N, pvalue = p, overlapGenes = paste(overlap, collapse = ","), stringsAsFactors = FALSE)
})
res <- do.call(rbind, rows)
res$padj <- p.adjust(res$pvalue, method = "BH")
res <- res[order(res$padj, res$pvalue, decreasing = FALSE), ]
p_thr <- parse_num(values$pvalue_threshold, 0.05)
top <- head(res, 25)
sig <- res[res$padj <= p_thr, , drop = FALSE]
write_tsv(res, file.path(outdir, "enrichment-results.tsv"))
write_tsv(top, file.path(outdir, "enrichment-top.tsv"))

svg(file.path(outdir, "enrichment-barplot.svg"), width = 8, height = 5)
plot_top <- head(res[res$overlapSize > 0, , drop = FALSE], 15)
if (nrow(plot_top) > 0) {
  scores <- -log10(pmax(plot_top$padj, .Machine$double.xmin))
  names(scores) <- plot_top$term
  par(mar = c(5, 10, 3, 1))
  barplot(rev(scores), horiz = TRUE, las = 1, col = "#0F766E", xlab = "-log10 adjusted p-value", main = "Functional enrichment")
} else {
  plot.new(); text(0.5, 0.5, "No overlapping gene sets")
}
dev.off()

write_outputs_json(outdir, list(
  queryGenes = n,
  universeGenes = N,
  geneSetsTested = nrow(res),
  significant = nrow(sig),
  results = "enrichment-results.tsv",
  topTable = "enrichment-top.tsv",
  plot = "enrichment-barplot.svg"
))
cat(sprintf("Enrichment complete: %d sets, %d significant\n", nrow(res), nrow(sig)))
