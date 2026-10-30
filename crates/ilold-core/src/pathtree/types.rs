use serde::{Deserialize, Serialize};

use crate::cfg::types::{BlockKind, BranchEdge, CfgGraph};
use crate::model::expression::CallKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathTree {
    pub contract: String,
    pub function: String,
    pub paths: Vec<ExecutionPath>,
    pub stats: PathTreeStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathTreeStats {
    pub total_paths: usize,
    pub paths_pruned: usize,
    pub max_depth_reached: usize,
    pub revert_paths: usize,
    pub happy_paths: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPath {
    pub id: usize,
    pub nodes: Vec<PathNode>,
    pub terminal: TerminalKind,
    pub annotations: PathAnnotations,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathNode {
    pub block_id: usize,
    pub block_kind: BlockKind,
    pub branch_taken: Option<BranchEdge>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalKind {
    Return,
    Revert,
    DepthCutoff,
    LoopCutoff,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PathAnnotations {
    pub state_writes: Vec<String>,
    pub state_reads: Vec<String>,
    #[serde(default)]
    pub state_write_paths: Vec<String>,
    #[serde(default)]
    pub state_read_paths: Vec<String>,
    /// Calls that leave the execution frame: external, library delegatecall, low-level,
    /// value transfers and contract creation.
    pub external_calls: Vec<ExternalCallInfo>,
    /// JUMPs within the contract hierarchy or into internal library functions, written
    /// `f`, `super.f`, `Base.f` or `Library.f`.
    pub internal_calls: Vec<String>,
    /// `internal_calls` with the callee identity, in the same order.
    #[serde(default)]
    pub internal_call_targets: Vec<InternalCallTarget>,
    pub events_emitted: Vec<String>,
    pub require_checks: Vec<String>,
    pub eth_transfers: Vec<String>,
    pub has_assembly: bool,
}

/// An internal call as written plus the declaration solc resolved it to. Callers dispatch
/// from it: `super.f` continues in the most derived linearization and an unqualified call to
/// a virtual function reaches the visible override.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct InternalCallTarget {
    /// `f`, `super.f`, `Base.f` or `Library.f`.
    pub call: String,
    /// (contract, function key); absent without solc ids or for function pointers.
    pub declaration: Option<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalCallInfo {
    pub target: String,
    pub function: String,
    #[serde(default = "external_kind")]
    pub kind: CallKind,
}

fn external_kind() -> CallKind { CallKind::External }

#[derive(Debug, Clone)]
pub struct FunctionAnalysis {
    pub contract: String,
    pub function: String,
    pub cfg: CfgGraph,
    pub path_tree: PathTree,
}
