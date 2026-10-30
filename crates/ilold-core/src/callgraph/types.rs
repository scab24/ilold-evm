use petgraph::stable_graph::StableDiGraph;
use serde::{Deserialize, Serialize};

use crate::model::function::{Mutability, Visibility};

pub use crate::model::expression::CallKind;

pub type CallGraph = StableDiGraph<CallNode, CallEdge>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallNode {
    pub contract: String,
    pub function: String,
    /// Function key within `contract` (name, or signature when overloaded).
    #[serde(default)]
    pub key: String,
    pub visibility: Visibility,
    pub mutability: Mutability,
    /// Outside the analysed contract's linearization (library, other contract, unresolved).
    pub is_external: bool,
}

impl CallNode {
    /// Stable node id: `Contract::key`.
    pub fn id(&self) -> String {
        let key = if self.key.is_empty() { &self.function } else { &self.key };
        format!("{}::{}", self.contract, key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallEdge {
    pub kind: CallKind,
    pub call_count: usize,
    /// Id of the declaration the source names when virtual dispatch from the most derived
    /// contract selects a different implementation.
    #[serde(default)]
    pub static_target: Option<String>,
}
