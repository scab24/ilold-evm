
use serde::{Deserialize, Serialize};

use crate::model::expression::AssignOperator;
use crate::narrative::trace::collect_path_conditions;
use crate::util::target_base_name;

use super::session::{ExplorationSession, MutationScope};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableTimeline {
    pub variable: String,
    pub state_entries: Vec<TimelineEntry>,
    pub local_entries: Vec<TimelineEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub session_step_index: usize,
    pub function: String,
    pub flow_step_id: Option<usize>,
    pub target: String,
    pub operator: AssignOperator,
    pub value_expr: String,
    pub reached_when: Vec<String>,
    pub via: Option<String>,
    pub scope: MutationScope,
}

pub fn build_variable_timeline(
    session: &ExplorationSession,
    variable: &str,
) -> VariableTimeline {
    let mut state_entries: Vec<TimelineEntry> = Vec::new();
    let mut local_entries: Vec<TimelineEntry> = Vec::new();

    for (idx, step) in session.steps.iter().enumerate() {
        for mutation in &step.mutations {
            if !matches_variable(&mutation.variable, variable) {
                continue;
            }

            let reached_when = match (mutation.flow_step_id, &step.flow_tree) {
                (Some(flow_id), Some(tree)) => {
                    collect_path_conditions(tree, flow_id).unwrap_or_default()
                }
                _ => Vec::new(),
            };

            let entry = TimelineEntry {
                session_step_index: idx,
                function: step.function.clone(),
                flow_step_id: mutation.flow_step_id,
                target: mutation.variable.clone(),
                operator: mutation.operator,
                value_expr: mutation.value_expr.clone(),
                reached_when,
                via: mutation.via.clone(),
                scope: mutation.scope,
            };

            match mutation.scope {
                MutationScope::State => state_entries.push(entry),
                MutationScope::Local => local_entries.push(entry),
            }
        }
    }

    let sort_key = |e: &TimelineEntry| (e.session_step_index, e.flow_step_id.unwrap_or(usize::MAX));
    state_entries.sort_by_key(sort_key);
    local_entries.sort_by_key(sort_key);

    VariableTimeline {
        variable: variable.to_string(),
        state_entries,
        local_entries,
    }
}

fn matches_variable(target: &str, query: &str) -> bool {
    target_base_name(target) == query
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exploration::session::{ExplorationStep, StateMutation, TraceConfig};

    fn make_mutation(var: &str, op: AssignOperator, val: &str, flow_id: Option<usize>) -> StateMutation {
        StateMutation {
            variable: var.into(),
            operator: op,
            value_expr: val.into(),
            step_index: 0,
            via: None,
            flow_step_id: flow_id,
            scope: MutationScope::State,
        }
    }

    fn make_step(func: &str, mutations: Vec<StateMutation>) -> ExplorationStep {
        ExplorationStep {
            function: func.into(),
            mutations,
            flow_tree: None,
            trace_config: TraceConfig::default(),
        }
    }

    #[test]
    fn timeline_collects_state_writes_in_session_order() {
        let mut session = ExplorationSession::new("C", "p");
        session.steps.push(make_step("deposit", vec![
            make_mutation("balances[user]", AssignOperator::AddAssign, "amount", Some(7)),
            make_mutation("totalStaked", AssignOperator::AddAssign, "amount", Some(8)),
        ]));
        session.steps.push(make_step("withdraw", vec![
            make_mutation("balances[user]", AssignOperator::SubAssign, "amount", Some(14)),
        ]));

        let tl = build_variable_timeline(&session, "balances");
        assert_eq!(tl.variable, "balances");
        assert_eq!(tl.state_entries.len(), 2);
        assert!(tl.local_entries.is_empty());

        assert_eq!(tl.state_entries[0].session_step_index, 0);
        assert_eq!(tl.state_entries[0].function, "deposit");
        assert_eq!(tl.state_entries[0].target, "balances[user]");
        assert_eq!(tl.state_entries[1].session_step_index, 1);
        assert_eq!(tl.state_entries[1].function, "withdraw");
    }

    #[test]
    fn timeline_matches_by_base_name() {
        let mut session = ExplorationSession::new("C", "p");
        session.steps.push(make_step("f", vec![
            make_mutation("balances[a]", AssignOperator::Assign, "1", None),
            make_mutation("balancesOther", AssignOperator::Assign, "2", None),
            make_mutation("config.fee", AssignOperator::Assign, "3", None),
        ]));

        let tl = build_variable_timeline(&session, "balances");
        assert_eq!(tl.state_entries.len(), 1);
        assert_eq!(tl.state_entries[0].target, "balances[a]");

        let tl_config = build_variable_timeline(&session, "config");
        assert_eq!(tl_config.state_entries.len(), 1);
        assert_eq!(tl_config.state_entries[0].target, "config.fee");
    }

    #[test]
    fn timeline_returns_empty_when_no_matches() {
        let mut session = ExplorationSession::new("C", "p");
        session.steps.push(make_step("f", vec![
            make_mutation("balances[a]", AssignOperator::Assign, "1", None),
        ]));

        let tl = build_variable_timeline(&session, "totalSupply");
        assert!(tl.state_entries.is_empty());
        assert!(tl.local_entries.is_empty());
    }

    #[test]
    fn timeline_legacy_mutation_has_empty_reached_when() {
        let mut session = ExplorationSession::new("C", "p");
        session.steps.push(make_step("f", vec![
            make_mutation("balances", AssignOperator::Assign, "1", None),
        ]));

        let tl = build_variable_timeline(&session, "balances");
        assert_eq!(tl.state_entries.len(), 1);
        assert!(tl.state_entries[0].reached_when.is_empty());
        assert!(tl.state_entries[0].flow_step_id.is_none());
    }
}
