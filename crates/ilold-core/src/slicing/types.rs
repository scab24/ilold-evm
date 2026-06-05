use serde::{Deserialize, Serialize};

use crate::model::common::SourceSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SliceDirection {
    Backward,
    Forward,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name")]
pub enum StatementOrigin {
    FunctionBody,
    Modifier(String),
}

impl Default for StatementOrigin {
    fn default() -> Self {
        StatementOrigin::FunctionBody
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StatementPath(pub Vec<usize>);

impl StatementPath {
    pub fn new(indices: Vec<usize>) -> Self {
        StatementPath(indices)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceEntry {
    pub path: StatementPath,
    #[serde(default)]
    pub span: Option<SourceSpan>,
    pub text: String,
    #[serde(default)]
    pub origin: StatementOrigin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceResult {
    pub function: String,
    pub variable: String,
    pub direction: SliceDirection,
    pub backward: Vec<SliceEntry>,
    pub forward: Vec<SliceEntry>,
}
