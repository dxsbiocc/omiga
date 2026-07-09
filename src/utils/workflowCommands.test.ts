import { describe, expect, it } from "vitest";
import {
  parseGoalCommand,
  parseSkillCommand,
  parseResearchCommand,
  parseWorkflowCommand,
  shouldInvokeResearchSystemCommand,
  WORKFLOW_SLASH_COMMANDS,
} from "./workflowCommands";

describe("parseWorkflowCommand", () => {
  it("parses /schedule with task body", () => {
    expect(parseWorkflowCommand("/schedule refactor login flow")).toEqual({
      command: "schedule",
      body: "refactor login flow",
    });
  });

  it("parses /plan with task body", () => {
    expect(parseWorkflowCommand("/plan design an agent avatar system")).toEqual({
      command: "plan",
      body: "design an agent avatar system",
    });
  });

  it("parses /team with task body", () => {
    expect(parseWorkflowCommand("/team fix export race condition")).toEqual({
      command: "team",
      body: "fix export race condition",
    });
  });

  it("parses /autopilot with task body", () => {
    expect(parseWorkflowCommand("/autopilot build a verified settings sync")).toEqual({
      command: "autopilot",
      body: "build a verified settings sync",
    });
  });

  it("keeps multiline task-card bodies inside workflow commands", () => {
    expect(
      parseWorkflowCommand(
        "/schedule 请分析实验数据：\n\n研究目标：\n已有数据：",
      ),
    ).toEqual({
      command: "schedule",
      body: "请分析实验数据：\n\n研究目标：\n已有数据：",
    });
  });

  it("returns null for non-workflow slash commands", () => {
    expect(parseWorkflowCommand("/Explore")).toBeNull();
    expect(parseWorkflowCommand("/planthis")).toBeNull();
  });

  it("parses /research commands separately", () => {
    expect(
      parseResearchCommand(
        "/research run 帮我检索单细胞 RNA-seq 差异分析方法",
      ),
    ).toEqual({
      command: "research",
      body: "run 帮我检索单细胞 RNA-seq 差异分析方法",
    });
    expect(parseWorkflowCommand("/research run hello")).toBeNull();
    expect(parseResearchCommand("/orchestrate run hello")).toBeNull();
  });

  it("parses multiline /research and /goal task-card prompts", () => {
    expect(
      parseResearchCommand(
        "/research 请围绕以下科研问题做综述：\n\n研究问题：\n关键词：",
      ),
    ).toEqual({
      command: "research",
      body: "请围绕以下科研问题做综述：\n\n研究问题：\n关键词：",
    });
    expect(
      parseGoalCommand(
        "/goal 请长期推进以下科研目标：\n\n目标：\n成功标准：",
      ),
    ).toEqual({
      command: "goal",
      body: "请长期推进以下科研目标：\n\n目标：\n成功标准：",
    });
  });

  it("keeps natural-language /research tasks on the live chat path", () => {
    expect(shouldInvokeResearchSystemCommand("")).toBe(true);
    expect(shouldInvokeResearchSystemCommand("help")).toBe(true);
    expect(shouldInvokeResearchSystemCommand("plan THRSP literature review")).toBe(false);
    expect(shouldInvokeResearchSystemCommand("run THRSP literature review")).toBe(false);
    expect(
      shouldInvokeResearchSystemCommand(
        "请围绕 THRSP 在肝癌脂质代谢中的功能做文献综述",
      ),
    ).toBe(false);
  });

  it("parses /goal commands separately from regular workflow rewriting", () => {
    expect(parseGoalCommand("/goal 解析 QS 核心基因")).toEqual({
      command: "goal",
      body: "解析 QS 核心基因",
    });
    expect(parseGoalCommand("/goal")).toEqual({ command: "goal", body: "" });
    expect(parseWorkflowCommand("/goal run")).toBeNull();
    expect(parseResearchCommand("/goal run")).toBeNull();
    expect(WORKFLOW_SLASH_COMMANDS.some((command) => command.id === "goal")).toBe(true);
  });
});

describe("parseSkillCommand", () => {
  it("parses $skill with optional args", () => {
    expect(parseSkillCommand("$tdd fix the login test")).toEqual({
      skill: "tdd",
      args: "fix the login test",
    });
  });

  it("parses hyphenated skill names without args", () => {
    expect(parseSkillCommand("$code-review")).toEqual({
      skill: "code-review",
      args: "",
    });
  });

  it("ignores empty or non-skill inputs", () => {
    expect(parseSkillCommand("$")).toBeNull();
    expect(parseSkillCommand("$   ")).toBeNull();
    expect(parseSkillCommand("please use $tdd")).toBeNull();
  });
});
