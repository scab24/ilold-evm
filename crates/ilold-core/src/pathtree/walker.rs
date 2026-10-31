use std::collections::HashMap;

use petgraph::stable_graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use crate::cfg::types::*;
use crate::model::common::StateVar;
use crate::util::push_unique;

use super::config::PruningConfig;
use super::types::*;

struct WalkState {
    node: NodeIndex,
    path: Vec<PathNode>,
    annotations: PathAnnotations,
    depth: usize,
    loop_counts: HashMap<usize, usize>,
    edge_taken: Option<BranchEdge>,
}

/// Enumerates the paths of `cfg`. Reads and writes come from the storage the CFG builder
/// resolved, so `_state_vars` is no longer consulted.
pub fn build_path_tree(
    cfg: &CfgGraph,
    contract_name: &str,
    function_name: &str,
    _state_vars: &[StateVar],
    config: &PruningConfig,
) -> PathTree {
    let mut paths: Vec<ExecutionPath> = Vec::new();
    let mut paths_pruned: usize = 0;
    let mut max_depth_reached: usize = 0;
    let mut next_path_id: usize = 0;

    let entry = match cfg.node_indices().find(|&n| cfg[n].kind == BlockKind::Entry) {
        Some(n) => n,
        None => {
            return PathTree {
                contract: contract_name.to_string(),
                function: function_name.to_string(),
                paths: Vec::new(),
                stats: PathTreeStats {
                    total_paths: 0,
                    paths_pruned: 0,
                    max_depth_reached: 0,
                    revert_paths: 0,
                    happy_paths: 0,
                },
            };
        }
    };

    let mut stack: Vec<WalkState> = vec![WalkState {
        node: entry,
        path: Vec::new(),
        annotations: PathAnnotations::default(),
        depth: 0,
        loop_counts: HashMap::new(),
        edge_taken: None,
    }];

    while let Some(state) = stack.pop() {
        if paths.len() >= config.max_paths {
            paths_pruned += 1;
            continue;
        }

        let block = &cfg[state.node];
        let mut path = state.path;
        let mut annotations = state.annotations;
        let mut loop_counts = state.loop_counts;
        let depth = state.depth;

        if depth > max_depth_reached {
            max_depth_reached = depth;
        }

        if depth > config.max_depth {
            paths_pruned += 1;
            path.push(PathNode {
                block_id: block.id,
                block_kind: block.kind,
                branch_taken: state.edge_taken,
            });
            let id = next_path_id;
            next_path_id += 1;
            paths.push(ExecutionPath {
                id,
                nodes: path,
                terminal: TerminalKind::DepthCutoff,
                annotations,
                depth,
            });
            continue;
        }

        if block.kind == BlockKind::LoopCondition {
            let count = loop_counts.entry(block.id).or_insert(0);
            *count += 1;
            if *count > config.max_loop_unroll {
                paths_pruned += 1;
                path.push(PathNode {
                    block_id: block.id,
                    block_kind: block.kind,
                    branch_taken: state.edge_taken,
                });
                let id = next_path_id;
                next_path_id += 1;
                paths.push(ExecutionPath {
                    id,
                    nodes: path,
                    terminal: TerminalKind::LoopCutoff,
                    annotations,
                    depth,
                });
                continue;
            }
        }

        path.push(PathNode {
            block_id: block.id,
            block_kind: block.kind,
            branch_taken: state.edge_taken,
        });

        collect_reads(&block.reads, &mut annotations);
        collect_annotations(&block.statements, &mut annotations);

        match block.kind {
            BlockKind::Return => {
                let id = next_path_id;
                next_path_id += 1;
                paths.push(ExecutionPath {
                    id,
                    nodes: path,
                    terminal: TerminalKind::Return,
                    annotations,
                    depth,
                });
                continue;
            }
            BlockKind::Revert => {
                let id = next_path_id;
                next_path_id += 1;
                paths.push(ExecutionPath {
                    id,
                    nodes: path,
                    terminal: TerminalKind::Revert,
                    annotations,
                    depth,
                });
                continue;
            }
            _ => {}
        }

        let edges: Vec<_> = cfg
            .edges_directed(state.node, Direction::Outgoing)
            .map(|e| (e.target(), e.weight().clone()))
            .collect();

        if edges.is_empty() {
            if block.kind == BlockKind::Entry && block.statements.is_empty() {
                continue;
            }
            let id = next_path_id;
            next_path_id += 1;
            paths.push(ExecutionPath {
                id,
                nodes: path,
                terminal: TerminalKind::Return,
                annotations,
                depth,
            });
            continue;
        }

        for (i, (target, edge)) in edges.into_iter().enumerate() {
            let (p, a, lc) = if i == 0 {
                (path.clone(), annotations.clone(), loop_counts.clone())
            } else {
                (path.clone(), annotations.clone(), loop_counts.clone())
            };
            stack.push(WalkState {
                node: target,
                path: p,
                annotations: a,
                depth: depth + 1,
                loop_counts: lc,
                edge_taken: Some(edge),
            });
        }
    }

    let revert_paths = paths.iter().filter(|p| p.terminal == TerminalKind::Revert).count();
    let happy_paths = paths.iter().filter(|p| p.terminal == TerminalKind::Return).count();

    PathTree {
        contract: contract_name.to_string(),
        function: function_name.to_string(),
        paths,
        stats: PathTreeStats {
            total_paths: next_path_id,
            paths_pruned,
            max_depth_reached,
            revert_paths,
            happy_paths,
        },
    }
}

/// `a[x].b[y]` -> `a[].b[]`.
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

// state the block reads; reads through `storage` parameters are summarised per function
fn collect_reads(reads: &[StorageRef], annotations: &mut PathAnnotations) {
    for read in reads.iter().filter(|r| r.param.is_none()) {
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
                annotations.state_reads.push(variable.clone());
                push_unique(&mut annotations.state_read_paths, normalize_path(variable));
            }
        }
    }
}
