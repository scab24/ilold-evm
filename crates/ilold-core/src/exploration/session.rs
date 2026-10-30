use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::cfg::types::CfgGraph;
use crate::journal::types::AuditJournal;
use crate::model::common::StateVar;
use crate::model::contract::ContractDef;
use crate::model::expression::AssignOperator;
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::narrative::trace::{build_flow_tree_with_mutations, FlowConfig, FlowTree};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationSession {
    pub contract: String,
    pub steps: Vec<ExplorationStep>,
    pub journal: AuditJournal,
    #[serde(default)]
    pub forked_from: Option<ForkOrigin>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForkOrigin {
    pub scenario: String,
    pub at_step: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationStep {
    pub function: String,
    pub mutations: Vec<StateMutation>,
    #[serde(default)]
    pub flow_tree: Option<FlowTree>,
    #[serde(default)]
    pub trace_config: TraceConfig,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum MutationScope {
    #[default]
    State,
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMutation {
    pub variable: String,
    pub operator: AssignOperator,
    pub value_expr: String,
    pub step_index: usize,
    #[serde(default)]
    pub via: Option<String>,
    #[serde(default)]
    pub flow_step_id: Option<usize>,
    #[serde(default)]
    pub scope: MutationScope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceConfig {
    #[serde(default = "default_trace_depth")]
    pub depth: usize,
    #[serde(default)]
    pub include_reverts: bool,
    #[serde(default)]
    pub expand_set: Vec<usize>,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            depth: 2,
            include_reverts: false,
            expand_set: Vec::new(),
        }
    }
}

fn default_trace_depth() -> usize { 2 }

fn unix_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

impl ExplorationSession {
    pub fn new(contract: &str, project: &str) -> Self {
        ExplorationSession {
            contract: contract.into(),
            steps: Vec::new(),
            journal: AuditJournal::new(project, contract, &unix_now()),
            forked_from: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_step_with_internals(
        &mut self,
        function: &FunctionDef,
        cfg: &CfgGraph,
        state_vars: &[StateVar],
        project: &Project,
        owning_contract: &ContractDef,
        all_cfgs: &HashMap<(String, String), CfgGraph>,
        timestamp: &str,
        trace_config: TraceConfig,
    ) -> &ExplorationStep {
        let step_index = self.steps.len();

        let flow_config = FlowConfig {
            max_depth: trace_config.depth,
            include_reverts: trace_config.include_reverts,
            expand_set: trace_config.expand_set.iter().copied().collect(),
        };

        let (flow_tree, raw_mutations) = build_flow_tree_with_mutations(
            owning_contract,
            function,
            cfg,
            project,
            all_cfgs,
            &flow_config,
        );

        let mutations: Vec<StateMutation> = raw_mutations
            .into_iter()
            .filter_map(|fm| {
                let base = crate::util::target_base_name(&fm.target);
                if !state_vars.iter().any(|sv| sv.name == base) {
                    return None;
                }
                Some(StateMutation {
                    variable: fm.target,
                    operator: fm.operator,
                    value_expr: fm.value,
                    step_index,
                    via: fm.via,
                    flow_step_id: Some(fm.flow_step_id),
                    scope: MutationScope::State,
                })
            })
            .collect();

        self.steps.push(ExplorationStep {
            function: function.name.clone(),
            mutations,
            flow_tree: Some(flow_tree),
            trace_config,
        });

        self.journal.record(crate::journal::types::JournalEntry::SequenceExplored {
            steps: self.steps.iter().map(|s| s.function.clone()).collect(),
            timestamp: timestamp.into(),
        });

        self.steps.last().unwrap()
    }

    pub fn remove_last_step(&mut self) -> bool {
        self.steps.pop().is_some()
    }

    pub fn clear(&mut self) {
        self.steps.clear();
    }

    pub fn current_sequence(&self) -> Vec<&str> {
        self.steps.iter().map(|s| s.function.as_str()).collect()
    }

    fn variable_history(&self) -> HashMap<String, Vec<&StateMutation>> {
        let mut history: HashMap<String, Vec<&StateMutation>> = HashMap::new();
        for step in &self.steps {
            for mutation in &step.mutations {
                history.entry(mutation.variable.clone())
                    .or_default()
                    .push(mutation);
            }
        }
        history
    }

    pub fn variable_summary(&self) -> Vec<VariableSummary> {
        let history = self.variable_history();
        let mut summaries: Vec<VariableSummary> = history.into_iter().map(|(var, muts)| {
            let changes: Vec<String> = muts.iter().map(|m| {
                let op_str = m.operator.as_str();
                let func = self.steps.get(m.step_index)
                    .map(|s| s.function.as_str())
                    .unwrap_or("?");
                let step_ref = match m.flow_step_id {
                    Some(id) => format!("step {}:{}", m.step_index, id),
                    None => format!("step {}", m.step_index),
                };
                let suffix = match &m.via {
                    Some(chain) => format!(" via {}", chain),
                    None => String::new(),
                };
                format!("{} {} ({}, {}{})", op_str, m.value_expr, step_ref, func, suffix)
            }).collect();

            VariableSummary { variable: var, changes }
        }).collect();

        summaries.sort_by(|a, b| a.variable.cmp(&b.variable));
        summaries
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableSummary {
    pub variable: String,
    pub changes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_is_empty() {
        let s = ExplorationSession::new("Staking", "myproject");
        assert_eq!(s.contract, "Staking");
        assert!(s.steps.is_empty());
        assert_eq!(s.current_sequence().len(), 0);
    }

    #[test]
    fn remove_last_step_empty() {
        let mut s = ExplorationSession::new("Staking", "myproject");
        assert!(!s.remove_last_step());
    }

    #[test]
    fn clear_resets() {
        let mut s = ExplorationSession::new("Staking", "myproject");
        s.steps.push(ExplorationStep {
            function: "deposit".into(),
            mutations: vec![],
            flow_tree: None,
            trace_config: TraceConfig::default(),
        });
        s.steps.push(ExplorationStep {
            function: "withdraw".into(),
            mutations: vec![],
            flow_tree: None,
            trace_config: TraceConfig::default(),
        });
        assert_eq!(s.current_sequence(), vec!["deposit", "withdraw"]);
        s.clear();
        assert!(s.steps.is_empty());
    }

    #[test]
    fn variable_summary_computes() {
        let mut s = ExplorationSession::new("Staking", "myproject");
        s.steps.push(ExplorationStep {
            function: "deposit".into(),
            mutations: vec![
                StateMutation {
                    variable: "balances".into(),
                    operator: AssignOperator::AddAssign,
                    value_expr: "amount".into(),
                    step_index: 0,
                    via: None,
                    flow_step_id: None,
                    scope: MutationScope::State,
                },
                StateMutation {
                    variable: "totalStaked".into(),
                    operator: AssignOperator::AddAssign,
                    value_expr: "amount".into(),
                    step_index: 0,
                    via: None,
                    flow_step_id: None,
                    scope: MutationScope::State,
                },
            ],
            flow_tree: None,
            trace_config: TraceConfig::default(),
        });
        s.steps.push(ExplorationStep {
            function: "withdraw".into(),
            mutations: vec![
                StateMutation {
                    variable: "balances".into(),
                    operator: AssignOperator::SubAssign,
                    value_expr: "amount".into(),
                    step_index: 1,
                    via: None,
                    flow_step_id: None,
                    scope: MutationScope::State,
                },
            ],
            flow_tree: None,
            trace_config: TraceConfig::default(),
        });

        let summaries = s.variable_summary();
        assert_eq!(summaries.len(), 2);

        let balances = summaries.iter().find(|s| s.variable == "balances").unwrap();
        assert_eq!(balances.changes.len(), 2);
        assert!(balances.changes[0].contains("+= amount"));
        assert!(balances.changes[1].contains("-= amount"));
    }
}
