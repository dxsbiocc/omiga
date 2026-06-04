export interface TitleBarContentLeftInput {
  buttonRailEnd: number;
  chatRailInset: number;
  /** Ignored for chat sessions so the title/status anchor does not jump left. */
  leftPanelCollapsed: boolean;
  leftPanelWidth: number;
  resizeHandleWidth: number;
  showSettingsPanel: boolean;
}

export function computeTitleBarContentLeft({
  buttonRailEnd,
  chatRailInset,
  leftPanelWidth,
  resizeHandleWidth,
  showSettingsPanel,
}: TitleBarContentLeftInput): number {
  const chatRailLeft =
    (showSettingsPanel ? 0 : leftPanelWidth + resizeHandleWidth) +
    chatRailInset;
  return Math.max(buttonRailEnd, chatRailLeft);
}
