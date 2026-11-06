//! The keys of each storage access: for every index level of the raw path, its text and where its value comes from.
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::cfg::builder::expr_to_string;
use crate::cfg::types::{CfgGraph, CfgStatement, StorageRef};
use crate::model::common::{Param, SourceSpan};
use crate::model::contract::ContractDef;
use crate::model::decl_id::DeclId;
use crate::model::expression::{Expression, ExpressionKind};
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::model::source::{uncast, value_source, ValueSource};
use crate::model::statement::{walk_all, Statement, StatementKind};
use crate::sequence::effects::reachable;

use super::{param_source, state_var_ref};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyKind {
    MsgSender,
    Parameter,
    StateVar,
    Local,
    Literal,
    This,
    /// Anything else, and any index the model cannot tell apart.
    Expression,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AccessKey {
    /// As the source writes it.
    pub text: String,
    pub kind: KeyKind,
    /// `<contract key>::<function or modifier key>` declaring a parameter; `<contract key>::<name>` of a state variable.
    #[serde(rename = "ref")]
    pub reference: Option<String>,
}

/// One index level per key, outermost first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeySite {
    pub keys: Vec<AccessKey>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeysReason {
    /// The access happens in a callee, whose summary keeps no keys; its direct record has them.
    Transitive,
    /// The sites of one record index with different keys.
    Several,
}

/// The keys of one reads or writes record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordKeys {
    /// The key set every site shares.
    pub keys: Option<Vec<AccessKey>>,
    pub keys_reason: Option<KeysReason>,
    /// Distinct, sorted by span, then key text.
    pub key_sites: Vec<KeySite>,
}

impl RecordKeys {
    pub fn direct(mut key_sites: Vec<KeySite>) -> Self {
        key_sites.sort_by(|a, b| (a.span, &a.keys).cmp(&(b.span, &b.keys)));
        key_sites.dedup();
        let shared = key_sites.first().map(|s| &s.keys).filter(|keys| key_sites.iter().all(|s| &s.keys == *keys)).cloned();
        let keys_reason = (shared.is_none() && !key_sites.is_empty()).then_some(KeysReason::Several);
        Self { keys: shared, keys_reason, key_sites }
    }

    pub fn transitive() -> Self {
        Self { keys: None, keys_reason: Some(KeysReason::Transitive), key_sites: Vec::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessKind {
    Read,
    Write,
}

/// A read or write in a function's own code or its modifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageAccess {
    pub kind: AccessKind,
    /// The raw path, keys included.
    pub path: String,
    /// Index of the `storage` parameter the path is rooted at.
    pub param: Option<usize>,
    pub modifier: Option<String>,
    /// The statement of a write; the block of a read.
    pub span: SourceSpan,
    pub block: usize,
    pub keys: Vec<AccessKey>,
}

/// Every storage access of `func`'s reachable code, as `context` runs it.
pub fn storage_accesses(
    project: &Project,
    context: &ContractDef,
    owner: &ContractDef,
    func: &FunctionDef,
    cfg: &CfgGraph,
) -> Vec<StorageAccess> {
    let mut indexes: HashMap<Option<String>, Indexes> = HashMap::new();
    let mut out = Vec::new();
    let mut push = |kind: AccessKind, place: &StorageRef, modifier: Option<&str>, span: SourceSpan, block: usize| {
        let codes = indexes.entry(modifier.map(str::to_string)).or_insert_with(|| match modifier {
            None => Indexes::of(project, context, Code::function(owner, func)),
            Some(name) => modifier_indexes(project, context, name),
        });
        let keys = index_texts(&place.path).into_iter().map(|text| codes.key(text)).collect();
        let modifier = modifier.map(str::to_string);
        out.push(StorageAccess { kind, path: place.path.clone(), param: place.param, modifier, span, block, keys });
    };
    for n in reachable(cfg) {
        let block = &cfg[n];
        let block_span = block.span.unwrap_or(func.span);
        for read in &block.reads {
            push(AccessKind::Read, &read.place, read.from_modifier.as_deref(), block_span, block.id);
        }
        for stmt in &block.statements {
            let (span, modifier) = stmt.origin();
            let place = match stmt {
                CfgStatement::Assignment { storage: Some(place), .. } => place.clone(),
                CfgStatement::StateWrite { variable, param, .. } => StorageRef { path: variable.clone(), param: *param },
                _ => continue,
            };
            push(AccessKind::Write, &place, modifier, span.unwrap_or(func.span), block.id);
        }
    }
    out
}

fn code_ref(c: &ContractDef, member: &str) -> String {
    format!("{}::{member}", c.key())
}

fn modifier_indexes(project: &Project, context: &ContractDef, name: &str) -> Indexes {
    let Some(def) = project.resolve_modifier(context, name) else { return Indexes::default() };
    let owner = project.linearization(context).into_iter().find(|c| c.modifiers.iter().any(|m| std::ptr::eq(m, def))).unwrap_or(context);
    Indexes::of(project, context, Code { reference: code_ref(owner, name), params: &def.params, returns: &[], body: &def.body })
}

/// A function's or modifier's code, with the declarations its indexes may name.
struct Code<'a> {
    /// `<contract key>::<function or modifier key>`.
    reference: String,
    params: &'a [Param],
    returns: &'a [Param],
    body: &'a [Statement],
}

impl<'a> Code<'a> {
    fn function(owner: &ContractDef, f: &'a FunctionDef) -> Self {
        Self { reference: code_ref(owner, &f.key), params: &f.params, returns: &f.returns, body: f.body.as_deref().unwrap_or_default() }
    }
}

/// The texts inside the outermost brackets of a path, outermost level first.
fn index_texts(path: &str) -> Vec<&str> {
    let (mut texts, mut depth, mut start) = (Vec::new(), 0usize, 0);
    for (i, ch) in path.char_indices() {
        match ch {
            '[' => {
                if depth == 0 {
                    start = i + 1;
                }
                depth += 1;
            }
            ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    texts.push(&path[start..i]);
                }
            }
            _ => {}
        }
    }
    texts
}

/// The key each index text of some code stands for; `None` when the same text means different things.
#[derive(Default)]
struct Indexes(HashMap<String, Option<AccessKey>>);

impl Indexes {
    /// Callees returning a storage pointer count too: the builder follows them once, so their indexes reach the path.
    fn of(project: &Project, context: &ContractDef, code: Code) -> Self {
        let mut indexes = Self::default();
        indexes.add(project, context, &code);
        for (owner, callee) in storage_getters(project, code.body) {
            indexes.add(project, context, &Code::function(owner, callee));
        }
        indexes
    }

    fn add(&mut self, project: &Project, context: &ContractDef, code: &Code) {
        let mut locals = Locals::default();
        locals.extend(code.returns);
        let mut index_exprs = Vec::new();
        walk_all(
            code.body,
            &mut |stmt| match &stmt.kind {
                StatementKind::VariableDeclaration { name, id, .. } => locals.insert(name, *id),
                StatementKind::TryCatch { clauses, .. } => clauses.iter().for_each(|c| locals.extend(&c.params)),
                _ => {}
            },
            &mut |expr| {
                if let ExpressionKind::IndexAccess { index: Some(index), .. } = &expr.kind {
                    index_exprs.push(index.as_ref());
                }
            },
        );
        for index in index_exprs {
            let key = classify(project, context, code, &locals, index);
            match self.0.entry(key.text.clone()) {
                Entry::Vacant(slot) => {
                    slot.insert(Some(key));
                }
                Entry::Occupied(mut slot) => {
                    if slot.get().as_ref() != Some(&key) {
                        slot.insert(None);
                    }
                }
            }
        }
    }

    fn key(&self, text: &str) -> AccessKey {
        self.0.get(text).cloned().flatten().unwrap_or_else(|| AccessKey { text: text.to_string(), kind: KeyKind::Expression, reference: None })
    }
}

/// Local variables by declaration id, or by name when the model has no ids.
#[derive(Default)]
struct Locals {
    ids: HashSet<DeclId>,
    names: HashSet<String>,
}

impl Locals {
    fn insert(&mut self, name: &str, id: Option<DeclId>) {
        match id {
            Some(id) => self.ids.insert(id),
            None => self.names.insert(name.to_string()),
        };
    }

    fn extend(&mut self, params: &[Param]) {
        params.iter().for_each(|p| self.insert(&p.name, p.id));
    }

    fn contains(&self, name: &str, resolved: Option<DeclId>) -> bool {
        match resolved {
            Some(id) => self.ids.contains(&id),
            None => self.names.contains(name),
        }
    }
}

fn classify(project: &Project, context: &ContractDef, code: &Code, locals: &Locals, index: &Expression) -> AccessKey {
    let text = expr_to_string(index);
    let key = |kind: KeyKind, reference: Option<String>| AccessKey { text: text.clone(), kind, reference };
    if let ExpressionKind::Identifier { name, resolved } = &uncast(index).kind {
        if name == "this" {
            return key(KeyKind::This, None);
        }
        if param_source(code.params, name, *resolved).is_none() && locals.contains(name, *resolved) {
            return key(KeyKind::Local, None);
        }
    }
    let source = value_source(Some(project), &context.state_vars, index, |name, resolved| param_source(code.params, name, resolved));
    let state_var = state_var_ref(project, context, &source);
    match source {
        ValueSource::MsgSender => key(KeyKind::MsgSender, None),
        ValueSource::Param { .. } => key(KeyKind::Parameter, Some(code.reference.clone())),
        ValueSource::Literal { .. } => key(KeyKind::Literal, None),
        ValueSource::StateVar { .. } | ValueSource::Immutable { .. } | ValueSource::Constant { .. } if state_var.is_some() => {
            key(KeyKind::StateVar, state_var)
        }
        _ => key(KeyKind::Expression, None),
    }
}

/// The functions `body` calls that return one storage pointer.
fn storage_getters<'p>(project: &'p Project, body: &[Statement]) -> Vec<(&'p ContractDef, &'p FunctionDef)> {
    let mut found: Vec<(&ContractDef, &FunctionDef)> = Vec::new();
    walk_all(body, &mut |_| {}, &mut |expr| {
        let ExpressionKind::FunctionCall { callee, .. } = &expr.kind else { return };
        let (ExpressionKind::Identifier { resolved: Some(id), .. } | ExpressionKind::MemberAccess { resolved: Some(id), .. }) = &callee.kind
        else {
            return;
        };
        let Some(target) = project.decl_table.function(*id) else { return };
        let Some(owner) = project.contract_index.get(&target.contract).map(|&i| &project.contracts[i]) else { return };
        let Some(def) = owner.functions.iter().find(|f| f.key == target.function) else { return };
        if matches!(def.returns.as_slice(), [ret] if ret.is_storage) && !found.iter().any(|(_, f)| std::ptr::eq(*f, def)) {
            found.push((owner, def));
        }
    });
    found
}
