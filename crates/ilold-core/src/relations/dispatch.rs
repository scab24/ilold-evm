//! Which in-scope contracts may answer each external call, where the called address comes from, and the contract map they add up to.
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::cfg::builder::expr_to_string;
use crate::cfg::types::{CfgGraph, CfgStatement};
use crate::model::common::SourceSpan;
use crate::model::contract::ContractDef;
use crate::model::decl_id::DeclKind;
use crate::model::expression::{CallKind, Expression, ExpressionKind};
use crate::model::function::{FunctionDef, FunctionKind, Visibility};
use crate::model::project::Project;
use crate::model::source::{uncast, value_source, ValueSource};
use crate::model::statement::walk_all;
use crate::sequence::effects::reachable;

use super::{
    param_source, state_var_ref, AssignmentKind, AssignmentSite, Basis, ContractLink, Dispatch, DispatchCandidate, LinkCall, Relations, Tag,
    UnresolvedReason,
};

/// The kind and the declared `<contract key>::<member key>` of an external call to a contract, `this` included.
pub fn call_target(project: &Project, stmt: &CfgStatement) -> Option<(CallKind, Option<String>)> {
    let CfgStatement::ExternalCall { kind: kind @ (CallKind::External | CallKind::ExternalSelf), resolved, .. } = stmt else {
        return None;
    };
    let target = resolved.and_then(|id| project.decl_table.lookup(id)).and_then(|t| match t.kind {
        DeclKind::Function => Some(format!("{}::{}", t.contract, t.function)),
        DeclKind::StateVariable => Some(format!("{}::{}", t.contract, t.name)),
        _ => None,
    });
    Some((*kind, target))
}

pub(super) fn build(project: &Project, cfgs: &HashMap<(String, String), CfgGraph>) -> Relations {
    let mut keys: Vec<&(String, String)> = cfgs.keys().collect();
    keys.sort();
    let mut dispatches = BTreeMap::new();
    let mut targeted = BTreeSet::new();
    let mut links: BTreeMap<(String, String), ContractLink> = BTreeMap::new();
    for (owner_key, func_key) in keys {
        let Some(owner) = contract(project, owner_key) else { continue };
        let Some(func) = owner.functions.iter().find(|f| &f.key == func_key) else { continue };
        for ExternalCallSite { kind, target, span, via } in external_calls(project, owner, &cfgs[&(owner_key.clone(), func_key.clone())]) {
            let dispatch = dispatches
                .entry((context_of(owner, kind), target))
                .or_insert_with_key(|(context, target)| resolve(project, context.as_deref(), kind, target.as_deref()));
            targeted.extend(via.clone());
            if owner.is_dependency {
                continue;
            }
            let call = LinkCall { function: format!("{owner_key}::{func_key}"), span: span.unwrap_or(func.span) };
            let only = dispatch.candidates.len() == 1;
            for candidate in &dispatch.candidates {
                let link = links.entry((owner_key.clone(), candidate.contract.clone())).or_insert_with(|| ContractLink {
                    from_contract: owner_key.clone(),
                    to_contract: candidate.contract.clone(),
                    via_state_vars: Vec::new(),
                    calls: Vec::new(),
                    basis: candidate.basis,
                    tag: Tag::Ambiguous,
                });
                link.basis = link.basis.min(candidate.basis);
                if only {
                    link.tag = Tag::Inferred;
                }
                if let Some(name) = via.as_deref().and_then(|v| v.rsplit_once("::")).map(|(_, n)| n.to_string()) {
                    if !link.via_state_vars.contains(&name) {
                        link.via_state_vars.push(name);
                    }
                }
                if !link.calls.contains(&call) {
                    link.calls.push(call.clone());
                }
            }
        }
    }
    let mut links: Vec<ContractLink> = links.into_values().collect();
    for link in &mut links {
        link.via_state_vars.sort();
        link.calls.sort_by(|a, b| (&a.function, a.span).cmp(&(&b.function, b.span)));
    }
    let mut assignments: Vec<AssignmentSite> = targeted.iter().flat_map(|var| assignment_sites(project, var)).collect();
    assignments.sort_by(|a, b| (&a.state_var, a.span).cmp(&(&b.state_var, b.span)));
    Relations { dispatches: dispatches.into_values().collect(), assignments, links }
}

fn contract<'p>(project: &'p Project, key: &str) -> Option<&'p ContractDef> {
    project.contract_index.get(key).map(|&i| &project.contracts[i])
}

fn context_of(owner: &ContractDef, kind: CallKind) -> Option<String> {
    (kind == CallKind::ExternalSelf).then(|| owner.key().to_string())
}

/// An external call to a contract in a function's reachable code.
pub struct ExternalCallSite {
    pub kind: CallKind,
    pub target: Option<String>,
    pub span: Option<SourceSpan>,
    /// `<contract key>::<name>` of the state variable holding the called address.
    pub via: Option<String>,
}

/// `context` declares the code `cfg` runs.
pub fn external_calls(project: &Project, context: &ContractDef, cfg: &CfgGraph) -> Vec<ExternalCallSite> {
    reachable(cfg)
        .into_iter()
        .flat_map(|n| cfg[n].statements.iter())
        .filter_map(|stmt| {
            let (kind, target) = call_target(project, stmt)?;
            let CfgStatement::ExternalCall { target_source, span, .. } = stmt else { return None };
            let via = target_source.as_ref().and_then(|s| state_var_ref(project, context, s));
            Some(ExternalCallSite { kind, target, span: *span, via })
        })
        .collect()
}

pub(super) fn resolve(project: &Project, context: Option<&str>, kind: CallKind, target: Option<&str>) -> Dispatch {
    let declared = target.and_then(|t| t.rsplit_once("::")).and_then(|(c, m)| Some((contract(project, c)?, m)));
    let candidates = match (kind, context.and_then(|c| contract(project, c))) {
        (CallKind::ExternalSelf, Some(context)) => self_candidates(project, context, declared, target),
        _ => declared.map(|(c, m)| implementations(project, c, m)).unwrap_or_default(),
    };
    let unresolved_reason = candidates.is_empty().then(|| match declared {
        None if target.is_none() => UnresolvedReason::DynamicAddress,
        Some((c, _)) if c.is_dependency => UnresolvedReason::OutOfScopeInterface,
        _ => UnresolvedReason::NoImplementation,
    });
    Dispatch { context: context.map(str::to_string), target: target.map(str::to_string), candidates, unresolved_reason }
}

/// `this.f()` runs the implementation the calling contract sees.
fn self_candidates(
    project: &Project,
    context: &ContractDef,
    declared: Option<(&ContractDef, &str)>,
    target: Option<&str>,
) -> Vec<DispatchCandidate> {
    let signature = declared.and_then(|(c, m)| c.functions.iter().find(|f| f.key == m)).map(FunctionDef::match_signature);
    let name = target.and_then(|t| t.rsplit_once("::")).map(|(_, m)| m);
    let linearization = project.linearization(context);
    let found = match signature {
        Some(signature) => project.implementation(&linearization, signature, None),
        None => linearization.iter().find_map(|c| c.functions.iter().find(|f| Some(f.name.as_str()) == name).map(|f| (*c, f))),
    };
    found
        .map(|(owner, f)| DispatchCandidate {
            function: format!("{}::{}", owner.key(), f.key),
            contract: context.key().to_string(),
            basis: Basis::SelfCall,
            tag: Tag::Inferred,
        })
        .into_iter()
        .collect()
}

/// Deployable project contracts answering `member` of `declared`: those inheriting it, else any exposing its selector.
fn implementations(project: &Project, declared: &ContractDef, member: &str) -> Vec<DispatchCandidate> {
    let function = declared.functions.iter().find(|f| f.key == member);
    let selector = function
        .and_then(|f| f.selector.as_deref())
        .or_else(|| declared.state_vars.iter().find(|v| v.name == member).and_then(|v| v.selector.as_deref()));
    let Some(selector) = selector else { return Vec::new() };
    let mut inheriting = Vec::new();
    let mut matching = Vec::new();
    for c in project.project_contracts().filter(|c| c.is_deployable()) {
        let Some(answer) = answering(project, c, selector) else { continue };
        let inherits = project.linearization(c).iter().any(|base| base.key() == declared.key());
        let (list, basis) = if inherits { (&mut inheriting, Basis::InheritsInterface) } else { (&mut matching, Basis::SelectorMatch) };
        list.push(DispatchCandidate { function: answer, contract: c.key().to_string(), basis, tag: Tag::Inferred });
    }
    let mut candidates = if inheriting.is_empty() { matching } else { inheriting };
    if candidates.len() > 1 {
        candidates.iter_mut().for_each(|c| c.tag = Tag::Ambiguous);
    }
    candidates
}

/// The public function or getter of `c` with `selector`, as `<declaring contract key>::<member key>`.
fn answering(project: &Project, c: &ContractDef, selector: &str) -> Option<String> {
    let function = project.accessible_functions(c).into_iter().find(|af| {
        let f = af.function;
        f.kind != FunctionKind::Constructor
            && matches!(f.visibility, Visibility::Public | Visibility::External)
            && f.selector.as_deref() == Some(selector)
    });
    if let Some(af) = function {
        return Some(format!("{}::{}", af.owner.key(), af.function.key));
    }
    project
        .accessible_state_vars(c)
        .into_iter()
        .find(|(v, _, _)| v.visibility == Visibility::Public && v.selector.as_deref() == Some(selector))
        .map(|(v, owner, _)| format!("{owner}::{}", v.name))
}

/// The declaration's initial value and every direct assignment in a contract that inherits the variable.
fn assignment_sites(project: &Project, state_var: &str) -> Vec<AssignmentSite> {
    let Some((owner, name)) = state_var.rsplit_once("::").and_then(|(c, n)| Some((contract(project, c)?, n))) else {
        return Vec::new();
    };
    let Some(var) = owner.state_vars.iter().find(|v| v.name == name) else { return Vec::new() };
    let mut sites = Vec::new();
    if let Some(value) = &var.initial_value {
        let literal = matches!(value_source(Some(project), &owner.state_vars, value, |_, _| None), ValueSource::Literal { .. });
        let kind = if literal { AssignmentKind::Literal } else { AssignmentKind::Other };
        sites.push(site(state_var, None, value.span, value, kind));
    }
    let heirs = project.contracts.iter().filter(|c| project.linearization(c).iter().any(|b| b.key() == owner.key()));
    for c in heirs {
        for f in c.functions.iter() {
            let Some(body) = &f.body else { continue };
            let function = format!("{}::{}", c.key(), f.key);
            walk_all(body, &mut |_| {}, &mut |e: &Expression| {
                let ExpressionKind::Assignment { target, value, .. } = &e.kind else { return };
                let assigns = match &uncast(target).kind {
                    ExpressionKind::Identifier { name, resolved } => match (resolved, var.id) {
                        (Some(id), Some(var_id)) => *id == var_id,
                        _ => name == &var.name,
                    },
                    _ => false,
                };
                if assigns {
                    sites.push(site(state_var, Some(function.clone()), e.span, value, assignment_kind(project, c, f, value)));
                }
            });
        }
    }
    sites
}

fn site(state_var: &str, function: Option<String>, span: SourceSpan, value: &Expression, kind: AssignmentKind) -> AssignmentSite {
    AssignmentSite { state_var: state_var.to_string(), function, span, expr_text: expr_to_string(value), kind }
}

fn assignment_kind(project: &Project, c: &ContractDef, f: &FunctionDef, value: &Expression) -> AssignmentKind {
    let initializer = f.name == "initialize" || f.modifiers.iter().any(|m| matches!(m.name.as_str(), "initializer" | "reinitializer"));
    if f.kind != FunctionKind::Constructor {
        return if initializer { AssignmentKind::Initializer } else { AssignmentKind::Setter };
    }
    let source = value_source(Some(project), &c.state_vars, value, |name, resolved| param_source(&f.params, name, resolved));
    match source {
        ValueSource::Param { .. } => AssignmentKind::ConstructorParam,
        ValueSource::Literal { .. } => AssignmentKind::Literal,
        _ => AssignmentKind::Other,
    }
}
