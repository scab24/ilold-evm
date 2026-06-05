use serde::{Deserialize, Serialize};

use crate::cfg::types::{BlockKind, BranchEdge, CfgGraph};

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
    pub external_calls: Vec<ExternalCallInfo>,
    pub internal_calls: Vec<String>,
    pub events_emitted: Vec<String>,
    pub require_checks: Vec<String>,
    pub eth_transfers: Vec<String>,
    pub has_assembly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalCallInfo {
    pub target: String,
    pub function: String,
}

#[derive(Debug, Clone)]
pub struct FunctionAnalysis {
    pub contract: String,
    pub function: String,
    pub cfg: CfgGraph,
    pub path_tree: PathTree,
}
