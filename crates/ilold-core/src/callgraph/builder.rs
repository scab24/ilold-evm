use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::stable_graph::NodeIndex;

use crate::cfg::builder::CfgBuilder;
use crate::cfg::types::CfgStatement;
use crate::model::contract::ContractDef;
use crate::model::decl_id::DeclId;
use crate::model::function::{FunctionDef, FunctionKind, Mutability, Visibility};
use crate::model::project::Project;
use crate::util::is_type_cast;

use super::types::*;

/// Call graph of `contract` as the most derived contract: every function visible from it
/// (own and inherited, overrides applied), every constructor of its linearization, plus the
/// base implementations reached through `super`. Targets come from solc declaration ids and the C3 linearization; names are
/// only a fallback for models without them.
pub fn build_call_graph(project: &Project, contract: &ContractDef) -> CallGraph {
    let linearization = project.linearization(contract);
    let mut builder = Builder {
        project,
        linearization,
        graph: CallGraph::new(),
        index: HashMap::new(),
        queue: VecDeque::new(),
        queued: HashSet::new(),
    };

    let visible: Vec<(&ContractDef, &FunctionDef)> = builder
        .linearization
        .iter()
        .flat_map(|c| c.functions.iter().map(move |f| (*c, f)))
        .filter(|(c, f)| {
            // constructors are not inherited or overridden; each base constructor runs
            f.kind == FunctionKind::Constructor
                || builder
                    .project
                    .implementation(&builder.linearization, f.match_signature(), None)
                    .is_some_and(|(ic, i_f)| std::ptr::eq(ic, *c) && std::ptr::eq(i_f, *f))
        })
        .collect();
    for (c, f) in visible {
        builder.function_node(c, f);
    }

    while let Some((owner, func, from)) = builder.queue.pop_front() {
        builder.add_calls(owner, func, from);
    }

    builder.graph
}

struct Builder<'a> {
    project: &'a Project,
    linearization: Vec<&'a ContractDef>,
    graph: CallGraph,
    index: HashMap<String, NodeIndex>,
    // hierarchy functions whose bodies still have to be scanned
    queue: VecDeque<(&'a ContractDef, &'a FunctionDef, NodeIndex)>,
    queued: HashSet<String>,
}

// A callee before it becomes a node.
struct Target<'a> {
    contract: String,
    key: String,
    function: String,
    def: Option<(&'a ContractDef, &'a FunctionDef)>,
    static_target: Option<String>,
}

impl<'a> Builder<'a> {
    fn in_hierarchy(&self, contract: &str) -> bool {
        self.linearization.iter().any(|c| c.name == contract)
    }

    fn model_function(&self, contract: &str, key: &str) -> Option<(&'a ContractDef, &'a FunctionDef)> {
        let project = self.project;
        let owner = &project.contracts[*project.contract_index.get(contract)?];
        let func = owner.functions.iter().find(|f| f.key == key)?;
        Some((owner, func))
    }

    fn declared(&self, resolved: Option<DeclId>) -> Option<(String, String)> {
        let target = self.project.decl_table.lookup(resolved?)?;
        (!target.function.is_empty()).then(|| (target.contract.clone(), target.function.clone()))
    }

    fn function_node(&mut self, owner: &'a ContractDef, func: &'a FunctionDef) -> NodeIndex {
        let target = Target {
            contract: owner.name.clone(),
            key: func.key.clone(),
            function: func.name.clone(),
            def: Some((owner, func)),
            static_target: None,
        };
        self.node(&target, Visibility::Internal)
    }

    fn node(&mut self, target: &Target<'a>, default_visibility: Visibility) -> NodeIndex {
        let key = if target.key.is_empty() { &target.function } else { &target.key };
        let id = format!("{}::{}", target.contract, key);
        let idx = match self.index.get(&id) {
            Some(&idx) => idx,
            None => {
                let (visibility, mutability) = target
                    .def
                    .map(|(_, f)| (f.visibility, f.mutability))
                    .unwrap_or((default_visibility, Mutability::NonPayable));
                let idx = self.graph.add_node(CallNode {
                    contract: target.contract.clone(),
                    function: target.function.clone(),
                    key: key.clone(),
                    visibility,
                    mutability,
                    is_external: !self.in_hierarchy(&target.contract),
                });
                self.index.insert(id.clone(), idx);
                idx
            }
        };
        // bodies of hierarchy functions are part of this contract's code
        if let Some((owner, func)) = target.def {
            if self.in_hierarchy(&owner.name) && self.queued.insert(id) {
                self.queue.push_back((owner, func, idx));
            }
        }
        idx
    }

    fn add_calls(&mut self, owner: &'a ContractDef, func: &'a FunctionDef, from: NodeIndex) {
        // with the project, inherited modifiers are inlined too
        let Ok(cfg) = CfgBuilder::build_with_project(func, owner, Some(self.project)) else {
            return;
        };
        for block in cfg.node_weights() {
            for stmt in &block.statements {
                let (target, kind, visibility) = match stmt {
                    CfgStatement::InternalCall { function, kind, target, resolved, .. } => {
                        if is_type_cast(function) && resolved.is_none() {
                            continue;
                        }
                        let callee = self.internal_target(owner, function, *kind, target.as_deref(), *resolved);
                        (callee, *kind, Visibility::Internal)
                    }
                    CfgStatement::ExternalCall { target, function, kind, resolved, contract, .. } => {
                        if resolved.is_none()
                            && *kind == CallKind::External
                            && (is_type_cast(function) || is_type_cast(target))
                        {
                            continue;
                        }
                        let callee = self.external_target(target, function, *kind, *resolved, contract.as_deref());
                        (callee, *kind, Visibility::External)
                    }
                    _ => continue,
                };
                let to = self.node(&target, visibility);
                add_or_increment_edge(&mut self.graph, from, to, kind, target.static_target);
            }
        }
    }

    fn internal_target(
        &self,
        owner: &'a ContractDef,
        function: &str,
        kind: CallKind,
        qualifier: Option<&str>,
        resolved: Option<DeclId>,
    ) -> Target<'a> {
        let declared = self.declared(resolved);
        let declared_def = declared.as_ref().and_then(|(c, k)| self.model_function(c, k));
        let signature = declared_def.map(|(_, f)| f.match_signature().to_string());

        if kind == CallKind::Internal && qualifier == Some("super") {
            // the next implementation after the calling contract, in the most derived C3 order
            let found = self.project.implementation(
                &self.linearization,
                signature.as_deref().unwrap_or(function),
                Some(&owner.name),
            );
            if let Some((c, f)) = found {
                return self.defined(c, f, None);
            }
        } else if kind == CallKind::Internal && qualifier.is_none() {
            // virtual dispatch from the most derived contract
            match declared_def {
                Some((c, f)) if f.is_virtual && self.in_hierarchy(&c.name) => {
                    if let Some((ic, i_f)) =
                        self.project.implementation(&self.linearization, f.match_signature(), None)
                    {
                        let moved = !(std::ptr::eq(ic, c) && std::ptr::eq(i_f, f));
                        let static_target = moved.then(|| format!("{}::{}", c.name, f.key));
                        return self.defined(ic, i_f, static_target);
                    }
                }
                // no declaration id at all: first implementation by name. An id that is not a
                // function (a function-pointer variable) must not be matched by name.
                None if resolved.is_none() => {
                    let by_name = self.linearization.iter().find_map(|c| {
                        c.functions.iter().find(|f| f.name == function && f.body.is_some()).map(|f| (*c, f))
                    });
                    if let Some((c, f)) = by_name {
                        return self.defined(c, f, None);
                    }
                }
                _ => {}
            }
        }

        match (declared_def, declared) {
            (Some((c, f)), _) => self.defined(c, f, None),
            (None, Some((contract, key))) => Target {
                function: function.to_string(),
                contract,
                key,
                def: None,
                static_target: None,
            },
            (None, None) => Target {
                contract: match (kind, qualifier) {
                    (CallKind::InternalLibrary, Some(lib)) => lib.to_string(),
                    _ => "?".into(),
                },
                key: function.to_string(),
                function: function.to_string(),
                def: None,
                static_target: None,
            },
        }
    }

    fn external_target(
        &self,
        target: &str,
        function: &str,
        kind: CallKind,
        resolved: Option<DeclId>,
        contract: Option<&str>,
    ) -> Target<'a> {
        let unresolved = |contract: String, key: String| Target {
            contract,
            function: function.to_string(),
            key,
            def: None,
            static_target: None,
        };
        match kind {
            CallKind::LowLevelCall
            | CallKind::LowLevelDelegatecall
            | CallKind::LowLevelStaticcall
            | CallKind::Transfer
            | CallKind::Send => unresolved("address".into(), function.to_string()),
            CallKind::Creation => {
                let created = contract.unwrap_or(target).to_string();
                match self.model_function(&created, "constructor") {
                    Some((c, f)) => self.defined(c, f, None),
                    None => unresolved(created, "constructor".into()),
                }
            }
            _ => {
                let declared = self.declared(resolved);
                if kind == CallKind::ExternalSelf {
                    // `this.f()` runs the implementation visible from the most derived contract
                    let signature = declared
                        .as_ref()
                        .and_then(|(c, k)| self.model_function(c, k))
                        .map(|(_, f)| f.match_signature().to_string());
                    let found = match &signature {
                        Some(sig) => self.project.implementation(&self.linearization, sig, None),
                        None => self.linearization.iter().find_map(|c| {
                            c.functions.iter().find(|f| f.name == function).map(|f| (*c, f))
                        }),
                    };
                    if let Some((c, f)) = found {
                        return self.defined(c, f, None);
                    }
                }
                match declared {
                    Some((c, k)) => match self.model_function(&c, &k) {
                        Some((oc, of)) => self.defined(oc, of, None),
                        None => unresolved(c, k),
                    },
                    None => unresolved(contract.unwrap_or(target).to_string(), function.to_string()),
                }
            }
        }
    }

    fn defined(&self, owner: &'a ContractDef, func: &'a FunctionDef, static_target: Option<String>) -> Target<'a> {
        Target {
            contract: owner.name.clone(),
            key: func.key.clone(),
            function: func.name.clone(),
            def: Some((owner, func)),
            static_target,
        }
    }
}

fn add_or_increment_edge(
    graph: &mut CallGraph,
    from: NodeIndex,
    to: NodeIndex,
    kind: CallKind,
    static_target: Option<String>,
) {
    if let Some(edge) = graph.find_edge(from, to) {
        graph[edge].call_count += 1;
    } else {
        graph.add_edge(from, to, CallEdge { kind, call_count: 1, static_target });
    }
}
