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

read_matrix_file <- function(path, delimiter = "auto", row_names = TRUE) {
  sep <- choose_sep(path, delimiter)
  data <- read.table(path, header = TRUE, sep = sep, quote = "", comment.char = "", check.names = FALSE, stringsAsFactors = FALSE)
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
