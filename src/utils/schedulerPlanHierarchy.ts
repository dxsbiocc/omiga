export type SchedulerTaskStage =
  | "intent"
  | "retrieve"
  | "download"
  | "analyze"
  | "visualize"
  | "report"
  | "verify"
  | "debug"
  | "synthesize"
  | "other";

export interface SchedulerPlanHierarchyTask {
  id: string;
  description: string;
  agentType: string;
  dependencies: string[];
  critical: boolean;
  estimatedSecs?: number;
  supervisorAgentType?: string | null;
  stage?: SchedulerTaskStage | string | null;
}

export interface SchedulerPlanHierarchyInput {
  entryAgentType?: string | null;
  executionSupervisorAgentType?: string | null;
  subtasks: SchedulerPlanHierarchyTask[];
}

export interface SchedulerPlanHierarchy {
  legacyFlat: boolean;
  entryAgentType: string;
  executionSupervisorAgentType: string;
  children: SchedulerPlanHierarchyTask[];
}

export const TASK_STAGE_LABELS: Record<string, string> = {
  intent: "意图解析",
  retrieve: "资料/数据检索",
  download: "数据获取",
  analyze: "数据分析",
  visualize: "可视化",
  report: "报告撰写",
  verify: "结果核查",
  debug: "问题排查",
  synthesize: "综合汇报",
  other: "其他",
};

function cleanAgent(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

export function buildSchedulerPlanHierarchy(
  plan: SchedulerPlanHierarchyInput,
): SchedulerPlanHierarchy {
  const explicitHierarchy = Boolean(
    cleanAgent(plan.entryAgentType) ||
      cleanAgent(plan.executionSupervisorAgentType) ||
      plan.subtasks.some(
        (task) => cleanAgent(task.supervisorAgentType) || Boolean(task.stage),
      ),
  );

  const entryAgentType = cleanAgent(plan.entryAgentType) ?? "general-purpose";
  const executionSupervisorAgentType =
    cleanAgent(plan.executionSupervisorAgentType) ?? "executor";

  return {
    legacyFlat: !explicitHierarchy,
    entryAgentType,
    executionSupervisorAgentType,
    children: plan.subtasks.map((task) => ({
      ...task,
      supervisorAgentType:
        cleanAgent(task.supervisorAgentType) ?? executionSupervisorAgentType,
      stage: task.stage ?? "other",
    })),
  };
}

export function schedulerStageLabel(stage: string | null | undefined): string {
  return TASK_STAGE_LABELS[stage ?? "other"] ?? stage ?? TASK_STAGE_LABELS.other;
}
