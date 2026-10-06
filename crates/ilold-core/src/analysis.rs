//! Whole-project derived analysis shared by the web server and the CLI commands.
use std::collections::HashMap;

use crate::callgraph::builder::build_call_graph;
use crate::callgraph::types::CallGraph;
use crate::cfg::builder::CfgBuilder;
use crate::cfg::types::CfgGraph;
use crate::classify::entry_points::{classify_all, AccessLevel};
use crate::depgraph::ContractDeps;
use crate::model::project::Project;
use crate::parse::ProjectKey;
use crate::pathtree::config::PruningConfig;
use crate::pathtree::types::PathTree;
use crate::pathtree::walker::build_path_tree;
use crate::sequence::analysis::{analyze_project, analyze_sequences, SequenceAnalysis};
use crate::sequence::builder::build_sequence_tree;
use crate::sequence::types::SequenceTree;

/// Every input of the analysis besides the project itself. All of it is part of the cache key.
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

/// Derived analysis only; sessions, annotations and runtime handles live elsewhere.
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
    pub fn build(project: &Project, params: &AnalysisParams) -> Self {
        let config = &params.pruning;
        let mut cfgs = HashMap::new();
        let mut path_trees = HashMap::new();
        let mut call_graphs = HashMap::new();
        let mut sequence_trees = HashMap::new();
        let mut sequence_analyses = HashMap::new();
        let mut classifications = HashMap::new();

        for contract in &project.contracts {
            let cg = build_call_graph(project, contract);
            call_graphs.insert(contract.name.clone(), cg);

            let mut contract_path_trees = Vec::new();
            let combined_state_vars = project.inherited_state_vars(contract);

            for func in &contract.functions {
                let key = (contract.name.clone(), func.name.clone());

                if let Ok(cfg) = CfgBuilder::build_with_project(func, contract, Some(project)) {
                    let pt = build_path_tree(
                        &cfg,
                        &contract.name,
                        &func.name,
                        &combined_state_vars,
                        config,
                    );
                    contract_path_trees.push(pt.clone());
                    path_trees.insert(key.clone(), pt);
                    cfgs.insert(key, cfg);
                }
            }

            let st = build_sequence_tree(contract, &contract_path_trees, params.max_seq_depth);
            sequence_trees.insert(contract.name.clone(), st);

            let pt_map: HashMap<(String, String), PathTree> = contract_path_trees
                .iter()
                .map(|pt| ((pt.contract.clone(), pt.function.clone()), pt.clone()))
                .collect();
            let analysis = analyze_sequences(&pt_map, &contract.name);
            sequence_analyses.insert(contract.name.clone(), analysis);

            classifications.insert(contract.name.clone(), classify_all(contract));
        }

        analyze_project(project, &mut sequence_analyses);

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

    /// Reuses a persisted analysis for this validated project key, or builds and publishes one.
    /// Without a key (project cache bypassed) the analysis is always built fresh.
    pub fn load_or_build(project: &Project, key: Option<&ProjectKey>, params: &AnalysisParams) -> Self {
        crate::analysis_cache::load_or_build_from_env(key, params, || Self::build(project, params))
    }
}
