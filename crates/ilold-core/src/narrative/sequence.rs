use crate::classify::entry_points::AccessLevel;
use crate::sequence::analysis::{FunctionBehavior, TransitionInfo};

use super::trace::{FlowKind, FlowNode, FlowTree};
use super::types::*;

pub fn build_sequence_narrative(
    contract_name: &str,
    function_names: &[&str],
    behaviors: &[FunctionBehavior],
    transitions: &[TransitionInfo],
    classifications: &[(String, AccessLevel)],
) -> SequenceNarrative {
    let mut steps = Vec::new();
    let mut observations = Vec::new();

    for (i, func_name) in function_names.iter().enumerate() {
        let behavior = behaviors.iter().find(|b| b.name == *func_name);
        let access = classifications
            .iter()
            .find(|(name, _)| name == func_name)
            .map(|(_, a)| a.clone())
            .unwrap_or(AccessLevel::Public);

        let (requires, effects, ext_calls, events) = match behavior {
            Some(b) => (
                b.preconditions.clone(),
                b.state_writes.clone(),
                b.external_calls.clone(),
                b.events.clone(),
            ),
            None => (vec![], vec![], vec![], vec![]),
        };

        let mut deps = Vec::new();

        for prev_idx in 0..i {
            let prev_name = function_names[prev_idx];
            let transition = transitions
                .iter()
                .find(|t| t.from == prev_name && t.to == *func_name);

            if let Some(t) = transition {
                for cond in &t.conditions_affected {
                    deps.push(Dependency {
                        from_step: prev_idx,
                        variable: extract_variable_from_condition(cond),
                        relationship: cond.clone(),
                    });
                }
            }
        }

        steps.push(SequenceStep {
            function: func_name.to_string(),
            access,
            requires,
            effects,
            external_calls: ext_calls,
            events,
            dependencies: deps,
            flow_summary: None,
        });
    }

    for i in 0..steps.len() {
        if steps[i].external_calls.is_empty() { continue; }
        for j in (i + 1)..steps.len() {
            if steps[j].dependencies.iter().any(|d| d.from_step == i) {
                observations.push(Observation {
                    kind: ObservationKind::SharedState,
                    description: format!(
                        "Step {} ({}) makes external calls, step {} ({}) depends on its state — cross-step interaction",
                        i + 1, steps[i].function,
                        j + 1, steps[j].function,
                    ),
                });
            }
        }
    }

    SequenceNarrative {
        contract: contract_name.to_string(),
        steps,
        observations,
    }
}

fn extract_variable_from_condition(cond: &str) -> String {
    if let Some(start) = cond.find('\'') {
        if let Some(end) = cond[start + 1..].find('\'') {
            return cond[start + 1..start + 1 + end].to_string();
        }
    }
    String::new()
}

pub fn compute_flow_summary(tree: &FlowTree, session_step_index: usize) -> FlowSummary {
    let mut summary = FlowSummary {
        total_steps: 0,
        mutation_count: 0,
        external_call_count: 0,
        internal_call_count: 0,
        depth_limited_count: 0,
        mutation_refs: Vec::new(),
    };
    walk_for_summary(&tree.root, session_step_index, &mut summary);
    summary
}

fn walk_for_summary(node: &FlowNode, session_step_index: usize, summary: &mut FlowSummary) {
    summary.total_steps += 1;
    match &node.kind {
        FlowKind::Write { target, .. } => {
            summary.mutation_count += 1;
            summary.mutation_refs.push(MutationRef {
                variable: target.clone(),
                flow_step_id: node.step_id,
                session_step_index,
            });
        }
        FlowKind::StateWrite { variable } => {
            summary.mutation_count += 1;
            summary.mutation_refs.push(MutationRef {
                variable: variable.clone(),
                flow_step_id: node.step_id,
                session_step_index,
            });
        }
        FlowKind::ExternalCall { .. } | FlowKind::EthTransfer { .. } => {
            summary.external_call_count += 1;
        }
        FlowKind::InternalCall { depth_limited, .. } => {
            summary.internal_call_count += 1;
            if *depth_limited {
                summary.depth_limited_count += 1;
            }
        }
        _ => {}
    }
    for child in &node.children {
        walk_for_summary(child, session_step_index, summary);
    }
}
