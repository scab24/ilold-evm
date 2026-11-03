use std::collections::{BTreeMap, HashMap};

use petgraph::stable_graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use crate::cfg::types::*;
use crate::util::push_unique;

use super::config::PruningConfig;
use super::types::*;

struct WalkState {
    node: NodeIndex,
    path: Vec<u32>,
    depth: usize,
    loop_counts: HashMap<usize, usize>,
    edge: Option<EdgeIndex>,
}

pub fn build_path_tree(
    cfg: &CfgGraph,
    contract_name: &str,
    function_name: &str,
    config: &PruningConfig,
) -> PathTree {
    let mut tree = PathTree {
        contract: contract_name.to_string(),
        function: function_name.to_string(),
        nodes: Vec::new(),
        blocks: BTreeMap::new(),
        paths: Vec::new(),
        stats: PathTreeStats::default(),
    };
    // one table entry per CFG edge taken (the entry block has none)
    let mut node_ids: HashMap<Option<EdgeIndex>, u32> = HashMap::new();

    let Some(entry) = cfg.node_indices().find(|&n| cfg[n].kind == BlockKind::Entry) else {
        return tree;
    };

    let mut stack: Vec<WalkState> = vec![WalkState {
        node: entry,
        path: Vec::new(),
        depth: 0,
        loop_counts: HashMap::new(),
        edge: None,
    }];

    while let Some(state) = stack.pop() {
        if tree.paths.len() >= config.max_paths {
            tree.stats.cap_reached = true;
            tree.stats.paths_dropped = stack.len() + 1;
            break;
        }

        let block = &cfg[state.node];
        let mut path = state.path;
        let mut loop_counts = state.loop_counts;
        let depth = state.depth;
        tree.stats.max_depth_reached = tree.stats.max_depth_reached.max(depth);

        let node = *node_ids.entry(state.edge).or_insert_with(|| {
            tree.nodes.push(PathNode {
                block_id: block.id,
                block_kind: block.kind,
                branch_taken: state.edge.map(|e| cfg[e].clone()),
            });
            (tree.nodes.len() - 1) as u32
        });
        path.push(node);

        if block.kind == BlockKind::LoopCondition {
            *loop_counts.entry(block.id).or_insert(0) += 1;
        }
        let cutoff = if depth > config.max_depth {
            Some(TerminalKind::DepthCutoff)
        } else if loop_counts.get(&block.id).is_some_and(|&count| count > config.max_loop_unroll) {
            Some(TerminalKind::LoopCutoff)
        } else {
            None
        };
        if let Some(terminal) = cutoff {
            tree.paths.push(StoredPath { nodes: path, terminal });
            continue;
        }

        tree.blocks.entry(block.id).or_insert_with(|| block_annotations(block));

        let terminal = match block.kind {
            BlockKind::Return => Some(TerminalKind::Return),
            BlockKind::Revert => Some(TerminalKind::Revert),
            _ => None,
        };
        let edges: Vec<_> = cfg.edges_directed(state.node, Direction::Outgoing).map(|e| (e.target(), e.id())).collect();
        if edges.is_empty() && block.kind == BlockKind::Entry && block.statements.is_empty() {
            continue;
        }
        if terminal.is_some() || edges.is_empty() {
            tree.paths.push(StoredPath { nodes: path, terminal: terminal.unwrap_or(TerminalKind::Return) });
            continue;
        }

        for (target, edge) in edges {
            stack.push(WalkState {
                node: target,
                path: path.clone(),
                depth: depth + 1,
                loop_counts: loop_counts.clone(),
                edge: Some(edge),
            });
        }
    }

    tree.blocks.retain(|_, annotations| *annotations != PathAnnotations::default());
    let stats = &mut tree.stats;
    for path in &tree.paths {
        match path.terminal {
            TerminalKind::Return => stats.happy_paths += 1,
            TerminalKind::Revert => stats.revert_paths += 1,
            TerminalKind::LoopCutoff => stats.loop_cutoff += 1,
            TerminalKind::DepthCutoff => stats.depth_cutoff += 1,
        }
    }
    stats.total_paths = tree.paths.len();
    stats.truncated = stats.cap_reached || stats.loop_cutoff + stats.depth_cutoff > 0;
    tree
}

pub fn normalize_path(target: &str) -> String {
    let mut out = String::with_capacity(target.len());
    let mut depth = 0i32;
    for ch in target.chars() {
        match ch {
            '[' => {
                if depth == 0 {
                    out.push('[');
                }
                depth += 1;
            }
            ']' => {
                depth -= 1;
                if depth == 0 {
                    out.push(']');
                }
            }
            _ if depth > 0 => {}
            _ => out.push(ch),
        }
    }
    out
}

pub(crate) fn block_annotations(block: &BasicBlock) -> PathAnnotations {
    let mut annotations = PathAnnotations::default();
    collect_reads(&block.reads, &mut annotations);
    collect_annotations(&block.statements, &mut annotations);
    annotations
}

// reads through `storage` parameters are summarised per function
fn collect_reads(reads: &[BlockRead], annotations: &mut PathAnnotations) {
    for read in reads.iter().map(|r| &r.place).filter(|r| r.param.is_none()) {
        push_unique(&mut annotations.state_reads, crate::util::target_base_name(&read.path).to_string());
        push_unique(&mut annotations.state_read_paths, normalize_path(&read.path));
    }
}

fn record_write(path: &str, annotations: &mut PathAnnotations) {
    annotations.state_writes.push(path.to_string());
    push_unique(&mut annotations.state_write_paths, normalize_path(path));
}

fn collect_annotations(statements: &[CfgStatement], annotations: &mut PathAnnotations) {
    for stmt in statements {
        match stmt {
            CfgStatement::ExternalCall { target, function, kind, .. } => {
                annotations.external_calls.push(ExternalCallInfo {
                    target: target.clone(),
                    function: function.clone(),
                    kind: *kind,
                });
            }
            CfgStatement::InternalCall { function, target, declaration, .. } => {
                let call = match target {
                    Some(target) => format!("{target}.{function}"),
                    None => function.clone(),
                };
                annotations.internal_call_targets.push(InternalCallTarget {
                    call: call.clone(),
                    declaration: declaration.clone(),
                });
                annotations.internal_calls.push(call);
            }
            CfgStatement::EmitEvent { event, .. } => {
                annotations.events_emitted.push(event.clone());
            }
            CfgStatement::EthTransfer { to, .. } => {
                annotations.eth_transfers.push(to.clone());
            }
            CfgStatement::RequireCheck { condition, .. } => {
                annotations.require_checks.push(condition.clone());
            }
            CfgStatement::AssertCheck { condition, .. } => {
                annotations.require_checks.push(format!("assert: {condition}"));
            }
            CfgStatement::AssemblyBlock { .. } => {
                annotations.has_assembly = true;
            }
            CfgStatement::UnresolvedModifier { .. } => {}
            CfgStatement::Assignment { storage: Some(place), .. } if place.param.is_none() => {
                record_write(&place.path, annotations);
            }
            CfgStatement::Assignment { .. } => {}
            CfgStatement::StateWrite { variable, param: None, .. } => record_write(variable, annotations),
            CfgStatement::StateWrite { .. } => {}
            CfgStatement::StateRead { variable, .. } => {
                push_unique(&mut annotations.state_reads, variable.clone());
                push_unique(&mut annotations.state_read_paths, normalize_path(variable));
            }
        }
    }
}
