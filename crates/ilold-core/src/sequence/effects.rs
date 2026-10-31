//! Effects of a function across the calls it makes, which run on the caller's storage.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::rc::Rc;

use petgraph::graph::DiGraph;
use petgraph::stable_graph::NodeIndex;

use crate::cfg::types::{BlockKind, CfgGraph, CfgStatement, StorageArg};
use crate::model::contract::ContractDef;
use crate::model::decl_id::DeclId;
use crate::model::expression::CallKind;
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::pathtree::walker::normalize_path;
use crate::util::{is_type_cast, push_unique};

use super::analysis::{EffectKind, EffectTrace};

/// A recursion passing `n.kids[i]` for `n` writes ever longer paths, so a cycle may not converge.
const MAX_ROUNDS: usize = 64;

#[derive(Debug, Clone)]
pub struct CallSite {
    pub call: String,
    pub kind: CallKind,
    pub declaration: Option<(String, String)>,
    pub storage_args: Vec<StorageArg>,
    pub modifier: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CfgSummary {
    pub state_writes: Vec<String>,
    pub state_reads: Vec<String>,
    pub param_writes: Vec<(usize, String)>,
    pub param_reads: Vec<(usize, String)>,
    pub external_calls: Vec<String>,
    /// All but staticcalls and calls to view or pure functions and getters.
    pub state_changing_calls: Vec<String>,
    pub events: Vec<String>,
    pub reverts: Vec<String>,
    pub calls: Vec<CallSite>,
    pub unresolved_modifiers: Vec<String>,
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

fn is_view_call(project: &Project, kind: CallKind, resolved: Option<DeclId>) -> bool {
    if kind == CallKind::LowLevelStaticcall {
        return true;
    }
    if !matches!(kind, CallKind::External | CallKind::ExternalSelf | CallKind::LibraryDelegatecall) {
        return false;
    }
    resolved.and_then(|id| project.is_view_declaration(id)).unwrap_or(false)
}

pub fn summarize(cfg: &CfgGraph, project: &Project) -> CfgSummary {
    let decls = &project.decl_table;
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
                    if !is_view_call(project, *kind, *resolved) {
                        push_unique(&mut s.state_changing_calls, call.clone());
                    }
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

/// `super.f` continues after `owner` in C3 order and a virtual call reaches the visible override.
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

/// Of several call chains reaching the same effect, the shortest is kept.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Effects {
    pub state_writes: BTreeMap<String, Vec<String>>,
    pub state_reads: BTreeMap<String, Vec<String>>,
    pub param_writes: BTreeMap<(usize, String), Vec<String>>,
    pub param_reads: BTreeMap<(usize, String), Vec<String>>,
    pub external_calls: BTreeMap<String, Vec<String>>,
    pub state_changing_calls: BTreeSet<String>,
    pub events: BTreeMap<String, Vec<String>>,
    pub reverts: BTreeMap<String, Vec<String>>,
    pub internal_calls: BTreeSet<String>,
    /// A cycle stopped at `MAX_ROUNDS` before converging.
    pub incomplete: bool,
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
        e.state_changing_calls.extend(summary.state_changing_calls.iter().cloned());
        for ev in &summary.events { insert(&mut e.events, ev.clone(), none()); }
        for r in &summary.reverts { insert(&mut e.reverts, r.clone(), none()); }
        e
    }

    fn lift(&mut self, callee_effects: &Effects, head: &[String], call: &CallSite, callee: &FunctionDef, shares_storage: bool) {
        let via = |v: &Vec<String>| head.iter().chain(v).cloned().collect::<Vec<_>>();
        // a library has no storage of its own: its state paths are constants
        if shares_storage {
            for (p, v) in &callee_effects.state_writes { insert(&mut self.state_writes, p.clone(), via(v)); }
            for (p, v) in &callee_effects.state_reads { insert(&mut self.state_reads, p.clone(), via(v)); }
        }
        let bind = |(i, p): &(usize, String)| -> Vec<(Option<usize>, String)> {
            let Some(suffix) = callee.params.get(*i).and_then(|param| p.strip_prefix(param.name.as_str())) else {
                return Vec::new();
            };
            call.storage_args
                .iter()
                .filter(|a| a.param == *i)
                .map(|arg| (arg.place.param, format!("{}{suffix}", normalize_path(&arg.place.path))))
                .collect()
        };
        for (writes, params, effects) in [
            (&mut self.state_writes, &mut self.param_writes, &callee_effects.param_writes),
            (&mut self.state_reads, &mut self.param_reads, &callee_effects.param_reads),
        ] {
            for (key, v) in effects {
                for bound in bind(key) {
                    match bound {
                        (Some(j), path) => insert(params, (j, path), via(v)),
                        (None, path) => insert(writes, path, via(v)),
                    }
                }
            }
        }
        for (c, v) in &callee_effects.external_calls { insert(&mut self.external_calls, c.clone(), via(v)); }
        self.state_changing_calls.extend(callee_effects.state_changing_calls.iter().cloned());
        for (e, v) in &callee_effects.events { insert(&mut self.events, e.clone(), via(v)); }
        for (r, v) in &callee_effects.reverts { insert(&mut self.reverts, r.clone(), via(v)); }
        self.incomplete |= callee_effects.incomplete;
        self.internal_calls.insert(call.call.clone());
        self.internal_calls.extend(callee_effects.internal_calls.iter().cloned());
    }

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

/// (context contract, defining contract, function key)
type EffectKey = (String, String, String);

struct Pending<'a> {
    summary: Option<&'a CfgSummary>,
    calls: Vec<(usize, EffectKey, &'a ContractDef, &'a FunctionDef, bool)>,
}

/// Per strongly connected component, so every member of a cycle gets all of its effects.
pub struct EffectEngine<'a> {
    project: &'a Project,
    summaries: &'a HashMap<(String, String), CfgSummary>,
    /// Inherited functions whose modifiers the context contract overrides.
    context_summaries: &'a HashMap<EffectKey, CfgSummary>,
    memo: HashMap<EffectKey, Rc<Effects>>,
}

impl<'a> EffectEngine<'a> {
    pub fn new(
        project: &'a Project,
        summaries: &'a HashMap<(String, String), CfgSummary>,
        context_summaries: &'a HashMap<EffectKey, CfgSummary>,
    ) -> Self {
        Self { project, summaries, context_summaries, memo: HashMap::new() }
    }

    pub fn summary(&self, context: &ContractDef, owner: &ContractDef, func: &FunctionDef) -> Option<&'a CfgSummary> {
        let key = (context.key().to_string(), owner.key().to_string(), func.key.clone());
        self.context_summaries
            .get(&key)
            .or_else(|| self.summaries.get(&(key.1, key.2)))
    }

    pub fn effects(&mut self, context: &'a ContractDef, owner: &'a ContractDef, func: &'a FunctionDef) -> Rc<Effects> {
        let root = (context.key().to_string(), owner.key().to_string(), func.key.clone());
        if let Some(found) = self.memo.get(&root) {
            return Rc::clone(found);
        }

        let project = self.project;
        let mut graph: DiGraph<EffectKey, ()> = DiGraph::new();
        let mut index: HashMap<EffectKey, petgraph::graph::NodeIndex> = HashMap::new();
        let mut pending: Vec<Pending<'a>> = Vec::new();
        let mut stack = vec![(root.clone(), context, owner, func)];
        index.insert(root.clone(), graph.add_node(root.clone()));
        while let Some((key, context, owner, func)) = stack.pop() {
            let summary = self.summary(context, owner, func);
            let mut calls = Vec::new();
            for (i, call) in summary.map(|s| s.calls.as_slice()).unwrap_or_default().iter().enumerate() {
                let Some((callee_owner, callee)) = resolve_call(project, context, owner, call) else { continue };
                let shares_storage = project.linearization(context).iter().any(|c| c.key() == callee_owner.key());
                let callee_context = if shares_storage { context } else { callee_owner };
                let callee_key = (callee_context.key().to_string(), callee_owner.key().to_string(), callee.key.clone());
                if !self.memo.contains_key(&callee_key) {
                    let to = match index.get(&callee_key) {
                        Some(&n) => n,
                        None => {
                            let n = graph.add_node(callee_key.clone());
                            index.insert(callee_key.clone(), n);
                            stack.push((callee_key.clone(), callee_context, callee_owner, callee));
                            n
                        }
                    };
                    graph.add_edge(index[&key], to, ());
                }
                calls.push((i, callee_key, callee_owner, callee, shares_storage));
            }
            let n = index[&key].index();
            if pending.len() <= n {
                pending.resize_with(n + 1, || Pending { summary: None, calls: Vec::new() });
            }
            pending[n] = Pending { summary, calls };
        }

        // tarjan_scc yields callees first
        for scc in petgraph::algo::tarjan_scc(&graph) {
            let mut current: HashMap<EffectKey, Effects> = HashMap::new();
            let mut converged = false;
            for _ in 0..MAX_ROUNDS {
                let mut changed = false;
                for n in &scc {
                    let next = self.lift_calls(&pending[n.index()], &current);
                    let key = &graph[*n];
                    if current.get(key) != Some(&next) {
                        current.insert(key.clone(), next);
                        changed = true;
                    }
                }
                let cyclic = scc.len() > 1 || graph.contains_edge(scc[0], scc[0]);
                if !changed || !cyclic {
                    converged = true;
                    break;
                }
            }
            for (key, mut effects) in current {
                effects.incomplete |= !converged;
                self.memo.insert(key, Rc::new(effects));
            }
        }
        self.memo.get(&root).cloned().unwrap_or_default()
    }

    fn lift_calls(&self, pending: &Pending<'a>, cycle: &HashMap<EffectKey, Effects>) -> Effects {
        let Some(summary) = pending.summary else { return Effects::default() };
        let mut effects = Effects::direct(summary);
        let empty = Effects::default();
        for (i, callee_key, callee_owner, callee, shares_storage) in &pending.calls {
            let call = &summary.calls[*i];
            let callee_effects = match cycle.get(callee_key) {
                Some(e) => e,
                None => self.memo.get(callee_key).map(|e| e.as_ref()).unwrap_or(&empty),
            };
            let mut head = Vec::with_capacity(2);
            if let Some(modifier) = &call.modifier {
                head.push(format!("modifier {modifier}"));
            }
            head.push(format!("{}::{}", callee_owner.key(), callee.key));
            effects.lift(callee_effects, &head, call, callee, *shares_storage);
        }
        effects
    }
}
