//! Shortest chains of calls from one contract to another, function to function.
use std::collections::HashMap;

use petgraph::algo::{dijkstra, has_path_connecting};
use petgraph::graphmap::DiGraphMap;
use petgraph::visit::{EdgeFiltered, Reversed};
use serde::{Deserialize, Serialize};

use crate::callgraph::types::CallGraph;

use super::{ContractCall, LinkKind, Relations};

/// Beyond this many equally short routes the rest are left out.
const MAX_ROUTES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteCall {
    pub call: ContractCall,
    /// `<contract key>::<member key>` the call runs in the step's target contract.
    pub callee: String,
    /// The first step, or the code a call of the previous step runs reaches this call inside the contract.
    pub recorded: bool,
}

/// Every call from `from` that `to` may answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteStep {
    pub from: String,
    pub to: String,
    pub calls: Vec<RouteCall>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub steps: Vec<RouteStep>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Routes {
    /// Sorted by the contracts they pass.
    pub routes: Vec<Route>,
    /// More than `MAX_ROUTES` were equally short.
    pub truncated: bool,
}

/// The shortest routes from `from` to `to` following calls, and reads when asked; library and self calls stay out.
pub fn routes(relations: &Relations, call_graphs: &HashMap<String, CallGraph>, from: &str, to: &str, include_reads: bool) -> Routes {
    let follows = |c: &ContractCall| c.kind == LinkKind::Call || (include_reads && c.kind == LinkKind::Read);
    let mut graph: DiGraphMap<&str, ()> = DiGraphMap::new();
    for call in relations.calls.iter().filter(|c| follows(c)) {
        for callee in call.callees.iter().filter(|d| d.contract != call.context) {
            graph.add_edge(call.context.as_str(), callee.contract.as_str(), ());
        }
    }
    if from == to || !graph.contains_node(from) || !graph.contains_node(to) {
        return Routes::default();
    }
    let ahead = dijkstra(&graph, from, Some(to), |_| 1usize);
    let Some(&length) = ahead.get(to) else { return Routes::default() };
    let behind = dijkstra(Reversed(&graph), to, Some(from), |_| 1usize);
    let mut paths = Vec::new();
    let on_route = |n: &str, depth: usize| ahead.get(n) == Some(&depth) && behind.get(n) == Some(&(length - depth));
    let complete = shortest(&graph, &on_route, length, &mut vec![from], &mut paths);
    let routes = paths.iter().map(|contracts| route(relations, call_graphs, contracts, &follows)).collect();
    Routes { routes, truncated: !complete }
}

/// Each path through contracts `on_route` at their depth, in key order; false once `MAX_ROUTES` stops it.
fn shortest<'a>(
    graph: &DiGraphMap<&'a str, ()>,
    on_route: &dyn Fn(&str, usize) -> bool,
    length: usize,
    path: &mut Vec<&'a str>,
    paths: &mut Vec<Vec<&'a str>>,
) -> bool {
    let depth = path.len() - 1;
    if depth == length {
        if paths.len() == MAX_ROUTES {
            return false;
        }
        paths.push(path.clone());
        return true;
    }
    let mut next: Vec<&str> = graph
        .neighbors(path[depth])
        .filter(|n| on_route(n, depth + 1))
        .collect();
    next.sort();
    for n in next {
        path.push(n);
        let complete = shortest(graph, on_route, length, path, paths);
        path.pop();
        if !complete {
            return false;
        }
    }
    true
}

fn route(relations: &Relations, call_graphs: &HashMap<String, CallGraph>, contracts: &[&str], follows: &dyn Fn(&ContractCall) -> bool) -> Route {
    let mut steps: Vec<RouteStep> = Vec::new();
    for pair in contracts.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let previous: Vec<&str> = steps.last().map(|s| s.calls.iter().map(|c| c.callee.as_str()).collect()).unwrap_or_default();
        let calls = relations
            .calls_from(from)
            .iter()
            .filter(|c| follows(c))
            .filter_map(|c| {
                let callee = c.callees.iter().find(|d| d.contract == to)?.function.clone();
                let recorded = steps.is_empty() || previous.iter().any(|entry| reaches(call_graphs.get(from), entry, &c.function));
                Some(RouteCall { call: c.clone(), callee, recorded })
            })
            .collect();
        steps.push(RouteStep { from: from.to_string(), to: to.to_string(), calls });
    }
    Route { steps }
}

/// `entry` runs `function` itself or calls it through internal calls of the contract's call graph.
fn reaches(graph: Option<&CallGraph>, entry: &str, function: &str) -> bool {
    if entry == function {
        return true;
    }
    let Some(graph) = graph else { return false };
    let node = |id: &str| graph.node_indices().find(|&n| graph[n].id() == id);
    let (Some(start), Some(end)) = (node(entry), node(function)) else { return false };
    let internal = EdgeFiltered::from_fn(graph, |e| e.weight().kind.is_internal());
    has_path_connecting(&internal, start, end, None)
}
