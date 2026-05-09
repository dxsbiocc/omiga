# Output helpers for visualization-r templates.
ensure_dir <- function(path) {
  if (!dir.exists(path)) dir.create(path, recursive = TRUE, showWarnings = FALSE)
  normalizePath(path, mustWork = TRUE)
}

shell_quote <- function(x) {
  paste0("'", gsub("'", "'\\''", as.character(x), fixed = TRUE), "'")
}

json_escape <- function(x) {
  x <- as.character(x)
  x <- gsub('\\\\', '\\\\\\\\', x)
  x <- gsub('"', '\\"', x)
  x <- gsub('\n', '\\n', x)
  x <- gsub('\r', '\\r', x)
  x <- gsub('\t', '\\t', x)
  x
}

write_outputs_json <- function(outdir, summary = list()) {
  keys <- names(summary)
  fields <- character()
  for (key in keys) {
    value <- summary[[key]]
    if (is.numeric(value) || is.integer(value)) {
      rendered <- ifelse(is.finite(value), as.character(value), 'null')
    } else if (is.logical(value)) {
      rendered <- ifelse(isTRUE(value), 'true', 'false')
    } else {
      rendered <- paste0('"', json_escape(value), '"')
    }
    fields <- c(fields, paste0('"', json_escape(key), '":', rendered))
  }
  raw <- paste0('{"summary":{', paste(fields, collapse = ','), '}}')
  writeLines(raw, file.path(outdir, 'outputs.json'))
}

save_ggplot_outputs <- function(plot, outdir, prefix = 'figure', width = 8, height = 6, dpi = 300, formats = c('png', 'pdf')) {
  outdir <- ensure_dir(outdir)
  paths <- character()
  for (fmt in tolower(formats)) {
    path <- file.path(outdir, paste0(prefix, '.', fmt))
    if (fmt == 'png') {
      ggplot2::ggsave(path, plot = plot, width = width, height = height, units = 'in', dpi = dpi, device = 'png')
    } else if (fmt == 'pdf') {
      ggplot2::ggsave(path, plot = plot, width = width, height = height, units = 'in', device = 'pdf')
    } else if (fmt == 'svg') {
      ggplot2::ggsave(path, plot = plot, width = width, height = height, units = 'in', device = 'svg')
    } else {
      stop(sprintf('Unsupported output format: %s', fmt), call. = FALSE)
    }
    paths <- c(paths, path)
  }
  invisible(paths)
}

write_working_artifacts <- function(
  outdir,
  script_path,
  args,
  source_name = 'plot-script.R',
  rerun_name = 'rerun.sh',
  command = 'Rscript'
) {
  outdir <- ensure_dir(outdir)
  script_target <- file.path(outdir, source_name)
  same_source <- normalizePath(script_path, mustWork = FALSE) == normalizePath(script_target, mustWork = FALSE)
  if (!same_source) {
    file.copy(script_path, script_target, overwrite = TRUE)
  }
  Sys.chmod(script_target, mode = '0644')
  if (is.null(rerun_name) || !nzchar(rerun_name)) {
    return(invisible(c(source = script_target)))
  }
  rerun <- file.path(outdir, rerun_name)
  cmd <- paste(c(command, shell_quote(source_name), vapply(args, shell_quote, character(1))), collapse = ' ')
  writeLines(c('#!/usr/bin/env bash', 'set -euo pipefail', 'cd "$(dirname "$0")"', cmd), rerun)
  Sys.chmod(rerun, mode = '0755')
  invisible(c(source = script_target, rerun = rerun))
}
