use serde::{Deserialize, Serialize};
use std::fmt;

use crate::classify::entry_points::AccessLevel;
use crate::pathtree::types::TerminalKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionNarrative {
    pub contract: String,
    pub name: String,
    pub access: AccessLevel,
    pub total_paths: usize,
    pub happy_paths: usize,
    pub revert_paths: usize,
    pub paths: Vec<PathNarrative>,
    pub observations: Vec<Observation>,
    pub state_writes: Vec<String>,
    pub state_reads: Vec<String>,
    pub external_calls: Vec<String>,
    #[serde(default)]
    pub internal_calls: Vec<String>,
    pub modifiers: Vec<String>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub transitive_state_writes: Vec<TransitiveEffect>,
    #[serde(default)]
    pub transitive_state_reads: Vec<TransitiveEffect>,
    #[serde(default)]
    pub transitive_external_calls: Vec<TransitiveEffect>,
    #[serde(default)]
    pub transitive_events: Vec<TransitiveEffect>,
    /// Reverts here or in a callee (path counts only cover this function's own code).
    #[serde(default)]
    pub can_revert: bool,
    /// Where it can revert: own checks with an empty `via`, then those reached through calls.
    #[serde(default)]
    pub revert_sources: Vec<TransitiveEffect>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitiveEffect {
    pub via: Vec<String>,
    pub item: String,
    pub origin_contract: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathNarrative {
    pub id: usize,
    pub terminal: TerminalKind,
    pub steps: Vec<NarrativeStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NarrativeStep {
    pub step_type: StepType,
    pub description: String,
    pub detail: Option<String>,
    pub branch: Option<BranchDirection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepType {
    Entry,
    Condition,
    StateWrite,
    StateRead,
    ExternalCall,
    InternalCall,
    EthTransfer,
    Event,
    Return,
    Revert,
    Assembly,
}

impl StepType {
    pub fn icon(&self) -> &str {
        match self {
            StepType::Entry => "▶",
            StepType::Condition => "▸",
            StepType::StateWrite => "✏",
            StepType::StateRead => "◇",
            StepType::ExternalCall => "→",
            StepType::InternalCall => "○",
            StepType::EthTransfer => "Ξ",
            StepType::Event => "◆",
            StepType::Return => "✓",
            StepType::Revert => "✗",
            StepType::Assembly => "⚙",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BranchDirection {
    True,
    False,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub kind: ObservationKind,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservationKind {
    WriteAfterExternalCall,
    SharedState,
    NoAccessControl,
}

impl fmt::Display for ObservationKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ObservationKind::WriteAfterExternalCall => write!(f, "Write after external call"),
            ObservationKind::SharedState => write!(f, "Shared state"),
            ObservationKind::NoAccessControl => write!(f, "No access control"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceNarrative {
    pub contract: String,
    pub steps: Vec<SequenceStep>,
    pub observations: Vec<Observation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceStep {
    pub function: String,
    pub access: AccessLevel,
    pub requires: Vec<String>,
    pub effects: Vec<String>,
    pub external_calls: Vec<String>,
    pub events: Vec<String>,
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub flow_summary: Option<FlowSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub from_step: usize,
    pub variable: String,
    pub relationship: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowSummary {
    pub total_steps: usize,
    pub mutation_count: usize,
    pub external_call_count: usize,
    pub internal_call_count: usize,
    pub depth_limited_count: usize,
    pub mutation_refs: Vec<MutationRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationRef {
    pub variable: String,
    pub flow_step_id: usize,
    pub session_step_index: usize,
}
