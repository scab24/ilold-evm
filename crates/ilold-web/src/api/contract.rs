use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use ilold_core::classify::entry_points::{classify_in, AccessLevel};
use ilold_core::classify::guards::GuardFact;
use ilold_core::model::common::SourceSpan;
use ilold_core::model::contract::ContractDef;
use ilold_core::model::function::FunctionDef;
use ilold_core::model::project::{ContractLookupError, FunctionLookupError};
use ilold_core::pathtree::types::PathTreePage;
use ilold_core::sequence::types::SequencePage;
use ilold_core::util::{DEFAULT_PAGE_LIMIT, MAX_PAGE_LIMIT};

use crate::state::AppState;


#[derive(Serialize)]
pub struct ContractDetail {
    /// Route segment for `/api/contract/{name}`: the contract key (bare name when unique).
    pub name: String,
    /// Solidity name as declared.
    pub display_name: String,
    /// Stable `<source path>:<Name>` id.
    pub qualified_id: String,
    pub is_dependency: bool,
    pub kind: String,
    /// Direct bases as written in the source.
    pub inherits: Vec<String>,
    /// The same bases as route segments (contract keys).
    pub inherit_keys: Vec<String>,
    pub functions: Vec<FunctionSummary>,
    pub state_vars: Vec<StateVarSummary>,
    pub inherited_functions: Vec<FunctionSummary>,
    pub inherited_state_vars: Vec<StateVarSummary>,
}

#[derive(Serialize)]
pub struct FunctionSummary {
    pub name: String,
    /// Route segment for `/api/contract/{c}/{key}/...`: the bare name when it is unique on
    /// this contract, otherwise the signature.
    pub key: String,
    pub signature: String,
    pub kind: String,
    pub visibility: String,
    pub mutability: String,
    pub params: Vec<ParamSummary>,
    pub path_count: usize,
    pub happy_paths: usize,
    pub revert_paths: usize,
    /// The enumeration hit a cutoff or the path cap; counts cover only the paths found.
    pub paths_truncated: bool,
    pub modifiers: Vec<String>,
    pub access: AccessLevel,
    /// Access and state guards read from code, as this contract runs the function.
    pub guards: Vec<GuardFact>,
}

#[derive(Serialize)]
pub struct ParamSummary {
    pub name: String,
    pub type_name: String,
}

#[derive(Serialize)]
pub struct StateVarSummary {
    pub name: String,
    pub type_name: String,
    pub visibility: String,
    pub is_constant: bool,
    pub is_immutable: bool,
}

/// Selects the contract behind `{name}`: a key, a qualified id (`src/A.sol:A`) or a bare
/// name that is unambiguous. An ambiguous bare name is a 409 listing the qualified ids.
pub(crate) fn route_contract<'a>(state: &'a AppState, name: &str) -> Result<&'a ContractDef, Response> {
    state.project.lookup_contract(name).map_err(|e| match e {
        ContractLookupError::NotFound => StatusCode::NOT_FOUND.into_response(),
        ContractLookupError::Ambiguous(candidates) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("contract name '{name}' is ambiguous; use a qualified id"),
                "candidates": candidates,
            })),
        )
            .into_response(),
    })
}

pub async fn get_contract(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<ContractDetail>, Response> {
    let contract = route_contract(&state, &name)?;

    let accessible = state.project.accessible_functions(contract);
    let route_key = |f: &FunctionDef| -> String {
        let same_name = accessible.iter().filter(|af| af.function.name == f.name).count();
        if same_name > 1 { f.match_signature().to_string() } else { f.name.clone() }
    };
    // read from code, as the session lists do
    let guarded = |owner: &ContractDef, f: &FunctionDef| -> (AccessLevel, Vec<GuardFact>) {
        classify_in(&state.project, contract, owner, f)
    };

    let functions = contract
        .functions
        .iter()
        .map(|f| {
            let key = (contract.key().to_string(), f.key.clone());
            let pt = state.path_trees.get(&key);
            let (access, guards) = guarded(contract, f);
            FunctionSummary {
                name: f.name.clone(),
                key: route_key(f),
                signature: f.match_signature().to_string(),
                kind: format!("{:?}", f.kind),
                visibility: format!("{:?}", f.visibility),
                mutability: format!("{:?}", f.mutability),
                params: f.params.iter().map(|p| ParamSummary {
                    name: p.name.clone(),
                    type_name: p.type_name.clone(),
                }).collect(),
                path_count: pt.map(|p| p.stats.total_paths).unwrap_or(0),
                happy_paths: pt.map(|p| p.stats.happy_paths).unwrap_or(0),
                revert_paths: pt.map(|p| p.stats.revert_paths).unwrap_or(0),
                paths_truncated: pt.is_some_and(|p| p.stats.truncated),
                modifiers: f.modifiers.iter().map(|m| m.name.clone()).collect(),
                access,
                guards,
            }
        })
        .collect();

    let state_vars = contract
        .state_vars
        .iter()
        .map(|sv| StateVarSummary {
            name: sv.name.clone(),
            type_name: sv.type_name.clone(),
            visibility: format!("{:?}", sv.visibility),
            is_constant: sv.is_constant,
            is_immutable: sv.is_immutable,
        })
        .collect();

    let inherited_functions: Vec<FunctionSummary> = state.project
        .accessible_functions(contract)
        .into_iter()
        .filter(|af| af.is_inherited)
        .map(|af| {
            let f = af.function;
            let key = (af.origin.clone(), f.key.clone());
            let pt = state.path_trees.get(&key);
            let (access, guards) = guarded(af.owner, f);
            FunctionSummary {
                name: f.name.clone(),
                key: route_key(f),
                signature: f.match_signature().to_string(),
                kind: format!("{:?}", f.kind),
                visibility: format!("{:?}", f.visibility),
                mutability: format!("{:?}", f.mutability),
                params: f.params.iter().map(|p| ParamSummary {
                    name: p.name.clone(),
                    type_name: p.type_name.clone(),
                }).collect(),
                path_count: pt.map(|p| p.stats.total_paths).unwrap_or(0),
                happy_paths: pt.map(|p| p.stats.happy_paths).unwrap_or(0),
                revert_paths: pt.map(|p| p.stats.revert_paths).unwrap_or(0),
                paths_truncated: pt.is_some_and(|p| p.stats.truncated),
                modifiers: f.modifiers.iter().map(|m| m.name.clone()).collect(),
                access,
                guards,
            }
        })
        .collect();

    // storage order, most base contract first
    let inherited_state_vars: Vec<StateVarSummary> = state.project
        .accessible_state_vars(contract)
        .into_iter()
        .filter(|(_, _, inherited)| *inherited)
        .map(|(sv, _, _)| StateVarSummary {
            name: sv.name.clone(),
            type_name: sv.type_name.clone(),
            visibility: format!("{:?}", sv.visibility),
            is_constant: sv.is_constant,
            is_immutable: sv.is_immutable,
        })
        .collect();

    Ok(Json(ContractDetail {
        name: contract.key().to_string(),
        display_name: contract.name.clone(),
        qualified_id: contract.qualified_id.clone(),
        is_dependency: contract.is_dependency,
        kind: format!("{:?}", contract.kind),
        inherits: contract.inherits.clone(),
        inherit_keys: super::project::base_keys(&state, contract),
        functions,
        state_vars,
        inherited_functions,
        inherited_state_vars,
    }))
}


#[derive(Serialize)]
pub struct CytoscapeGraph {
    pub nodes: Vec<CytoscapeNode>,
    pub edges: Vec<CytoscapeEdge>,
}

#[derive(Serialize)]
pub struct CytoscapeNode {
    pub data: CytoscapeNodeData,
}

#[derive(Serialize)]
pub struct CytoscapeNodeData {
    pub id: String,
    pub label: String,
    pub node_type: String,
    pub contract: String,
    pub is_external: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub statements: Vec<String>,
    /// CFG blocks: the source lines the block runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
}

#[derive(Serialize)]
pub struct CytoscapeEdge {
    pub data: CytoscapeEdgeData,
}

#[derive(Serialize)]
pub struct CytoscapeEdgeData {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: String,
    pub call_count: usize,
    /// Human label of a call-graph edge kind (absent on CFG edges).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind_label: Option<String>,
    /// Declaration named in the source when virtual dispatch picked another implementation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub static_target: Option<String>,
}

pub async fn get_callgraph(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<CytoscapeGraph>, Response> {
    let contract = route_contract(&state, &name)?;
    let cg = state
        .call_graphs
        .get(contract.key())
        .ok_or_else(|| StatusCode::NOT_FOUND.into_response())?;

    let nodes: Vec<CytoscapeNode> = cg
        .node_indices()
        .map(|idx| {
            let node = &cg[idx];
            CytoscapeNode {
                data: CytoscapeNodeData {
                    id: node.id(),
                    label: node.function.clone(),
                    node_type: if node.is_external { "external" } else { "internal" }.into(),
                    contract: node.contract.clone(),
                    is_external: node.is_external,
                    statements: Vec::new(),
                    span: None,
                },
            }
        })
        .collect();

    let edges: Vec<CytoscapeEdge> = cg
        .edge_indices()
        .enumerate()
        .map(|(i, edge_idx)| {
            let (src, dst) = cg.edge_endpoints(edge_idx).unwrap();
            let edge = &cg[edge_idx];
            CytoscapeEdge {
                data: CytoscapeEdgeData {
                    id: format!("e{i}"),
                    source: cg[src].id(),
                    target: cg[dst].id(),
                    kind: format!("{:?}", edge.kind),
                    call_count: edge.call_count,
                    kind_label: Some(edge.kind.label().to_string()),
                    static_target: edge.static_target.clone(),
                },
            }
        })
        .collect();

    Ok(Json(CytoscapeGraph { nodes, edges }))
}


/// Selects the function behind `{func}`: a bare name when unambiguous on the contract, or a
/// full signature such as `setPoolPause(bool,uint40)`. An ambiguous bare name is a 409 that
/// lists the candidate signatures.
fn route_function<'a>(
    state: &'a AppState,
    contract_name: &str,
    func: &str,
) -> Result<(&'a ContractDef, &'a FunctionDef), Response> {
    let contract = route_contract(state, contract_name)?;
    state.project.lookup_function(contract, func).map_err(|e| match e {
        FunctionLookupError::NotFound => StatusCode::NOT_FOUND.into_response(),
        FunctionLookupError::Ambiguous(candidates) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("'{func}' is overloaded on {contract_name}; use a signature"),
                "candidates": candidates,
            })),
        )
            .into_response(),
    })
}

pub async fn get_cfg(
    State(state): State<Arc<AppState>>,
    Path((contract_name, func_name)): Path<(String, String)>,
) -> Result<Json<CytoscapeGraph>, Response> {
    let (owning, func) = route_function(&state, &contract_name, &func_name)?;

    let key = (owning.key().to_string(), func.key.clone());
    let cfg = state.cfgs.get(&key).ok_or_else(|| StatusCode::NOT_FOUND.into_response())?;

    let nodes: Vec<CytoscapeNode> = cfg
        .node_indices()
        .map(|idx| {
            let block = &cfg[idx];
            let stmts: Vec<String> = block.statements.iter().map(|s| summarize_stmt(s)).collect();
            let label = make_block_label(block, &stmts);
            CytoscapeNode {
                data: CytoscapeNodeData {
                    id: format!("b{}", block.id),
                    label,
                    node_type: format!("{:?}", block.kind),
                    contract: key.0.clone(),
                    is_external: false,
                    statements: stmts,
                    span: block.span,
                },
            }
        })
        .collect();

    let edges: Vec<CytoscapeEdge> = cfg
        .edge_indices()
        .enumerate()
        .map(|(i, edge_idx)| {
            let (src, dst) = cfg.edge_endpoints(edge_idx).unwrap();
            let edge = &cfg[edge_idx];
            CytoscapeEdge {
                data: CytoscapeEdgeData {
                    id: format!("e{i}"),
                    source: format!("b{}", cfg[src].id),
                    target: format!("b{}", cfg[dst].id),
                    kind: format!("{:?}", edge),
                    call_count: 0,
                    kind_label: None,
                    static_target: None,
                },
            }
        })
        .collect();

    Ok(Json(CytoscapeGraph { nodes, edges }))
}


#[derive(Serialize)]
pub struct FunctionSourceResponse {
    pub file_path: String,
    pub source: String,
    pub span: SourceSpan,
}

pub async fn get_function_source(
    State(state): State<Arc<AppState>>,
    Path((contract_name, func_name)): Path<(String, String)>,
) -> Result<Json<FunctionSourceResponse>, Response> {
    let (_owning, func) = route_function(&state, &contract_name, &func_name)?;

    let file = state.project.source_files.get(func.span.file_index)
        .ok_or_else(|| StatusCode::NOT_FOUND.into_response())?;

    let source = slice_lines(
        &file.content,
        func.span.start_line as usize,
        func.span.end_line as usize,
    );

    Ok(Json(FunctionSourceResponse {
        file_path: file.path.clone(),
        source,
        span: func.span,
    }))
}

fn slice_lines(src: &str, start_1based: usize, end_1based: usize) -> String {
    if start_1based == 0 || end_1based < start_1based {
        return String::new();
    }
    src.lines()
        .skip(start_1based - 1)
        .take(end_1based - start_1based + 1)
        .collect::<Vec<_>>()
        .join("\n")
}


#[derive(Deserialize)]
pub struct PageQuery {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

// larger limits are clamped; the response's `page` reports the one applied
fn page_window(offset: Option<usize>, limit: Option<usize>) -> (usize, usize) {
    (offset.unwrap_or(0), limit.unwrap_or(DEFAULT_PAGE_LIMIT).min(MAX_PAGE_LIMIT))
}

pub async fn get_paths(
    State(state): State<Arc<AppState>>,
    Path((contract_name, func_name)): Path<(String, String)>,
    Query(query): Query<PageQuery>,
) -> Result<Json<PathTreePage>, Response> {
    let (owning, func) = route_function(&state, &contract_name, &func_name)?;

    let key = (owning.key().to_string(), func.key.clone());
    let (offset, limit) = page_window(query.offset, query.limit);
    state
        .path_trees
        .get(&key)
        .map(|pt| Json(pt.page(offset, limit)))
        .ok_or_else(|| StatusCode::NOT_FOUND.into_response())
}


/// Deepest sequence depth served; the count grows as functions^depth.
pub const MAX_SEQUENCE_DEPTH: usize = 8;

#[derive(Deserialize)]
pub struct SequenceQuery {
    pub depth: Option<usize>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

pub async fn get_sequences(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(query): Query<SequenceQuery>,
) -> Result<Json<SequencePage>, Response> {
    let contract = route_contract(&state, &name)?;
    let tree = state
        .sequence_trees
        .get(contract.key())
        .ok_or_else(|| StatusCode::NOT_FOUND.into_response())?;
    let depth = query.depth.unwrap_or(tree.max_depth);
    if depth > MAX_SEQUENCE_DEPTH {
        let message = format!("depth must be at most {MAX_SEQUENCE_DEPTH}");
        return Err((StatusCode::BAD_REQUEST, message).into_response());
    }
    let (offset, limit) = page_window(query.offset, query.limit);
    Ok(Json(tree.page(depth, offset, limit)))
}


use ilold_core::sequence::analysis::{analyze_sequences, SequenceAnalysis};

pub async fn get_sequence_analysis(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<SequenceAnalysis>, Response> {
    let contract = route_contract(&state, &name)?;
    // the project analysis carries transitive effects and the inherited functions
    let analysis = state
        .sequence_analyses
        .get(contract.key())
        .cloned()
        .unwrap_or_else(|| analyze_sequences(&state.path_trees, contract.key()));
    Ok(Json(analysis))
}


#[derive(Serialize)]
pub struct SearchSuggestions {
    pub functions: Vec<String>,
    pub state_vars: Vec<String>,
    pub events: Vec<String>,
    pub external_calls: Vec<String>,
    pub categories: Vec<SuggestionCategory>,
}

#[derive(Serialize)]
pub struct SuggestionCategory {
    pub label: String,
    pub items: Vec<String>,
}

pub async fn get_search_suggestions(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<SearchSuggestions>, Response> {
    let contract = route_contract(&state, &name)?;

    let accessible = state.project.accessible_functions(contract);
    let functions: Vec<String> = accessible
        .iter()
        .filter(|af| !af.function.name.is_empty())
        .map(|af| af.function.name.clone())
        .collect();

    let state_vars: Vec<String> = state.project
        .inherited_state_vars(contract)
        .into_iter()
        .map(|sv| sv.name)
        .collect();

    let events: Vec<String> = contract
        .events
        .iter()
        .map(|e| e.name.clone())
        .collect();

    let mut origins: std::collections::HashSet<String> = std::collections::HashSet::new();
    origins.insert(contract.key().to_string());
    for af in &accessible {
        if af.is_inherited {
            origins.insert(af.origin.clone());
        }
    }

    let mut ext_calls = std::collections::BTreeSet::new();
    for ((c, _), pt) in &state.path_trees {
        if !origins.contains(c) { continue; }
        for annotations in pt.block_annotations() {
            for call in &annotations.external_calls {
                ext_calls.insert(format!("{}.{}", call.target, call.function));
            }
        }
    }
    let external_calls: Vec<String> = ext_calls.into_iter().collect();

    let categories = vec![
        SuggestionCategory {
            label: "Functions".into(),
            items: functions.clone(),
        },
        SuggestionCategory {
            label: "State Variables".into(),
            items: state_vars.clone(),
        },
        SuggestionCategory {
            label: "Events".into(),
            items: events.clone(),
        },
        SuggestionCategory {
            label: "External Calls".into(),
            items: external_calls.clone(),
        },
        SuggestionCategory {
            label: "Path Types".into(),
            items: vec!["revert".into(), "return".into(), "assembly".into()],
        },
    ];

    Ok(Json(SearchSuggestions {
        functions,
        state_vars,
        events,
        external_calls,
        categories,
    }))
}


use ilold_core::cfg::types::{BasicBlock, BlockKind, CfgStatement};

fn summarize_stmt(stmt: &CfgStatement) -> String {
    match stmt {
        CfgStatement::RequireCheck { condition, .. } => format!("require({})", condition),
        CfgStatement::AssertCheck { condition, .. } => format!("assert({})", condition),
        CfgStatement::ExternalCall { target, function, .. } => format!("{}.{}()", target, function),
        CfgStatement::InternalCall { function, .. } => format!("{}()", function),
        CfgStatement::EmitEvent { event, .. } => format!("emit {}", event),
        CfgStatement::StateWrite { variable, .. } => format!("{} = ...", variable),
        CfgStatement::StateRead { variable, .. } => format!("read {}", variable),
        CfgStatement::EthTransfer { to, .. } => format!("transfer → {}", to),
        CfgStatement::Assignment { target, .. } => {
            if target.is_empty() {
                "...".into()
            } else {
                format!("{} = ...", target)
            }
        }
        CfgStatement::AssemblyBlock { .. } => "assembly { ... }".into(),
        CfgStatement::UnresolvedModifier { name, .. } => format!("{name} (unresolved modifier)"),
    }
}

fn make_block_label(block: &BasicBlock, stmts: &[String]) -> String {
    match block.kind {
        BlockKind::Entry => {
            if stmts.is_empty() {
                "▶ Entry".into()
            } else {
                let s = truncate_label(&stmts[0], 24);
                format!("▶ {s}")
            }
        }
        BlockKind::Return => "✓ Return".into(),
        BlockKind::Revert => "✗ Revert".into(),
        BlockKind::Assembly => "⚙ Assembly".into(),
        BlockKind::LoopCondition => "⟳ Loop".into(),
        BlockKind::Normal => {
            if stmts.is_empty() {
                "·".into()
            } else {
                let first = truncate_label(&stmts[0], 24);
                if stmts.len() == 1 {
                    first
                } else {
                    format!("{first}  (+{})", stmts.len() - 1)
                }
            }
        }
    }
}

fn truncate_label(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &s[..cut]),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn labels_are_cut_on_character_boundaries() {
        assert_eq!(super::truncate_label("short", 24), "short");
        assert_eq!(super::truncate_label("ñandú", 3), "ñan…");
        assert_eq!(super::truncate_label("aéb", 2), "aé…");
        assert_eq!(super::truncate_label(&"é".repeat(30), 24), format!("{}…", "é".repeat(24)));
    }
}
