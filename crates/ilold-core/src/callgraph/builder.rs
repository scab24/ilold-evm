use std::collections::HashMap;

use petgraph::stable_graph::NodeIndex;

use crate::cfg::builder::CfgBuilder;
use crate::cfg::types::CfgStatement;
use crate::model::contract::ContractDef;
use crate::model::decl_id::DeclId;
use crate::model::function::{Mutability, Visibility};
use crate::model::project::Project;
use crate::util::is_type_cast;

use super::types::*;

pub fn build_call_graph(project: &Project, contract: &ContractDef) -> CallGraph {
    let mut graph = CallGraph::new();
    let mut node_index: HashMap<String, NodeIndex> = HashMap::new();

    for func in &contract.functions {
        let key = format!("{}::{}", contract.name, func.name);
        let idx = graph.add_node(CallNode {
            contract: contract.name.clone(),
            function: func.name.clone(),
            visibility: func.visibility,
            mutability: func.mutability,
            is_external: false,
        });
        node_index.insert(key, idx);
    }

    for func in &contract.functions {
        let caller_key = format!("{}::{}", contract.name, func.name);
        let caller_idx = match node_index.get(&caller_key) {
            Some(idx) => *idx,
            None => continue,
        };

        let cfg = match CfgBuilder::build(func, contract) {
            Ok(cfg) => cfg,
            Err(_) => continue,
        };

        for block in cfg.node_weights() {
            for stmt in &block.statements {
                match stmt {
                    CfgStatement::InternalCall { function, .. } => {
                        if !is_type_cast(function) {
                            let callee_idx = resolve_internal(
                                function,
                                contract,
                                project,
                                &mut graph,
                                &mut node_index,
                            );
                            add_or_increment_edge(&mut graph, caller_idx, callee_idx);
                        }
                    }
                    CfgStatement::ExternalCall { target, function, resolved, .. } => {
                        let unresolved_cast = resolved.is_none()
                            && (is_type_cast(function) || is_type_cast(target));
                        if !unresolved_cast {
                            let callee_idx = resolve_external(
                                target,
                                function,
                                *resolved,
                                project,
                                &mut graph,
                                &mut node_index,
                            );
                            add_or_increment_edge(&mut graph, caller_idx, callee_idx);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    graph
}

fn resolve_internal(
    function_name: &str,
    contract: &ContractDef,
    project: &Project,
    graph: &mut CallGraph,
    index: &mut HashMap<String, NodeIndex>,
) -> NodeIndex {
    let key = format!("{}::{}", contract.name, function_name);
    if let Some(&idx) = index.get(&key) {
        return idx;
    }

    for parent_name in &contract.inherits {
        if let Some(&parent_idx) = project.contract_index.get(parent_name) {
            let parent = &project.contracts[parent_idx];
            if let Some(func) = parent.functions.iter().find(|f| f.name == function_name) {
                let key = format!("{parent_name}::{function_name}");
                if let Some(&idx) = index.get(&key) {
                    return idx;
                }
                let idx = graph.add_node(CallNode {
                    contract: parent_name.clone(),
                    function: function_name.to_string(),
                    visibility: func.visibility,
                    mutability: func.mutability,
                    is_external: false,
                });
                index.insert(key, idx);
                return idx;
            }
        }
    }

    let key = format!("?::{function_name}");
    if let Some(&idx) = index.get(&key) {
        return idx;
    }
    let idx = graph.add_node(CallNode {
        contract: "?".to_string(),
        function: function_name.to_string(),
        visibility: Visibility::Public,
        mutability: Mutability::NonPayable,
        is_external: true,
    });
    index.insert(key, idx);
    idx
}

fn resolve_external(
    target: &str,
    function_name: &str,
    resolved: Option<DeclId>,
    project: &Project,
    graph: &mut CallGraph,
    index: &mut HashMap<String, NodeIndex>,
) -> NodeIndex {
    let (contract, function) = resolved
        .and_then(|id| project.decl_table.lookup(id))
        .map(|t| (t.contract.clone(), t.function.clone()))
        .unwrap_or_else(|| (target.to_string(), function_name.to_string()));

    let key = format!("{contract}::{function}");
    if let Some(&idx) = index.get(&key) {
        return idx;
    }

    let (visibility, mutability) = project
        .contract_index
        .get(&contract)
        .map(|&i| &project.contracts[i])
        .and_then(|c| c.functions.iter().find(|f| f.name == function))
        .map(|f| (f.visibility, f.mutability))
        .unwrap_or((Visibility::External, Mutability::NonPayable));

    let idx = graph.add_node(CallNode {
        contract,
        function,
        visibility,
        mutability,
        is_external: true,
    });
    index.insert(key, idx);
    idx
}

fn add_or_increment_edge(graph: &mut CallGraph, from: NodeIndex, to: NodeIndex) {
    if let Some(edge) = graph.find_edge(from, to) {
        graph[edge].call_count += 1;
    } else {
        let kind = if graph[to].is_external {
            CallKind::External
        } else if graph[from].contract != graph[to].contract {
            CallKind::Inherited
        } else {
            CallKind::Internal
        };
        graph.add_edge(from, to, CallEdge { kind, call_count: 1 });
    }
}
