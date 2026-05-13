#!/usr/bin/env Rscript
# Smoke-test visualization-r templates without requiring Omiga runtime.

args <- commandArgs(trailingOnly = TRUE)
arg_value <- function(flag, default = NULL) {
  idx <- match(flag, args)
  if (!is.na(idx) && idx + 1 <= length(args)) return(args[[idx + 1]])
  default
}
has_flag <- function(flag) flag %in% args

script_path <- normalizePath(sub('^--file=', '', grep('^--file=', commandArgs(FALSE), value = TRUE)[[1]]), mustWork = TRUE)
plugin_root <- normalizePath(file.path(dirname(script_path), '..'), mustWork = TRUE)
templates_root <- file.path(plugin_root, 'templates')
selected <- arg_value('--template', NULL)
out_root <- arg_value('--outdir', file.path(tempdir(), paste0('visualization-r-smoke-', Sys.getpid())))

template_dirs <- list.dirs(templates_root, recursive = TRUE, full.names = TRUE)
template_dirs <- template_dirs[file.exists(file.path(template_dirs, 'template.R.j2'))]
slugs <- sub(paste0('^', normalizePath(templates_root, mustWork = TRUE), '/?'), '', normalizePath(template_dirs, mustWork = TRUE))
if (!is.null(selected)) {
  keep <- slugs == selected
  template_dirs <- template_dirs[keep]
  slugs <- slugs[keep]
}
if (!length(template_dirs)) stop('No templates matched.', call. = FALSE)

dir.create(out_root, recursive = TRUE, showWarnings = FALSE)
render_template <- function(path, manifest_dir) {
  raw <- readLines(path, warn = FALSE)
  raw <- gsub('{{ template.pluginRoot }}', plugin_root, raw, fixed = TRUE)
  raw <- gsub('{{ template.manifestDir }}', manifest_dir, raw, fixed = TRUE)
  raw
}

required_output_globs <- function(template_yaml) {
  lines <- readLines(template_yaml, warn = FALSE)
  output_start <- grep('^  outputs:\\s*$', lines)
  if (!length(output_start)) return(c('figure.png', 'figure.pdf'))
  start <- output_start[[1]]
  end_candidates <- grep('^[^[:space:]]', lines)
  end_candidates <- end_candidates[end_candidates > start]
  end <- if (length(end_candidates)) end_candidates[[1]] - 1 else length(lines)
  block <- lines[(start + 1):end]

  globs <- character()
  current_glob <- NULL
  current_required <- FALSE
  flush <- function() {
    if (!is.null(current_glob) && isTRUE(current_required)) {
      globs <<- c(globs, current_glob)
    }
  }
  for (line in block) {
    if (grepl('^    [A-Za-z0-9_.-]+:\\s*$', line)) {
      flush()
      current_glob <- NULL
      current_required <- FALSE
      next
    }
    glob <- sub('^\\s*glob:\\s*', '', line)
    if (!identical(glob, line)) {
      current_glob <- trimws(glob)
      next
    }
    required <- sub('^\\s*required:\\s*', '', line)
    if (!identical(required, line)) {
      current_required <- tolower(trimws(required)) == 'true'
    }
  }
  flush()
  if (!length(globs)) c('figure.png', 'figure.pdf') else unique(globs)
}

failures <- character()
for (i in seq_along(template_dirs)) {
  slug <- slugs[[i]]
  dir <- template_dirs[[i]]
  example <- file.path(dir, 'example.tsv')
  if (!file.exists(example)) {
    failures <- c(failures, paste(slug, 'missing example.tsv'))
    next
  }
  run_dir <- file.path(out_root, gsub('/', '__', slug, fixed = TRUE))
  dir.create(run_dir, recursive = TRUE, showWarnings = FALSE)
  rendered <- file.path(run_dir, 'rendered-template.R')
  writeLines(render_template(file.path(dir, 'template.R.j2'), dir), rendered)
  outdir <- file.path(run_dir, 'out')
  cmd_args <- c(rendered, example, outdir)
  status <- system2('Rscript', cmd_args, stdout = TRUE, stderr = TRUE)
  exit <- attr(status, 'status')
  if (is.null(exit)) exit <- 0L
  if (!identical(as.integer(exit), 0L)) {
    failures <- c(failures, paste(slug, 'Rscript failed:', paste(status, collapse = '\n')))
    next
  }
  required <- file.path(outdir, required_output_globs(file.path(dir, 'template.yaml')))
  missing <- required[!file.exists(required) | file.info(required)$size <= 0]
  if (length(missing)) failures <- c(failures, paste(slug, 'missing/empty outputs:', paste(basename(missing), collapse = ', ')))
  cat(sprintf('ok %s -> %s\n', slug, outdir))
}

if (length(failures)) {
  cat('Smoke failures:\n')
  cat(paste0('- ', failures, collapse = '\n'), '\n')
  quit(save = 'no', status = 1)
}
cat(sprintf('All %d visualization-r template smoke tests passed. Output root: %s\n', length(template_dirs), out_root))
