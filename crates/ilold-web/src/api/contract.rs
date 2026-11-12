use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use ilold_core::callgraph::types::{CallKind, CallNode};
use ilold_core::classify::entry_points::{classify_in, AccessLevel};
use ilold_core::classify::guards::GuardFact;
use ilold_core::model::common::{SourceSpan, StateVar};
use ilold_core::model::contract::ContractDef;
use ilold_core::model::function::FunctionDef;
use ilold_core::model::project::{ContractLookupError, FunctionLookupError};
use ilold_core::pathtree::types::PathTreePage;
use ilold_core::relations::{
    declared_target, external_calls, key_kinds, phase_candidates, storage_accesses, try_catches, value_facts, value_flows, AssignmentSite,
    BalanceRead, ContractLink, Dispatch, DispatchCandidate, KeyKind, PhaseCandidate, StorageAccess, TryCatchFact, UnresolvedReason, ValueFlow,
    ValueTransfer,
};
use ilold_core::sequence::types::SequencePage;
use ilold_core::util::{DEFAULT_PAGE_LIMIT, MAX_PAGE_LIMIT};

use super::ApiError;
use crate::state::AppState;


#[derive(Serialize)]
pub struct ContractDetail {
    /// The contract key: the bare name when unique, otherwise the qualified id.
    pub name: String,
    pub display_name: String,
    pub qualified_id: String,
    pub is_dependency: bool,
    pub kind: String,
    pub inherits: Vec<String>,
    pub inherit_keys: Vec<String>,
    pub functions: Vec<FunctionSummary>,
    pub state_vars: Vec<StateVarSummary>,
    pub inherited_functions: Vec<FunctionSummary>,
    pub inherited_state_vars: Vec<StateVarSummary>,
    pub outgoing_links: Vec<ContractLink>,
    pub incoming_links: Vec<ContractLink>,
    /// The transfers of its own and inherited functions, summed by direction, asset and counterparty.
    pub value_flows: Vec<ValueFlow>,
    /// Variables that gate which of its entry points can run; none unless it is a deployable project contract.
    pub phase_candidates: Vec<PhaseCandidate>,
}

#[derive(Serialize)]
pub struct FunctionSummary {
    pub name: String,
    /// The bare name when it selects the function on this contract, otherwise the signature.
    pub key: String,
    pub signature: String,
    pub kind: String,
    pub visibility: String,
    pub mutability: String,
    pub params: Vec<ParamSummary>,
    pub path_count: usize,
    pub happy_paths: usize,
    pub revert_paths: usize,
    /// Counts cover only the paths found.
    pub paths_truncated: bool,
    pub modifiers: Vec<String>,
    pub access: AccessLevel,
    /// As this contract runs the function.
    pub guards: Vec<GuardFact>,
    /// The reads and writes of its own code and modifiers, with their keys.
    pub accesses: Vec<StorageAccess>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub try_catches: Vec<TryCatchFact>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub value_transfers: Vec<ValueTransfer>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub balance_reads: Vec<BalanceRead>,
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
    /// The kinds of key the functions the contract runs index it with.
    pub key_kinds: Vec<KeyKind>,
}

/// A key, a qualified id or an unambiguous bare name; an ambiguous name is a 409 with candidates.
pub(crate) fn route_contract<'a>(state: &'a AppState, name: &str) -> Result<&'a ContractDef, ApiError> {
    state.project.lookup_contract(name).map_err(|e| ApiError::from(match e {
        ContractLookupError::NotFound => {
            (StatusCode::NOT_FOUND, format!("Contract '{name}' not found")).into_response()
        }
        ContractLookupError::Ambiguous(candidates) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("contract name '{name}' is ambiguous; use a qualified id"),
                "candidates": candidates,
            })),
        )
            .into_response(),
    }))
}

pub async fn get_contract(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<ContractDetail>, ApiError> {
    let contract = route_contract(&state, &name)?;

    let summary = |owner: &ContractDef, f: &FunctionDef| -> FunctionSummary {
        let pt = state.path_trees.get(&(owner.key().to_string(), f.key.clone()));
        let (access, guards) = classify_in(&state.project, contract, owner, f);
        let cfg = state.cfgs.get(&(owner.key().to_string(), f.key.clone()));
        let values = cfg.map(|cfg| value_facts(&state.project, owner, owner, f, cfg)).unwrap_or_default();
        FunctionSummary {
            name: f.name.clone(),
            key: state.project.route_key(contract, f),
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
            accesses: cfg.map(|cfg| storage_accesses(&state.project, owner, owner, f, cfg)).unwrap_or_default(),
            try_catches: cfg.map(|cfg| try_catches(&state.project, owner, owner, f, cfg)).unwrap_or_default(),
            value_transfers: values.transfers,
            balance_reads: values.balance_reads,
        }
    };
    let kinds = key_kinds(&state.project, contract, &state.cfgs);
    let var_summary = |sv: &StateVar| StateVarSummary {
        name: sv.name.clone(),
        type_name: sv.type_name.clone(),
        visibility: format!("{:?}", sv.visibility),
        is_constant: sv.is_constant,
        is_immutable: sv.is_immutable,
        key_kinds: kinds.get(&sv.name).map(|k| k.iter().copied().collect()).unwrap_or_default(),
    };

    let functions: Vec<FunctionSummary> = contract.functions.iter().map(|f| summary(contract, f)).collect();
    let state_vars = contract.state_vars.iter().map(var_summary).collect();
    let inherited_functions: Vec<FunctionSummary> = state.project
        .accessible_functions(contract)
        .into_iter()
        .filter(|af| af.is_inherited)
        .map(|af| summary(af.owner, af.function))
        .collect();
    let behaviors = state.sequence_analyses.get(contract.key()).map(|s| s.functions.as_slice()).unwrap_or_default();
    let transfers: Vec<ValueTransfer> = functions.iter().chain(&inherited_functions).flat_map(|f| f.value_transfers.iter().cloned()).collect();
    // storage order, most base contract first
    let inherited_state_vars = state.project
        .accessible_state_vars(contract)
        .into_iter()
        .filter(|(_, _, inherited)| *inherited)
        .map(|(sv, _, _)| var_summary(&sv))
        .collect();

    Ok(Json(ContractDetail {
        name: contract.key().to_string(),
        display_name: contract.name.clone(),
        qualified_id: contract.qualified_id.clone(),
        is_dependency: contract.is_dependency,
        kind: format!("{:?}", contract.kind),
        inherits: contract.inherits.clone(),
        inherit_keys: state.project.base_keys(contract),
        functions,
        state_vars,
        inherited_functions,
        inherited_state_vars,
        outgoing_links: state.relations.outgoing(contract.key()).cloned().collect(),
        incoming_links: state.relations.incoming(contract.key()).cloned().collect(),
        value_flows: value_flows(&transfers),
        phase_candidates: phase_candidates(&state.project, contract, behaviors),
    }))
}


#[derive(Serialize)]
pub struct CytoscapeGraph {
    pub nodes: Vec<CytoscapeNode>,
    pub edges: Vec<CytoscapeEdge>,
    /// CFGs: the try statements, naming the blocks of their branches.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub try_catches: Vec<TryCatchFact>,
}

#[derive(Serialize)]
pub struct CytoscapeNode {
    pub data: CytoscapeNodeData,
}

#[derive(Serialize)]
pub struct CytoscapeNodeData {
    pub id: String,
    pub label: String,
    /// Call-graph nodes of functions the contract runs: the key its function routes take.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub node_type: String,
    pub contract: String,
    pub is_external: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub statements: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// CFG blocks: the reads and writes they make, with their keys.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub accesses: Vec<StorageAccess>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind_label: Option<String>,
    /// Declaration named in the source when virtual dispatch picked another implementation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub static_target: Option<String>,
    /// External calls to a contract: what may answer them, the same for every call site.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_candidates: Option<Vec<DispatchCandidate>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unresolved_reason: Option<UnresolvedReason>,
    /// Where the state variables holding the called address are set, over every call site.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_at: Option<Vec<AssignmentSite>>,
}

/// What may answer a call-graph edge to `target`, for calls to a contract only.
pub(crate) fn edge_dispatch(state: &AppState, context: &ContractDef, kind: CallKind, target: &CallNode) -> Option<Dispatch> {
    if !matches!(kind, CallKind::External | CallKind::ExternalSelf) {
        return None;
    }
    let key = if target.key.is_empty() { &target.function } else { &target.key };
    let declared = declared_target(&state.project, &target.contract, key);
    Some(state.relations.dispatch(&state.project, context, kind, declared.as_deref()).into_owned())
}

/// The assignments of every state variable `source` calls `dispatch`'s target through.
fn edge_assignments(state: &AppState, source: &CallNode, kind: CallKind, dispatch: &Dispatch) -> Vec<AssignmentSite> {
    let Ok(owner) = state.project.lookup_contract(&source.contract) else { return Vec::new() };
    let Some(cfg) = state.cfgs.get(&(source.contract.clone(), source.key.clone())) else { return Vec::new() };
    let mut sites: Vec<AssignmentSite> = Vec::new();
    for call in external_calls(&state.project, owner, cfg).into_iter().filter(|c| c.kind == kind && c.target == dispatch.target) {
        for site in call.via.as_deref().map(|v| state.relations.assignments(v)).unwrap_or_default() {
            if !sites.contains(site) {
                sites.push(site.clone());
            }
        }
    }
    sites
}

pub async fn get_callgraph(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<CytoscapeGraph>, ApiError> {
    let contract = route_contract(&state, &name)?;
    let cg = state
        .call_graphs
        .get(contract.key())
        .ok_or_else(|| ApiError::from(StatusCode::NOT_FOUND.into_response()))?;

    let accessible = state.project.accessible_functions(contract);
    let nodes: Vec<CytoscapeNode> = cg
        .node_indices()
        .map(|idx| {
            let node = &cg[idx];
            let key = accessible
                .iter()
                .find(|af| af.owner.key() == node.contract && af.function.key == node.key)
                .map(|af| state.project.route_key(contract, af.function));
            CytoscapeNode {
                data: CytoscapeNodeData {
                    id: node.id(),
                    label: node.function.clone(),
                    key,
                    node_type: if node.is_external { "external" } else { "internal" }.into(),
                    contract: node.contract.clone(),
                    is_external: node.is_external,
                    statements: Vec::new(),
                    span: None,
                    file: None,
                    accesses: Vec::new(),
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
            let dispatch = edge_dispatch(&state, contract, edge.kind, &cg[dst]);
            CytoscapeEdge {
                data: CytoscapeEdgeData {
                    id: format!("e{i}"),
                    source: cg[src].id(),
                    target: cg[dst].id(),
                    kind: format!("{:?}", edge.kind),
                    call_count: edge.call_count,
                    kind_label: Some(edge.kind.label().to_string()),
                    static_target: edge.static_target.clone(),
                    assigned_at: dispatch.as_ref().map(|d| edge_assignments(&state, &cg[src], edge.kind, d)),
                    unresolved_reason: dispatch.as_ref().and_then(|d| d.unresolved_reason),
                    dispatch_candidates: dispatch.map(|d| d.candidates),
                },
            }
        })
        .collect();

    Ok(Json(CytoscapeGraph { nodes, edges, try_catches: Vec::new() }))
}


/// A bare name when unambiguous, or a full signature; an ambiguous name is a 409 with candidates.
fn route_function<'a>(
    state: &'a AppState,
    contract_name: &str,
    func: &str,
) -> Result<(&'a ContractDef, &'a FunctionDef), ApiError> {
    let contract = route_contract(state, contract_name)?;
    state.project.lookup_function(contract, func).map_err(|e| ApiError::from(match e {
        FunctionLookupError::NotFound => StatusCode::NOT_FOUND.into_response(),
        FunctionLookupError::Ambiguous(candidates) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("'{func}' is overloaded on {contract_name}; use a signature"),
                "candidates": candidates,
            })),
        )
            .into_response(),
    }))
}

pub async fn get_cfg(
    State(state): State<Arc<AppState>>,
    Path((contract_name, func_name)): Path<(String, String)>,
) -> Result<Json<CytoscapeGraph>, ApiError> {
    let (owning, func) = route_function(&state, &contract_name, &func_name)?;

    let key = (owning.key().to_string(), func.key.clone());
    let cfg = state.cfgs.get(&key).ok_or_else(|| ApiError::from(StatusCode::NOT_FOUND.into_response()))?;
    let mut accesses = storage_accesses(&state.project, owning, owning, func, cfg);

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
                    key: None,
                    node_type: format!("{:?}", block.kind),
                    contract: key.0.clone(),
                    is_external: false,
                    statements: stmts,
                    span: block.span,
                    file: block.span
                        .and_then(|span| state.project.source_files.get(span.file_index))
                        .map(|f| f.path.clone()),
                    accesses: accesses.extract_if(.., |a| a.block == block.id).collect(),
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
                    dispatch_candidates: None,
                    unresolved_reason: None,
                    assigned_at: None,
                },
            }
        })
        .collect();

    Ok(Json(CytoscapeGraph { nodes, edges, try_catches: try_catches(&state.project, owning, owning, func, cfg) }))
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
) -> Result<Json<FunctionSourceResponse>, ApiError> {
    let (_owning, func) = route_function(&state, &contract_name, &func_name)?;

    let file = state.project.source_files.get(func.span.file_index)
        .ok_or_else(|| ApiError::from(StatusCode::NOT_FOUND.into_response()))?;

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

impl PageQuery {
    // larger limits are clamped; the response's `page` reports the one applied
    fn window(&self) -> (usize, usize) {
        (self.offset.unwrap_or(0), self.limit.unwrap_or(DEFAULT_PAGE_LIMIT).min(MAX_PAGE_LIMIT))
    }
}

pub async fn get_paths(
    State(state): State<Arc<AppState>>,
    Path((contract_name, func_name)): Path<(String, String)>,
    Query(query): Query<PageQuery>,
) -> Result<Json<PathTreePage>, ApiError> {
    let (owning, func) = route_function(&state, &contract_name, &func_name)?;

    let key = (owning.key().to_string(), func.key.clone());
    let (offset, limit) = query.window();
    state
        .path_trees
        .get(&key)
        .map(|pt| Json(pt.page(offset, limit)))
        .ok_or_else(|| ApiError::from(StatusCode::NOT_FOUND.into_response()))
}


pub const MAX_SEQUENCE_DEPTH: usize = 8;

pub fn valid_sequence_depth(depth: usize) -> bool {
    (1..=MAX_SEQUENCE_DEPTH).contains(&depth)
}

#[derive(Deserialize)]
pub struct DepthQuery {
    pub depth: Option<usize>,
}

pub async fn get_sequences(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(query): Query<DepthQuery>,
    Query(page): Query<PageQuery>,
) -> Result<Json<SequencePage>, ApiError> {
    let contract = route_contract(&state, &name)?;
    let tree = state
        .sequence_trees
        .get(contract.key())
        .ok_or_else(|| ApiError::from(StatusCode::NOT_FOUND.into_response()))?;
    let depth = query.depth.unwrap_or(tree.max_depth);
    if !valid_sequence_depth(depth) {
        let message = format!("depth must be between 1 and {MAX_SEQUENCE_DEPTH}");
        return Err((StatusCode::BAD_REQUEST, message).into());
    }
    let (offset, limit) = page.window();
    Ok(Json(tree.page(depth, offset, limit)))
}


use ilold_core::sequence::analysis::{analyze_sequences, SequenceAnalysis};

pub async fn get_sequence_analysis(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<SequenceAnalysis>, ApiError> {
    let contract = route_contract(&state, &name)?;
    // the project analysis carries transitive effects and inherited functions
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
) -> Result<Json<SearchSuggestions>, ApiError> {
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
            origins.insert(af.owner.key().to_string());
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
