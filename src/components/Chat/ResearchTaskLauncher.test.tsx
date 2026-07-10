import { renderToStaticMarkup } from "react-dom/server";
import { createTheme, ThemeProvider } from "@mui/material/styles";
import { describe, expect, it, vi } from "vitest";
import {
  RESEARCH_TASK_LAUNCHER_CARDS,
  ResearchTaskLauncher,
  buildResearchTaskPromptWithIntake,
  getResearchTaskLauncherPrompt,
  mergeResearchTaskPromptWithDraft,
  researchTaskPromptHasBlankFields,
  shouldRequestResearchTaskIntake,
} from "./ResearchTaskLauncher";
import {
  parseGoalCommand,
  parseResearchCommand,
  parseWorkflowCommand,
} from "../../utils/workflowCommands";

function renderLauncher() {
  return renderToStaticMarkup(
    <ThemeProvider theme={createTheme()}>
      <ResearchTaskLauncher onSelect={vi.fn()} />
    </ThemeProvider>,
  );
}

describe("ResearchTaskLauncher", () => {
  it("offers a focused set of novice-friendly scientific workflows", () => {
    expect(RESEARCH_TASK_LAUNCHER_CARDS.map((card) => card.id)).toEqual([
      "literature-review",
      "data-analysis",
      "result-interpretation",
      "paper-figures",
      "long-term-goal",
      "paper-draft",
    ]);

    for (const card of RESEARCH_TASK_LAUNCHER_CARDS) {
      expect(card.title).not.toMatch(/agent|codex|开发/i);
      expect(card.description).not.toMatch(/agent|codex|开发/i);
      expect(card.prompt).toContain("交付物");
    }
  });

  it("keeps card prompts routed through the research-oriented command surface", () => {
    expect(getResearchTaskLauncherPrompt("literature-review")).toMatch(
      /^\/research\b/,
    );
    expect(getResearchTaskLauncherPrompt("data-analysis")).toMatch(
      /^\/schedule\b/,
    );
    expect(getResearchTaskLauncherPrompt("long-term-goal")).toMatch(
      /^\/goal\b/,
    );
  });

  it("keeps multiline card prompts parseable by their command routers", () => {
    expect(
      parseResearchCommand(getResearchTaskLauncherPrompt("literature-review"))
        ?.body,
    ).toContain("研究问题：");
    expect(
      parseWorkflowCommand(getResearchTaskLauncherPrompt("data-analysis"))
        ?.command,
    ).toBe("schedule");
    expect(
      parseGoalCommand(getResearchTaskLauncherPrompt("long-term-goal"))?.body,
    ).toContain("成功标准");
    expect(
      parseWorkflowCommand(getResearchTaskLauncherPrompt("paper-draft"))
        ?.command,
    ).toBe("schedule");
  });

  it("preserves an existing composer draft when applying a task prompt", () => {
    const prompt = getResearchTaskLauncherPrompt("result-interpretation");
    const merged = mergeResearchTaskPromptWithDraft(
      prompt,
      "我已经发现 A 组的 IL6 显著升高，但不确定如何解释。",
    );

    expect(merged).toContain(prompt);
    expect(merged).toContain("用户已输入的背景/问题");
    expect(merged).toContain("IL6 显著升高");
  });

  it("detects blank structured prompts before auto execution", () => {
    expect(
      researchTaskPromptHasBlankFields(
        getResearchTaskLauncherPrompt("literature-review"),
      ),
    ).toBe(true);
    expect(
      researchTaskPromptHasBlankFields(
        "/research 请围绕 THRSP 在乳腺癌脂代谢中的作用做综述",
      ),
    ).toBe(false);
  });

  it("fills required intake fields before a research task can run", () => {
    const filled = buildResearchTaskPromptWithIntake(
      getResearchTaskLauncherPrompt("literature-review"),
      {
        researchQuestion: "THRSP 是否影响乳腺癌脂代谢重塑？",
        researchScope: "乳腺癌代谢组学与转录组学",
        keywordsOrTimeRange: "THRSP, lipid metabolism, breast cancer, 2020-2026",
      },
    );

    expect(filled).toContain("研究问题：THRSP 是否影响乳腺癌脂代谢重塑？");
    expect(filled).toContain("研究对象/领域：乳腺癌代谢组学与转录组学");
    expect(filled).toContain(
      "时间范围或关键词：THRSP, lipid metabolism, breast cancer, 2020-2026",
    );
    expect(researchTaskPromptHasBlankFields(filled)).toBe(false);
  });

  it("requests intake for blank /research runs but not for other workflow cards", () => {
    expect(
      shouldRequestResearchTaskIntake(
        getResearchTaskLauncherPrompt("literature-review"),
      ),
    ).toBe(true);
    expect(shouldRequestResearchTaskIntake("/research run")).toBe(true);
    expect(
      shouldRequestResearchTaskIntake(
        getResearchTaskLauncherPrompt("data-analysis"),
      ),
    ).toBe(false);
    expect(
      shouldRequestResearchTaskIntake(
        getResearchTaskLauncherPrompt("result-interpretation"),
      ),
    ).toBe(false);
    expect(
      shouldRequestResearchTaskIntake(
        "/research 请围绕 THRSP 在乳腺癌脂代谢中的作用做综述",
      ),
    ).toBe(false);
  });

  it("renders task cards without exposing developer-first labels", () => {
    const html = renderLauncher();

    expect(html).toContain("科研任务入口");
    expect(html).toContain("做文献综述");
    expect(html).toContain("分析实验数据");
    expect(html).toContain("设定长期课题");
    expect(html).toContain("撰写论文草稿");
    expect(html).toContain("填入");
    expect(html).toContain("开始");
    expect(html).not.toContain("Codex");
    expect(html).not.toContain("Claude");
  });

  it("keeps card containers non-interactive and uses explicit button controls", () => {
    const html = renderLauncher();
    const buttonCount = html.match(/type="button"/g)?.length ?? 0;

    expect(html).not.toContain('role="button"');
    expect(buttonCount).toBeGreaterThanOrEqual(
      RESEARCH_TASK_LAUNCHER_CARDS.length * 2,
    );
  });
});
