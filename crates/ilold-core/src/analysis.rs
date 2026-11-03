use std::collections::{HashMap, HashSet, VecDeque};

use crate::callgraph::builder::build_call_graph;
use crate::callgraph::types::CallGraph;
use crate::cfg::builder::CfgBuilder;
use crate::cfg::types::CfgGraph;
use crate::classify::entry_points::{classify_contract, AccessLevel};
use crate::classify::guards::function_guards;
use crate::depgraph::ContractDeps;
use crate::model::contract::ContractDef;
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::parse::ProjectKey;
use crate::pathtree::config::PruningConfig;
use crate::pathtree::types::PathTree;
use crate::pathtree::walker::build_path_tree;
use crate::sequence::analysis::{apply_effects, direct_behaviors, transitions, SequenceAnalysis};
use crate::sequence::effects::{summarize, CfgSummary, EffectEngine};
use crate::sequence::builder::build_sequence_tree;
use crate::sequence::types::SequenceTree;

/// All of it is part of the cache key.
#[derive(Debug, Clone)]
pub struct AnalysisParams {
    pub max_seq_depth: usize,
    pub pruning: PruningConfig,
}

impl AnalysisParams {
    pub fn new(max_seq_depth: usize) -> Self {
        Self { max_seq_depth, pruning: PruningConfig::default() }
    }
}

pub struct ProjectAnalysis {
    pub cfgs: HashMap<(String, String), CfgGraph>,
    pub path_trees: HashMap<(String, String), PathTree>,
    pub call_graphs: HashMap<String, CallGraph>,
    pub dep_graph: ContractDeps,
    pub sequence_trees: HashMap<String, SequenceTree>,
    pub sequence_analyses: HashMap<String, SequenceAnalysis>,
    pub classifications: HashMap<String, Vec<(String, AccessLevel)>>,
}

impl ProjectAnalysis {
    pub fn build<'p>(project: &'p Project, params: &AnalysisParams) -> Self {
        let config = &params.pruning;
        let mut cfgs = HashMap::new();
        let mut path_trees = HashMap::new();
        let mut summaries: HashMap<(String, String), CfgSummary> = HashMap::new();
        let mut call_graphs = HashMap::new();
        let mut sequence_trees = HashMap::new();
        let mut sequence_analyses = HashMap::new();
        let mut classifications = HashMap::new();

        // other dependency functions (libraries, free functions) only once project code calls them
        let inherited_dependencies: HashSet<&str> = project
            .project_contracts()
            .flat_map(|c| project.linearization(c))
            .filter(|c| c.is_dependency)
            .map(|c| c.key())
            .collect();
        let mut pending: VecDeque<(&'p ContractDef, &'p FunctionDef)> = project
            .contracts
            .iter()
            .filter(|c| !c.is_dependency || inherited_dependencies.contains(c.key()))
            .flat_map(|c| c.functions.iter().map(move |f| (c, f)))
            .collect();
        let mut queued: HashSet<(String, String)> =
            pending.iter().map(|(c, f)| (c.key().to_string(), f.key.clone())).collect();
        let mut analyse = |pending: &mut VecDeque<(&'p ContractDef, &'p FunctionDef)>, queued: &mut HashSet<(String, String)>| {
            while let Some((contract, func)) = pending.pop_front() {
                let key = (contract.key().to_string(), func.key.clone());
                let cfg = CfgBuilder::build_with_project(func, contract, Some(project));
                let summary = summarize(&cfg, project);
                queue_dependency_callees(project, &summary, pending, queued);
                path_trees.insert(key.clone(), build_path_tree(&cfg, &key.0, &key.1, config));
                summaries.insert(key.clone(), summary);
                cfgs.insert(key, cfg);
            }
        };
        analyse(&mut pending, &mut queued);

        // an inherited function runs the derived contract's modifier overrides
        let mut context_summaries: HashMap<(String, String, String), CfgSummary> = HashMap::new();
        let mut context_trees: HashMap<(String, String, String), PathTree> = HashMap::new();
        for contract in project.project_contracts() {
            for owner in project.linearization(contract).into_iter().skip(1) {
                for func in owner.functions.iter().filter(|f| overrides_modifiers(project, contract, owner, f)) {
                    let cfg = CfgBuilder::build_with_project(func, contract, Some(project));
                    let key = (contract.key().to_string(), owner.key().to_string(), func.key.clone());
                    let summary = summarize(&cfg, project);
                    queue_dependency_callees(project, &summary, &mut pending, &mut queued);
                    context_trees.insert(key.clone(), build_path_tree(&cfg, &key.1, &key.2, config));
                    context_summaries.insert(key, summary);
                }
            }
        }
        analyse(&mut pending, &mut queued);

        let mut engine = EffectEngine::new(project, &summaries, &context_summaries);
        for contract in project.project_contracts() {
            let contract_key = contract.key().to_string();
            let own: HashMap<(String, String), PathTree> = path_trees
                .iter()
                .filter(|((c, _), _)| c == &contract_key)
                .map(|(k, pt)| (k.clone(), pt.clone()))
                .collect();
            let contract_path_trees: Vec<PathTree> = contract
                .functions
                .iter()
                .filter_map(|f| own.get(&(contract_key.clone(), f.key.clone())).cloned())
                .collect();

            call_graphs.insert(contract_key.clone(), build_call_graph(project, contract));
            sequence_trees.insert(
                contract_key.clone(),
                build_sequence_tree(contract, &contract_path_trees, params.max_seq_depth),
            );

            let mut functions = direct_behaviors(&own, &contract_key);
            for af in project.accessible_functions(contract).into_iter().filter(|af| af.is_inherited) {
                let key = (af.owner.key().to_string(), af.function.key.clone());
                let in_context = (contract_key.clone(), key.0.clone(), key.1.clone());
                let Some(pt) = context_trees.get(&in_context).or_else(|| path_trees.get(&key)) else { continue };
                let single = HashMap::from([(key.clone(), pt.clone())]);
                functions.extend(direct_behaviors(&single, &key.0));
            }
            functions.sort_by(|a, b| (&a.name, &a.owner).cmp(&(&b.name, &b.owner)));
            for behavior in &mut functions {
                let Some((owner, func)) = owner_of(project, contract, &behavior.owner, &behavior.name) else { continue };
                // read from code, so a behavior without a summary still gets them
                behavior.guards = function_guards(Some(project), contract, owner, func);
                let Some(summary) = engine.summary(contract, owner, func) else { continue };
                let effects = engine.effects(contract, owner, func);
                apply_effects(behavior, summary, &effects, func);
            }
            let transitions = transitions(&functions);
            sequence_analyses.insert(contract_key.clone(), SequenceAnalysis { functions, transitions });

            classifications.insert(contract_key, classify_contract(project, contract));
        }

        let dep_graph = ContractDeps::from_call_graphs(project, &call_graphs);

        Self {
            cfgs,
            path_trees,
            call_graphs,
            dep_graph,
            sequence_trees,
            sequence_analyses,
            classifications,
        }
    }

    /// Without a key the project cache was bypassed and the analysis is always built fresh.
    pub fn load_or_build(project: &Project, key: Option<&ProjectKey>, params: &AnalysisParams) -> Self {
        crate::analysis_cache::load_or_build_from_env(key, params, || Self::build(project, params))
    }
}

fn owner_of<'a>(
    project: &'a Project,
    contract: &'a ContractDef,
    owner: &str,
    key: &str,
) -> Option<(&'a ContractDef, &'a FunctionDef)> {
    project
        .accessible_functions(contract)
        .into_iter()
        .find(|af| af.owner.key() == owner && af.function.key == key)
        .map(|af| (af.owner, af.function))
}

fn queue_dependency_callees<'p>(
    project: &'p Project,
    summary: &CfgSummary,
    pending: &mut VecDeque<(&'p ContractDef, &'p FunctionDef)>,
    queued: &mut HashSet<(String, String)>,
) {
    for (callee_contract, callee_key) in summary.calls.iter().filter_map(|c| c.declaration.as_ref()) {
        let Some(&i) = project.contract_index.get(callee_contract) else { continue };
        let owner = &project.contracts[i];
        let Some(callee) = owner.functions.iter().find(|f| &f.key == callee_key) else { continue };
        if owner.is_dependency && queued.insert((owner.key().to_string(), callee.key.clone())) {
            pending.push_back((owner, callee));
        }
    }
}

fn overrides_modifiers(project: &Project, context: &ContractDef, owner: &ContractDef, func: &FunctionDef) -> bool {
    func.modifiers.iter().any(|m| {
        let there = project.resolve_modifier(context, &m.name).map(|d| d as *const _);
        let here = project.resolve_modifier(owner, &m.name).map(|d| d as *const _);
        there != here
    })
}
