use std::collections::{BTreeSet, HashSet};

use super::backward::add_ancestors_with_uses;
use super::extract::extract_statement_uses_defs;
use super::flatten::FlatStatement;
use super::types::StatementPath;

pub fn forward_slice(flat: &[FlatStatement<'_>], variable: &str) -> Vec<StatementPath> {
    let mut tainted: HashSet<String> = HashSet::new();
    tainted.insert(variable.to_string());

    let mut included: BTreeSet<usize> = BTreeSet::new();

    loop {
        let mut changed = false;

        for i in 0..flat.len() {
            if included.contains(&i) {
                continue;
            }
            let (uses, defs) = extract_statement_uses_defs(flat[i].statement);
            if uses.is_disjoint(&tainted) {
                continue;
            }
            included.insert(i);
            changed = true;
            for d in defs {
                if tainted.insert(d) {
                    changed = true;
                }
            }
        }

        if add_ancestors_with_uses(flat, &mut included, &mut tainted) {
            changed = true;
        }

        if !changed {
            break;
        }
    }

    included.into_iter().map(|i| flat[i].path.clone()).collect()
}
