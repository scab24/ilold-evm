//! State variables that gate which entry points can run: the phases of a contract.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::cfg::builder::{expr_to_string, modifier_code};
use crate::classify::guards::check_call;
use crate::model::common::{SourceSpan, StateVar};
use crate::model::contract::ContractDef;
use crate::model::decl_id::{DeclId, DeclKind};
use crate::model::expression::{BinaryOperator, CallKind, CallType, Expression, ExpressionKind, LiteralType};
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::model::source::uncast;
use crate::model::statement::{walk_all, Statement, StatementKind};
use crate::sequence::analysis::{find_behavior, FunctionBehavior};
use crate::sequence::effects::{resolve_call, CallSite};

use super::Tag;

/// A bool, an enum, or a uint compared with a constant, tested in the conditions of two or more entry points and written by one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseCandidate {
    /// `<contract key>::<name>` of the declaration.
    pub state_var: String,
    /// Key of the deployable contract whose entry points are counted.
    pub in_context: String,
    /// The declared type, as solc writes it.
    pub type_name: String,
    /// `<contract key>::<function key>` of each entry point whose code, modifiers or internal helpers test it.
    pub guarded_functions: Vec<String>,
    /// Entry points that write it, directly or through internal helpers.
    pub writers: Vec<String>,
    /// Every distinct condition that tests it, as the source writes it.
    pub condition_texts: Vec<String>,
    /// The declaration.
    pub span: SourceSpan,
    pub tag: Tag,
}

impl PhaseCandidate {
    /// `function` is `<contract key>::<function key>`.
    pub fn involves(&self, function: &str) -> bool {
        self.guarded_functions.iter().chain(&self.writers).any(|f| f == function)
    }
}

/// The phase candidates of a deployable project contract, by state variable; `behaviors` are the contract's own.
pub fn phase_candidates(project: &Project, context: &ContractDef, behaviors: &[FunctionBehavior]) -> Vec<PhaseCandidate> {
    if context.is_dependency || !context.is_deployable() {
        return Vec::new();
    }
    let entries: Vec<(&ContractDef, &FunctionDef)> =
        project.accessible_functions(context).into_iter().filter(|af| af.function.is_entry_point()).map(|af| (af.owner, af.function)).collect();
    let mut code = Code { project, context, direct: HashMap::new() };
    let mut tested: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    for &(owner, func) in &entries {
        for (state_var, text) in code.reached(owner, func) {
            let (functions, texts) = tested.entry(state_var).or_default();
            functions.insert(label(owner, func));
            texts.insert(text);
        }
    }
    let mut out = Vec::new();
    for (state_var, (functions, texts)) in tested {
        // a single function's own check is a precondition, not a phase
        if functions.len() < 2 {
            continue;
        }
        let Some((declared_in, name)) = state_var.rsplit_once("::") else { continue };
        let Some(var) = project.state_var_owner(context, name).filter(|c| c.key() == declared_in).and_then(|c| c.state_vars.iter().find(|v| v.name == name))
        else {
            continue;
        };
        let writes = |&&(owner, func): &&(&ContractDef, &FunctionDef)| {
            find_behavior(behaviors, owner.key(), &func.key).is_some_and(|b| b.effective_state_writes().iter().any(|w| w == name))
        };
        let writers: BTreeSet<String> = entries.iter().filter(writes).map(|&(owner, func)| label(owner, func)).collect();
        if writers.is_empty() {
            continue;
        }
        out.push(PhaseCandidate {
            state_var,
            in_context: context.key().to_string(),
            type_name: var.type_name.clone(),
            guarded_functions: functions.into_iter().collect(),
            writers: writers.into_iter().collect(),
            condition_texts: texts.into_iter().collect(),
            span: var.span,
            tag: Tag::Inferred,
        });
    }
    out
}

fn label(owner: &ContractDef, func: &FunctionDef) -> String {
    format!("{}::{}", owner.key(), func.key)
}

/// What one function's code and modifiers test, and the internal functions it calls.
struct Direct<'p> {
    tests: Vec<(String, String)>,
    callees: Vec<(&'p ContractDef, &'p FunctionDef)>,
}

/// The code functions run in `context`, read once per function.
struct Code<'p> {
    project: &'p Project,
    context: &'p ContractDef,
    direct: HashMap<(String, String), Direct<'p>>,
}

impl<'p> Code<'p> {
    /// Each state variable tested on the way through the function, its modifiers and its internal helpers, with the condition.
    fn reached(&mut self, owner: &'p ContractDef, func: &'p FunctionDef) -> BTreeSet<(String, String)> {
        let mut out = BTreeSet::new();
        let mut seen = HashSet::new();
        let mut stack = vec![(owner, func)];
        while let Some((owner, func)) = stack.pop() {
            let key = (owner.key().to_string(), func.key.clone());
            if !seen.insert(key.clone()) {
                continue;
            }
            if !self.direct.contains_key(&key) {
                let direct = self.read(owner, func);
                self.direct.insert(key.clone(), direct);
            }
            let direct = &self.direct[&key];
            out.extend(direct.tests.iter().cloned());
            stack.extend(direct.callees.iter().copied());
        }
        out
    }

    fn read(&self, owner: &'p ContractDef, func: &'p FunctionDef) -> Direct<'p> {
        let (project, context) = (self.project, self.context);
        let modifiers = func.modifiers.iter().filter_map(|m| modifier_code(Some(project), context, &m.name)).map(|m| m.body.as_slice());
        let mut branches: Vec<&'p Expression> = Vec::new();
        let mut checks: Vec<&'p Expression> = Vec::new();
        let mut calls: Vec<(&'p Expression, CallKind)> = Vec::new();
        for body in std::iter::once(func.body.as_deref().unwrap_or_default()).chain(modifiers) {
            walk_all(
                body,
                &mut |stmt: &'p Statement| match &stmt.kind {
                    StatementKind::If { condition, .. } | StatementKind::While { condition, .. } | StatementKind::DoWhile { condition, .. } => {
                        branches.push(condition)
                    }
                    StatementKind::For { condition: Some(condition), .. } => branches.push(condition),
                    _ => {}
                },
                &mut |expr: &'p Expression| match &expr.kind {
                    ExpressionKind::Ternary { condition, .. } => checks.push(condition),
                    ExpressionKind::FunctionCall { callee, call: CallType::Call(kind @ CallKind::Internal), .. } => calls.push((callee, *kind)),
                    _ => {
                        if let Some(("require" | "assert", [condition, ..])) = check_call(expr) {
                            checks.push(condition);
                        }
                    }
                },
            );
        }
        let tests = branches.into_iter().chain(checks).flat_map(|cond| tested(project, cond).into_iter().map(|var| (var, expr_to_string(cond)))).collect();
        let callees = calls
            .into_iter()
            .filter_map(|(callee, kind)| {
                let declaration = resolved(callee).and_then(|id| project.decl_table.function(id)).map(|t| (t.contract.clone(), t.function.clone()));
                let call = CallSite { call: expr_to_string(callee), kind, declaration, storage_args: Vec::new(), modifier: None };
                resolve_call(project, context, owner, &call)
            })
            .collect();
        Direct { tests, callees }
    }
}

fn resolved(expr: &Expression) -> Option<DeclId> {
    match &expr.kind {
        ExpressionKind::Identifier { resolved, .. } | ExpressionKind::MemberAccess { resolved, .. } => *resolved,
        _ => None,
    }
}

/// `<contract key>::<name>` and declaration of the state variable an identifier resolves to.
fn declared_var<'p>(project: &'p Project, expr: &Expression) -> Option<(String, &'p StateVar)> {
    let ExpressionKind::Identifier { resolved: Some(id), .. } = &uncast(expr).kind else { return None };
    let target = project.decl_table.lookup(*id).filter(|t| t.kind == DeclKind::StateVariable)?;
    let contract = &project.contracts[*project.contract_index.get(&target.contract)?];
    let var = contract.state_vars.iter().find(|v| v.name == target.name)?;
    Some((format!("{}::{}", target.contract, target.name), var))
}

/// The state variables a condition tests by declaration: any bool or enum, a uint only where it is compared for equality with a constant.
fn tested(project: &Project, cond: &Expression) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    cond.walk(&mut |e| match &e.kind {
        ExpressionKind::Identifier { .. } => {
            if let Some((key, var)) = declared_var(project, e).filter(|(_, v)| v.is_storage()) {
                if var.type_name == "bool" || var.type_name.starts_with("enum ") {
                    found.insert(key);
                }
            }
        }
        ExpressionKind::BinaryOp { left, operator: BinaryOperator::Eq | BinaryOperator::Neq, right } => {
            for (var, other) in [(left, right), (right, left)] {
                let Some((key, var)) = declared_var(project, var).filter(|(_, v)| v.is_storage()) else { continue };
                if var.type_name.starts_with("uint") && is_constant(project, other) {
                    found.insert(key);
                }
            }
        }
        _ => {}
    });
    found
}

fn is_constant(project: &Project, expr: &Expression) -> bool {
    match &uncast(expr).kind {
        ExpressionKind::Literal { literal_type: LiteralType::Number, .. } => true,
        _ => declared_var(project, expr).is_some_and(|(_, v)| v.is_constant),
    }
}
