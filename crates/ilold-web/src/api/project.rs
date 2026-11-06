use std::sync::Arc;

use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use ilold_core::model::contract::ContractDef;
use ilold_core::relations::{Basis, DispatchCandidate, Tag, UnresolvedReason};

use crate::api::contract::edge_dispatch;
use crate::state::AppState;

#[derive(Deserialize, Default)]
pub struct ListingQuery {
    #[serde(default)]
    pub include_dependencies: bool,
}

impl ListingQuery {
    pub fn lists(&self, contract: &ContractDef) -> bool {
        self.include_dependencies || contract.is_listed()
    }
}

#[derive(Serialize)]
pub struct ProjectSummary {
    pub files: usize,
    pub contracts: Vec<ContractSummary>,
    pub dependency_contracts: usize,
    pub hidden_contracts: usize,
    pub warnings: Vec<String>,
    pub default_contract: Option<String>,
}

#[derive(Serialize)]
pub struct ContractSummary {
    /// The bare name unless it is ambiguous.
    pub name: String,
    pub display_name: String,
    pub qualified_id: String,
    pub is_dependency: bool,
    pub kind: String,
    pub folder: String,
    pub functions: usize,
    pub state_vars: usize,
    pub inherits: Vec<String>,
    pub inherit_keys: Vec<String>,
}

pub async fn get_project(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListingQuery>,
) -> Json<ProjectSummary> {
    let contracts = state
        .project
        .contracts
        .iter()
        .filter(|c| query.lists(c))
        .map(|c| ContractSummary {
            name: c.key().to_string(),
            display_name: c.name.clone(),
            qualified_id: c.qualified_id.clone(),
            is_dependency: c.is_dependency,
            kind: format!("{:?}", c.kind),
            folder: state.project.contract_folder(c),
            functions: c.functions.len(),
            state_vars: c.state_vars.len(),
            inherits: c.inherits.clone(),
            inherit_keys: state.project.base_keys(c),
        })
        .collect();

    Json(ProjectSummary {
        files: state.project.source_files.len(),
        contracts,
        dependency_contracts: state.project.contracts.iter().filter(|c| c.is_dependency).count(),
        hidden_contracts: state.project.hidden_contracts(),
        warnings: state.project.warnings.clone(),
        default_contract: state.project.default_contract().map(|c| c.key().to_string()),
    })
}


#[derive(Serialize)]
pub struct ProjectMap {
    pub contracts: Vec<MapContract>,
    pub relationships: Vec<MapRelationship>,
}

#[derive(Serialize)]
pub struct MapContract {
    pub name: String,
    pub kind: String,
    pub folder: String,
    pub inherits: Vec<String>,
    pub inherit_keys: Vec<String>,
    pub functions: Vec<MapFunction>,
    pub state_vars: Vec<MapStateVar>,
}

#[derive(Serialize)]
pub struct MapFunction {
    pub name: String,
    pub visibility: String,
    pub mutability: String,
    pub path_count: usize,
    pub happy_paths: usize,
    pub revert_paths: usize,
    pub paths_truncated: bool,
    pub has_external_calls: bool,
}

#[derive(Serialize)]
pub struct MapStateVar {
    pub name: String,
    pub type_name: String,
    pub is_constant: bool,
}

/// A call between functions, or with kind `ContractLink` and no functions, a contract link.
#[derive(Serialize)]
pub struct MapRelationship {
    pub from_contract: String,
    pub from_function: Option<String>,
    pub to_contract: String,
    pub to_function: Option<String>,
    pub kind: String,
    pub call_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_candidates: Option<Vec<DispatchCandidate>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unresolved_reason: Option<UnresolvedReason>,
    pub basis: Option<Basis>,
    pub tag: Option<Tag>,
    pub via_state_vars: Vec<String>,
}

pub async fn get_project_map(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListingQuery>,
) -> Json<ProjectMap> {
    let mut contracts = Vec::new();
    let mut relationships = Vec::new();

    for contract in state.project.contracts.iter().filter(|c| query.lists(c)) {
        let functions: Vec<MapFunction> = contract
            .functions
            .iter()
            .filter(|f| !f.name.is_empty())
            .map(|f| {
                let key = (contract.key().to_string(), f.key.clone());
                let pt = state.path_trees.get(&key);
                let has_ext = pt
                    .is_some_and(|p| p.block_annotations().any(|a| !a.external_calls.is_empty()));
                MapFunction {
                    name: f.name.clone(),
                    visibility: format!("{:?}", f.visibility),
                    mutability: format!("{:?}", f.mutability),
                    path_count: pt.map(|p| p.stats.total_paths).unwrap_or(0),
                    happy_paths: pt.map(|p| p.stats.happy_paths).unwrap_or(0),
                    revert_paths: pt.map(|p| p.stats.revert_paths).unwrap_or(0),
                    paths_truncated: pt.is_some_and(|p| p.stats.truncated),
                    has_external_calls: has_ext,
                }
            })
            .collect();

        let state_vars: Vec<MapStateVar> = contract
            .state_vars
            .iter()
            .map(|sv| MapStateVar {
                name: sv.name.clone(),
                type_name: sv.type_name.clone(),
                is_constant: sv.is_constant,
            })
            .collect();

        contracts.push(MapContract {
            name: contract.key().to_string(),
            kind: format!("{:?}", contract.kind),
            folder: state.project.contract_folder(contract),
            inherits: contract.inherits.clone(),
            inherit_keys: state.project.base_keys(contract),
            functions,
            state_vars,
        });

        if let Some(cg) = state.call_graphs.get(contract.key()) {
            for edge_idx in cg.edge_indices() {
                let (src, dst) = cg.edge_endpoints(edge_idx).unwrap();
                let src_node = &cg[src];
                let dst_node = &cg[dst];
                let edge = &cg[edge_idx];

                // the graph also holds inherited bodies; keep this contract's own calls
                if src_node.contract != contract.key() {
                    continue;
                }
                if dst_node.is_external || src_node.contract != dst_node.contract {
                    let dispatch = edge_dispatch(&state, contract, edge.kind, dst_node);
                    relationships.push(MapRelationship {
                        from_contract: src_node.contract.clone(),
                        from_function: Some(src_node.function.clone()),
                        to_contract: dst_node.contract.clone(),
                        to_function: Some(dst_node.function.clone()),
                        kind: format!("{:?}", edge.kind),
                        call_count: edge.call_count,
                        unresolved_reason: dispatch.as_ref().and_then(|d| d.unresolved_reason),
                        dispatch_candidates: dispatch.map(|d| d.candidates),
                        basis: None,
                        tag: None,
                        via_state_vars: Vec::new(),
                    });
                }
            }
        }
        for link in state.relations.outgoing(contract.key()) {
            relationships.push(MapRelationship {
                from_contract: link.from_contract.clone(),
                from_function: None,
                to_contract: link.to_contract.clone(),
                to_function: None,
                kind: "ContractLink".into(),
                call_count: link.calls.len(),
                dispatch_candidates: None,
                unresolved_reason: None,
                basis: Some(link.basis),
                tag: Some(link.tag),
                via_state_vars: link.via_state_vars.clone(),
            });
        }
    }

    Json(ProjectMap {
        contracts,
        relationships,
    })
}
