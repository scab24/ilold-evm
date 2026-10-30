use std::collections::HashMap;
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::common::StateVar;
use super::contract::{ContractDef, ContractKind};
use super::decl_id::DeclTable;
use super::function::{FunctionDef, Visibility};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub source_files: Vec<SourceFile>,
    pub contracts: Vec<ContractDef>,
    #[serde(skip)]
    pub contract_index: HashMap<String, usize>,
    #[serde(skip)]
    pub decl_table: DeclTable,
}

impl Project {
    pub fn contract_folder(&self, c: &ContractDef) -> String {
        let path = self
            .source_files
            .get(c.span.file_index)
            .map(|s| s.path.as_str())
            .unwrap_or("");
        let rel = path.rsplit_once("/src/").map(|(_, r)| r).unwrap_or(path);
        rel.rsplit_once('/').map(|(dir, _)| dir.to_string()).unwrap_or_default()
    }

    // call after deserialization; serde(skip) leaves the index empty
    pub fn rebuild_index(&mut self) {
        self.contract_index = self
            .contracts
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.clone(), i))
            .collect();
    }

    /// Contract a session starts on when none is named; see [`pick_default_contract`].
    pub fn default_contract(&self) -> Option<&ContractDef> {
        let name = pick_default_contract(
            self.contracts.iter().map(|c| (c.name.as_str(), c.kind, c.inherits.as_slice())),
        )?;
        self.contracts.iter().find(|c| c.name == name)
    }

    pub fn find_contract(&self, filter: Option<&str>) -> Result<&ContractDef, String> {
        if let Some(name) = filter {
            return self
                .contracts
                .iter()
                .find(|c| c.name == name)
                .ok_or_else(|| format!("Contract '{}' not found", name));
        }

        let non_interface: Vec<&ContractDef> = self
            .contracts
            .iter()
            .filter(|c| c.kind != ContractKind::Interface)
            .collect();

        match non_interface.len() {
            0 => Err("No contracts found".into()),
            1 => Ok(non_interface[0]),
            _ => {
                let inherited: HashSet<&str> = non_interface
                    .iter()
                    .flat_map(|c| c.inherits.iter().map(|s| s.as_str()))
                    .collect();

                let top_level: Vec<&&ContractDef> = non_interface
                    .iter()
                    .filter(|c| !inherited.contains(c.name.as_str()))
                    .collect();

                match top_level.len() {
                    1 => Ok(*top_level[0]),
                    _ => {
                        let names: Vec<&str> = non_interface.iter().map(|c| c.name.as_str()).collect();
                        Err(format!(
                            "Multiple contracts, specify one: {}",
                            names.join(", ")
                        ))
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: String,
    pub content: String,
}

pub struct AccessibleFunction<'a> {
    pub function: &'a FunctionDef,
    pub origin: String,
    pub is_inherited: bool,
}

impl Project {
    pub fn accessible_functions<'a>(&'a self, contract: &'a ContractDef) -> Vec<AccessibleFunction<'a>> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut result: Vec<AccessibleFunction<'a>> = Vec::new();

        for f in &contract.functions {
            if f.name.is_empty() { continue; }
            if seen.insert(f.name.clone()) {
                result.push(AccessibleFunction {
                    function: f,
                    origin: contract.name.clone(),
                    is_inherited: false,
                });
            }
        }

        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: Vec<&str> = contract.inherits.iter().map(|s| s.as_str()).collect();

        while let Some(parent_name) = queue.pop() {
            if !visited.insert(parent_name.to_string()) { continue; }

            let parent_idx = match self.contract_index.get(parent_name) {
                Some(&i) => i,
                None => continue,
            };
            let parent = &self.contracts[parent_idx];

            for f in &parent.functions {
                if f.name.is_empty() { continue; }
                if !matches!(f.visibility, Visibility::Public | Visibility::External) { continue; }
                if seen.insert(f.name.clone()) {
                    result.push(AccessibleFunction {
                        function: f,
                        origin: parent.name.clone(),
                        is_inherited: true,
                    });
                }
            }

            for grand in &parent.inherits {
                queue.push(grand.as_str());
            }
        }

        result
    }

    pub fn resolve_function<'a>(
        &'a self,
        contract: &'a ContractDef,
        func_name: &str,
    ) -> Option<(&'a ContractDef, &'a FunctionDef)> {
        let accessible = self.accessible_functions(contract);
        for af in accessible {
            if af.function.name == func_name {
                if af.is_inherited {
                    return self.contracts.iter()
                        .find(|c| c.name == af.origin)
                        .map(|parent| (parent, af.function));
                } else {
                    return Some((contract, af.function));
                }
            }
        }
        None
    }

    pub fn accessible_state_vars(&self, contract: &ContractDef) -> Vec<(StateVar, String, bool)> {
        let mut result: Vec<(StateVar, String, bool)> = contract
            .state_vars
            .iter()
            .cloned()
            .map(|sv| (sv, contract.name.clone(), false))
            .collect();

        let mut seen_names: HashSet<String> = contract.state_vars.iter()
            .map(|sv| sv.name.clone())
            .collect();

        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: Vec<&str> = contract.inherits.iter().map(|s| s.as_str()).collect();

        while let Some(parent_name) = queue.pop() {
            if !visited.insert(parent_name.to_string()) { continue; }

            let parent_idx = match self.contract_index.get(parent_name) {
                Some(&i) => i,
                None => continue,
            };
            let parent = &self.contracts[parent_idx];

            for sv in &parent.state_vars {
                if seen_names.insert(sv.name.clone()) {
                    result.push((sv.clone(), parent.name.clone(), true));
                }
            }

            for grand in &parent.inherits {
                queue.push(grand.as_str());
            }
        }

        result
    }

    pub fn resolve_modifier<'a>(
        &'a self,
        contract: &'a ContractDef,
        mod_name: &str,
    ) -> Option<&'a crate::model::modifier::ModifierDef> {
        if let Some(m) = contract.modifiers.iter().find(|m| m.name == mod_name) {
            return Some(m);
        }

        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: Vec<&str> = contract.inherits.iter().map(|s| s.as_str()).collect();

        while let Some(parent_name) = queue.pop() {
            if !visited.insert(parent_name.to_string()) {
                continue;
            }

            let parent_idx = match self.contract_index.get(parent_name) {
                Some(&i) => i,
                None => continue,
            };
            let parent = &self.contracts[parent_idx];

            if let Some(m) = parent.modifiers.iter().find(|m| m.name == mod_name) {
                return Some(m);
            }

            for grand in &parent.inherits {
                queue.push(grand.as_str());
            }
        }

        None
    }

    pub fn inherited_state_vars(&self, contract: &ContractDef) -> Vec<StateVar> {
        let mut result: Vec<StateVar> = contract.state_vars.clone();

        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: Vec<&str> = contract.inherits.iter().map(|s| s.as_str()).collect();

        while let Some(parent_name) = queue.pop() {
            if !visited.insert(parent_name.to_string()) { continue; }

            let parent_idx = match self.contract_index.get(parent_name) {
                Some(&i) => i,
                None => continue,
            };
            let parent = &self.contracts[parent_idx];

            result.extend(parent.state_vars.iter().cloned());

            for grand in &parent.inherits {
                queue.push(grand.as_str());
            }
        }

        result
    }
}

/// Deterministic default contract shared by the web server and both REPL modes.
/// Prefers deployable contracts, then abstract contracts, libraries and interfaces;
/// within a kind the most derived contract (most transitive ancestors) wins and
/// ties break by name. Unnamed entries are ignored; `None` only for no contracts.
pub fn pick_default_contract<'a>(
    candidates: impl IntoIterator<Item = (&'a str, ContractKind, &'a [String])>,
) -> Option<&'a str> {
    let candidates: Vec<(&str, ContractKind, &[String])> = candidates
        .into_iter()
        .filter(|(name, _, _)| !name.is_empty())
        .collect();
    let parents: HashMap<&str, &[String]> =
        candidates.iter().map(|(name, _, inherits)| (*name, *inherits)).collect();
    let ancestors = |name: &str| -> usize {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut queue: Vec<&str> = parents.get(name).into_iter()
            .flat_map(|p| p.iter().map(String::as_str)).collect();
        while let Some(parent) = queue.pop() {
            if parent == name || !seen.insert(parent) { continue; }
            if let Some(grand) = parents.get(parent) {
                queue.extend(grand.iter().map(String::as_str));
            }
        }
        seen.len()
    };
    let kind_rank = |kind: ContractKind| match kind {
        ContractKind::Contract => 0,
        ContractKind::Abstract => 1,
        ContractKind::Library => 2,
        ContractKind::Interface => 3,
    };
    candidates
        .iter()
        .map(|(name, kind, _)| (kind_rank(*kind), std::cmp::Reverse(ancestors(name)), *name))
        .min()
        .map(|(_, _, name)| name)
}

#[cfg(test)]
mod default_contract_tests {
    use super::*;

    fn pick(items: &[(&str, ContractKind, Vec<String>)]) -> Option<String> {
        pick_default_contract(items.iter().map(|(n, k, i)| (*n, *k, i.as_slice())))
            .map(str::to_string)
    }

    fn parents(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn prefers_most_derived_deployable_contract() {
        let items = [
            ("SafeMath", ContractKind::Library, vec![]),
            ("IPool", ContractKind::Interface, vec![]),
            ("BasePool", ContractKind::Abstract, vec![]),
            ("Vault", ContractKind::Contract, vec![]),
            ("LendingPool", ContractKind::Contract, parents(&["BasePool", "IPool"])),
        ];
        assert_eq!(pick(&items).as_deref(), Some("LendingPool"));
    }

    #[test]
    fn counts_transitive_ancestors_and_breaks_ties_by_name() {
        let items = [
            ("Zeta", ContractKind::Contract, parents(&["Base"])),
            ("Alpha", ContractKind::Contract, parents(&["Base"])),
            ("Base", ContractKind::Contract, parents(&["Root"])),
            ("Root", ContractKind::Abstract, vec![]),
        ];
        assert_eq!(pick(&items).as_deref(), Some("Alpha"));
        let order_independent: Vec<_> = items.iter().rev().cloned().collect();
        assert_eq!(pick(&order_independent).as_deref(), Some("Alpha"));
    }

    #[test]
    fn falls_back_to_non_deployable_kinds_and_ignores_unnamed() {
        let items = [
            ("", ContractKind::Contract, vec![]),
            ("IToken", ContractKind::Interface, vec![]),
            ("Math", ContractKind::Library, vec![]),
        ];
        assert_eq!(pick(&items).as_deref(), Some("Math"));
        assert_eq!(pick(&[]), None);
    }
}
