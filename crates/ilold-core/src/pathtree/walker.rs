use std::collections::HashMap;

use petgraph::stable_graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use crate::cfg::types::*;
use crate::model::common::StateVar;
use crate::model::expression::AssignOperator;

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

pub fn build_path_tree(
    cfg: &CfgGraph,
    contract_name: &str,
    function_name: &str,
    state_vars: &[StateVar],
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

        if let Some(
            BranchEdge::ConditionalTrue { condition } | BranchEdge::ConditionalFalse { condition },
        ) = &state.edge_taken
        {
            scan_reads(condition, state_vars, &mut annotations);
        }

        path.push(PathNode {
            block_id: block.id,
            block_kind: block.kind,
            branch_taken: state.edge_taken,
        });

        collect_annotations(&block.statements, state_vars, &mut annotations);

        if let Some(rv) = &block.return_value {
            scan_reads(rv, state_vars, &mut annotations);
        }

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

fn normalize_path(target: &str) -> String {
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

fn expression_mentions_var(expr: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let bytes = expr.as_bytes();
    let nb = name.as_bytes();
    let mut i = 0;
    while i + nb.len() <= bytes.len() {
        if &bytes[i..i + nb.len()] == nb {
            let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
            let after_idx = i + nb.len();
            let after_ok = after_idx >= bytes.len() || !is_ident_char(bytes[after_idx]);
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn extract_qualified_paths(expr: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    if name.is_empty() {
        return out;
    }
    let bytes = expr.as_bytes();
    let nb = name.as_bytes();
    let mut i = 0;
    while i + nb.len() <= bytes.len() {
        if &bytes[i..i + nb.len()] == nb {
            let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
            let after_idx = i + nb.len();
            let after_ok = after_idx >= bytes.len() || !is_ident_char(bytes[after_idx]);
            if before_ok && after_ok {
                let mut end = after_idx;
                loop {
                    if end < bytes.len() && bytes[end] == b'[' {
                        let mut depth = 1i32;
                        end += 1;
                        while end < bytes.len() && depth > 0 {
                            match bytes[end] {
                                b'[' => depth += 1,
                                b']' => depth -= 1,
                                _ => {}
                            }
                            end += 1;
                        }
                        continue;
                    }
                    if end < bytes.len() && bytes[end] == b'.' {
                        let mut j = end + 1;
                        while j < bytes.len() && is_ident_char(bytes[j]) {
                            j += 1;
                        }
                        if j > end + 1 {
                            end = j;
                            continue;
                        }
                    }
                    break;
                }
                let raw = &expr[i..end];
                let norm = normalize_path(raw);
                if !out.contains(&norm) {
                    out.push(norm);
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn collect_annotations(
    statements: &[CfgStatement],
    state_vars: &[StateVar],
    annotations: &mut PathAnnotations,
) {
    for stmt in statements {
        match stmt {
            CfgStatement::ExternalCall { target, function, arguments, kind, .. } => {
                annotations.external_calls.push(ExternalCallInfo {
                    target: target.clone(),
                    function: function.clone(),
                    kind: *kind,
                });
                scan_reads(arguments, state_vars, annotations);
            }
            CfgStatement::InternalCall { function, arguments, target, declaration, .. } => {
                let call = match target {
                    Some(target) => format!("{target}.{function}"),
                    None => function.clone(),
                };
                annotations.internal_call_targets.push(InternalCallTarget {
                    call: call.clone(),
                    declaration: declaration.clone(),
                });
                annotations.internal_calls.push(call);
                scan_reads(arguments, state_vars, annotations);
            }
            CfgStatement::EmitEvent { event, arguments, .. } => {
                annotations.events_emitted.push(event.clone());
                scan_reads(arguments, state_vars, annotations);
            }
            CfgStatement::EthTransfer { to, .. } => {
                annotations.eth_transfers.push(to.clone());
            }
            CfgStatement::RequireCheck { condition, .. } => {
                annotations.require_checks.push(condition.clone());
                scan_reads(condition, state_vars, annotations);
            }
            CfgStatement::AssertCheck { condition, .. } => {
                annotations.require_checks.push(format!("assert: {condition}"));
                scan_reads(condition, state_vars, annotations);
            }
            CfgStatement::AssemblyBlock { .. } => {
                annotations.has_assembly = true;
            }
            CfgStatement::Assignment { target, value, operator, .. } => {
                let base_name = crate::util::target_base_name(target);
                if state_vars.iter().any(|sv| sv.name == base_name) {
                    annotations.state_writes.push(target.clone());
                    let normalized = normalize_path(target);
                    if !annotations.state_write_paths.contains(&normalized) {
                        annotations.state_write_paths.push(normalized);
                    }
                }
                scan_reads(value, state_vars, annotations);
                if !matches!(operator, AssignOperator::Assign) {
                    scan_reads(target, state_vars, annotations);
                }
            }
            CfgStatement::StateWrite { variable, .. } => {
                annotations.state_writes.push(variable.clone());
                let normalized = normalize_path(variable);
                if !annotations.state_write_paths.contains(&normalized) {
                    annotations.state_write_paths.push(normalized);
                }
            }
            CfgStatement::StateRead { variable, .. } => {
                annotations.state_reads.push(variable.clone());
                let normalized = normalize_path(variable);
                if !annotations.state_read_paths.contains(&normalized) {
                    annotations.state_read_paths.push(normalized);
                }
            }
        }
    }
}

fn scan_reads(text: &str, state_vars: &[StateVar], annotations: &mut PathAnnotations) {
    for sv in state_vars {
        if expression_mentions_var(text, &sv.name) {
            if !annotations.state_reads.contains(&sv.name) {
                annotations.state_reads.push(sv.name.clone());
            }
            for path in extract_qualified_paths(text, &sv.name) {
                if !annotations.state_read_paths.contains(&path) {
                    annotations.state_read_paths.push(path);
                }
            }
        }
    }
}
