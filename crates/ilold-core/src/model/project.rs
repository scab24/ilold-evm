use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use super::common::StateVar;
use super::contract::{ContractDef, ContractKind};
use super::decl_id::{DeclId, DeclKind, DeclTable};
use super::function::{FunctionDef, FunctionKind, Visibility};
use crate::util::compact;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub source_files: Vec<SourceFile>,
    /// Project contracts and the dependency contracts they reach.
    pub contracts: Vec<ContractDef>,
    /// By `key` and by `qualified_id`.
    #[serde(skip)]
    pub contract_index: HashMap<String, usize>,
    #[serde(skip)]
    pub id_index: HashMap<DeclId, usize>,
    #[serde(skip)]
    pub decl_table: DeclTable,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractLookupError {
    NotFound,
    /// Carries the qualified ids.
    Ambiguous(Vec<String>),
}

impl Project {
    pub fn new(source_files: Vec<SourceFile>, contracts: Vec<ContractDef>, decl_table: DeclTable) -> Self {
        let mut project = Self {
            source_files,
            contracts,
            contract_index: HashMap::new(),
            id_index: HashMap::new(),
            decl_table,
            warnings: Vec::new(),
        };
        project.rebuild_index();
        project
    }

    pub fn contract_folder(&self, c: &ContractDef) -> String {
        let path = self
            .source_files
            .get(c.span.file_index)
            .map(|s| s.path.as_str())
            .unwrap_or("");
        let rel = path.rsplit_once("/src/").map(|(_, r)| r).unwrap_or(path);
        rel.rsplit_once('/').map(|(dir, _)| dir.to_string()).unwrap_or_default()
    }

    // call after deserialization; serde(skip) leaves the indexes empty
    pub fn rebuild_index(&mut self) {
        self.contract_index.clear();
        self.id_index.clear();
        for (i, c) in self.contracts.iter().enumerate() {
            self.contract_index.insert(c.key().to_string(), i);
            if !c.qualified_id.is_empty() {
                self.contract_index.insert(c.qualified_id.clone(), i);
            }
            if let Some(id) = c.id {
                self.id_index.insert(id, i);
            }
        }
    }

    pub fn project_contracts(&self) -> impl Iterator<Item = &ContractDef> {
        self.contracts.iter().filter(|c| !c.is_dependency)
    }

    pub fn listed_contracts(&self) -> impl Iterator<Item = &ContractDef> {
        self.contracts.iter().filter(|c| c.is_listed())
    }

    pub fn hidden_contracts(&self) -> usize {
        self.contracts.len() - self.listed_contracts().count()
    }

    /// `None` when unknown.
    pub fn is_view_declaration(&self, id: DeclId) -> Option<bool> {
        let target = self.decl_table.lookup(id)?;
        Some(match target.kind {
            DeclKind::StateVariable => true,
            DeclKind::Function => self
                .contract_index
                .get(&target.contract)
                .and_then(|&i| self.contracts[i].functions.iter().find(|f| f.key == target.function))
                .is_some_and(|f| f.mutability.is_view_or_pure()),
            _ => false,
        })
    }

    /// A bare name selects the one project contract with it, or else the one dependency.
    pub fn lookup_contract(&self, query: &str) -> Result<&ContractDef, ContractLookupError> {
        if let Some(&i) = self.contract_index.get(query) {
            return Ok(&self.contracts[i]);
        }
        let named = |dependency: bool| -> Vec<&ContractDef> {
            self.contracts.iter().filter(|c| c.name == query && c.is_dependency == dependency).collect()
        };
        for candidates in [named(false), named(true)] {
            match candidates.as_slice() {
                [] => continue,
                [only] => return Ok(only),
                many => {
                    return Err(ContractLookupError::Ambiguous(
                        many.iter().map(|c| c.qualified_id.clone()).collect(),
                    ))
                }
            }
        }
        Err(ContractLookupError::NotFound)
    }

    pub fn default_contract(&self) -> Option<&ContractDef> {
        let key = pick_default_contract(
            self.listed_contracts().map(|c| (c.key(), c.kind, c.inherits.as_slice())),
        )?;
        self.contracts.iter().find(|c| c.key() == key)
    }

    pub fn find_contract(&self, filter: Option<&str>) -> Result<&ContractDef, String> {
        if let Some(name) = filter {
            return self.lookup_contract(name).map_err(|e| match e {
                ContractLookupError::NotFound => format!("Contract '{}' not found", name),
                ContractLookupError::Ambiguous(ids) => format!(
                    "Contract name '{}' is ambiguous; use one of: {}",
                    name,
                    ids.join(", ")
                ),
            });
        }

        let non_interface: Vec<&ContractDef> = self
            .project_contracts()
            .filter(|c| c.kind != ContractKind::Interface && !c.is_free_functions())
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
                        let names: Vec<&str> = non_interface.iter().map(|c| c.key()).collect();
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
    pub is_inherited: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionLookupError {
    NotFound,
    /// Carries the signatures.
    Ambiguous(Vec<String>),
}

impl Project {
    pub fn contract_by_id(&self, id: DeclId) -> Option<&ContractDef> {
        match self.id_index.get(&id) {
            Some(&i) => self.contracts.get(i),
            // hand-built models may skip `rebuild_index`
            None => self.contracts.iter().find(|c| c.id == Some(id)),
        }
    }

    /// By the declarations solc resolved, so aliased or same-named bases select the right contract; bases outside the model are omitted.
    pub fn base_keys(&self, contract: &ContractDef) -> Vec<String> {
        if contract.base_ids.is_empty() {
            contract.inherits.clone()
        } else {
            contract.base_ids.iter().filter_map(|id| self.contract_by_id(*id)).map(|c| c.key().to_string()).collect()
        }
    }

    /// Most derived first; models without `linearizedBaseContracts` fall back to the declared bases, right to left, breadth first.
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

    /// Starts after the contract keyed `after` (`super`); bodiless declarations only when nothing implements it.
    pub fn implementation<'a>(
        &self,
        linearization: &[&'a ContractDef],
        signature: &str,
        after: Option<&str>,
    ) -> Option<(&'a ContractDef, &'a FunctionDef)> {
        let start = match after {
            Some(key) => linearization.iter().position(|c| c.key() == key)? + 1,
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

    /// Own functions, then inherited non-private ones in linearization order; getter-implemented interface functions are not listed.
    pub fn accessible_functions<'a>(&'a self, contract: &'a ContractDef) -> Vec<AccessibleFunction<'a>> {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut result: Vec<AccessibleFunction<'a>> = Vec::new();
        let linearization = self.linearization(contract);
        let getters: HashSet<&str> = linearization
            .iter()
            .flat_map(|c| c.state_vars.iter().filter_map(|v| v.selector.as_deref()))
            .collect();

        for (depth, owner) in linearization.into_iter().enumerate() {
            let is_inherited = depth > 0;
            for f in &owner.functions {
                if f.name.is_empty() { continue; }
                if is_inherited
                    && (f.visibility == Visibility::Private || f.kind == FunctionKind::Constructor)
                {
                    continue;
                }
                if is_inherited
                    && f.body.is_none()
                    && f.selector.as_deref().is_some_and(|s| getters.contains(s))
                {
                    continue;
                }
                if seen.insert(f.match_signature()) {
                    result.push(AccessibleFunction {
                        function: f,
                        owner,
                        is_inherited,
                    });
                }
            }
        }

        result
    }

    pub fn lookup_function<'a>(
        &'a self,
        contract: &'a ContractDef,
        query: &str,
    ) -> Result<(&'a ContractDef, &'a FunctionDef), FunctionLookupError> {
        // internal signatures carry spaced locations (`f(uint256[] storage)`); compare unspaced
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

    /// Like `lookup_function`, but an ambiguous bare name picks the first overload.
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

    /// The name when it selects `function` on `contract`, otherwise the signature.
    pub fn route_key(&self, contract: &ContractDef, function: &FunctionDef) -> String {
        match self.lookup_function(contract, &function.name) {
            Ok((_, found)) if std::ptr::eq(found, function) => function.name.clone(),
            _ => function.match_signature().to_string(),
        }
    }

    /// Storage order, most base contract first, with the declaring contract's key and whether it is inherited.
    pub fn accessible_state_vars(&self, contract: &ContractDef) -> Vec<(StateVar, String, bool)> {
        self.linearization(contract)
            .into_iter()
            .rev()
            .flat_map(|c| {
                let inherited = !std::ptr::eq(c, contract);
                c.state_vars.iter().map(move |sv| (sv.clone(), c.key().to_string(), inherited))
            })
            .collect()
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

    /// Constants and immutables stay in place although they take no slot.
    pub fn inherited_state_vars(&self, contract: &ContractDef) -> Vec<StateVar> {
        self.accessible_state_vars(contract).into_iter().map(|(sv, _, _)| sv).collect()
    }
}

/// Deployable contracts first, then the most derived one (most ancestors), ties by name.
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
