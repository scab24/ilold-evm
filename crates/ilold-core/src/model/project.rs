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
