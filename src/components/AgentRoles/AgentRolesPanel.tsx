import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Box,
  Card,
  CardContent,
  Chip,
  CircularProgress,
  Divider,
  Stack,
  Typography,
} from "@mui/material";
import { SmartToy } from "@mui/icons-material";
import { normalizeAgentDisplayName } from "../../state/agentStore";

type AgentRoleInfo = {
  agent_type: string;
  when_to_use: string;
  source: string;
  model_tier: string;
  explicit_model?: string | null;
  background: boolean;
  user_facing: boolean;
};

function tierColor(tier: string): "default" | "primary" | "secondary" | "warning" {
  const t = tier.toLowerCase();
  if (t.includes("frontier")) return "secondary";
  if (t.includes("spark")) return "warning";
  if (t.includes("standard")) return "primary";
  return "default";
}

export function AgentRolesPanel() {
  const [roles, setRoles] = useState<AgentRoleInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    invoke<AgentRoleInfo[]>("list_agent_roles")
      .then((rows) => {
        if (cancelled) return;
        setRoles(rows);
      })
      .catch((err) => {
        if (cancelled) return;
        setError(String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const grouped = useMemo(() => {
    const userFacing = roles.filter((r) => r.user_facing);
    const internal = roles.filter((r) => !r.user_facing);
    return { userFacing, internal };
  }, [roles]);

  if (loading) {
    return (
      <Box display="flex" alignItems="center" gap={1} py={1}>
        <CircularProgress size={18} />
        <Typography variant="body2" color="text.secondary">
          正在加载 Agent 角色…
        </Typography>
      </Box>
    );
  }

  if (error) {
    return (
      <Alert severity="error" sx={{ borderRadius: 2 }}>
        加载 Agent 角色失败：{error}
      </Alert>
    );
  }

  const renderRole = (role: AgentRoleInfo) => (
    <Card
      key={role.agent_type}
      variant="outlined"
      sx={{ borderRadius: 2 }}
    >
      <CardContent sx={{ p: 1.5, "&:last-child": { pb: 1.5 } }}>
        <Stack spacing={1}>
          <Stack
            direction="row"
            spacing={1}
            alignItems="center"
            flexWrap="wrap"
            useFlexGap
          >
            <Chip
              icon={<SmartToy />}
              label={normalizeAgentDisplayName(role.agent_type)}
              size="small"
              color="default"
              variant="outlined"
            />
            <Chip
              label={role.model_tier}
              size="small"
              color={tierColor(role.model_tier)}
              variant="filled"
            />
            <Chip
              label={role.source}
              size="small"
              variant="outlined"
            />
            {role.background && (
              <Chip label="Background" size="small" color="warning" variant="outlined" />
            )}
            {role.explicit_model && (
              <Chip label={role.explicit_model} size="small" variant="outlined" />
            )}
          </Stack>

          <Typography variant="body2" color="text.secondary">
            {role.when_to_use}
          </Typography>
        </Stack>
      </CardContent>
    </Card>
  );

  return (
    <Stack spacing={2}>
      <Box>
        <Typography variant="subtitle2" fontWeight={600}>
          角色目录
        </Typography>
        <Typography variant="body2" color="text.secondary">
          这些角色来自后端 Agent Registry，展示可用角色、模型层级与用途说明。
        </Typography>
      </Box>

      <Box>
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 1 }}>
          面向用户的角色（可直接选择）
        </Typography>
        <Stack spacing={1}>
          {grouped.userFacing.length > 0 ? (
            grouped.userFacing.map(renderRole)
          ) : (
            <Alert severity="info" sx={{ borderRadius: 2 }}>
              当前没有标记为面向用户的角色。
            </Alert>
          )}
        </Stack>
      </Box>

      <Divider />

      <Box>
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 1 }}>
          内部编排角色（由调度器 / mode runtime 使用）
        </Typography>
        <Stack spacing={1}>
          {grouped.internal.map(renderRole)}
        </Stack>
      </Box>
    </Stack>
  );
}
