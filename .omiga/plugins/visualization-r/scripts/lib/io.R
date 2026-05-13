# Input helpers for visualization-r templates.
choose_sep <- function(path, delimiter = 'auto') {
  delimiter <- tolower(as.character(delimiter %||% 'auto'))
  if (delimiter %in% c('tab', 'tsv', '\\t')) return('\t')
  if (delimiter %in% c('comma', 'csv', ',')) return(',')
  first <- readLines(path, n = 1, warn = FALSE)
  if (length(first) == 0) return('\t')
  if (grepl(',', first, fixed = TRUE) && !grepl('\t', first, fixed = TRUE)) ',' else '\t'
}

read_table_auto <- function(path, delimiter = 'auto') {
  if (is.null(path) || !nzchar(as.character(path))) stop('Input table path is required.', call. = FALSE)
  if (!file.exists(path)) stop(sprintf('Input table does not exist: %s', path), call. = FALSE)
  utils::read.table(
    path,
    header = TRUE,
    sep = choose_sep(path, delimiter),
    quote = '"',
    comment.char = '',
    check.names = FALSE,
    stringsAsFactors = FALSE
  )
}

write_tsv <- function(data, path) {
  utils::write.table(data, path, sep = '\t', quote = FALSE, row.names = FALSE, col.names = TRUE, na = '')
}
