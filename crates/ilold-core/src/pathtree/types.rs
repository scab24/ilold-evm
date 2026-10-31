use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::cfg::types::{BlockKind, BranchEdge, CfgGraph};
use crate::model::expression::CallKind;
use crate::util::{push_unique, Page};

/// Paths are stored as indices into a table of distinct nodes, and annotations once per
/// block; each path's annotations are derived on demand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathTree {
    pub contract: String,
    pub function: String,
    pub(crate) nodes: Vec<PathNode>,
    /// What each block adds to the paths that run it, by block id.
    pub(crate) blocks: BTreeMap<usize, PathAnnotations>,
    pub(crate) paths: Vec<StoredPath>,
    pub stats: PathTreeStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredPath {
    pub(crate) nodes: Vec<u32>,
    pub(crate) terminal: TerminalKind,
}

/// Every path ends in exactly one category: `happy_paths` (return), `revert_paths`,
/// `loop_cutoff` (a loop condition reached more than `max_loop_unroll` times) or
/// `depth_cutoff` (more than `max_depth` blocks after the entry), so they add up to `total_paths`. Once `max_paths` paths exist the walk
/// stops: `cap_reached`, with `paths_dropped` unexplored branches of at least one path each.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathTreeStats {
    pub total_paths: usize,
    pub happy_paths: usize,
    pub revert_paths: usize,
    pub loop_cutoff: usize,
    pub depth_cutoff: usize,
    pub cap_reached: bool,
    pub paths_dropped: usize,
    /// Some path was cut off or dropped: the paths do not cover every execution.
    pub truncated: bool,
    pub max_depth_reached: usize,
}

impl PathTreeStats {
    /// Why the paths are incomplete, e.g. `truncated: 12 depth cutoffs, path cap reached (3 branches dropped)`.
    pub fn truncation_note(&self) -> Option<String> {
        if !self.truncated { return None; }
        let mut causes = Vec::new();
        if self.depth_cutoff > 0 { causes.push(format!("{} depth cutoffs", self.depth_cutoff)); }
        if self.loop_cutoff > 0 { causes.push(format!("{} loop cutoffs", self.loop_cutoff)); }
        if self.cap_reached {
            causes.push(format!("path cap reached ({} branches dropped)", self.paths_dropped));
        }
        Some(format!("truncated: {}", causes.join(", ")))
    }
}

/// One page of a path tree with each path's annotations.
#[derive(Debug, Clone, Serialize)]
pub struct PathTreePage {
    pub contract: String,
    pub function: String,
    pub paths: Vec<ExecutionPath>,
    pub stats: PathTreeStats,
    pub page: Page,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPath {
    pub id: usize,
    pub nodes: Vec<PathNode>,
    pub terminal: TerminalKind,
    pub annotations: PathAnnotations,
    pub depth: usize,
}

impl PathTree {
    /// Paths in id order.
    pub fn paths(&self) -> impl Iterator<Item = ExecutionPath> + '_ {
        (0..self.paths.len()).map(|id| self.expand(id))
    }

    /// Each path's id, terminal and nodes, without deriving its annotations.
    pub fn routes(&self) -> impl Iterator<Item = (usize, TerminalKind, impl Iterator<Item = &PathNode> + '_)> + '_ {
        self.paths.iter().enumerate()
            .map(|(id, path)| (id, path.terminal, path.nodes.iter().map(|&n| &self.nodes[n as usize])))
    }

    pub fn page(&self, offset: usize, limit: usize) -> PathTreePage {
        let page = Page::new(offset, limit, self.paths.len());
        PathTreePage {
            contract: self.contract.clone(),
            function: self.function.clone(),
            paths: page.range().map(|id| self.expand(id)).collect(),
            stats: self.stats.clone(),
            page,
        }
    }

    /// Annotations of every block some path runs, once each: their union is the union of
    /// every path's annotations.
    pub fn block_annotations(&self) -> impl Iterator<Item = &PathAnnotations> + '_ {
        let mut run = BTreeSet::new();
        for path in &self.paths {
            run.extend(self.annotated(path).map(|n| self.nodes[n as usize].block_id));
        }
        run.into_iter().filter_map(|block| self.blocks.get(&block))
    }

    // a cut-off path stops before running its last block
    fn annotated<'a>(&self, path: &'a StoredPath) -> impl Iterator<Item = u32> + 'a {
        let cut = matches!(path.terminal, TerminalKind::DepthCutoff | TerminalKind::LoopCutoff);
        path.nodes[..path.nodes.len() - usize::from(cut)].iter().copied()
    }

    fn expand(&self, id: usize) -> ExecutionPath {
        let path = &self.paths[id];
        let mut annotations = PathAnnotations::default();
        for n in self.annotated(path) {
            if let Some(block) = self.blocks.get(&self.nodes[n as usize].block_id) {
                annotations.extend(block);
            }
        }
        ExecutionPath {
            id,
            nodes: path.nodes.iter().map(|&n| self.nodes[n as usize].clone()).collect(),
            terminal: path.terminal,
            annotations,
            depth: path.nodes.len() - 1,
        }
    }
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

impl PathAnnotations {
    /// Appends what a later block adds, keeping the deduplicated lists unique.
    pub(crate) fn extend(&mut self, block: &PathAnnotations) {
        self.state_writes.extend(block.state_writes.iter().cloned());
        for read in &block.state_reads { push_unique(&mut self.state_reads, read.clone()); }
        for path in &block.state_write_paths { push_unique(&mut self.state_write_paths, path.clone()); }
        for path in &block.state_read_paths { push_unique(&mut self.state_read_paths, path.clone()); }
        self.external_calls.extend(block.external_calls.iter().cloned());
        self.internal_calls.extend(block.internal_calls.iter().cloned());
        self.internal_call_targets.extend(block.internal_call_targets.iter().cloned());
        self.events_emitted.extend(block.events_emitted.iter().cloned());
        self.require_checks.extend(block.require_checks.iter().cloned());
        self.eth_transfers.extend(block.eth_transfers.iter().cloned());
        self.has_assembly |= block.has_assembly;
    }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
