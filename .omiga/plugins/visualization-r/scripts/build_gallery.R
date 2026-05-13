#!/usr/bin/env Rscript
# Build local gallery previews into a cache/temp directory, never into template source directories.
args <- commandArgs(trailingOnly = TRUE)
arg_value <- function(flag, default = NULL) { idx <- match(flag, args); if (!is.na(idx) && idx + 1 <= length(args)) args[[idx + 1]] else default }
script_path <- normalizePath(sub('^--file=', '', grep('^--file=', commandArgs(FALSE), value = TRUE)[[1]]), mustWork = TRUE)
plugin_root <- normalizePath(file.path(dirname(script_path), '..'), mustWork = TRUE)
outdir <- arg_value('--outdir', file.path(tempdir(), paste0('visualization-r-gallery-', Sys.getpid())))
smoke_out <- file.path(outdir, 'runs')
dir.create(smoke_out, recursive = TRUE, showWarnings = FALSE)
status <- system2('Rscript', c(file.path(plugin_root, 'scripts', 'smoke_templates.R'), '--outdir', smoke_out), stdout = TRUE, stderr = TRUE)
exit <- attr(status, 'status'); if (is.null(exit)) exit <- 0L
if (!identical(as.integer(exit), 0L)) { cat(paste(status, collapse = '\n'), '\n'); quit(save = 'no', status = 1) }
figs <- list.files(smoke_out, pattern = '^figure\\.png$', recursive = TRUE, full.names = TRUE)
md <- c('# Visualization-R Gallery', '', sprintf('Generated: %s', Sys.time()), '')
for (fig in figs) {
  slug <- basename(dirname(dirname(fig)))
  target_dir <- file.path(outdir, 'previews', slug)
  dir.create(target_dir, recursive = TRUE, showWarnings = FALSE)
  target <- file.path(target_dir, 'figure.png')
  file.copy(fig, target, overwrite = TRUE)
  rel <- file.path('previews', slug, 'figure.png')
  md <- c(md, sprintf('## %s', gsub('__', '/', slug, fixed = TRUE)), '', sprintf('![%s](%s)', slug, rel), '')
}
writeLines(md, file.path(outdir, 'GALLERY.md'))
cat(sprintf('Gallery written to %s\n', outdir))
