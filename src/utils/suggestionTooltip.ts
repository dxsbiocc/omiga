function normalizeSuggestionTextForCompare(value: string): string {
  return value
    .replace(/\[(.*?)\]\((.*?)\)/g, "$1")
    .replace(/\*\*/g, "")
    .replace(/__/g, "")
    .replace(/`/g, "")
    .replace(/^[-*#>\s]+/u, "")
    .replace(/\s+/g, " ")
    .trim()
    .toLowerCase();
}

function stripSuggestionInlineWrapper(value: string): string {
  let current = value.trim();
  const wrappers: Array<[string, string]> = [
    ["**", "**"],
    ["__", "__"],
    ["`", "`"],
  ];

  let changed = true;
  while (changed && current.length > 1) {
    changed = false;
    for (const [start, end] of wrappers) {
      if (current.startsWith(start) && current.endsWith(end)) {
        current = current.slice(start.length, current.length - end.length).trim();
        changed = true;
      }
    }
  }

  return current;
}

function trimLeadingSuggestionTitle(raw: string, normalizedLabel: string): string {
  const lines = raw.split(/\r?\n/u);
  const firstLineIndex = lines.findIndex((line) => line.trim().length > 0);
  if (firstLineIndex < 0) return raw;

  const first = lines[firstLineIndex].trim();
  const heading = first.replace(/^#{1,6}\s+/u, "").trim();
  const listHead = heading.replace(/^[-*+]\s+/u, "").trim();
  const numberedHead = listHead.replace(/^\d+[.、)\]]\s*/u, "").trim();
  const firstCandidate = stripSuggestionInlineWrapper(numberedHead);
  const normalizedFirst = normalizeSuggestionTextForCompare(firstCandidate);

  if (normalizedFirst !== normalizedLabel) return raw;

  const rest = lines.slice(firstLineIndex + 1).join("\n").trim();
  return rest;
}

export function extractSuggestionTooltipMarkdown(
  text: string,
  label: string,
): string | null {
  const raw = text.trim();
  if (!raw) return null;

  const normalizedLabel = normalizeSuggestionTextForCompare(label);
  const normalizedRaw = normalizeSuggestionTextForCompare(raw);

  if (normalizedRaw === normalizedLabel) {
    return null;
  }

  const trimmedLeadingTitle = trimLeadingSuggestionTitle(raw, normalizedLabel);
  if (trimmedLeadingTitle !== raw) {
    if (!trimmedLeadingTitle) return null;
    const normalizedRest = normalizeSuggestionTextForCompare(trimmedLeadingTitle);
    if (normalizedRest === normalizedLabel) return null;
    return trimmedLeadingTitle;
  }

  const headingMatch = raw.match(/^(.{1,80}?)[：:\-—]\s*(.+)$/u);
  if (headingMatch) {
    const head = stripSuggestionInlineWrapper(headingMatch[1] ?? "");
    const rest = (headingMatch[2] ?? "").trim();
    if (
      rest &&
      normalizeSuggestionTextForCompare(head) === normalizedLabel &&
      normalizeSuggestionTextForCompare(rest) !== normalizedLabel
    ) {
      return rest;
    }
  }

  const colonSplit = raw.split(/[：:]/u);
  if (colonSplit.length > 1) {
    const head = colonSplit[0]?.trim() ?? "";
    const rest = colonSplit.slice(1).join("：").trim();
    if (
      rest &&
      normalizeSuggestionTextForCompare(head) === normalizedLabel
    ) {
      return rest;
    }
  }

  return raw;
}

