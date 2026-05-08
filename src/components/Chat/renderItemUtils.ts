export type RenderItemKeyInput =
  | { kind: "react_fold"; id: string }
  | { kind: "row"; message: { id: string } };

export function messageRenderItemKey(item: RenderItemKeyInput): string {
  return item.kind === "react_fold" ? item.id : item.message.id;
}

export function messageEntranceDelayMs(itemIndex: number): number {
  return Math.min(itemIndex * 35, 280);
}

export function shouldAnimateMessageItem({
  restoringOlderItems,
}: {
  /** True while progressive phase-2 is mounting older history above the viewport. */
  restoringOlderItems: boolean;
}): boolean {
  return !restoringOlderItems;
}
