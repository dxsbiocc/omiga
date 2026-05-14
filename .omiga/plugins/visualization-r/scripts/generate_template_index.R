#!/usr/bin/env Rscript
# Generate a human-readable index from template.yaml files. template.yaml is the source of truth.
args <- commandArgs(trailingOnly = TRUE)
has_flag <- function(flag) flag %in% args
arg_value <- function(flag, default = NULL) { idx <- match(flag, args); if (!is.na(idx) && idx + 1 <= length(args)) args[[idx + 1]] else default }
script_path <- normalizePath(sub('^--file=', '', grep('^--file=', commandArgs(FALSE), value = TRUE)[[1]]), mustWork = TRUE)
plugin_root <- normalizePath(file.path(dirname(script_path), '..'), mustWork = TRUE)
outfile <- arg_value('--out', file.path(plugin_root, 'TEMPLATE_INDEX.md'))
extract <- function(lines, key) {
  hit <- grep(paste0('^  ', key, ': |^', key, ': '), lines, value = TRUE)[1]
  if (is.na(hit)) return('')
  sub('^[^:]+: *', '', hit)
}
extract_category <- function(lines) {
  idx <- grep('^classification:', lines)[1]
  if (is.na(idx)) return('')
  rest <- lines[idx:length(lines)]
  hit <- grep('^  category: ', rest, value = TRUE)[1]
  if (is.na(hit)) '' else sub('^  category: *', '', hit)
}
files <- list.files(file.path(plugin_root, 'templates'), pattern = '^template\\.ya?ml$', recursive = TRUE, full.names = TRUE)
rows <- lapply(files, function(path) {
  lines <- readLines(path, warn = FALSE)
  rel <- sub(paste0('^', normalizePath(plugin_root, mustWork = TRUE), '/?'), '', normalizePath(path, mustWork = TRUE))
  data.frame(id = extract(lines, 'id'), name = extract(lines, 'name'), category = extract_category(lines), description = extract(lines, 'description'), path = rel, stringsAsFactors = FALSE)
})
idx <- do.call(rbind, rows)
idx <- idx[order(idx$category, idx$id), ]
md <- c('# Visualization-R Template Index', '', '| ID | Name | Category | Description | Path |', '| --- | --- | --- | --- | --- |')
for (i in seq_len(nrow(idx))) md <- c(md, sprintf('| `%s` | %s | `%s` | %s | `%s` |', idx$id[i], idx$name[i], idx$category[i], idx$description[i], idx$path[i]))
rendered <- paste(md, collapse = '\n')
if (has_flag('--check')) {
  if (!file.exists(outfile) || !identical(readChar(outfile, file.info(outfile)$size), paste0(rendered, '\n'))) {
    cat(sprintf('Template index is out of date: %s\n', outfile)); quit(save = 'no', status = 1)
  }
  cat('Template index is up to date.\n')
} else {
  writeLines(rendered, outfile)
  cat(sprintf('Template index written to %s\n', outfile))
}
