export interface WorkflowSlashCommandDefinition {
  id: "plan" | "schedule" | "team" | "autopilot" | "research";
  label: string;
  description: string;
}

export const WORKFLOW_SLASH_COMMANDS: WorkflowSlashCommandDefinition[] = [
  {
    id: "plan",
    label: "/plan",
    description: "进入 Plan 模式：先多轮澄清需求，再生成方案，不直接执行。",
  },
  {
    id: "schedule",
    label: "/schedule",
    description: "按阶段执行科研分析计划，适合单主题文献/数据分析。",
  },
  {
    id: "team",
    label: "/team",
    description: "并行协作分析，多路检索/数据处理后由 Leader 综合。",
  },
  {
    id: "autopilot",
    label: "/autopilot",
    description: "全流程科研分析自动驾驶，包含方案、执行、核查与报告。",
  },
  {
    id: "research",
    label: "/research",
    description: "调用分层 Research System。支持 init / list-agents / plan / run / review-traces。",
  },
];

export type WorkflowCommandId = Exclude<
  WorkflowSlashCommandDefinition["id"],
  "research"
>;
export type SlashCommandId = WorkflowSlashCommandDefinition["id"];

export interface ParsedWorkflowCommand {
  command: WorkflowCommandId;
  body: string;
}

export function parseWorkflowCommand(
  input: string,
): ParsedWorkflowCommand | null {
  const trimmed = input.trim();
  const match = trimmed.match(/^\/(plan|schedule|team|autopilot)(?:\s+(.*))?$/iu);
  if (!match) return null;
  return {
    command: match[1].toLowerCase() as WorkflowCommandId,
    body: (match[2] ?? "").trim(),
  };
}

export interface ParsedResearchCommand {
  command: "research";
  body: string;
}

export function parseResearchCommand(
  input: string,
): ParsedResearchCommand | null {
  const trimmed = input.trim();
  const match = trimmed.match(/^\/research(?:\s+(.*))?$/iu);
  if (!match) return null;
  return {
    command: "research",
    body: (match[1] ?? "").trim(),
  };
}
