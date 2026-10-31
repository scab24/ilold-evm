//! Guards of a function read from its code. A guard is a revert check on the unconditional
//! entry path -- in an inlined modifier before its `_`, in the body, or in an internal helper
//! or library function either of them calls -- classified by what its condition depends on:
//! the caller (access control) or a pause, reentrancy or initialization flag (state guards).
//! Modifier names are used only when a modifier has no code to read, and such facts are
//! marked `heuristic`.
use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::cfg::builder::{call_arguments, expr_to_string};
use crate::model::common::SourceSpan;
use crate::model::contract::ContractDef;
use crate::model::decl_id::{DeclId, DeclKind};
use crate::model::expression::{BinaryOperator, CallKind, CallType, Expression, ExpressionKind, LiteralType, UnaryOperator};
use crate::model::function::{FunctionDef, Mutability};
use crate::model::modifier::{ModifierDef, ModifierRef};
use crate::model::project::Project;
use crate::model::statement::{Statement, StatementKind};
use crate::sequence::effects::{resolve_call, CallSite};
use crate::util::is_type_cast;

/// Helpers deeper than this are not followed.
const MAX_DEPTH: usize = 12;

/// Modifier names that suggest access control; used only for modifiers without code.
const ACCESS_MODIFIERS: &[&str] = &[
    "onlyowner", "onlyadmin", "onlyrole", "onlygovernance",
    "onlyminter", "onlypauser", "onlyoperator", "onlyguardian",
    "onlyauthorized", "onlymanager", "onlycontroller",
    "onlybridge", "onlyrelayer", "onlyvault",
    "auth", "restricted",
];

/// Modifier names of state guards; used only for modifiers without code.
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
    /// The caller compared with an address: `msg.sender == owner`, `owner() != _msgSender()`.
    CallerEquals,
    /// The caller passed to a boolean check or used as a key: `hasRole(r, sender)`,
    /// `acl.isPoolAdmin(msg.sender)`, `authorized[msg.sender]`.
    RoleCheck,
    /// A check on `tx.origin`.
    TxOrigin,
    /// A modifier without code whose name suggests access control (always `heuristic`).
    AccessByName,
    Pause,
    Reentrancy,
    Initializer,
}

impl GuardKind {
    /// Restricts who may call; the state guards do not.
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

/// One guard: the check as written (`require(c)` or `if (c) revert E()`), the chain that
/// reaches it from the function (`modifier m`, then `Contract::function` per helper; empty
/// for the body) and where the check is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardFact {
    pub kind: GuardKind,
    pub condition: String,
    pub via: Vec<String>,
    pub span: Option<SourceSpan>,
    /// Taken from a modifier name, not from code.
    #[serde(default)]
    pub heuristic: bool,
}

/// Guards of `func` (declared in `owner`) when `context` is the most derived contract:
/// modifiers and virtual helpers resolve to the overrides `context` sees. Without a project
/// only `owner`'s own modifiers and functions are visible.
pub fn function_guards(project: Option<&Project>, context: &ContractDef, owner: &ContractDef, func: &FunctionDef) -> Vec<GuardFact> {
    let mut walker = Walker { project, facts: Vec::new(), pending: Vec::new(), active: Vec::new() };
    let frame = Frame { context, owner, via: Vec::new(), callers: HashMap::new(), depth: 0 };
    walker.function(&frame, func);
    walker.facts
}

/// Who a value is: `msg.sender` (directly, through `_msgSender()` or a variable holding it)
/// or `tx.origin`.
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

/// The code being read: its contracts, the chain that reached it and the parameters and
/// locals holding the caller.
#[derive(Clone)]
struct Frame<'a> {
    context: &'a ContractDef,
    owner: &'a ContractDef,
    via: Vec<String>,
    callers: HashMap<VarKey, Caller>,
    depth: usize,
}

/// A check whose condition is neither about the caller nor about a pause or initialization
/// flag; inside a modifier it may still be a reentrancy lock.
struct Pending {
    condition: String,
    via: Vec<String>,
    span: SourceSpan,
    names: BTreeSet<String>,
}

struct Walker<'a> {
    project: Option<&'a Project>,
    facts: Vec<GuardFact>,
    pending: Vec<Pending>,
    // (contract key, function key) being read, to stop recursion
    active: Vec<(String, String)>,
}

impl<'a> Walker<'a> {
    fn push(&mut self, fact: GuardFact) {
        let same = |f: &GuardFact| f.kind == fact.kind && f.condition == fact.condition && f.via == fact.via;
        if !self.facts.iter().any(same) {
            self.facts.push(fact);
        }
    }

    // Modifiers in order, then the body.
    fn function(&mut self, frame: &Frame<'a>, func: &'a FunctionDef) {
        let key = (frame.owner.key().to_string(), func.key.clone());
        if frame.depth > MAX_DEPTH || self.active.contains(&key) {
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
            self.by_name(&mref.name, via);
            return;
        };
        let callers = def
            .params
            .iter()
            .zip(&mref.arguments)
            .filter_map(|(p, arg)| Some((var_key(&p.name, p.id), self.caller_of(frame, arg)?)))
            .collect();
        let inner = Frame { context: frame.context, owner: declared_in, via: via.clone(), callers, depth: frame.depth };
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
                self.push(GuardFact { kind: GuardKind::Reentrancy, condition: p.condition, via: p.via, span: Some(p.span), heuristic: false });
            }
        }
        if self.facts.len() == facts && contains_assembly(before) {
            self.by_name(&mref.name, via);
        }
    }

    // A modifier whose code cannot be read: only its name is left.
    fn by_name(&mut self, name: &str, via: Vec<String>) {
        let lower = name.to_lowercase();
        let kind = if ACCESS_MODIFIERS.iter().any(|p| lower.contains(p)) {
            GuardKind::AccessByName
        } else if let Some((_, kind)) = STATE_GUARD_MODIFIERS.iter().find(|(p, _)| lower.contains(p)) {
            *kind
        } else {
            return;
        };
        let condition = format!("{name} (no code to read; assumed from the name)");
        self.push(GuardFact { kind, condition, via, span: None, heuristic: true });
    }

    /// The modifier `name` as `frame.context` sees it, with the contract declaring it.
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

    // Straight-line statements; false once control leaves (return, revert).
    fn statements(&mut self, frame: &Frame<'a>, stmts: &'a [Statement]) -> bool {
        let mut frame = frame.clone();
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
                    // a branch that may leave the function: later checks are not on every path
                    if may_exit(then_body) || else_body.as_deref().is_some_and(may_exit) {
                        return false;
                    }
                }
                StatementKind::For { body, .. } | StatementKind::While { body, .. } | StatementKind::DoWhile { body, .. } => {
                    if may_exit(body) {
                        return false;
                    }
                }
                StatementKind::TryCatch { clauses, .. } => {
                    if clauses.iter().any(|c| may_exit(&c.body)) {
                        return false;
                    }
                }
                // Yul can `return` or `stop`
                StatementKind::Assembly { .. } => return false,
                StatementKind::Block { statements } | StatementKind::UncheckedBlock { statements } => {
                    if !self.statements(&frame, statements) {
                        return false;
                    }
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
        true
    }

    /// A revert check whose condition `cond` must evaluate to `holds` for execution to go on.
    fn check(&mut self, frame: &Frame<'a>, cond: &'a Expression, holds: bool, condition: String, span: SourceSpan) {
        let via = frame.via.clone();
        if let Some(kind) = self.access_kind(frame, cond, holds, 0) {
            self.push(GuardFact { kind, condition, via, span: Some(span), heuristic: false });
            return;
        }
        let mut names = BTreeSet::new();
        names_in(cond, &mut names);
        let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
        let kind = if lower.iter().any(|n| n.contains("paused")) {
            Some(GuardKind::Pause)
        } else if lower.iter().any(|n| n.contains("initializ")) {
            Some(GuardKind::Initializer)
        } else {
            None
        };
        match kind {
            Some(kind) => self.push(GuardFact { kind, condition, via, span: Some(span), heuristic: false }),
            None => self.pending.push(Pending { condition, via, span, names }),
        }
    }

    /// The access guard expressed by requiring `cond == holds`: only callers that satisfy a
    /// caller-dependent test get through. `require(msg.sender != owner)` lets everyone but the
    /// owner through and is not one; a disjunction is one only when every disjunct is.
    fn access_kind(&self, frame: &Frame<'a>, cond: &'a Expression, holds: bool, depth: usize) -> Option<GuardKind> {
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
                // `x == true`, `x != false`, ...
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
                    (Some(c), None) if !is_zero(right) => Some(c.kind(true)),
                    (None, Some(c)) if !is_zero(left) => Some(c.kind(true)),
                    // `tx.origin == msg.sender`: only externally owned accounts
                    (Some(a), Some(b)) if a != b => Some(GuardKind::TxOrigin),
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
                // a call that changes state (`transferFrom(msg.sender, ...)`) is not a check
                if !self.is_view_call(frame, callee, kind) {
                    return None;
                }
                let args = match kind {
                    Some(kind) => call_arguments(callee, arguments, kind),
                    None => arguments.iter().collect(),
                };
                if let Some(c) = args.iter().find_map(|a| self.caller_of(frame, a)) {
                    return Some(c.kind(false));
                }
                // a boolean helper: `require(isOwner())`, every return of which is a guard
                if depth >= MAX_DEPTH {
                    return None;
                }
                let (inner, func) = self.callee(frame, callee, arguments, kind)?;
                let values = returned(func);
                let kinds: Vec<Option<GuardKind>> = values.iter().map(|r| self.access_kind(&inner, r, true, depth + 1)).collect();
                kinds.iter().all(Option::is_some).then(|| kinds.first().copied().flatten()).flatten()
            }
            ExpressionKind::IndexAccess { .. } => {
                let mut indexes = Vec::new();
                index_chain(cond, &mut indexes);
                indexes.into_iter().find_map(|i| self.caller_of(frame, i)).map(|c| c.kind(false))
            }
            ExpressionKind::MemberAccess { object, .. } => self.access_kind(frame, object, holds, depth),
            _ => None,
        }
    }

    /// Whether the function a call reaches is declared `view` or `pure` (a public state
    /// variable's getter counts). Unknown callees are not.
    fn is_view_call(&self, frame: &Frame<'a>, callee: &'a Expression, kind: Option<CallKind>) -> bool {
        let is_view = |f: &FunctionDef| matches!(f.mutability, Mutability::View | Mutability::Pure);
        let resolved = match &callee.kind {
            ExpressionKind::Identifier { resolved, .. } | ExpressionKind::MemberAccess { resolved, .. } => *resolved,
            _ => None,
        };
        if let (Some(project), Some(id)) = (self.project, resolved) {
            if let Some(target) = project.decl_table.lookup(id) {
                return match target.kind {
                    DeclKind::StateVariable => true,
                    DeclKind::Function => project
                        .contract_index
                        .get(&target.contract)
                        .and_then(|&i| project.contracts[i].functions.iter().find(|f| f.key == target.function))
                        .is_some_and(is_view),
                    _ => false,
                };
            }
        }
        self.resolve(frame, callee, kind).is_some_and(|(_, f)| is_view(f))
    }

    /// Whether `expr` is the caller's identity.
    fn caller_of(&self, frame: &Frame<'a>, expr: &'a Expression) -> Option<Caller> {
        self.caller_at(frame, expr, 0)
    }

    fn caller_at(&self, frame: &Frame<'a>, expr: &'a Expression, depth: usize) -> Option<Caller> {
        match &expr.kind {
            ExpressionKind::MemberAccess { object, member, .. } => match (&object.kind, member.as_str()) {
                (ExpressionKind::Identifier { name, .. }, "sender") if name == "msg" => Some(Caller::Sender),
                (ExpressionKind::Identifier { name, .. }, "origin") if name == "tx" => Some(Caller::Origin),
                _ => None,
            },
            ExpressionKind::Identifier { name, resolved } => frame.callers.get(&var_key(name, *resolved)).copied(),
            ExpressionKind::TypeCast { expression, .. } => self.caller_at(frame, expression, depth),
            ExpressionKind::Tuple { elements } if elements.len() == 1 => {
                self.caller_at(frame, elements[0].as_ref()?, depth)
            }
            ExpressionKind::FunctionCall { callee, arguments, call, .. } => {
                // `address(x)`, `payable(x)`
                let is_cast = match &callee.kind {
                    ExpressionKind::TypeMeta { .. } => true,
                    ExpressionKind::Identifier { name, .. } => is_type_cast(name),
                    _ => false,
                };
                if is_cast {
                    return match arguments.as_slice() {
                        [inner] => self.caller_at(frame, inner, depth),
                        _ => None,
                    };
                }
                let kind = match call {
                    CallType::Call(kind) if kind.is_internal() => Some(*kind),
                    CallType::Unknown => None,
                    _ => return None,
                };
                if depth >= MAX_DEPTH {
                    return None;
                }
                // `_msgSender()`: a helper every return of which is the caller
                match self.callee(frame, callee, arguments, kind) {
                    Some((inner, func)) => {
                        let values = returned(func);
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

    /// Internal and library calls made by `expr`; each callee is read as a helper.
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

    /// The function a call reaches and the frame it runs in: parameters bound to the caller
    /// where the arguments are the caller.
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
            .zip(args)
            .filter_map(|(p, a)| Some((var_key(&p.name, p.id), self.caller_of(frame, a)?)))
            .collect();
        let shares_storage = match self.project {
            Some(p) => p.linearization(frame.context).iter().any(|c| c.key() == owner.key()),
            None => true,
        };
        let mut via = frame.via.clone();
        via.push(format!("{}::{}", owner.key(), func.key));
        let context = if shares_storage { frame.context } else { owner };
        Some((Frame { context, owner, via, callers, depth: frame.depth + 1 }, func))
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
            ExpressionKind::FunctionCall { callee, arguments, call, .. } if frame.depth <= MAX_DEPTH => {
                let kind = match call {
                    CallType::Call(k) if k.is_internal() => Some(*k),
                    CallType::Unknown => None,
                    _ => return,
                };
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

// `require(c, ...)`, `assert(c)` and `revert(...)` as expression statements.
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

// How a block that always reverts ends: `revert E(args)` or `revert(args)`.
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

/// Values a function returns: `return e` anywhere in its body, and named return variables
/// assigned at top level.
fn returned(func: &FunctionDef) -> Vec<&Expression> {
    fn walk<'e>(stmts: &'e [Statement], func: &FunctionDef, out: &mut Vec<&'e Expression>) {
        for stmt in stmts {
            match &stmt.kind {
                StatementKind::Return { value: Some(v) } => out.push(v),
                StatementKind::ExpressionStmt { expression } => {
                    if let ExpressionKind::Assignment { target, value, .. } = &expression.kind {
                        if let ExpressionKind::Identifier { name, resolved } = &target.kind {
                            let named = func.returns.iter().any(|r| match (resolved, r.id) {
                                (Some(a), Some(b)) => *a == b,
                                _ => !r.name.is_empty() && r.name == *name,
                            });
                            if named {
                                out.push(value);
                            }
                        }
                    }
                }
                StatementKind::If { then_body, else_body, .. } => {
                    walk(then_body, func, out);
                    if let Some(e) = else_body {
                        walk(e, func, out);
                    }
                }
                StatementKind::Block { statements } | StatementKind::UncheckedBlock { statements } => walk(statements, func, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    if let Some(body) = &func.body {
        walk(body, func, &mut out);
    }
    out
}

// Indexes of `a[i][j].k`, outermost last.
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

// Identifiers, members and called names a condition mentions.
fn names_in(expr: &Expression, out: &mut BTreeSet<String>) {
    match &expr.kind {
        ExpressionKind::Identifier { name, .. } => {
            out.insert(name.clone());
        }
        ExpressionKind::MemberAccess { object, member, .. } => {
            out.insert(member.clone());
            names_in(object, out);
        }
        ExpressionKind::FunctionCall { callee, arguments, .. } => {
            names_in(callee, out);
            for a in arguments {
                names_in(a, out);
            }
        }
        ExpressionKind::BinaryOp { left, right, .. } => {
            names_in(left, out);
            names_in(right, out);
        }
        ExpressionKind::UnaryOp { operand, .. } => names_in(operand, out),
        ExpressionKind::IndexAccess { base, index } => {
            names_in(base, out);
            if let Some(i) = index {
                names_in(i, out);
            }
        }
        ExpressionKind::Tuple { elements } => {
            for e in elements.iter().flatten() {
                names_in(e, out);
            }
        }
        ExpressionKind::TypeCast { expression, .. } => names_in(expression, out),
        ExpressionKind::Ternary { condition, true_expr, false_expr } => {
            names_in(condition, out);
            names_in(true_expr, out);
            names_in(false_expr, out);
        }
        _ => {}
    }
}

// The variable an assignment target is rooted at: `x`, `x[k]`, `x.f`.
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

// `0`, `0x00`, `address(0)`: a sanity check, not a caller check.
fn is_zero(expr: &Expression) -> bool {
    match &expr.kind {
        ExpressionKind::Literal { value, .. } => {
            let digits = value.strip_prefix("0x").unwrap_or(value);
            !digits.is_empty() && digits.chars().all(|c| c == '0')
        }
        ExpressionKind::TypeCast { expression, .. } => is_zero(expression),
        ExpressionKind::FunctionCall { callee, arguments, .. } => {
            let cast = match &callee.kind {
                ExpressionKind::TypeMeta { .. } => true,
                ExpressionKind::Identifier { name, .. } => is_type_cast(name),
                _ => false,
            };
            cast && matches!(arguments.as_slice(), [inner] if is_zero(inner))
        }
        _ => false,
    }
}

/// Whether `stmts` can leave the function without reverting: a `return`, a `_` (the rest
/// of a modifier then runs after the body) or assembly, anywhere inside.
fn may_exit(stmts: &[Statement]) -> bool {
    stmts.iter().any(|s| match &s.kind {
        StatementKind::Return { .. } | StatementKind::Placeholder | StatementKind::Assembly { .. } => true,
        StatementKind::If { then_body, else_body, .. } => may_exit(then_body) || else_body.as_deref().is_some_and(may_exit),
        StatementKind::For { body, .. } | StatementKind::While { body, .. } | StatementKind::DoWhile { body, .. } => may_exit(body),
        StatementKind::Block { statements } | StatementKind::UncheckedBlock { statements } => may_exit(statements),
        StatementKind::TryCatch { clauses, .. } => clauses.iter().any(|c| may_exit(&c.body)),
        _ => false,
    })
}

fn contains_assembly(stmts: &[Statement]) -> bool {
    stmts.iter().any(|s| match &s.kind {
        StatementKind::Assembly { .. } => true,
        StatementKind::Block { statements } | StatementKind::UncheckedBlock { statements } => contains_assembly(statements),
        StatementKind::If { then_body, else_body, .. } => {
            contains_assembly(then_body) || else_body.as_deref().is_some_and(contains_assembly)
        }
        _ => false,
    })
}
