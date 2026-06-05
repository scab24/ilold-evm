use std::collections::{BTreeSet, HashMap};

use petgraph::algo::tarjan_scc;
use petgraph::graph::NodeIndex;

use super::DepGraph;

#[derive(Debug, Clone)]
pub struct Layer {
    pub index: usize,
    pub groups: Vec<LayerGroup>,
}

#[derive(Debug, Clone)]
pub struct LayerGroup {
    pub members: Vec<NodeIndex>,
}

impl LayerGroup {
    pub fn is_cycle(&self) -> bool {
        self.members.len() > 1
    }
}

pub fn layers(graph: &DepGraph) -> Vec<Layer> {
    let sccs = tarjan_scc(graph);
    if sccs.is_empty() {
        return Vec::new();
    }

    let mut scc_of: HashMap<NodeIndex, usize> = HashMap::new();
    for (sid, comp) in sccs.iter().enumerate() {
        for &n in comp {
            scc_of.insert(n, sid);
        }
    }

    let mut succ: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); sccs.len()];
    for e in graph.edge_indices() {
        let (s, t) = graph.edge_endpoints(e).unwrap();
        let (a, b) = (scc_of[&s], scc_of[&t]);
        if a != b {
            succ[a].insert(b);
        }
    }

    let mut layer_of: Vec<Option<usize>> = vec![None; sccs.len()];
    for sid in 0..sccs.len() {
        compute_layer(sid, &succ, &mut layer_of);
    }

    let max = layer_of.iter().filter_map(|x| *x).max().unwrap_or(0);
    let mut layers: Vec<Layer> = (0..=max)
        .map(|index| Layer {
            index,
            groups: Vec::new(),
        })
        .collect();
    for (sid, comp) in sccs.iter().enumerate() {
        let l = layer_of[sid].unwrap_or(0);
        layers[l].groups.push(LayerGroup {
            members: comp.clone(),
        });
    }
    layers
}

fn compute_layer(sid: usize, succ: &[BTreeSet<usize>], memo: &mut [Option<usize>]) -> usize {
    if let Some(l) = memo[sid] {
        return l;
    }
    memo[sid] = Some(0); // guard against re-entry; condensation is a DAG
    let mut max = 0;
    for &t in &succ[sid] {
        max = max.max(1 + compute_layer(t, succ, memo));
    }
    memo[sid] = Some(max);
    max
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::depgraph::{DepEdge, DepGraph, DepNode};
    use crate::model::contract::ContractKind;

    fn node(g: &mut DepGraph, name: &str) -> NodeIndex {
        g.add_node(DepNode {
            name: name.into(),
            kind: ContractKind::Contract,
            folder: String::new(),
        })
    }

    fn name_at(g: &DepGraph, layers: &[Layer], layer: usize) -> Vec<String> {
        let mut names: Vec<String> = layers[layer]
            .groups
            .iter()
            .flat_map(|gr| gr.members.iter().map(|&m| g[m].name.clone()))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn linear_chain_orders_base_first() {
        let mut g = DepGraph::new();
        let a = node(&mut g, "A");
        let b = node(&mut g, "B");
        let c = node(&mut g, "C");
        g.add_edge(a, b, DepEdge::default());
        g.add_edge(b, c, DepEdge::default());

        let layers = layers(&g);
        assert_eq!(layers.len(), 3);
        assert_eq!(name_at(&g, &layers, 0), vec!["C"]);
        assert_eq!(name_at(&g, &layers, 1), vec!["B"]);
        assert_eq!(name_at(&g, &layers, 2), vec!["A"]);
    }

    #[test]
    fn cycle_is_condensed_into_one_group() {
        let mut g = DepGraph::new();
        let a = node(&mut g, "A");
        let b = node(&mut g, "B");
        let c = node(&mut g, "C");
        g.add_edge(a, b, DepEdge::default());
        g.add_edge(b, a, DepEdge::default());
        g.add_edge(a, c, DepEdge::default());
        g.add_edge(b, c, DepEdge::default());

        let layers = layers(&g);
        assert_eq!(layers.len(), 2);
        assert_eq!(name_at(&g, &layers, 0), vec!["C"]);
        assert_eq!(name_at(&g, &layers, 1), vec!["A", "B"]);
        let cycle = &layers[1].groups[0];
        assert!(cycle.is_cycle());
    }
}
