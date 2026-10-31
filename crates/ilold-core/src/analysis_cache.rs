//! Persisted derived analysis, stored beside the parsed-project snapshot in the same private
//! directory and envelope format. A snapshot is valid only for the exact validated project key,
//! analysis parameters and analysis format version it was written with.
use std::collections::HashMap;
use std::hash::Hash;
use std::path::{Path, PathBuf};

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::stable_graph::StableDiGraph;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::analysis::{AnalysisParams, ProjectAnalysis};
use crate::callgraph::types::{CallEdge, CallNode};
use crate::cfg::types::{BasicBlock, BranchEdge};
use crate::classify::entry_points::AccessLevel;
use crate::depgraph::{ContractDeps, DepEdge, DepNode};
use crate::parse::cache::{read_envelope, write_envelope, Store};
use crate::parse::ProjectKey;
use crate::pathtree::types::PathTree;
use crate::sequence::analysis::SequenceAnalysis;
use crate::sequence::types::SequenceTree;

/// Bump whenever an analysis algorithm or any persisted type changes shape or meaning.
const VERSION: &str = concat!("ilold-analysis-v9-", env!("CARGO_PKG_VERSION"));
/// Bounds disk use and load memory per snapshot.
const MAX_SNAPSHOT: u64 = 256 * 1024 * 1024;
/// Snapshot files per project. Parameter sets map to a slot and keep only their latest result.
const SLOTS: u8 = 4;

fn digest(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }

fn parameters(params: &AnalysisParams) -> [usize; 4] {
    let pruning = &params.pruning;
    [params.max_seq_depth, pruning.max_depth, pruning.max_loop_unroll, pruning.max_paths]
}

fn analysis_key(project_key: &str, params: &AnalysisParams) -> String {
    let material = format!("{VERSION}\0{project_key}\0{:?}", parameters(params));
    digest(material.as_bytes())
}

fn snapshot_path(dir: &Path, params: &AnalysisParams) -> PathBuf {
    let slot = Sha256::digest(format!("{:?}", parameters(params)).as_bytes())[0] % SLOTS;
    dir.join(format!("analysis-{slot}.json"))
}

/// Graphs are persisted as nodes and edges in index order. Builders never remove nodes or
/// edges, so re-adding them in order restores identical indices and adjacency order.
#[derive(Serialize, Deserialize)]
struct Graph<N, E> {
    nodes: Vec<N>,
    edges: Vec<(usize, usize, E)>,
}

fn stable_ref<N, E>(graph: &StableDiGraph<N, E>) -> Result<Graph<&N, &E>, String> {
    let nodes = graph.node_indices().enumerate()
        .map(|(i, n)| (n.index() == i).then(|| &graph[n]))
        .collect::<Option<Vec<_>>>().ok_or("sparse graph")?;
    let edges = graph.edge_indices().enumerate()
        .map(|(i, e)| {
            let (from, to) = graph.edge_endpoints(e)?;
            (e.index() == i).then(|| (from.index(), to.index(), &graph[e]))
        })
        .collect::<Option<Vec<_>>>().ok_or("sparse graph")?;
    Ok(Graph { nodes, edges })
}

fn stable_graph<N, E>(dto: Graph<N, E>) -> Option<StableDiGraph<N, E>> {
    let mut graph = StableDiGraph::with_capacity(dto.nodes.len(), dto.edges.len());
    let count = dto.nodes.len();
    for node in dto.nodes { graph.add_node(node); }
    for (from, to, edge) in dto.edges {
        if from >= count || to >= count { return None; }
        graph.add_edge(NodeIndex::new(from), NodeIndex::new(to), edge);
    }
    Some(graph)
}

fn dense_ref<N, E>(graph: &DiGraph<N, E>) -> Graph<&N, &E> {
    let nodes = graph.node_indices().map(|n| &graph[n]).collect();
    let edges = graph.edge_indices()
        .filter_map(|e| graph.edge_endpoints(e).map(|(from, to)| (from.index(), to.index(), &graph[e])))
        .collect();
    Graph { nodes, edges }
}

fn dense_graph<N, E>(dto: Graph<N, E>) -> Option<DiGraph<N, E>> {
    let mut graph = DiGraph::with_capacity(dto.nodes.len(), dto.edges.len());
    let count = dto.nodes.len();
    for node in dto.nodes { graph.add_node(node); }
    for (from, to, edge) in dto.edges {
        if from >= count || to >= count { return None; }
        graph.add_edge(NodeIndex::new(from), NodeIndex::new(to), edge);
    }
    Some(graph)
}

type FunctionKey = (String, String);

#[derive(Serialize)]
struct SnapshotRef<'a> {
    cfgs: Vec<(&'a FunctionKey, Graph<&'a BasicBlock, &'a BranchEdge>)>,
    path_trees: Vec<(&'a FunctionKey, &'a PathTree)>,
    call_graphs: Vec<(&'a String, Graph<&'a CallNode, &'a CallEdge>)>,
    dep_graph: Graph<&'a DepNode, &'a DepEdge>,
    sequence_trees: Vec<(&'a String, &'a SequenceTree)>,
    sequence_analyses: Vec<(&'a String, &'a SequenceAnalysis)>,
    classifications: Vec<(&'a String, &'a Vec<(String, AccessLevel)>)>,
}

#[derive(Deserialize)]
struct Snapshot {
    cfgs: Vec<(FunctionKey, Graph<BasicBlock, BranchEdge>)>,
    path_trees: Vec<(FunctionKey, PathTree)>,
    call_graphs: Vec<(String, Graph<CallNode, CallEdge>)>,
    dep_graph: Graph<DepNode, DepEdge>,
    sequence_trees: Vec<(String, SequenceTree)>,
    sequence_analyses: Vec<(String, SequenceAnalysis)>,
    classifications: Vec<(String, Vec<(String, AccessLevel)>)>,
}

fn snapshot_ref(analysis: &ProjectAnalysis) -> Result<SnapshotRef<'_>, String> {
    Ok(SnapshotRef {
        cfgs: analysis.cfgs.iter().map(|(k, g)| Ok((k, stable_ref(g)?)))
            .collect::<Result<_, String>>()?,
        path_trees: analysis.path_trees.iter().collect(),
        call_graphs: analysis.call_graphs.iter().map(|(k, g)| Ok((k, stable_ref(g)?)))
            .collect::<Result<_, String>>()?,
        dep_graph: dense_ref(&analysis.dep_graph.graph),
        sequence_trees: analysis.sequence_trees.iter().collect(),
        sequence_analyses: analysis.sequence_analyses.iter().collect(),
        classifications: analysis.classifications.iter().collect(),
    })
}

/// Rejects duplicate keys, which a snapshot written from a map can never contain.
fn unique<K: Eq + Hash, V>(entries: Vec<(K, V)>) -> Option<HashMap<K, V>> {
    let len = entries.len();
    let map: HashMap<K, V> = entries.into_iter().collect();
    (map.len() == len).then_some(map)
}

impl Snapshot {
    fn into_analysis(self) -> Option<ProjectAnalysis> {
        let cfgs = self.cfgs.into_iter().map(|(k, g)| Some((k, stable_graph(g)?)))
            .collect::<Option<Vec<_>>>()?;
        let call_graphs = self.call_graphs.into_iter().map(|(k, g)| Some((k, stable_graph(g)?)))
            .collect::<Option<Vec<_>>>()?;
        Some(ProjectAnalysis {
            cfgs: unique(cfgs)?,
            path_trees: unique(self.path_trees)?,
            call_graphs: unique(call_graphs)?,
            dep_graph: ContractDeps::from_graph(dense_graph(self.dep_graph)?)?,
            sequence_trees: unique(self.sequence_trees)?,
            sequence_analyses: unique(self.sequence_analyses)?,
            classifications: unique(self.classifications)?,
        })
    }
}

fn load(path: &Path, key: &str) -> Result<ProjectAnalysis, String> {
    let bytes = read_envelope(path, VERSION, key, MAX_SNAPSHOT)?;
    serde_json::from_slice::<Snapshot>(&bytes).map_err(|e| e.to_string())?
        .into_analysis().ok_or_else(|| "invalid graph or duplicate entry".into())
}

fn publish(path: &Path, key: &str, analysis: &ProjectAnalysis) -> Result<(), String> {
    write_envelope(path, VERSION, key, MAX_SNAPSHOT, &snapshot_ref(analysis)?)
}

pub(crate) fn load_or_build_from_env(
    key: Option<&ProjectKey>, params: &AnalysisParams, build: impl FnOnce() -> ProjectAnalysis,
) -> ProjectAnalysis {
    let store = Store::from_env().ok();
    load_or_build_in(store.as_ref(), key, params, build)
}

/// Injection seam: `store_base` replaces the environment-selected store (`None` bypasses),
/// and `build` is called only when no valid snapshot exists.
pub fn load_or_build_with(
    store_base: Option<&Path>, key: Option<&ProjectKey>, params: &AnalysisParams,
    build: impl FnOnce() -> ProjectAnalysis,
) -> ProjectAnalysis {
    let store = store_base.map(Store::at);
    load_or_build_in(store.as_ref(), key, params, build)
}

fn load_or_build_in(
    store: Option<&Store>, key: Option<&ProjectKey>, params: &AnalysisParams,
    build: impl FnOnce() -> ProjectAnalysis,
) -> ProjectAnalysis {
    let (Some(store), Some(project)) = (store, key) else {
        eprintln!("ilold analysis cache bypass: no validated project key or store");
        return build();
    };
    let path = match store.cache_dir(&project.root) {
        Ok(dir) => snapshot_path(&dir, params),
        Err(reason) => {
            eprintln!("ilold analysis cache bypass: {reason}");
            return build();
        }
    };
    let key = analysis_key(&project.key, params);
    match load(&path, &key) {
        Ok(analysis) => {
            eprintln!("ilold analysis cache hit");
            return analysis;
        }
        Err(reason) => eprintln!("ilold analysis cache miss: {reason}"),
    }
    let analysis = build();
    if let Err(reason) = publish(&path, &key, &analysis) {
        eprintln!("ilold analysis cache publish skipped: {reason}");
    }
    analysis
}
