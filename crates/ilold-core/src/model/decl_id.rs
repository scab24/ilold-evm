use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeclId(pub isize);

#[derive(Debug, Clone)]
pub struct DeclTarget {
    pub contract: String,
    pub function: String,
}

#[derive(Debug, Clone, Default)]
pub struct DeclTable {
    targets: HashMap<DeclId, DeclTarget>,
}

impl DeclTable {
    pub fn insert(&mut self, id: DeclId, contract: String, function: String) {
        self.targets.insert(id, DeclTarget { contract, function });
    }

    pub fn lookup(&self, id: DeclId) -> Option<&DeclTarget> {
        self.targets.get(&id)
    }

    pub(crate) fn len(&self) -> usize { self.targets.len() }

    pub(crate) fn snapshot(&self) -> Vec<(isize, String, String)> {
        let mut entries: Vec<_> = self.targets.iter().map(|(id, target)|
            (id.0, target.contract.clone(), target.function.clone())).collect();
        entries.sort_by_key(|entry| entry.0);
        entries
    }

    pub(crate) fn from_snapshot(entries: Vec<(isize, String, String)>) -> Option<Self> {
        let mut table = Self::default();
        for (id, contract, function) in entries {
            if table.targets.insert(DeclId(id), DeclTarget { contract, function }).is_some() {
                return None;
            }
        }
        Some(table)
    }
}
