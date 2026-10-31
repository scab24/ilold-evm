
use std::collections::{HashMap, HashSet};

use petgraph::visit::EdgeRef;
use petgraph::Direction;
use serde::{Deserialize, Serialize};

use crate::cfg::types::{BlockKind, BranchEdge, CfgGraph, CfgStatement};
use crate::model::contract::ContractDef;
use crate::model::decl_id::DeclId;
use crate::model::expression::AssignOperator;
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::pathtree::types::PathTreeStats;

const CANONICAL_WALK_SAFETY_CAP: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowTree {
    pub contract: String,
    pub function: String,
    pub signature: String,
    pub modifiers: Vec<String>,
    pub max_depth: usize,
    pub root: FlowNode,
    /// Path enumeration of the traced function, when the analysis has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_stats: Option<PathTreeStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowNode {
    pub step_id: usize,
    pub depth: usize,
    pub kind: FlowKind,
    #[serde(default)]
    pub from_modifier: Option<String>,
    pub children: Vec<FlowNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FlowKind {
    Entry {
        signature: String,
    },
    Require {
        condition: String,
        message: Option<String>,
    },
    Assert {
        condition: String,
    },
    Write {
        target: String,
        value: String,
        op: AssignOperator,
    },
    StateWrite {
        variable: String,
    },
    StateRead {
        variable: String,
    },
    InternalCall {
        function: String,
        origin: String,
        depth_limited: bool,
        ops_count: usize,
    },
    ExternalCall {
        target: String,
        function: String,
    },
    EmitEvent {
        name: String,
    },
    BranchTrue {
        condition: String,
    },
    BranchFalse {
        condition: String,
    },
    LoopHeader {
        kind: String,
    },
    Return,
    Revert {
        reason: Option<String>,
    },
    EthTransfer {
        to: String,
    },
    AssemblyBlock,
    DepthLimit,
}

pub struct FlowConfig {
    pub max_depth: usize,
    pub include_reverts: bool,
    pub expand_set: HashSet<usize>,
}

impl Default for FlowConfig {
    fn default() -> Self {
        FlowConfig {
            max_depth: 2,
            include_reverts: false,
            expand_set: HashSet::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FlowMutation {
    pub flow_step_id: usize,
    pub target: String,
    pub value: String,
    pub operator: AssignOperator,
    pub via: Option<String>,
}

pub fn build_flow_tree(
    contract: &ContractDef,
    function: &FunctionDef,
    cfg: &CfgGraph,
    project: &Project,
    all_cfgs: &HashMap<(String, String), CfgGraph>,
    config: &FlowConfig,
) -> FlowTree {
    let canonical = build_canonical_tree(contract, function, cfg, project, all_cfgs, config);
    filter_for_render(canonical, config)
}

pub fn build_flow_tree_with_mutations(
    contract: &ContractDef,
    function: &FunctionDef,
    cfg: &CfgGraph,
    project: &Project,
    all_cfgs: &HashMap<(String, String), CfgGraph>,
    config: &FlowConfig,
) -> (FlowTree, Vec<FlowMutation>) {
    let canonical = build_canonical_tree(contract, function, cfg, project, all_cfgs, config);
    let mutations = harvest_mutations_from_tree(&canonical);
    let rendered = filter_for_render(canonical, config);
    (rendered, mutations)
}

fn build_canonical_tree(
    contract: &ContractDef,
    function: &FunctionDef,
    cfg: &CfgGraph,
    project: &Project,
    all_cfgs: &HashMap<(String, String), CfgGraph>,
    config: &FlowConfig,
) -> FlowTree {
    let signature = build_signature(function);

    let mut counter: usize = 0;
    let mut root = FlowNode {
        step_id: next_id(&mut counter),
        depth: 0,
        kind: FlowKind::Entry { signature: signature.clone() },
        from_modifier: None,
        children: Vec::new(),
    };

    let mut visited_calls: HashSet<(String, String)> = HashSet::new();
    let own_key = if function.key.is_empty() { &function.name } else { &function.key };
    visited_calls.insert((contract.key().to_string(), own_key.clone()));

    walk_cfg(
        cfg,
        contract,
        project,
        all_cfgs,
        config,
        0,
        &mut counter,
        &mut visited_calls,
        &mut root.children,
    );

    FlowTree {
        contract: contract.key().to_string(),
        function: function.name.clone(),
        signature,
        modifiers: function.modifiers.iter().map(|m| m.name.clone()).collect(),
        max_depth: config.max_depth,
        root,
        path_stats: None,
    }
}

fn filter_for_render(mut tree: FlowTree, config: &FlowConfig) -> FlowTree {
    filter_node_children(&mut tree.root, config);
    tree
}

fn filter_node_children(node: &mut FlowNode, config: &FlowConfig) {
    if let FlowKind::InternalCall {
        ref mut depth_limited,
        ref mut ops_count,
        ..
    } = node.kind
    {
        let children_depth = node.depth + 1;
        let should_collapse = children_depth > config.max_depth
            && !config.expand_set.contains(&node.step_id);
        if should_collapse && !node.children.is_empty() {
            *ops_count = count_subtree_nodes(&node.children);
            *depth_limited = true;
            node.children.clear();
            return;
        }
    }

    for child in &mut node.children {
        filter_node_children(child, config);
    }
}

fn count_subtree_nodes(children: &[FlowNode]) -> usize {
    let mut total = 0;
    for child in children {
        total += 1;
        total += count_subtree_nodes(&child.children);
    }
    total
}

fn harvest_mutations_from_tree(tree: &FlowTree) -> Vec<FlowMutation> {
    let mut out = Vec::new();
    let mut seen: HashSet<(String, AssignOperator, String)> = HashSet::new();
    harvest_node(&tree.root, &[], &mut out, &mut seen);
    out
}

fn harvest_node(
    node: &FlowNode,
    call_chain: &[String],
    out: &mut Vec<FlowMutation>,
    seen: &mut HashSet<(String, AssignOperator, String)>,
) {
    match &node.kind {
        FlowKind::Write { target, value, op } => {
            let key = (target.clone(), *op, value.clone());
            if seen.insert(key) {
                out.push(FlowMutation {
                    flow_step_id: node.step_id,
                    target: target.clone(),
                    value: value.clone(),
                    operator: *op,
                    via: call_chain_via(call_chain),
                });
            }
        }
        FlowKind::StateWrite { variable } => {
            let key = (variable.clone(), AssignOperator::Assign, String::new());
            if seen.insert(key) {
                out.push(FlowMutation {
                    flow_step_id: node.step_id,
                    target: variable.clone(),
                    value: String::new(),
                    operator: AssignOperator::Assign,
                    via: call_chain_via(call_chain),
                });
            }
        }
        FlowKind::InternalCall { function, .. } => {
            let mut new_chain: Vec<String> = call_chain.to_vec();
            new_chain.push(function.clone());
            for child in &node.children {
                harvest_node(child, &new_chain, out, seen);
            }
            return;
        }
        _ => {}
    }
    for child in &node.children {
        harvest_node(child, call_chain, out, seen);
    }
}

fn call_chain_via(chain: &[String]) -> Option<String> {
    if chain.is_empty() {
        None
    } else {
        Some(chain.join(" -> "))
    }
}

pub fn collect_path_conditions(
    tree: &FlowTree,
    target_step_id: usize,
) -> Option<Vec<String>> {
    let mut path: Vec<String> = Vec::new();
    if collect_path_rec(&tree.root, target_step_id, &mut path) {
        Some(path)
    } else {
        None
    }
}

fn collect_path_rec(node: &FlowNode, target: usize, path: &mut Vec<String>) -> bool {
    if node.step_id == target {
        return true;
    }

    let pushed = match &node.kind {
        FlowKind::BranchTrue { condition } => {
            path.push(condition.clone());
            true
        }
        FlowKind::BranchFalse { condition } => {
            path.push(format!("!({})", condition));
            true
        }
        _ => false,
    };

    for child in &node.children {
        if collect_path_rec(child, target, path) {
            return true;
        }
    }

    if pushed {
        path.pop();
    }
    false
}

fn next_id(counter: &mut usize) -> usize {
    let id = *counter;
    *counter += 1;
    id
}

fn build_signature(function: &FunctionDef) -> String {
    let params: Vec<String> = function
        .params
        .iter()
        .map(|p| format!("{} {}", p.type_name, p.name))
        .collect();
    format!("{}({})", function.name, params.join(", "))
}

#[allow(clippy::too_many_arguments)]
fn walk_cfg(
    cfg: &CfgGraph,
    contract: &ContractDef,
    project: &Project,
    all_cfgs: &HashMap<(String, String), CfgGraph>,
    config: &FlowConfig,
    depth: usize,
    counter: &mut usize,
    visited_calls: &mut HashSet<(String, String)>,
    out: &mut Vec<FlowNode>,
) {
    let entry = match cfg
        .node_indices()
        .find(|&n| cfg[n].kind == BlockKind::Entry)
    {
        Some(n) => n,
        None => return,
    };

    let mut visited_blocks: HashSet<petgraph::stable_graph::NodeIndex> = HashSet::new();
    walk_block(
        cfg,
        entry,
        contract,
        project,
        all_cfgs,
        config,
        depth,
        counter,
        visited_calls,
        &mut visited_blocks,
        out,
    );
}

#[allow(clippy::too_many_arguments)]
fn walk_block(
    cfg: &CfgGraph,
    node: petgraph::stable_graph::NodeIndex,
    contract: &ContractDef,
    project: &Project,
    all_cfgs: &HashMap<(String, String), CfgGraph>,
    config: &FlowConfig,
    depth: usize,
    counter: &mut usize,
    visited_calls: &mut HashSet<(String, String)>,
    visited_blocks: &mut HashSet<petgraph::stable_graph::NodeIndex>,
    out: &mut Vec<FlowNode>,
) {
    if !visited_blocks.insert(node) {
        return;
    }

    let block = &cfg[node];

    match block.kind {
        BlockKind::Return => {
            out.push(FlowNode {
                step_id: next_id(counter),
                depth,
                kind: FlowKind::Return,
                from_modifier: None,
                children: Vec::new(),
            });
            return;
        }
        BlockKind::Revert => {
            if config.include_reverts {
                out.push(FlowNode {
                    step_id: next_id(counter),
                    depth,
                    kind: FlowKind::Revert { reason: None },
                    from_modifier: None,
                    children: Vec::new(),
                });
            }
            return;
        }
        _ => {}
    }

    for stmt in &block.statements {
        if let Some(node) = stmt_to_flow_node(
            stmt,
            depth,
            counter,
            contract,
            project,
            all_cfgs,
            config,
            visited_calls,
        ) {
            out.push(node);
        }
    }

    let mut edges: Vec<(petgraph::stable_graph::NodeIndex, BranchEdge)> = cfg
        .edges_directed(node, Direction::Outgoing)
        .map(|e| (e.target(), e.weight().clone()))
        .collect();

    // stable edge order so step_ids stay consistent across runs
    edges.sort_by_key(|(target, edge)| (edge.variant_order(), target.index()));

    if edges.is_empty() {
        return;
    }

    if edges.len() == 1 {
        walk_block(
            cfg,
            edges[0].0,
            contract,
            project,
            all_cfgs,
            config,
            depth,
            counter,
            visited_calls,
            visited_blocks,
            out,
        );
        return;
    }

    let mut useful: Vec<(petgraph::stable_graph::NodeIndex, BranchEdge)> = Vec::new();
    for (target, edge) in edges {
        if cfg[target].kind == BlockKind::Revert && !config.include_reverts {
            continue;
        }
        useful.push((target, edge));
    }

    if useful.is_empty() {
        return;
    }

    if useful.len() == 1 {
        let (target, _edge) = useful.into_iter().next().unwrap();
        walk_block(
            cfg,
            target,
            contract,
            project,
            all_cfgs,
            config,
            depth,
            counter,
            visited_calls,
            visited_blocks,
            out,
        );
        return;
    }

    for (target, edge) in useful {
        let mut arm_visited = visited_blocks.clone();
        let mut children = Vec::new();
        walk_block(
            cfg,
            target,
            contract,
            project,
            all_cfgs,
            config,
            depth + 1,
            counter,
            visited_calls,
            &mut arm_visited,
            &mut children,
        );

        if children.is_empty() {
            continue;
        }

        let kind = match edge {
            BranchEdge::ConditionalTrue { condition } => FlowKind::BranchTrue { condition },
            BranchEdge::ConditionalFalse { condition } => FlowKind::BranchFalse { condition },
            BranchEdge::ExternalCallSuccess => FlowKind::BranchTrue {
                condition: "<call success>".to_string(),
            },
            BranchEdge::ExternalCallFailure => FlowKind::BranchFalse {
                condition: "<call failure>".to_string(),
            },
            BranchEdge::CatchClause { kind: catch_kind } => FlowKind::BranchTrue {
                condition: format!("catch {}", catch_kind),
            },
            BranchEdge::LoopBack => FlowKind::LoopHeader { kind: "back".to_string() },
            BranchEdge::LoopExit => FlowKind::LoopHeader { kind: "exit".to_string() },
            BranchEdge::Unconditional => {
                out.extend(children);
                continue;
            }
        };
        out.push(FlowNode {
            step_id: next_id(counter),
            depth,
            kind,
            from_modifier: None,
            children,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn stmt_to_flow_node(
    stmt: &CfgStatement,
    depth: usize,
    counter: &mut usize,
    contract: &ContractDef,
    project: &Project,
    all_cfgs: &HashMap<(String, String), CfgGraph>,
    config: &FlowConfig,
    visited_calls: &mut HashSet<(String, String)>,
) -> Option<FlowNode> {
    match stmt {
        CfgStatement::Assignment { target, value, operator, from_modifier, .. } => {
            Some(FlowNode {
                step_id: next_id(counter),
                depth,
                kind: FlowKind::Write {
                    target: target.clone(),
                    value: value.clone(),
                    op: *operator,
                },
                from_modifier: from_modifier.clone(),
                children: Vec::new(),
            })
        }
        CfgStatement::RequireCheck { condition, message, from_modifier, .. } => {
            Some(FlowNode {
                step_id: next_id(counter),
                depth,
                kind: FlowKind::Require {
                    condition: condition.clone(),
                    message: message.clone(),
                },
                from_modifier: from_modifier.clone(),
                children: Vec::new(),
            })
        }
        CfgStatement::AssertCheck { condition, from_modifier, .. } => {
            Some(FlowNode {
                step_id: next_id(counter),
                depth,
                kind: FlowKind::Assert { condition: condition.clone() },
                from_modifier: from_modifier.clone(),
                children: Vec::new(),
            })
        }
        CfgStatement::InternalCall { function, from_modifier, resolved, .. } => {
            Some(build_internal_call_node(
                function,
                *resolved,
                from_modifier.clone(),
                depth,
                counter,
                contract,
                project,
                all_cfgs,
                config,
                visited_calls,
            ))
        }
        CfgStatement::ExternalCall { target, function, from_modifier, .. } => {
            Some(FlowNode {
                step_id: next_id(counter),
                depth,
                kind: FlowKind::ExternalCall {
                    target: target.clone(),
                    function: function.clone(),
                },
                from_modifier: from_modifier.clone(),
                children: Vec::new(),
            })
        }
        CfgStatement::EmitEvent { event, from_modifier, .. } => {
            Some(FlowNode {
                step_id: next_id(counter),
                depth,
                kind: FlowKind::EmitEvent { name: event.clone() },
                from_modifier: from_modifier.clone(),
                children: Vec::new(),
            })
        }
        CfgStatement::StateWrite { variable, from_modifier, .. } => {
            Some(FlowNode {
                step_id: next_id(counter),
                depth,
                kind: FlowKind::StateWrite { variable: variable.clone() },
                from_modifier: from_modifier.clone(),
                children: Vec::new(),
            })
        }
        CfgStatement::StateRead { variable, from_modifier, .. } => {
            Some(FlowNode {
                step_id: next_id(counter),
                depth,
                kind: FlowKind::StateRead { variable: variable.clone() },
                from_modifier: from_modifier.clone(),
                children: Vec::new(),
            })
        }
        CfgStatement::EthTransfer { to, from_modifier, .. } => {
            Some(FlowNode {
                step_id: next_id(counter),
                depth,
                kind: FlowKind::EthTransfer { to: to.clone() },
                from_modifier: from_modifier.clone(),
                children: Vec::new(),
            })
        }
        CfgStatement::AssemblyBlock { from_modifier, .. } => {
            Some(FlowNode {
                step_id: next_id(counter),
                depth,
                kind: FlowKind::AssemblyBlock,
                from_modifier: from_modifier.clone(),
                children: Vec::new(),
            })
        }
        // shown as a call that cannot be expanded: the modifier's code is unknown
        CfgStatement::UnresolvedModifier { name, from_modifier, .. } => Some(FlowNode {
            step_id: next_id(counter),
            depth,
            kind: FlowKind::InternalCall {
                function: format!("modifier {name}"),
                origin: "unresolved".into(),
                depth_limited: true,
                ops_count: 0,
            },
            from_modifier: from_modifier.clone(),
            children: Vec::new(),
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_internal_call_node(
    callee_name: &str,
    resolved_decl: Option<DeclId>,
    from_modifier: Option<String>,
    depth: usize,
    counter: &mut usize,
    contract: &ContractDef,
    project: &Project,
    all_cfgs: &HashMap<(String, String), CfgGraph>,
    config: &FlowConfig,
    visited_calls: &mut HashSet<(String, String)>,
) -> FlowNode {
    // the declaration solc resolved (super, library, base) wins over a lookup by name
    let declared = resolved_decl
        .and_then(|id| project.decl_table.function(id))
        .and_then(|t| {
            let key = (t.contract.clone(), t.function.clone());
            all_cfgs.get(&key).map(|cfg| (key, cfg))
        });
    let (callee_key, resolved) = match declared {
        Some(((owner, key), cfg)) => (key, Some((owner, cfg))),
        None => (
            callee_name.to_string(),
            resolve_callee_cfg(callee_name, contract, project, all_cfgs),
        ),
    };

    let (owning_name, callee_cfg) = match resolved {
        Some(x) => x,
        None => {
            return FlowNode {
                step_id: next_id(counter),
                depth,
                kind: FlowKind::InternalCall {
                    function: callee_name.to_string(),
                    origin: String::new(),
                    depth_limited: false,
                    ops_count: 0,
                },
                from_modifier,
                children: Vec::new(),
            };
        }
    };

    let call_key = (owning_name.clone(), callee_key);
    let already_visited = visited_calls.contains(&call_key);
    let depth_exhausted = depth + 1 > CANONICAL_WALK_SAFETY_CAP;

    if already_visited || depth_exhausted {
        let ops_count = count_statements(callee_cfg);
        return FlowNode {
            step_id: next_id(counter),
            depth,
            kind: FlowKind::InternalCall {
                function: callee_name.to_string(),
                origin: owning_name,
                depth_limited: true,
                ops_count,
            },
            from_modifier,
            children: Vec::new(),
        };
    }

    visited_calls.insert(call_key.clone());

    let owning_contract = project
        .contract_index
        .get(&owning_name)
        .map(|&i| &project.contracts[i])
        .unwrap_or(contract);

    let mut children = Vec::new();
    walk_cfg(
        callee_cfg,
        owning_contract,
        project,
        all_cfgs,
        config,
        depth + 1,
        counter,
        visited_calls,
        &mut children,
    );

    visited_calls.remove(&call_key);

    let ops_count = children.len();

    FlowNode {
        step_id: next_id(counter),
        depth,
        kind: FlowKind::InternalCall {
            function: callee_name.to_string(),
            origin: owning_name,
            depth_limited: false,
            ops_count,
        },
        from_modifier,
        children,
    }
}

fn resolve_callee_cfg<'a>(
    callee: &str,
    starting_contract: &ContractDef,
    project: &Project,
    all_cfgs: &'a HashMap<(String, String), CfgGraph>,
) -> Option<(String, &'a CfgGraph)> {
    // the first contract of the linearization that defines it
    project.linearization(starting_contract).into_iter().find_map(|c| {
        let key = (c.key().to_string(), callee.to_string());
        all_cfgs.get(&key).map(|cfg| (key.0, cfg))
    })
}

fn count_statements(cfg: &CfgGraph) -> usize {
    cfg.node_indices()
        .map(|n| cfg[n].statements.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(step_id: usize, kind: FlowKind) -> FlowNode {
        FlowNode {
            step_id,
            depth: 0,
            kind,
            from_modifier: None,
            children: Vec::new(),
        }
    }

    fn parent(step_id: usize, kind: FlowKind, children: Vec<FlowNode>) -> FlowNode {
        FlowNode {
            step_id,
            depth: 0,
            kind,
            from_modifier: None,
            children,
        }
    }

    fn fixture_tree() -> FlowTree {
        let inner_write = leaf(3, FlowKind::Write {
            target: "reserve0".into(),
            value: "x".into(),
            op: AssignOperator::Assign,
        });
        let bf_inner = parent(2, FlowKind::BranchFalse { condition: "b > 0".into() }, vec![inner_write]);
        let bt_outer = parent(1, FlowKind::BranchTrue { condition: "a > 0".into() }, vec![bf_inner]);

        let other_write = leaf(5, FlowKind::Write {
            target: "balance".into(),
            value: "y".into(),
            op: AssignOperator::Assign,
        });
        let bf_outer = parent(4, FlowKind::BranchFalse { condition: "a > 0".into() }, vec![other_write]);

        let root = parent(
            0,
            FlowKind::Entry { signature: "f()".into() },
            vec![bt_outer, bf_outer],
        );

        FlowTree {
            contract: "C".into(),
            function: "f".into(),
            signature: "f()".into(),
            modifiers: Vec::new(),
            max_depth: 4,
            root,
            path_stats: None,
        }
    }

    #[test]
    fn collect_path_conditions_inside_nested_branches() {
        let tree = fixture_tree();
        let path = collect_path_conditions(&tree, 3).expect("step 3 should be reachable");
        assert_eq!(path, vec!["a > 0".to_string(), "!(b > 0)".to_string()]);
    }

    #[test]
    fn collect_path_conditions_in_other_branch() {
        let tree = fixture_tree();
        let path = collect_path_conditions(&tree, 5).expect("step 5 should be reachable");
        assert_eq!(path, vec!["!(a > 0)".to_string()]);
    }

    #[test]
    fn collect_path_conditions_root_returns_empty_path() {
        let tree = fixture_tree();
        let path = collect_path_conditions(&tree, 0).expect("root should be reachable");
        assert!(path.is_empty());
    }

    #[test]
    fn collect_path_conditions_branch_node_itself() {
        let tree = fixture_tree();
        let path = collect_path_conditions(&tree, 2).expect("step 2 should be reachable");
        assert_eq!(path, vec!["a > 0".to_string()]);
    }

    #[test]
    fn collect_path_conditions_returns_none_for_unknown_step() {
        let tree = fixture_tree();
        assert!(collect_path_conditions(&tree, 9999).is_none());
    }
}
