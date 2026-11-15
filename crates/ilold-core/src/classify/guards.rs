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

pub(crate) const MAX_DEPTH: usize = 12;

const ACCESS_MODIFIERS: &[&str] = &[
    "onlyowner", "onlyadmin", "onlyrole", "onlygovernance",
    "onlyminter", "onlypauser", "onlyoperator", "onlyguardian",
    "onlyauthorized", "onlymanager", "onlycontroller",
    "onlybridge", "onlyrelayer", "onlyvault",
    "auth", "restricted",
];

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
    RoleCheck,
    TxOrigin,
    AccessByName,
    Pause,
    Reentrancy,
    Initializer,
    Condition,
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
            GuardKind::Condition => "condition",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardFact {
    pub kind: GuardKind,
    pub condition: String,
    pub via: Vec<String>,
    pub span: Option<SourceSpan>,
    #[serde(default)]
    pub heuristic: bool,
    #[serde(default)]
    pub scope: GuardScope,
    #[serde(default)]
    pub negated: bool,
    #[serde(default)]
    pub requires_caller: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compared_against: Option<ValueSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<ValueSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_implied_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<ValueSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<GuardTerm>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conjuncts: Vec<GuardTerm>,
    #[serde(default)]
    pub ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardTerm {
    pub kind: GuardKind,
    pub condition: String,
    #[serde(default)]
    pub negated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compared_against: Option<ValueSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<ValueSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_implied_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<ValueSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<GuardTerm>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conjuncts: Vec<GuardTerm>,
}

impl GuardTerm {
    fn leaf(kind: GuardKind, condition: &Expression, holds: bool) -> Self {
        let (compared_against, checked_by, role, role_implied_by, account) = (None, None, None, None, None);
        let (alternatives, conjuncts) = (Vec::new(), Vec::new());
        let condition = expr_to_string(condition);
        Self { kind, condition, negated: !holds, compared_against, checked_by, role, role_implied_by, account, alternatives, conjuncts }
    }

    fn compound(all: bool, condition: &Expression, parts: [GuardTerm; 2]) -> Self {
        let mut flat = Vec::new();
        for part in parts {
            let same = if all { &part.conjuncts } else { &part.alternatives };
            if same.is_empty() { flat.push(part) } else { flat.extend(same.iter().cloned()) }
        }
        let rank = |t: &&GuardTerm| if all && t.restricts() { 0 } else if all && t.denies() { 1 } else if t.has_access() { 2 } else { 3 };
        let decides = flat.iter().min_by_key(rank).expect("a compound has parts");
        let (kind, holds) = (decides.kind, !decides.negated);
        let (alternatives, conjuncts) = if all { (Vec::new(), flat) } else { (flat, Vec::new()) };
        GuardTerm { alternatives, conjuncts, ..GuardTerm::leaf(kind, condition, holds) }
    }

    fn has_access(&self) -> bool {
        self.kind.is_access() || self.alternatives.iter().chain(&self.conjuncts).any(GuardTerm::has_access)
    }

    fn restricts(&self) -> bool {
        self.decide(|t| t.kind.is_access() && !t.negated)
    }

    fn denies(&self) -> bool {
        self.decide(|t| t.kind.is_access() && t.negated)
    }

    fn decide(&self, leaf: fn(&GuardTerm) -> bool) -> bool {
        if !self.conjuncts.is_empty() {
            self.conjuncts.iter().any(|t| t.decide(leaf))
        } else if !self.alternatives.is_empty() {
            self.alternatives.iter().all(|t| t.decide(leaf))
        } else {
            leaf(self)
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum GuardScope {
    #[default]
    Unconditional,
    Conditional,
}

impl GuardFact {
    pub fn plain(kind: GuardKind, condition: String, via: Vec<String>, span: Option<SourceSpan>, heuristic: bool, scope: GuardScope) -> Self {
        let (compared_against, checked_by, role, role_implied_by, account) = (None, None, None, None, None);
        let (alternatives, conjuncts, negated, ambiguous) = (Vec::new(), Vec::new(), false, false);
        Self {
            kind, condition, via, span, heuristic, scope, negated, requires_caller: kind.is_access(), compared_against, checked_by, role,
            role_implied_by, account, alternatives, conjuncts, ambiguous,
        }
    }

    pub fn restricts(&self) -> bool {
        self.requires_caller && self.scope == GuardScope::Unconditional
    }

    pub fn term(&self) -> GuardTerm {
        GuardTerm {
            kind: self.kind,
            condition: self.condition.clone(),
            negated: self.negated,
            compared_against: self.compared_against.clone(),
            checked_by: self.checked_by.clone(),
            role: self.role.clone(),
            role_implied_by: self.role_implied_by.clone(),
            account: self.account.clone(),
            alternatives: self.alternatives.clone(),
            conjuncts: self.conjuncts.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GuardScan {
    pub facts: Vec<GuardFact>,
    pub incomplete: bool,
}

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

    fn account(self) -> ValueSource {
        match self {
            Caller::Sender => ValueSource::MsgSender,
            Caller::Origin => ValueSource::TxOrigin,
        }
    }

    fn checked(self, cond: &Expression, holds: bool, checked_by: Option<String>, role: Option<ValueSource>) -> GuardTerm {
        let role_implied_by = if role.is_none() { checked_by.clone() } else { None };
        GuardTerm { checked_by, role, role_implied_by, account: Some(self.account()), ..GuardTerm::leaf(self.kind(false), cond, holds) }
    }

    fn compared(self, cond: &Expression, equal: bool, against: ValueSource) -> GuardTerm {
        GuardTerm { compared_against: Some(against), account: Some(self.account()), ..GuardTerm::leaf(self.kind(true), cond, equal) }
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

#[derive(Clone)]
struct Frame<'a> {
    context: &'a ContractDef,
    owner: &'a ContractDef,
    via: Vec<String>,
    callers: HashMap<VarKey, Caller>,
    sources: HashMap<VarKey, ValueSource>,
    depth: usize,
    conditional: bool,
}

impl Frame<'_> {
    fn scope(&self) -> GuardScope {
        if self.conditional { GuardScope::Conditional } else { GuardScope::Unconditional }
    }
}

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
    active: Vec<(String, String)>,
    capped: Cell<bool>,
}

impl<'a> Walker<'a> {
    fn push(&mut self, fact: GuardFact) {
        let same = |f: &&mut GuardFact| f.kind == fact.kind && f.condition == fact.condition && f.via == fact.via;
        match self.facts.iter_mut().find(same) {
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

        let mut set_before = BTreeSet::new();
        let mut set_after = BTreeSet::new();
        self.assigned(&inner, before, &mut set_before);
        self.assigned(&inner, after, &mut set_after);
        let candidates: Vec<Pending> = self.pending.drain(pending..).collect();
        for p in candidates {
            if p.names.iter().any(|n| set_before.contains(n) && set_after.contains(n)) {
                let kind = GuardKind::Reentrancy;
                let (via, span, scope) = (p.via, Some(p.span), p.scope);
                self.push(GuardFact::plain(kind, p.condition, via, span, false, scope));
            }
        }
        // code facts override the name because a false restriction is worse than a gap
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
        self.push(GuardFact::plain(kind, condition, via, None, true, scope));
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

    // later checks hold on some paths only
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

    fn check(&mut self, frame: &Frame<'a>, cond: &'a Expression, holds: bool, condition: String, span: SourceSpan) {
        let (via, scope) = (frame.via.clone(), frame.scope());
        let mut opaque = false;
        let term = self.term(frame, cond, holds, 0, &mut opaque);
        if term.has_access() {
            let requires_caller = term.restricts();
            let (alternatives, conjuncts) = if opaque { Default::default() } else { (term.alternatives, term.conjuncts) };
            self.push(GuardFact {
                negated: term.negated,
                requires_caller,
                compared_against: term.compared_against,
                checked_by: term.checked_by,
                role: term.role,
                role_implied_by: term.role_implied_by,
                account: term.account,
                alternatives,
                conjuncts,
                ambiguous: opaque,
                ..GuardFact::plain(term.kind, condition, via, Some(span), false, scope)
            });
            return;
        }
        let names = names_in(cond);
        match state_kind(&names) {
            Some(kind) => self.push(GuardFact::plain(kind, condition, via, Some(span), false, scope)),
            None => self.pending.push(Pending { condition, via, span, names, scope }),
        }
    }

    fn term(&self, frame: &Frame<'a>, cond: &'a Expression, holds: bool, depth: usize, opaque: &mut bool) -> GuardTerm {
        let compared = |c: Caller, other: &'a Expression, equal: bool| c.compared(cond, equal, self.source(frame, other, 0));
        match &cond.kind {
            ExpressionKind::UnaryOp { operator: UnaryOperator::Not, operand } => self.term(frame, operand, !holds, depth, opaque),
            ExpressionKind::Tuple { elements } => match elements.as_slice() {
                [Some(inner)] => self.term(frame, inner, holds, depth, opaque),
                _ => self.other(frame, cond, holds, opaque),
            },
            ExpressionKind::BinaryOp { left, operator: op @ (BinaryOperator::And | BinaryOperator::Or), right } => {
                let parts = [self.term(frame, left, holds, depth, opaque), self.term(frame, right, holds, depth, opaque)];
                // `a && b` holding, or `a || b` failing, needs both sides
                GuardTerm::compound(matches!(op, BinaryOperator::And) == holds, cond, parts)
            }
            ExpressionKind::BinaryOp { left, operator: op @ (BinaryOperator::Eq | BinaryOperator::Neq), right } => {
                let equal = matches!(op, BinaryOperator::Eq) == holds;
                if let Some(b) = bool_literal(right) {
                    return self.term(frame, left, equal == b, depth, opaque);
                }
                if let Some(b) = bool_literal(left) {
                    return self.term(frame, right, equal == b, depth, opaque);
                }
                match (self.caller_of(frame, left), self.caller_of(frame, right)) {
                    (Some(c), None) if !is_zero(right) => compared(c, right, equal),
                    (None, Some(c)) if !is_zero(left) => compared(c, left, equal),
                    // `tx.origin == msg.sender` admits only externally owned accounts
                    (Some(a), Some(b)) if a != b => Caller::Origin.compared(cond, equal, ValueSource::MsgSender),
                    // a zero-address comparison is a sanity check
                    (Some(_), None) | (None, Some(_)) => GuardTerm::leaf(GuardKind::Condition, cond, holds),
                    _ => self.other(frame, cond, holds, opaque),
                }
            }
            ExpressionKind::FunctionCall { callee, arguments, call, .. } => {
                let kind = match call {
                    CallType::Builtin => return self.other(frame, cond, holds, opaque),
                    CallType::Call(kind) => Some(*kind),
                    CallType::Unknown => None,
                };
                if !self.is_view_call(frame, callee, kind) {
                    return GuardTerm::leaf(GuardKind::Condition, cond, holds);
                }
                let args = match kind {
                    Some(kind) => call_arguments(callee, arguments, kind),
                    None => arguments.iter().collect(),
                };
                if let Some(c) = args.iter().find_map(|a| self.caller_of(frame, a)) {
                    let role = args.iter().copied().find(|a| self.caller_of(frame, a).is_none());
                    return c.checked(cond, holds, Some(expr_to_string(callee)), role.map(|r| self.source(frame, r, 0)));
                }
                let helper = if self.cap(depth + 1) { None } else { self.callee(frame, callee, arguments, kind) };
                let Some((inner, func)) = helper else { return self.other(frame, cond, holds, opaque) };
                let mut inner_opaque = false;
                let found: Vec<GuardTerm> =
                    func.returned_values().iter().map(|r| self.term(&inner, r, holds, depth + 1, &mut inner_opaque)).collect();
                match found.first() {
                    Some(first) if found.iter().all(GuardTerm::has_access) => {
                        *opaque |= inner_opaque;
                        first.clone()
                    }
                    _ => self.other(frame, cond, holds, opaque),
                }
            }
            ExpressionKind::IndexAccess { .. } => {
                let mut indexes = Vec::new();
                index_chain(cond, &mut indexes);
                let Some(c) = indexes.iter().find_map(|i| self.caller_of(frame, i)) else { return self.other(frame, cond, holds, opaque) };
                let role = indexes.iter().copied().find(|i| self.caller_of(frame, i).is_none());
                c.checked(cond, holds, root_name(cond), role.map(|r| self.source(frame, r, 0)))
            }
            ExpressionKind::MemberAccess { object, .. } => {
                let inner = self.term(frame, object, holds, depth, opaque);
                if inner.has_access() { inner } else { self.other(frame, cond, holds, opaque) }
            }
            _ => self.other(frame, cond, holds, opaque),
        }
    }

    fn other(&self, frame: &Frame<'a>, cond: &'a Expression, holds: bool, opaque: &mut bool) -> GuardTerm {
        let mut caller = false;
        cond.walk(&mut |e| caller |= self.caller_of(frame, e).is_some());
        *opaque |= caller;
        GuardTerm::leaf(state_kind(&names_in(cond)).unwrap_or(GuardKind::Condition), cond, holds)
    }

    fn is_view_call(&self, frame: &Frame<'a>, callee: &'a Expression, kind: Option<CallKind>) -> bool {
        if let Some(view) = self.project.zip(callee.resolved()).and_then(|(project, id)| project.is_view_declaration(id)) {
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

pub(crate) fn check_call(expr: &Expression) -> Option<(&'static str, &[Expression])> {
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

fn state_kind(names: &BTreeSet<String>) -> Option<GuardKind> {
    let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    if lower.iter().any(|n| n.contains("paused")) {
        Some(GuardKind::Pause)
    } else if lower.iter().any(|n| n.contains("initializ")) {
        Some(GuardKind::Initializer)
    } else {
        None
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

fn is_zero(expr: &Expression) -> bool {
    match &uncast(expr).kind {
        ExpressionKind::Literal { value, .. } => {
            let digits = value.strip_prefix("0x").unwrap_or(value);
            !digits.is_empty() && digits.chars().all(|c| c == '0')
        }
        _ => false,
    }
}

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

fn may_exit(stmts: &[Statement]) -> bool {
    contains(stmts, |k| matches!(k, StatementKind::Return { .. } | StatementKind::Placeholder | StatementKind::Assembly { .. }))
}
