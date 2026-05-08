export type NestedToolPanelOpenByFold = Record<string, Record<string, boolean>>;

export const EMPTY_NESTED_TOOL_PANEL_OPEN: Readonly<Record<string, boolean>> =
  Object.freeze({});

export function getNestedToolPanelOpenForFold(
  state: NestedToolPanelOpenByFold,
  foldId: string,
): Readonly<Record<string, boolean>> {
  return state[foldId] ?? EMPTY_NESTED_TOOL_PANEL_OPEN;
}

export function toggleNestedToolPanelOpenForFold(
  state: NestedToolPanelOpenByFold,
  foldId: string,
  messageId: string,
  currentlyOpen: boolean,
): NestedToolPanelOpenByFold {
  const foldOverrides = getNestedToolPanelOpenForFold(state, foldId);
  return {
    ...state,
    [foldId]: { ...foldOverrides, [messageId]: !currentlyOpen },
  };
}
