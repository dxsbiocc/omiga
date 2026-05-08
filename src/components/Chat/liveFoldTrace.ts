export interface LiveReActFoldTraceInput {
  isStreaming: boolean;
  activeReactFoldId: string | null;
  /** User-visible assistant draft. This must render as a normal assistant bubble. */
  currentResponse: string;
  /** Hidden/thinking text that belongs inside the ReAct trace fold. */
  currentFoldIntermediate: string;
}

/**
 * Only hidden intermediate/thinking text belongs in the ReAct fold while a
 * turn is streaming. Visible assistant text after a tool result may be the
 * final answer, so folding `currentResponse` hides the reply behind "Thoughts".
 * If that visible text later precedes another tool call, the tool_use handler
 * moves it into `prefaceBeforeTools` at that point.
 */
export function selectLiveReActFoldTraceText(
  input: LiveReActFoldTraceInput,
): string {
  if (!input.isStreaming || !input.activeReactFoldId) return "";
  return input.currentFoldIntermediate.trim();
}

