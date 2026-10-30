use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeclId(pub isize);

/// What a solc declaration id names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeclKind {
    Contract,
    Function,
    Modifier,
    StateVariable,
    Event,
    Error,
}

#[derive(Debug, Clone)]
pub struct DeclTarget {
    pub kind: DeclKind,
    /// Key of the declaring contract (`ContractDef::key`).
    pub contract: String,
    /// Function key for functions, empty for every other kind.
    pub function: String,
    /// Member name (function key for functions); empty for contracts.
    pub name: String,
    /// Readable stable key: `<contract qualified id>` or `<contract qualified id>::<name>`.
    pub key: String,
}

/// One persisted declaration: id, kind, contract key, function key, member name, readable key.
pub(crate) type DeclEntry = (isize, DeclKind, String, String, String, String);

#[derive(Debug, Clone, Default)]
pub struct DeclTable {
    targets: HashMap<DeclId, DeclTarget>,
}

impl DeclTable {
    /// Registers a function (or, with an empty `function`, its contract) by contract key.
    pub fn insert(&mut self, id: DeclId, contract: String, function: String) {
        let kind = if function.is_empty() { DeclKind::Contract } else { DeclKind::Function };
        let key = if function.is_empty() { contract.clone() } else { format!("{contract}::{function}") };
        self.targets.insert(id, DeclTarget { kind, contract, name: function.clone(), function, key });
    }

    pub fn insert_target(&mut self, id: DeclId, target: DeclTarget) {
        self.targets.insert(id, target);
    }

    pub fn lookup(&self, id: DeclId) -> Option<&DeclTarget> {
        self.targets.get(&id)
    }

    /// Function declarations only.
    pub fn function(&self, id: DeclId) -> Option<&DeclTarget> {
        self.lookup(id).filter(|t| t.kind == DeclKind::Function)
    }

    pub(crate) fn len(&self) -> usize { self.targets.len() }

    pub(crate) fn snapshot(&self) -> Vec<DeclEntry> {
        let mut entries: Vec<_> = self.targets.iter().map(|(id, t)|
            (id.0, t.kind, t.contract.clone(), t.function.clone(), t.name.clone(), t.key.clone())).collect();
        entries.sort_by_key(|entry| entry.0);
        entries
    }

    pub(crate) fn from_snapshot(entries: Vec<DeclEntry>) -> Option<Self> {
        let mut table = Self::default();
        for (id, kind, contract, function, name, key) in entries {
            let target = DeclTarget { kind, contract, function, name, key };
            if table.targets.insert(DeclId(id), target).is_some() {
                return None;
            }
        }
        Some(table)
    }
}
