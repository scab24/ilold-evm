use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use super::common::StateVar;
use super::contract::{ContractDef, ContractKind};
use super::decl_id::{DeclId, DeclTable};
use super::function::{FunctionDef, FunctionKind, Visibility};

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
    pub owner: &'a ContractDef,
    pub origin: String,
    pub is_inherited: bool,
}

/// Why a function name or signature does not select exactly one function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionLookupError {
    NotFound,
    /// A bare name shared by several overloads; carries their signatures.
    Ambiguous(Vec<String>),
}

impl Project {
    pub fn contract_by_id(&self, id: DeclId) -> Option<&ContractDef> {
        self.contracts.iter().find(|c| c.id == Some(id))
    }

    /// The contracts of `contract`'s C3 linearization that are in the model, most derived
    /// first. Uses solc `linearizedBaseContracts`; models without it fall back to the
    /// declared bases, right to left, breadth first.
    pub fn linearization<'a>(&'a self, contract: &'a ContractDef) -> Vec<&'a ContractDef> {
        let mut out = vec![contract];
        if !contract.linearized_bases.is_empty() {
            out.extend(
                contract
                    .linearized_bases
                    .iter()
                    .filter(|id| Some(**id) != contract.id)
                    .filter_map(|id| self.contract_by_id(*id)),
            );
            return out;
        }
        let mut seen: HashSet<&str> = HashSet::from([contract.name.as_str()]);
        let mut queue: VecDeque<&str> = contract.inherits.iter().rev().map(String::as_str).collect();
        while let Some(name) = queue.pop_front() {
            if !seen.insert(name) { continue; }
            let Some(&i) = self.contract_index.get(name) else { continue };
            let parent = &self.contracts[i];
            out.push(parent);
            queue.extend(parent.inherits.iter().rev().map(String::as_str));
        }
        out
    }

    /// First implementation of `signature` in `linearization`, starting after `after` when
    /// given (`super`). Declarations without a body are used only when nothing implements it.
    pub fn implementation<'a>(
        &self,
        linearization: &[&'a ContractDef],
        signature: &str,
        after: Option<&str>,
    ) -> Option<(&'a ContractDef, &'a FunctionDef)> {
        let start = match after {
            Some(name) => linearization.iter().position(|c| c.name == name)? + 1,
            None => 0,
        };
        let candidates = || {
            linearization[start..].iter().flat_map(|c| {
                c.functions
                    .iter()
                    .filter(move |f| f.match_signature() == signature)
                    .map(move |f| (*c, f))
            })
        };
        candidates().find(|(_, f)| f.body.is_some()).or_else(|| candidates().next())
    }

    /// Functions callable on `contract`: its own, then inherited public/external ones in
    /// linearization order. An override hides the base function with the same signature.
    pub fn accessible_functions<'a>(&'a self, contract: &'a ContractDef) -> Vec<AccessibleFunction<'a>> {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut result: Vec<AccessibleFunction<'a>> = Vec::new();

        for (depth, owner) in self.linearization(contract).into_iter().enumerate() {
            let is_inherited = depth > 0;
            for f in &owner.functions {
                if f.name.is_empty() { continue; }
                if is_inherited
                    && (!matches!(f.visibility, Visibility::Public | Visibility::External)
                        || f.kind == FunctionKind::Constructor)
                {
                    continue;
                }
                if seen.insert(f.match_signature()) {
                    result.push(AccessibleFunction {
                        function: f,
                        owner,
                        origin: owner.name.clone(),
                        is_inherited,
                    });
                }
            }
        }

        result
    }

    /// Selects one accessible function by bare name or by signature (`f(uint256,bool)`).
    pub fn lookup_function<'a>(
        &'a self,
        contract: &'a ContractDef,
        query: &str,
    ) -> Result<(&'a ContractDef, &'a FunctionDef), FunctionLookupError> {
        // internal signatures carry spaced locations (`f(uint256[] storage)`); compare unspaced
        let compact = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        let query = compact(query);
        let matches: Vec<AccessibleFunction<'a>> = self
            .accessible_functions(contract)
            .into_iter()
            .filter(|af| {
                if query.contains('(') {
                    compact(af.function.match_signature()) == query || compact(&af.function.key) == query
                } else {
                    af.function.name == query
                }
            })
            .collect();
        // `receive` names the receive function even when a regular `receive()` exists
        let special: Vec<&AccessibleFunction<'a>> = matches
            .iter()
            .filter(|af| af.function.kind != FunctionKind::Function && af.function.key == query)
            .collect();
        if let [only] = special.as_slice() {
            return Ok((only.owner, only.function));
        }
        match matches.as_slice() {
            [] => Err(FunctionLookupError::NotFound),
            [only] => Ok((only.owner, only.function)),
            _ => Err(FunctionLookupError::Ambiguous(
                matches.iter().map(|af| af.function.match_signature().to_string()).collect(),
            )),
        }
    }

    /// Lenient lookup for interactive commands: like [`Project::lookup_function`], but an
    /// ambiguous bare name picks the first overload.
    pub fn resolve_function<'a>(
        &'a self,
        contract: &'a ContractDef,
        func_name: &str,
    ) -> Option<(&'a ContractDef, &'a FunctionDef)> {
        match self.lookup_function(contract, func_name) {
            Ok(found) => Some(found),
            Err(FunctionLookupError::NotFound) => None,
            Err(FunctionLookupError::Ambiguous(_)) => self
                .accessible_functions(contract)
                .into_iter()
                .find(|af| af.function.name == func_name)
                .map(|af| (af.owner, af.function)),
        }
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
        self.linearization(contract)
            .into_iter()
            .find_map(|c| c.modifiers.iter().find(|m| m.name == mod_name))
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
