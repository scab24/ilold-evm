use std::collections::HashMap;

use petgraph::graph::NodeIndex;

use crate::callgraph::builder::build_call_graph;
use crate::callgraph::types::CallGraph;
use crate::model::decl_id::DeclKind;
use crate::model::project::Project;

use super::{ContractDeps, DepEdge, DepGraph, DepNode};

type Index = HashMap<String, NodeIndex>;

pub fn build(project: &Project) -> ContractDeps {
    let (mut graph, index) = nodes(project);
    static_edges(project, &mut graph, &index);
    // dependency contracts contribute inheritance and holds edges, not call edges
    for c in project.project_contracts() {
        if let Some(&from) = index.get(c.key()) {
            let cg = build_call_graph(project, c);
            call_edges(&mut graph, &index, from, c.key(), &cg);
        }
    }
    ContractDeps { graph, index }
}

pub fn from_call_graphs(project: &Project, call_graphs: &HashMap<String, CallGraph>) -> ContractDeps {
    let (mut graph, index) = nodes(project);
    static_edges(project, &mut graph, &index);
    for c in &project.contracts {
        if let (Some(&from), Some(cg)) = (index.get(c.key()), call_graphs.get(c.key())) {
            call_edges(&mut graph, &index, from, c.key(), cg);
        }
    }
    ContractDeps { graph, index }
}

fn nodes(project: &Project) -> (DepGraph, Index) {
    let mut graph = DepGraph::new();
    let mut index: Index = HashMap::new();
    for c in &project.contracts {
        if index.contains_key(c.key()) {
            continue;
        }
        let idx = graph.add_node(DepNode {
            name: c.key().to_string(),
            kind: c.kind,
            folder: project.contract_folder(c),
            qualified_id: c.qualified_id.clone(),
            is_dependency: c.is_dependency,
            is_free_functions: c.is_free_functions(),
        });
        index.insert(c.key().to_string(), idx);
    }
    (graph, index)
}

fn static_edges(project: &Project, graph: &mut DepGraph, index: &Index) {
    for c in &project.contracts {
        let Some(&from) = index.get(c.key()) else {
            continue;
        };
        for parent in project.base_keys(c) {
            if let Some(&to) = index.get(parent.as_str()) {
                if to != from {
                    edge_mut(graph, from, to).inherits = true;
                }
            }
        }
        for sv in &c.state_vars {
            for type_id in &sv.resolved_types {
                let target = project.decl_table.lookup(*type_id).filter(|t| t.kind == DeclKind::Contract);
                if let Some(target) = target {
                    if let Some(&to) = index.get(&target.contract) {
                        if to != from {
                            edge_mut(graph, from, to).holds = true;
                        }
                    }
                }
            }
        }
    }
}

fn call_edges(graph: &mut DepGraph, index: &Index, from: NodeIndex, name: &str, cg: &CallGraph) {
    for eidx in cg.edge_indices() {
        let (s, t) = cg.edge_endpoints(eidx).unwrap();
        // inherited bodies count for their own contract; calls into the own hierarchy are inheritance
        if cg[s].contract != name || !cg[t].is_external {
            continue;
        }
        let Some(&to) = index.get(&cg[t].contract) else {
            continue;
        };
        if to != from {
            let e = edge_mut(graph, from, to);
            e.calls = true;
            e.call_count += cg[eidx].call_count;
        }
    }
}

fn edge_mut(graph: &mut DepGraph, from: NodeIndex, to: NodeIndex) -> &mut DepEdge {
    let e = match graph.find_edge(from, to) {
        Some(e) => e,
        None => graph.add_edge(from, to, DepEdge::default()),
    };
    &mut graph[e]
}
