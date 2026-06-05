use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use ilold_core::classify::entry_points::AccessLevel;
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

use crate::state::{AppState, ScenarioStore};

#[derive(Deserialize)]
pub struct CommandRequest {
    pub contract: Option<String>,
    pub command: SessionCommand,
}

fn build_analysis_data<'a>(
    state: &'a AppState,
    contract_name: &str,
) -> Result<AnalysisData<'a>, (StatusCode, String)> {
    let contract = state.project.contracts.iter()
        .find(|c| c.name == contract_name)
        .ok_or((StatusCode::NOT_FOUND, format!("Contract '{}' not found", contract_name)))?;

    let seq_analysis = state.sequence_analyses.get(contract_name)
        .ok_or((StatusCode::NOT_FOUND, "No analysis for contract".into()))?;

    let classifs = state.classifications.get(contract_name)
        .ok_or((StatusCode::NOT_FOUND, "No classifications for contract".into()))?;

    Ok(AnalysisData {
        project: &state.project,
        contract,
        cfgs: &state.cfgs,
        path_trees: &state.path_trees,
        behaviors: &seq_analysis.functions,
        transitions: &seq_analysis.transitions,
        classifications: classifs,
        all_sequence_analyses: &state.sequence_analyses,
        all_classifications: &state.classifications,
    })
}

fn resolve_contract(state: &AppState, explicit: Option<&str>) -> Result<String, (StatusCode, String)> {
    if let Some(name) = explicit {
        return Ok(name.to_string());
    }

    let scenarios_guard = state.scenarios.read().unwrap();
    let session = scenarios_guard.active_session();
    if !session.contract.is_empty() {
        return Ok(session.contract.clone());
    }
    drop(scenarios_guard);

    state.project.find_contract(None)
        .map(|c| c.name.clone())
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
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

    let mut scenarios_guard = state.scenarios.write().unwrap();
    let is_persistence = matches!(
        req.command,
        SessionCommand::SaveSession | SessionCommand::LoadSession { .. }
    );
    let needs_reset = !is_persistence
        && scenarios_guard.active_session().contract != contract_name;
    if needs_reset {
        let had_steps = !scenarios_guard.active_session().steps.is_empty();
        let active_before_reset = scenarios_guard.active().to_string();
        *scenarios_guard = ScenarioStore::new_for_contract(&contract_name);
        if had_steps {
            state.session_tx.send(CanvasPatch::ClearAll {
                scenario: active_before_reset,
            }).ok();
        }
    }
    match req.command {
        SessionCommand::Scenario { sub } => {
            let active_before = scenarios_guard.active().to_string();
            let result = execute_scenario(&mut scenarios_guard, sub, &timestamp, &contract_name);
            if let Some(patch) = canvas_patch_from(&result, &active_before) {
                state.session_tx.send(patch).ok();
            }
            Ok(Json(result))
        }
        SessionCommand::SaveSession => {
            let active_before = scenarios_guard.active().to_string();
            let result = match scenarios_guard.save_to_json() {
                Ok(json) => CommandResult::SessionSaved { json },
                Err(message) => CommandResult::Error { message },
            };
            if let Some(patch) = canvas_patch_from(&result, &active_before) {
                state.session_tx.send(patch).ok();
            }
            Ok(Json(result))
        }
        SessionCommand::LoadSession { json } => {
            let result = match ScenarioStore::load_from_json(&json) {
                Ok(loaded) => {
                    let contract = loaded.contract.clone();
                    let step_names: Vec<String> = loaded
                        .active_session()
                        .steps
                        .iter()
                        .map(|s| s.function.clone())
                        .collect();
                    *scenarios_guard = loaded;
                    CommandResult::SessionLoaded { contract, steps: step_names }
                }
                Err(message) => CommandResult::Error { message },
            };
            let active_after = scenarios_guard.active().to_string();
            if let Some(patch) = canvas_patch_from(&result, &active_after) {
                state.session_tx.send(patch).ok();
            }
            Ok(Json(result))
        }
        other => {
            let active_name = scenarios_guard.active().to_string();
            let session = scenarios_guard.active_session_mut();
            let result = execute_command(other, session, &data, &timestamp);
            if let Some(patch) = canvas_patch_from(&result, &active_name) {
                state.session_tx.send(patch).ok();
            }
            Ok(Json(result))
        }
    }
}

pub async fn get_step_detail(
    State(state): State<Arc<AppState>>,
    Path(step_index): Path<usize>,
) -> Result<Json<FunctionNarrative>, (StatusCode, String)> {
    let scenarios_guard = state.scenarios.read().unwrap();
    let session = scenarios_guard.active_session();

    let data = build_analysis_data(&state, &session.contract)?;

    let narrative = get_step_narrative(session, step_index, &data)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;

    Ok(Json(narrative))
}

pub async fn get_session_step_trace(
    State(state): State<Arc<AppState>>,
    Path(step_index): Path<usize>,
) -> Result<Json<FlowTree>, (StatusCode, String)> {
    let scenarios_guard = state.scenarios.read().unwrap();
    let session = scenarios_guard.active_session();

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
) -> Result<Json<VariableTimeline>, (StatusCode, String)> {
    let scenarios_guard = state.scenarios.read().unwrap();
    let session = scenarios_guard.active_session();

    let timeline = build_variable_timeline(session, &variable);
    Ok(Json(timeline))
}

pub async fn get_state_detail(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<VariableSummary>>, (StatusCode, String)> {
    let scenarios_guard = state.scenarios.read().unwrap();
    let session = scenarios_guard.active_session();

    Ok(Json(get_session_state(session)))
}

pub async fn get_sequence_detail(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SequenceNarrative>, (StatusCode, String)> {
    let scenarios_guard = state.scenarios.read().unwrap();
    let session = scenarios_guard.active_session();

    let data = build_analysis_data(&state, &session.contract)?;

    let narrative = get_sequence_narrative(session, &data)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    Ok(Json(narrative))
}

pub async fn get_function_detail(
    State(state): State<Arc<AppState>>,
    Path((contract_name, func_name)): Path<(String, String)>,
) -> Result<Json<FunctionNarrative>, (StatusCode, String)> {
    let data = build_analysis_data(&state, &contract_name)?;

    let narrative = get_function_info(&func_name, &data)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;

    Ok(Json(narrative))
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
}

pub async fn get_function_slice(
    State(state): State<Arc<AppState>>,
    Path((func_name, variable)): Path<(String, String)>,
    Query(params): Query<SliceQuery>,
) -> Result<Json<SliceResult>, (StatusCode, String)> {
    let contract_name = {
        let guard = state.scenarios.read().unwrap();
        guard.active_session().contract.clone()
    };

    let contract = state.project.contracts.iter()
        .find(|c| c.name == contract_name)
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("Contract '{}' not found", contract_name),
        ))?;

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
    pub active: String,
    pub scenarios: Vec<ScenarioSnapshot>,
}

pub async fn get_scenarios(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<ScenarioInfo>> {
    let guard = state.scenarios.read().unwrap();
    let active = guard.active().to_string();
    let items = guard
        .names()
        .iter()
        .map(|name| ScenarioInfo {
            name: name.clone(),
            active: name == &active,
            step_count: guard.get(name).map(|s| s.steps.len()).unwrap_or(0),
        })
        .collect();
    Json(items)
}

pub async fn get_all_scenarios(
    State(state): State<Arc<AppState>>,
) -> Json<AllScenariosResponse> {
    let guard = state.scenarios.read().unwrap();
    let active = guard.active().to_string();
    let mut scenarios: Vec<ScenarioSnapshot> = Vec::with_capacity(guard.len());
    for name in guard.names() {
        let Some(session) = guard.get(name) else { continue };
        let classifs = state.classifications.get(&session.contract);
        let steps = session
            .steps
            .iter()
            .enumerate()
            .map(|(idx, step)| {
                let access = classifs
                    .and_then(|c| c.iter().find(|(n, _)| n == &step.function))
                    .map(|(_, a)| a.clone())
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
    Json(AllScenariosResponse { active, scenarios })
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
