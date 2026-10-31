//! Effects of a function across the calls it makes. Each function's CFG is summarised once;
//! effects then flow from callees to callers: internal calls, internal library calls and
//! library delegatecalls all run on the caller's storage, so their writes through `storage`
//! parameters land on whatever the caller bound to those parameters.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::rc::Rc;

use petgraph::stable_graph::NodeIndex;

use crate::cfg::types::{BlockKind, CfgGraph, CfgStatement, StorageArg};
use crate::model::contract::ContractDef;
use crate::model::decl_id::DeclTable;
use crate::model::expression::CallKind;
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::pathtree::walker::normalize_path;
use crate::util::is_type_cast;

use super::analysis::{EffectKind, EffectTrace};

/// Calls deeper than this contribute nothing further; recursion stops at the first repeat.
const MAX_CALL_DEPTH: usize = 24;

/// A call that runs code on the caller's storage.
#[derive(Debug, Clone)]
pub struct CallSite {
    /// As written: `f`, `super.f`, `Base.f`, `Library.f`.
    pub call: String,
    pub kind: CallKind,
    /// (contract key, function key) of the declaration solc resolved.
    pub declaration: Option<(String, String)>,
    pub storage_args: Vec<StorageArg>,
    /// The modifier whose body makes the call.
    pub modifier: Option<String>,
}

/// What one function does by itself, over every block reachable from its entry.
#[derive(Debug, Clone, Default)]
pub struct CfgSummary {
    /// Normalised paths rooted at state variables.
    pub state_writes: Vec<String>,
    pub state_reads: Vec<String>,
    /// (parameter index, normalised path rooted at the parameter name).
    pub param_writes: Vec<(usize, String)>,
    pub param_reads: Vec<(usize, String)>,
    pub external_calls: Vec<String>,
    pub events: Vec<String>,
    pub reverts: Vec<String>,
    pub calls: Vec<CallSite>,
    pub unresolved_modifiers: Vec<String>,
}

fn push_unique<T: PartialEq>(list: &mut Vec<T>, item: T) {
    if !list.contains(&item) {
        list.push(item);
    }
}

fn reachable(cfg: &CfgGraph) -> Vec<NodeIndex> {
    let Some(entry) = cfg.node_indices().find(|&n| cfg[n].kind == BlockKind::Entry) else {
        return Vec::new();
    };
    let mut seen = HashSet::from([entry]);
    let mut queue = VecDeque::from([entry]);
    let mut order = Vec::new();
    while let Some(n) = queue.pop_front() {
        order.push(n);
        for next in cfg.neighbors(n) {
            if seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    order.sort_by_key(|n| cfg[*n].id);
    order
}

pub fn summarize(cfg: &CfgGraph, decls: &DeclTable) -> CfgSummary {
    let mut s = CfgSummary::default();
    let write = |s: &mut CfgSummary, path: &str, param: Option<usize>| match param {
        Some(i) => push_unique(&mut s.param_writes, (i, normalize_path(path))),
        None => push_unique(&mut s.state_writes, normalize_path(path)),
    };
    for n in reachable(cfg) {
        let block = &cfg[n];
        for read in &block.reads {
            match read.param {
                Some(i) => push_unique(&mut s.param_reads, (i, normalize_path(&read.path))),
                None => push_unique(&mut s.state_reads, normalize_path(&read.path)),
            }
        }
        if let Some(reason) = &block.revert_reason {
            push_unique(&mut s.reverts, reason.clone());
        }
        for stmt in &block.statements {
            match stmt {
                CfgStatement::Assignment { storage: Some(place), .. } => write(&mut s, &place.path, place.param),
                CfgStatement::StateWrite { variable, param, .. } => write(&mut s, variable, *param),
                CfgStatement::ExternalCall { target, function, kind, resolved, storage_args, from_modifier, .. } => {
                    let call = format!("{target}.{function}");
                    push_unique(&mut s.external_calls, call.clone());
                    if *kind == CallKind::LibraryDelegatecall {
                        s.calls.push(CallSite {
                            call,
                            kind: *kind,
                            declaration: resolved
                                .and_then(|id| decls.function(id))
                                .map(|t| (t.contract.clone(), t.function.clone())),
                            storage_args: storage_args.clone(),
                            modifier: from_modifier.clone(),
                        });
                    }
                }
                CfgStatement::InternalCall { function, target, kind, declaration, storage_args, from_modifier, .. } => {
                    s.calls.push(CallSite {
                        call: match target {
                            Some(target) => format!("{target}.{function}"),
                            None => function.clone(),
                        },
                        kind: *kind,
                        declaration: declaration.clone(),
                        storage_args: storage_args.clone(),
                        modifier: from_modifier.clone(),
                    });
                }
                CfgStatement::EmitEvent { event, .. } => push_unique(&mut s.events, event.clone()),
                CfgStatement::UnresolvedModifier { name, .. } => push_unique(&mut s.unresolved_modifiers, name.clone()),
                _ => {}
            }
        }
    }
    s
}

/// The implementation a call reaches when `context` is the most derived contract: `super.f`
/// continues after `owner` in the C3 order, an unqualified call to a virtual function reaches
/// the visible override, anything else is the declaration solc resolved. Without a
/// declaration, falls back to names.
pub fn resolve_call<'a>(
    project: &'a Project,
    context: &'a ContractDef,
    owner: &'a ContractDef,
    call: &CallSite,
) -> Option<(&'a ContractDef, &'a FunctionDef)> {
    let linearization = project.linearization(context);
    let in_context = linearization.iter().any(|c| c.key() == owner.key());
    let order = || if in_context { project.linearization(context) } else { project.linearization(owner) };
    let is_super = call.call.starts_with("super.");

    let Some((contract, key)) = &call.declaration else {
        if is_type_cast(&call.call) {
            return None;
        }
        let with_body = |c: &'a ContractDef, name: &str| {
            c.functions.iter().find(|f| f.name == name && f.body.is_some()).map(|f| (c, f))
        };
        if let Some(name) = call.call.strip_prefix("super.") {
            let order = order();
            let start = order.iter().position(|c| c.key() == owner.key())? + 1;
            return order[start..].iter().find_map(|c| with_body(c, name));
        }
        if let Some((qualifier, name)) = call.call.split_once('.') {
            return with_body(project.lookup_contract(qualifier).ok()?, name);
        }
        return order().into_iter().find_map(|c| with_body(c, &call.call));
    };
    let declared_in = &project.contracts[*project.contract_index.get(contract)?];
    let declared = declared_in.functions.iter().find(|f| &f.key == key)?;
    let signature = declared.match_signature();
    let implementation = if is_super {
        project.implementation(&order(), signature, Some(owner.key()))
    } else if call.kind == CallKind::Internal
        && !call.call.contains('.')
        && declared.is_virtual
        && linearization.iter().any(|c| c.key() == declared_in.key())
    {
        project.implementation(&linearization, signature, None)
    } else {
        None
    };
    Some(implementation.unwrap_or((declared_in, declared)))
}

/// Effects of one function with the call chain (`via`) reaching each; direct effects have
/// an empty chain. Of several chains reaching the same effect, the shortest is kept.
#[derive(Debug, Clone, Default)]
pub struct Effects {
    pub state_writes: BTreeMap<String, Vec<String>>,
    pub state_reads: BTreeMap<String, Vec<String>>,
    pub param_writes: BTreeMap<(usize, String), Vec<String>>,
    pub param_reads: BTreeMap<(usize, String), Vec<String>>,
    pub external_calls: BTreeMap<String, Vec<String>>,
    pub events: BTreeMap<String, Vec<String>>,
    pub reverts: BTreeMap<String, Vec<String>>,
    /// Every callee reached, as written at its call site.
    pub internal_calls: BTreeSet<String>,
}

fn insert<K: Ord>(map: &mut BTreeMap<K, Vec<String>>, key: K, via: Vec<String>) {
    match map.get(&key) {
        Some(existing) if existing.len() <= via.len() => {}
        _ => {
            map.insert(key, via);
        }
    }
}

impl Effects {
    fn direct(summary: &CfgSummary) -> Self {
        let mut e = Effects::default();
        let none = Vec::new;
        for p in &summary.state_writes { insert(&mut e.state_writes, p.clone(), none()); }
        for p in &summary.state_reads { insert(&mut e.state_reads, p.clone(), none()); }
        for p in &summary.param_writes { insert(&mut e.param_writes, p.clone(), none()); }
        for p in &summary.param_reads { insert(&mut e.param_reads, p.clone(), none()); }
        for c in &summary.external_calls { insert(&mut e.external_calls, c.clone(), none()); }
        for ev in &summary.events { insert(&mut e.events, ev.clone(), none()); }
        for r in &summary.reverts { insert(&mut e.reverts, r.clone(), none()); }
        e
    }

    // Adds a callee's effects as seen from the call site.
    fn lift(&mut self, callee_effects: &Effects, head: &[String], call: &CallSite, callee: &FunctionDef, shares_storage: bool) {
        let via = |v: &Vec<String>| head.iter().chain(v).cloned().collect::<Vec<_>>();
        // a library has no storage of its own: its state paths are constants
        if shares_storage {
            for (p, v) in &callee_effects.state_writes { insert(&mut self.state_writes, p.clone(), via(v)); }
            for (p, v) in &callee_effects.state_reads { insert(&mut self.state_reads, p.clone(), via(v)); }
        }
        let bind = |(i, p): &(usize, String)| -> Option<(Option<usize>, String)> {
            let arg = call.storage_args.iter().find(|a| a.param == *i)?;
            let suffix = p.strip_prefix(callee.params.get(*i)?.name.as_str())?;
            Some((arg.place.param, format!("{}{suffix}", normalize_path(&arg.place.path))))
        };
        for (writes, params, effects) in [
            (&mut self.state_writes, &mut self.param_writes, &callee_effects.param_writes),
            (&mut self.state_reads, &mut self.param_reads, &callee_effects.param_reads),
        ] {
            for (key, v) in effects {
                match bind(key) {
                    Some((Some(j), path)) => insert(params, (j, path), via(v)),
                    Some((None, path)) => insert(writes, path, via(v)),
                    None => {}
                }
            }
        }
        for (c, v) in &callee_effects.external_calls { insert(&mut self.external_calls, c.clone(), via(v)); }
        for (e, v) in &callee_effects.events { insert(&mut self.events, e.clone(), via(v)); }
        for (r, v) in &callee_effects.reverts { insert(&mut self.reverts, r.clone(), via(v)); }
        self.internal_calls.insert(call.call.clone());
        self.internal_calls.extend(callee_effects.internal_calls.iter().cloned());
    }

    /// Effects reached through calls, each with its chain.
    pub fn traces(&self) -> Vec<EffectTrace> {
        let mut out = Vec::new();
        let mut push = |kind: EffectKind, item: &str, via: &Vec<String>| {
            if !via.is_empty() {
                out.push(EffectTrace { kind, item: item.to_string(), via: via.clone() });
            }
        };
        for (p, v) in &self.state_writes { push(EffectKind::StateWrite, p, v); }
        for (p, v) in &self.state_reads { push(EffectKind::StateRead, p, v); }
        for ((_, p), v) in &self.param_writes { push(EffectKind::StorageParamWrite, p, v); }
        for ((_, p), v) in &self.param_reads { push(EffectKind::StorageParamRead, p, v); }
        for (c, v) in &self.external_calls { push(EffectKind::ExternalCall, c, v); }
        for (e, v) in &self.events { push(EffectKind::Event, e, v); }
        for (r, v) in &self.reverts { push(EffectKind::Revert, r, v); }
        out
    }
}

/// Memoised effects per (context contract, defining contract, function key).
pub struct EffectEngine<'a> {
    project: &'a Project,
    summaries: &'a HashMap<(String, String), CfgSummary>,
    memo: HashMap<(String, String, String), Rc<Effects>>,
    active: HashSet<(String, String, String)>,
}

impl<'a> EffectEngine<'a> {
    pub fn new(project: &'a Project, summaries: &'a HashMap<(String, String), CfgSummary>) -> Self {
        Self { project, summaries, memo: HashMap::new(), active: HashSet::new() }
    }

    /// Effects of `func` (declared in `owner`) when `context` is the most derived contract.
    pub fn effects(&mut self, context: &'a ContractDef, owner: &'a ContractDef, func: &'a FunctionDef) -> Rc<Effects> {
        self.effects_at(context, owner, func, 0)
    }

    fn effects_at(&mut self, context: &'a ContractDef, owner: &'a ContractDef, func: &'a FunctionDef, depth: usize) -> Rc<Effects> {
        let key = (context.key().to_string(), owner.key().to_string(), func.key.clone());
        if let Some(found) = self.memo.get(&key) {
            return Rc::clone(found);
        }
        let Some(summary) = self.summaries.get(&(key.1.clone(), key.2.clone())) else {
            return Rc::new(Effects::default());
        };
        let mut effects = Effects::direct(summary);
        // a recursive call or an over-deep chain adds nothing beyond what is already counted
        if depth >= MAX_CALL_DEPTH || !self.active.insert(key.clone()) {
            return Rc::new(effects);
        }
        let project = self.project;
        for call in &summary.calls {
            let Some((callee_owner, callee)) = resolve_call(project, context, owner, call) else { continue };
            let shares_storage = project.linearization(context).iter().any(|c| c.key() == callee_owner.key());
            let callee_context = if shares_storage { context } else { callee_owner };
            let callee_effects = self.effects_at(callee_context, callee_owner, callee, depth + 1);
            let mut head = Vec::with_capacity(2);
            if let Some(modifier) = &call.modifier {
                head.push(format!("modifier {modifier}"));
            }
            head.push(format!("{}::{}", callee_owner.key(), callee.key));
            effects.lift(&callee_effects, &head, call, callee, shares_storage);
        }
        self.active.remove(&key);
        let effects = Rc::new(effects);
        self.memo.insert(key, Rc::clone(&effects));
        effects
    }
}
