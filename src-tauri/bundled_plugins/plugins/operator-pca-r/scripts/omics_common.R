if (!nzchar(Sys.getenv("XDG_CACHE_HOME"))) Sys.setenv(XDG_CACHE_HOME = tempdir())

args_named <- function(required = character(), optional = list()) {
  args <- commandArgs(trailingOnly = TRUE)
  if (length(args) < length(required)) {
    stop(sprintf("expected at least %d arguments, got %d", length(required), length(args)), call. = FALSE)
  }
  values <- list()
  for (i in seq_along(required)) values[[required[[i]]]] <- args[[i]]
  if (length(optional) > 0) {
    offset <- length(required)
    names_optional <- names(optional)
    for (i in seq_along(optional)) {
      idx <- offset + i
      values[[names_optional[[i]]]] <- if (idx <= length(args) && nzchar(args[[idx]])) args[[idx]] else optional[[i]]
    }
  }
  values
}

parse_bool <- function(x, default = FALSE) {
  if (is.null(x) || is.na(x) || !nzchar(as.character(x))) return(default)
  tolower(as.character(x)) %in% c("1", "true", "t", "yes", "y")
}

parse_num <- function(x, default) {
  value <- suppressWarnings(as.numeric(x))
  if (is.na(value)) default else value
}

parse_int <- function(x, default) {
  value <- suppressWarnings(as.integer(x))
  if (is.na(value)) default else value
}

choose_sep <- function(path, delimiter = "auto") {
  delimiter <- tolower(delimiter)
  if (delimiter %in% c("tab", "tsv", "\\t")) return("\t")
  if (delimiter %in% c("comma", "csv", ",")) return(",")
  first <- readLines(path, n = 1, warn = FALSE)
  if (length(first) == 0) return("\t")
  if (grepl(",", first, fixed = TRUE) && !grepl("\t", first, fixed = TRUE)) "," else "\t"
}

read_table_file <- function(path, delimiter = "auto", header = TRUE) {
  read.table(path, header = header, sep = choose_sep(path, delimiter), quote = "", comment.char = "", check.names = FALSE, stringsAsFactors = FALSE)
}

read_matrix_file <- function(path, delimiter = "auto", row_names = TRUE) {
  data <- read_table_file(path, delimiter, header = TRUE)
  if (nrow(data) == 0 || ncol(data) == 0) stop("matrix file is empty", call. = FALSE)
  if (row_names) {
    ids <- data[[1]]
    data <- data[, -1, drop = FALSE]
    rownames(data) <- make.unique(as.character(ids))
  }
  mat <- as.matrix(data)
  suppressWarnings(storage.mode(mat) <- "numeric")
  if (anyNA(mat)) stop("matrix contains non-numeric values after parsing", call. = FALSE)
  mat
}

write_tsv <- function(data, path) {
  write.table(data, path, sep = "\t", quote = FALSE, row.names = FALSE, col.names = TRUE, na = "")
}

normalize_sample_id <- function(value) {
  vapply(as.character(value), function(one) {
    text <- trimws(one)
    if (!nzchar(text)) return("")
    name <- basename(text)
    suffixes <- c(".sorted.bam", ".bam", ".counts", ".count", ".txt", ".tsv", ".csv")
    lower <- tolower(name)
    for (suffix in suffixes) {
      if (endsWith(lower, suffix)) {
        name <- substr(name, 1, nchar(name) - nchar(suffix))
        lower <- tolower(name)
        break
      }
    }
    tolower(gsub("[-.]", "_", name))
  }, character(1), USE.NAMES = FALSE)
}

sample_match_key <- function(value) {
  key <- normalize_sample_id(value)
  sub("_?(counts?|tpm|fpkm|rpkm|cpm|expression|expr|matrix)$", "", key, perl = TRUE)
}

first_existing_col <- function(data, candidates, fallback = NULL) {
  normalized <- setNames(colnames(data), tolower(gsub("[ .-]+", "_", colnames(data))))
  for (candidate in candidates) {
    key <- tolower(gsub("[ .-]+", "_", candidate))
    value <- normalized[[key]]
    if (!is.null(value) && !is.na(value)) return(value)
  }
  if (!is.null(fallback)) return(fallback)
  colnames(data)[1]
}

read_sample_metadata <- function(path, sample_names, delimiter = "auto", sample_column = "sample", group_column = "group") {
  if (is.null(path) || !nzchar(as.character(path)) || !file.exists(path)) {
    return(data.frame(sample = sample_names, group = "All samples", stringsAsFactors = FALSE))
  }
  meta <- read_table_file(path, delimiter, header = TRUE)
  if (nrow(meta) == 0) stop("sample metadata file is empty", call. = FALSE)
  sample_col <- if (nzchar(sample_column) && sample_column %in% colnames(meta)) sample_column else first_existing_col(meta, c("sample", "sample_id", "sampleid", "id"), colnames(meta)[1])
  group_col <- if (nzchar(group_column) && group_column %in% colnames(meta)) group_column else first_existing_col(meta, c("group", "condition", "type", "treatment", "class"), if (ncol(meta) >= 2) colnames(meta)[2] else colnames(meta)[1])

  exact_map <- setNames(sample_names, normalize_sample_id(sample_names))
  loose_map <- setNames(sample_names, sample_match_key(sample_names))
  rows <- lapply(seq_len(nrow(meta)), function(i) {
    raw_sample <- as.character(meta[[sample_col]][[i]])
    key_exact <- normalize_sample_id(raw_sample)
    key_loose <- sample_match_key(raw_sample)
    sample <- if (key_exact %in% names(exact_map)) exact_map[[key_exact]] else NA_character_
    if (is.na(sample) && key_loose %in% names(loose_map)) sample <- loose_map[[key_loose]]
    if (is.na(sample)) return(NULL)
    group <- trimws(as.character(meta[[group_col]][[i]]))
    if (!nzchar(group)) group <- "Unknown"
    data.frame(sample = sample, group = group, stringsAsFactors = FALSE)
  })
  rows <- do.call(rbind, rows[!vapply(rows, is.null, logical(1))])
  if (is.null(rows) || nrow(rows) == 0) stop("no metadata samples matched matrix columns", call. = FALSE)
  rows <- rows[!duplicated(rows$sample), , drop = FALSE]
  missing <- setdiff(sample_names, rows$sample)
  if (length(missing)) rows <- rbind(rows, data.frame(sample = missing, group = "Unmatched", stringsAsFactors = FALSE))
  rows$sample <- factor(rows$sample, levels = sample_names)
  rows <- rows[order(rows$sample), , drop = FALSE]
  rows$sample <- as.character(rows$sample)
  rows
}

parse_comparisons <- function(text) {
  if (is.null(text) || !nzchar(trimws(as.character(text)))) return(list())
  chunks <- unlist(strsplit(as.character(text), "[;,]"))
  pairs <- list()
  seen <- character()
  for (chunk in chunks) {
    chunk <- trimws(chunk)
    if (!nzchar(chunk)) next
    parts <- unlist(strsplit(chunk, "\\s*(?:vs|VS|Vs|v|V|:|\\|>|/)\\s*", perl = TRUE))
    if (length(parts) < 2) next
    a <- trimws(parts[[1]])
    b <- trimws(parts[[2]])
    if (!nzchar(a) || !nzchar(b) || identical(a, b)) next
    key <- paste(a, b, sep = "\r")
    if (key %in% seen) stop("duplicate comparisons are not allowed", call. = FALSE)
    seen <- c(seen, key)
    pairs[[length(pairs) + 1]] <- c(a, b)
  }
  pairs
}

build_default_pairs <- function(groups) {
  groups <- unique(as.character(groups))
  groups <- groups[nzchar(groups)]
  if (length(groups) < 2) return(list())
  pairs <- list()
  for (i in seq_len(length(groups) - 1)) {
    for (j in seq((i + 1), length(groups))) pairs[[length(pairs) + 1]] <- c(groups[[i]], groups[[j]])
  }
  pairs
}

operator_palette <- function(n) {
  base <- c("#2563EB", "#DC2626", "#059669", "#D97706", "#7C3AED", "#0891B2", "#DB2777", "#65A30D", "#4B5563")
  if (n <= length(base)) return(base[seq_len(n)])
  grDevices::hcl.colors(n, palette = "Dark 3")
}

safe_neg_log10 <- function(x) {
  -log10(pmax(as.numeric(x), .Machine$double.xmin))
}

plot_empty_svg <- function(path, title, message, width = 7, height = 5) {
  svg(path, width = width, height = height)
  plot.new()
  title(main = title)
  text(0.5, 0.5, message, cex = 1.1)
  invisible(dev.off())
}

json_escape <- function(x) {
  x <- as.character(x)
  x <- gsub("\\\\", "\\\\\\\\", x)
  x <- gsub('"', '\\\\"', x)
  x <- gsub("\n", "\\\\n", x)
  x <- gsub("\r", "\\\\r", x)
  x <- gsub("\t", "\\\\t", x)
  x
}

json_scalar <- function(x) {
  if (is.null(x) || length(x) == 0 || is.na(x)) return("null")
  if (is.logical(x)) return(if (isTRUE(x)) "true" else "false")
  if (is.numeric(x)) return(format(x, scientific = FALSE, trim = TRUE))
  sprintf('"%s"', json_escape(x))
}

json_object <- function(named_values) {
  parts <- vapply(names(named_values), function(name) {
    sprintf('"%s":%s', json_escape(name), json_scalar(named_values[[name]]))
  }, character(1))
  paste0("{", paste(parts, collapse = ","), "}")
}

write_outputs_json <- function(outdir, summary_values, extra_json = NULL) {
  summary <- json_object(summary_values)
  parts <- c(sprintf('"summary":%s', summary))
  if (!is.null(extra_json) && nzchar(extra_json)) parts <- c(parts, extra_json)
  writeLines(paste0("{", paste(parts, collapse = ","), "}"), file.path(outdir, "outputs.json"))
}

ensure_outdir <- function(outdir) {
  dir.create(outdir, recursive = TRUE, showWarnings = FALSE)
  normalizePath(outdir, mustWork = TRUE)
}
