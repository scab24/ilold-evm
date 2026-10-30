use crate::model::contract::ContractDef;
use crate::model::function::{FunctionKind, Mutability};
use crate::pathtree::types::PathTree;

use super::types::*;

pub fn build_sequence_tree(
    contract: &ContractDef,
    path_trees: &[PathTree],
    max_depth: usize,
) -> SequenceTree {
    let functions: Vec<SequenceFunction> = contract
        .functions
        .iter()
        .filter(|f| f.kind != FunctionKind::Constructor)
        .filter(|f| matches!(f.visibility, crate::model::function::Visibility::Public | crate::model::function::Visibility::External))
        .map(|f| {
            // path trees are keyed like the analysis maps; overloads by signature
            let key = if f.key.is_empty() { &f.name } else { &f.key };
            let path_count = path_trees
                .iter()
                .find(|pt| &pt.function == key)
                .map(|pt| pt.stats.total_paths)
                .unwrap_or(0);

            SequenceFunction {
                name: key.clone(),
                visibility: f.visibility,
                read_only: matches!(f.mutability, Mutability::Pure | Mutability::View),
                path_count,
            }
        })
        .collect();

    let n = functions.len();
    let mut sequences: Vec<TransactionSequence> = Vec::new();

    for i in 0..n {
        sequences.push(TransactionSequence {
            steps: vec![i],
            depth: 1,
            path_count: functions[i].path_count as u64,
            has_state_change: !functions[i].read_only,
        });
    }

    for d in 2..=max_depth {
        let prev: Vec<TransactionSequence> = sequences
            .iter()
            .filter(|s| s.depth == d - 1)
            .cloned()
            .collect();

        for seq in &prev {
            for i in 0..n {
                let mut steps = seq.steps.clone();
                steps.push(i);
                sequences.push(TransactionSequence {
                    steps,
                    depth: d,
                    path_count: seq.path_count * functions[i].path_count as u64,
                    has_state_change: seq.has_state_change || !functions[i].read_only,
                });
            }
        }
    }

    SequenceTree {
        contract: contract.name.clone(),
        functions,
        sequences,
        max_depth,
    }
}
