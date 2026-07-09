import type { ElementType } from "react";
import {
  Box,
  Button,
  Chip,
  Paper,
  Stack,
  Typography,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import {
  AutoGraphRounded,
  ArticleRounded,
  BiotechRounded,
  FactCheckRounded,
  FlagRounded,
  InsertChartRounded,
  MenuBookRounded,
} from "@mui/icons-material";
import { OmigaLogo } from "../OmigaLogo";

export type ResearchTaskLauncherCardId =
  | "literature-review"
  | "data-analysis"
  | "result-interpretation"
  | "paper-figures"
  | "long-term-goal"
  | "paper-draft";

type ResearchTaskTone =
  | "primary"
  | "success"
  | "secondary"
  | "warning"
  | "error"
  | "info";

interface ResearchTaskLauncherCard {
  id: ResearchTaskLauncherCardId;
  title: string;
  description: string;
  outcome: string;
  commandLabel: string;
  prompt: string;
  tone: ResearchTaskTone;
  recommended?: boolean;
}

export const RESEARCH_TASK_LAUNCHER_CARDS: readonly ResearchTaskLauncherCard[] = [
  {
    id: "literature-review",
    title: "做文献综述",
    description: "把研究问题拆成关键词、证据表和结论边界。",
    outcome: "论文清单 + 证据矩阵",
    commandLabel: "证据综述",
    tone: "primary",
    recommended: true,
    prompt:
      "/research 请围绕以下科研问题做一份可追溯的文献综述：\n\n研究问题：\n研究对象/领域：\n时间范围或关键词：\n\n交付物：关键论文表、证据矩阵、共识/争议、结论边界和下一步阅读建议。",
  },
  {
    id: "data-analysis",
    title: "分析实验数据",
    description: "从已有数据开始，生成分阶段分析路线和可复现记录。",
    outcome: "分析计划 + 图表建议",
    commandLabel: "数据分析",
    tone: "success",
    prompt:
      "/schedule 请为以下科研数据分析目标制定并推进一个分阶段计划：\n\n研究目标：\n已有数据：\n希望比较的组别/变量：\n\n交付物：分析步骤、质量控制、图表建议、结果解释和可复现记录。",
  },
  {
    id: "result-interpretation",
    title: "解读结果",
    description: "区分事实、推断和不确定性，避免过度解释。",
    outcome: "结论 + 局限性 + 下一步",
    commandLabel: "结果解读",
    tone: "secondary",
    prompt:
      "/research 请解读以下科研结果，并严格区分事实、推断和不确定性：\n\n结果/图表/表格：\n需要回答的问题：\n可能的背景信息：\n\n交付物：主要结论、支持证据、局限性、替代解释和下一步验证建议。",
  },
  {
    id: "paper-figures",
    title: "规划论文图表",
    description: "把结果组织成投稿友好的图表故事线。",
    outcome: "图表清单 + 图注草稿",
    commandLabel: "论文图表",
    tone: "warning",
    prompt:
      "/schedule 请根据以下科研结果规划论文图表：\n\n数据/结果：\n目标期刊或图表风格：\n需要突出的科学问题：\n\n交付物：图表清单、每张图的表达目的、图注草稿和可复现绘图方案。",
  },
  {
    id: "long-term-goal",
    title: "设定长期课题",
    description: "把模糊想法变成可持续推进、可审计的科研目标。",
    outcome: "目标 + 成功标准 + 循环推进",
    commandLabel: "长期目标",
    tone: "error",
    prompt:
      "/goal 请长期推进以下科研目标：\n\n目标：\n已有基础：\n成功标准：形成证据可追溯的报告，并在证据不足时明确缺口和下一步。\n\n交付物：目标拆解、成功标准、第一轮执行建议和后续循环推进计划。",
  },
  {
    id: "paper-draft",
    title: "撰写论文草稿",
    description: "把已有结果组织成摘要、引言、方法和讨论草稿。",
    outcome: "论文大纲 + 摘要 + 初稿",
    commandLabel: "科研写作",
    tone: "info",
    prompt:
      "/schedule 请基于以下科研材料撰写论文或报告草稿：\n\n研究主题：\n已有结果/图表：\n目标期刊或受众：\n\n交付物：论文大纲、摘要草稿、各章节写作要点、关键证据缺口和下一步补充建议。",
  },
];

const RESEARCH_TASK_ICONS: Record<ResearchTaskLauncherCardId, ElementType> = {
  "literature-review": MenuBookRounded,
  "data-analysis": AutoGraphRounded,
  "result-interpretation": FactCheckRounded,
  "paper-figures": InsertChartRounded,
  "long-term-goal": FlagRounded,
  "paper-draft": ArticleRounded,
};

export function getResearchTaskLauncherPrompt(
  id: ResearchTaskLauncherCardId,
): string {
  return (
    RESEARCH_TASK_LAUNCHER_CARDS.find((card) => card.id === id)?.prompt ?? ""
  );
}

export function mergeResearchTaskPromptWithDraft(
  prompt: string,
  draft: string,
): string {
  const trimmedPrompt = prompt.trim();
  const trimmedDraft = draft.trim();

  if (!trimmedDraft) return trimmedPrompt;
  if (!trimmedPrompt) return trimmedDraft;
  if (trimmedPrompt.includes(trimmedDraft)) return trimmedPrompt;

  return `${trimmedPrompt}\n\n用户已输入的背景/问题：\n${trimmedDraft}`;
}

export function researchTaskPromptHasBlankFields(prompt: string): boolean {
  return prompt
    .split(/\r?\n/u)
    .some((line) => /^[^：:\n]{2,24}[：:]\s*$/u.test(line.trim()));
}

export function researchTaskPromptHasBlankResearchIntakeFields(
  prompt: string,
): boolean {
  return prompt
    .split(/\r?\n/u)
    .some((line) =>
      /^(研究问题|研究对象\/领域|时间范围或关键词)[：:]\s*$/u.test(
        line.trim(),
      ),
    );
}

export function shouldRequestResearchTaskIntake(content: string): boolean {
  const trimmed = content.trim();
  if (!/^\/research(?:\s|$)/iu.test(trimmed)) return false;
  return (
    researchTaskPromptHasBlankResearchIntakeFields(trimmed) ||
    /^\/research\s+run\s*$/iu.test(trimmed)
  );
}

export interface ResearchTaskIntakeAnswers {
  researchQuestion: string;
  researchScope: string;
  keywordsOrTimeRange: string;
}

export function buildResearchTaskPromptWithIntake(
  prompt: string,
  answers: ResearchTaskIntakeAnswers,
): string {
  let filledQuestion = false;
  let filledScope = false;
  let filledKeywords = false;

  const lines = prompt.split(/\r?\n/u).map((line) => {
    const trimmed = line.trim();
    if (/^研究问题[：:]\s*$/u.test(trimmed)) {
      filledQuestion = true;
      return "研究问题：" + answers.researchQuestion.trim();
    }
    if (/^研究对象\/领域[：:]\s*$/u.test(trimmed)) {
      filledScope = true;
      return "研究对象/领域：" + answers.researchScope.trim();
    }
    if (/^时间范围或关键词[：:]\s*$/u.test(trimmed)) {
      filledKeywords = true;
      return "时间范围或关键词：" + answers.keywordsOrTimeRange.trim();
    }
    return line;
  });

  if (filledQuestion && filledScope && filledKeywords) {
    return lines.join("\n");
  }

  return `${prompt.trim()}\n\n用户补充信息：\n研究问题：${answers.researchQuestion.trim()}\n研究对象/领域：${answers.researchScope.trim()}\n时间范围或关键词：${answers.keywordsOrTimeRange.trim()}`;
}

interface ResearchTaskLauncherProps {
  onSelect: (prompt: string, autoSend: boolean) => void;
  disabled?: boolean;
}

export function ResearchTaskLauncher({
  onSelect,
  disabled = false,
}: ResearchTaskLauncherProps) {
  const theme = useTheme();

  return (
    <Paper
      elevation={0}
      sx={{
        width: "100%",
        maxWidth: 1040,
        p: { xs: 2, md: 2.5 },
        borderRadius: 4,
        border: `1px solid ${alpha(theme.palette.primary.main, 0.12)}`,
        background:
          theme.palette.mode === "dark"
            ? alpha(theme.palette.background.paper, 0.72)
            : `linear-gradient(180deg, ${alpha(
                theme.palette.primary.main,
                0.045,
              )}, ${alpha(theme.palette.background.paper, 0.92)})`,
        boxShadow:
          theme.palette.mode === "dark"
            ? "none"
            : `0 18px 60px ${alpha(theme.palette.common.black, 0.05)}`,
      }}
    >
      <Box sx={{ display: "flex", justifyContent: "center", mb: 1 }}>
        <OmigaLogo size={46} style={{ opacity: 0.92 }} />
      </Box>

      <Stack
        direction={{ xs: "column", md: "row" }}
        spacing={1.5}
        alignItems={{ xs: "flex-start", md: "center" }}
        justifyContent="space-between"
        sx={{ mb: 2 }}
      >
        <Stack direction="row" spacing={1.25} alignItems="flex-start">
          <Box
            sx={{
              width: 40,
              height: 40,
              borderRadius: "14px",
              display: "grid",
              placeItems: "center",
              color: theme.palette.primary.main,
              bgcolor: alpha(theme.palette.primary.main, 0.1),
              flexShrink: 0,
            }}
          >
            <BiotechRounded fontSize="small" />
          </Box>
          <Stack spacing={0.4}>
            <Typography
              variant="overline"
              sx={{ color: "text.secondary", letterSpacing: 0.8 }}
            >
              科研任务入口
            </Typography>
            <Typography
              variant="h5"
              sx={{ fontWeight: 750, color: "text.primary", lineHeight: 1.2 }}
            >
              从一个科研任务开始
            </Typography>
            <Typography
              variant="body2"
              sx={{
                color: "text.secondary",
                maxWidth: 660,
                lineHeight: 1.65,
              }}
            >
              选择任务后，Omiga 会把结构化提示填入输入框；你只需要补充研究问题、数据或结果，再决定是否开始。
            </Typography>
          </Stack>
        </Stack>

        <Stack
          direction="row"
          spacing={0.75}
          useFlexGap
          flexWrap="wrap"
          alignItems="center"
          justifyContent={{ xs: "flex-start", md: "flex-end" }}
        >
          <Chip
            size="small"
            label="推荐：先做文献综述"
            sx={{
              bgcolor: alpha(theme.palette.primary.main, 0.1),
              color: theme.palette.primary.main,
              fontWeight: 700,
            }}
          />
          <Chip size="small" variant="outlined" label="可先填草稿" />
          <Chip size="small" variant="outlined" label="保留证据边界" />
        </Stack>
      </Stack>

      <Box
        sx={{
          display: "grid",
          gridTemplateColumns: {
            xs: "1fr",
            sm: "repeat(2, minmax(0, 1fr))",
            lg: "repeat(3, minmax(0, 1fr))",
          },
          gap: 1.2,
        }}
      >
        {RESEARCH_TASK_LAUNCHER_CARDS.map((card, index) => {
          const Icon = RESEARCH_TASK_ICONS[card.id];
          const accent = theme.palette[card.tone].main;
          return (
            <Paper
              key={card.id}
              elevation={0}
              sx={{
                p: 1.5,
                minHeight: 174,
                display: "flex",
                flexDirection: "column",
                justifyContent: "space-between",
                gap: 1.4,
                cursor: "default",
                border: `1px solid ${alpha(accent, 0.18)}`,
                borderRadius: 2.75,
                borderTop: `3px solid ${alpha(accent, 0.72)}`,
                background:
                  theme.palette.mode === "dark"
                    ? alpha(theme.palette.background.paper, 0.7)
                    : alpha(theme.palette.background.paper, 0.86),
                opacity: disabled ? 0.55 : 1,
                transition:
                  "transform 160ms ease, border-color 160ms ease, box-shadow 160ms ease, background-color 160ms ease",
                "&:hover": disabled
                  ? undefined
                  : {
                      transform: "translateY(-2px)",
                      borderColor: alpha(accent, 0.34),
                      boxShadow: `0 14px 32px ${alpha(accent, 0.1)}`,
                      bgcolor: alpha(accent, 0.035),
                    },
              }}
            >
              <Stack spacing={1.15}>
                <Stack
                  direction="row"
                  spacing={1}
                  alignItems="flex-start"
                  sx={{ minHeight: 70 }}
                >
                  <Box
                    sx={{
                      width: 36,
                      height: 36,
                      borderRadius: "13px",
                      display: "grid",
                      placeItems: "center",
                      color: accent,
                      bgcolor: alpha(accent, 0.12),
                      flexShrink: 0,
                    }}
                  >
                    <Icon fontSize="small" />
                  </Box>
                  <Box sx={{ minWidth: 0, flex: 1 }}>
                    <Stack
                      direction="row"
                      spacing={0.75}
                      alignItems="center"
                      sx={{ mb: 0.35 }}
                    >
                      <Typography
                        variant="subtitle1"
                        sx={{
                          fontWeight: 750,
                          color: "text.primary",
                          lineHeight: 1.25,
                        }}
                      >
                        {card.title}
                      </Typography>
                      {card.recommended ? (
                        <Chip
                          size="small"
                          label="推荐"
                          sx={{
                            height: 20,
                            fontSize: 11,
                            fontWeight: 700,
                            color: accent,
                            bgcolor: alpha(accent, 0.12),
                          }}
                        />
                      ) : null}
                    </Stack>
                    <Typography
                      variant="caption"
                      sx={{
                        color: "text.secondary",
                        lineHeight: 1.5,
                        display: "block",
                        minHeight: "3em",
                      }}
                    >
                      {card.description}
                    </Typography>
                  </Box>
                </Stack>

                <Box
                  sx={{
                    display: "flex",
                    alignItems: "center",
                    gap: 0.8,
                    p: 0.9,
                    borderRadius: 2,
                    bgcolor: alpha(accent, 0.075),
                    color: accent,
                  }}
                >
                  <Typography
                    variant="caption"
                    sx={{ fontWeight: 800, fontVariantNumeric: "tabular-nums" }}
                  >
                    {String(index + 1).padStart(2, "0")}
                  </Typography>
                  <Typography
                    variant="caption"
                    sx={{
                      fontWeight: 700,
                      lineHeight: 1.35,
                      whiteSpace: "normal",
                    }}
                  >
                    {card.outcome}
                  </Typography>
                </Box>
              </Stack>

              <Stack
                direction="row"
                spacing={1}
                alignItems="center"
                justifyContent="space-between"
                sx={{ minHeight: 40 }}
              >
                <Typography
                  variant="caption"
                  sx={{
                    color: "text.disabled",
                    fontWeight: 600,
                    lineHeight: 1,
                    userSelect: "none",
                  }}
                >
                  {card.commandLabel}
                </Typography>
                <Stack direction="row" spacing={0.75}>
                  <Button
                    type="button"
                    size="medium"
                    variant="text"
                    disabled={disabled}
                    onClick={(event) => {
                      event.preventDefault();
                      event.stopPropagation();
                      onSelect(card.prompt, false);
                    }}
                    sx={{ px: 1, minWidth: 0, fontWeight: 700 }}
                  >
                    填入草稿
                  </Button>
                  <Button
                    type="button"
                    size="medium"
                    variant="contained"
                    disabled={disabled}
                    onClick={(event) => {
                      event.preventDefault();
                      event.stopPropagation();
                      onSelect(card.prompt, true);
                    }}
                    sx={{
                      px: 1.5,
                      minWidth: 0,
                      minHeight: 38,
                      fontWeight: 750,
                      bgcolor: accent,
                      "&:hover": { bgcolor: alpha(accent, 0.86) },
                    }}
                  >
                    开始
                  </Button>
                </Stack>
              </Stack>
            </Paper>
          );
        })}
      </Box>
    </Paper>
  );
}
