# Shared visual defaults.
theme_visualization <- function(base_size = 12) {
  ggplot2::theme_minimal(base_size = base_size) +
    ggplot2::theme(
      panel.grid.minor = ggplot2::element_blank(),
      plot.title = ggplot2::element_text(face = 'bold'),
      legend.position = 'right'
    )
}

visualization_palette <- function(n) {
  base <- c('#2563EB', '#DC2626', '#059669', '#D97706', '#7C3AED', '#0891B2', '#DB2777', '#65A30D', '#4B5563')
  if (n <= length(base)) return(base[seq_len(n)])
  grDevices::hcl.colors(n, palette = 'Dark 3')
}
