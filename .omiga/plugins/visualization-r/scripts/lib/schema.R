# Lightweight validation helpers.
validate_columns <- function(data, columns, context = 'input') {
  columns <- columns[!is.na(columns) & nzchar(columns)]
  missing <- setdiff(columns, colnames(data))
  if (length(missing) > 0) {
    stop(sprintf('%s missing required column(s): %s', context, paste(missing, collapse = ', ')), call. = FALSE)
  }
  invisible(TRUE)
}

ensure_numeric_columns <- function(data, columns, context = 'input') {
  for (column in columns) {
    if (is.na(column) || !nzchar(column)) next
    validate_columns(data, column, context)
    converted <- suppressWarnings(as.numeric(data[[column]]))
    bad <- is.na(converted) & !is.na(data[[column]]) & nzchar(as.character(data[[column]]))
    if (any(bad)) stop(sprintf('%s column must be numeric: %s', context, column), call. = FALSE)
    data[[column]] <- converted
  }
  data
}

resolve_optional_column <- function(data, column) {
  if (is.null(column) || is.na(column) || !nzchar(as.character(column))) return(NULL)
  if (!as.character(column) %in% colnames(data)) return(NULL)
  as.character(column)
}
