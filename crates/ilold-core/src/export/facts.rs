//! Nodes for declarations and edges for what each function does in every contract that runs it.
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;
use serde_json::{json, Value};

use crate::analysis::{function_behavior, overrides_modifiers, ProjectAnalysis};
use crate::cfg::builder::{modifier_code, CfgBuilder};
use crate::cfg::types::{CfgGraph, CfgStatement};
use crate::classify::guards::{GuardFact, GuardScope, GuardTerm};
use crate::model::common::{Param, SourceSpan};
use crate::model::contract::ContractDef;
use crate::model::decl_id::{DeclId, DeclKind};
use crate::model::expression::CallKind;
use crate::model::function::{FunctionDef, Visibility};
use crate::model::project::Project;
use crate::model::source::ValueSource;
use crate::parse::cache::digest;
use crate::pathtree::walker::normalize_path;
use crate::relations::{
    call_target, state_var_ref, storage_accesses, try_catches, value_facts, AccessKey, AccessKind, ContractLink, EffectSite, KeySite, RecordKeys,
    TryCatchFact, ValueTransfer,
};
use crate::sequence::analysis::{find_behavior, EffectKind, FunctionBehavior};
use crate::sequence::effects::{reachable, resolve_call, summarize, CallSite, EffectEngine};
use crate::util::target_base_name;

use super::ids::{param_types, Ids};

const EXTRACTED: &str = "EXTRACTED";
const TYPE_RESOLVED: &str = "TYPE_RESOLVED";
const INFERRED: &str = "INFERRED";
const AMBIGUOUS: &str = "AMBIGUOUS";

/// Fields added to a kind after schema 1.0, as JSON pointers; ids leave them out so they stay what 1.0 gave.
const ADDED_FIELDS: &[(&str, &[&str])] = &[
    ("calls", &["/dispatch_candidates", "/unresolved_reason", "/target_source/assigned_at", "/in_try"]),
    ("reads", KEY_FIELDS),
    ("writes", KEY_FIELDS),
];
const KEY_FIELDS: &[&str] = &["/keys", "/keys_reason", "/key_sites"];

/// The record as schema 1.0 wrote its kind.
pub fn as_1_0(record: &Value) -> Value {
    let mut v1_0 = record.clone();
    let kind = record["kind"].as_str().unwrap_or_default();
    for pointer in ADDED_FIELDS.iter().filter(|(k, _)| *k == kind).flat_map(|(_, fields)| fields.iter()) {
        let (parent, field) = pointer.rsplit_once('/').unwrap_or_default();
        if let Some(object) = v1_0.pointer_mut(parent).and_then(Value::as_object_mut) {
            object.remove(field);
        }
    }
    v1_0
}

/// A serde variant name in snake case: `LowLevelCall` is `low_level_call`.
fn snake(value: impl Serialize) -> String {
    let name = serde_json::to_value(value).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let mut out = String::with_capacity(name.len() + 4);
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(ch.to_lowercase());
    }
    out
}

/// `context` runs `func`, declared in `owner`.
struct Unit<'p> {
    context: &'p ContractDef,
    owner: &'p ContractDef,
    func: &'p FunctionDef,
    behavior: Cow<'p, FunctionBehavior>,
}

#[derive(Default)]
pub(super) struct Truncation {
    pub effects_incomplete: usize,
    pub reads_incomplete: usize,
    pub reverts_incomplete: usize,
    pub guards_incomplete: usize,
}

pub(super) struct Graph {
    pub nodes: BTreeMap<String, Value>,
    pub edges: BTreeMap<String, Value>,
    pub truncation: Truncation,
}

pub(super) struct Builder<'p> {
    project: &'p Project,
    analysis: &'p ProjectAnalysis,
    ids: Ids<'p>,
    /// Distinct facts by id without its occurrence index.
    edges: BTreeMap<String, Vec<Pending>>,
    /// Per function id, its flags in each context.
    flags: BTreeMap<String, BTreeMap<String, Value>>,
    /// Try statements wait for the ids of the calls they guard.
    tries: Vec<AwaitingCalls<TryCatchFact>>,
    /// Value transfers wait for the ids of the calls that make them.
    transfers: Vec<AwaitingCalls<ValueTransfer>>,
    truncation: Truncation,
}

struct AwaitingCalls<F> {
    function: String,
    context_id: Option<String>,
    via: Vec<Value>,
    fact: F,
}

/// A fact about `function`, declared in `owner`, as `context` runs it.
struct Site<'u> {
    function: &'u str,
    owner: &'u ContractDef,
    context: &'u ContractDef,
    context_id: Option<&'u str>,
}

/// A fact before numbering; a direct read or write also collects the key sets it occurs with.
struct Pending {
    record: Value,
    key_sites: Option<Vec<KeySite>>,
}

/// Where a fact is, the chain that reaches it and the pass that found it.
#[derive(Clone)]
struct Origin<'r> {
    span: SourceSpan,
    via: Vec<Value>,
    rule: &'r str,
}

impl<'p> Builder<'p> {
    pub fn new(project: &'p Project, analysis: &'p ProjectAnalysis, ids: Ids<'p>) -> Self {
        Self { project, analysis, ids, edges: BTreeMap::new(), flags: BTreeMap::new(), tries: Vec::new(), transfers: Vec::new(), truncation: Truncation::default() }
    }

    fn contract(&self, key: &str) -> Option<&'p ContractDef> {
        self.project.contract_index.get(key).map(|&i| &self.project.contracts[i])
    }

    /// Each declaration in every deployable contract that runs it, else in its own contract.
    fn units(&self) -> Vec<Unit<'p>> {
        let (project, analysis) = (self.project, self.analysis);
        let mut units = Vec::new();
        let mut covered = BTreeSet::new();
        for context in project.project_contracts().filter(|c| c.is_deployable()) {
            let Some(sequence) = analysis.sequence_analyses.get(context.key()) else { continue };
            for behavior in &sequence.functions {
                let Some(owner) = self.contract(&behavior.owner) else { continue };
                let Some(func) = owner.functions.iter().find(|f| f.key == behavior.name) else { continue };
                covered.insert((owner.key(), func.key.as_str()));
                units.push(Unit { context, owner, func, behavior: Cow::Borrowed(behavior) });
            }
        }
        let mut dependencies = Vec::new();
        let mut keys: Vec<&(String, String)> = analysis.cfgs.keys().collect();
        keys.sort();
        for (owner_key, func_key) in keys {
            let Some(owner) = self.contract(owner_key) else { continue };
            let Some(func) = owner.functions.iter().find(|f| &f.key == func_key) else { continue };
            if covered.contains(&(owner.key(), func.key.as_str())) {
                continue;
            }
            let own = analysis.sequence_analyses.get(owner.key()).and_then(|s| find_behavior(&s.functions, owner.key(), &func.key));
            match own {
                Some(behavior) => units.push(Unit { context: owner, owner, func, behavior: Cow::Borrowed(behavior) }),
                None if owner.is_dependency => dependencies.push((owner, func)),
                None => {}
            }
        }
        if !dependencies.is_empty() {
            // dependency code outside every project contract is analysed in its own contract
            let summaries = analysis.cfgs.iter().map(|(key, cfg)| (key.clone(), summarize(cfg, project))).collect();
            let none = HashMap::new();
            let mut engine = EffectEngine::new(project, &summaries, &none);
            for (owner, func) in dependencies {
                if let Some(behavior) = function_behavior(project, &mut engine, owner, owner, func) {
                    units.push(Unit { context: owner, owner, func, behavior: Cow::Owned(behavior) });
                }
            }
        }
        units
    }

    pub fn build(mut self) -> Graph {
        for unit in self.units() {
            self.unit(&unit);
        }
        for c in self.project.contracts.iter().filter(|c| !c.is_free_functions()) {
            self.inherits(c);
            if !c.is_dependency && c.is_deployable() {
                self.exposes(c);
            }
        }
        let mut edges = self.numbered();
        let mut calls: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for (id, call) in edges.iter().filter(|(_, e)| e["kind"] == "calls" && e.get("dispatch_candidates").is_some()) {
            let from = call["from"].as_str().unwrap_or_default().to_string();
            calls.entry((from, call["span"].to_string())).or_default().push(id.clone());
        }
        for link in &self.analysis.relations.links {
            self.link(link, &calls);
        }
        let mut call_ids: HashMap<(String, String, String), String> = HashMap::new();
        for (id, call) in edges.iter().filter(|(_, e)| e["kind"] == "calls") {
            let key = (call["from"].to_string(), call["in_context"].to_string(), call["span"].to_string());
            call_ids.insert(key, id.clone());
        }
        for pending in std::mem::take(&mut self.tries) {
            self.try_catch(pending, &call_ids);
        }
        for pending in std::mem::take(&mut self.transfers) {
            self.value_transfer(pending, &call_ids);
        }
        edges.extend(self.numbered());
        let guarded: Vec<(String, Value)> = edges
            .iter()
            .filter(|(_, e)| e["kind"] == "try_catch")
            .filter_map(|(id, e)| Some((e["guarded_call"].as_str()?.to_string(), Value::from(id.as_str()))))
            .collect();
        for (call, id) in guarded {
            if let Some(call) = edges.get_mut(&call) {
                call["in_try"] = id;
            }
        }
        let nodes = self.nodes(&edges);
        Graph { nodes, edges, truncation: self.truncation }
    }

    /// The hash leaves out the position and later fields, so moving other code or extending a kind keeps the id; equal facts collapse.
    fn edge(&mut self, kind: &str, from: &str, mut record: Value, span: Option<SourceSpan>, tag: &str, rule: &str) -> &mut Pending {
        record["kind"] = Value::from(kind);
        let hash = digest(as_1_0(&record).to_string().as_bytes());
        record["span"] = span.map_or(Value::Null, |s| self.ids.span(s));
        record["tag"] = Value::from(tag);
        record["rule"] = Value::from(rule);
        let same = self.edges.entry(format!("{kind}:{from}#{}", &hash[..16])).or_default();
        let at = same.iter().position(|p| p.record == record).unwrap_or_else(|| {
            same.push(Pending { record, key_sites: None });
            same.len() - 1
        });
        &mut same[at]
    }

    /// Facts differing only in position are numbered from 0 in source order.
    fn numbered(&mut self) -> BTreeMap<String, Value> {
        let mut edges = BTreeMap::new();
        for (prefix, mut same) in std::mem::take(&mut self.edges) {
            same.sort_by_cached_key(|p| (position(&p.record["span"]), p.record.to_string()));
            for (n, Pending { mut record, key_sites }) in same.into_iter().enumerate() {
                if let Some(sites) = key_sites {
                    self.keys(&mut record, &RecordKeys::direct(sites));
                }
                let id = format!("{prefix}.{n}");
                record["id"] = Value::from(id.as_str());
                edges.insert(id, record);
            }
        }
        edges
    }

    fn fact(&mut self, site: &Site, kind: &str, mut record: Value, span: SourceSpan, tag: &str, rule: &str) -> &mut Pending {
        record["function"] = Value::from(site.function);
        record["in_context"] = site.context_id.map_or(Value::Null, Value::from);
        self.edge(kind, site.function, record, Some(span), tag, rule)
    }

    fn unit(&mut self, unit: &Unit<'p>) {
        let (project, context, owner, func) = (self.project, unit.context, unit.owner, unit.func);
        let function = self.ids.function(owner, func);
        let context_id = self.ids.contract(context);
        let site = Site { function: &function, owner, context, context_id: context_id.as_deref() };
        let b = unit.behavior.as_ref();

        // a derived contract's modifier overrides change the code an inherited function runs
        let cfg: Option<Cow<CfgGraph>> = if !std::ptr::eq(context, owner) && overrides_modifiers(project, context, owner, func) {
            Some(Cow::Owned(CfgBuilder::build_with_project(func, context, Some(project))))
        } else {
            self.analysis.cfgs.get(&(owner.key().to_string(), func.key.clone())).map(Cow::Borrowed)
        };
        let mut call_spans = HashMap::new();
        if let Some(cfg) = &cfg {
            self.direct(&site, func, cfg, &mut call_spans);
        }
        for trace in &b.transitive_effects {
            let span = trace.via.iter().find_map(|label| call_spans.get(label)).copied().unwrap_or(func.span);
            let origin = Origin { span, via: self.via(context, &trace.via), rule: "effects" };
            match trace.kind {
                EffectKind::StateWrite => self.access(&site, "writes", &trace.item, None, origin, None),
                EffectKind::StateRead => self.access(&site, "reads", &trace.item, None, origin, None),
                EffectKind::StorageParamWrite | EffectKind::StorageParamRead => {
                    let kind = if trace.kind == EffectKind::StorageParamWrite { "writes" } else { "reads" };
                    let param = target_base_name(&trace.item).to_string();
                    self.access(&site, kind, &trace.item, Some(param), origin, None);
                }
                EffectKind::Event => {
                    let callee = trace.via.last().and_then(|label| label.split_once("::"));
                    let declared_in = callee.and_then(|(c, _)| self.contract(c));
                    for event_id in callee.map(|(c, f)| self.emitted(c, f, &trace.item)).unwrap_or_default() {
                        self.emits(&site, event_id, declared_in, &trace.item, origin.clone());
                    }
                }
                EffectKind::Revert => self.reverts(&site, &trace.item, origin),
                // the call is an edge of the callee that makes it
                EffectKind::ExternalCall => {}
            }
        }
        for guard in &b.guards {
            self.guard(&site, func, guard);
        }
        for used in &b.builtin_uses {
            self.fact(&site, "builtin_use", json!({"name": used.builtin.name()}), used.span, EXTRACTED, "code_facts");
        }
        for span in &b.unchecked_blocks {
            self.fact(&site, "unchecked_block", json!({}), *span, EXTRACTED, "code_facts");
        }
        for (order, used) in func.modifiers.iter().enumerate() {
            let declared = self.modifier(context, &used.name);
            let tag = if modifier_code(Some(project), context, &used.name).is_some() { EXTRACTED } else { AMBIGUOUS };
            let record = json!({"modifier": declared, "name": used.name, "order": order});
            self.fact(&site, "uses_modifier", record, func.span, tag, "linearization");
        }

        let counters = &mut self.truncation;
        counters.effects_incomplete += usize::from(b.effects_incomplete);
        counters.reads_incomplete += usize::from(b.reads_incomplete);
        counters.reverts_incomplete += usize::from(b.reverts_incomplete);
        counters.guards_incomplete += usize::from(b.guards_incomplete);
        let flags = json!({
            "in_context": context_id,
            "effects_incomplete": b.effects_incomplete,
            "reads_incomplete": b.reads_incomplete,
            "reverts_incomplete": b.reverts_incomplete,
            "guards_incomplete": b.guards_incomplete,
        });
        self.flags.entry(function.clone()).or_default().insert(context_id.unwrap_or_default(), flags);
    }

    /// What the function's own code and modifiers do, from every reachable block.
    fn direct(&mut self, site: &Site, func: &FunctionDef, cfg: &CfgGraph, call_spans: &mut HashMap<String, SourceSpan>) {
        let param_name = |index: Option<usize>| index.map(|i| func.params.get(i).map_or_else(|| i.to_string(), |p| p.name.clone()));
        for access in storage_accesses(self.project, site.context, site.owner, func, cfg) {
            let origin = Origin { span: access.span, via: self.in_modifier(site, access.modifier.as_deref()), rule: "cfg" };
            let kind = if access.kind == AccessKind::Read { "reads" } else { "writes" };
            let key_site = KeySite { keys: access.keys, span: access.span };
            self.access(site, kind, &access.path, param_name(access.param), origin, Some(key_site));
        }
        for n in reachable(cfg) {
            let block = &cfg[n];
            let block_span = block.span.unwrap_or(func.span);
            if let Some(reason) = &block.revert_reason {
                let via = self.in_modifier(site, block.revert_modifier.as_deref());
                self.reverts(site, reason, Origin { span: block_span, via, rule: "cfg" });
            }
            for stmt in &block.statements {
                let (span, modifier) = stmt.origin();
                let origin = Origin { span: span.unwrap_or(func.span), via: self.in_modifier(site, modifier), rule: "cfg" };
                match stmt {
                    CfgStatement::ExternalCall { .. } | CfgStatement::InternalCall { .. } => {
                        self.call(site, stmt, origin, call_spans);
                    }
                    CfgStatement::EmitEvent { event, event_id, .. } => self.emits(site, *event_id, None, event, origin),
                    CfgStatement::AssemblyBlock { .. } => {
                        self.fact(site, "opaque_assembly", json!({"via": origin.via}), origin.span, EXTRACTED, "cfg");
                    }
                    _ => {}
                }
            }
        }
        for fact in try_catches(self.project, site.context, site.owner, func, cfg) {
            let via = self.in_modifier(site, fact.modifier.as_deref());
            self.tries.push(AwaitingCalls { function: site.function.to_string(), context_id: site.context_id.map(str::to_string), via, fact });
        }
        let values = value_facts(self.project, site.context, site.owner, func, cfg);
        for fact in values.transfers {
            let via = self.in_modifier(site, fact.modifier.as_deref());
            self.transfers.push(AwaitingCalls { function: site.function.to_string(), context_id: site.context_id.map(str::to_string), via, fact });
        }
        for read in values.balance_reads {
            let record = json!({
                "asset_kind": read.asset_kind,
                "token_expr": read.token_expr,
                "owner_expr": read.owner_expr,
                "id_expr": read.id_expr,
                "via_library": read.via_library.as_deref().and_then(|f| self.reference(f)),
                "via": self.in_modifier(site, read.modifier.as_deref()),
            });
            let tag = json!(read.tag);
            let pending = self.fact(site, "balance_read", record, read.span, tag.as_str().unwrap_or_default(), "value");
            pending.record["asset_candidates"] = json!(read.asset_candidates);
            pending.record["reason"] = json!(read.reason);
        }
    }

    /// The call is found by function, context and span, as try statements find theirs.
    fn value_transfer(&mut self, pending: AwaitingCalls<ValueTransfer>, calls: &HashMap<(String, String, String), String>) {
        let AwaitingCalls { function, context_id, via, fact } = pending;
        let in_context = json!(context_id);
        let key = (Value::from(function.as_str()).to_string(), in_context.to_string(), self.ids.span(fact.span).to_string());
        let call_id = calls.get(&key).cloned();
        let record = json!({
            "function": function,
            "in_context": in_context,
            "asset_kind": fact.asset_kind,
            "operation": fact.operation,
            "token_expr": fact.token_expr,
            "from_expr": fact.from_expr,
            "to_expr": fact.to_expr,
            "amount_expr": fact.amount_expr,
            "id_expr": fact.id_expr,
            "via_library": fact.via_library.as_deref().and_then(|f| self.reference(f)),
            "via": via,
        });
        let tag = json!(fact.tag);
        let pending = self.edge("value_transfer", &function, record, Some(fact.span), tag.as_str().unwrap_or_default(), "value");
        pending.record["asset_candidates"] = json!(fact.asset_candidates);
        pending.record["direction"] = json!(fact.direction);
        pending.record["call_id"] = json!(call_id);
        pending.record["reason"] = json!(fact.reason);
    }

    /// Clauses and positions stay out of the id, so editing a catch keeps it.
    fn try_catch(&mut self, pending: AwaitingCalls<TryCatchFact>, calls: &HashMap<(String, String, String), String>) {
        let AwaitingCalls { function, context_id, via, fact } = pending;
        let in_context = json!(context_id);
        let span = |s: SourceSpan| self.ids.span(s);
        let guarded = fact.guarded_call.as_ref().and_then(|call| {
            let key = (Value::from(function.as_str()).to_string(), in_context.to_string(), span(call.span).to_string());
            calls.get(&key).cloned()
        });
        let site = |site: &EffectSite, field: &str| json!({field: site.text, "span": span(site.span)});
        let context = self.contract(&fact.in_context);
        let clauses: Vec<Value> = fact
            .catch_clauses
            .iter()
            .map(|clause| {
                let writes: Vec<Value> = clause
                    .effects
                    .writes
                    .iter()
                    .map(|w| {
                        let mut write = site(w, "path");
                        write["state_var"] = json!(context.and_then(|c| self.state_var(c, &w.text)));
                        write
                    })
                    .collect();
                json!({
                    "kind": clause.kind,
                    "span": span(clause.span),
                    "empty": clause.empty,
                    "effects": {
                        "writes": writes,
                        "calls": clause.effects.calls.iter().map(|c| site(c, "text")).collect::<Vec<_>>(),
                        "emits": clause.effects.emits.iter().map(|e| site(e, "event")).collect::<Vec<_>>(),
                        "reverts": clause.effects.reverts,
                    },
                })
            })
            .collect();
        let success_span = fact.success_span.map(span);
        let record = json!({"function": function, "in_context": in_context, "guarded_call": guarded, "via": via});
        let tag = json!(fact.tag);
        let pending = self.edge("try_catch", &function, record, Some(fact.span), tag.as_str().unwrap_or_default(), "cfg");
        pending.record["success_span"] = json!(success_span);
        pending.record["catch_clauses"] = Value::from(clauses);
        pending.record["reason"] = json!(fact.reason);
    }

    fn call(&mut self, site: &Site, stmt: &CfgStatement, origin: Origin, call_spans: &mut HashMap<String, SourceSpan>) {
        let project = self.project;
        let (target_text, kind, static_target, dispatch_target, tag) = match CallSite::of(stmt, &project.decl_table) {
            Some(call) => {
                let declared = call.declaration.as_ref().and_then(|(c, f)| self.ids.function_by_key(c, f));
                let runs = resolve_call(project, site.context, site.owner, &call);
                if let Some((callee_owner, callee)) = runs {
                    call_spans.entry(format!("{}::{}", callee_owner.key(), callee.key)).or_insert(origin.span);
                }
                let dispatch = runs.map(|(c, f)| self.ids.function(c, f));
                let tag = match (&declared, &dispatch) {
                    (Some(_), _) => TYPE_RESOLVED,
                    (None, Some(_)) => INFERRED,
                    (None, None) => AMBIGUOUS,
                };
                (call.call, call.kind, declared, dispatch, tag)
            }
            None => {
                let CfgStatement::ExternalCall { target, function, kind, resolved, contract, .. } = stmt else { return };
                let declared = resolved.and_then(|id| project.decl_table.lookup(id)).and_then(|t| match t.kind {
                    DeclKind::Function => self.ids.function_by_key(&t.contract, &t.function),
                    DeclKind::StateVariable => self.contract(&t.contract).map(|c| self.ids.member(c, &t.name)),
                    _ => None,
                });
                let created = contract.as_deref().filter(|_| *kind == CallKind::Creation).and_then(|name| project.lookup_contract(name).ok());
                let (declared, tag) = match (declared, created.and_then(|c| self.ids.contract(c))) {
                    (Some(id), _) => (Some(id), TYPE_RESOLVED),
                    (None, Some(id)) => (Some(id), INFERRED),
                    (None, None) => (None, AMBIGUOUS),
                };
                (format!("{target}.{function}"), *kind, declared, None, tag)
            }
        };
        let (options, target_source, result_used) = match stmt {
            CfgStatement::ExternalCall { options, target_source, result_used, .. } => (
                options.iter().map(|(name, value)| (name.clone(), Value::from(value.as_str()))).collect(),
                target_source.as_ref().map(|s| self.target_source(site.context, s)),
                *result_used,
            ),
            _ => (serde_json::Map::new(), None, None),
        };
        let mut record = json!({
            "from": site.function,
            "to": dispatch_target.as_ref().or(static_target.as_ref()),
            "target_text": target_text,
            "call_kind": snake(kind),
            "options": options,
            "static_target": static_target,
            "dispatch_target": dispatch_target,
            "target_source": target_source,
            "result_used": result_used,
            "via": origin.via,
            "in_context": site.context_id,
        });
        if let Some((kind, target)) = call_target(project, stmt) {
            let dispatch = self.analysis.relations.dispatch(project, site.context, kind, target.as_deref());
            let candidates: Vec<Value> = dispatch
                .candidates
                .iter()
                .map(|c| {
                    let contract = self.contract(&c.contract).and_then(|c| self.ids.contract(c));
                    json!({"function": self.reference(&c.function), "contract": contract, "basis": c.basis, "tag": c.tag})
                })
                .collect();
            record["dispatch_candidates"] = Value::from(candidates);
            if let Some(reason) = dispatch.unresolved_reason {
                record["unresolved_reason"] = json!(reason);
            }
        }
        self.edge("calls", site.function, record, Some(origin.span), tag, origin.rule);
    }

    /// A call's receiver, with the assignments of the state variable holding it.
    fn target_source(&self, context: &ContractDef, source: &ValueSource) -> Value {
        let mut value = self.source(source);
        if let Some(var) = state_var_ref(self.project, context, source) {
            let sites: Vec<Value> = self
                .analysis
                .relations
                .assignments(&var)
                .iter()
                .map(|a| {
                    let function = a.function.as_deref().and_then(|f| self.reference(f));
                    json!({"function": function, "span": self.ids.span(a.span), "expr_text": a.expr_text, "kind": a.kind})
                })
                .collect();
            value["assigned_at"] = Value::from(sites);
        }
        value
    }

    /// The id of a `<contract key>::<member key>` reference: a function, else a state variable.
    fn reference(&self, reference: &str) -> Option<String> {
        let (contract, member) = reference.rsplit_once("::")?;
        self.ids.function_by_key(contract, member).or_else(|| self.ids.member_by_key(reference))
    }

    /// One edge per contract pair, listing the export's ids of the calls that make it.
    fn link(&mut self, link: &ContractLink, calls: &BTreeMap<(String, String), Vec<String>>) {
        let (Some(from), Some(to)) = (self.contract(&link.from_contract), self.contract(&link.to_contract)) else { return };
        let Some(from_id) = self.ids.contract(from) else { return };
        let call_ids: Vec<&String> = link
            .calls
            .iter()
            .filter_map(|c| Some((self.reference(&c.function)?, self.ids.span(c.span).to_string())))
            .flat_map(|key| calls.get(&key).into_iter().flatten())
            .collect();
        let via: Vec<Option<String>> = link.via_state_vars.iter().map(|name| self.state_var(from, name)).collect();
        let record = json!({
            "from_contract": from_id,
            "to_contract": self.ids.contract(to),
            "via_state_vars": via,
            "call_ids": call_ids,
            "basis": link.basis,
        });
        let tag = json!(link.tag);
        self.edge("contract_link", &from_id, record, link.calls.first().map(|c| c.span), tag.as_str().unwrap_or_default(), "dispatch");
    }

    /// Without a key site the access happens in a callee.
    fn access(&mut self, site: &Site, kind: &str, path: &str, param: Option<String>, origin: Origin, key_site: Option<KeySite>) {
        let state_var = if param.is_some() { None } else { self.state_var(site.context, path) };
        let tag = match (&param, &state_var) {
            (Some(_), _) => EXTRACTED,
            (None, Some(_)) => TYPE_RESOLVED,
            (None, None) => AMBIGUOUS,
        };
        let mut record = json!({"state_var": state_var, "path": normalize_path(path), "param": param, "via": origin.via});
        if key_site.is_none() {
            self.keys(&mut record, &RecordKeys::transitive());
        }
        let pending = self.fact(site, kind, record, origin.span, tag, origin.rule);
        if let Some(key_site) = key_site {
            pending.key_sites.get_or_insert_with(Vec::new).push(key_site);
        }
    }

    fn keys(&self, record: &mut Value, keys: &RecordKeys) {
        let key = |k: &AccessKey| json!({"text": k.text, "kind": k.kind, "ref": k.reference.as_deref().and_then(|r| self.reference(r))});
        let set = |keys: &[AccessKey]| Value::from(keys.iter().map(key).collect::<Vec<_>>());
        record["keys"] = keys.keys.as_deref().map_or(Value::Null, set);
        record["keys_reason"] = json!(keys.keys_reason);
        record["key_sites"] = keys.key_sites.iter().map(|s| json!({"keys": set(&s.keys), "span": self.ids.span(s.span)})).collect();
    }

    /// The declarations `name` resolves to where the function keyed `func` in `contract` emits it; `None` stands for an emit without an id.
    fn emitted(&self, contract: &str, func: &str, name: &str) -> BTreeSet<Option<DeclId>> {
        let mut ids = BTreeSet::new();
        if let Some(cfg) = self.analysis.cfgs.get(&(contract.to_string(), func.to_string())) {
            for n in reachable(cfg) {
                for stmt in &cfg[n].statements {
                    if let CfgStatement::EmitEvent { event, event_id, .. } = stmt {
                        if event == name {
                            ids.insert(*event_id);
                        }
                    }
                }
            }
        }
        if ids.is_empty() {
            ids.insert(None);
        }
        ids
    }

    /// By the declaration solc resolved; without one, by name from the context, then from the contract that declares the emitting function.
    fn emits(&mut self, site: &Site, event_id: Option<DeclId>, declared_in: Option<&ContractDef>, name: &str, origin: Origin) {
        let (event, tag) = match event_id.and_then(|id| self.event_by_id(id)) {
            Some(event) => (Some(event), TYPE_RESOLVED),
            None => match self.event(site.context, name).or_else(|| declared_in.and_then(|c| self.event(c, name))) {
                Some(event) => (Some(event), INFERRED),
                None => (None, AMBIGUOUS),
            },
        };
        let record = json!({"event": event, "event_text": name, "via": origin.via});
        self.fact(site, "emits", record, origin.span, tag, origin.rule);
    }

    fn reverts(&mut self, site: &Site, reason: &str, origin: Origin) {
        self.fact(site, "reverts", json!({"reason": reason, "via": origin.via}), origin.span, EXTRACTED, origin.rule);
    }

    fn guard(&mut self, site: &Site, func: &FunctionDef, guard: &GuardFact) {
        let tag = if guard.ambiguous {
            AMBIGUOUS
        } else if guard.heuristic || guard.scope == GuardScope::Conditional {
            INFERRED
        } else {
            EXTRACTED
        };
        let mut record = self.term(&guard.term());
        record["via"] = Value::from(self.via(site.context, &guard.via));
        record["scope"] = Value::from(snake(guard.scope));
        record["restricts"] = Value::from(guard.restricts());
        record["heuristic"] = Value::from(guard.heuristic);
        self.fact(site, "guards", record, guard.span.unwrap_or(func.span), tag, "guards");
    }

    fn term(&self, term: &GuardTerm) -> Value {
        let source = |s: &Option<ValueSource>| s.as_ref().map(|s| self.source(s));
        json!({
            "guard_kind": snake(term.kind),
            "condition_text": term.condition,
            "negated": term.negated,
            "compared_against": source(&term.compared_against),
            "checked_by": term.checked_by,
            "role": source(&term.role),
            "role_implied_by": term.role_implied_by,
            "account": source(&term.account),
            "alternatives": term.alternatives.iter().map(|t| self.term(t)).collect::<Vec<_>>(),
            "conjuncts": term.conjuncts.iter().map(|t| self.term(t)).collect::<Vec<_>>(),
        })
    }

    fn in_modifier(&self, site: &Site, modifier: Option<&str>) -> Vec<Value> {
        modifier.map(|m| self.via(site.context, &[format!("modifier {m}")])).unwrap_or_default()
    }

    /// Analysis labels (`modifier m`, `<contract key>::<function key>`) as ids; an unknown label stays as it is.
    fn via(&self, context: &ContractDef, labels: &[String]) -> Vec<Value> {
        labels
            .iter()
            .map(|label| {
                let id = match label.strip_prefix("modifier ") {
                    Some(name) => self.modifier(context, name),
                    None => label.split_once("::").and_then(|(c, f)| self.ids.function_by_key(c, f)),
                };
                Value::from(id.unwrap_or_else(|| label.clone()))
            })
            .collect()
    }

    fn modifier(&self, context: &ContractDef, name: &str) -> Option<String> {
        let def = self.project.resolve_modifier(context, name)?;
        let owner = self.project.linearization(context).into_iter().find(|c| c.modifiers.iter().any(|m| std::ptr::eq(m, def)))?;
        Some(self.ids.member(owner, name))
    }

    fn state_var(&self, context: &ContractDef, path: &str) -> Option<String> {
        let name = target_base_name(path);
        self.project.state_var_owner(context, name).map(|c| self.ids.member(c, name))
    }

    /// Only a name: an overloaded event stays unresolved.
    fn event(&self, scope: &ContractDef, name: &str) -> Option<String> {
        let (contracts, short) = match name.rsplit_once('.') {
            Some((qualifier, short)) => (self.project.lookup_contract(qualifier).ok().into_iter().collect(), short),
            None => (self.project.linearization(scope), name),
        };
        let found: Vec<String> = contracts
            .iter()
            .flat_map(|c| c.events.iter().filter(|e| e.name == short).map(move |e| self.event_id(c, &e.name, &e.params)))
            .collect();
        match found.as_slice() {
            [only] => Some(only.clone()),
            _ => None,
        }
    }

    fn event_by_id(&self, id: DeclId) -> Option<String> {
        let target = self.project.decl_table.lookup(id).filter(|t| t.kind == DeclKind::Event)?;
        let c = self.contract(&target.contract)?;
        let event = c.events.iter().find(|e| e.id == Some(id))?;
        Some(self.event_id(c, &event.name, &event.params))
    }

    fn event_id(&self, c: &ContractDef, name: &str, params: &[Param]) -> String {
        let overloaded = c.events.iter().filter(|e| e.name == name).count() > 1;
        self.ids.overloadable(c, name, params, overloaded)
    }

    fn source(&self, source: &ValueSource) -> Value {
        let declared = |kind: &str, name: &str, key: &Option<String>| {
            json!({"kind": kind, "name": name, "id": key.as_deref().and_then(|k| self.ids.member_by_key(k))})
        };
        match source {
            ValueSource::StateVar { name, key } => declared("state_var", name, key),
            ValueSource::Immutable { name, key } => declared("immutable", name, key),
            ValueSource::Constant { name, key } => declared("constant", name, key),
            ValueSource::Param { index, name } => json!({"kind": "param", "index": index, "name": name}),
            ValueSource::Literal { text } => json!({"kind": "literal", "text": text}),
            ValueSource::MsgSender => json!({"kind": "msg_sender"}),
            ValueSource::TxOrigin => json!({"kind": "tx_origin"}),
            ValueSource::Expression { text } => json!({"kind": "expression", "text": text}),
        }
    }

    fn linearization_tag(c: &ContractDef) -> &'static str {
        if c.linearized_bases.is_empty() { INFERRED } else { TYPE_RESOLVED }
    }

    fn inherits(&mut self, c: &'p ContractDef) {
        let Some(child) = self.ids.contract(c) else { return };
        for (i, base) in self.project.linearization(c).into_iter().enumerate().skip(1) {
            let direct = base.id.is_some_and(|id| c.base_ids.contains(&id)) || c.inherits.contains(&base.name);
            let record = json!({"child": child, "base": self.ids.contract(base), "order": i, "direct": direct});
            self.edge("inherits", &child, record, Some(c.span), Self::linearization_tag(c), "linearization");
        }
    }

    /// Everything a deployed contract answers: functions, inherited ones included, and public state getters.
    fn exposes(&mut self, c: &'p ContractDef) {
        let Some(contract) = self.ids.contract(c) else { return };
        let tag = Self::linearization_tag(c);
        for af in self.project.accessible_functions(c) {
            let f = af.function;
            if !f.is_entry_point() {
                continue;
            }
            let via_base = if af.is_inherited { self.ids.contract(af.owner) } else { None };
            let record = json!({"contract": contract, "function": self.ids.function(af.owner, f), "via_base": via_base, "selector": f.selector, "getter": false});
            self.edge("exposes", &contract, record, Some(f.span), tag, "linearization");
        }
        for (var, owner_key, inherited) in self.project.accessible_state_vars(c) {
            let Some(owner) = self.contract(&owner_key).filter(|_| var.visibility == Visibility::Public) else { continue };
            let via_base = if inherited { self.ids.contract(owner) } else { None };
            let record = json!({"contract": contract, "function": self.ids.member(owner, &var.name), "via_base": via_base, "selector": var.selector, "getter": true});
            self.edge("exposes", &contract, record, Some(var.span), tag, "linearization");
        }
    }

    fn node(&self, kind: &str, id: String, mut record: Value, span: SourceSpan) -> (String, Value) {
        record["id"] = Value::from(id.as_str());
        record["kind"] = Value::from(kind);
        record["span"] = self.ids.span(span);
        record["tag"] = Value::from(EXTRACTED);
        record["rule"] = Value::from("parse");
        (id, record)
    }

    /// A dependency contract is a stub unless project code inherits it; its other members appear once an edge reaches them.
    fn nodes(&mut self, edges: &BTreeMap<String, Value>) -> BTreeMap<String, Value> {
        let project = self.project;
        let inherited: BTreeSet<&str> =
            project.project_contracts().flat_map(|c| project.linearization(c)).filter(|c| c.is_dependency).map(|c| c.key()).collect();
        let mut referenced = BTreeSet::new();
        for edge in edges.values() {
            collect_ids(edge, &mut referenced);
        }
        referenced.extend(self.flags.keys().cloned());

        let mut nodes = BTreeMap::new();
        for c in &project.contracts {
            let full = !c.is_dependency || inherited.contains(c.key());
            let members: Vec<(String, Value)> = self.members(c).into_iter().filter(|(id, _)| full || referenced.contains(id)).collect();
            if let Some(id) = self.ids.contract(c) {
                let record = json!({
                    "name": c.name,
                    "contract_kind": snake(c.kind),
                    "bases": project.linearization(c).iter().skip(1).filter_map(|b| self.ids.contract(b)).collect::<Vec<_>>(),
                    "is_dependency": c.is_dependency,
                    "deployable": c.is_deployable(),
                    "reachable_from_project": full || !members.is_empty(),
                });
                let (id, record) = self.node("contract", id, record, c.span);
                nodes.insert(id, record);
            }
            nodes.extend(members);
        }
        nodes
    }

    fn members(&mut self, c: &'p ContractDef) -> Vec<(String, Value)> {
        let project = self.project;
        let contract = self.ids.contract(c);
        let mut out = Vec::new();
        for f in &c.functions {
            let id = self.ids.function(c, f);
            let overrides = f
                .is_override
                .then(|| project.implementation(&project.linearization(c), f.match_signature(), Some(c.key())))
                .flatten()
                .map(|(o, base)| self.ids.function(o, base));
            let record = json!({
                "name": f.name,
                "contract": contract,
                "function_kind": snake(f.kind),
                "visibility": snake(f.visibility),
                "mutability": snake(f.mutability),
                "selector": f.selector,
                "signature": f.match_signature(),
                "modifiers": f.modifiers.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
                "virtual": f.is_virtual,
                "override": f.is_override,
                "overrides": overrides,
                "is_entry_point": f.is_entry_point(),
                "has_body": f.body.is_some(),
                "paths": self.analysis.path_trees.get(&(c.key().to_string(), f.key.clone())).map(|t| &t.stats),
                "analysis": self.flags.remove(&id).map(|flags| flags.into_values().collect::<Vec<_>>()).unwrap_or_default(),
            });
            out.push(self.node("function", id, record, f.span));
        }
        for (order, v) in c.state_vars.iter().enumerate() {
            let record = json!({
                "name": v.name,
                "contract": contract,
                "type": v.type_name,
                "visibility": snake(v.visibility),
                "constant": v.is_constant,
                "immutable": v.is_immutable,
                "order": order,
                "selector": v.selector,
            });
            out.push(self.node("state_var", self.ids.member(c, &v.name), record, v.span));
        }
        for e in &c.events {
            let record = json!({"name": e.name, "contract": contract, "params": param_types(&e.params)});
            out.push(self.node("event", self.event_id(c, &e.name, &e.params), record, e.span));
        }
        for e in &c.errors {
            let overloaded = c.errors.iter().filter(|other| other.name == e.name).count() > 1;
            let record = json!({"name": e.name, "contract": contract, "params": param_types(&e.params)});
            out.push(self.node("error", self.ids.overloadable(c, &e.name, &e.params, overloaded), record, e.span));
        }
        for m in &c.modifiers {
            let record = json!({"name": m.name, "contract": contract, "params": param_types(&m.params), "has_body": !m.body.is_empty()});
            out.push(self.node("modifier", self.ids.member(c, &m.name), record, m.span));
        }
        out
    }
}

fn position(span: &Value) -> (String, [u64; 4]) {
    let at = |field: &str| span[field].as_u64().unwrap_or_default();
    let file = span["file"].as_str().unwrap_or_default().to_string();
    (file, [at("start_line"), at("start_col"), at("end_line"), at("end_col")])
}

fn collect_ids(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::String(s) if s.starts_with("evm:") => {
            out.insert(s.clone());
        }
        Value::Array(items) => items.iter().for_each(|v| collect_ids(v, out)),
        Value::Object(map) => map.values().for_each(|v| collect_ids(v, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{as_1_0, snake};
    use serde_json::json;
    use crate::classify::guards::GuardKind;
    use crate::model::expression::CallKind;
    use crate::model::function::Mutability;

    #[test]
    fn variant_names_become_snake_case() {
        assert_eq!(snake(CallKind::LowLevelCall), "low_level_call");
        assert_eq!(snake(CallKind::Internal), "internal");
        assert_eq!(snake(GuardKind::TxOrigin), "tx_origin");
        assert_eq!(snake(Mutability::NonPayable), "non_payable");
    }

    #[test]
    fn fields_added_after_1_0_stay_out_of_the_identity() {
        let v1 = json!({"kind": "calls", "to": "t", "target_source": {"kind": "state_var", "name": "x"}});
        let mut v1_1 = v1.clone();
        v1_1["dispatch_candidates"] = json!([]);
        v1_1["unresolved_reason"] = json!("no_implementation");
        v1_1["target_source"]["assigned_at"] = json!([]);
        v1_1["in_try"] = json!("try_catch:f#0123456789abcdef.0");
        assert_eq!(as_1_0(&v1_1), v1);
        v1_1["kind"] = json!("reads");
        assert_eq!(as_1_0(&v1_1), v1_1, "other kinds keep every field");

        let read = json!({"kind": "writes", "path": "m[]"});
        let mut keyed = read.clone();
        keyed["keys"] = json!(null);
        keyed["keys_reason"] = json!("transitive");
        keyed["key_sites"] = json!([]);
        assert_eq!(as_1_0(&keyed), read);
    }
}
