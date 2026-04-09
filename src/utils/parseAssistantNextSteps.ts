export interface NextStepSuggestion {
  label: string;
  text: string;
}

/** `### 下一步建议` / `## 下一步建议`，标题后可跟（条件出现）等括号说明 */
const NEXT_STEPS_HEADING =
  /^#{2,3}\s*下一步建议(?:\s*[（(][^）)]*[）)])?\s*$/u;

/** 编号行：`1. xxx` 或 `1、xxx` */
const NUMBERED_ITEM = /^(\d+)[.、]\s+(.+)$/u;

function clampLabelForChip(s: string, maxChars: number): string {
  const t = s.replace(/\s+/g, " ").trim();
  if (!t) return "";
  const chars = [...t];
  if (chars.length <= maxChars) return t;
  return `${chars.slice(0, maxChars - 1).join("")}…`;
}

/**
 * 从助手 Markdown 正文中解析「### 下一步建议」小节下的编号列表，用于生成快捷按钮。
 * 无该小节或列表为空时返回 []（按需出现，不生成泛化建议）。
 */
export function parseNextStepSuggestionsFromMarkdown(raw: string): NextStepSuggestion[] {
  const text = (raw ?? "").replace(/\r\n/g, "\n");
  const lines = text.split("\n");
  let sectionStart = -1;
  for (let i = 0; i < lines.length; i++) {
    if (NEXT_STEPS_HEADING.test(lines[i].trim())) {
      sectionStart = i + 1;
      break;
    }
  }
  if (sectionStart < 0) return [];

  const out: NextStepSuggestion[] = [];
  const seen = new Set<string>();

  for (let i = sectionStart; i < lines.length; i++) {
    const trimmed = lines[i].trim();
    if (/^#{1,3}\s+\S/.test(trimmed)) {
      break;
    }
    const m = trimmed.match(NUMBERED_ITEM);
    if (!m) continue;
    const body = m[2].trim();
    if (!body) continue;
    if (seen.has(body)) continue;
    seen.add(body);
    const label = clampLabelForChip(body, 14);
    if (!label) continue;
    out.push({ label, text: body });
    if (out.length >= 8) break;
  }

  return out;
}
