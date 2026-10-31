use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use ilold_core::classify::entry_points::{classify_named, AccessLevel};
use ilold_core::exploration::commands::{
    AnalysisData, CanvasPatch, CommandResult, ScenarioAction, ScenarioInfo, SessionCommand,
    canvas_patch_from, execute_command, get_flow_tree, get_function_info, get_sequence_narrative,
    get_session_state, get_step_narrative, validate_scenario_name,
};
use ilold_core::exploration::session::{ExplorationSession, ForkOrigin, VariableSummary};
use ilold_core::journal::types::JournalEntry;
use ilold_core::exploration::timeline::{build_variable_timeline, VariableTimeline};
use ilold_core::narrative::trace::FlowTree;
use ilold_core::narrative::types::{FunctionNarrative, SequenceNarrative};
use ilold_core::slicing::{build_slice_result, SliceDirection, SliceResult};

use ilold_core::model::project::ContractLookupError;

use crate::state::{AppState, ScenarioStore, SessionBroadcast};

#[derive(Deserialize)]
pub struct CommandRequest {
    pub contract: Option<String>,
    pub command: SessionCommand,
}

fn build_analysis_data<'a>(
    state: &'a AppState,
    contract_name: &str,
) -> Result<AnalysisData<'a>, (StatusCode, String)> {
    let contract = state.project.find_contract(Some(contract_name))
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    let contract_name = contract.key();

    let seq_analysis = state.sequence_analyses.get(contract_name)
        .ok_or((StatusCode::NOT_FOUND, "No analysis for contract".into()))?;

    Ok(AnalysisData {
        project: &state.project,
        contract,
        cfgs: &state.cfgs,
        path_trees: &state.path_trees,
        behaviors: &seq_analysis.functions,
        all_sequence_analyses: &state.sequence_analyses,
    })
}

/// Contract a session request targets: the explicit name when given (it must exist),
/// otherwise the contract named by the most recent `/api/cmd` (initially the
/// project's default contract).
fn resolve_contract(state: &AppState, explicit: Option<&str>) -> Result<String, (StatusCode, String)> {
    match explicit.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => state.project.lookup_contract(name)
            .map(|c| c.key().to_string())
            .map_err(|e| {
                let status = match e {
                    ContractLookupError::NotFound => StatusCode::NOT_FOUND,
                    ContractLookupError::Ambiguous(_) => StatusCode::CONFLICT,
                };
                (status, state.project.find_contract(Some(name)).err().unwrap_or_default())
            }),
        None => Ok(state.scenarios.read().unwrap().current_contract().to_string()),
    }
}

/// Optional `?contract=` selector accepted by every session-scoped read.
#[derive(Deserialize)]
pub struct ContractQuery {
    #[serde(default)]
    pub contract: Option<String>,
}

/// Runs `read` on the store of the resolved contract without changing ownership;
/// a contract with no store yet reads as a fresh empty one.
fn read_store<R>(
    state: &AppState,
    explicit: Option<&str>,
    read: impl FnOnce(&ScenarioStore) -> Result<R, (StatusCode, String)>,
) -> Result<R, (StatusCode, String)> {
    let contract = resolve_contract(state, explicit)?;
    let guard = state.scenarios.read().unwrap();
    match guard.store(&contract) {
        Some(store) => read(store),
        None => read(&ScenarioStore::new_for_contract(&contract)),
    }
}

fn broadcast(state: &AppState, contract: &str, patch: Option<CanvasPatch>) {
    if let Some(patch) = patch {
        state.session_tx.send(SessionBroadcast { contract: contract.to_string(), patch }).ok();
    }
}

fn timestamp_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("{}", d.as_secs()))
        .unwrap_or_default()
}

fn reserve_name(store: &ScenarioStore, name: &str) -> Result<(), CommandResult> {
    if let Err(e) = validate_scenario_name(name) {
        return Err(CommandResult::Error { message: e });
    }
    if store.contains(name) {
        return Err(CommandResult::Error {
            message: format!("Scenario '{name}' already exists"),
        });
    }
    Ok(())
}

fn execute_scenario(
    store: &mut ScenarioStore,
    action: ScenarioAction,
    timestamp: &str,
    contract: &str,
) -> CommandResult {
    match action {
        ScenarioAction::New { name } => {
            if let Err(err) = reserve_name(store, &name) {
                return err;
            }
            let session = ExplorationSession::new(contract, "ilold");
            store.insert(name.clone(), session);
            CommandResult::ScenarioCreated { name }
        }
        ScenarioAction::List => {
            let active = store.active().to_string();
            let items: Vec<ScenarioInfo> = store
                .names()
                .iter()
                .map(|n| ScenarioInfo {
                    name: n.clone(),
                    active: n == &active,
                    step_count: store.get(n).map(|s| s.steps.len()).unwrap_or(0),
                })
                .collect();
            CommandResult::ScenarioList { items }
        }
        ScenarioAction::Switch { name } => {
            let from = store.active().to_string();
            if name == from {
                return CommandResult::ScenarioSwitched { from, to: name };
            }
            match store.set_active(name.clone()) {
                Ok(()) => CommandResult::ScenarioSwitched { from, to: name },
                Err(e) => CommandResult::Error { message: e },
            }
        }
        ScenarioAction::Fork { name, at_step } => fork_scenario(store, name, at_step, timestamp),
        ScenarioAction::Delete { name } => {
            if name == store.active() {
                return CommandResult::Error {
                    message: "Cannot delete active scenario — switch first.".into(),
                };
            }
            if store.len() == 1 {
                return CommandResult::Error {
                    message: "Cannot delete the only remaining scenario.".into(),
                };
            }
            if !store.contains(&name) {
                return CommandResult::Error {
                    message: format!("Scenario '{name}' does not exist"),
                };
            }
            store.remove(&name);
            CommandResult::ScenarioDeleted { name }
        }
    }
}

fn fork_scenario(
    store: &mut ScenarioStore,
    new_name: String,
    at_step: Option<usize>,
    timestamp: &str,
) -> CommandResult {
    if let Err(err) = reserve_name(store, &new_name) {
        return err;
    }
    let from = store.active().to_string();
    let mut cloned = store.active_session().clone();

    let len = cloned.steps.len();
    let effective = match at_step {
        None => len,
        Some(n) if n > len => {
            let noun = if len == 1 { "step" } else { "steps" };
            return CommandResult::Error {
                message: format!(
                    "Cannot fork at step {n}: only {len} {noun} in active scenario"
                ),
            };
        }
        Some(n) => {
            cloned.steps.truncate(n);
            n
        }
    };

    cloned.forked_from = Some(ForkOrigin {
        scenario: from.clone(),
        at_step: effective,
    });
    cloned.journal.record(JournalEntry::BranchCreated {
        from_function: from.clone(),
        branch_function: new_name.clone(),
        timestamp: timestamp.to_string(),
    });
    store.insert(new_name.clone(), cloned);
    CommandResult::ScenarioForked {
        from,
        to: new_name,
        at_step: effective,
    }
}

pub async fn handle_command(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CommandRequest>,
) -> Result<Json<CommandResult>, (StatusCode, String)> {
    let contract_name = resolve_contract(&state, req.contract.as_deref())?;
    let data = build_analysis_data(&state, &contract_name)?;
    let timestamp = timestamp_now();

    let mut registry = state.scenarios.write().unwrap();
    if let SessionCommand::LoadSession { json } = req.command {
        let result = match ScenarioStore::load_from_json(&json) {
            Ok(loaded) if state.project.find_contract(Some(&loaded.contract)).is_err() => {
                CommandResult::Error {
                    message: format!("Saved session targets unknown contract '{}'", loaded.contract),
                }
            }
            Ok(loaded) => {
                let contract = loaded.contract.clone();
                let step_names: Vec<String> = loaded
                    .active_session()
                    .steps
                    .iter()
                    .map(|s| s.function.clone())
                    .collect();
                registry.replace(loaded);
                CommandResult::SessionLoaded { contract, steps: step_names }
            }
            Err(message) => CommandResult::Error { message },
        };
        let active_after = registry.active().to_string();
        let loaded_contract = registry.current_contract().to_string();
        broadcast(&state, &loaded_contract, canvas_patch_from(&result, &active_after));
        return Ok(Json(result));
    }

    let store = registry.select(&contract_name);
    let active_before = store.active().to_string();
    let result = match req.command {
        SessionCommand::Scenario { sub } => {
            execute_scenario(store, sub, &timestamp, &contract_name)
        }
        SessionCommand::SaveSession => match store.save_to_json() {
            Ok(json) => CommandResult::SessionSaved { json },
            Err(message) => CommandResult::Error { message },
        },
        other => execute_command(other, store.active_session_mut(), &data, &timestamp),
    };
    broadcast(&state, &contract_name, canvas_patch_from(&result, &active_before));
    Ok(Json(result))
}

pub async fn get_step_detail(
    State(state): State<Arc<AppState>>,
    Path(step_index): Path<usize>,
    Query(query): Query<ContractQuery>,
) -> Result<Json<FunctionNarrative>, (StatusCode, String)> {
    read_store(&state, query.contract.as_deref(), |store| {
        let session = store.active_session();
        let data = build_analysis_data(&state, &store.contract)?;
        get_step_narrative(session, step_index, &data)
            .map(narrated)
            .map_err(|e| (StatusCode::NOT_FOUND, e))
    })
}

pub async fn get_session_step_trace(
    State(state): State<Arc<AppState>>,
    Path(step_index): Path<usize>,
    Query(query): Query<ContractQuery>,
) -> Result<Json<FlowTree>, (StatusCode, String)> {
    read_store(&state, query.contract.as_deref(), |store| step_trace(store.active_session(), step_index))
}

fn step_trace(session: &ExplorationSession, step_index: usize) -> Result<Json<FlowTree>, (StatusCode, String)> {
    let step = session.steps.get(step_index)
        .ok_or((StatusCode::NOT_FOUND, format!("step {} not found", step_index)))?;

    let tree = step.flow_tree.clone()
        .ok_or((
            StatusCode::NOT_FOUND,
            format!(
                "step {} has no persisted trace (loaded from a pre-Phase-2a session); \
                 use 'tr <func>' to rebuild from source",
                step_index
            ),
        ))?;

    Ok(Json(tree))
}

pub async fn get_variable_timeline_handler(
    State(state): State<Arc<AppState>>,
    Path(variable): Path<String>,
    Query(query): Query<ContractQuery>,
) -> Result<Json<VariableTimeline>, (StatusCode, String)> {
    read_store(&state, query.contract.as_deref(), |store| {
        Ok(Json(build_variable_timeline(store.active_session(), &variable)))
    })
}

pub async fn get_state_detail(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ContractQuery>,
) -> Result<Json<Vec<VariableSummary>>, (StatusCode, String)> {
    read_store(&state, query.contract.as_deref(), |store| {
        Ok(Json(get_session_state(store.active_session())))
    })
}

pub async fn get_sequence_detail(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ContractQuery>,
) -> Result<Json<SequenceNarrative>, (StatusCode, String)> {
    read_store(&state, query.contract.as_deref(), |store| {
        let data = build_analysis_data(&state, &store.contract)?;
        get_sequence_narrative(store.active_session(), &data)
            .map(Json)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))
    })
}

pub async fn get_function_detail(
    State(state): State<Arc<AppState>>,
    Path((contract_name, func_name)): Path<(String, String)>,
) -> Result<Json<FunctionNarrative>, (StatusCode, String)> {
    let data = build_analysis_data(&state, &contract_name)?;

    get_function_info(&func_name, &data)
        .map(narrated)
        .map_err(|e| (StatusCode::NOT_FOUND, e))
}

/// Path narratives a response carries; the path pages hold every path.
pub const MAX_NARRATIVE_PATHS: usize = 100;

fn narrated(mut narrative: FunctionNarrative) -> Json<FunctionNarrative> {
    narrative.truncate_paths(MAX_NARRATIVE_PATHS);
    Json(narrative)
}

#[derive(Deserialize)]
pub struct TraceQuery {
    #[serde(default)]
    pub depth: Option<usize>,
    #[serde(default)]
    pub reverts: Option<bool>,
    #[serde(default)]
    pub expand: Option<String>,
}

pub async fn get_flow_trace(
    State(state): State<Arc<AppState>>,
    Path((contract_name, func_name)): Path<(String, String)>,
    Query(params): Query<TraceQuery>,
) -> Result<Json<FlowTree>, (StatusCode, String)> {
    let data = build_analysis_data(&state, &contract_name)?;

    let max_depth = params.depth.unwrap_or(2);
    let include_reverts = params.reverts.unwrap_or(false);
    let expand_set = parse_expand_set(params.expand.as_deref())
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let tree = get_flow_tree(&func_name, &data, max_depth, include_reverts, expand_set)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;

    Ok(Json(tree))
}

#[derive(Deserialize)]
pub struct SliceQuery {
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub contract: Option<String>,
}

pub async fn get_function_slice(
    State(state): State<Arc<AppState>>,
    Path((func_name, variable)): Path<(String, String)>,
    Query(params): Query<SliceQuery>,
) -> Result<Json<SliceResult>, (StatusCode, String)> {
    let contract_name = resolve_contract(&state, params.contract.as_deref())?;

    let contract = state.project.find_contract(Some(&contract_name))
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;

    let function = contract.functions.iter()
        .find(|f| f.name == func_name)
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("Function '{}' not found in {}", func_name, contract_name),
        ))?;

    let direction = parse_slice_direction(params.direction.as_deref())
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    Ok(Json(build_slice_result(
        &state.project,
        contract,
        function,
        &variable,
        direction,
    )))
}

fn parse_slice_direction(raw: Option<&str>) -> Result<SliceDirection, String> {
    let Some(value) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(SliceDirection::Both);
    };
    match value.to_ascii_lowercase().as_str() {
        "backward" | "back" | "b" => Ok(SliceDirection::Backward),
        "forward" | "fwd" | "f" => Ok(SliceDirection::Forward),
        "both" | "all" => Ok(SliceDirection::Both),
        other => Err(format!(
            "invalid direction {:?}, expected backward|forward|both",
            other
        )),
    }
}

#[derive(Serialize)]
pub struct SessionStepView {
    pub function: String,
    pub access: AccessLevel,
    pub step_index: usize,
}

#[derive(Serialize)]
pub struct ScenarioSnapshot {
    pub name: String,
    pub steps: Vec<SessionStepView>,
    pub forked_from: Option<ForkOrigin>,
}

#[derive(Serialize)]
pub struct AllScenariosResponse {
    pub contract: String,
    pub active: String,
    pub scenarios: Vec<ScenarioSnapshot>,
}

pub async fn get_scenarios(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ContractQuery>,
) -> Result<Json<Vec<ScenarioInfo>>, (StatusCode, String)> {
    read_store(&state, query.contract.as_deref(), |store| {
        let active = store.active();
        Ok(Json(store
            .names()
            .iter()
            .map(|name| ScenarioInfo {
                name: name.clone(),
                active: name == active,
                step_count: store.get(name).map(|s| s.steps.len()).unwrap_or(0),
            })
            .collect()))
    })
}

pub async fn get_all_scenarios(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ContractQuery>,
) -> Result<Json<AllScenariosResponse>, (StatusCode, String)> {
    read_store(&state, query.contract.as_deref(), |store| {
        let contract = state.project.lookup_contract(&store.contract).ok();
        let mut scenarios: Vec<ScenarioSnapshot> = Vec::with_capacity(store.len());
        for name in store.names() {
            let Some(session) = store.get(name) else { continue };
            let steps = session
                .steps
                .iter()
                .enumerate()
                .map(|(idx, step)| {
                    // steps of overloaded functions are stored by signature
                    let access = contract
                        .and_then(|c| classify_named(&state.project, c, &step.function))
                        .unwrap_or(AccessLevel::Internal);
                    SessionStepView {
                        function: step.function.clone(),
                        access,
                        step_index: idx,
                    }
                })
                .collect();
            scenarios.push(ScenarioSnapshot {
                name: name.clone(),
                steps,
                forked_from: session.forked_from.clone(),
            });
        }
        Ok(Json(AllScenariosResponse {
            contract: store.contract.clone(),
            active: store.active().to_string(),
            scenarios,
        }))
    })
}

fn parse_expand_set(raw: Option<&str>) -> Result<std::collections::HashSet<usize>, String> {
    let mut set = std::collections::HashSet::new();
    let raw = match raw {
        Some(s) if !s.trim().is_empty() => s,
        _ => return Ok(set),
    };
    for part in raw.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let id: usize = trimmed.parse()
            .map_err(|_| format!("invalid step_id in expand: {:?}", trimmed))?;
        set.insert(id);
    }
    Ok(set)
}
