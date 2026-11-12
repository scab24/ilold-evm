//! Facts that relate code across contracts, computed once from the analysis; every surface reads them from here.
mod dispatch;
mod keys;
mod phases;
mod route;
mod trycatch;
mod value;

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::analysis::context_cfg;
use crate::cfg::types::{CfgGraph, CfgStatement};
use crate::model::common::{Param, SourceSpan};
use crate::model::contract::{ContractDef, ContractKind};
use crate::model::decl_id::{DeclId, DeclTable};
use crate::model::expression::CallKind;
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::model::source::ValueSource;
use crate::sequence::effects::{reachable, resolve_call, CallSite};

pub use dispatch::call_target;
pub use keys::{key_kinds, storage_accesses, AccessKey, AccessKind, KeyKind, KeySite, KeysReason, RecordKeys, StorageAccess};
pub use phases::{phase_candidates, PhaseCandidate};
pub use route::{routes, Route, RouteCall, RouteStep, Routes};
pub use trycatch::{try_catches, CatchClauseFact, CatchEffects, CatchKind, EffectSite, GuardedCall, TryCatchFact, TryReason};
pub use value::{
    value_flows, AssetKind, BalanceRead, Direction, Operation, Party, ValueFacts, ValueFlow, ValueFlowPairs, ValuePair, ValueReason, ValueTransfer,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Tag {
    Inferred,
    Ambiguous,
    Extracted,
    TypeResolved,
}

/// Strongest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// The library function the call names runs.
    Library,
    /// `this.f()`: the calling contract answers.
    #[serde(rename = "self")]
    SelfCall,
    /// The contract inherits the interface the call names, directly or transitively.
    InheritsInterface,
    /// The contract exposes the selector without inheriting the interface.
    SelectorMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnresolvedReason {
    /// The interface is outside the project sources and nothing in them implements it.
    OutOfScopeInterface,
    NoImplementation,
    /// The call names no declaration, such as a call through a function pointer.
    DynamicAddress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentKind {
    ConstructorParam,
    Initializer,
    Setter,
    Literal,
    Other,
}

/// A deployable contract that may answer an external call, and the function that runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchCandidate {
    /// `<contract key>::<function key>` of the implementation, a getter's state variable included.
    pub function: String,
    pub contract: String,
    pub basis: Basis,
    pub tag: Tag,
}

/// Every candidate for one call target; several are AMBIGUOUS, none carry the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dispatch {
    /// Only `this.f()` depends on the contract that runs the call.
    pub context: Option<String>,
    /// `<contract key>::<member key>` the source names.
    pub target: Option<String>,
    pub candidates: Vec<DispatchCandidate>,
    pub unresolved_reason: Option<UnresolvedReason>,
}

/// Where a state variable used as a call target gets its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentSite {
    /// `<contract key>::<name>` of the declaration.
    pub state_var: String,
    /// `<contract key>::<function key>`; none for the declaration's initial value.
    pub function: Option<String>,
    pub span: SourceSpan,
    pub expr_text: String,
    pub kind: AssignmentKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkCall {
    /// `<contract key>::<function key>` making the call.
    pub function: String,
    pub span: SourceSpan,
}

/// One per ordered contract pair: the calls from code declared in `from_contract` that `to_contract` may answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractLink {
    pub from_contract: String,
    pub to_contract: String,
    /// Names of the state variables holding the called address.
    pub via_state_vars: Vec<String>,
    pub calls: Vec<LinkCall>,
    /// The strongest basis of any call.
    pub basis: Basis,
    /// INFERRED when some call has this pair as its only candidate.
    pub tag: Tag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractCallKind {
    Call,
    /// A call to a view or pure function or a getter.
    Read,
    Library,
    #[serde(rename = "self")]
    SelfCall,
}

/// A call to another contract, a library or `this`, as one contract runs the code making it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractCall {
    /// Key of the contract running the code: the declaring one, or one inheriting it.
    pub in_context: String,
    /// `<contract key>::<function key>` making the call.
    pub function: String,
    pub span: SourceSpan,
    pub kind: ContractCallKind,
    /// `<contract key>::<member key>` the source names; a dependency member stays here when nothing answers it.
    pub target: Option<String>,
    /// Several are AMBIGUOUS; none carry the reason.
    pub callees: Vec<DispatchCandidate>,
    pub unresolved_reason: Option<UnresolvedReason>,
    /// `<contract key>::<name>` of the state variable holding the called address.
    pub via: Option<String>,
    /// A try statement guards the call: its failure is caught.
    pub in_try: bool,
    /// Ids of the value transfers the call makes, as `in_context` runs it.
    pub transfers: Vec<String>,
    /// Path of the source file holding the call.
    pub file: Option<String>,
}

/// Calls between the running contract and one other contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionGroup {
    /// The contract that may answer the outgoing calls, or the running contract making the incoming ones.
    pub contract: String,
    /// Sorted by calling function, then position; an AMBIGUOUS call is listed under each candidate.
    pub calls: Vec<ContractCall>,
}

/// What the code a contract runs, its inherited code included, calls, and what calls it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connections {
    pub contract: String,
    /// By key of the contract that may answer: `this` calls under the contract itself, library calls under the library.
    pub outgoing: Vec<ConnectionGroup>,
    /// By key of every other deployed contract whose calls it may answer.
    pub incoming: Vec<ConnectionGroup>,
    /// Calls nothing in the project answers, with their reason.
    pub unresolved: Vec<ContractCall>,
    /// Where the state variables holding the called addresses are set, sorted by variable, then position.
    pub assignments: Vec<AssignmentSite>,
    /// The value transfers the listed calls make, sorted by context, then function, then position.
    pub transfers: Vec<ValueTransfer>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Relations {
    /// Sorted by context, then target.
    pub dispatches: Vec<Dispatch>,
    /// Sorted by state variable, then position.
    pub assignments: Vec<AssignmentSite>,
    /// Sorted by source, then target contract.
    pub links: Vec<ContractLink>,
    /// Sorted by context, then calling function, then position.
    pub calls: Vec<ContractCall>,
    /// Every function's facts in each contract that runs it, sorted by context, then function, then position.
    pub values: ValueFacts,
    /// Deployed project contracts only, sorted by context, then the parties.
    pub value_pairs: Vec<ValuePair>,
    /// Keys of the project contracts that get deployed, sorted; only their calls reach other contracts.
    pub deployed: Vec<String>,
}

/// `context` runs `func`, declared in `owner`, as `cfg`.
pub(crate) struct Running<'p> {
    pub context: &'p ContractDef,
    pub owner: &'p ContractDef,
    pub func: &'p FunctionDef,
    pub cfg: Cow<'p, CfgGraph>,
}

/// Each analysed function in its own contract, then in every project contract each base and project library function it runs: base constructors, inherited code with private helpers, overridden code its code still calls, as through `super`, and the project library functions its code calls.
fn running<'p>(project: &'p Project, cfgs: &'p HashMap<(String, String), CfgGraph>) -> Vec<Running<'p>> {
    let mut keys: Vec<&(String, String)> = cfgs.keys().collect();
    keys.sort();
    let mut units = Vec::new();
    for key in keys {
        let Some(owner) = project.contract_index.get(&key.0).map(|&i| &project.contracts[i]) else { continue };
        let Some(func) = owner.functions.iter().find(|f| f.key == key.1) else { continue };
        units.push(Running { context: owner, owner, func, cfg: Cow::Borrowed(&cfgs[key]) });
    }
    for context in project.project_contracts() {
        for (owner, func) in project.running_functions(context).into_iter().filter(|(owner, _)| !std::ptr::eq(*owner, context)) {
            let Some(cfg) = context_cfg(project, cfgs, context, owner, func) else { continue };
            units.push(Running { context, owner, func, cfg });
        }
        let bases = &project.linearization(context)[1..];
        // dependency libraries stay at their call site, where the value recognizers read them
        let runs_here = |owner: &ContractDef| bases.iter().any(|b| std::ptr::eq(*b, owner)) || (owner.kind == ContractKind::Library && !owner.is_dependency);
        let mut pending: Vec<usize> = (0..units.len()).filter(|&i| std::ptr::eq(units[i].context, context)).collect();
        let mut runs: HashSet<(&str, &str)> = pending.iter().map(|&i| (units[i].owner.key(), units[i].func.key.as_str())).collect();
        while let Some(i) = pending.pop() {
            for (owner, func) in internal_callees(project, &units[i]) {
                if !runs_here(owner) || !runs.insert((owner.key(), func.key.as_str())) {
                    continue;
                }
                let Some(cfg) = context_cfg(project, cfgs, context, owner, func) else { continue };
                units.push(Running { context, owner, func, cfg });
                pending.push(units.len() - 1);
            }
        }
    }
    units
}

/// The functions a unit's reachable code calls internally, resolved as its context runs them.
fn internal_callees<'p>(project: &'p Project, unit: &Running<'p>) -> Vec<(&'p ContractDef, &'p FunctionDef)> {
    let statements = reachable(&unit.cfg).into_iter().flat_map(|n| unit.cfg[n].statements.iter());
    let sites = statements.filter_map(|stmt| CallSite::of(stmt, &project.decl_table));
    sites.filter_map(|site| resolve_call(project, unit.context, unit.owner, &site)).collect()
}

impl Relations {
    pub fn build(project: &Project, cfgs: &HashMap<(String, String), CfgGraph>) -> Self {
        let units = running(project, cfgs);
        let mut relations = dispatch::build(project, &units);
        relations.deployed = project.contracts.iter().filter(|c| c.is_project_deployable()).map(|c| c.key().to_string()).collect();
        relations.deployed.sort();
        (relations.values, relations.value_pairs) = value::build(project, &units, &relations);
        let moved: Vec<Vec<String>> = relations.calls.iter().map(|c| relations.transfers_at(c).iter().map(|t| t.id.clone()).collect()).collect();
        for (call, ids) in relations.calls.iter_mut().zip(moved) {
            call.transfers = ids;
        }
        relations
    }

    /// What may answer an external call to `target` made as `context` runs it; code no analysed function contains is resolved here.
    pub fn dispatch(&self, project: &Project, context: &ContractDef, kind: CallKind, target: Option<&str>) -> Cow<'_, Dispatch> {
        let context = (kind == CallKind::ExternalSelf).then(|| context.key());
        match self.dispatches.binary_search_by(|d| (d.context.as_deref(), d.target.as_deref()).cmp(&(context, target))) {
            Ok(i) => Cow::Borrowed(&self.dispatches[i]),
            Err(_) => Cow::Owned(dispatch::resolve(project, context, kind, target)),
        }
    }

    /// `state_var` is `<contract key>::<name>`.
    pub fn assignments(&self, state_var: &str) -> &[AssignmentSite] {
        let start = self.assignments.partition_point(|a| a.state_var.as_str() < state_var);
        let end = self.assignments.partition_point(|a| a.state_var.as_str() <= state_var);
        &self.assignments[start..end]
    }

    pub fn outgoing<'a>(&'a self, contract: &'a str) -> impl Iterator<Item = &'a ContractLink> {
        self.links.iter().filter(move |l| l.from_contract == contract)
    }

    pub fn deploys(&self, contract: &str) -> bool {
        self.deployed.binary_search_by(|k| k.as_str().cmp(contract)).is_ok()
    }

    /// The calls the code `context` runs makes, its inherited code included.
    pub fn calls_from(&self, context: &str) -> &[ContractCall] {
        run_of(&self.calls, |c| c.in_context.as_str().cmp(context))
    }

    /// The value transfers of the code `context` runs, its inherited and project library code included.
    pub fn values_of(&self, context: &str) -> &[ValueTransfer] {
        run_of(&self.values.transfers, |t| t.in_context.as_str().cmp(context))
    }

    /// The calls `function` makes at `span` as `context` runs it.
    pub fn calls_at(&self, context: &str, function: &str, span: SourceSpan) -> &[ContractCall] {
        run_of(&self.calls, |c| (c.in_context.as_str(), c.function.as_str(), c.span).cmp(&(context, function, span)))
    }

    /// The value transfers `call` makes.
    pub fn transfers_at(&self, call: &ContractCall) -> &[ValueTransfer] {
        let wanted = (call.in_context.as_str(), call.function.as_str(), call.span);
        run_of(&self.values.transfers, |t| (t.in_context.as_str(), t.function.as_str(), t.span).cmp(&wanted))
    }

    /// What `function` (`<contract key>::<function key>`) moves and reads as `context` runs it.
    pub fn values(&self, context: &str, function: &str) -> ValueFacts {
        let wanted = (context, function);
        ValueFacts {
            transfers: run_of(&self.values.transfers, |t| (t.in_context.as_str(), t.function.as_str()).cmp(&wanted)).to_vec(),
            balance_reads: run_of(&self.values.balance_reads, |r| (r.in_context.as_str(), r.function.as_str()).cmp(&wanted)).to_vec(),
        }
    }

    /// What `func`, declared in `owner`, moves and reads as `contract` runs it; a dependency runs only its own code.
    pub fn values_in(&self, contract: &ContractDef, owner: &ContractDef, func: &FunctionDef) -> ValueFacts {
        let context = if contract.is_dependency { owner } else { contract };
        self.values(context.key(), &format!("{}::{}", owner.key(), func.key))
    }

    /// The calls `contract` may answer, from every other deployed contract running the calling code.
    pub fn calls_into<'a>(&'a self, contract: &'a str) -> impl Iterator<Item = &'a ContractCall> {
        self.calls.iter().filter(move |c| c.in_context != contract && self.deploys(&c.in_context) && c.callees.iter().any(|d| d.contract == contract))
    }

    pub fn connections(&self, contract: &str) -> Connections {
        let made = self.calls_from(contract);
        let outgoing = groups(made.iter().flat_map(|c| c.callees.iter().map(move |d| (d.contract.as_str(), c))));
        let incoming = groups(self.calls_into(contract).map(|c| (c.in_context.as_str(), c)));
        let mut vias: Vec<&str> = made.iter().filter_map(|c| c.via.as_deref()).collect();
        vias.sort_unstable();
        vias.dedup();
        let mut transfers: Vec<ValueTransfer> = made.iter().chain(self.calls_into(contract)).flat_map(|c| self.transfers_at(c)).cloned().collect();
        transfers.sort_by(|a, b| (&a.in_context, &a.function, a.span).cmp(&(&b.in_context, &b.function, b.span)));
        Connections {
            contract: contract.to_string(),
            outgoing,
            incoming,
            unresolved: made.iter().filter(|c| c.callees.is_empty()).cloned().collect(),
            assignments: vias.into_iter().flat_map(|v| self.assignments(v)).cloned().collect(),
            transfers,
        }
    }

    /// The links the calls of the code `contract` runs add up to, and those of the calls into it from every other deployed contract.
    pub fn context_links(&self, contract: &str) -> (Vec<ContractLink>, Vec<ContractLink>) {
        let incoming = dispatch::links(self.calls_into(contract)).into_iter().filter(|l| l.to_contract == contract).collect();
        (dispatch::links(self.calls_from(contract)), incoming)
    }

    /// The pairs `contract` runs or is a party of, every pair without one, with the transfers they group.
    pub fn value_flow(&self, contract: Option<&str>) -> ValueFlowPairs {
        let touches = |p: &ValuePair| contract.is_none_or(|c| p.in_context == c || p.from.names(c) || p.to.names(c));
        let pairs: Vec<ValuePair> = self.value_pairs.iter().filter(|p| touches(p)).cloned().collect();
        let ids: HashSet<(&str, &str)> = pairs.iter().flat_map(|p| p.transfers.iter().map(|id| (p.in_context.as_str(), id.as_str()))).collect();
        let transfers = self.values.transfers.iter().filter(|t| ids.contains(&(t.in_context.as_str(), t.id.as_str()))).cloned().collect();
        ValueFlowPairs { contract: contract.map(str::to_string), pairs, transfers }
    }
}

/// The facts of `sorted` that `order`, comparing a fact's key with the wanted one, finds equal.
fn run_of<T>(sorted: &[T], order: impl Fn(&T) -> Ordering) -> &[T] {
    let start = sorted.partition_point(|f| order(f) == Ordering::Less);
    let end = sorted.partition_point(|f| order(f) != Ordering::Greater);
    &sorted[start..end]
}

/// Calls by contract key, in key order.
fn groups<'a>(calls: impl Iterator<Item = (&'a str, &'a ContractCall)>) -> Vec<ConnectionGroup> {
    let mut groups: BTreeMap<&str, Vec<ContractCall>> = BTreeMap::new();
    for (contract, call) in calls {
        groups.entry(contract).or_default().push(call.clone());
    }
    groups.into_iter().map(|(contract, calls)| ConnectionGroup { contract: contract.to_string(), calls }).collect()
}

/// `<contract key>::<member key>` when the contract declares that function or state variable.
pub fn declared_target(project: &Project, contract: &str, member: &str) -> Option<String> {
    let c = &project.contracts[*project.contract_index.get(contract)?];
    let declared = c.functions.iter().any(|f| f.key == member) || c.state_vars.iter().any(|v| v.name == member);
    declared.then(|| format!("{contract}::{member}"))
}

/// `<contract key>::<name>` of the state variable a value comes from, as `context` sees it.
pub fn state_var_ref(project: &Project, context: &ContractDef, source: &ValueSource) -> Option<String> {
    let (ValueSource::StateVar { name, .. } | ValueSource::Immutable { name, .. } | ValueSource::Constant { name, .. }) = source
    else {
        return None;
    };
    Some(format!("{}::{name}", project.state_var_owner(context, name)?.key()))
}

/// `<target>.<function>` of a call, as the effect summaries name it.
fn call_text(stmt: &CfgStatement, decls: &DeclTable) -> Option<String> {
    CallSite::of(stmt, decls).map(|c| c.call).or_else(|| match stmt {
        CfgStatement::ExternalCall { target, function, .. } => Some(format!("{target}.{function}")),
        _ => None,
    })
}

/// The parameter an identifier names: by declaration id, or by name when the model has none.
fn param_source(params: &[Param], name: &str, resolved: Option<DeclId>) -> Option<ValueSource> {
    let index = params.iter().position(|p| match resolved {
        Some(id) => p.id == Some(id),
        None => p.name == name,
    })?;
    Some(ValueSource::Param { index, name: params[index].name.clone() })
}
