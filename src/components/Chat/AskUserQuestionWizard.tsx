import { useEffect, useMemo, useState } from "react";
import {
  Box,
  Button,
  Checkbox,
  FormControl,
  FormControlLabel,
  Radio,
  RadioGroup,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import { useTheme } from "@mui/material/styles";
import QuizOutlinedIcon from "@mui/icons-material/QuizOutlined";
import { getChatTokens, type ChatTokenSet } from "./chatTokens";

export interface AskUserQuestionOption {
  label: string;
  description: string;
  preview?: string;
  custom?: boolean;
  customPlaceholder?: string;
}

export interface AskUserQuestionItem {
  question: string;
  header: string;
  multiSelect?: boolean;
  options: AskUserQuestionOption[];
}

function parseMultiLabels(raw: string): string[] {
  return raw
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

function isQuestionAnswered(
  q: AskUserQuestionItem,
  selections: Record<string, string>,
): boolean {
  const qt = q.question.trim();
  const v = (selections[qt] ?? "").trim();
  if (!q.multiSelect) {
    const selected = askUserSelectedOption(q, v);
    if (selected?.custom) {
      return Boolean(askUserCustomValue(v, selected.label));
    }
    return Boolean(selected);
  }
  return parseMultiLabels(v).length > 0;
}

type AskUserQuestionWizardProps = {
  /** Resets step index when a new pending tool call appears */
  resetKey: string;
  questions: AskUserQuestionItem[];
  selections: Record<string, string>;
  onSelectionsChange: React.Dispatch<
    React.SetStateAction<Record<string, string>>
  >;
  onSubmit: () => void;
  /**
   * `inline` — inside tool message card (thread).
   * `composer` — above the input, same band as `PermissionPromptBar`.
   */
  variant?: "inline" | "composer";
};

export function AskUserQuestionWizard({
  resetKey,
  questions,
  selections,
  onSelectionsChange,
  onSubmit,
  variant = "inline",
}: AskUserQuestionWizardProps) {
  const theme = useTheme();
  const CHAT: ChatTokenSet = useMemo(() => getChatTokens(theme), [theme]);
  const [step, setStep] = useState(0);

  useEffect(() => {
    setStep(0);
  }, [resetKey]);

  const total = questions.length;
  const safeIndex = Math.min(Math.max(0, step), Math.max(0, total - 1));
  const q = questions[safeIndex];
  const stepNum = safeIndex + 1;

  const currentAnswered = q ? isQuestionAnswered(q, selections) : false;

  const allAnswered = useMemo(
    () => questions.every((qq) => isQuestionAnswered(qq, selections)),
    [questions, selections],
  );

  const goPrev = () => setStep((s) => Math.max(0, s - 1));
  const goNext = () => setStep((s) => Math.min(total - 1, s + 1));

  const isLast = total <= 1 || safeIndex >= total - 1;

  if (!q || total === 0) return null;

  const isComposer = variant === "composer";

  const body = (
    <>
        <Stack
          direction="row"
          alignItems="flex-start"
          justifyContent="space-between"
          gap={1}
          sx={{ mb: 1.25 }}
        >
          <Typography
            sx={{
              fontSize: 12,
              fontWeight: 600,
              color: CHAT.textPrimary,
              lineHeight: 1.4,
              flex: 1,
              pr: 1,
            }}
          >
            {q.question}
          </Typography>
          {!isComposer && (
            <Typography
              component="span"
              sx={{
                fontSize: 11,
                fontWeight: 600,
                color: CHAT.textMuted,
                flexShrink: 0,
                userSelect: "none",
              }}
            >
              {stepNum} / {total}
            </Typography>
          )}
        </Stack>

        <FormControl component="fieldset" sx={{ display: "block", width: "100%", mb: 1.5 }}>
          {q.multiSelect ? (
            <Stack spacing={0.35}>
              {q.options.map((opt) => {
                const qt = q.question.trim();
                const cur = parseMultiLabels(askUserSelectionRaw(selections, qt));
                const checked = cur.includes(opt.label);
                return (
                  <Box
                    key={opt.label}
                    sx={{
                      borderRadius: "8px",
                      px: 0.75,
                      py: 0.5,
                      bgcolor: checked
                        ? alpha(theme.palette.primary.main, 0.08)
                        : "transparent",
                      transition: "background-color 0.15s ease",
                    }}
                  >
                    <FormControlLabel
                      control={
                        <Checkbox
                          size="small"
                          checked={checked}
                          onChange={() => {
                            onSelectionsChange((prev) => {
                              const qtInner = q.question.trim();
                              const curInner = parseMultiLabels(
                                askUserSelectionRaw(prev, qtInner),
                              );
                              const set = new Set(curInner);
                              if (set.has(opt.label)) set.delete(opt.label);
                              else set.add(opt.label);
                              return {
                                ...prev,
                                [qtInner]: Array.from(set).join(", "),
                              };
                            });
                          }}
                        />
                      }
                      label={
                        <Box>
                          <Typography
                            variant="caption"
                            sx={{ fontWeight: 600, display: "block" }}
                          >
                            {opt.label}
                          </Typography>
                          <Typography
                            variant="caption"
                            sx={{
                              display: "block",
                              color: "text.secondary",
                              lineHeight: 1.35,
                            }}
                          >
                            {opt.description}
                          </Typography>
                        </Box>
                      }
                    />
                  </Box>
                );
              })}
            </Stack>
          ) : (
            <RadioGroup
              value={askUserSelectedOptionLabel(
                q,
                askUserSelectionRaw(selections, q.question.trim()),
              )}
              onChange={(_, v) =>
                onSelectionsChange((prev) => ({
                  ...prev,
                  [q.question.trim()]: q.options.find((opt) => opt.label === v)
                    ?.custom
                    ? formatAskUserCustomSelection(
                        v,
                        askUserCustomValue(prev[q.question.trim()] ?? "", v) ??
                          "",
                      )
                    : v,
                }))
              }
            >
              {q.options.map((opt) => {
                const raw = askUserSelectionRaw(selections, q.question.trim());
                const selected =
                  askUserSelectedOptionLabel(q, raw) === opt.label;
                return (
                  <Box
                    key={opt.label}
                    sx={{
                      borderRadius: "8px",
                      px: 0.75,
                      py: 0.35,
                      bgcolor: selected
                        ? alpha(theme.palette.primary.main, 0.08)
                        : "transparent",
                      transition: "background-color 0.15s ease",
                    }}
                  >
                    <FormControlLabel
                      value={opt.label}
                      control={<Radio size="small" />}
                      label={
                        <Box>
                          <Typography
                            variant="caption"
                            sx={{ fontWeight: 600, display: "block" }}
                          >
                            {opt.label}
                          </Typography>
                          <Typography
                            variant="caption"
                            sx={{
                              display: "block",
                              color: "text.secondary",
                              lineHeight: 1.35,
                            }}
                          >
                            {opt.description}
                          </Typography>
                        </Box>
                      }
                    />
                    {selected && opt.custom ? (
                      <TextField
                        fullWidth
                        size="small"
                        value={askUserCustomValue(raw, opt.label) ?? ""}
                        placeholder={
                          opt.customPlaceholder ?? "输入自定义值后提交"
                        }
                        onChange={(event) => {
                          const value = event.target.value;
                          onSelectionsChange((prev) => ({
                            ...prev,
                            [q.question.trim()]: formatAskUserCustomSelection(
                              opt.label,
                              value,
                            ),
                          }));
                        }}
                        sx={{
                          mt: 0.5,
                          ml: 3.75,
                          width: "calc(100% - 30px)",
                          "& .MuiInputBase-input": {
                            py: 0.65,
                            fontSize: 12,
                          },
                        }}
                      />
                    ) : null}
                  </Box>
                );
              })}
            </RadioGroup>
          )}
        </FormControl>

        <Stack
          direction="row"
          alignItems="center"
          justifyContent="space-between"
          flexWrap="wrap"
          gap={1}
        >
          <Typography sx={{ fontSize: 10, color: CHAT.labelMuted }}>
            {q.multiSelect
              ? `${parseMultiLabels(askUserSelectionRaw(selections, q.question.trim())).length} 项已选`
              : currentAnswered
                ? "已选择"
                : "请选择一项"}
          </Typography>
          <Stack direction="row" spacing={1} alignItems="center">
            {safeIndex > 0 && (
              <Button
                variant="outlined"
                size="small"
                onClick={goPrev}
                sx={{ textTransform: "none", minWidth: 72 }}
              >
                上一步
              </Button>
            )}
            {!isLast && (
              <Button
                variant="contained"
                size="small"
                disabled={!currentAnswered}
                onClick={goNext}
                sx={{ textTransform: "none", minWidth: 88 }}
              >
                下一题
              </Button>
            )}
            {isLast && (
              <Button
                variant="contained"
                size="small"
                disabled={!allAnswered}
                onClick={() => onSubmit()}
                sx={{ textTransform: "none", minWidth: 96 }}
              >
                提交答案
              </Button>
            )}
          </Stack>
        </Stack>
    </>
  );

  if (isComposer) {
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
        <Stack spacing={1.25}>
          <Stack direction="row" alignItems="center" gap={1} flexWrap="wrap">
            <QuizOutlinedIcon
              sx={{ fontSize: 22, color: "primary.main", flexShrink: 0 }}
            />
            <Typography variant="subtitle2" fontWeight={700}>
              需要你的选择
            </Typography>
            <Typography
              component="span"
              sx={{
                fontSize: 11,
                fontWeight: 600,
                color: CHAT.textMuted,
                userSelect: "none",
              }}
            >
              {stepNum} / {total}
            </Typography>
          </Stack>
          {body}
        </Stack>
      </Box>
    );
  }

  return (
    <Box
      sx={{
        mt: 1.5,
        pt: 1.5,
        borderTop: `1px solid ${alpha(CHAT.agentBubbleBorder, 0.85)}`,
      }}
    >
      <Box
        sx={{
          borderRadius: "10px",
          border: `1px solid ${alpha(CHAT.agentBubbleBorder, 0.9)}`,
          bgcolor: alpha(CHAT.outputBg, 0.45),
          p: 1.5,
        }}
      >
        {body}
      </Box>
    </Box>
  );
}

function askUserSelectionRaw(
  selections: Record<string, string>,
  qt: string,
): string {
  return selections[qt] ?? "";
}

function askUserSelectedOption(
  q: AskUserQuestionItem,
  raw: string,
): AskUserQuestionOption | undefined {
  const label = askUserSelectedOptionLabel(q, raw);
  return q.options.find((opt) => opt.label === label);
}

function askUserSelectedOptionLabel(q: AskUserQuestionItem, raw: string): string {
  const trimmed = raw.trim();
  for (const opt of q.options) {
    if (opt.custom && askUserCustomValue(trimmed, opt.label) !== null) {
      return opt.label;
    }
  }
  return trimmed;
}

function askUserCustomValue(raw: string, label: string): string | null {
  const trimmed = raw.trim();
  if (trimmed === label) return "";
  for (const separator of [":", "："]) {
    const prefix = `${label}${separator}`;
    if (trimmed.startsWith(prefix)) {
      return trimmed.slice(prefix.length).trimStart();
    }
  }
  return null;
}

function formatAskUserCustomSelection(label: string, value: string): string {
  const trimmed = value.trimStart();
  return trimmed ? `${label}：${trimmed}` : label;
}
