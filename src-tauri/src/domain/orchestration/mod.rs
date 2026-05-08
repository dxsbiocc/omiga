//! Orchestration helpers for mode-specific runtime control.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionLane {
    pub lane_id: &'static str,
    pub preferred_agent_type: Option<&'static str>,
    pub supplemental_agent_types: &'static [&'static str],
    pub instructions: &'static str,
}

pub mod autopilot;
pub mod ralph;
pub mod team;
