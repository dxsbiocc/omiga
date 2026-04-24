export function compactLabel(text: string, maxChars = 18): string {
  const cleaned = (text ?? "").replace(/\s+/g, " ").trim();
  if (!cleaned) return "";
  const chars = [...cleaned];
  if (chars.length <= maxChars) return cleaned;
  return `${chars.slice(0, Math.max(1, maxChars - 1)).join("")}…`;
}

export function isLabelCompacted(original: string, compacted: string): boolean {
  const cleaned = (original ?? "").replace(/\s+/g, " ").trim();
  return cleaned !== compacted;
}

