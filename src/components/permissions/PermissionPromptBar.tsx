import React, { useState } from "react";
import {
  Button,
  Typography,
  Alert,
  AlertTitle,
  Box,
  Chip,
  Divider,
  FormControl,
  FormControlLabel,
  FormLabel,
  Radio,
  RadioGroup,
  Stack,
  Accordion,
  AccordionSummary,
  AccordionDetails,
  CircularProgress,
} from "@mui/material";
import {
  Warning as WarningIcon,
  Error as ErrorIcon,
  CheckCircle as CheckIcon,
  Info as InfoIcon,
  ExpandMore as ExpandMoreIcon,
} from "@mui/icons-material";
import {
  usePermissionStore,
  type ToolPermissionMode,
  type RiskLevel,
} from "../../state/permissionStore";

const getRiskColor = (level: RiskLevel) => {
  switch (level) {
    case "safe":
      return "success";
    case "low":
      return "info";
    case "medium":
      return "warning";
    case "high":
    case "critical":
      return "error";
    default:
      return "warning";
  }
};

const getRiskIcon = (level: RiskLevel) => {
  switch (level) {
    case "safe":
      return <CheckIcon color="success" />;
    case "low":
      return <InfoIcon color="info" />;
    case "medium":
      return <WarningIcon color="warning" />;
    case "high":
    case "critical":
      return <ErrorIcon color="error" />;
    default:
      return <WarningIcon color="warning" />;
  }
};

const getRiskLabel = (level: RiskLevel) => {
  switch (level) {
    case "safe":
      return "安全";
    case "low":
      return "低风险";
    case "medium":
      return "中等风险";
    case "high":
      return "高风险";
    case "critical":
      return "严重风险";
    default:
      return "未知风险";
  }
};

type ModeChoice = "askEveryTime" | "session" | "timeWindow" | "plan";

const convertModeToBackend = (
  modeValue: ModeChoice,
  minutes: number,
): ToolPermissionMode => {
  switch (modeValue) {
    case "askEveryTime":
      return "AskEveryTime";
    case "session":
      return "Session";
    case "timeWindow":
      return { TimeWindow: { minutes } };
    case "plan":
      return "Plan";
    default:
      return "Session";
  }
};

/** 内联在输入框上方，非弹窗 */
export const PermissionPromptBar: React.FC = () => {
  const { pendingRequest, approveRequest, denyRequest, error, clearError } =
    usePermissionStore();
  const [modeValue, setModeValue] = useState<ModeChoice>("session");
  const [timeWindowMinutes, setTimeWindowMinutes] = useState<number>(60);
  const [showDetails, setShowDetails] = useState(false);
  const [processing, setProcessing] = useState(false);

  if (!pendingRequest) return null;

  const isDangerous =
    pendingRequest.risk_level === "high" ||
    pendingRequest.risk_level === "critical";
  const isCritical = pendingRequest.risk_level === "critical";

  const handleApprove = async () => {
    setProcessing(true);
    clearError();
    try {
      const mode = convertModeToBackend(modeValue, timeWindowMinutes);
      await approveRequest(mode);
    } catch {
      // store 已记录
    } finally {
      setProcessing(false);
    }
  };

  const handleDeny = async () => {
    setProcessing(true);
    clearError();
    try {
      await denyRequest("用户拒绝");
    } catch {
      // store 已记录
    } finally {
      setProcessing(false);
    }
  };

  return (
    <Box
      sx={{
        px: 2,
        py: 1.5,
        borderBottom: 1,
        borderColor: "divider",
        bgcolor: (t) =>
          t.palette.mode === "dark"
            ? "rgba(255,255,255,0.04)"
            : "rgba(0,0,0,0.02)",
      }}
    >
      <Stack spacing={1.5}>
        <Stack
          direction="row"
          alignItems="center"
          gap={1}
          flexWrap="wrap"
          justifyContent="space-between"
        >
          <Stack direction="row" alignItems="center" gap={1} flexWrap="wrap">
            {getRiskIcon(pendingRequest.risk_level)}
            <Typography variant="subtitle2" fontWeight={700}>
              权限确认
            </Typography>
            <Chip
              label={getRiskLabel(pendingRequest.risk_level)}
              color={getRiskColor(pendingRequest.risk_level) as never}
              size="small"
            />
            <Box
              component="code"
              sx={{
                px: 1,
                py: 0.25,
                borderRadius: 1,
                bgcolor: "action.hover",
                fontSize: "0.85rem",
              }}
            >
              {pendingRequest.tool_name}
            </Box>
          </Stack>
          <Stack direction="row" spacing={1}>
            <Button
              size="small"
              onClick={handleDeny}
              color="inherit"
              variant="outlined"
              disabled={processing}
            >
              拒绝
            </Button>
            <Button
              size="small"
              onClick={handleApprove}
              color={isDangerous ? "error" : "primary"}
              variant="contained"
              disabled={processing}
              startIcon={
                processing ? <CircularProgress size={14} color="inherit" /> : null
              }
            >
              {processing
                ? "处理中…"
                : isCritical
                  ? "我已了解风险，确认允许"
                  : "允许"}
            </Button>
          </Stack>
        </Stack>

        {error && (
          <Alert severity="error" onClose={clearError}>
            {error}
          </Alert>
        )}

        {isCritical && (
          <Alert severity="error">
            <AlertTitle>严重风险操作</AlertTitle>
            此操作可能导致系统损坏或数据丢失，请格外谨慎！
          </Alert>
        )}

        {!isCritical && isDangerous && (
          <Alert severity="warning">
            <AlertTitle>高风险操作</AlertTitle>
            此操作可能影响系统稳定性，请确认您了解其后果。
          </Alert>
        )}

        <Typography variant="body2" color="text.secondary">
          {pendingRequest.risk_description}
        </Typography>

        {pendingRequest.recommendations.length > 0 && (
          <Box>
            <Typography variant="caption" color="text.secondary" display="block">
              建议
            </Typography>
            <Stack spacing={0.25}>
              {pendingRequest.recommendations.map((rec, idx) => (
                <Typography key={idx} variant="caption" color="text.secondary">
                  • {rec}
                </Typography>
              ))}
            </Stack>
          </Box>
        )}

        {pendingRequest.detected_risks.length > 0 && (
          <Accordion
            expanded={showDetails}
            onChange={() => setShowDetails(!showDetails)}
            variant="outlined"
            disableGutters
            sx={{ "&:before": { display: "none" } }}
          >
            <AccordionSummary expandIcon={<ExpandMoreIcon />}>
              <Typography variant="caption">
                检测到 {pendingRequest.detected_risks.length} 个风险点
              </Typography>
            </AccordionSummary>
            <AccordionDetails>
              <Stack spacing={1}>
                {pendingRequest.detected_risks.map((risk, idx) => (
                  <Alert
                    key={idx}
                    severity={getRiskColor(risk.severity) as never}
                    variant="outlined"
                    sx={{ textAlign: "left" }}
                  >
                    <Typography variant="caption" fontWeight={600} display="block">
                      {risk.category} — {getRiskLabel(risk.severity)}
                    </Typography>
                    <Typography variant="body2">{risk.description}</Typography>
                    {risk.mitigation && (
                      <Typography variant="caption" color="text.secondary">
                        建议: {risk.mitigation}
                      </Typography>
                    )}
                  </Alert>
                ))}
              </Stack>
            </AccordionDetails>
          </Accordion>
        )}

        <Divider flexItem />

        <FormControl component="fieldset" variant="standard" disabled={processing}>
          <FormLabel component="legend" sx={{ typography: "caption", mb: 0.5 }}>
            记住我的选择
          </FormLabel>
          <RadioGroup
            value={modeValue}
            onChange={(e) => setModeValue(e.target.value as ModeChoice)}
          >
            <FormControlLabel
              value="askEveryTime"
              control={<Radio size="small" />}
              label="仅这次允许"
            />
            <FormControlLabel
              value="session"
              control={<Radio size="small" />}
              label="本次会话内允许"
            />
            <FormControlLabel
              value="timeWindow"
              control={<Radio size="small" />}
              label="在选定时间窗口内允许"
            />
            <FormControlLabel
              value="plan"
              control={<Radio size="small" />}
              label="Plan 模式（批量确认）"
            />
          </RadioGroup>
        </FormControl>

        {modeValue === "timeWindow" && (
          <FormControl component="fieldset" variant="standard" disabled={processing}>
            <FormLabel component="legend" sx={{ typography: "caption", mb: 0.5 }}>
              时长
            </FormLabel>
            <RadioGroup
              row
              value={String(timeWindowMinutes)}
              onChange={(e) => setTimeWindowMinutes(Number(e.target.value))}
            >
              <FormControlLabel
                value="60"
                control={<Radio size="small" />}
                label="1 小时"
              />
              <FormControlLabel
                value="240"
                control={<Radio size="small" />}
                label="4 小时"
              />
              <FormControlLabel
                value="1440"
                control={<Radio size="small" />}
                label="24 小时"
              />
            </RadioGroup>
          </FormControl>
        )}
      </Stack>
    </Box>
  );
};
