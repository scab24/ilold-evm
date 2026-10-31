use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::cfg::types::CfgGraph;
use crate::classify::entry_points::{classify_all, AccessLevel};
use crate::journal::types::{Finding, ReviewStatus, Severity};
use crate::model::contract::ContractDef;
use crate::model::project::Project;
use crate::narrative::function::build_function_narrative;
use crate::narrative::sequence::{build_sequence_narrative, compute_flow_summary};
use crate::narrative::trace::{build_flow_tree, FlowConfig, FlowTree};
use crate::narrative::types::{FunctionNarrative, SequenceNarrative};
use crate::pathtree::types::PathTree;
use crate::journal::export::export_markdown;
use crate::sequence::analysis::{FunctionBehavior, SequenceAnalysis, TransitionInfo};

use super::session::{ExplorationSession, TraceConfig, VariableSummary};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionEntry {
    pub name: String,
    pub access: AccessLevel,
    pub writes_state: bool,
    pub has_external_calls: bool,
    pub is_read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibleFunctionEntry {
    pub name: String,
    pub access: AccessLevel,
    pub origin: String,
    pub is_inherited: bool,
    pub writes_state: bool,
    pub has_external_calls: bool,
    pub is_read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibleStateVarEntry {
    pub name: String,
    pub type_name: String,
    pub is_constant: bool,
    pub is_immutable: bool,
    pub origin: String,
    pub is_inherited: bool,
}

pub struct AnalysisData<'a> {
    pub project: &'a Project,
    pub contract: &'a ContractDef,
    pub cfgs: &'a HashMap<(String, String), CfgGraph>,
    pub path_trees: &'a HashMap<(String, String), PathTree>,
    pub behaviors: &'a [FunctionBehavior],
    pub transitions: &'a [TransitionInfo],
    pub classifications: &'a [(String, AccessLevel)],
    pub all_sequence_analyses: &'a HashMap<String, SequenceAnalysis>,
    pub all_classifications: &'a HashMap<String, Vec<(String, AccessLevel)>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScenarioAction {
    New { name: String },
    List,
    Switch { name: String },
    Fork {
        name: String,
        #[serde(default)]
        at_step: Option<usize>,
    },
    Delete { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionCommand {
    Call {
        func: String,
        #[serde(default)]
        trace_config: Option<TraceConfig>,
    },
    Back,
    Clear,
    State,
    Functions,
    Finding { severity: Severity, title: String, description: String },
    Note { text: String },
    Status { func: String, status: ReviewStatus },
    Session,
    Who { variable: String },
    Export,
    SaveSession,
    LoadSession { json: String },
    FunctionsAll,
    StateVarsAll,
    Scenario { sub: ScenarioAction },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioInfo {
    pub name: String,
    pub active: bool,
    pub step_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandResult {
    StepAdded {
        step_index: usize,
        function: String,
        access: AccessLevel,
        state_changed: Vec<String>,
    },
    StepRemoved {
        remaining: usize,
    },
    Cleared,
    StateView {
        summary: Vec<VariableSummary>,
    },
    FunctionList {
        functions: Vec<FunctionEntry>,
    },
    FunctionListAll {
        functions: Vec<AccessibleFunctionEntry>,
    },
    StateVarListAll {
        state_vars: Vec<AccessibleStateVarEntry>,
    },
    FindingAdded {
        id: String,
    },
    NoteAdded,
    StatusUpdated,
    SessionView {
        contract: String,
        steps: Vec<String>,
        findings_count: usize,
    },
    VariableInfo {
        variable: String,
        writers: Vec<(String, AccessLevel)>,
        readers: Vec<(String, AccessLevel)>,
    },
    Exported {
        markdown: String,
    },
    SessionSaved {
        json: String,
    },
    SessionLoaded {
        contract: String,
        steps: Vec<String>,
    },
    Error {
        message: String,
    },
    ScenarioList { items: Vec<ScenarioInfo> },
    ScenarioCreated { name: String },
    ScenarioSwitched { from: String, to: String },
    ScenarioForked { from: String, to: String, at_step: usize },
    ScenarioDeleted { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CanvasPatch {
    AddNode { scenario: String, function: String, access: AccessLevel, step_index: usize },
    RemoveLastNode { scenario: String },
    ClearAll { scenario: String },
    Highlight { scenario: String, function: String },
    ScenarioEvent(ScenarioEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScenarioEvent {
    Created { name: String },
    Switched { from: String, to: String },
    Deleted { name: String },
    Forked { from: String, to: String, at_step: usize },
    Reloaded { active: String },
}

pub fn validate_scenario_name(name: &str) -> Result<(), String> {
    const ERR: &str = "Invalid scenario name: must match ^[a-z][a-z0-9_-]{0,31}$";
    if name.is_empty() || name.len() > 32 {
        return Err(ERR.to_string());
    }
    let mut chars = name.chars();
    let first = chars.next().ok_or_else(|| ERR.to_string())?;
    if !first.is_ascii_lowercase() {
        return Err(ERR.to_string());
    }
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') {
            return Err(ERR.to_string());
        }
    }
    Ok(())
}

pub fn canvas_patch_from(result: &CommandResult, active_scenario: &str) -> Option<CanvasPatch> {
    match result {
        CommandResult::StepAdded { function, access, step_index, .. } => {
            Some(CanvasPatch::AddNode {
                scenario: active_scenario.to_string(),
                function: function.clone(),
                access: access.clone(),
                step_index: *step_index,
            })
        }
        CommandResult::StepRemoved { .. } => Some(CanvasPatch::RemoveLastNode {
            scenario: active_scenario.to_string(),
        }),
        CommandResult::Cleared => Some(CanvasPatch::ClearAll {
            scenario: active_scenario.to_string(),
        }),
        CommandResult::ScenarioCreated { name } => {
            Some(CanvasPatch::ScenarioEvent(ScenarioEvent::Created { name: name.clone() }))
        }
        CommandResult::ScenarioSwitched { from, to } => {
            if from == to {
                None
            } else {
                Some(CanvasPatch::ScenarioEvent(ScenarioEvent::Switched {
                    from: from.clone(),
                    to: to.clone(),
                }))
            }
        }
        CommandResult::ScenarioDeleted { name } => {
            Some(CanvasPatch::ScenarioEvent(ScenarioEvent::Deleted { name: name.clone() }))
        }
        CommandResult::ScenarioForked { from, to, at_step } => {
            Some(CanvasPatch::ScenarioEvent(ScenarioEvent::Forked {
                from: from.clone(),
                to: to.clone(),
                at_step: *at_step,
            }))
        }
        CommandResult::SessionLoaded { contract: _, steps: _ } => {
            Some(CanvasPatch::ScenarioEvent(ScenarioEvent::Reloaded {
                active: active_scenario.to_string(),
            }))
        }
        _ => None,
    }
}

pub fn execute_command(
    cmd: SessionCommand,
    session: &mut ExplorationSession,
    data: &AnalysisData,
    timestamp: &str,
) -> CommandResult {
    match cmd {
        SessionCommand::Call { func, trace_config } => {
            execute_call(session, data, &func, trace_config, timestamp)
        }
        SessionCommand::Back => execute_back(session),
        SessionCommand::Clear => { session.clear(); CommandResult::Cleared }
        SessionCommand::State => CommandResult::StateView {
            summary: session.variable_summary(),
        },
        SessionCommand::Functions => {
            let classifications = classify_all(data.contract);
            let functions: Vec<FunctionEntry> = classifications.into_iter().map(|(name, access)| {
                let behavior = data.behaviors.iter().find(|b| b.name == name);
                let writes_state = behavior
                    .map(|b| !b.effective_state_writes().is_empty())
                    .unwrap_or(false);
                let has_external_calls = behavior
                    .map(|b| !b.effective_external_calls().is_empty())
                    .unwrap_or(false);
                let is_read_only = !writes_state && !has_external_calls;
                FunctionEntry {
                    name,
                    access,
                    writes_state,
                    has_external_calls,
                    is_read_only,
                }
            }).collect();
            CommandResult::FunctionList { functions }
        }
        SessionCommand::Finding { severity, title, description } => {
            execute_finding(session, severity, title, description, timestamp)
        }
        SessionCommand::Note { text } => execute_note(session, &text, timestamp),
        SessionCommand::Status { func, status } => {
            execute_status(session, &func, status, timestamp, data)
        }
        SessionCommand::Session => CommandResult::SessionView {
            contract: session.contract.clone(),
            steps: session.current_sequence().into_iter().map(|s| s.to_string()).collect(),
            findings_count: session.journal.findings.len(),
        },
        SessionCommand::Who { variable } => execute_who(data, &variable),
        SessionCommand::Export => {
            let mut journal = session.journal.clone();
            if let Some(name) = data.project.source_files.get(data.contract.span.file_index)
                .and_then(|f| project_root_name(&f.path))
            {
                journal.project = name;
            }
            let md = export_markdown(&journal, data.contract.functions.len());
            CommandResult::Exported { markdown: md }
        }
        SessionCommand::SaveSession | SessionCommand::LoadSession { .. } => CommandResult::Error {
            message: "Save/Load commands must be dispatched at the store level, not through execute_command".into(),
        },
        SessionCommand::FunctionsAll => execute_functions_all(data),
        SessionCommand::StateVarsAll => execute_state_vars_all(data),
        SessionCommand::Scenario { .. } => CommandResult::Error {
            message: "Scenario commands must be dispatched at the store level, not through execute_command".into(),
        },
    }
}

/// Directory name of the Foundry root (nearest ancestor with foundry.toml) of a source file.
pub fn project_root_name(source_path: &str) -> Option<String> {
    std::path::Path::new(source_path)
        .ancestors()
        .skip(1)
        .find(|dir| dir.join("foundry.toml").is_file())
        .and_then(|dir| dir.file_name())
        .map(|name| name.to_string_lossy().into_owned())
}

fn execute_state_vars_all(data: &AnalysisData) -> CommandResult {
    let state_vars: Vec<AccessibleStateVarEntry> = data.project
        .accessible_state_vars(data.contract)
        .into_iter()
        .map(|(sv, origin, is_inherited)| AccessibleStateVarEntry {
            name: sv.name,
            type_name: sv.type_name,
            is_constant: sv.is_constant,
            is_immutable: sv.is_immutable,
            origin,
            is_inherited,
        })
        .collect();

    CommandResult::StateVarListAll { state_vars }
}

fn execute_functions_all(data: &AnalysisData) -> CommandResult {
    use crate::classify::entry_points::classify_function;

    let functions: Vec<AccessibleFunctionEntry> = data.project
        .accessible_functions(data.contract)
        .into_iter()
        .map(|af| {
            let access = if af.is_inherited {
                if let Some(parent) = data.project.contract_index.get(&af.origin).map(|&i| &data.project.contracts[i]) {
                    classify_function(af.function, parent)
                } else {
                    classify_function(af.function, data.contract)
                }
            } else {
                classify_function(af.function, data.contract)
            };

            // inherited functions are analysed as this contract runs them
            let behavior = data.behaviors.iter().find(|b| b.name == af.function.key).or_else(|| {
                data.all_sequence_analyses.get(&af.origin)
                    .and_then(|sa| sa.functions.iter().find(|b| b.name == af.function.key))
            });

            let writes_state = behavior
                .map(|b| !b.effective_state_writes().is_empty())
                .unwrap_or(false);
            let has_external_calls = behavior
                .map(|b| !b.effective_external_calls().is_empty())
                .unwrap_or(false);
            let is_read_only = !writes_state && !has_external_calls;

            AccessibleFunctionEntry {
                name: af.function.name.clone(),
                access,
                origin: af.origin,
                is_inherited: af.is_inherited,
                writes_state,
                has_external_calls,
                is_read_only,
            }
        })
        .collect();

    CommandResult::FunctionListAll { functions }
}

fn execute_call(
    session: &mut ExplorationSession,
    data: &AnalysisData,
    func: &str,
    trace_config: Option<TraceConfig>,
    timestamp: &str,
) -> CommandResult {
    let accessible = data.project.accessible_functions(data.contract);
    // a bare name, a key or a signature; ambiguous names take the first overload
    let resolved = match accessible.iter().find(|a| {
        a.function.name == func || a.function.key == func || a.function.match_signature() == func
    }) {
        Some(af) => af,
        None => return CommandResult::Error {
            message: format!("Function '{}' not found. Use 'functions' or 'funcs-all' to see available.", func),
        },
    };

    let owning_contract = if resolved.is_inherited {
        match data.project.contract_index.get(&resolved.origin).map(|&i| &data.project.contracts[i]) {
            Some(c) => c,
            None => return CommandResult::Error {
                message: format!("Parent contract '{}' not found", resolved.origin),
            },
        }
    } else {
        data.contract
    };

    let key = (owning_contract.key().to_string(), resolved.function.key.clone());
    let cfg = match data.cfgs.get(&key) {
        Some(c) => c,
        None => return CommandResult::Error {
            message: format!("No CFG available for {}::{}", owning_contract.name, func),
        },
    };

    let function_def = resolved.function;

    use crate::model::function::Visibility;
    if matches!(function_def.visibility, Visibility::Internal | Visibility::Private) {
        return CommandResult::Error {
            message: format!(
                "'{}' is {} and cannot be called from outside the contract — \
                 not a valid session entry point. Use `tr {}` to view its flow, \
                 or `c <public_caller>` to trace a real entry point.",
                func,
                match function_def.visibility {
                    Visibility::Internal => "internal",
                    Visibility::Private => "private",
                    _ => unreachable!(),
                },
                func,
            ),
        };
    }

    let access = crate::classify::entry_points::classify_function(function_def, owning_contract);

    let combined_state_vars = data.project.inherited_state_vars(data.contract);
    session.add_step_with_internals(
        function_def,
        cfg,
        &combined_state_vars,
        data.project,
        owning_contract,
        data.cfgs,
        timestamp,
        trace_config.unwrap_or_default(),
    );

    let state_changed: Vec<String> = session.steps.last()
        .map(|s| {
            let mut vars: Vec<String> = s.mutations.iter().map(|m| m.variable.clone()).collect();
            vars.sort();
            vars.dedup();
            vars
        })
        .unwrap_or_default();

    CommandResult::StepAdded {
        step_index: session.steps.len() - 1,
        function: func.to_string(),
        access,
        state_changed,
    }
}

fn execute_back(session: &mut ExplorationSession) -> CommandResult {
    if session.remove_last_step() {
        CommandResult::StepRemoved { remaining: session.steps.len() }
    } else {
        CommandResult::Error { message: "No steps to undo".into() }
    }
}

fn execute_finding(
    session: &mut ExplorationSession,
    severity: Severity,
    title: String,
    description: String,
    timestamp: &str,
) -> CommandResult {
    let affected_sequence = if session.steps.is_empty() {
        None
    } else {
        Some(session.current_sequence().into_iter().map(|s| s.to_string()).collect())
    };

    let finding = Finding {
        id: String::new(),
        severity,
        title,
        affected_function: session.steps.last().map(|s| s.function.clone()).unwrap_or_default(),
        affected_sequence,
        description,
        notes: vec![],
        created_at: String::new(),
    };

    session.journal.add_finding(finding, timestamp);
    let id = session.journal.findings.last().map(|f| f.id.clone()).unwrap_or_default();
    CommandResult::FindingAdded { id }
}

fn execute_note(session: &mut ExplorationSession, text: &str, timestamp: &str) -> CommandResult {
    let anchor = session.current_sequence().join(" → ");
    session.journal.record(crate::journal::types::JournalEntry::NoteAdded {
        anchor,
        content: text.into(),
        timestamp: timestamp.into(),
    });
    CommandResult::NoteAdded
}

fn execute_status(
    session: &mut ExplorationSession,
    func: &str,
    status: ReviewStatus,
    timestamp: &str,
    data: &AnalysisData,
) -> CommandResult {
    if data.project.resolve_function(data.contract, func).is_none() {
        return CommandResult::Error {
            message: format!("Function '{}' not found in {} or its ancestors", func, data.contract.name),
        };
    }
    session.journal.record(crate::journal::types::JournalEntry::StatusChanged {
        function: func.into(),
        status,
        timestamp: timestamp.into(),
    });
    CommandResult::StatusUpdated
}

fn execute_who(data: &AnalysisData, variable: &str) -> CommandResult {
    let var_lower = variable.to_lowercase();

    // the contract's own analysis first: it lists inherited functions as this contract runs them
    let mut contract_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for af in data.project.accessible_functions(data.contract) {
        if af.is_inherited {
            contract_set.insert(af.origin);
        }
    }
    contract_set.remove(data.contract.key());
    let contract_names: Vec<String> =
        std::iter::once(data.contract.key().to_string()).chain(contract_set).collect();

    let accessible = data.project.accessible_functions(data.contract);
    let origin_of = |func_key: &str| -> Option<&str> {
        accessible.iter().find(|af| af.function.key == func_key).map(|af| af.origin.as_str())
    };
    let access_for = |func_name: &str, contract_name: &str| -> AccessLevel {
        data.all_classifications.get(contract_name)
            .and_then(|classifs| classifs.iter().find(|(n, _)| n == func_name).map(|(_, a)| a.clone()))
            .unwrap_or(AccessLevel::Internal)
    };

    let path_matches = |path: &str| -> bool {
        let p = path.to_lowercase();
        if p == var_lower {
            return true;
        }
        if p == format!("{}[]", var_lower) {
            return true;
        }
        if p.ends_with(&format!(".{}", var_lower)) {
            return true;
        }
        let base = p
            .split(|c| c == '[' || c == '.')
            .next()
            .unwrap_or(&p);
        base == var_lower
    };

    let mut writers: Vec<(String, AccessLevel)> = Vec::new();
    let mut readers: Vec<(String, AccessLevel)> = Vec::new();
    let mut seen_writers: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut seen_readers: std::collections::HashSet<String> = std::collections::HashSet::new();

    for contract_name in &contract_names {
        let behaviors = match data.all_sequence_analyses.get(contract_name) {
            Some(sa) => &sa.functions,
            None => continue,
        };

        for b in behaviors {
            let eff_write_paths = b.effective_state_write_paths();
            let eff_read_paths = b.effective_state_read_paths();
            let eff_writes = b.effective_state_writes();
            let eff_reads = b.effective_state_reads();

            let writes = eff_write_paths.iter().any(|p| path_matches(p))
                || eff_writes.iter().any(|w| {
                    let base = w
                        .split(|c| c == '[' || c == '.')
                        .next()
                        .unwrap_or(w)
                        .to_lowercase();
                    base == var_lower
                });
            let reads = eff_read_paths.iter().any(|p| path_matches(p))
                || eff_reads.iter().any(|r| {
                    let base = r
                        .split(|c| c == '[' || c == '.')
                        .next()
                        .unwrap_or(r)
                        .to_lowercase();
                    base == var_lower
                });

            let origin = origin_of(&b.name).unwrap_or(contract_name);
            if writes && seen_writers.insert(b.name.clone()) {
                writers.push((b.name.clone(), access_for(&b.name, origin)));
            }
            if reads && !writes && seen_readers.insert(b.name.clone()) {
                readers.push((b.name.clone(), access_for(&b.name, origin)));
            }
        }
    }

    if writers.is_empty() && readers.is_empty() {
        return CommandResult::Error {
            message: format!("Variable '{}' not found in any function", variable),
        };
    }

    CommandResult::VariableInfo { variable: variable.to_string(), writers, readers }
}

/// Behaviors that describe `func` as `data.contract` runs it: the contract's own analysis
/// lists inherited functions too; the defining contract's is a fallback.
fn behaviors_for<'a>(data: &AnalysisData<'a>, owning: &ContractDef, func: &crate::model::function::FunctionDef) -> &'a [FunctionBehavior] {
    if data.behaviors.iter().any(|b| b.name == func.key) {
        return data.behaviors;
    }
    data.all_sequence_analyses.get(owning.key()).map(|sa| sa.functions.as_slice()).unwrap_or(&[])
}

pub fn get_step_narrative(
    session: &ExplorationSession,
    step_index: usize,
    data: &AnalysisData,
) -> Result<FunctionNarrative, String> {
    let step = session.steps.get(step_index)
        .ok_or_else(|| format!("Step {} not found", step_index))?;

    let (owning, func) = data.project
        .resolve_function(data.contract, &step.function)
        .ok_or_else(|| format!("Function '{}' not found", step.function))?;

    let key = (owning.key().to_string(), func.key.clone());
    let cfg = data.cfgs.get(&key).ok_or("No CFG")?;
    let pt = data.path_trees.get(&key).ok_or("No paths")?;

    let behaviors = behaviors_for(data, owning, func);

    Ok(build_function_narrative(owning, func, pt, cfg, behaviors))
}

pub fn get_function_info(
    func_name: &str,
    data: &AnalysisData,
) -> Result<FunctionNarrative, String> {
    let (owning, func) = data.project
        .resolve_function(data.contract, func_name)
        .ok_or_else(|| format!("Function '{}' not found", func_name))?;

    let key = (owning.key().to_string(), func.key.clone());
    let cfg = data.cfgs.get(&key).ok_or_else(|| format!("No CFG for {}::{}", owning.name, func_name))?;
    let pt = data.path_trees.get(&key).ok_or("No paths")?;

    let behaviors = behaviors_for(data, owning, func);

    Ok(build_function_narrative(owning, func, pt, cfg, behaviors))
}

pub fn get_flow_tree(
    func_name: &str,
    data: &AnalysisData,
    max_depth: usize,
    include_reverts: bool,
    expand_set: std::collections::HashSet<usize>,
) -> Result<FlowTree, String> {
    let (owning, func) = data.project
        .resolve_function(data.contract, func_name)
        .ok_or_else(|| format!("Function '{}' not found", func_name))?;

    let key = (owning.key().to_string(), func.key.clone());
    let cfg = data.cfgs.get(&key)
        .ok_or_else(|| format!("No CFG for {}::{}", owning.name, func_name))?;

    let config = FlowConfig {
        max_depth,
        include_reverts,
        expand_set,
    };
    Ok(build_flow_tree(owning, func, cfg, data.project, data.cfgs, &config))
}

pub fn get_sequence_narrative(
    session: &ExplorationSession,
    data: &AnalysisData,
) -> Result<SequenceNarrative, String> {
    if session.steps.len() < 2 {
        return Err("Need at least 2 steps for a sequence narrative".into());
    }

    let names: Vec<&str> = session.current_sequence();
    let mut narrative = build_sequence_narrative(
        &session.contract, &names,
        data.behaviors, data.transitions, data.classifications,
    );

    for (i, step) in narrative.steps.iter_mut().enumerate() {
        if let Some(session_step) = session.steps.get(i) {
            step.flow_summary = session_step.flow_tree.as_ref()
                .map(|tree| compute_flow_summary(tree, i));
        }
    }

    Ok(narrative)
}

pub fn get_session_state(session: &ExplorationSession) -> Vec<VariableSummary> {
    session.variable_summary()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_root_name_is_the_foundry_root_directory() {
        let file = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/solc/cross/src/Vault.sol");
        assert_eq!(project_root_name(file).as_deref(), Some("cross"));
        assert_eq!(project_root_name("/no/such/dir/src/A.sol"), None);
    }

    #[test]
    fn new_session_records_its_start_time() {
        let s = ExplorationSession::new("Vault", "p");
        assert!(s.journal.started_at.parse::<u64>().is_ok_and(|t| t > 1_700_000_000), "{}", s.journal.started_at);
    }
}
