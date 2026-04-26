/**
 * Text immediately before a tool call is user-visible assistant output.
 * Preserve it as-authored (including paragraphs, lists, and code fences) so the
 * ReAct trace does not replace it with a lossy headline/summary fragment.
 */
export function toolTracePrefaceFromText(text: string): string {
  return text.trim();
}
