use std::sync::Arc;

use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use ilold_core::model::contract::ContractDef;

use crate::state::AppState;

/// `?include_dependencies=true` adds dependency contracts (libraries, vendored code) to a
/// listing; by default only the project sources are listed.
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
    /// Dependency contracts in the model, listed or not.
    pub dependency_contracts: usize,
    /// Non-fatal compilation problems (e.g. test files left out of the model).
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
pub struct ContractSummary {
    /// Contract key: the route segment, the bare name unless it is ambiguous.
    pub name: String,
    pub display_name: String,
    pub qualified_id: String,
    pub is_dependency: bool,
    pub kind: String,
    pub folder: String,
    pub functions: usize,
    pub state_vars: usize,
    pub inherits: Vec<String>,
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
        })
        .collect();

    Json(ProjectSummary {
        files: state.project.source_files.len(),
        contracts,
        dependency_contracts: state.project.contracts.iter().filter(|c| c.is_dependency).count(),
        warnings: state.project.warnings.clone(),
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
    pub has_external_calls: bool,
}

#[derive(Serialize)]
pub struct MapStateVar {
    pub name: String,
    pub type_name: String,
    pub is_constant: bool,
}

#[derive(Serialize)]
pub struct MapRelationship {
    pub from_contract: String,
    pub from_function: String,
    pub to_contract: String,
    pub to_function: String,
    pub kind: String,
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
                    .map(|p| p.paths.iter().any(|path| !path.annotations.external_calls.is_empty()))
                    .unwrap_or(false);
                MapFunction {
                    name: f.name.clone(),
                    visibility: format!("{:?}", f.visibility),
                    mutability: format!("{:?}", f.mutability),
                    path_count: pt.map(|p| p.stats.total_paths).unwrap_or(0),
                    happy_paths: pt.map(|p| p.stats.happy_paths).unwrap_or(0),
                    revert_paths: pt.map(|p| p.stats.revert_paths).unwrap_or(0),
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
                    relationships.push(MapRelationship {
                        from_contract: src_node.contract.clone(),
                        from_function: src_node.function.clone(),
                        to_contract: dst_node.contract.clone(),
                        to_function: dst_node.function.clone(),
                        kind: format!("{:?}", edge.kind),
                    });
                }
            }
        }
    }

    Json(ProjectMap {
        contracts,
        relationships,
    })
}
