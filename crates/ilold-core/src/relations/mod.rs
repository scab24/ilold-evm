//! Facts that relate code across contracts, computed once from the analysis; every surface reads them from here.
mod dispatch;

use std::borrow::Cow;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::cfg::types::CfgGraph;
use crate::model::common::SourceSpan;
use crate::model::contract::ContractDef;
use crate::model::expression::CallKind;
use crate::model::project::Project;
use crate::model::source::ValueSource;

pub use dispatch::{call_target, external_calls, ExternalCallSite};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Tag {
    Inferred,
    Ambiguous,
}

/// Strongest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Relations {
    /// Sorted by context, then target.
    pub dispatches: Vec<Dispatch>,
    /// Sorted by state variable, then position.
    pub assignments: Vec<AssignmentSite>,
    /// Sorted by source, then target contract.
    pub links: Vec<ContractLink>,
}

impl Relations {
    pub fn build(project: &Project, cfgs: &HashMap<(String, String), CfgGraph>) -> Self {
        dispatch::build(project, cfgs)
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
