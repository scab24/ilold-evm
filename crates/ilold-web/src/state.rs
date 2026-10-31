use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use tokio::sync::broadcast;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::depgraph::ContractDeps;
use ilold_core::exploration::commands::{CanvasPatch, ScenarioInfo};
use ilold_core::callgraph::types::CallGraph;
use ilold_core::cfg::types::CfgGraph;
use ilold_core::classify::entry_points::AccessLevel;
use ilold_core::exploration::session::ExplorationSession;
use ilold_core::model::project::Project;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::pathtree::types::PathTree;
use ilold_core::sequence::analysis::SequenceAnalysis;
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

    pub fn infos(&self) -> Vec<ScenarioInfo> {
        self.order
            .iter()
            .map(|name| ScenarioInfo {
                name: name.clone(),
                active: name == self.active(),
                step_count: self.get(name).map(|s| s.steps.len()).unwrap_or(0),
            })
            .collect()
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

/// One store per contract; `current` is the contract named by the most recent `/api/cmd`.
pub struct ScenarioRegistry {
    current: String,
    stores: HashMap<String, ScenarioStore>,
}

impl ScenarioRegistry {
    pub fn new(default_contract: impl Into<String>) -> Self {
        let current = default_contract.into();
        let mut stores = HashMap::new();
        stores.insert(current.clone(), ScenarioStore::new_for_contract(&current));
        Self { current, stores }
    }

    pub fn current_contract(&self) -> &str {
        &self.current
    }

    pub fn store(&self, contract: &str) -> Option<&ScenarioStore> {
        self.stores.get(contract)
    }

    /// Created on first use; it becomes the current contract.
    pub fn select(&mut self, contract: &str) -> &mut ScenarioStore {
        self.current = contract.to_string();
        self.stores
            .entry(contract.to_string())
            .or_insert_with(|| ScenarioStore::new_for_contract(contract))
    }

    /// Makes it current.
    pub fn replace(&mut self, store: ScenarioStore) {
        self.current = store.contract.clone();
        self.stores.insert(store.contract.clone(), store);
    }
}

impl std::ops::Deref for ScenarioRegistry {
    type Target = ScenarioStore;

    fn deref(&self) -> &ScenarioStore {
        self.stores.get(&self.current).expect("current contract always has a store")
    }
}

impl std::ops::DerefMut for ScenarioRegistry {
    fn deref_mut(&mut self) -> &mut ScenarioStore {
        self.stores.get_mut(&self.current).expect("current contract always has a store")
    }
}

#[derive(Debug, Clone)]
pub struct SessionBroadcast {
    pub contract: String,
    pub patch: CanvasPatch,
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
    pub scenarios: RwLock<ScenarioRegistry>,
    pub session_tx: broadcast::Sender<SessionBroadcast>,
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
        let (mut project, key) = parser.parse_keyed(paths)?;
        project.rebuild_index();

        let params = AnalysisParams::new(max_seq_depth);
        let ProjectAnalysis {
            cfgs,
            path_trees,
            call_graphs,
            dep_graph,
            sequence_trees,
            sequence_analyses,
            classifications,
        } = ProjectAnalysis::load_or_build(&project, key.as_ref(), &params);

        let (session_tx, _) = broadcast::channel(64);

        let default_contract = project.default_contract()
            .map(|c| c.key().to_string())
            .ok_or_else(|| anyhow::anyhow!("No contracts found"))?;

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
            scenarios: RwLock::new(ScenarioRegistry::new(default_contract)),
            session_tx,
            port,
            contract_path,
        })
    }
}
