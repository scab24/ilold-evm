use std::collections::HashSet;

use crate::cfg::types::{BlockKind, BranchEdge, CfgGraph, CfgStatement};
use crate::classify::entry_points::access_level;
use crate::classify::guards::function_guards;
use crate::model::contract::ContractDef;
use crate::model::function::FunctionDef;
use crate::narrative::types::TransitiveEffect;
use crate::pathtree::types::{PathNode, PathTree, TerminalKind};
use crate::sequence::analysis::{find_behavior, EffectKind, FunctionBehavior};

use super::types::*;

pub fn build_function_narrative(
    contract: &ContractDef,
    function: &FunctionDef,
    path_tree: &PathTree,
    cfg: &CfgGraph,
    all_behaviors: &[FunctionBehavior],
) -> FunctionNarrative {
    build_function_narrative_capped(contract, function, path_tree, cfg, all_behaviors, usize::MAX)
}

/// `paths_omitted` counts the paths not narrated.
pub fn build_function_narrative_capped(
    contract: &ContractDef,
    function: &FunctionDef,
    path_tree: &PathTree,
    cfg: &CfgGraph,
    all_behaviors: &[FunctionBehavior],
    max_paths: usize,
) -> FunctionNarrative {
    let root_behavior = find_behavior(all_behaviors, contract.key(), &function.key);
    // guards come from the analysis, which reads inherited modifiers and helpers
    let (access, guards) = match root_behavior {
        Some(b) => (access_level(function, &b.guards), b.guards.clone()),
        None => {
            let guards = function_guards(None, contract, contract, function);
            (access_level(function, &guards), guards)
        }
    };

    let mut routes = path_tree.routes();
    let paths: Vec<PathNarrative> = routes
        .by_ref()
        .take(max_paths)
        .map(|(id, terminal, nodes)| {
            let mut steps = Vec::new();

            for node in nodes {
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
                id,
                terminal,
                steps,
            }
        })
        .collect();
    let paths_omitted = routes.count();

    let unguarded = access.is_unrestricted() && !guards.iter().any(|g| g.restricts());
    let observations = generate_observations(unguarded, path_tree, cfg, all_behaviors, root_behavior);

    let mut writes = HashSet::new();
    let mut reads = HashSet::new();
    let mut calls = HashSet::new();
    let mut internal = HashSet::new();
    let mut events_set = HashSet::new();
    for a in path_tree.block_annotations() {
        for w in &a.state_writes { writes.insert(w.clone()); }
        for r in &a.state_reads { reads.insert(r.clone()); }
        for c in &a.external_calls {
            calls.insert(format!("{}.{}", c.target, c.function));
        }
        for ic in &a.internal_calls {
            internal.insert(ic.clone());
        }
        for ev in &a.events_emitted {
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

    let traced = |kind: EffectKind| -> Vec<TransitiveEffect> {
        root_behavior.map(|b| transitive_effects(b, kind)).unwrap_or_default()
    };
    let mut revert_sources: Vec<TransitiveEffect> = root_behavior
        .map(|b| b.revert_sources.iter().map(|r| TransitiveEffect {
            via: Vec::new(),
            item: r.clone(),
            origin_contract: contract.key().to_string(),
        }).collect())
        .unwrap_or_default();
    revert_sources.extend(traced(EffectKind::Revert));

    FunctionNarrative {
        contract: contract.key().to_string(),
        name: function.name.clone(),
        access,
        total_paths: path_tree.stats.total_paths,
        happy_paths: path_tree.stats.happy_paths,
        revert_paths: path_tree.stats.revert_paths,
        path_stats: path_tree.stats.clone(),
        paths_omitted,
        paths,
        observations,
        state_writes: writes,
        state_reads: reads,
        external_calls: calls,
        internal_calls: internal,
        modifiers: function.modifiers.iter().map(|m| m.name.clone()).collect(),
        events,
        transitive_state_writes: traced(EffectKind::StateWrite),
        transitive_state_reads: traced(EffectKind::StateRead),
        transitive_external_calls: traced(EffectKind::ExternalCall),
        transitive_events: traced(EffectKind::Event),
        can_revert: root_behavior.map_or(path_tree.stats.revert_paths > 0, |b| b.can_revert),
        revert_sources,
        guards,
        accesses: Vec::new(),
    }
}

// the origin is the contract of the last callee
fn transitive_effects(behavior: &FunctionBehavior, kind: EffectKind) -> Vec<TransitiveEffect> {
    behavior
        .transitive_effects
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| TransitiveEffect {
            via: e.via.clone(),
            item: e.item.clone(),
            origin_contract: e
                .via
                .last()
                .and_then(|callee| callee.split_once("::"))
                .map(|(contract, _)| contract.to_string())
                .unwrap_or_default(),
        })
        .collect()
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
        CfgStatement::UnresolvedModifier { name, .. } => Some(NarrativeStep {
            step_type: StepType::InternalCall,
            description: format!("modifier {name} (unresolved, not inlined)"),
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

fn check_cei_violation<'a>(
    nodes: impl Iterator<Item = &'a PathNode>,
    cfg: &CfgGraph,
) -> Option<(String, String)> {
    let mut seen_external_call: Option<String> = None;

    for node in nodes {
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
                CfgStatement::Assignment { target, storage: Some(_), .. } => {
                    if let Some(call) = &seen_external_call {
                        return Some((call.clone(), target.clone()));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn generate_observations(
    unguarded: bool,
    path_tree: &PathTree,
    cfg: &CfgGraph,
    all_behaviors: &[FunctionBehavior],
    this: Option<&FunctionBehavior>,
) -> Vec<Observation> {
    let mut obs = Vec::new();

    for (id, terminal, nodes) in path_tree.routes() {
        if terminal == TerminalKind::Revert { continue; }

        if let Some((call, write)) = check_cei_violation(nodes, cfg) {
            obs.push(Observation {
                kind: ObservationKind::WriteAfterExternalCall,
                description: format!(
                    "Path #{}: external call {} then writes {}",
                    id, call, write,
                ),
            });
            break;
        }
    }

    let my_writes: HashSet<&str> = path_tree
        .block_annotations()
        .flat_map(|a| a.state_writes.iter().map(|s| s.as_str()))
        .collect();

    for behavior in all_behaviors {
        if this.is_some_and(|this| std::ptr::eq(this, behavior)) { continue; }
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

    if unguarded && !my_writes.is_empty() {
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
