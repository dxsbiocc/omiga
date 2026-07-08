use crate::domain::agents::scheduler::{SchedulingResult, SchedulingStrategy};
use crate::domain::skills::SkillCacheMap;
use crate::llm::LlmConfig;
use std::sync::{Arc, Mutex as StdMutex};

pub(super) struct TurnSpawnContext {
    pub(super) llm_config: LlmConfig,
    pub(super) skill_cache: Arc<StdMutex<SkillCacheMap>>,
    pub(super) scheduler: Option<SchedulingResult>,
    pub(super) project_root_str: String,
    pub(super) is_team_mode: bool,
    pub(super) is_ralph_mode: bool,
    pub(super) is_autopilot_mode: bool,
    pub(super) is_explicit_execution_workflow: bool,
    pub(super) ralph_env: Option<String>,
    pub(super) autopilot_env: Option<String>,
    pub(super) strategy: SchedulingStrategy,
}
