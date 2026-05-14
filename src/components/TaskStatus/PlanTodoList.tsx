import {
  List,
  ListItem,
  ListItemIcon,
  ListItemText,
  Stack,
  Typography,
} from "@mui/material";
import {
  CheckBoxOutlineBlank,
  ErrorOutline,
  RadioButtonUnchecked,
  CheckCircle,
} from "@mui/icons-material";
import { alpha } from "@mui/material/styles";
import { formatStepElapsedLabel } from "../ExecutionStepPanel";

export interface PlanTodoItem {
  id: string;
  name: string;
  status: "pending" | "running" | "completed" | "error";
  /** Unix ms when this plan item entered execution. */
  startedAt?: number;
  /** Unix ms when this plan item finished or failed. */
  completedAt?: number;
}

export function planTodoRuntimeLabel(
  item: Pick<PlanTodoItem, "status" | "startedAt" | "completedAt">,
): string | null {
  if (item.startedAt == null) return null;
  return formatStepElapsedLabel(
    item.startedAt,
    item.status === "running" ? null : item.completedAt,
  );
}

interface PlanTodoListProps {
  items: PlanTodoItem[];
  compact?: boolean;
  onItemClick?: (item: PlanTodoItem) => void;
}

/**
 * Plan 模式：待办列表组件
 * - completed: 带删除线的完成项
 * - running: 高亮当前执行项
 * - pending: 待办项
 * - error: 错误项
 */
export function PlanTodoList({
  items,
  compact = false,
  onItemClick,
}: PlanTodoListProps) {
  if (items.length === 0) {
    return (
      <Typography variant="body2" color="text.secondary" sx={{ fontSize: 12, py: 0.5 }}>
        暂无计划项
      </Typography>
    );
  }

  return (
    <List dense disablePadding sx={{ py: 0 }}>
      {items.map((item) => {
        const isDone = item.status === "completed";
        const isRun = item.status === "running";
        const isErr = item.status === "error";
        const clickable = Boolean(onItemClick);
        const runtimeLabel = planTodoRuntimeLabel(item);
        
        return (
          <ListItem
            key={item.id}
            onClick={clickable ? () => onItemClick?.(item) : undefined}
            onKeyDown={
              clickable
                ? (event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      onItemClick?.(item);
                    }
                  }
                : undefined
            }
            role={clickable ? "button" : undefined}
            tabIndex={clickable ? 0 : undefined}
            sx={{
              alignItems: "flex-start",
              py: compact ? 0.5 : 0.75,
              px: compact ? 0 : 0.5,
              borderRadius: 1,
              bgcolor: isRun ? alpha("#6366f1", 0.06) : "transparent",
              opacity: isDone ? 0.7 : 1,
              cursor: clickable ? "pointer" : "default",
              transition: "background-color 0.18s ease, border-color 0.18s ease",
              "&:hover": clickable
                ? {
                    bgcolor: isRun
                      ? alpha("#6366f1", 0.08)
                      : alpha("#6366f1", 0.035),
                  }
                : undefined,
            }}
          >
            <ListItemIcon sx={{ minWidth: compact ? 24 : 28, mt: compact ? 0.1 : 0.15 }}>
              {isDone ? (
                <CheckCircle 
                  sx={{ 
                    fontSize: compact ? 16 : 18, 
                    color: "success.main",
                  }} 
                />
              ) : isErr ? (
                <ErrorOutline 
                  sx={{ 
                    fontSize: compact ? 16 : 18, 
                    color: "error.main" 
                  }} 
                />
              ) : isRun ? (
                <CheckBoxOutlineBlank
                  sx={{ 
                    fontSize: compact ? 16 : 18, 
                    color: "primary.main",
                    animation: "pulse 2s ease-in-out infinite",
                    "@keyframes pulse": {
                      "0%, 100%": { opacity: 1 },
                      "50%": { opacity: 0.6 },
                    },
                  }}
                />
              ) : (
                <RadioButtonUnchecked
                  sx={{ 
                    fontSize: compact ? 16 : 18, 
                    color: alpha("#000", 0.2) 
                  }}
                />
              )}
            </ListItemIcon>
            <ListItemText
              primary={
                <Stack
                  direction="row"
                  spacing={0.75}
                  alignItems="baseline"
                  sx={{ minWidth: 0 }}
                >
                  <Typography
                    variant="body2"
                    sx={{
                      fontSize: compact ? 11 : 12,
                      lineHeight: 1.4,
                      fontWeight: isRun ? 600 : isDone ? 400 : 500,
                      color: isErr
                        ? "error.main"
                        : isDone
                          ? "text.disabled"
                          : "text.primary",
                      textDecoration: isDone ? "line-through" : "none",
                      minWidth: 0,
                      flex: 1,
                    }}
                  >
                    {item.name}
                  </Typography>
                  {runtimeLabel && (
                    <Typography
                      variant="caption"
                      color="text.secondary"
                      sx={{
                        fontSize: 9,
                        fontVariantNumeric: "tabular-nums",
                        flexShrink: 0,
                      }}
                      title={isRun ? "当前步骤已运行时间" : "步骤执行耗时"}
                    >
                      {runtimeLabel}
                    </Typography>
                  )}
                </Stack>
              }
            />
          </ListItem>
        );
      })}
    </List>
  );
}

/** 简洁版待办列表 - 仅显示已完成项目 */
export function CompletedTodoList({ items }: { items: PlanTodoItem[] }) {
  const completed = items.filter(i => i.status === "completed");
  
  if (completed.length === 0) return null;
  
  return (
    <PlanTodoList items={completed} compact />
  );
}

/** 分组待办列表 - 按状态分组显示 */
interface GroupedTodoListProps {
  items: PlanTodoItem[];
}

export function GroupedTodoList({ items }: GroupedTodoListProps) {
  const running = items.filter(i => i.status === "running");
  const pending = items.filter(i => i.status === "pending");
  const completed = items.filter(i => i.status === "completed");
  const error = items.filter(i => i.status === "error");

  return (
    <>
      {running.length > 0 && (
        <PlanTodoList items={running} />
      )}
      {pending.length > 0 && (
        <PlanTodoList items={pending} />
      )}
      {completed.length > 0 && (
        <PlanTodoList items={completed} compact />
      )}
      {error.length > 0 && (
        <PlanTodoList items={error} />
      )}
    </>
  );
}
