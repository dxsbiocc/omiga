# Shared bootstrap for visualization-r templates.
`%||%` <- function(x, y) if (is.null(x) || length(x) == 0 || (length(x) == 1 && is.na(x))) y else x

load_packages <- function(packages) {
  missing <- packages[!vapply(packages, requireNamespace, logical(1), quietly = TRUE)]
  if (length(missing) > 0) {
    stop(
      paste0(
        'Missing required R package(s): ', paste(missing, collapse = ', '),
        '. Install them in the active Omiga analysis environment before running this template.'
      ),
      call. = FALSE
    )
  }
  invisible(TRUE)
}

get_current_script <- function() {
  file_arg <- grep('^--file=', commandArgs(FALSE), value = TRUE)
  if (length(file_arg) > 0) return(normalizePath(sub('^--file=', '', file_arg[[1]]), mustWork = TRUE))
  args <- commandArgs(FALSE)
  idx <- match('-f', args)
  if (!is.na(idx) && idx + 1 <= length(args)) return(normalizePath(args[[idx + 1]], mustWork = TRUE))
  stop('Cannot determine current R script path. Run this file with Rscript.', call. = FALSE)
}

source_if_exists <- function(path) {
  if (!file.exists(path)) stop(sprintf('Required helper not found: %s', path), call. = FALSE)
  source(path, local = FALSE)
}
