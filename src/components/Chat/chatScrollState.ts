export interface ScrollMetrics {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
}

/** How close to the bottom (px) before auto-scroll kicks in. */
export const AUTO_SCROLL_BOTTOM_THRESHOLD_PX = 100;

export function isNearScrollBottom(
  metrics: ScrollMetrics,
  thresholdPx = AUTO_SCROLL_BOTTOM_THRESHOLD_PX,
): boolean {
  return (
    metrics.scrollTop + metrics.clientHeight >=
    metrics.scrollHeight - thresholdPx
  );
}

export function shouldShowJumpToLatestButton(
  metrics: ScrollMetrics,
  thresholdPx = AUTO_SCROLL_BOTTOM_THRESHOLD_PX,
  hasVisibleTranscriptContent = true,
): boolean {
  if (!hasVisibleTranscriptContent) return false;
  const hasScrollableHistory = metrics.scrollHeight > metrics.clientHeight;
  return hasScrollableHistory && !isNearScrollBottom(metrics, thresholdPx);
}
