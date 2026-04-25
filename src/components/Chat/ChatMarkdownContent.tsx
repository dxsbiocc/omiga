import { memo, useMemo } from "react";
import { Box, Typography } from "@mui/material";
import type { Components } from "react-markdown";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import type { ChatTokenSet } from "./chatTokens";

const GFM_PLUGINS = [remarkGfm];
const GFM_MATH_PLUGINS = [remarkGfm, remarkMath];
const KATEX_PLUGINS = [rehypeKatex];

/**
 * Detect whether markdown actually contains math syntax before enabling the
 * remark-math + rehype-katex pipeline. Most chat messages do not contain math;
 * skipping that parser avoids unnecessary work on every parent re-render.
 */
export function hasMarkdownMath(source: string): boolean {
  if (!source) return false;
  return (
    /\\\(|\\\[/.test(source) ||
    /\\begin\{(?:equation|align|aligned|gather|matrix|pmatrix|bmatrix|cases)\}/.test(source) ||
    /\$\$[\s\S]+?\$\$/.test(source) ||
    /(^|[^$])\$[^$\n]{1,240}\$(?!\$)/m.test(source)
  );
}

/** Normalize cheap text-only transforms once per content change. */
export function normalizeChatMarkdown(source: string): string {
  return fixBrokenGfmTables(source.replace(/<br\s*\/?>/gi, "\n"));
}

/**
 * Repair common GFM table breakage caused by LLMs placing newlines inside
 * table cells or embedding next-row markers mid-line.
 */
export function fixBrokenGfmTables(md: string): string {
  const lines = md.split("\n");
  const out: string[] = [];
  let afterSeparator = false;

  const isTableRow = (s: string) => s.trimStart().startsWith("|");
  const isSeparatorRow = (s: string) => /^\s*\|[\s\-:|]+\|/.test(s);

  for (const line of lines) {
    const trimmed = line.trim();

    if (isSeparatorRow(trimmed)) {
      afterSeparator = true;
      out.push(line);
      continue;
    }

    if (!trimmed) {
      afterSeparator = false;
      out.push(line);
      continue;
    }

    if (isTableRow(trimmed)) {
      out.push(line);
      continue;
    }

    if (afterSeparator) {
      const embeddedRow = / \| \| | \|\| /;
      if (embeddedRow.test(line)) {
        const parts = line.split(/ \| \| | \|\| /);
        if (parts[0].trim() && out.length > 0 && isTableRow(out[out.length - 1])) {
          const prev = out[out.length - 1].trimEnd();
          out[out.length - 1] = prev.endsWith("|")
            ? `${prev.slice(0, -1).trimEnd()} ${parts[0].trim()} |`
            : `${prev} ${parts[0].trim()}`;
        } else if (parts[0].trim()) {
          out.push(parts[0].trim());
        }
        for (let p = 1; p < parts.length; p++) {
          if (parts[p].trim()) {
            const rowContent = parts[p].trim();
            out.push(rowContent.startsWith("|") ? rowContent : `| ${rowContent}`);
          }
        }
        continue;
      }

      if (out.length > 0 && isTableRow(out[out.length - 1])) {
        const prev = out[out.length - 1].trimEnd();
        out[out.length - 1] = prev.endsWith("|")
          ? `${prev.slice(0, -1).trimEnd()} ${trimmed} |`
          : `${prev} ${trimmed}`;
        continue;
      }

      afterSeparator = false;
    }

    out.push(line);
  }

  return out.join("\n");
}

interface ChatMarkdownContentProps {
  content: string;
  tone?: "default" | "agent";
  components: Components;
  chat: ChatTokenSet;
}

export const ChatMarkdownContent = memo(function ChatMarkdownContent({
  content,
  tone = "default",
  components,
  chat,
}: ChatMarkdownContentProps) {
  const isAgent = tone === "agent";
  const normalized = useMemo(() => normalizeChatMarkdown(content), [content]);
  const useMath = useMemo(() => hasMarkdownMath(normalized), [normalized]);

  if (!content || content.trim() === "") {
    return (
      <Typography variant="body1" color="text.secondary" sx={{ fontStyle: "italic" }}>
        (Empty response)
      </Typography>
    );
  }

  return (
    <Box
      sx={{
        fontFamily: chat.font,
        minWidth: 0,
        maxWidth: "100%",
        overflowX: "hidden",
        overflowWrap: "anywhere",
        wordBreak: "break-word",
        ...(isAgent ? { "& :first-of-type": { mt: 0 } } : {}),
      }}
    >
      <ReactMarkdown
        remarkPlugins={useMath ? GFM_MATH_PLUGINS : GFM_PLUGINS}
        rehypePlugins={useMath ? KATEX_PLUGINS : []}
        components={components}
      >
        {normalized}
      </ReactMarkdown>
    </Box>
  );
});
