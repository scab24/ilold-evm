mod build;
mod order;

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use serde::{Deserialize, Serialize};

use crate::model::contract::ContractKind;
use crate::model::project::Project;

pub use order::{layers, Layer, LayerGroup};

pub type DepGraph = DiGraph<DepNode, DepEdge>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepNode {
    pub name: String,
    pub kind: ContractKind,
    pub folder: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DepEdge {
    pub inherits: bool,
    pub calls: bool,
    pub holds: bool,
    pub call_count: usize,
}

impl DepEdge {
    pub fn kinds(&self) -> Vec<DepKind> {
        let mut k = Vec::new();
        if self.inherits {
            k.push(DepKind::Inherits);
        }
        if self.calls {
            k.push(DepKind::Calls);
        }
        if self.holds {
            k.push(DepKind::Holds);
        }
        k
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DepKind {
    Inherits,
    Calls,
    Holds,
}

impl DepKind {
    pub fn label(self) -> &'static str {
        match self {
            DepKind::Inherits => "inherits",
            DepKind::Calls => "calls",
            DepKind::Holds => "holds",
        }
    }
}

pub struct DepRef<'a> {
    pub node: &'a DepNode,
    pub edge: &'a DepEdge,
}

pub struct ContractDeps {
    pub graph: DepGraph,
    index: HashMap<String, NodeIndex>,
}

impl ContractDeps {
    pub fn build(project: &Project) -> Self {
        build::build(project)
    }

    pub fn from_call_graphs(
        project: &Project,
        call_graphs: &std::collections::HashMap<String, crate::callgraph::types::CallGraph>,
    ) -> Self {
        build::from_call_graphs(project, call_graphs)
    }

    /// Restores a persisted graph. Built graphs hold one node per name; anything else is rejected.
    pub fn from_graph(graph: DepGraph) -> Option<Self> {
        let mut index = HashMap::new();
        for idx in graph.node_indices() {
            if index.insert(graph[idx].name.clone(), idx).is_some() {
                return None;
            }
        }
        Some(ContractDeps { graph, index })
    }

    pub fn node(&self, name: &str) -> Option<NodeIndex> {
        self.index.get(name).copied()
    }

    pub fn dependencies(&self, name: &str) -> Vec<DepRef<'_>> {
        self.neighbors(name, Direction::Outgoing)
    }

    pub fn dependents(&self, name: &str) -> Vec<DepRef<'_>> {
        self.neighbors(name, Direction::Incoming)
    }

    pub fn layers(&self) -> Vec<Layer> {
        order::layers(&self.graph)
    }

    fn neighbors(&self, name: &str, dir: Direction) -> Vec<DepRef<'_>> {
        let Some(&start) = self.index.get(name) else {
            return Vec::new();
        };
        let mut out: Vec<DepRef<'_>> = self
            .graph
            .edges_directed(start, dir)
            .map(|e| {
                let other = if e.source() == start { e.target() } else { e.source() };
                DepRef {
                    node: &self.graph[other],
                    edge: e.weight(),
                }
            })
            .collect();
        out.sort_by(|a, b| a.node.name.cmp(&b.node.name));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::common::SourceSpan;
    use crate::model::contract::ContractDef;
    use crate::model::decl_id::DeclTable;
    use crate::model::project::Project;

    fn span() -> SourceSpan {
        SourceSpan {
            file_index: 0,
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 0,
        }
    }

    fn contract(name: &str, inherits: &[&str]) -> ContractDef {
        ContractDef {
            name: name.into(),
            kind: ContractKind::Contract,
            functions: vec![],
            modifiers: vec![],
            state_vars: vec![],
            structs: vec![],
            enums: vec![],
            events: vec![],
            errors: vec![],
            inherits: inherits.iter().map(|s| s.to_string()).collect(),
            span: span(),
        }
    }

    fn project(contracts: Vec<ContractDef>) -> Project {
        let mut p = Project {
            source_files: vec![],
            contracts,
            contract_index: Default::default(),
            decl_table: DeclTable::default(),
        };
        p.rebuild_index();
        p
    }

    #[test]
    fn inherits_and_blast_radius() {
        let p = project(vec![
            contract("IPool", &[]),
            contract("BasePool", &["IPool"]),
            contract("Vault", &["BasePool"]),
        ]);
        let deps = ContractDeps::build(&p);

        assert!(deps
            .dependencies("Vault")
            .iter()
            .any(|d| d.node.name == "BasePool" && d.edge.inherits));

        let used: Vec<String> = deps
            .dependents("IPool")
            .iter()
            .map(|d| d.node.name.clone())
            .collect();
        assert_eq!(used, vec!["BasePool"]);
    }

    #[test]
    fn duplicate_names_collapse_to_one_node() {
        let p = project(vec![
            contract("Lib", &[]),
            contract("Lib", &[]),
            contract("User", &["Lib"]),
        ]);
        let deps = ContractDeps::build(&p);

        let lib_nodes = deps
            .graph
            .node_indices()
            .filter(|&i| deps.graph[i].name == "Lib")
            .count();
        assert_eq!(lib_nodes, 1, "same-named contracts must collapse to one node");
        assert!(deps
            .dependencies("User")
            .iter()
            .any(|d| d.node.name == "Lib" && d.edge.inherits));
    }
}
