use crate::model::contract::ContractDef;
use crate::model::function::{FunctionKind, Mutability, Visibility};
use crate::pathtree::types::PathTree;

use super::types::*;

/// Interfaces and unimplemented functions run nothing.
pub fn build_sequence_tree(
    contract: &ContractDef,
    path_trees: &[PathTree],
    max_depth: usize,
) -> SequenceTree {
    let functions: Vec<SequenceFunction> = contract
        .functions
        .iter()
        .filter(|f| f.kind != FunctionKind::Constructor && f.body.is_some())
        .filter(|f| matches!(f.visibility, Visibility::Public | Visibility::External))
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

    SequenceTree {
        contract: contract.key().to_string(),
        functions,
        max_depth,
    }
}
