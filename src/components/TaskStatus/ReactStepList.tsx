import {
  Box,
  List,
  ListItem,
  ListItemIcon,
  ListItemText,
  Typography,
  CircularProgress,
  useTheme,
  Stack,
  Chip,
} from "@mui/material";
import { CheckCircle, CloudQueue, RadioButtonUnchecked, WarningAmber } from "@mui/icons-material";
import { alpha } from "@mui/material/styles";
import type { ExecutionStep } from "../../state/activityStore";
import {
  formatToolDisplayName,
  getExecutionSurfacePrimaryLabel,
  type ExecutionSurfaceContext,
} from "../../utils/executionSurfaceLabel";
import { compactLabel } from "../../utils/compactLabel";

const ACCENT = "#6366f1";
const BACKGROUND_RUNNING_ACCENT = "#f97316";

function stepLineLabel(step: ExecutionStep): string {
  if (step.id.startsWith("tool-")) {
    const tn = step.toolName?.trim();
    if (tn) return formatToolDisplayName(tn);
  }
  return step.title;
}

interface StepDisplayRow {
  id: string;
  label: string;
  status: ExecutionStep["status"];
  failed: boolean;
  background: boolean;
  count: number;
  output?: string;
}

function isBackgroundOperationStep(step: ExecutionStep): boolean {
  return step.id.startsWith("op-");
}

function buildDisplayRows(steps: ExecutionStep[]): StepDisplayRow[] {
  const rows: StepDisplayRow[] = [];
  for (const step of steps) {
    const label = stepLineLabel(step);
    const failed = Boolean(step.failed);
    const background = isBackgroundOperationStep(step);
    const previous = rows[rows.length - 1];
    if (
      previous &&
      previous.status === "done" &&
      step.status === "done" &&
      previous.label === label &&
      previous.failed === failed &&
      previous.background === background
    ) {
      previous.count += 1;
      continue;
    }
    rows.push({
      id: step.id,
      label,
      status: step.status,
      failed,
      background,
      count: 1,
      output: step.toolOutput,
    });
  }
  return rows;
}

function buildGroupedFailureRows(steps: ExecutionStep[]): StepDisplayRow[] {
  const rows: StepDisplayRow[] = [];
  const byLabel = new Map<string, StepDisplayRow>();
  for (const step of [...steps].reverse()) {
    if (step.status !== "done" || !step.failed) continue;
    const label = stepLineLabel(step);
    const background = isBackgroundOperationStep(step);
    const existing = byLabel.get(label);
    if (existing) {
      existing.count += 1;
      continue;
    }
    const row: StepDisplayRow = {
      id: step.id,
      label,
      status: step.status,
      failed: true,
      background,
      count: 1,
      output: step.toolOutput,
    };
    byLabel.set(label, row);
    rows.push(row);
  }
  return rows;
}

interface ReactStepListProps {
  steps: ExecutionStep[];
  elapsedLabel: string;
  surfaceContext: ExecutionSurfaceContext;
}

/**
 * ReAct 模式：仅步骤标题/短描述，无 Accordion、无参数与输出。
 */
export function ReactStepList({
  steps,
  elapsedLabel,
  surfaceContext,
}: ReactStepListProps) {
  const theme = useTheme();
  const doneColor = theme.palette.primary.main;
  if (steps.length === 0) return null;

  const current = getExecutionSurfacePrimaryLabel(steps, surfaceContext);
  const rows = buildDisplayRows(steps);
  const runningRows = rows.filter((row) => row.status === "running");
  const failedRows = buildGroupedFailureRows(steps);
  const completedRows = rows
    .filter((row) => row.status === "done" && !row.failed)
    .reverse();
  const successfulToolCount = steps.filter(
    (step) => step.id.startsWith("tool-") && step.status === "done" && !step.failed,
  ).length;
  const failedToolCount = steps.filter(
    (step) => step.id.startsWith("tool-") && step.status === "done" && step.failed,
  ).length;

  const renderRows = (items: StepDisplayRow[]) => (
    <List dense disablePadding>
      {items.map((row) => {
        const isDone = row.status === "done";
        const isRun = row.status === "running";
        const isBackgroundRun = row.background && isRun;
        return (
          <ListItem
            key={row.id}
            sx={{
              py: 0.35,
              px: 0.5,
              alignItems: "flex-start",
              ...(isBackgroundRun
                ? {
                    borderRadius: 1,
                    bgcolor: alpha(BACKGROUND_RUNNING_ACCENT, 0.07),
                    outline: `1px solid ${alpha(BACKGROUND_RUNNING_ACCENT, 0.18)}`,
                  }
                : {}),
            }}
          >
            <ListItemIcon sx={{ minWidth: 28, mt: 0.2 }}>
              {row.failed ? (
                <WarningAmber sx={{ fontSize: 16, color: theme.palette.error.main }} />
              ) : isDone ? (
                <CheckCircle sx={{ fontSize: 16, color: doneColor }} />
              ) : isBackgroundRun ? (
                <CloudQueue sx={{ fontSize: 16, color: BACKGROUND_RUNNING_ACCENT }} />
              ) : isRun ? (
                <CircularProgress size={14} thickness={5} sx={{ color: ACCENT }} />
              ) : (
                <RadioButtonUnchecked sx={{ fontSize: 16, color: "action.disabled" }} />
              )}
            </ListItemIcon>
            <ListItemText
              primary={
                <Stack direction="row" spacing={0.5} alignItems="center" sx={{ minWidth: 0 }}>
                  <Typography
                    variant="body2"
                    sx={{
                      fontSize: 12,
                      lineHeight: 1.35,
                      fontWeight: isRun ? 600 : 400,
                      color: row.failed
                        ? "error.main"
                        : isBackgroundRun
                          ? BACKGROUND_RUNNING_ACCENT
                          : isRun
                          ? "primary.main"
                          : "text.primary",
                      minWidth: 0,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                    title={row.label}
                  >
                    {row.label}
                  </Typography>
                  {row.count > 1 && (
                    <Chip
                      size="small"
                      label={`×${row.count}`}
                      sx={{
                        height: 16,
                        fontSize: 8.5,
                        flexShrink: 0,
                        bgcolor: row.failed ? alpha(theme.palette.error.main, 0.08) : undefined,
                      }}
                    />
                  )}
                  {isBackgroundRun && (
                    <Chip
                      size="small"
                      label="后台"
                      sx={{
                        height: 16,
                        fontSize: 8.5,
                        flexShrink: 0,
                        fontWeight: 700,
                        bgcolor: alpha(BACKGROUND_RUNNING_ACCENT, 0.12),
                        color: BACKGROUND_RUNNING_ACCENT,
                        "& .MuiChip-label": { px: 0.55 },
                      }}
                    />
                  )}
                </Stack>
              }
              secondary={
                row.failed && row.output ? (
                  <Typography
                    variant="caption"
                    color="text.secondary"
                    sx={{
                      display: "block",
                      fontSize: 9,
                      lineHeight: 1.35,
                      mt: 0.1,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                    title={row.output}
                  >
                    {compactLabel(row.output, 84)}
                  </Typography>
                ) : undefined
              }
            />
          </ListItem>
        );
      })}
    </List>
  );

  return (
    <Box>
      <Box
        sx={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 1,
          mb: 1,
        }}
      >
        <Typography
          variant="body2"
          fontWeight={600}
          color="text.primary"
          noWrap
          sx={{ flex: 1, minWidth: 0, fontSize: 13 }}
          title={current}
        >
          {current}
        </Typography>
        <Typography
          variant="caption"
          color="text.secondary"
          sx={{ fontVariantNumeric: "tabular-nums", flexShrink: 0 }}
        >
          {elapsedLabel}
        </Typography>
      </Box>

      <Stack spacing={1}>
        {runningRows.length > 0 && (
          <Box>
            <Typography variant="caption" color="primary.main" sx={{ fontSize: 10, fontWeight: 700 }}>
              正在执行
            </Typography>
            {renderRows(runningRows)}
          </Box>
        )}
        {failedRows.length > 0 && (
          <Box>
            <Stack direction="row" alignItems="center" spacing={0.5} sx={{ mb: 0.15 }}>
              <Typography variant="caption" color="warning.main" sx={{ fontSize: 10, fontWeight: 700 }}>
                异常返回（模型已接收）
              </Typography>
              {failedToolCount > 0 && (
                <Chip
                  size="small"
                  label={`${failedToolCount} 次异常`}
                  sx={{ height: 16, fontSize: 8.5 }}
                />
              )}
            </Stack>
            <Typography
              variant="caption"
              color="text.secondary"
              sx={{ display: "block", fontSize: 9, lineHeight: 1.35, mb: 0.25 }}
            >
              这些是工具调用返回的错误，已写入上下文让模型继续修正；只有权限确认或等待用户输入时才需要手动介入。
            </Typography>
            {renderRows(failedRows)}
          </Box>
        )}
        {completedRows.length > 0 && (
          <Box>
            <Stack direction="row" alignItems="center" spacing={0.5} sx={{ mb: 0.15 }}>
              <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10, fontWeight: 700 }}>
                成功调用（最近优先）
              </Typography>
              {successfulToolCount > 0 && (
                <Chip
                  size="small"
                  label={`${successfulToolCount} 次成功`}
                  sx={{ height: 16, fontSize: 8.5 }}
                />
              )}
            </Stack>
            {renderRows(completedRows)}
          </Box>
        )}
      </Stack>
    </Box>
  );
}
