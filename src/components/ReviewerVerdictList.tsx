import { useMemo, useState } from "react";
import { Box, Button, Chip, Stack, Typography } from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import {
  isBlockerVerdict,
  latestReviewerVerdicts,
  reviewerVerdictColor,
  type ReviewerVerdictChip,
} from "../utils/reviewerVerdict";
import { normalizeAgentDisplayName } from "../state/agentStore";
import { MarkdownTextViewer } from "./MarkdownText";
import { RightDetailDrawer } from "./RightDetailDrawer";

function verdictRank(verdict: string): number {
  switch (verdict) {
    case "reject":
      return 0;
    case "fail":
      return 1;
    case "partial":
      return 2;
    case "pass":
      return 3;
    default:
      return 4;
  }
}

function severityRank(severity: string): number {
  switch (severity) {
    case "critical":
      return 0;
    case "high":
      return 1;
    case "medium":
      return 2;
    case "low":
      return 3;
    default:
      return 4;
  }
}

function verdictLabel(verdict: string): string {
  switch (verdict) {
    case "reject":
      return "REJECT";
    case "fail":
      return "FAIL";
    case "partial":
      return "PARTIAL";
    case "pass":
      return "PASS";
    default:
      return "INFO";
  }
}

function severityLabel(severity: string): string {
  return (severity || "info").toUpperCase();
}

function statusLabel(status: ReviewerVerdictChip["taskStatus"]): string {
  switch (status) {
    case "Running":
      return "运行中";
    case "Completed":
      return "已完成";
    case "Failed":
      return "失败";
    case "Cancelled":
      return "已取消";
    default:
      return "待处理";
  }
}

function compactTaskDescription(text: string | undefined): string | null {
  const value = text?.trim();
  if (!value) return null;
  return value.length > 88 ? `${value.slice(0, 88)}…` : value;
}

interface ReviewerVerdictListProps {
  verdicts: ReviewerVerdictChip[];
  title?: string;
  onSelectVerdict?: (verdict: ReviewerVerdictChip) => void;
}

export function ReviewerVerdictList({
  verdicts,
  title = "Reviewer 详细结论：",
  onSelectVerdict,
}: ReviewerVerdictListProps) {
  const theme = useTheme();
  const [latestOnly, setLatestOnly] = useState(true);
  const [blockersOnly, setBlockersOnly] = useState(false);
  const [selectedVerdict, setSelectedVerdict] =
    useState<ReviewerVerdictChip | null>(null);

  if (verdicts.length === 0) return null;

  const latestVerdicts = useMemo(
    () => latestReviewerVerdicts(verdicts),
    [verdicts],
  );
  const visibleVerdicts = useMemo(() => {
    let items = latestOnly ? latestVerdicts : verdicts;
    if (blockersOnly) {
      items = items.filter(isBlockerVerdict);
    }
    return items;
  }, [blockersOnly, latestOnly, latestVerdicts, verdicts]);

  const sortedVerdicts = [...visibleVerdicts].sort((a, b) => {
    const verdictDelta = verdictRank(a.verdict) - verdictRank(b.verdict);
    if (verdictDelta !== 0) return verdictDelta;
    return severityRank(a.severity) - severityRank(b.severity);
  });

  const blockerCount = useMemo(
    () => verdicts.filter(isBlockerVerdict).length,
    [verdicts],
  );
  const hasMultipleRuns = latestVerdicts.length !== verdicts.length;
  const selectedColor = selectedVerdict
    ? reviewerVerdictColor(selectedVerdict.verdict, selectedVerdict.severity)
    : theme.palette.text.secondary;

  return (
    <Box sx={{ mt: 1 }}>
      <Stack
        direction="row"
        alignItems="center"
        spacing={0.75}
        flexWrap="wrap"
        useFlexGap
        sx={{ mb: 0.75 }}
      >
        <Typography
          variant="caption"
          sx={{ color: "text.secondary", display: "block" }}
        >
          {title}
        </Typography>
        {hasMultipleRuns && (
          <Chip
            size="small"
            label={
              latestOnly
                ? `按任务最新 ${latestVerdicts.length}`
                : `全部记录 ${verdicts.length}`
            }
            color={latestOnly ? "primary" : "default"}
            variant={latestOnly ? "filled" : "outlined"}
            onClick={() => setLatestOnly((prev) => !prev)}
            sx={{ height: 18, fontSize: 9, cursor: "pointer" }}
          />
        )}
        {blockerCount > 0 && (
          <Chip
            size="small"
            label={blockersOnly ? `仅 blocker ${blockerCount}` : `Blocker ${blockerCount}`}
            color={blockersOnly ? "warning" : "default"}
            variant={blockersOnly ? "filled" : "outlined"}
            onClick={() => setBlockersOnly((prev) => !prev)}
            sx={{ height: 18, fontSize: 9, cursor: "pointer" }}
          />
        )}
      </Stack>

      {sortedVerdicts.length === 0 ? (
        <Typography variant="caption" color="text.secondary">
          当前筛选条件下暂无 reviewer 结论。
        </Typography>
      ) : (
        <Stack spacing={0.75}>
          {sortedVerdicts.map((item, index) => {
            const color = reviewerVerdictColor(item.verdict, item.severity);
            const taskDescription = compactTaskDescription(item.taskDescription);
            return (
              <Box
                key={`${item.agentType}-${item.verdict}-${item.severity}-${index}`}
                onClick={() => setSelectedVerdict(item)}
                sx={{
                  p: 0.9,
                  borderRadius: 1.25,
                  border: `1px solid ${alpha(color, 0.22)}`,
                  bgcolor: alpha(color, 0.06),
                  cursor: "pointer",
                  transition:
                    "transform 0.15s ease, background-color 0.15s ease",
                  "&:hover": {
                    bgcolor: alpha(color, 0.1),
                    transform: "translateY(-1px)",
                  },
                }}
              >
                <Stack
                  direction="row"
                  spacing={0.5}
                  flexWrap="wrap"
                  useFlexGap
                  sx={{ mb: 0.5 }}
                >
                  <Chip
                    size="small"
                    label={normalizeAgentDisplayName(item.agentType)}
                    sx={{
                      height: 18,
                      fontSize: 9,
                      bgcolor: alpha(theme.palette.text.primary, 0.06),
                    }}
                  />
                  <Chip
                    size="small"
                    label={verdictLabel(item.verdict)}
                    sx={{
                      height: 18,
                      fontSize: 9,
                      fontWeight: 700,
                      bgcolor: alpha(color, 0.14),
                      color,
                    }}
                  />
                  <Chip
                    size="small"
                    label={severityLabel(item.severity)}
                    variant="outlined"
                    sx={{
                      height: 18,
                      fontSize: 9,
                      borderColor: alpha(color, 0.32),
                      color,
                    }}
                  />
                  {item.taskStatus && (
                    <Chip
                      size="small"
                      label={statusLabel(item.taskStatus)}
                      variant="outlined"
                      sx={{
                        height: 18,
                        fontSize: 9,
                        borderColor: alpha(theme.palette.text.primary, 0.18),
                        color: "text.secondary",
                      }}
                    />
                  )}
                </Stack>
                {taskDescription && (
                  <Typography
                    variant="caption"
                    sx={{
                      display: "block",
                      mb: 0.35,
                      color: "text.secondary",
                      fontWeight: 600,
                      lineHeight: 1.35,
                    }}
                  >
                    {taskDescription}
                  </Typography>
                )}
                <Typography
                  variant="caption"
                  sx={{
                    display: "block",
                    lineHeight: 1.45,
                    color: "text.primary",
                    whiteSpace: "pre-wrap",
                    wordBreak: "break-word",
                  }}
                >
                  {item.summary}
                </Typography>
                <Typography
                  variant="caption"
                  sx={{
                    display: "block",
                    mt: 0.5,
                    color,
                    fontWeight: 600,
                  }}
                >
                  点击查看 reviewer 详情
                </Typography>
              </Box>
            );
          })}
        </Stack>
      )}
      <RightDetailDrawer
        open={selectedVerdict !== null}
        onClose={() => setSelectedVerdict(null)}
        title="Reviewer 结论"
        subtitle={
          selectedVerdict
            ? `${normalizeAgentDisplayName(selectedVerdict.agentType)} · ${verdictLabel(selectedVerdict.verdict)}`
            : undefined
        }
        width={500}
        titleWeight={700}
        titleAlign="flex-start"
      >
        {selectedVerdict && (
          <Stack spacing={1.5}>
            <Box
              sx={{
                p: 1.25,
                borderRadius: 2,
                border: `1px solid ${alpha(selectedColor, 0.22)}`,
                bgcolor: alpha(selectedColor, 0.06),
              }}
            >
              <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap sx={{ mb: 0.75 }}>
                <Chip
                  size="small"
                  label={normalizeAgentDisplayName(selectedVerdict.agentType)}
                  sx={{ height: 20, fontSize: 10 }}
                />
                <Chip
                  size="small"
                  label={verdictLabel(selectedVerdict.verdict)}
                  sx={{
                    height: 20,
                    fontSize: 10,
                    fontWeight: 700,
                    bgcolor: alpha(selectedColor, 0.14),
                    color: selectedColor,
                  }}
                />
                <Chip
                  size="small"
                  label={severityLabel(selectedVerdict.severity)}
                  variant="outlined"
                  sx={{
                    height: 20,
                    fontSize: 10,
                    borderColor: alpha(selectedColor, 0.32),
                    color: selectedColor,
                  }}
                />
                {selectedVerdict.taskStatus && (
                  <Chip
                    size="small"
                    label={statusLabel(selectedVerdict.taskStatus)}
                    variant="outlined"
                    sx={{ height: 20, fontSize: 10 }}
                  />
                )}
              </Stack>
              {selectedVerdict.taskDescription && (
                <Typography
                  variant="body2"
                  color="text.secondary"
                  sx={{ whiteSpace: "pre-wrap", wordBreak: "break-word" }}
                >
                  {selectedVerdict.taskDescription}
                </Typography>
              )}
              <Typography
                variant="body2"
                sx={{
                  mt: 1,
                  color: selectedColor,
                  fontWeight: 700,
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-word",
                }}
              >
                {selectedVerdict.summary}
              </Typography>
              {selectedVerdict.taskId && onSelectVerdict && (
                <Button
                  size="small"
                  variant="outlined"
                  onClick={() => onSelectVerdict(selectedVerdict)}
                  sx={{ mt: 1, fontSize: 11 }}
                >
                  打开队友记录
                </Button>
              )}
            </Box>
            <Box
              sx={{
                p: 1.25,
                borderRadius: 2,
                border: `1px solid ${alpha(theme.palette.text.primary, 0.08)}`,
                bgcolor: alpha(theme.palette.warning.main, 0.06),
              }}
            >
              <Typography variant="caption" color="warning.dark" fontWeight={700}>
                原始 reviewer 输出
              </Typography>
              <Box sx={{ mt: 0.75 }}>
                <MarkdownTextViewer>
                  {selectedVerdict.rawText || selectedVerdict.summary}
                </MarkdownTextViewer>
              </Box>
            </Box>
          </Stack>
        )}
      </RightDetailDrawer>
    </Box>
  );
}
