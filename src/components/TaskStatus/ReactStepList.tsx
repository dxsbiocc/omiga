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
import { CheckCircle, RadioButtonUnchecked, WarningAmber } from "@mui/icons-material";
import type { ExecutionStep } from "../../state/activityStore";
import {
  formatToolDisplayName,
  getExecutionSurfacePrimaryLabel,
  type ExecutionSurfaceContext,
} from "../../utils/executionSurfaceLabel";

const ACCENT = "#6366f1";

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
  count: number;
}

function buildDisplayRows(steps: ExecutionStep[]): StepDisplayRow[] {
  const rows: StepDisplayRow[] = [];
  for (const step of steps) {
    const label = stepLineLabel(step);
    const failed = Boolean(step.failed);
    const previous = rows[rows.length - 1];
    if (
      previous &&
      previous.status === "done" &&
      step.status === "done" &&
      previous.label === label &&
      previous.failed === failed
    ) {
      previous.count += 1;
      continue;
    }
    rows.push({
      id: step.id,
      label,
      status: step.status,
      failed,
      count: 1,
    });
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
  const failedRows = rows.filter((row) => row.status === "done" && row.failed);
  const completedRows = rows
    .filter((row) => row.status === "done" && !row.failed)
    .reverse();
  const toolCount = steps.filter((step) => step.id.startsWith("tool-")).length;

  const renderRows = (items: StepDisplayRow[]) => (
    <List dense disablePadding>
      {items.map((row) => {
        const isDone = row.status === "done";
        const isRun = row.status === "running";
        return (
          <ListItem key={row.id} sx={{ py: 0.35, px: 0.5, alignItems: "flex-start" }}>
            <ListItemIcon sx={{ minWidth: 28, mt: 0.2 }}>
              {row.failed ? (
                <WarningAmber sx={{ fontSize: 16, color: theme.palette.error.main }} />
              ) : isDone ? (
                <CheckCircle sx={{ fontSize: 16, color: doneColor }} />
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
                      sx={{ height: 16, fontSize: 8.5, flexShrink: 0 }}
                    />
                  )}
                </Stack>
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
              当前
            </Typography>
            {renderRows(runningRows)}
          </Box>
        )}
        {failedRows.length > 0 && (
          <Box>
            <Typography variant="caption" color="error.main" sx={{ fontSize: 10, fontWeight: 700 }}>
              需处理
            </Typography>
            {renderRows(failedRows)}
          </Box>
        )}
        {completedRows.length > 0 && (
          <Box>
            <Stack direction="row" alignItems="center" spacing={0.5} sx={{ mb: 0.15 }}>
              <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10, fontWeight: 700 }}>
                已完成（最近优先）
              </Typography>
              {toolCount > 0 && (
                <Chip
                  size="small"
                  label={`${toolCount} 次工具调用`}
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
