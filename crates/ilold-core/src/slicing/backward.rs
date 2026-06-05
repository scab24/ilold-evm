use std::collections::{BTreeSet, HashSet};

use super::extract::extract_statement_uses_defs;
use super::flatten::FlatStatement;
use super::types::StatementPath;

pub fn backward_slice(flat: &[FlatStatement<'_>], variable: &str) -> Vec<StatementPath> {
    let mut relevant: HashSet<String> = HashSet::new();
    relevant.insert(variable.to_string());

    let mut included: BTreeSet<usize> = BTreeSet::new();

    loop {
        let mut changed = false;

        for i in (0..flat.len()).rev() {
            if included.contains(&i) {
                continue;
            }
            let (uses, defs) = extract_statement_uses_defs(flat[i].statement);
            if defs.is_disjoint(&relevant) {
                continue;
            }
            included.insert(i);
            changed = true;
            for u in uses {
                if relevant.insert(u) {
                    changed = true;
                }
            }
        }

        if add_ancestors_with_uses(flat, &mut included, &mut relevant) {
            changed = true;
        }

        if !changed {
            break;
        }
    }

    included.into_iter().map(|i| flat[i].path.clone()).collect()
}

pub(super) fn add_ancestors_with_uses(
    flat: &[FlatStatement<'_>],
    included: &mut BTreeSet<usize>,
    relevant: &mut HashSet<String>,
) -> bool {
    let mut changed = false;
    let seeds: Vec<usize> = included.iter().copied().collect();
    for idx in seeds {
        let mut cur = flat[idx].parent;
        while let Some(p) = cur {
            if included.insert(p) {
                changed = true;
                let (uses, _defs) = extract_statement_uses_defs(flat[p].statement);
                for u in uses {
                    if relevant.insert(u) {
                        changed = true;
                    }
                }
            }
            cur = flat[p].parent;
        }
    }
    changed
}
