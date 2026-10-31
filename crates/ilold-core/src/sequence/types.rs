use serde::{Deserialize, Serialize};

use crate::model::function::Visibility;
use crate::util::Page;

/// Functions a transaction sequence can call; the sequences themselves are generated per
/// request because there are `n + n^2 + ... + n^depth` of them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceTree {
    pub contract: String,
    pub functions: Vec<SequenceFunction>,
    /// Depth served when a request names none.
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

/// One page of the sequences up to `max_depth`, shortest first, each depth in lexicographic order.
#[derive(Debug, Clone, Serialize)]
pub struct SequencePage {
    pub contract: String,
    pub functions: Vec<SequenceFunction>,
    pub max_depth: usize,
    pub sequences: Vec<TransactionSequence>,
    pub page: Page,
}

impl SequenceTree {
    /// Number of sequences of length 1 to `depth`.
    pub fn sequence_count(&self, depth: usize) -> usize {
        (1..=depth).fold(0usize, |total, d| total.saturating_add(self.at_depth(d)))
    }

    pub fn page(&self, depth: usize, offset: usize, limit: usize) -> SequencePage {
        let page = Page::new(offset, limit, self.sequence_count(depth));
        SequencePage {
            contract: self.contract.clone(),
            functions: self.functions.clone(),
            max_depth: depth,
            sequences: page.range().map(|index| self.sequence(index)).collect(),
            page,
        }
    }

    fn at_depth(&self, depth: usize) -> usize {
        u32::try_from(depth).map_or(usize::MAX, |d| self.functions.len().saturating_pow(d))
    }

    // the index-th sequence: its depth, then its steps as base-n digits
    fn sequence(&self, mut index: usize) -> TransactionSequence {
        let mut depth = 1;
        while index >= self.at_depth(depth) {
            index -= self.at_depth(depth);
            depth += 1;
        }
        let n = self.functions.len();
        let mut steps = vec![0; depth];
        for step in steps.iter_mut().rev() {
            *step = index % n;
            index /= n;
        }
        TransactionSequence {
            path_count: steps.iter().fold(1u64, |count, &i| count.saturating_mul(self.functions[i].path_count as u64)),
            has_state_change: steps.iter().any(|&i| !self.functions[i].read_only),
            depth,
            steps,
        }
    }
}
