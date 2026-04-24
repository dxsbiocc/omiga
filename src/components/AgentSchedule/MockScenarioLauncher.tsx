import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Alert, Box, Button, Stack, Typography } from "@mui/material";
import { useAgentStore } from "../../state/agentStore";

type Scenario = "schedule" | "team" | "autopilot";

const SCENARIOS: Array<{ id: Scenario; label: string; desc: string }> = [
  {
    id: "schedule",
    label: "Mock /schedule",
    desc: "注入调度计划、worker 完成与 reviewer verdict，用于验收任务区 trace。",
  },
  {
    id: "team",
    label: "Mock /team",
    desc: "注入 team 的 verifying → fixing → synthesizing 轨迹与相关事件。",
  },
  {
    id: "autopilot",
    label: "Mock /autopilot",
    desc: "注入 autopilot 的 QA / validation 状态与 reviewer 结果。",
  },
];

interface Props {
  sessionId: string;
  projectRoot: string;
}

export function MockScenarioLauncher({ sessionId, projectRoot }: Props) {
  const [running, setRunning] = useState<Scenario | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const runScenario = async (scenario: Scenario) => {
    setRunning(scenario);
    setMessage(null);
    try {
      await invoke("run_mock_orchestration_scenario", {
        request: {
          sessionId,
          projectRoot,
          scenario,
        },
      });
      useAgentStore.getState().setTaskPanelVisible(true);
      setMessage(`已注入 mock 场景：${scenario}`);
    } catch (error) {
      setMessage(`mock 场景注入失败：${String(error)}`);
    } finally {
      setRunning(null);
    }
  };

  return (
    <Box
      sx={{
        border: 1,
        borderColor: "divider",
        borderRadius: 2,
        p: 1.5,
      }}
    >
      <Typography variant="subtitle2" fontWeight={700} sx={{ mb: 0.5 }}>
        Mock 场景验收辅助
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 1.25 }}>
        临时验收工具：在当前会话注入 mock orchestration 数据，便于验证 dashboard / trace /
        transcript 链路。验收通过后可删除。
      </Typography>

      <Stack spacing={1}>
        {SCENARIOS.map((scenario) => (
          <Box
            key={scenario.id}
            sx={{
              p: 1,
              borderRadius: 1.5,
              bgcolor: "background.default",
              border: 1,
              borderColor: "divider",
            }}
          >
            <Typography variant="body2" fontWeight={600}>
              {scenario.label}
            </Typography>
            <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 0.75 }}>
              {scenario.desc}
            </Typography>
            <Button
              size="small"
              variant="outlined"
              disabled={running !== null}
              onClick={() => void runScenario(scenario.id)}
            >
              {running === scenario.id ? "注入中…" : "注入场景"}
            </Button>
          </Box>
        ))}
      </Stack>

      {message && (
        <Alert
          severity={message.startsWith("已注入") ? "success" : "warning"}
          sx={{ mt: 1.25, py: 0 }}
        >
          {message}
        </Alert>
      )}
    </Box>
  );
}
