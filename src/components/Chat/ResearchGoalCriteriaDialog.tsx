import { memo, useEffect, useMemo, useState } from "react";
import {
  Alert,
  Autocomplete,
  Box,
  Button,
  Checkbox,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControlLabel,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import type {
  ResearchGoal,
  ResearchGoalAutoRunPolicy,
} from "./ResearchGoalStatusPill";

const MAX_GOAL_CYCLES = 20;
const MAX_AUTO_RUN_CYCLES = 10;
const DEFAULT_AUTO_RUN_CYCLES_PER_RUN = 10;
const DEFAULT_AUTO_RUN_IDLE_DELAY_MS = 650;
const MIN_AUTO_RUN_IDLE_DELAY_MS = 250;
const MAX_AUTO_RUN_IDLE_DELAY_MS = 60_000;
const MAX_AUTO_RUN_ELAPSED_MINUTES = 24 * 60;
const MAX_AUTO_RUN_TOKENS = 100_000_000;

export interface ResearchGoalSettingsDraft {
  criteria: string[];
  maxCycles: number;
  secondOpinionProviderEntry: string;
  autoRunPolicy: ResearchGoalAutoRunPolicyDraft;
}

export interface ResearchGoalAutoRunPolicyDraft {
  enabled: boolean;
  cyclesPerRun: number;
  idleDelayMs: number;
  maxElapsedMinutes?: number | null;
  maxTokens?: number | null;
}

export interface ResearchGoalProviderEntryOption {
  name: string;
  providerType: string;
  model: string;
  enabled?: boolean;
  isDefault?: boolean;
}

export interface ResearchGoalProviderTestResult {
  available: boolean;
  provider?: string | null;
  model?: string | null;
  latencyMs?: number | null;
  error?: string | null;
}

export function criteriaDraftFromGoal(goal: ResearchGoal | null): string {
  return goal?.successCriteria.join("\n") ?? "";
}

export function maxCyclesDraftFromGoal(goal: ResearchGoal | null): string {
  return goal ? String(goal.maxCycles) : "3";
}

export function secondOpinionProviderEntryDraftFromGoal(
  goal: ResearchGoal | null,
): string {
  return goal?.secondOpinionProviderEntry?.trim() ?? "";
}

export function autoRunPolicyDraftFromGoal(
  goal: ResearchGoal | null,
): ResearchGoalAutoRunPolicy {
  return {
    enabled: goal?.autoRunPolicy?.enabled ?? false,
    cyclesPerRun:
      goal?.autoRunPolicy?.cyclesPerRun ?? DEFAULT_AUTO_RUN_CYCLES_PER_RUN,
    idleDelayMs: goal?.autoRunPolicy?.idleDelayMs ?? DEFAULT_AUTO_RUN_IDLE_DELAY_MS,
    maxElapsedMinutes: goal?.autoRunPolicy?.maxElapsedMinutes ?? null,
    maxTokens: goal?.autoRunPolicy?.maxTokens ?? null,
    startedAt: goal?.autoRunPolicy?.startedAt ?? null,
  };
}

export function providerEntryOptionLabel(
  option: ResearchGoalProviderEntryOption,
): string {
  const model = option.model?.trim();
  const provider = option.providerType?.trim();
  const details = [provider, model].filter(Boolean).join("/");
  return details ? `${option.name} · ${details}` : option.name;
}

export function providerTestResultMessage(
  result: ResearchGoalProviderTestResult,
): string {
  if (!result.available) {
    return result.error || "二审 provider 测试失败。";
  }
  const target = [result.provider, result.model].filter(Boolean).join(" / ");
  const latency =
    typeof result.latencyMs === "number" ? `，${result.latencyMs}ms` : "";
  return target
    ? `二审 provider 真实 LLM 调用通过：${target}${latency}`
    : `二审 provider 真实 LLM 调用通过${latency}`;
}

function errorMessageFromUnknown(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (error && typeof error === "object") {
    const record = error as Record<string, unknown>;
    if (typeof record.message === "string") return record.message;
    if (
      record.type === "Config" &&
      typeof record.message === "string"
    ) {
      return record.message;
    }
    if (
      record.details &&
      typeof record.details === "object" &&
      typeof (record.details as Record<string, unknown>).message === "string"
    ) {
      return (record.details as Record<string, string>).message;
    }
  }
  return fallback;
}

export function criteriaLinesFromDraft(draft: string): string[] {
  const seen = new Set<string>();
  const lines: string[] = [];
  for (const raw of draft.split(/\r?\n/u)) {
    const item = raw.replace(/\s+/gu, " ").trim();
    if (!item) continue;
    const key = item.toLocaleLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    lines.push(item);
  }
  return lines;
}

export function validateCriteriaLines(lines: string[]): string | null {
  if (lines.length === 0) return "至少需要 1 条成功标准。";
  if (lines.length > 12) return "成功标准最多支持 12 条。";
  if (lines.some((line) => [...line].length > 240)) {
    return "单条成功标准不能超过 240 个字符。";
  }
  return null;
}

export function parseMaxCyclesDraft(draft: string): number | null {
  const trimmed = draft.trim();
  if (!/^\d+$/u.test(trimmed)) return null;
  const parsed = Number.parseInt(trimmed, 10);
  return Number.isSafeInteger(parsed) ? parsed : null;
}

export function validateMaxCycles(
  value: number | null,
  currentCycle: number,
): string | null {
  if (value === null) return "轮次预算必须是整数。";
  const min = Math.max(1, currentCycle);
  if (value < min) {
    return currentCycle > 0
      ? `轮次预算不能小于当前已运行轮次（${currentCycle}）。`
      : "轮次预算必须大于 0。";
  }
  if (value > MAX_GOAL_CYCLES) {
    return `轮次预算最多支持 ${MAX_GOAL_CYCLES} 轮。`;
  }
  return null;
}

export function parseOptionalPositiveIntegerDraft(draft: string): number | null {
  const trimmed = draft.trim();
  if (!trimmed) return null;
  if (!/^\d+$/u.test(trimmed)) return Number.NaN;
  const parsed = Number.parseInt(trimmed, 10);
  return Number.isSafeInteger(parsed) ? parsed : Number.NaN;
}

export function validateAutoRunPolicyDraft(
  enabled: boolean,
  cyclesPerRun: number | null,
  idleDelayMs: number | null,
  maxElapsedMinutes: number | null,
  maxTokens: number | null = null,
): string | null {
  if (!enabled) return null;
  if (
    cyclesPerRun === null ||
    !Number.isFinite(cyclesPerRun) ||
    cyclesPerRun < 1 ||
    cyclesPerRun > MAX_AUTO_RUN_CYCLES
  ) {
    return `自动续跑每次轮数必须在 1 到 ${MAX_AUTO_RUN_CYCLES} 之间。`;
  }
  if (
    idleDelayMs === null ||
    !Number.isFinite(idleDelayMs) ||
    idleDelayMs < MIN_AUTO_RUN_IDLE_DELAY_MS ||
    idleDelayMs > MAX_AUTO_RUN_IDLE_DELAY_MS
  ) {
    return `自动续跑空闲延迟必须在 ${MIN_AUTO_RUN_IDLE_DELAY_MS} 到 ${MAX_AUTO_RUN_IDLE_DELAY_MS} ms 之间。`;
  }
  if (
    maxElapsedMinutes !== null &&
    (!Number.isFinite(maxElapsedMinutes) ||
      maxElapsedMinutes < 1 ||
      maxElapsedMinutes > MAX_AUTO_RUN_ELAPSED_MINUTES)
  ) {
    return `自动续跑最长耗时必须在 1 到 ${MAX_AUTO_RUN_ELAPSED_MINUTES} 分钟之间；留空表示不限制。`;
  }
  if (
    maxTokens !== null &&
    (!Number.isFinite(maxTokens) ||
      maxTokens < 1 ||
      maxTokens > MAX_AUTO_RUN_TOKENS)
  ) {
    return `自动续跑 token 预算必须在 1 到 ${MAX_AUTO_RUN_TOKENS} 之间；留空表示不限制。`;
  }
  return null;
}

interface ResearchGoalCriteriaDialogProps {
  open: boolean;
  goal: ResearchGoal | null;
  saving?: boolean;
  error?: string | null;
  providerEntryOptions?: ResearchGoalProviderEntryOption[];
  providerEntryOptionsLoading?: boolean;
  onClose: () => void;
  onSave: (settings: ResearchGoalSettingsDraft) => void | Promise<void>;
  onSuggestCriteria?: () => Promise<string[]>;
  onTestSecondOpinionProvider?: (
    providerEntry: string,
  ) => Promise<ResearchGoalProviderTestResult>;
}

export const ResearchGoalCriteriaDialog = memo(
  function ResearchGoalCriteriaDialog({
    open,
    goal,
    saving = false,
    error = null,
    providerEntryOptions = [],
    providerEntryOptionsLoading = false,
    onClose,
    onSave,
    onSuggestCriteria,
    onTestSecondOpinionProvider,
  }: ResearchGoalCriteriaDialogProps) {
    const [draft, setDraft] = useState("");
    const [maxCyclesDraft, setMaxCyclesDraft] = useState("3");
    const [secondOpinionProviderEntryDraft, setSecondOpinionProviderEntryDraft] =
      useState("");
    const [autoRunEnabledDraft, setAutoRunEnabledDraft] = useState(false);
    const [autoRunCyclesPerRunDraft, setAutoRunCyclesPerRunDraft] =
      useState(String(DEFAULT_AUTO_RUN_CYCLES_PER_RUN));
    const [autoRunIdleDelayMsDraft, setAutoRunIdleDelayMsDraft] = useState(
      String(DEFAULT_AUTO_RUN_IDLE_DELAY_MS),
    );
    const [autoRunMaxElapsedMinutesDraft, setAutoRunMaxElapsedMinutesDraft] =
      useState("");
    const [autoRunMaxTokensDraft, setAutoRunMaxTokensDraft] = useState("");
    const [suggesting, setSuggesting] = useState(false);
    const [suggestError, setSuggestError] = useState<string | null>(null);
    const [testingProvider, setTestingProvider] = useState(false);
    const [providerTestResult, setProviderTestResult] =
      useState<ResearchGoalProviderTestResult | null>(null);

    useEffect(() => {
      if (open) {
        setDraft(criteriaDraftFromGoal(goal));
        setMaxCyclesDraft(maxCyclesDraftFromGoal(goal));
        setSecondOpinionProviderEntryDraft(
          secondOpinionProviderEntryDraftFromGoal(goal),
        );
        const autoRunPolicy = autoRunPolicyDraftFromGoal(goal);
        setAutoRunEnabledDraft(autoRunPolicy.enabled);
        setAutoRunCyclesPerRunDraft(String(autoRunPolicy.cyclesPerRun));
        setAutoRunIdleDelayMsDraft(String(autoRunPolicy.idleDelayMs));
        setAutoRunMaxElapsedMinutesDraft(
          autoRunPolicy.maxElapsedMinutes
            ? String(autoRunPolicy.maxElapsedMinutes)
            : "",
        );
        setAutoRunMaxTokensDraft(
          autoRunPolicy.maxTokens ? String(autoRunPolicy.maxTokens) : "",
        );
        setSuggestError(null);
        setProviderTestResult(null);
      }
    }, [
      open,
      goal?.goalId,
      goal?.successCriteria,
      goal?.maxCycles,
      goal?.secondOpinionProviderEntry,
      goal?.autoRunPolicy,
    ]);

    const lines = useMemo(() => criteriaLinesFromDraft(draft), [draft]);
    const validation = validateCriteriaLines(lines);
    const maxCycles = parseMaxCyclesDraft(maxCyclesDraft);
    const maxCyclesValidation = validateMaxCycles(
      maxCycles,
      goal?.currentCycle ?? 0,
    );
    const autoRunCyclesPerRun = parseMaxCyclesDraft(autoRunCyclesPerRunDraft);
    const autoRunIdleDelayMs = parseMaxCyclesDraft(autoRunIdleDelayMsDraft);
    const autoRunMaxElapsedMinutes = parseOptionalPositiveIntegerDraft(
      autoRunMaxElapsedMinutesDraft,
    );
    const autoRunMaxTokens =
      parseOptionalPositiveIntegerDraft(autoRunMaxTokensDraft);
    const autoRunValidation = validateAutoRunPolicyDraft(
      autoRunEnabledDraft,
      autoRunCyclesPerRun,
      autoRunIdleDelayMs,
      autoRunMaxElapsedMinutes,
      autoRunMaxTokens,
    );
    const canSave =
      Boolean(goal) &&
      !saving &&
      !validation &&
      !maxCyclesValidation &&
      !autoRunValidation;
    const providerEntryToTest = secondOpinionProviderEntryDraft.trim();
    const canTestProvider =
      Boolean(onTestSecondOpinionProvider) &&
      Boolean(providerEntryToTest) &&
      !saving &&
      !testingProvider;
    const handleSuggestCriteria = async () => {
      if (!goal || !onSuggestCriteria || saving || suggesting) return;
      setSuggesting(true);
      setSuggestError(null);
      try {
        const suggested = await onSuggestCriteria();
        setDraft(suggested.join("\n"));
      } catch (err: unknown) {
        setSuggestError(
          err instanceof Error
            ? err.message
            : typeof err === "string"
              ? err
              : "LLM 成功标准生成失败",
        );
      } finally {
        setSuggesting(false);
      }
    };
    const handleTestSecondOpinionProvider = async () => {
      if (!onTestSecondOpinionProvider || !providerEntryToTest || testingProvider) {
        return;
      }
      setTestingProvider(true);
      setProviderTestResult(null);
      try {
        const result = await onTestSecondOpinionProvider(providerEntryToTest);
        setProviderTestResult(result);
      } catch (err: unknown) {
        setProviderTestResult({
          available: false,
          error: errorMessageFromUnknown(err, "二审 provider 测试失败"),
        });
      } finally {
        setTestingProvider(false);
      }
    };

    return (
      <Dialog open={open && Boolean(goal)} onClose={saving ? undefined : onClose} fullWidth maxWidth="sm">
        <DialogTitle>编辑科研目标设置</DialogTitle>
        <DialogContent>
          <Stack spacing={1.5} sx={{ pt: 0.5 }}>
            {goal && (
              <Box>
                <Typography variant="caption" color="text.secondary">
                  当前科研目标
                </Typography>
                <Typography variant="body2" sx={{ mt: 0.25 }}>
                  {goal.objective}
                </Typography>
              </Box>
            )}
            <TextField
              label="最大轮次预算"
              value={maxCyclesDraft}
              onChange={(event) => setMaxCyclesDraft(event.target.value)}
              type="number"
              inputProps={{
                min: Math.max(1, goal?.currentCycle ?? 0),
                max: MAX_GOAL_CYCLES,
              }}
              fullWidth
              disabled={saving}
              helperText="用于限制 /goal run 的总轮次；增加预算可让 budget_limited 目标继续推进。"
            />
            <Box
              sx={{
                border: 1,
                borderColor: "divider",
                borderRadius: 1.5,
                p: 1.25,
              }}
            >
              <FormControlLabel
                control={
                  <Checkbox
                    checked={autoRunEnabledDraft}
                    disabled={saving}
                    onChange={(event) =>
                      setAutoRunEnabledDraft(event.target.checked)
                    }
                  />
                }
                label="持久化自动续跑策略"
              />
              <Typography
                variant="caption"
                color="text.secondary"
                component="div"
                sx={{ mb: 1 }}
              >
                开启后，Chat 在主会话空闲时自动触发 /goal run，并随科研目标保存/恢复。
              </Typography>
              <Stack direction={{ xs: "column", sm: "row" }} spacing={1}>
                <TextField
                  label="每次最多轮数"
                  value={autoRunCyclesPerRunDraft}
                  onChange={(event) =>
                    setAutoRunCyclesPerRunDraft(event.target.value)
                  }
                  type="number"
                  inputProps={{ min: 1, max: MAX_AUTO_RUN_CYCLES }}
                  fullWidth
                  disabled={saving || !autoRunEnabledDraft}
                />
                <TextField
                  label="空闲延迟 ms"
                  value={autoRunIdleDelayMsDraft}
                  onChange={(event) =>
                    setAutoRunIdleDelayMsDraft(event.target.value)
                  }
                  type="number"
                  inputProps={{
                    min: MIN_AUTO_RUN_IDLE_DELAY_MS,
                    max: MAX_AUTO_RUN_IDLE_DELAY_MS,
                  }}
                  fullWidth
                  disabled={saving || !autoRunEnabledDraft}
                />
                <TextField
                  label="最长耗时分钟"
                  value={autoRunMaxElapsedMinutesDraft}
                  onChange={(event) =>
                    setAutoRunMaxElapsedMinutesDraft(event.target.value)
                  }
                  type="number"
                  inputProps={{ min: 1, max: MAX_AUTO_RUN_ELAPSED_MINUTES }}
                  placeholder="不限制"
                  fullWidth
                  disabled={saving || !autoRunEnabledDraft}
                />
                <TextField
                  label="Token 预算"
                  value={autoRunMaxTokensDraft}
                  onChange={(event) =>
                    setAutoRunMaxTokensDraft(event.target.value)
                  }
                  type="number"
                  inputProps={{ min: 1, max: MAX_AUTO_RUN_TOKENS }}
                  placeholder="不限制"
                  fullWidth
                  disabled={saving || !autoRunEnabledDraft}
                />
              </Stack>
            </Box>
            <Stack direction={{ xs: "column", sm: "row" }} spacing={1}>
              <Autocomplete<ResearchGoalProviderEntryOption, false, false, true>
                freeSolo
                options={providerEntryOptions}
                value={secondOpinionProviderEntryDraft}
                inputValue={secondOpinionProviderEntryDraft}
                onInputChange={(_, value) => {
                  setSecondOpinionProviderEntryDraft(value);
                  setProviderTestResult(null);
                }}
                onChange={(_, value) => {
                  setSecondOpinionProviderEntryDraft(
                    typeof value === "string" ? value : value?.name ?? "",
                  );
                  setProviderTestResult(null);
                }}
                getOptionLabel={(option) =>
                  typeof option === "string" ? option : option.name
                }
                isOptionEqualToValue={(option, value) =>
                  option.name === (typeof value === "string" ? value : value.name)
                }
                loading={providerEntryOptionsLoading}
                disabled={saving}
                noOptionsText="没有可用 provider entry"
                loadingText="加载 provider entries…"
                renderOption={(props, option) => (
                  <Box component="li" {...props}>
                    <Box>
                      <Typography variant="body2" fontWeight={700}>
                        {option.name}
                      </Typography>
                      <Typography variant="caption" color="text.secondary">
                        {providerEntryOptionLabel(option)}
                        {option.isDefault ? " · 默认" : ""}
                      </Typography>
                    </Box>
                  </Box>
                )}
                renderInput={(params) => (
                  <TextField
                    {...params}
                    label="二审 Provider Entry（覆盖全局，可选）"
                    placeholder="选择或输入 provider entry；留空使用全局设置"
                    helperText="仅当前科研目标使用；保存时会校验 entry 是否存在、启用且 API key 可用。"
                  />
                )}
                sx={{ flex: 1 }}
              />
              {onTestSecondOpinionProvider && (
                <Button
                  type="button"
                  variant="outlined"
                  disabled={!canTestProvider}
                  onClick={() => void handleTestSecondOpinionProvider()}
                  sx={{
                    alignSelf: { xs: "stretch", sm: "flex-start" },
                    mt: { sm: 1 },
                    minWidth: 108,
                    textTransform: "none",
                    fontWeight: 700,
                  }}
                >
                  {testingProvider ? "测试中…" : "测试"}
                </Button>
              )}
            </Stack>
            {providerTestResult && (
              <Alert severity={providerTestResult.available ? "success" : "error"}>
                {providerTestResultMessage(providerTestResult)}
              </Alert>
            )}
            {maxCyclesValidation && (
              <Alert severity="warning">{maxCyclesValidation}</Alert>
            )}
            {autoRunValidation && (
              <Alert severity="warning">{autoRunValidation}</Alert>
            )}
            <TextField
              label="成功标准"
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              multiline
              minRows={6}
              fullWidth
              disabled={saving}
              helperText="每行一条标准；保存后会清空上一轮完成审计，下一次 /goal run 将按新标准重新判断。"
            />
            <Box sx={{ display: "flex", justifyContent: "flex-end", mt: -0.75 }}>
              <Button
                type="button"
                size="small"
                variant="text"
                disabled={saving || suggesting || !goal || !onSuggestCriteria}
                onClick={() => void handleSuggestCriteria()}
                sx={{ textTransform: "none", fontWeight: 700 }}
              >
                {suggesting ? "LLM 生成中…" : "LLM 生成成功标准"}
              </Button>
            </Box>
            {validation && <Alert severity="warning">{validation}</Alert>}
            {suggestError && <Alert severity="error">{suggestError}</Alert>}
            {error && <Alert severity="error">{error}</Alert>}
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={onClose} disabled={saving}>
            取消
          </Button>
          <Button
            variant="contained"
            disableElevation
            onClick={() => {
              if (canSave && maxCycles !== null) {
                void onSave({
                  criteria: lines,
                  maxCycles,
                  secondOpinionProviderEntry:
                    secondOpinionProviderEntryDraft.trim(),
                  autoRunPolicy: {
                    enabled: autoRunEnabledDraft,
                    cyclesPerRun: autoRunEnabledDraft
                      ? (autoRunCyclesPerRun ?? DEFAULT_AUTO_RUN_CYCLES_PER_RUN)
                      : DEFAULT_AUTO_RUN_CYCLES_PER_RUN,
                    idleDelayMs: autoRunEnabledDraft
                      ? (autoRunIdleDelayMs ?? DEFAULT_AUTO_RUN_IDLE_DELAY_MS)
                      : DEFAULT_AUTO_RUN_IDLE_DELAY_MS,
                    maxElapsedMinutes:
                      autoRunEnabledDraft && autoRunMaxElapsedMinutes
                        ? autoRunMaxElapsedMinutes
                        : null,
                    maxTokens:
                      autoRunEnabledDraft && autoRunMaxTokens
                        ? autoRunMaxTokens
                        : null,
                  },
                });
              }
            }}
            disabled={!canSave}
          >
            {saving ? "保存中…" : "保存设置"}
          </Button>
        </DialogActions>
      </Dialog>
    );
  },
);
