import React, { useEffect } from "react";
import {
  Box,
  Typography,
  List,
  ListItem,
  ListItemText,
  ListItemSecondaryAction,
  IconButton,
  Chip,
  Button,
  Paper,
  Divider,
  Tooltip,
} from "@mui/material";
import {
  Delete as DeleteIcon,
  Add as AddIcon,
  Warning as WarningIcon,
} from "@mui/icons-material";
import { usePermissionStore } from "../../state/permissionStore";

const getModeLabel = (mode: any) => {
  if (typeof mode === "object" && mode.timeWindow) {
    return `${mode.timeWindow}分钟内`;
  }
  switch (mode) {
    case "askEveryTime":
      return "每次询问";
    case "session":
      return "本次会话";
    case "plan":
      return "Plan模式";
    case "auto":
      return "自动批准";
    default:
      return "未知";
  }
};

export const PermissionSettings: React.FC = () => {
  const { rules, recentDenials, loadRules, loadRecentDenials, deleteRule } = usePermissionStore();

  useEffect(() => {
    loadRules();
    loadRecentDenials();
  }, [loadRules, loadRecentDenials]);

  return (
    <Box sx={{ p: 2 }}>
      <Typography variant="h6" gutterBottom>
        权限规则
      </Typography>

      <Paper variant="outlined" sx={{ mb: 3 }}>
        <List>
          {rules.length === 0 ? (
            <ListItem>
              <ListItemText
                secondary="暂无自定义规则，危险操作将每次询问"
                secondaryTypographyProps={{ align: "center" }}
              />
            </ListItem>
          ) : (
            rules.map((rule) => (
              <ListItem key={rule.id} divider>
                <ListItemText
                  primary={
                    <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                      {rule.name}
                      <Chip
                        label={getModeLabel(rule.mode)}
                        size="small"
                        color="primary"
                        variant="outlined"
                      />
                    </Box>
                  }
                  secondary={rule.description}
                />
                <ListItemSecondaryAction>
                  <Tooltip title="删除规则">
                    <IconButton
                      edge="end"
                      size="small"
                      onClick={() => deleteRule(rule.id)}
                    >
                      <DeleteIcon />
                    </IconButton>
                  </Tooltip>
                </ListItemSecondaryAction>
              </ListItem>
            ))
          )}
        </List>
      </Paper>

      <Box sx={{ display: "flex", gap: 1, mb: 4 }}>
        <Button variant="contained" startIcon={<AddIcon />} size="small">
          添加规则
        </Button>
        <Button variant="outlined" size="small">
          应用预设
        </Button>
      </Box>

      <Divider sx={{ my: 2 }} />

      <Typography variant="h6" gutterBottom>
        最近拒绝的操作
      </Typography>

      <Paper variant="outlined">
        <List>
          {recentDenials.length === 0 ? (
            <ListItem>
              <ListItemText
                secondary="没有最近拒绝的操作"
                secondaryTypographyProps={{ align: "center" }}
              />
            </ListItem>
          ) : (
            recentDenials.slice(0, 10).map((denial) => (
              <ListItem key={denial.id} divider>
                <ListItemText
                  primary={
                    <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                      <WarningIcon color="error" fontSize="small" />
                      {denial.tool_name}
                    </Box>
                  }
                  secondary={`${new Date(denial.timestamp).toLocaleString()} - ${denial.reason}`}
                />
              </ListItem>
            ))
          )}
        </List>
      </Paper>
    </Box>
  );
};
