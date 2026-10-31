use serde::{Deserialize, Serialize};

use ilold_core::pathtree::types::PathTree;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub contract: Option<String>,
    pub function: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub contract: String,
    pub function: String,
    pub path_id: usize,
    pub terminal: String,
    pub matches: Vec<SearchMatch>,
    pub depth: usize,
}

#[derive(Debug, Serialize)]
pub struct SearchMatch {
    pub field: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct SearchComplete {
    pub total: usize,
}

/// Results streamed per search; the completion message reports how many matched in all.
pub const MAX_SEARCH_RESULTS: usize = 100;

/// The first `limit` matching paths and the number of paths that match.
pub fn search_paths(state: &AppState, query: &SearchQuery, limit: usize) -> (Vec<SearchResult>, usize) {
    let q = query.query.to_lowercase();
    let mut results = Vec::new();
    let mut total = 0;

    // (contract reported, function key, tree): a contract filter covers the functions it
    // inherits, whose trees are keyed by the declaring contract
    let trees: Vec<(&str, &str, &PathTree)> = match &query.contract {
        Some(name) => match state.project.lookup_contract(name) {
            Ok(contract) => state.project.accessible_functions(contract).into_iter()
                .filter_map(|af| {
                    let tree = state.path_trees.get(&(af.owner.key().to_string(), af.function.key.clone()))?;
                    Some((contract.key(), af.function.key.as_str(), tree))
                })
                .collect(),
            Err(_) => Vec::new(),
        },
        None => state.path_trees.iter().map(|((c, f), tree)| (c.as_str(), f.as_str(), tree)).collect(),
    };

    for (contract, function, path_tree) in trees {
        if query.function.as_deref().is_some_and(|f| f != function) {
            continue;
        }

        for path in path_tree.paths() {
            let mut matches = Vec::new();

            for check in &path.annotations.require_checks {
                if check.to_lowercase().contains(&q) {
                    matches.push(SearchMatch {
                        field: "require".into(),
                        value: check.clone(),
                    });
                }
            }

            for call in &path.annotations.external_calls {
                let call_str = format!("{}.{}", call.target, call.function);
                if call_str.to_lowercase().contains(&q)
                    || "external".contains(&q)
                    || "ext call".contains(&q)
                {
                    matches.push(SearchMatch {
                        field: "external_call".into(),
                        value: call_str,
                    });
                }
            }

            for call in &path.annotations.internal_calls {
                if call.to_lowercase().contains(&q) {
                    matches.push(SearchMatch {
                        field: "internal_call".into(),
                        value: call.clone(),
                    });
                }
            }

            for write in &path.annotations.state_writes {
                if write.to_lowercase().contains(&q)
                    || "state write".contains(&q)
                    || "write".contains(&q)
                {
                    matches.push(SearchMatch {
                        field: "state_write".into(),
                        value: write.clone(),
                    });
                }
            }

            for event in &path.annotations.events_emitted {
                if event.to_lowercase().contains(&q) {
                    matches.push(SearchMatch {
                        field: "event".into(),
                        value: event.clone(),
                    });
                }
            }

            if path.annotations.has_assembly && "assembly".contains(&q) {
                matches.push(SearchMatch {
                    field: "assembly".into(),
                    value: "contains assembly block".into(),
                });
            }

            let terminal_str = format!("{:?}", path.terminal);
            if terminal_str.to_lowercase().contains(&q) {
                matches.push(SearchMatch {
                    field: "terminal".into(),
                    value: terminal_str.clone(),
                });
            }

            if function.to_lowercase().contains(&q) {
                matches.push(SearchMatch {
                    field: "function".into(),
                    value: function.to_string(),
                });
            }

            if matches.is_empty() { continue; }
            total += 1;
            if results.len() < limit {
                results.push(SearchResult {
                    contract: contract.to_string(),
                    function: function.to_string(),
                    path_id: path.id,
                    terminal: format!("{:?}", path.terminal),
                    matches,
                    depth: path.depth,
                });
            }
        }
    }

    (results, total)
}
