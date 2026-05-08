import { useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Box, Button, Stack } from "@mui/material";
import { alpha } from "@mui/material/styles";

export function MarkdownText({
  children,
  compact = false,
  color = "text.secondary",
}: {
  children: string;
  compact?: boolean;
  color?: string;
}) {
  return (
    <Box
      sx={{
        color,
        fontSize: compact ? 10 : 12,
        lineHeight: compact ? 1.45 : 1.7,
        overflow: compact ? "hidden" : "visible",
        maxHeight: compact ? 74 : "none",
        "& > :first-of-type": { mt: 0 },
        "& > :last-child": { mb: 0 },
        "& p": { my: compact ? 0.25 : 0.75 },
        "& h1, & h2, & h3, & h4": {
          mt: compact ? 0.25 : 1.25,
          mb: compact ? 0.25 : 0.75,
          color: "text.primary",
          fontWeight: 700,
          lineHeight: 1.25,
        },
        "& h1": { fontSize: compact ? 12 : 18 },
        "& h2": { fontSize: compact ? 11.5 : 16 },
        "& h3": { fontSize: compact ? 11 : 14 },
        "& h4": { fontSize: compact ? 10.5 : 13 },
        "& ul, & ol": { pl: compact ? 2 : 2.5, my: compact ? 0.25 : 0.75 },
        "& li": { my: compact ? 0.1 : 0.25 },
        "& strong": { color: "text.primary", fontWeight: 700 },
        "& code": {
          px: 0.4,
          py: 0.1,
          borderRadius: 0.75,
          bgcolor: (theme) => alpha(theme.palette.text.primary, 0.08),
          fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
          fontSize: "0.92em",
        },
        "& pre": {
          m: compact ? "4px 0" : "8px 0",
          p: compact ? 0.75 : 1,
          borderRadius: 1,
          bgcolor: (theme) => alpha(theme.palette.text.primary, 0.08),
          overflowX: "auto",
        },
        "& pre code": {
          p: 0,
          bgcolor: "transparent",
          fontSize: compact ? 10 : 12,
        },
        "& blockquote": {
          my: compact ? 0.4 : 0.8,
          mx: 0,
          pl: 1,
          borderLeft: "3px solid",
          borderColor: "divider",
          color: "text.secondary",
        },
        "& table": {
          borderCollapse: "collapse",
          width: "100%",
          my: compact ? 0.5 : 1,
          fontSize: compact ? 10 : 12,
        },
        "& th, & td": {
          border: "1px solid",
          borderColor: "divider",
          px: 0.75,
          py: 0.5,
          verticalAlign: "top",
        },
        "& th": {
          bgcolor: (theme) => alpha(theme.palette.text.primary, 0.06),
          color: "text.primary",
          fontWeight: 700,
        },
      }}
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{children || " "}</ReactMarkdown>
    </Box>
  );
}

export function MarkdownTextViewer({
  children,
  copyText,
  color = "text.secondary",
}: {
  children: string;
  copyText?: string;
  color?: string;
}) {
  const [raw, setRaw] = useState(false);
  const [copied, setCopied] = useState(false);
  const text = children ?? "";
  const textToCopy = copyText ?? text;

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(textToCopy);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      /* ignore clipboard failures */
    }
  };

  return (
    <Box>
      <Stack direction="row" spacing={0.5} alignItems="center" sx={{ mb: 0.75 }}>
        <Button
          size="small"
          variant={!raw ? "contained" : "outlined"}
          onClick={() => setRaw(false)}
          sx={{ minWidth: 0, fontSize: 11, py: 0.15, px: 0.8 }}
        >
          格式化
        </Button>
        <Button
          size="small"
          variant={raw ? "contained" : "outlined"}
          onClick={() => setRaw(true)}
          sx={{ minWidth: 0, fontSize: 11, py: 0.15, px: 0.8 }}
        >
          原文
        </Button>
        <Button
          size="small"
          variant="text"
          onClick={handleCopy}
          sx={{ minWidth: 0, fontSize: 11, py: 0.15, px: 0.8 }}
        >
          {copied ? "已复制" : "复制"}
        </Button>
      </Stack>

      {raw ? (
        <Box
          component="pre"
          sx={{
            m: 0,
            p: 1,
            borderRadius: 1,
            color,
            bgcolor: (theme) => alpha(theme.palette.text.primary, 0.08),
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
            overflowX: "auto",
            fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
            fontSize: 12,
            lineHeight: 1.65,
          }}
        >
          {text || " "}
        </Box>
      ) : (
        <MarkdownText color={color}>{text}</MarkdownText>
      )}
    </Box>
  );
}
