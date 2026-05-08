import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { PlayArrow, ExpandMore, SmartToy, Warning } from "@mui/icons-material";
import {
  Box,
  Button,
  TextField,
  Select,
  MenuItem,
  FormControl,
  InputLabel,
  Typography,
  Alert,
  Accordion,
  AccordionSummary,
  AccordionDetails,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  Chip,
  Stack,
} from "@mui/material";
import {
  useAgentStore,
  type ScheduleConfirmationPayload,
  type ScheduleRequest,
} from "../../state/agentStore";

type SchedulingStrategy =
  | "Auto"
  | "Single"
  | "Sequential"
  | "Parallel"
  | "Phased"
  | "Competitive"
  | "VerificationFirst";

const STRATEGIES: { value: SchedulingStrategy; label: string; desc: string }[] = [
  { value: "Auto", label: "自动", desc: "根据任务复杂度自动选择最佳执行策略" },
  { value: "Single", label: "单 Agent", desc: "使用单个 Agent 完成任务，适用于简单直接的请求" },
  { value: "Sequential", label: "顺序执行", desc: "按顺序执行多个 Agent，每个依赖前一个的结果" },
  { value: "Parallel", label: "并行执行", desc: "同时启动多个 Agent，各自处理不同方面，最后合并结果" },
  { value: "Phased", label: "分阶段", desc: "探索→设计→实现→验证，适用于复杂功能开发" },
  { value: "Competitive", label: "竞争执行", desc: "多个 Agent 同时解决同一问题，选择最佳结果" },
  { value: "VerificationFirst", label: "验证优先", desc: "先验证现有代码，再进行修改，适用于重构和优化" },
];

interface Props {
  sessionId: string;
  projectRoot: string;
}

function fireSchedule(req: ScheduleRequest, onError: (msg: string) => void) {
  invoke("run_agent_schedule", { request: req }).catch((err: unknown) => {
    onError(String(err));
  });
  useAgentStore.getState().setTaskPanelVisible(true);
}

function fireConfirmedPlan(
  payload: ScheduleConfirmationPayload,
  onError: (msg: string) => void,
) {
  invoke("run_existing_agent_plan", {
    request: {
      plan: payload.plan,
      projectRoot: payload.projectRoot || payload.originalRequest.projectRoot,
      sessionId: payload.sessionId || payload.originalRequest.sessionId,
      modeHint: payload.modeHint ?? payload.originalRequest.modeHint ?? "schedule",
      strategy: payload.strategy ?? payload.originalRequest.strategy ?? "Phased",
    },
  }).catch((err: unknown) => {
    onError(String(err));
  });
  useAgentStore.getState().setTaskPanelVisible(true);
}

/** 确认对话框：当计划需要用户审批时弹出（在 App 根级挂载，不依赖 projectRoot） */
export function ConfirmationDialog() {
  const { pendingConfirmation, setPendingConfirmation } = useAgentStore();
  const [error, setError] = useState<string | null>(null);

  if (!pendingConfirmation) return null;

  const handleConfirm = () => {
    const confirmed = pendingConfirmation;
    setPendingConfirmation(null);
    setError(null);
    fireConfirmedPlan(confirmed, setError);
  };

  const handleCancel = () => {
    setPendingConfirmation(null);
    setError(null);
  };

  return (
    <Dialog open onClose={handleCancel} maxWidth="sm" fullWidth>
      <DialogTitle sx={{ display: "flex", alignItems: "center", gap: 1 }}>
        <Warning color="warning" />
        需要确认执行计划
      </DialogTitle>
      <DialogContent>
        <Stack spacing={2}>
          <Typography variant="body2" color="text.secondary">
            {pendingConfirmation.summary}
          </Typography>

          <Box>
            <Typography variant="caption" color="text.secondary" gutterBottom>
              将使用以下 Agent（预计 {pendingConfirmation.estimatedMinutes} 分钟）：
            </Typography>
            <Box display="flex" flexWrap="wrap" gap={0.5} mt={0.5}>
              {pendingConfirmation.agents.map((a) => (
                <Chip
                  key={a}
                  label={a}
                  size="small"
                  icon={<SmartToy />}
                  variant="outlined"
                />
              ))}
            </Box>
          </Box>

          {error && (
            <Alert severity="error" sx={{ py: 0 }}>
              {error}
            </Alert>
          )}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={handleCancel} color="inherit">
          取消
        </Button>
        <Button onClick={handleConfirm} variant="contained" color="primary">
          确认执行
        </Button>
      </DialogActions>
    </Dialog>
  );
}

export function AgentScheduleLauncher({ sessionId, projectRoot }: Props) {
  const [request, setRequest] = useState("");
  const [strategy, setStrategy] = useState<SchedulingStrategy>("Auto");
  const [maxAgents, setMaxAgents] = useState(5);
  const [error, setError] = useState<string | null>(null);

  const selectedDesc = STRATEGIES.find((s) => s.value === strategy)?.desc ?? "";

  const launch = () => {
    if (!request.trim()) return;
    setError(null);

    fireSchedule(
      {
        userRequest: request.trim(),
        projectRoot,
        sessionId,
        maxAgents,
        autoDecompose: true,
        strategy,
        skipConfirmation: false,
      },
      setError,
    );

    setRequest("");
  };

  return (
    <>

      <Accordion
        disableGutters
        elevation={0}
        sx={{
          border: "1px solid",
          borderColor: "divider",
          borderRadius: 1,
          "&:before": { display: "none" },
        }}
      >
        <AccordionSummary
          expandIcon={<ExpandMore />}
          sx={{ minHeight: 40, "& .MuiAccordionSummary-content": { my: 0.5 } }}
        >
          <Typography variant="body2" fontWeight={600}>
            启动 Agent 编排
          </Typography>
        </AccordionSummary>
        <AccordionDetails sx={{ pt: 0, pb: 1.5, px: 1.5 }}>
          <Box display="flex" flexDirection="column" gap={1.5}>
            <TextField
              size="small"
              multiline
              minRows={2}
              maxRows={5}
              placeholder="描述需要完成的任务..."
              value={request}
              onChange={(e) => setRequest(e.target.value)}
              fullWidth
            />

            <Box display="flex" gap={1} alignItems="flex-start">
              <FormControl size="small" sx={{ minWidth: 140 }}>
                <InputLabel>策略</InputLabel>
                <Select
                  value={strategy}
                  label="策略"
                  onChange={(e) => setStrategy(e.target.value as SchedulingStrategy)}
                >
                  {STRATEGIES.map((s) => (
                    <MenuItem key={s.value} value={s.value}>
                      {s.label}
                    </MenuItem>
                  ))}
                </Select>
              </FormControl>

              <TextField
                size="small"
                type="number"
                label="最大 Agent 数"
                value={maxAgents}
                onChange={(e) =>
                  setMaxAgents(Math.max(1, Math.min(10, Number(e.target.value))))
                }
                sx={{ width: 110 }}
                inputProps={{ min: 1, max: 10 }}
              />

              <Button
                size="small"
                variant="contained"
                startIcon={<PlayArrow />}
                onClick={launch}
                disabled={!request.trim()}
                sx={{ ml: "auto", whiteSpace: "nowrap" }}
              >
                启动
              </Button>
            </Box>

            {selectedDesc && (
              <Typography variant="caption" color="text.secondary">
                {selectedDesc}
              </Typography>
            )}

            {error && (
              <Alert severity="error" sx={{ py: 0 }}>
                {error}
              </Alert>
            )}
          </Box>
        </AccordionDetails>
      </Accordion>
    </>
  );
}
