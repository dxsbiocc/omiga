//! Playbook 固化系统 —— 冻结的数据契约(orchestrator 维护,实现者勿改字段)。
//!
//! 设计依据:`docs/PLAYBOOK_CRYSTALLIZATION_DESIGN.md`。
//!
//! 这些类型是匹配 / 重放 / 治理三条路径的共同契约。`Fingerprint::index_key` 是
//! 唯一在本文件中实现的方法,因为它是 fingerprint 与 store 两侧的共同依赖,必须先于
//! 二者存在以保证各自独立编译。其余行为分别在 `fingerprint.rs` / `store.rs` 实现。

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

/// 单元分隔符,用于把多个字段拼成无歧义的规范字符串再做哈希。
/// 选 0x1F(ASCII Unit Separator)因为它不会出现在 id / 版本 / 哈希文本里。
const FIELD_SEP: u8 = 0x1f;

/// 匹配与失效的**唯一依据**。两个 Fingerprint 逐字段相等才算精确命中(L2)。
///
/// - `canonical_id`:被固化单元的规范 id(如 `plugin/template/foo`)。
/// - `operator_version`:单元版本,**失效触发器**——版本一变,index_key 即变,旧
///   Playbook 自动失配。必须在执行当下从活 spec 采集(`ExecutionRecord` 不含版本)。
/// - `param_schema_hash`:MVP 取"参数值哈希"(复用 `execution_records::hash_execution_map`
///   的产物),保证只命中**完全相同**的参数;Phase 2+ 再放宽为形状哈希。
/// - `env_signature`:运行时 / 平台签名,非必要时为 `None`。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    pub canonical_id: String,
    pub operator_version: String,
    pub param_schema_hash: String,
    #[serde(default)]
    pub env_signature: Option<String>,
}

impl Fingerprint {
    /// 稳定索引键:对四个字段做 SHA-256,输出十六进制串。
    ///
    /// 契约保证(实现者与测试都依赖):
    /// 1. **确定性**:同样的字段值,任何进程、任何次调用都得到同一字符串。
    /// 2. **跨进程稳定**:用 SHA-256 而非 `DefaultHasher`(后者不保证跨版本稳定),
    ///    因为该键会被持久化进磁盘索引。
    /// 3. **无碰撞歧义**:字段间用 `FIELD_SEP` 分隔,避免 `("ab","c")` 与
    ///    `("a","bc")` 撞键。
    pub fn index_key(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.canonical_id.as_bytes());
        hasher.update([FIELD_SEP]);
        hasher.update(self.operator_version.as_bytes());
        hasher.update([FIELD_SEP]);
        hasher.update(self.param_schema_hash.as_bytes());
        hasher.update([FIELD_SEP]);
        hasher.update(self.env_signature.as_deref().unwrap_or("").as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

/// 重放后用于判定"这次复用是否真的成功"的验证契约。
///
/// MVP 的验证是 operator 原生的:重放执行的 `status` 必须等于 `expected_status`,
/// 且输出键集合必须**包含** `expected_output_keys` 全部条目。验证**在所有路径都跑**,
/// 快路径省的是规划、不是验证。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybookVerification {
    /// 期望状态,通常为 `"succeeded"`。
    pub expected_status: String,
    /// 期望输出键(来自固化时 `output_summary.outputKeys`)。
    #[serde(default)]
    pub expected_output_keys: Vec<String>,
}

/// 固化来源 / 审计信息,呼应 proposal-first 的"可审计、可撤销"原则。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    /// 蒸馏 / 固化所依据的 `ExecutionRecord` id(可多条)。
    #[serde(default)]
    pub distilled_from: Vec<String>,
    /// 若经 learning proposal 流固化,记录其 proposal id;手动保存则为 `None`。
    #[serde(default)]
    pub proposal_id: Option<String>,
    /// 批准 / 创建时间(RFC3339)。
    pub created_at: String,
}

/// Playbook 生命周期状态。
/// - `Active`:可参与匹配。
/// - `Stale`:指纹陈旧(如长期未验证),暂不参与匹配,等待复核。
/// - `Quarantined`:成功率跌破阈值被自动退役,退出匹配池等待人工处置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybookStatus {
    Active,
    Stale,
    Quarantined,
}

/// 闭环的反馈端:每次命中重放都回写,供 auto-demote 与可观测使用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    /// 命中并尝试重放的总次数。
    #[serde(default)]
    pub hit_count: u64,
    /// 重放且通过验证的次数。
    #[serde(default)]
    pub success_count: u64,
    /// 最近一次验证通过的时间(RFC3339);从未成功则 `None`。
    #[serde(default)]
    pub last_verified_at: Option<String>,
    pub status: PlaybookStatus,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            hit_count: 0,
            success_count: 0,
            last_verified_at: None,
            status: PlaybookStatus::Active,
        }
    }
}

/// 一份被验证过、可重放、带指纹的固化执行单元。
///
/// MVP 锚定单个 template/operator 调用:`canonical_id` + 完整 `params`/`inputs` 即可
/// 确定性重放。`params` 存**具体值**(不是哈希),因为重放需要真实参数;`param_hash`
/// 存于 `fingerprint.param_schema_hash` 用于匹配。多步 chain 的扩展留待后续(届时把
/// `params` 升级为 `Vec<PlaybookStep>`)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playbook {
    pub playbook_id: String,
    pub title: String,
    pub fingerprint: Fingerprint,
    /// 被固化单元的类型:`"template"` |(后续)`"operator"` | `"chain"`。
    pub kind: String,
    pub canonical_id: String,
    pub operator_version: String,
    /// 重放所需的具体参数(完整值,非哈希)。
    pub params: JsonValue,
    /// 重放所需的具体输入(完整值,非哈希)。
    #[serde(default)]
    pub inputs: JsonValue,
    pub verification: PlaybookVerification,
    pub provenance: Provenance,
    #[serde(default)]
    pub health: Health,
}

/// Playbook 持久化与查询契约。
///
/// 实现要求(`store.rs` 必须满足,测试会校验):
/// - `find_by_fingerprint` 必须是 **O(1)** 查表(按 `Fingerprint::index_key` 建索引),
///   **禁止**对全部 Playbook 线性扫描比对。
/// - 所有写操作返回 `Result`,失败给出可读错误,不 panic、不静默吞错。
/// - 持久化布局参照 `research_system/stores.rs::JsonFileTaskGraphStore`
///   (每个 Playbook 一个 `<playbook_id>.json`)。
/// `Send` 超 trait:重放编排会在 `.await` 间隙持有 `&mut dyn PlaybookStore`,
/// 故 trait 对象必须 `Send`(tauri 命令返回的 future 要求 `Send`)。
pub trait PlaybookStore: Send {
    /// 保存(新增或覆盖)一个 Playbook,并维护指纹索引。
    fn save(&mut self, playbook: Playbook) -> Result<(), String>;
    /// 按 id 取回。
    fn get(&self, playbook_id: &str) -> Option<Playbook>;
    /// 按指纹 **O(1)** 查找;命中要求 `index_key` 相等。
    /// 仅返回 `status == Active` 的 Playbook(退役 / 陈旧的不参与匹配)。
    fn find_by_fingerprint(&self, fingerprint: &Fingerprint) -> Option<Playbook>;
    /// 列出全部 Playbook(含非 Active),用于管理面板与可观测。
    fn list(&self) -> Vec<Playbook>;
    /// 删除一个 Playbook,并清理其指纹索引。
    fn delete(&mut self, playbook_id: &str) -> Result<(), String>;
}

// ───────────────────────── Phase 1 契约(orchestrator 维护) ─────────────────────────

/// 凭引用重放前的解析结果(`replay.rs::resolve_for_replay` 返回)。
///
/// 重放只在 `Ready` 时进行;其余分支都应让调用方回退到正常的链路规划。
#[derive(Debug, Clone)]
pub enum ReplayResolution {
    /// 仍 Active 且指纹与当前算子版本一致——可安全重放。
    Ready(Playbook),
    /// 算子版本漂移(指纹不再匹配)——已失效,回退正常规划。
    Invalidated,
    /// 不存在该 playbook。
    NotFound,
    /// 存在但非 Active(Stale / Quarantined)。
    Inactive,
}

/// auto-demote 触发阈值:累计重放达到 [`DEMOTE_MIN_ATTEMPTS`] 次后,
/// 若成功率低于 [`DEMOTE_SUCCESS_RATE`] 则 Quarantine,退出匹配池。
pub const DEMOTE_MIN_ATTEMPTS: u64 = 3;
/// auto-demote 的成功率下限(success_count / hit_count)。
pub const DEMOTE_SUCCESS_RATE: f64 = 0.5;
/// 探索阀门默认概率:命中后仍以此概率冷规划一次,防止能力固化。
pub const DEFAULT_EXPLORE_EPSILON: f64 = 0.1;
