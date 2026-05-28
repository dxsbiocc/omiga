// Playbook 固化系统的前端类型契约(orchestrator 维护,实现者勿改字段)。
//
// 这些类型**精确镜像** Rust 端 serde 序列化(rename_all = "camelCase";
// PlaybookStatus 为 snake_case)。后端定义见 src-tauri/src/domain/playbooks/types.rs
// 与 src-tauri/src/commands/playbooks.rs。

export type PlaybookStatus = "active" | "stale" | "quarantined";

export interface PlaybookFingerprint {
  canonicalId: string;
  operatorVersion: string;
  paramSchemaHash: string;
  envSignature?: string | null;
}

export interface PlaybookVerification {
  expectedStatus: string;
  expectedOutputKeys: string[];
}

export interface PlaybookProvenance {
  distilledFrom: string[];
  proposalId?: string | null;
  createdAt: string;
}

export interface PlaybookHealth {
  hitCount: number;
  successCount: number;
  lastVerifiedAt?: string | null;
  status: PlaybookStatus;
}

export interface Playbook {
  playbookId: string;
  title: string;
  fingerprint: PlaybookFingerprint;
  /** "chain" | (后续) "operator" | "template" */
  kind: string;
  canonicalId: string;
  operatorVersion: string;
  /** 重放所需的具体参数(链:序列化的 ChainStep[])。 */
  params: unknown;
  inputs: unknown;
  verification: PlaybookVerification;
  provenance: PlaybookProvenance;
  health: PlaybookHealth;
}

/** 链步骤(与后端 ChainStep 一致;保存链 Playbook 时用)。 */
export interface OperatorChainStep {
  alias: string;
  label?: string | null;
  arguments: unknown;
  inheritPrevOutputAs?: string | null;
  dependsOn?: string[];
}

export interface ChainStepResult {
  alias: string;
  ok: boolean;
  runDir?: string | null;
  result: unknown;
  error?: string | null;
}

export interface OperatorChainResult {
  steps: ChainStepResult[];
  ok: boolean;
  error?: string | null;
}

/** `replay_playbook` 命令返回。 */
export interface ReplayPlaybookResponse {
  /** "replayed" | "invalidated" | "notFound" | "inactive" */
  outcome: "replayed" | "invalidated" | "notFound" | "inactive";
  verified?: boolean;
  status?: PlaybookStatus;
  result?: OperatorChainResult;
}
