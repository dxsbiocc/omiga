import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Box,
  Button,
  Card,
  CardContent,
  Chip,
  CircularProgress,
  Divider,
  InputAdornment,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import { Search, SmartToy } from "@mui/icons-material";
import { normalizeAgentDisplayName } from "../../state/agentStore";
import { useChatComposerStore } from "../../state/chatComposerStore";

type AgentRoleInfo = {
  agent_type: string;
  when_to_use: string;
  source: string;
  model_tier: string;
  explicit_model?: string | null;
  background: boolean;
  user_facing: boolean;
};

type ModeLaneInfo = {
  session_id: string;
  mode: string;
  lane_id: string;
  preferred_agent_type?: string | null;
  supplemental_agent_types: string[];
};

function tierColor(tier: string): "default" | "primary" | "secondary" | "warning" {
  const t = tier.toLowerCase();
  if (t.includes("frontier")) return "secondary";
  if (t.includes("spark")) return "warning";
  if (t.includes("standard")) return "primary";
  return "default";
}

function tierOrder(tier: string): number {
  const t = tier.toLowerCase();
  if (t.includes("frontier")) return 0;
  if (t.includes("standard")) return 1;
  if (t.includes("spark")) return 2;
  return 3;
}

type VisibilityFilter = "all" | "user" | "internal";
type TierFilter = "all" | "frontier" | "standard" | "spark";

export function AgentRolesPanel({ projectRoot }: { projectRoot?: string }) {
  const composerAgentType = useChatComposerStore((s) => s.composerAgentType);
  const setComposerAgentType = useChatComposerStore((s) => s.setComposerAgentType);
  const [roles, setRoles] = useState<AgentRoleInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeLanes, setActiveLanes] = useState<ModeLaneInfo[]>([]);
  const [query, setQuery] = useState("");
  const [visibilityFilter, setVisibilityFilter] = useState<VisibilityFilter>("all");
  const [tierFilter, setTierFilter] = useState<TierFilter>("all");
  const [backgroundOnly, setBackgroundOnly] = useState(false);

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

  useEffect(() => {
    if (!projectRoot) {
      setActiveLanes([]);
      return;
    }
    let cancelled = false;
    invoke<ModeLaneInfo[]>("list_active_mode_lanes", { projectRoot })
      .then((rows) => {
        if (!cancelled) setActiveLanes(rows ?? []);
      })
      .catch(() => {
        if (!cancelled) setActiveLanes([]);
      });
    return () => {
      cancelled = true;
    };
  }, [projectRoot]);

  const activeRoleMeta = useMemo(() => {
    const meta = new Map<string, { primary: boolean; supplemental: boolean; lanes: string[] }>();
    for (const lane of activeLanes) {
      if (lane.preferred_agent_type) {
        const existing =
          meta.get(lane.preferred_agent_type) ?? { primary: false, supplemental: false, lanes: [] };
        existing.primary = true;
        if (!existing.lanes.includes(lane.lane_id)) existing.lanes.push(lane.lane_id);
        meta.set(lane.preferred_agent_type, existing);
      }
      for (const agent of lane.supplemental_agent_types) {
        const existing = meta.get(agent) ?? { primary: false, supplemental: false, lanes: [] };
        existing.supplemental = true;
        if (!existing.lanes.includes(lane.lane_id)) existing.lanes.push(lane.lane_id);
        meta.set(agent, existing);
      }
    }
    return meta;
  }, [activeLanes]);

  const filteredRoles = useMemo(() => {
    const q = query.trim().toLowerCase();
    return roles.filter((role) => {
      if (visibilityFilter === "user" && !role.user_facing) return false;
      if (visibilityFilter === "internal" && role.user_facing) return false;
      if (backgroundOnly && !role.background) return false;
      if (tierFilter !== "all" && !role.model_tier.toLowerCase().includes(tierFilter)) {
        return false;
      }
      if (!q) return true;
      const haystack = [
        role.agent_type,
        normalizeAgentDisplayName(role.agent_type),
        role.when_to_use,
        role.source,
        role.model_tier,
        role.explicit_model ?? "",
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(q);
    });
  }, [roles, query, visibilityFilter, tierFilter, backgroundOnly]);

  const grouped = useMemo(() => {
    const userFacing = filteredRoles.filter((r) => r.user_facing);
    const internal = filteredRoles.filter((r) => !r.user_facing);
    const byTier = (items: AgentRoleInfo[]) =>
      items.reduce<Record<string, AgentRoleInfo[]>>((acc, role) => {
        const tier = role.model_tier || "Other";
        (acc[tier] ??= []).push(role);
        return acc;
      }, {});
    return {
      userFacing,
      internal,
      userFacingByTier: byTier(userFacing),
      internalByTier: byTier(internal),
      total: filteredRoles.length,
    };
  }, [filteredRoles]);

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

  const renderRole = (role: AgentRoleInfo) => {
    const active = activeRoleMeta.get(role.agent_type);
    const isActive = Boolean(active);
    const borderColor = active?.primary ? "#2563eb" : active?.supplemental ? "#8b5cf6" : undefined;
    const isComposerSelected = composerAgentType === role.agent_type;
    return (
    <Card
      key={role.agent_type}
      variant="outlined"
      sx={{
        borderRadius: 2,
        borderColor: isComposerSelected
          ? alpha("#0ea5e9", 0.45)
          : isActive
            ? alpha(borderColor ?? "#2563eb", 0.45)
            : undefined,
        bgcolor: isComposerSelected
          ? alpha("#0ea5e9", 0.05)
          : isActive
            ? alpha(borderColor ?? "#2563eb", 0.04)
            : undefined,
      }}
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
            {isComposerSelected && (
              <Chip
                label="当前输入角色"
                size="small"
                sx={{
                  height: 16,
                  fontSize: 9,
                  bgcolor: alpha("#0ea5e9", 0.12),
                  color: "#0ea5e9",
                  fontWeight: 600,
                }}
              />
            )}
            {active?.primary && (
              <Chip
                label="当前主角色"
                size="small"
                sx={{
                  height: 16,
                  fontSize: 9,
                  bgcolor: alpha("#2563eb", 0.12),
                  color: "#2563eb",
                  fontWeight: 600,
                }}
              />
            )}
            {!active?.primary && active?.supplemental && (
              <Chip
                label="当前辅助角色"
                size="small"
                sx={{
                  height: 16,
                  fontSize: 9,
                  bgcolor: alpha("#8b5cf6", 0.12),
                  color: "#8b5cf6",
                  fontWeight: 600,
                }}
              />
            )}
          </Stack>

          <Typography variant="body2" color="text.secondary">
            {role.when_to_use}
          </Typography>

          {role.user_facing && (
            <Stack direction="row" spacing={1}>
              <Button
                size="small"
                variant={isComposerSelected ? "contained" : "outlined"}
                onClick={() => setComposerAgentType(role.agent_type)}
              >
                {isComposerSelected ? "已用于输入框" : "用于输入框"}
              </Button>
            </Stack>
          )}

          {active && (
            <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10 }}>
              活跃 lane：{active.lanes.join(" / ")}
            </Typography>
          )}
        </Stack>
      </CardContent>
    </Card>
    );
  };

  const renderTierGroup = (title: string, items: AgentRoleInfo[]) => (
    <Box key={title}>
      <Stack direction="row" alignItems="center" spacing={1} sx={{ mb: 1 }}>
        <Typography variant="caption" color="text.secondary">
          {title}
        </Typography>
        <Chip label={items.length} size="small" sx={{ height: 16, fontSize: 9 }} />
      </Stack>
      <Stack spacing={1}>{items.map(renderRole)}</Stack>
    </Box>
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
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.5 }}>
          当前输入框角色：{normalizeAgentDisplayName(composerAgentType || "auto")}
        </Typography>
        {activeLanes.length > 0 && (
          <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.5 }}>
            当前检测到 {activeLanes.length} 条活跃 execution lanes，相关角色会高亮显示。
          </Typography>
        )}
      </Box>

      <Stack spacing={1}>
        <TextField
          size="small"
          placeholder="搜索角色名称、用途、模型…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          InputProps={{
            startAdornment: (
              <InputAdornment position="start">
                <Search fontSize="small" />
              </InputAdornment>
            ),
          }}
        />
        <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
          <Chip
            label="全部"
            size="small"
            color={visibilityFilter === "all" ? "primary" : "default"}
            variant={visibilityFilter === "all" ? "filled" : "outlined"}
            onClick={() => setVisibilityFilter("all")}
          />
          <Chip
            label="面向用户"
            size="small"
            color={visibilityFilter === "user" ? "primary" : "default"}
            variant={visibilityFilter === "user" ? "filled" : "outlined"}
            onClick={() => setVisibilityFilter("user")}
          />
          <Chip
            label="内部编排"
            size="small"
            color={visibilityFilter === "internal" ? "primary" : "default"}
            variant={visibilityFilter === "internal" ? "filled" : "outlined"}
            onClick={() => setVisibilityFilter("internal")}
          />
          <Chip
            label="All tiers"
            size="small"
            color={tierFilter === "all" ? "secondary" : "default"}
            variant={tierFilter === "all" ? "filled" : "outlined"}
            onClick={() => setTierFilter("all")}
          />
          {(["frontier", "standard", "spark"] as TierFilter[]).map((tier) => (
            <Chip
              key={tier}
              label={tier}
              size="small"
              color={tierFilter === tier ? tierColor(tier) : "default"}
              variant={tierFilter === tier ? "filled" : "outlined"}
              onClick={() => setTierFilter(tier)}
            />
          ))}
          <Chip
            label="仅后台"
            size="small"
            color={backgroundOnly ? "warning" : "default"}
            variant={backgroundOnly ? "filled" : "outlined"}
            onClick={() => setBackgroundOnly((v) => !v)}
          />
        </Stack>
        <Typography variant="caption" color="text.secondary">
          当前筛选后共 {grouped.total} 个角色
        </Typography>
      </Stack>

      <Box>
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 1 }}>
          面向用户的角色（可直接选择）
        </Typography>
        <Stack spacing={1}>
          {grouped.userFacing.length > 0 ? (
            Object.entries(grouped.userFacingByTier)
              .sort((a, b) => tierOrder(a[0]) - tierOrder(b[0]))
              .map(([tier, items]) => renderTierGroup(tier, items))
          ) : (
            <Alert severity="info" sx={{ borderRadius: 2 }}>
              当前筛选条件下没有面向用户的角色。
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
          {grouped.internal.length > 0 ? (
            Object.entries(grouped.internalByTier)
              .sort((a, b) => tierOrder(a[0]) - tierOrder(b[0]))
              .map(([tier, items]) => renderTierGroup(tier, items))
          ) : (
            <Alert severity="info" sx={{ borderRadius: 2 }}>
              当前筛选条件下没有内部编排角色。
            </Alert>
          )}
        </Stack>
      </Box>
    </Stack>
  );
}
