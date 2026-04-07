import {
  List,
  ListItem,
  ListItemIcon,
  ListItemText,
  Typography,
} from "@mui/material";
import {
  CheckBox,
  CheckBoxOutlineBlank,
  ErrorOutline,
  RadioButtonUnchecked,
} from "@mui/icons-material";
import { alpha } from "@mui/material/styles";

export interface PlanTodoItem {
  id: string;
  name: string;
  status: "pending" | "running" | "completed" | "error";
}

interface PlanTodoListProps {
  items: PlanTodoItem[];
}

/**
 * Plan 模式：仅展示待办标题与状态，无展开/折叠、无长文本。
 */
export function PlanTodoList({ items }: PlanTodoListProps) {
  if (items.length === 0) {
    return (
      <Typography variant="body2" color="text.secondary" sx={{ fontSize: 12, py: 0.5 }}>
        暂无计划项。发送消息后，模型可通过待办工具更新清单。
      </Typography>
    );
  }

  return (
    <List dense disablePadding sx={{ py: 0 }}>
      {items.map((item) => {
        const isDone = item.status === "completed";
        const isRun = item.status === "running";
        const isErr = item.status === "error";
        return (
          <ListItem
            key={item.id}
            sx={{
              alignItems: "flex-start",
              py: 0.75,
              px: 0.5,
              borderRadius: 1,
              bgcolor: isRun ? alpha("#6366f1", 0.06) : "transparent",
            }}
          >
            <ListItemIcon sx={{ minWidth: 32, mt: 0.15 }}>
              {isDone ? (
                <CheckBox sx={{ fontSize: 20, color: "success.main" }} />
              ) : isErr ? (
                <ErrorOutline sx={{ fontSize: 20, color: "error.main" }} />
              ) : isRun ? (
                <CheckBoxOutlineBlank
                  sx={{ fontSize: 20, color: "primary.main" }}
                />
              ) : (
                <RadioButtonUnchecked
                  sx={{ fontSize: 20, color: alpha("#000", 0.25) }}
                />
              )}
            </ListItemIcon>
            <ListItemText
              primary={
                <Typography
                  variant="body2"
                  sx={{
                    fontSize: 12.5,
                    lineHeight: 1.4,
                    fontWeight: isRun ? 600 : 400,
                    color: isDone ? "text.secondary" : "text.primary",
                    textDecoration: isDone ? "line-through" : "none",
                  }}
                >
                  {item.name}
                </Typography>
              }
            />
          </ListItem>
        );
      })}
    </List>
  );
}
