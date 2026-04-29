import { memo, useId, useMemo } from "react";
import {
  Accordion,
  AccordionDetails,
  AccordionSummary,
  Box,
  Typography,
  alpha,
} from "@mui/material";
import { ExpandMore } from "@mui/icons-material";
import type { Components } from "react-markdown";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import { useTheme } from "@mui/material/styles";
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
  return fixBrokenGfmTables(
    normalizeSafeHtmlAnchors(source.replace(/<br\s*\/?>/gi, "\n")),
  );
}

function decodeBasicHtmlEntities(value: string): string {
  return value
    .replace(/&amp;/gi, "&")
    .replace(/&quot;/gi, '"')
    .replace(/&#39;|&apos;/gi, "'")
    .replace(/&lt;/gi, "<")
    .replace(/&gt;/gi, ">");
}

function escapeMarkdownLinkText(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/\[/g, "\\[").replace(/\]/g, "\\]");
}

function escapeMarkdownAngleUrl(value: string): string {
  return value.replace(/</g, "%3C").replace(/>/g, "%3E");
}

/**
 * ReactMarkdown intentionally ignores raw HTML in our chat renderer. Convert
 * safe HTTP(S) `<a href="...">label</a>` citations into Markdown links so model
 * output that asks for "a tags" still becomes the same clickable CitationLink UI
 * without enabling arbitrary raw HTML.
 */
export function normalizeSafeHtmlAnchors(source: string): string {
  if (!source || !/<a\s/i.test(source)) return source;

  return source.replace(
    /<a\b([^>]*?)\bhref\s*=\s*(["'])(https?:\/\/[^"']+)\2([^>]*)>([\s\S]*?)<\/a>/gi,
    (match, _before, _quote, rawUrl: string, _after, rawLabel: string) => {
      const url = decodeBasicHtmlEntities(rawUrl.trim());
      if (!/^https?:\/\//i.test(url) || /[\s<>"']/.test(url)) return match;

      const label = decodeBasicHtmlEntities(rawLabel.replace(/<[^>]*>/g, " "))
        .replace(/\s+/g, " ")
        .trim();
      if (!label) return match;

      return `[${escapeMarkdownLinkText(label)}](<${escapeMarkdownAngleUrl(url)}>)`;
    },
  );
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

const REFERENCE_HEADING_RE =
  /^\s{0,3}#{1,6}\s*(?:参考文献|References|参考资料|Sources|引用)(?:\s*[\/／]\s*(?:References|参考文献|Sources|参考资料|引用))?(?:\s*\(\d+\))?\s*$/i;

export interface TerminalReferencesSplit {
  main: string;
  references: string;
  heading: string;
  count: number;
}

function cleanReferenceHeading(line: string): string {
  return line.replace(/^\s{0,3}#{1,6}\s*/, "").trim();
}

function isFenceBoundary(line: string): boolean {
  return /^\s*(?:```|~~~)/.test(line);
}

export function countReferenceEntries(references: string): number {
  const lines = references.split("\n").map((line) => line.trim()).filter(Boolean);
  const numbered = lines.filter((line) =>
    /^(?:\d+[\.)]|\[\d+\]|\[\[\d+\]\]\([^)]+\))\s+/.test(line),
  ).length;
  if (numbered > 0) return numbered;

  const bullets = lines.filter((line) => /^[-*+]\s+/.test(line)).length;
  if (bullets > 0) return bullets;

  const urls = references.match(/https?:\/\/[^\s)>\]]+/gi) ?? [];
  if (urls.length > 0) return new Set(urls.map((url) => url.toLowerCase())).size;

  return (references.match(/\b10\.\d{4,9}\/\S+/g) ?? []).length;
}

export function splitTerminalReferences(md: string): TerminalReferencesSplit | null {
  const lines = md.split("\n");
  const headingIndexes: number[] = [];
  let inFence = false;

  lines.forEach((line, index) => {
    if (isFenceBoundary(line)) {
      inFence = !inFence;
      return;
    }
    if (!inFence && REFERENCE_HEADING_RE.test(line.trim())) {
      headingIndexes.push(index);
    }
  });

  for (let i = headingIndexes.length - 1; i >= 0; i--) {
    const headingIndex = headingIndexes[i];
    const references = lines.slice(headingIndex + 1).join("\n").trim();
    if (!references) continue;
    return {
      main: lines.slice(0, headingIndex).join("\n").trimEnd(),
      references,
      heading: cleanReferenceHeading(lines[headingIndex]),
      count: countReferenceEntries(references),
    };
  }

  return null;
}

function ReferencesAccordion({
  split,
  components,
  useMath,
  chat,
  isAgent,
}: {
  split: TerminalReferencesSplit;
  components: Components;
  useMath: boolean;
  chat: ChatTokenSet;
  isAgent: boolean;
}) {
  const theme = useTheme();
  const referenceId = useId();
  const isDark = theme.palette.mode === "dark";
  const summaryBg = isDark
    ? alpha(theme.palette.common.white, 0.04)
    : alpha(theme.palette.common.black, 0.025);
  const summaryHover = isDark
    ? alpha(theme.palette.common.white, 0.08)
    : alpha(theme.palette.common.black, 0.05);
  const border = isDark
    ? alpha(theme.palette.common.white, 0.22)
    : alpha(theme.palette.common.black, 0.45);
  const label = `${split.heading || "References"}${split.count ? ` (${split.count})` : ""}`;

  return (
    <Box sx={{ mt: 2 }}>
      <Accordion
        defaultExpanded
        disableGutters
        elevation={0}
        sx={{
          bgcolor: "transparent",
          color: isAgent ? chat.textPrimary : undefined,
          "&:before": { display: "none" },
        }}
      >
        <AccordionSummary
          expandIcon={<ExpandMore sx={{ fontSize: 20 }} />}
          aria-controls={`${referenceId}-content`}
          id={`${referenceId}-header`}
          sx={{
            display: "inline-flex",
            width: "fit-content",
            minHeight: 38,
            px: 1.5,
            py: 0,
            border: `1px solid ${border}`,
            borderRadius: 999,
            bgcolor: summaryBg,
            transition: "background 150ms ease, border-color 150ms ease",
            "&:hover": { bgcolor: summaryHover },
            "&.Mui-expanded": { minHeight: 38 },
            "& .MuiAccordionSummary-content": {
              my: 0,
              alignItems: "center",
            },
            "& .MuiAccordionSummary-content.Mui-expanded": { my: 0 },
            "& .MuiAccordionSummary-expandIconWrapper": {
              color: "text.secondary",
              ml: 0.75,
            },
          }}
        >
          <Typography sx={{ fontSize: 14, fontWeight: 500, lineHeight: 1.2 }}>
            {label}
          </Typography>
        </AccordionSummary>
        <AccordionDetails
          id={`${referenceId}-content`}
          sx={{
            px: 0,
            pt: 1.25,
            pb: 0,
            "& ol, & ul": { pl: 3, my: 1 },
            "& li": { my: 0.75, lineHeight: 1.55 },
            "& p": { my: 0.5 },
          }}
        >
          <ReactMarkdown
            remarkPlugins={useMath ? GFM_MATH_PLUGINS : GFM_PLUGINS}
            rehypePlugins={useMath ? KATEX_PLUGINS : []}
            components={components}
          >
            {split.references}
          </ReactMarkdown>
        </AccordionDetails>
      </Accordion>
    </Box>
  );
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
  const referenceSplit = useMemo(() => splitTerminalReferences(normalized), [normalized]);
  const bodyMarkdown = referenceSplit?.main ?? normalized;
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
        {bodyMarkdown}
      </ReactMarkdown>
      {referenceSplit && (
        <ReferencesAccordion
          split={referenceSplit}
          components={components}
          useMath={useMath}
          chat={chat}
          isAgent={isAgent}
        />
      )}
    </Box>
  );
});
