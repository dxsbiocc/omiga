import {
  Box,
  List,
  ListItem,
  ListItemIcon,
  ListItemText,
  Typography,
  CircularProgress,
  useTheme,
} from "@mui/material";
import { CheckCircle, RadioButtonUnchecked } from "@mui/icons-material";
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

      <List dense disablePadding>
        {steps.map((step) => {
          const label = stepLineLabel(step);
          const isDone = step.status === "done";
          const isRun = step.status === "running";
          return (
            <ListItem key={step.id} sx={{ py: 0.4, px: 0.5, alignItems: "flex-start" }}>
              <ListItemIcon sx={{ minWidth: 28, mt: 0.2 }}>
                {isDone ? (
                  <CheckCircle sx={{ fontSize: 16, color: doneColor }} />
                ) : isRun ? (
                  <CircularProgress size={14} thickness={5} sx={{ color: ACCENT }} />
                ) : (
                  <RadioButtonUnchecked sx={{ fontSize: 16, color: "action.disabled" }} />
                )}
              </ListItemIcon>
              <ListItemText
                primary={
                  <Typography
                    variant="body2"
                    sx={{
                      fontSize: 12,
                      lineHeight: 1.35,
                      fontWeight: isRun ? 600 : 400,
                      color: isRun ? "primary.main" : "text.primary",
                    }}
                  >
                    {label}
                  </Typography>
                }
              />
            </ListItem>
          );
        })}
      </List>
    </Box>
  );
}
