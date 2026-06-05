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
}
