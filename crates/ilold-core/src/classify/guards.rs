//! Access and state guards of a function, read from its code.
use std::cell::Cell;
use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::cfg::builder::{call_arguments, expr_to_string};
use crate::model::common::SourceSpan;
use crate::model::contract::ContractDef;
use crate::model::decl_id::DeclId;
use crate::model::expression::{BinaryOperator, CallKind, CallType, Expression, ExpressionKind, LiteralType, UnaryOperator};
use crate::model::function::FunctionDef;
use crate::model::modifier::{ModifierDef, ModifierRef};
use crate::model::project::Project;
use crate::model::source::{cast_operand, uncast, value_source, ValueSource};
use crate::model::statement::{walk_all, Statement, StatementKind};
use crate::sequence::effects::{resolve_call, CallSite};

/// Helpers deeper than this are not followed.
pub(crate) const MAX_DEPTH: usize = 12;

/// Used only for modifiers without code.
const ACCESS_MODIFIERS: &[&str] = &[
    "onlyowner", "onlyadmin", "onlyrole", "onlygovernance",
    "onlyminter", "onlypauser", "onlyoperator", "onlyguardian",
    "onlyauthorized", "onlymanager", "onlycontroller",
    "onlybridge", "onlyrelayer", "onlyvault",
    "auth", "restricted",
];

/// Used only for modifiers without code.
const STATE_GUARD_MODIFIERS: &[(&str, GuardKind)] = &[
    ("whennotpaused", GuardKind::Pause),
    ("whenpaused", GuardKind::Pause),
    ("nonreentrant", GuardKind::Reentrancy),
    ("noreentrant", GuardKind::Reentrancy),
    ("reinitializer", GuardKind::Initializer),
    ("initializer", GuardKind::Initializer),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum GuardKind {
    CallerEquals,
    /// The caller passed to a boolean check or used as a mapping key.
    RoleCheck,
    TxOrigin,
    /// A modifier without code whose name suggests access control.
    AccessByName,
    Pause,
    Reentrancy,
    Initializer,
}

impl GuardKind {
    pub fn is_access(self) -> bool {
        matches!(self, GuardKind::CallerEquals | GuardKind::RoleCheck | GuardKind::TxOrigin | GuardKind::AccessByName)
    }

    pub fn label(self) -> &'static str {
        match self {
            GuardKind::CallerEquals => "caller",
            GuardKind::RoleCheck => "role",
            GuardKind::TxOrigin => "tx.origin",
            GuardKind::AccessByName => "access",
            GuardKind::Pause => "pause",
            GuardKind::Reentrancy => "reentrancy",
            GuardKind::Initializer => "initializer",
        }
    }
}

/// `via` is the chain from the function to the check, empty for the body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardFact {
    pub kind: GuardKind,
    pub condition: String,
    pub via: Vec<String>,
    pub span: Option<SourceSpan>,
    /// Taken from a modifier name, not from code.
    #[serde(default)]
    pub heuristic: bool,
    #[serde(default)]
    pub scope: GuardScope,
    /// What the caller is compared with, or the role a role check asks for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compared_against: Option<ValueSource>,
    /// The function or mapping a role check consults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_by: Option<String>,
}

/// A conditional guard holds on some paths only, so it never restricts the function.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum GuardScope {
    #[default]
    Unconditional,
    Conditional,
}

impl GuardFact {
    /// Only an access check on every path restricts who may call.
    pub fn restricts(&self) -> bool {
        self.kind.is_access() && self.scope == GuardScope::Unconditional
    }
}

#[derive(Debug, Clone)]
pub struct GuardScan {
    pub facts: Vec<GuardFact>,
    /// A helper chain deeper than `MAX_DEPTH` was cut.
    pub incomplete: bool,
}

/// `context` is the most derived contract: modifiers and virtual helpers resolve as it sees them.
pub fn function_guards(project: Option<&Project>, context: &ContractDef, owner: &ContractDef, func: &FunctionDef) -> Vec<GuardFact> {
    function_guard_scan(project, context, owner, func).facts
}

pub fn function_guard_scan(project: Option<&Project>, context: &ContractDef, owner: &ContractDef, func: &FunctionDef) -> GuardScan {
    let mut walker = Walker { project, facts: Vec::new(), pending: Vec::new(), active: Vec::new(), capped: Cell::new(false) };
    let sources = func
        .params
        .iter()
        .enumerate()
        .map(|(index, p)| (var_key(&p.name, p.id), ValueSource::Param { index, name: p.name.clone() }))
        .collect();
    let frame = Frame { context, owner, via: Vec::new(), callers: HashMap::new(), sources, depth: 0, conditional: false };
    walker.function(&frame, func);
    GuardScan { facts: walker.facts, incomplete: walker.capped.get() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Caller {
    Sender,
    Origin,
}

impl Caller {
    fn kind(self, compared: bool) -> GuardKind {
        match (self, compared) {
            (Caller::Origin, _) => GuardKind::TxOrigin,
            (Caller::Sender, true) => GuardKind::CallerEquals,
            (Caller::Sender, false) => GuardKind::RoleCheck,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum VarKey {
    Id(DeclId),
    Name(String),
}

fn var_key(name: &str, id: Option<DeclId>) -> VarKey {
    match id {
        Some(id) => VarKey::Id(id),
        None => VarKey::Name(name.to_string()),
    }
}

/// A caller test; a role check also names the role and what consults it.
#[derive(Clone)]
struct Access {
    kind: GuardKind,
    against: Option<ValueSource>,
    checked_by: Option<String>,
}

#[derive(Clone)]
struct Frame<'a> {
    context: &'a ContractDef,
    owner: &'a ContractDef,
    via: Vec<String>,
    callers: HashMap<VarKey, Caller>,
    /// Parameters, bound to what the caller passed.
    sources: HashMap<VarKey, ValueSource>,
    depth: usize,
    /// Control may have left the function, or a branch was taken, before this point.
    conditional: bool,
}

impl Frame<'_> {
    fn scope(&self) -> GuardScope {
        if self.conditional { GuardScope::Conditional } else { GuardScope::Unconditional }
    }
}

/// Neither about the caller nor a flag; inside a modifier it may still be a reentrancy lock.
struct Pending {
    condition: String,
    via: Vec<String>,
    span: SourceSpan,
    names: BTreeSet<String>,
    scope: GuardScope,
}

struct Walker<'a> {
    project: Option<&'a Project>,
    facts: Vec<GuardFact>,
    pending: Vec<Pending>,
    // (contract key, function key) being read, to stop recursion
    active: Vec<(String, String)>,
    capped: Cell<bool>,
}

impl<'a> Walker<'a> {
    fn push(&mut self, fact: GuardFact) {
        let same = |f: &&mut GuardFact| f.kind == fact.kind && f.condition == fact.condition && f.via == fact.via;
        match self.facts.iter_mut().find(same) {
            // reached on every path after all
            Some(found) if found.scope > fact.scope => *found = fact,
            Some(_) => {}
            None => self.facts.push(fact),
        }
    }

    fn cap(&self, depth: usize) -> bool {
        let capped = depth > MAX_DEPTH;
        if capped {
            self.capped.set(true);
        }
        capped
    }

    fn function(&mut self, frame: &Frame<'a>, func: &'a FunctionDef) {
        let key = (frame.owner.key().to_string(), func.key.clone());
        if self.cap(frame.depth) || self.active.contains(&key) {
            return;
        }
        self.active.push(key);
        for mref in &func.modifiers {
            self.modifier(frame, mref);
        }
        if let Some(body) = &func.body {
            self.statements(frame, body);
        }
        self.active.pop();
    }

    fn modifier(&mut self, frame: &Frame<'a>, mref: &ModifierRef) {
        let mut via = frame.via.clone();
        via.push(format!("modifier {}", mref.name));
        let found = self.resolve_modifier(frame, &mref.name);
        let Some((declared_in, def)) = found.filter(|(_, d)| !d.body.is_empty()) else {
            self.by_name(&mref.name, via, frame.scope());
            return;
        };
        let callers = def
            .params
            .iter()
            .zip(&mref.arguments)
            .filter_map(|(p, arg)| Some((var_key(&p.name, p.id), self.caller_of(frame, arg)?)))
            .collect();
        let sources = def.params.iter().zip(&mref.arguments).map(|(p, arg)| (var_key(&p.name, p.id), self.bound_source(frame, arg))).collect();
        let inner = Frame { owner: declared_in, via: via.clone(), callers, sources, ..frame.clone() };
        let split = def.body.iter().position(|s| matches!(s.kind, StatementKind::Placeholder)).unwrap_or(def.body.len());
        let (before, after) = def.body.split_at(split);

        let facts = self.facts.len();
        let pending = self.pending.len();
        self.statements(&inner, before);

        // a lock: a flag checked and set before `_`, reset after it
        let mut set_before = BTreeSet::new();
        let mut set_after = BTreeSet::new();
        self.assigned(&inner, before, &mut set_before);
        self.assigned(&inner, after, &mut set_after);
        let candidates: Vec<Pending> = self.pending.drain(pending..).collect();
        for p in candidates {
            if p.names.iter().any(|n| set_before.contains(n) && set_after.contains(n)) {
                let kind = GuardKind::Reentrancy;
                let (via, span, scope) = (p.via, Some(p.span), p.scope);
                self.push(GuardFact { kind, condition: p.condition, via, span, heuristic: false, scope, compared_against: None, checked_by: None });
            }
        }
        // any fact read from code, conditional ones too, rules the name out: a false restriction is worse than a gap
        if self.facts.len() == facts && contains(before, |k| matches!(k, StatementKind::Assembly { .. })) {
            self.by_name(&mref.name, via, frame.scope());
        }
    }

    fn by_name(&mut self, name: &str, via: Vec<String>, scope: GuardScope) {
        let lower = name.to_lowercase();
        let kind = if ACCESS_MODIFIERS.iter().any(|p| lower.contains(p)) {
            GuardKind::AccessByName
        } else if let Some((_, kind)) = STATE_GUARD_MODIFIERS.iter().find(|(p, _)| lower.contains(p)) {
            *kind
        } else {
            return;
        };
        let condition = format!("{name} (no code to read; assumed from the name)");
        self.push(GuardFact { kind, condition, via, span: None, heuristic: true, scope, compared_against: None, checked_by: None });
    }

    fn resolve_modifier(&self, frame: &Frame<'a>, name: &str) -> Option<(&'a ContractDef, &'a ModifierDef)> {
        let find = |c: &'a ContractDef| c.modifiers.iter().find(|m| m.name == name).map(|m| (c, m));
        let Some(project) = self.project else {
            return find(frame.owner).or_else(|| find(frame.context));
        };
        let linearization = project.linearization(frame.context);
        let order = if linearization.iter().any(|c| c.key() == frame.owner.key()) {
            linearization
        } else {
            project.linearization(frame.owner)
        };
        order.into_iter().find_map(find)
    }

    // false once control may have left the function: later checks hold on some paths only
    fn statements(&mut self, frame: &Frame<'a>, stmts: &'a [Statement]) -> bool {
        let mut frame = frame.clone();
        let branch = |frame: &Frame<'a>| Frame { conditional: true, ..frame.clone() };
        for stmt in stmts {
            match &stmt.kind {
                StatementKind::ExpressionStmt { expression } => {
                    if let Some((check, args)) = check_call(expression) {
                        match check {
                            "revert" => return false,
                            _ => {
                                let Some(cond) = args.first() else { continue };
                                let text = format!("{check}({})", expr_to_string(cond));
                                self.check(&frame, cond, true, text, stmt.span);
                            }
                        }
                        continue;
                    }
                    self.calls(&frame, expression);
                    if let ExpressionKind::Assignment { target, value, .. } = &expression.kind {
                        if let ExpressionKind::Identifier { name, resolved } = &target.kind {
                            let key = var_key(name, *resolved);
                            match self.caller_of(&frame, value) {
                                Some(c) => frame.callers.insert(key, c),
                                None => frame.callers.remove(&key),
                            };
                        }
                    }
                }
                StatementKind::VariableDeclaration { name, id, initial_value: Some(value), .. } => {
                    self.calls(&frame, value);
                    if let Some(c) = self.caller_of(&frame, value) {
                        frame.callers.insert(var_key(name, *id), c);
                    }
                }
                StatementKind::If { condition, then_body, else_body } => {
                    if let (None, Some(revert)) = (else_body, revert_text(then_body)) {
                        let text = format!("if ({}) {revert}", expr_to_string(condition));
                        // `if (c) revert`: execution goes on only when `c` is false
                        self.check(&frame, condition, false, text, stmt.span);
                        continue;
                    }
                    self.statements(&branch(&frame), then_body);
                    if let Some(else_body) = else_body {
                        self.statements(&branch(&frame), else_body);
                    }
                    frame.conditional |= may_exit(then_body) || else_body.as_deref().is_some_and(may_exit);
                }
                // the body may run zero times
                StatementKind::For { body, .. } | StatementKind::While { body, .. } | StatementKind::DoWhile { body, .. } => {
                    self.statements(&branch(&frame), body);
                    frame.conditional |= may_exit(body);
                }
                StatementKind::TryCatch { clauses, .. } => {
                    for clause in clauses {
                        self.statements(&branch(&frame), &clause.body);
                    }
                    frame.conditional |= clauses.iter().any(|c| may_exit(&c.body));
                }
                // Yul can `return` or `stop`
                StatementKind::Assembly { .. } => frame.conditional = true,
                StatementKind::Block { statements } | StatementKind::UncheckedBlock { statements } => {
                    frame.conditional |= !self.statements(&frame, statements);
                }
                StatementKind::Return { value } => {
                    if let Some(value) = value {
                        self.calls(&frame, value);
                    }
                    return false;
                }
                StatementKind::Revert { .. } | StatementKind::Placeholder => return false,
                _ => {}
            }
        }
        !frame.conditional
    }

    /// `holds` is the value `cond` must have for execution to go on.
    fn check(&mut self, frame: &Frame<'a>, cond: &'a Expression, holds: bool, condition: String, span: SourceSpan) {
        let (via, scope) = (frame.via.clone(), frame.scope());
        if let Some(access) = self.access_kind(frame, cond, holds, 0) {
            let (kind, compared_against, checked_by) = (access.kind, access.against, access.checked_by);
            self.push(GuardFact { kind, condition, via, span: Some(span), heuristic: false, scope, compared_against, checked_by });
            return;
        }
        let names = names_in(cond);
        let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
        let kind = if lower.iter().any(|n| n.contains("paused")) {
            Some(GuardKind::Pause)
        } else if lower.iter().any(|n| n.contains("initializ")) {
            Some(GuardKind::Initializer)
        } else {
            None
        };
        match kind {
            Some(kind) => {
                self.push(GuardFact { kind, condition, via, span: Some(span), heuristic: false, scope, compared_against: None, checked_by: None })
            }
            None => self.pending.push(Pending { condition, via, span, names, scope }),
        }
    }

    /// Only a caller-dependent test that must hold restricts access; a disjunction only when every disjunct does.
    fn access_kind(&self, frame: &Frame<'a>, cond: &'a Expression, holds: bool, depth: usize) -> Option<Access> {
        let compared = |kind: GuardKind, other: &'a Expression| Access { kind, against: Some(self.source(frame, other, 0)), checked_by: None };
        match &cond.kind {
            ExpressionKind::UnaryOp { operator: UnaryOperator::Not, operand } => self.access_kind(frame, operand, !holds, depth),
            ExpressionKind::Tuple { elements } if elements.len() == 1 => {
                self.access_kind(frame, elements[0].as_ref()?, holds, depth)
            }
            ExpressionKind::BinaryOp { left, operator: op @ (BinaryOperator::And | BinaryOperator::Or), right } => {
                let (l, r) = (self.access_kind(frame, left, holds, depth), self.access_kind(frame, right, holds, depth));
                // `a && b` holding, or `a || b` failing, needs both sides
                let both_needed = matches!(op, BinaryOperator::And) == holds;
                if both_needed { l.or(r) } else { r.and(l) }
            }
            ExpressionKind::BinaryOp { left, operator: op @ (BinaryOperator::Eq | BinaryOperator::Neq), right } => {
                let equal = matches!(op, BinaryOperator::Eq) == holds;
                if let Some(b) = bool_literal(right) {
                    return self.access_kind(frame, left, equal == b, depth);
                }
                if let Some(b) = bool_literal(left) {
                    return self.access_kind(frame, right, equal == b, depth);
                }
                // only an equality that must hold restricts the caller
                if !equal {
                    return None;
                }
                match (self.caller_of(frame, left), self.caller_of(frame, right)) {
                    (Some(c), None) if !is_zero(right) => Some(compared(c.kind(true), right)),
                    (None, Some(c)) if !is_zero(left) => Some(compared(c.kind(true), left)),
                    // `tx.origin == msg.sender`: only externally owned accounts
                    (Some(a), Some(b)) if a != b => {
                        Some(Access { kind: GuardKind::TxOrigin, against: Some(ValueSource::MsgSender), checked_by: None })
                    }
                    _ => None,
                }
            }
            // a test that passes for some callers only when it must be true
            _ if !holds => None,
            ExpressionKind::FunctionCall { callee, arguments, call, .. } => {
                let kind = match call {
                    CallType::Builtin => return None,
                    CallType::Call(kind) => Some(*kind),
                    CallType::Unknown => None,
                };
                // a state-changing call that takes the caller is not a check
                if !self.is_view_call(frame, callee, kind) {
                    return None;
                }
                let args = match kind {
                    Some(kind) => call_arguments(callee, arguments, kind),
                    None => arguments.iter().collect(),
                };
                if let Some(c) = args.iter().find_map(|a| self.caller_of(frame, a)) {
                    let role = args.iter().copied().find(|a| self.caller_of(frame, a).is_none());
                    let against = role.map(|r| self.source(frame, r, 0));
                    return Some(Access { kind: c.kind(false), against, checked_by: Some(expr_to_string(callee)) });
                }
                // a boolean helper every return of which is a guard
                if self.cap(depth + 1) {
                    return None;
                }
                let (inner, func) = self.callee(frame, callee, arguments, kind)?;
                let values = func.returned_values();
                let found: Vec<Option<Access>> = values.iter().map(|r| self.access_kind(&inner, r, true, depth + 1)).collect();
                found.iter().all(Option::is_some).then(|| found.into_iter().next().flatten()).flatten()
            }
            ExpressionKind::IndexAccess { .. } => {
                let mut indexes = Vec::new();
                index_chain(cond, &mut indexes);
                let c = indexes.iter().find_map(|i| self.caller_of(frame, i))?;
                let role = indexes.iter().copied().find(|i| self.caller_of(frame, i).is_none());
                Some(Access { kind: c.kind(false), against: role.map(|r| self.source(frame, r, 0)), checked_by: root_name(cond) })
            }
            ExpressionKind::MemberAccess { object, .. } => self.access_kind(frame, object, holds, depth),
            _ => None,
        }
    }

    /// A public state variable's getter counts; unknown callees do not.
    fn is_view_call(&self, frame: &Frame<'a>, callee: &'a Expression, kind: Option<CallKind>) -> bool {
        let resolved = match &callee.kind {
            ExpressionKind::Identifier { resolved, .. } | ExpressionKind::MemberAccess { resolved, .. } => *resolved,
            _ => None,
        };
        if let Some(view) = self.project.zip(resolved).and_then(|(project, id)| project.is_view_declaration(id)) {
            return view;
        }
        self.resolve(frame, callee, kind).is_some_and(|(_, f)| f.mutability.is_view_or_pure())
    }

    fn caller_of(&self, frame: &Frame<'a>, expr: &'a Expression) -> Option<Caller> {
        self.caller_at(frame, expr, 0)
    }

    fn caller_at(&self, frame: &Frame<'a>, expr: &'a Expression, depth: usize) -> Option<Caller> {
        if let Some(inner) = cast_operand(expr) {
            return self.caller_at(frame, inner, depth);
        }
        match &expr.kind {
            ExpressionKind::MemberAccess { object, member, .. } => match (&object.kind, member.as_str()) {
                (ExpressionKind::Identifier { name, .. }, "sender") if name == "msg" => Some(Caller::Sender),
                (ExpressionKind::Identifier { name, .. }, "origin") if name == "tx" => Some(Caller::Origin),
                _ => None,
            },
            ExpressionKind::Identifier { name, resolved } => frame.callers.get(&var_key(name, *resolved)).copied(),
            ExpressionKind::Tuple { elements } if elements.len() == 1 => {
                self.caller_at(frame, elements[0].as_ref()?, depth)
            }
            ExpressionKind::FunctionCall { callee, arguments, call, .. } => {
                let kind = internal_kind(*call)?;
                if self.cap(depth + 1) {
                    return None;
                }
                // a helper every return of which is the caller
                match self.callee(frame, callee, arguments, kind) {
                    Some((inner, func)) => {
                        let values = func.returned_values();
                        let first = self.caller_at(&inner, values.first()?, depth + 1)?;
                        values[1..].iter().all(|v| self.caller_at(&inner, v, depth + 1) == Some(first)).then_some(first)
                    }
                    None => matches!(&callee.kind, ExpressionKind::Identifier { name, .. } if name == "_msgSender")
                        .then_some(Caller::Sender),
                }
            }
            _ => None,
        }
    }

    fn calls(&mut self, frame: &Frame<'a>, expr: &'a Expression) {
        match &expr.kind {
            ExpressionKind::FunctionCall { callee, arguments, call, options } => {
                if let ExpressionKind::MemberAccess { object, .. } = &callee.kind {
                    self.calls(frame, object);
                }
                for o in options {
                    self.calls(frame, &o.value);
                }
                for a in arguments {
                    self.calls(frame, a);
                }
                let kind = match call {
                    CallType::Call(k @ (CallKind::Internal | CallKind::InternalLibrary | CallKind::LibraryDelegatecall)) => Some(*k),
                    CallType::Unknown if matches!(callee.kind, ExpressionKind::Identifier { .. }) => None,
                    _ => return,
                };
                if let Some((inner, func)) = self.callee(frame, callee, arguments, kind) {
                    self.function(&inner, func);
                }
            }
            ExpressionKind::Assignment { target, value, .. } => {
                self.calls(frame, target);
                self.calls(frame, value);
            }
            ExpressionKind::BinaryOp { left, right, .. } => {
                self.calls(frame, left);
                self.calls(frame, right);
            }
            ExpressionKind::UnaryOp { operand, .. } => self.calls(frame, operand),
            ExpressionKind::MemberAccess { object, .. } => self.calls(frame, object),
            ExpressionKind::IndexAccess { base, index } => {
                self.calls(frame, base);
                if let Some(i) = index {
                    self.calls(frame, i);
                }
            }
            // only the condition runs on every path
            ExpressionKind::Ternary { condition, .. } => self.calls(frame, condition),
            ExpressionKind::Tuple { elements } => {
                for e in elements.iter().flatten() {
                    self.calls(frame, e);
                }
            }
            ExpressionKind::TypeCast { expression, .. } => self.calls(frame, expression),
            _ => {}
        }
    }

    /// Parameters are bound to the caller where the arguments are the caller.
    fn callee(
        &self,
        frame: &Frame<'a>,
        callee: &'a Expression,
        arguments: &'a [Expression],
        kind: Option<CallKind>,
    ) -> Option<(Frame<'a>, &'a FunctionDef)> {
        let (owner, func) = self.resolve(frame, callee, kind)?;
        func.body.as_ref()?;
        let args = match kind {
            Some(kind) => call_arguments(callee, arguments, kind),
            None => arguments.iter().collect(),
        };
        let callers = func
            .params
            .iter()
            .zip(&args)
            .filter_map(|(p, a)| Some((var_key(&p.name, p.id), self.caller_of(frame, a)?)))
            .collect();
        let sources = func.params.iter().zip(&args).map(|(p, a)| (var_key(&p.name, p.id), self.bound_source(frame, a))).collect();
        let shares_storage = match self.project {
            Some(p) => p.linearization(frame.context).iter().any(|c| c.key() == owner.key()),
            None => true,
        };
        let mut via = frame.via.clone();
        via.push(format!("{}::{}", owner.key(), func.key));
        let context = if shares_storage { frame.context } else { owner };
        Some((Frame { context, owner, via, callers, sources, depth: frame.depth + 1, conditional: frame.conditional }, func))
    }

    /// Through helpers every return of which has the same source.
    fn source(&self, frame: &Frame<'a>, expr: &'a Expression, depth: usize) -> ValueSource {
        if let ExpressionKind::FunctionCall { callee, arguments, call, .. } = &uncast(expr).kind {
            let helper = internal_kind(*call).filter(|_| depth < MAX_DEPTH).and_then(|kind| self.callee(frame, callee, arguments, kind));
            if let Some((inner, func)) = helper {
                let values: Vec<ValueSource> = func.returned_values().iter().map(|v| self.source(&inner, v, depth + 1)).collect();
                if let Some(first) = values.first().filter(|first| values.iter().all(|v| v == *first)) {
                    return first.clone();
                }
            }
        }
        self.bound_source(frame, expr)
    }

    fn bound_source(&self, frame: &Frame<'a>, expr: &Expression) -> ValueSource {
        value_source(self.project, &frame.context.state_vars, expr, |name, id| frame.sources.get(&var_key(name, id)).cloned())
    }

    fn resolve(&self, frame: &Frame<'a>, callee: &'a Expression, kind: Option<CallKind>) -> Option<(&'a ContractDef, &'a FunctionDef)> {
        let (call, resolved) = match &callee.kind {
            ExpressionKind::Identifier { name, resolved } => (name.clone(), *resolved),
            ExpressionKind::MemberAccess { object, member, resolved, .. } => {
                (format!("{}.{member}", expr_to_string(object)), *resolved)
            }
            _ => return None,
        };
        let Some(project) = self.project else {
            // hand-built models: a function of the same contract by name
            let owner = frame.owner;
            return owner.functions.iter().find(|f| f.name == call && f.body.is_some()).map(|f| (owner, f));
        };
        let declaration = resolved
            .and_then(|id| project.decl_table.function(id))
            .map(|t| (t.contract.clone(), t.function.clone()));
        let site = CallSite {
            call,
            kind: kind.unwrap_or(CallKind::Internal),
            declaration,
            storage_args: Vec::new(),
            modifier: None,
        };
        resolve_call(project, frame.context, frame.owner, &site)
    }

    /// Variables `stmts` assign anywhere, helpers included (top-level names).
    fn assigned(&self, frame: &Frame<'a>, stmts: &'a [Statement], out: &mut BTreeSet<String>) {
        for stmt in stmts {
            match &stmt.kind {
                StatementKind::ExpressionStmt { expression } => self.assigned_expr(frame, expression, out),
                StatementKind::VariableDeclaration { initial_value: Some(v), .. } => self.assigned_expr(frame, v, out),
                StatementKind::If { then_body, else_body, .. } => {
                    self.assigned(frame, then_body, out);
                    if let Some(e) = else_body {
                        self.assigned(frame, e, out);
                    }
                }
                StatementKind::Block { statements } | StatementKind::UncheckedBlock { statements } => {
                    self.assigned(frame, statements, out);
                }
                _ => {}
            }
        }
    }

    fn assigned_expr(&self, frame: &Frame<'a>, expr: &'a Expression, out: &mut BTreeSet<String>) {
        match &expr.kind {
            ExpressionKind::Assignment { target, .. } => {
                if let Some(name) = root_name(target) {
                    out.insert(name);
                }
            }
            ExpressionKind::UnaryOp { operator, operand } if operator.mutates_operand() => {
                if let Some(name) = root_name(operand) {
                    out.insert(name);
                }
            }
            ExpressionKind::FunctionCall { callee, arguments, call, .. } => {
                let Some(kind) = internal_kind(*call) else { return };
                if self.cap(frame.depth) {
                    return;
                }
                if let Some((inner, func)) = self.callee(frame, callee, arguments, kind) {
                    if let Some(body) = &func.body {
                        self.assigned(&inner, body, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn check_call(expr: &Expression) -> Option<(&'static str, &[Expression])> {
    let ExpressionKind::FunctionCall { callee, arguments, .. } = &expr.kind else { return None };
    let ExpressionKind::Identifier { name, .. } = &callee.kind else { return None };
    let check = match name.as_str() {
        "require" => "require",
        "assert" => "assert",
        "revert" => "revert",
        _ => return None,
    };
    Some((check, arguments))
}

/// Revert arguments longer than this are shown as `...`: they build a message, not the check.
const MAX_REVERT_ARGS: usize = 60;

fn revert_args(arguments: &[Expression]) -> String {
    let args = arguments.iter().map(expr_to_string).collect::<Vec<_>>().join(", ");
    if args.chars().count() > MAX_REVERT_ARGS { "...".to_string() } else { args }
}

fn revert_text(stmts: &[Statement]) -> Option<String> {
    match &stmts.last()?.kind {
        StatementKind::Revert { error_name, arguments } => {
            let args = revert_args(arguments);
            Some(match error_name.as_deref() {
                Some(name) if !name.is_empty() => format!("revert {name}({args})"),
                _ => format!("revert({args})"),
            })
        }
        StatementKind::ExpressionStmt { expression } => match check_call(expression)? {
            ("revert", args) => Some(format!("revert({})", revert_args(args))),
            _ => None,
        },
        StatementKind::Block { statements } | StatementKind::UncheckedBlock { statements } => revert_text(statements),
        _ => None,
    }
}

fn index_chain<'e>(expr: &'e Expression, out: &mut Vec<&'e Expression>) {
    match &expr.kind {
        ExpressionKind::IndexAccess { base, index } => {
            index_chain(base, out);
            if let Some(i) = index {
                out.push(i);
            }
        }
        ExpressionKind::MemberAccess { object, .. } => index_chain(object, out),
        _ => {}
    }
}

fn names_in(expr: &Expression) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    expr.walk(&mut |e| match &e.kind {
        ExpressionKind::Identifier { name, .. } | ExpressionKind::MemberAccess { member: name, .. } => {
            names.insert(name.clone());
        }
        _ => {}
    });
    names
}

fn root_name(expr: &Expression) -> Option<String> {
    match &expr.kind {
        ExpressionKind::Identifier { name, .. } => Some(name.clone()),
        ExpressionKind::IndexAccess { base, .. } => root_name(base),
        ExpressionKind::MemberAccess { object, .. } => root_name(object),
        _ => None,
    }
}

fn bool_literal(expr: &Expression) -> Option<bool> {
    match &expr.kind {
        ExpressionKind::Literal { literal_type: LiteralType::Bool, value } => Some(value == "true"),
        _ => None,
    }
}

// a sanity check, not a caller check
fn is_zero(expr: &Expression) -> bool {
    match &uncast(expr).kind {
        ExpressionKind::Literal { value, .. } => {
            let digits = value.strip_prefix("0x").unwrap_or(value);
            !digits.is_empty() && digits.chars().all(|c| c == '0')
        }
        _ => false,
    }
}

/// The kind of a call into contract code that runs in this frame; `None` inside means untyped.
fn internal_kind(call: CallType) -> Option<Option<CallKind>> {
    match call {
        CallType::Call(kind) if kind.is_internal() => Some(Some(kind)),
        CallType::Unknown => Some(None),
        _ => None,
    }
}

fn contains(stmts: &[Statement], kind: impl Fn(&StatementKind) -> bool) -> bool {
    let mut found = false;
    walk_all(stmts, &mut |s| found |= kind(&s.kind), &mut |_| {});
    found
}

/// A `_` counts: the rest of the modifier runs after the body.
fn may_exit(stmts: &[Statement]) -> bool {
    contains(stmts, |k| matches!(k, StatementKind::Return { .. } | StatementKind::Placeholder | StatementKind::Assembly { .. }))
}
