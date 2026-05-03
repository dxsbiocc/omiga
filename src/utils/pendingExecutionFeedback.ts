import type { SlashCommandId } from "./workflowCommands";

export interface PendingExecutionFeedbackInput {
  workflowCommand?: SlashCommandId | null;
  composerAgentType?: string | null;
}

export interface PendingExecutionFeedback {
  connectLabel: string;
  assistantHint: string | null;
}

const DEFAULT_FEEDBACK: PendingExecutionFeedback = {
  connectLabel: "等待响应",
  assistantHint: null,
};

export function buildPendingExecutionFeedback(
  input: PendingExecutionFeedbackInput,
): PendingExecutionFeedback {
  const workflowCommand = input.workflowCommand ?? null;
  const composerAgentType = (input.composerAgentType ?? "").trim();

  if (workflowCommand === "plan" || composerAgentType === "Plan") {
    return {
      connectLabel: "生成计划中",
      assistantHint: "正在生成结构化计划与待办清单…",
    };
  }

  if (workflowCommand === "schedule") {
    return {
      connectLabel: "生成调度计划中",
      assistantHint: "正在拆解任务并安排执行顺序…",
    };
  }

  if (workflowCommand === "team") {
    return {
      connectLabel: "组建团队中",
      assistantHint: "正在拆解任务并分配 Team 角色…",
    };
  }

  if (workflowCommand === "autopilot") {
    return {
      connectLabel: "规划执行中",
      assistantHint: "正在规划自动执行路径与核查阶段…",
    };
  }

  if (workflowCommand === "research") {
    return {
      connectLabel: "Research 编排中",
      assistantHint: "正在调用分层 Research System 命令…",
    };
  }

  if (workflowCommand === "goal") {
    return {
      connectLabel: "科研目标推进中",
      assistantHint: "正在围绕长期科研目标执行分析、解读与完成审计…",
    };
  }

  return DEFAULT_FEEDBACK;
}
