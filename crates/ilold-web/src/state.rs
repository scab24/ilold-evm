use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use tokio::sync::broadcast;

use ilold_core::callgraph::builder::build_call_graph;
use ilold_core::depgraph::ContractDeps;
use ilold_core::exploration::commands::CanvasPatch;
use ilold_core::callgraph::types::CallGraph;
use ilold_core::cfg::builder::CfgBuilder;
use ilold_core::cfg::types::CfgGraph;
use ilold_core::classify::entry_points::{classify_all, AccessLevel};
use ilold_core::exploration::session::ExplorationSession;
use ilold_core::model::project::Project;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::parse::ProjectParser;
use ilold_core::pathtree::config::PruningConfig;
use ilold_core::pathtree::types::PathTree;
use ilold_core::pathtree::walker::build_path_tree;
use ilold_core::sequence::analysis::{analyze_project, analyze_sequences, SequenceAnalysis};
use ilold_core::sequence::builder::build_sequence_tree;
use ilold_core::sequence::types::SequenceTree;

use serde::{Deserialize, Serialize};

pub const DEFAULT_SCENARIO: &str = "main";

pub struct ScenarioStore {
    pub version: u32,
    pub contract: String,
    active: String,
    sessions: HashMap<String, ExplorationSession>,
    order: Vec<String>,
}

impl ScenarioStore {
    pub fn new_for_contract(contract: impl Into<String>) -> Self {
        let contract = contract.into();
        let mut sessions = HashMap::new();
        sessions.insert(
            DEFAULT_SCENARIO.to_string(),
            ExplorationSession::new(&contract, "ilold"),
        );
        Self {
            version: 2,
            contract,
            active: DEFAULT_SCENARIO.to_string(),
            sessions,
            order: vec![DEFAULT_SCENARIO.to_string()],
        }
    }

    pub fn active(&self) -> &str {
        &self.active
    }

    pub fn active_session(&self) -> &ExplorationSession {
        self.sessions
            .get(&self.active)
            .expect("active scenario always present")
    }

    pub fn active_session_mut(&mut self) -> &mut ExplorationSession {
        self.sessions
            .get_mut(&self.active)
            .expect("active scenario always present")
    }

    pub fn get(&self, name: &str) -> Option<&ExplorationSession> {
        self.sessions.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut ExplorationSession> {
        self.sessions.get_mut(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.sessions.contains_key(name)
    }

    pub fn names(&self) -> &[String] {
        &self.order
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn insert(&mut self, name: impl Into<String>, session: ExplorationSession) {
        let name = name.into();
        if !self.sessions.contains_key(&name) {
            self.order.push(name.clone());
        }
        self.sessions.insert(name, session);
    }

    pub fn remove(&mut self, name: &str) -> Option<ExplorationSession> {
        let removed = self.sessions.remove(name)?;
        self.order.retain(|n| n != name);
        Some(removed)
    }

    pub fn set_active(&mut self, name: impl Into<String>) -> Result<(), String> {
        let name = name.into();
        if !self.sessions.contains_key(&name) {
            return Err(format!("Scenario '{name}' does not exist"));
        }
        self.active = name;
        Ok(())
    }

    pub fn save_to_json(&self) -> Result<String, String> {
        let file = ScenarioStoreFile {
            version: 2,
            contract: self.contract.clone(),
            active: self.active.clone(),
            scenarios: self.sessions.clone(),
            order: self.order.clone(),
        };
        serde_json::to_string_pretty(&file).map_err(|e| format!("Serialize failed: {e}"))
    }

    pub fn load_from_json(json: &str) -> Result<Self, String> {
        match serde_json::from_str::<ScenarioStoreFile>(json) {
            Ok(file) => Self::from_file(file),
            Err(_) => match serde_json::from_str::<ExplorationSession>(json) {
                Ok(legacy) => {
                    let contract = legacy.contract.clone();
                    let mut sessions = HashMap::new();
                    sessions.insert(DEFAULT_SCENARIO.to_string(), legacy);
                    Ok(Self {
                        version: 2,
                        contract,
                        active: DEFAULT_SCENARIO.to_string(),
                        sessions,
                        order: vec![DEFAULT_SCENARIO.to_string()],
                    })
                }
                Err(e) => Err(format!("Deserialize failed: {e}")),
            },
        }
    }

    fn from_file(file: ScenarioStoreFile) -> Result<Self, String> {
        if file.scenarios.is_empty() {
            return Err("Save file has no scenarios".into());
        }
        let mut order = file.order;
        order.retain(|n| file.scenarios.contains_key(n));
        for name in file.scenarios.keys() {
            if !order.contains(name) {
                order.push(name.clone());
            }
        }
        let active = if file.scenarios.contains_key(&file.active) {
            file.active
        } else {
            order
                .first()
                .cloned()
                .ok_or_else(|| "Save file has no usable active scenario".to_string())?
        };
        Ok(Self {
            version: 2,
            contract: file.contract,
            active,
            sessions: file.scenarios,
            order,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct ScenarioStoreFile {
    version: u32,
    contract: String,
    active: String,
    scenarios: HashMap<String, ExplorationSession>,
    order: Vec<String>,
}

pub struct AppState {
    pub project: Project,
    pub cfgs: HashMap<(String, String), CfgGraph>,
    pub path_trees: HashMap<(String, String), PathTree>,
    pub call_graphs: HashMap<String, CallGraph>,
    pub dep_graph: ContractDeps,
    pub sequence_trees: HashMap<String, SequenceTree>,
    pub sequence_analyses: HashMap<String, SequenceAnalysis>,
    pub classifications: HashMap<String, Vec<(String, AccessLevel)>>,
    pub annotations: RwLock<Vec<Annotation>>,
    pub scenarios: RwLock<ScenarioStore>,
    pub session_tx: broadcast::Sender<CanvasPatch>,
    pub port: u16,
    pub contract_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub id: String,
    pub contract: String,
    pub function: Option<String>,
    pub path_id: Option<usize>,
    pub block_id: Option<usize>,
    pub text: String,
    pub severity: AnnotationSeverity,
    pub status: AnnotationStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnnotationSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnnotationStatus {
    Open,
    Reviewed,
    Finding,
}

impl AppState {
    pub fn from_paths(paths: &[PathBuf], max_seq_depth: usize, port: u16, contract_path: PathBuf) -> anyhow::Result<Self> {
        let parser = SolcFrontend;
        let mut project = parser.parse(paths)?;
        project.rebuild_index();

        let config = PruningConfig::default();
        let mut cfgs = HashMap::new();
        let mut path_trees = HashMap::new();
        let mut call_graphs = HashMap::new();
        let mut sequence_trees = HashMap::new();
        let mut sequence_analyses = HashMap::new();
        let mut classifications = HashMap::new();

        for contract in &project.contracts {
            let cg = build_call_graph(&project, contract);
            call_graphs.insert(contract.name.clone(), cg);

            let mut contract_path_trees = Vec::new();
            let combined_state_vars = project.inherited_state_vars(contract);

            for func in &contract.functions {
                let key = (contract.name.clone(), func.name.clone());

                if let Ok(cfg) = CfgBuilder::build_with_project(func, contract, Some(&project)) {
                    let pt = build_path_tree(
                        &cfg,
                        &contract.name,
                        &func.name,
                        &combined_state_vars,
                        &config,
                    );
                    contract_path_trees.push(pt.clone());
                    path_trees.insert(key.clone(), pt);
                    cfgs.insert(key, cfg);
                }
            }

            let st = build_sequence_tree(contract, &contract_path_trees, max_seq_depth);
            sequence_trees.insert(contract.name.clone(), st);

            let pt_map: HashMap<(String, String), PathTree> = contract_path_trees
                .iter()
                .map(|pt| ((pt.contract.clone(), pt.function.clone()), pt.clone()))
                .collect();
            let analysis = analyze_sequences(&pt_map, &contract.name);
            sequence_analyses.insert(contract.name.clone(), analysis);

            classifications.insert(contract.name.clone(), classify_all(contract));
        }

        analyze_project(&project, &mut sequence_analyses);

        let dep_graph = ContractDeps::from_call_graphs(&project, &call_graphs);

        let (session_tx, _) = broadcast::channel(64);

        let default_contract = project.contracts.iter()
            .find(|c| !c.name.is_empty())
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "unknown".to_string());

        Ok(Self {
            project,
            cfgs,
            path_trees,
            call_graphs,
            dep_graph,
            sequence_trees,
            sequence_analyses,
            classifications,
            annotations: RwLock::new(Vec::new()),
            scenarios: RwLock::new(ScenarioStore::new_for_contract(default_contract)),
            session_tx,
            port,
            contract_path,
        })
    }
}
