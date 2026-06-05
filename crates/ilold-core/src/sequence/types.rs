use serde::{Deserialize, Serialize};

use crate::model::function::Visibility;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceTree {
    pub contract: String,
    pub functions: Vec<SequenceFunction>,
    pub sequences: Vec<TransactionSequence>,
    pub max_depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceFunction {
    pub name: String,
    pub visibility: Visibility,
    pub read_only: bool,
    pub path_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionSequence {
    pub steps: Vec<usize>,
    pub depth: usize,
    pub path_count: u64,
    pub has_state_change: bool,
}
