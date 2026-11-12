//! Facts that relate code across contracts, computed once from the analysis; every surface reads them from here.
mod dispatch;
mod keys;
mod phases;
mod route;
mod trycatch;
mod value;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::analysis::context_cfg;
use crate::cfg::types::{CfgGraph, CfgStatement};
use crate::model::common::{Param, SourceSpan};
use crate::model::contract::ContractDef;
use crate::model::decl_id::{DeclId, DeclTable};
use crate::model::expression::CallKind;
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::model::source::ValueSource;
use crate::sequence::effects::{reachable, resolve_call, CallSite};

pub use dispatch::{call_target, external_calls, ExternalCallSite};
pub use keys::{key_kinds, storage_accesses, AccessKey, AccessKind, KeyKind, KeySite, KeysReason, RecordKeys, StorageAccess};
pub use phases::{phase_candidates, PhaseCandidate};
pub use route::{routes, Route, RouteCall, RouteStep, Routes};
pub use trycatch::{try_catches, CatchClauseFact, CatchEffects, CatchKind, EffectSite, GuardedCall, TryCatchFact, TryReason};
pub use value::{
    value_flows, AssetKind, BalanceRead, Direction, Operation, Party, ValueFacts, ValueFlow, ValuePair, ValueReason, ValueTransfer,
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
    /// Project contracts only, sorted by context, then the parties.
    pub value_pairs: Vec<ValuePair>,
}

/// `context` runs `func`, declared in `owner`, as `cfg`.
pub(crate) struct Running<'p> {
    pub context: &'p ContractDef,
    pub owner: &'p ContractDef,
    pub func: &'p FunctionDef,
    pub cfg: Cow<'p, CfgGraph>,
}

/// Each analysed function in its own contract, then in every project contract each base function it runs: base constructors, inherited code with private helpers, and overridden code its code still calls, as through `super`.
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
        let mut pending: Vec<usize> = (0..units.len()).filter(|&i| std::ptr::eq(units[i].context, context)).collect();
        let mut runs: HashSet<(&str, &str)> = pending.iter().map(|&i| (units[i].owner.key(), units[i].func.key.as_str())).collect();
        while let Some(i) = pending.pop() {
            for (owner, func) in internal_callees(project, &units[i]) {
                if !bases.iter().any(|b| std::ptr::eq(*b, owner)) || !runs.insert((owner.key(), func.key.as_str())) {
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
        (relations.values, relations.value_pairs) = value::build(project, &units, &relations);
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

    pub fn incoming<'a>(&'a self, contract: &'a str) -> impl Iterator<Item = &'a ContractLink> {
        self.links.iter().filter(move |l| l.to_contract == contract)
    }

    /// The calls the code `context` runs makes, its inherited code included.
    pub fn calls_from(&self, context: &str) -> &[ContractCall] {
        let start = self.calls.partition_point(|c| c.in_context.as_str() < context);
        let end = self.calls.partition_point(|c| c.in_context.as_str() <= context);
        &self.calls[start..end]
    }

    /// What `function` (`<contract key>::<function key>`) moves and reads as `context` runs it.
    pub fn values(&self, context: &str, function: &str) -> ValueFacts {
        fn of<'a, T>(facts: &'a [T], key: impl Fn(&T) -> (&str, &str), wanted: (&str, &str)) -> &'a [T] {
            let start = facts.partition_point(|f| key(f) < wanted);
            let end = facts.partition_point(|f| key(f) <= wanted);
            &facts[start..end]
        }
        let wanted = (context, function);
        ValueFacts {
            transfers: of(&self.values.transfers, |t| (&t.in_context, &t.function), wanted).to_vec(),
            balance_reads: of(&self.values.balance_reads, |r| (&r.in_context, &r.function), wanted).to_vec(),
        }
    }

    /// The calls `contract` may answer, from every contract running the calling code.
    pub fn calls_into<'a>(&'a self, contract: &'a str) -> impl Iterator<Item = &'a ContractCall> {
        self.calls.iter().filter(move |c| c.callees.iter().any(|d| d.contract == contract))
    }
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
