import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { normalizeAgentDisplayName } from "../state/agentStore";

type AgentRoleInfoDto = {
  agent_type: string;
  when_to_use: string;
  source: string;
  model_tier: string;
  explicit_model?: string | null;
  background: boolean;
  user_facing: boolean;
};

export type AgentRoleCatalogEntry = {
  agentType: string;
  displayName: string;
  whenToUse: string;
  source: string;
  modelTier: string;
  explicitModel?: string | null;
  background: boolean;
  userFacing: boolean;
};

const FALLBACK_AGENT_ROLE_COPY: Record<
  string,
  Pick<AgentRoleCatalogEntry, "whenToUse" | "source" | "modelTier" | "background" | "userFacing">
> = {
  auto: {
    whenToUse: "用于默认对话或未显式指定专职角色时的通用协作。",
    source: "BuiltIn",
    modelTier: "Standard",
    background: false,
    userFacing: true,
  },
  "general-purpose": {
    whenToUse: "用于通用协作、主调度和跨阶段衔接。",
    source: "BuiltIn",
    modelTier: "Standard",
    background: false,
    userFacing: true,
  },
  Explore: {
    whenToUse: "快速检索代码、文件和只读上下文。",
    source: "BuiltIn",
    modelTier: "Spark",
    background: true,
    userFacing: false,
  },
  Plan: {
    whenToUse: "拆解需求、形成步骤和明确执行边界。",
    source: "BuiltIn",
    modelTier: "Frontier",
    background: false,
    userFacing: true,
  },
  verification: {
    whenToUse: "核查输出、证据和完成条件。",
    source: "BuiltIn",
    modelTier: "Frontier",
    background: true,
    userFacing: false,
  },
  executor: {
    whenToUse: "执行实现、整合结果并推动任务落地。",
    source: "BuiltIn",
    modelTier: "Standard",
    background: true,
    userFacing: false,
  },
  architect: {
    whenToUse: "处理高复杂度设计、边界和长期权衡。",
    source: "BuiltIn",
    modelTier: "Frontier",
    background: true,
    userFacing: false,
  },
  debugger: {
    whenToUse: "定位错误、回归和异常行为的根因。",
    source: "BuiltIn",
    modelTier: "Standard",
    background: true,
    userFacing: false,
  },
  "test-engineer": {
    whenToUse: "验证测试策略、覆盖和可靠性。",
    source: "BuiltIn",
    modelTier: "Frontier",
    background: true,
    userFacing: false,
  },
  critic: {
    whenToUse: "对方案、结论或执行结果做批判式审查。",
    source: "BuiltIn",
    modelTier: "Frontier",
    background: true,
    userFacing: false,
  },
  "code-reviewer": {
    whenToUse: "聚焦代码质量、回归风险和可维护性审查。",
    source: "BuiltIn",
    modelTier: "Frontier",
    background: true,
    userFacing: false,
  },
  "security-reviewer": {
    whenToUse: "聚焦权限边界、漏洞和安全风险。",
    source: "BuiltIn",
    modelTier: "Frontier",
    background: true,
    userFacing: false,
  },
  "quality-reviewer": {
    whenToUse: "聚焦逻辑缺陷、复杂度和长期维护质量。",
    source: "BuiltIn",
    modelTier: "Standard",
    background: true,
    userFacing: false,
  },
  "api-reviewer": {
    whenToUse: "聚焦接口契约、兼容性和输入输出边界。",
    source: "BuiltIn",
    modelTier: "Standard",
    background: true,
    userFacing: false,
  },
  "performance-reviewer": {
    whenToUse: "聚焦性能瓶颈、复杂度和运行成本。",
    source: "BuiltIn",
    modelTier: "Standard",
    background: true,
    userFacing: false,
  },
  "literature-search": {
    whenToUse: "检索文献、资料和外部证据。",
    source: "BuiltIn",
    modelTier: "Spark",
    background: true,
    userFacing: false,
  },
  "deep-research": {
    whenToUse: "做更重的综合研究和多来源归纳。",
    source: "BuiltIn",
    modelTier: "Frontier",
    background: true,
    userFacing: false,
  },
  "data-analysis": {
    whenToUse: "分析数据、比较结果和提炼结论。",
    source: "BuiltIn",
    modelTier: "Standard",
    background: true,
    userFacing: false,
  },
  "data-visual": {
    whenToUse: "生成图表方案和可视化表达。",
    source: "BuiltIn",
    modelTier: "Standard",
    background: true,
    userFacing: false,
  },
  "mind_hunter.intake": {
    whenToUse: "用于 Intake 阶段，暴露假设、歧义和执行路径。",
    source: "MVP",
    modelTier: "Standard",
    background: false,
    userFacing: false,
  },
  "planner.task_graph": {
    whenToUse: "把用户目标转换成结构化 TaskGraph。",
    source: "MVP",
    modelTier: "Standard",
    background: false,
    userFacing: false,
  },
  "executor.supervisor": {
    whenToUse: "调度任务图、收敛上下文并监督执行。",
    source: "MVP",
    modelTier: "Standard",
    background: false,
    userFacing: false,
  },
  "seeker.web_research": {
    whenToUse: "检索来源、归纳证据并标记不确定性。",
    source: "MVP",
    modelTier: "Standard",
    background: false,
    userFacing: false,
  },
  "processor.data": {
    whenToUse: "清洗数据、做格式转换和预处理。",
    source: "MVP",
    modelTier: "Standard",
    background: false,
    userFacing: false,
  },
  "analyzer.data": {
    whenToUse: "分析证据或数据并形成解释。",
    source: "MVP",
    modelTier: "Standard",
    background: false,
    userFacing: false,
  },
  "algorithm.method": {
    whenToUse: "做方法选型、算法比较和 trade-off 说明。",
    source: "MVP",
    modelTier: "Standard",
    background: false,
    userFacing: false,
  },
  "programmer.code": {
    whenToUse: "按任务规范生成或修改代码实现。",
    source: "MVP",
    modelTier: "Standard",
    background: false,
    userFacing: false,
  },
  "debugger.error": {
    whenToUse: "定位错误原因并给出修复建议。",
    source: "MVP",
    modelTier: "Standard",
    background: false,
    userFacing: false,
  },
  "painter.visualization": {
    whenToUse: "给出可视化方案、图表建议和表达结构。",
    source: "MVP",
    modelTier: "Standard",
    background: false,
    userFacing: false,
  },
  "reporter.final": {
    whenToUse: "汇总上游结果并生成最终报告。",
    source: "MVP",
    modelTier: "Standard",
    background: false,
    userFacing: false,
  },
  "biologist.domain": {
    whenToUse: "对生物学问题做机制解释和方向建议。",
    source: "MVP",
    modelTier: "Standard",
    background: false,
    userFacing: false,
  },
  "reviewer.verifier": {
    whenToUse: "检查 schema、证据、权限和完成条件。",
    source: "MVP",
    modelTier: "Standard",
    background: false,
    userFacing: false,
  },
  "creator.capability_refactorer": {
    whenToUse: "分析 traces 并提出 Agent 演进 proposal。",
    source: "MVP",
    modelTier: "Standard",
    background: false,
    userFacing: false,
  },
};

let cachedCatalog: Record<string, AgentRoleCatalogEntry> | null = null;
let catalogPromise: Promise<Record<string, AgentRoleCatalogEntry>> | null = null;

function buildFallbackCatalog(): Record<string, AgentRoleCatalogEntry> {
  return Object.fromEntries(
    Object.entries(FALLBACK_AGENT_ROLE_COPY).map(([agentType, entry]) => [
      agentType,
      {
        agentType,
        displayName: normalizeAgentDisplayName(agentType),
        whenToUse: entry.whenToUse,
        source: entry.source,
        modelTier: entry.modelTier,
        explicitModel: null,
        background: entry.background,
        userFacing: entry.userFacing,
      },
    ]),
  );
}

function mergeCatalog(rows: AgentRoleInfoDto[]): Record<string, AgentRoleCatalogEntry> {
  const merged = buildFallbackCatalog();
  for (const row of rows) {
    merged[row.agent_type] = {
      agentType: row.agent_type,
      displayName: normalizeAgentDisplayName(row.agent_type),
      whenToUse: row.when_to_use,
      source: row.source,
      modelTier: row.model_tier,
      explicitModel: row.explicit_model ?? null,
      background: row.background,
      userFacing: row.user_facing,
    };
  }
  return merged;
}

async function loadAgentRoleCatalog(): Promise<Record<string, AgentRoleCatalogEntry>> {
  if (cachedCatalog) {
    return cachedCatalog;
  }
  if (!catalogPromise) {
    catalogPromise = invoke<AgentRoleInfoDto[]>("list_agent_roles")
      .then((rows) => {
        cachedCatalog = mergeCatalog(rows ?? []);
        return cachedCatalog;
      })
      .catch(() => {
        cachedCatalog = buildFallbackCatalog();
        return cachedCatalog;
      })
      .finally(() => {
        catalogPromise = null;
      });
  }
  return catalogPromise;
}

export function getFallbackAgentRoleEntry(agentType: string): AgentRoleCatalogEntry {
  const fallback = buildFallbackCatalog()[agentType];
  if (fallback) {
    return fallback;
  }
  return {
    agentType,
    displayName: normalizeAgentDisplayName(agentType),
    whenToUse: "用于当前任务的专职执行阶段。",
    source: "Unknown",
    modelTier: "Standard",
    explicitModel: null,
    background: false,
    userFacing: false,
  };
}

export function useAgentRoleCatalog() {
  const [catalog, setCatalog] = useState<Record<string, AgentRoleCatalogEntry>>(
    () => cachedCatalog ?? buildFallbackCatalog(),
  );

  useEffect(() => {
    let cancelled = false;
    void loadAgentRoleCatalog().then((loaded) => {
      if (!cancelled) {
        setCatalog(loaded);
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return catalog;
}
