use std::collections::{HashMap, HashSet};

use crate::cfg::types::{BlockKind, BranchEdge, CfgGraph, CfgStatement};
use crate::classify::entry_points::{classify_function, AccessLevel};
use crate::model::contract::ContractDef;
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::narrative::types::TransitiveEffect;
use crate::pathtree::types::{PathTree, TerminalKind};
use crate::sequence::analysis::{FunctionBehavior, SequenceAnalysis};

use super::types::*;

pub fn build_function_narrative(
    contract: &ContractDef,
    function: &FunctionDef,
    path_tree: &PathTree,
    cfg: &CfgGraph,
    all_behaviors: &[FunctionBehavior],
    project: &Project,
    all_sequence_analyses: &HashMap<String, SequenceAnalysis>,
) -> FunctionNarrative {
    let access = classify_function(function, contract);

    let paths: Vec<PathNarrative> = path_tree
        .paths
        .iter()
        .map(|path| {
            let mut steps = Vec::new();

            for node in &path.nodes {
                let block = cfg
                    .node_indices()
                    .find(|&n| cfg[n].id == node.block_id)
                    .map(|n| &cfg[n]);

                let block = match block {
                    Some(b) => b,
                    None => continue,
                };

                let branch = branch_direction(&node.branch_taken);

                match block.kind {
                    BlockKind::Entry => {
                        steps.push(NarrativeStep {
                            step_type: StepType::Entry,
                            description: format!("{}()", function.name),
                            detail: None,
                            branch: None,
                        });
                    }
                    BlockKind::Return => {
                        steps.push(NarrativeStep {
                            step_type: StepType::Return,
                            description: "return".into(),
                            detail: None,
                            branch,
                        });
                    }
                    BlockKind::Revert => {
                        steps.push(NarrativeStep {
                            step_type: StepType::Revert,
                            description: "revert".into(),
                            detail: None,
                            branch,
                        });
                    }
                    _ => {
                        for stmt in &block.statements {
                            if let Some(step) = statement_to_step(stmt, branch) {
                                steps.push(step);
                            }
                        }
                    }
                }
            }

            PathNarrative {
                id: path.id,
                terminal: path.terminal,
                steps,
            }
        })
        .collect();

    let observations = generate_observations(
        &access, path_tree, cfg, all_behaviors, &function.name, &contract.state_vars,
    );

    let mut writes = HashSet::new();
    let mut reads = HashSet::new();
    let mut calls = HashSet::new();
    let mut internal = HashSet::new();
    let mut events_set = HashSet::new();
    for p in &path_tree.paths {
        for w in &p.annotations.state_writes { writes.insert(w.clone()); }
        for r in &p.annotations.state_reads { reads.insert(r.clone()); }
        for c in &p.annotations.external_calls {
            calls.insert(format!("{}.{}", c.target, c.function));
        }
        for ic in &p.annotations.internal_calls {
            internal.insert(ic.clone());
        }
        for ev in &p.annotations.events_emitted {
            events_set.insert(ev.clone());
        }
    }

    let mut writes: Vec<String> = writes.into_iter().collect();
    writes.sort();
    let mut reads: Vec<String> = reads.into_iter().collect();
    reads.sort();
    let mut calls: Vec<String> = calls.into_iter().collect();
    calls.sort();
    let mut internal: Vec<String> = internal.into_iter().collect();
    internal.sort();
    let mut events: Vec<String> = events_set.into_iter().collect();
    events.sort();

    let root_behavior = all_behaviors.iter().find(|b| b.name == function.name);

    let (transitive_writes, transitive_reads, transitive_external, transitive_events) =
        if let Some(root) = root_behavior {
            collect_transitive_effects(
                root,
                contract,
                project,
                all_sequence_analyses,
                &writes,
                &reads,
                &calls,
            )
        } else {
            (Vec::new(), Vec::new(), Vec::new(), Vec::new())
        };

    FunctionNarrative {
        contract: contract.key().to_string(),
        name: function.name.clone(),
        access,
        total_paths: path_tree.stats.total_paths,
        happy_paths: path_tree.stats.happy_paths,
        revert_paths: path_tree.stats.revert_paths,
        paths,
        observations,
        state_writes: writes,
        state_reads: reads,
        external_calls: calls,
        internal_calls: internal,
        modifiers: function.modifiers.iter().map(|m| m.name.clone()).collect(),
        events,
        transitive_state_writes: transitive_writes,
        transitive_state_reads: transitive_reads,
        transitive_external_calls: transitive_external,
        transitive_events,
    }
}

fn collect_transitive_effects(
    root: &FunctionBehavior,
    _root_contract: &ContractDef,
    _project: &Project,
    _all: &HashMap<String, SequenceAnalysis>,
    _direct_writes: &[String],
    _direct_reads: &[String],
    _direct_external: &[String],
) -> (Vec<TransitiveEffect>, Vec<TransitiveEffect>, Vec<TransitiveEffect>, Vec<TransitiveEffect>) {
    fn map_to_effects(items: &[String]) -> Vec<TransitiveEffect> {
        items
            .iter()
            .map(|item| TransitiveEffect {
                via: vec!["(transitive)".to_string()],
                item: item.clone(),
                origin_contract: String::new(),
            })
            .collect()
    }

    (
        map_to_effects(&root.transitive_state_writes),
        map_to_effects(&root.transitive_state_reads),
        map_to_effects(&root.transitive_external_calls),
        map_to_effects(&root.transitive_events),
    )
}

fn statement_to_step(stmt: &CfgStatement, branch: Option<BranchDirection>) -> Option<NarrativeStep> {
    match stmt {
        CfgStatement::RequireCheck { condition, .. } => Some(NarrativeStep {
            step_type: StepType::Condition,
            description: format!("require({})", condition),
            detail: None,
            branch,
        }),
        CfgStatement::AssertCheck { condition, .. } => Some(NarrativeStep {
            step_type: StepType::Condition,
            description: format!("assert({})", condition),
            detail: None,
            branch,
        }),
        CfgStatement::ExternalCall { target, function, .. } => Some(NarrativeStep {
            step_type: StepType::ExternalCall,
            description: format!("{}.{}()", target, function),
            detail: None,
            branch: None,
        }),
        CfgStatement::InternalCall { function, .. } => Some(NarrativeStep {
            step_type: StepType::InternalCall,
            description: format!("{}()", function),
            detail: None,
            branch: None,
        }),
        CfgStatement::StateWrite { variable, .. } => Some(NarrativeStep {
            step_type: StepType::StateWrite,
            description: variable.clone(),
            detail: None,
            branch: None,
        }),
        CfgStatement::StateRead { variable, .. } => Some(NarrativeStep {
            step_type: StepType::StateRead,
            description: variable.clone(),
            detail: None,
            branch: None,
        }),
        CfgStatement::EthTransfer { to, .. } => Some(NarrativeStep {
            step_type: StepType::EthTransfer,
            description: format!("transfer ETH to {}", to),
            detail: None,
            branch: None,
        }),
        CfgStatement::EmitEvent { event, .. } => Some(NarrativeStep {
            step_type: StepType::Event,
            description: format!("emit {}", event),
            detail: None,
            branch: None,
        }),
        CfgStatement::AssemblyBlock { .. } => Some(NarrativeStep {
            step_type: StepType::Assembly,
            description: "assembly block".into(),
            detail: None,
            branch: None,
        }),
        CfgStatement::Assignment { .. } => None,
    }
}

fn branch_direction(edge: &Option<BranchEdge>) -> Option<BranchDirection> {
    match edge {
        Some(BranchEdge::ConditionalTrue { .. }) => Some(BranchDirection::True),
        Some(BranchEdge::ConditionalFalse { .. }) => Some(BranchDirection::False),
        _ => None,
    }
}

fn check_cei_violation(
    path: &crate::pathtree::types::ExecutionPath,
    cfg: &CfgGraph,
    state_vars: &[crate::model::common::StateVar],
) -> Option<(String, String)> {
    let mut seen_external_call: Option<String> = None;

    for node in &path.nodes {
        let block = cfg
            .node_indices()
            .find(|&n| cfg[n].id == node.block_id)
            .map(|n| &cfg[n]);

        let block = match block {
            Some(b) => b,
            None => continue,
        };

        for stmt in &block.statements {
            match stmt {
                CfgStatement::ExternalCall { target, function, .. } => {
                    if seen_external_call.is_none() {
                        seen_external_call = Some(format!("{}.{}", target, function));
                    }
                }
                CfgStatement::StateWrite { variable, .. } => {
                    if let Some(call) = &seen_external_call {
                        return Some((call.clone(), variable.clone()));
                    }
                }
                CfgStatement::Assignment { target, .. } => {
                    let base = crate::util::target_base_name(target);
                    if state_vars.iter().any(|sv| sv.name == base) {
                        if let Some(call) = &seen_external_call {
                            return Some((call.clone(), target.clone()));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn generate_observations(
    access: &AccessLevel,
    path_tree: &PathTree,
    cfg: &CfgGraph,
    all_behaviors: &[FunctionBehavior],
    function_name: &str,
    state_vars: &[crate::model::common::StateVar],
) -> Vec<Observation> {
    let mut obs = Vec::new();

    for path in &path_tree.paths {
        if path.terminal == TerminalKind::Revert { continue; }

        if let Some((call, write)) = check_cei_violation(path, cfg, state_vars) {
            obs.push(Observation {
                kind: ObservationKind::WriteAfterExternalCall,
                description: format!(
                    "Path #{}: external call {} then writes {}",
                    path.id, call, write,
                ),
            });
            break;
        }
    }

    let my_writes: HashSet<&str> = path_tree
        .paths
        .iter()
        .flat_map(|p| p.annotations.state_writes.iter().map(|s| s.as_str()))
        .collect();

    for behavior in all_behaviors {
        if behavior.name == function_name { continue; }
        let shared: Vec<&str> = behavior
            .state_reads
            .iter()
            .filter(|r| my_writes.contains(r.as_str()))
            .map(|s| s.as_str())
            .collect();
        if !shared.is_empty() {
            obs.push(Observation {
                kind: ObservationKind::SharedState,
                description: format!(
                    "{} reads {} which this function writes",
                    behavior.name,
                    shared.join(", "),
                ),
            });
        }
    }

    if access.is_unrestricted() && !my_writes.is_empty() {
        obs.push(Observation {
            kind: ObservationKind::NoAccessControl,
            description: format!(
                "Public function writes state ({}) without access control",
                my_writes.into_iter().collect::<Vec<_>>().join(", "),
            ),
        });
    }

    obs
}
