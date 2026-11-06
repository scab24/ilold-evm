//! Each try statement a function runs: the call it guards and what every catch clause does when that call fails.
use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::stable_graph::NodeIndex;
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};

use crate::cfg::builder::modifier_code;
use crate::cfg::types::{BlockKind, BranchEdge, CfgGraph, CfgStatement};
use crate::model::common::SourceSpan;
use crate::model::contract::ContractDef;
use crate::model::decl_id::DeclTable;
use crate::model::expression::CallKind;
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::model::statement::{walk_all, CatchClause, Statement, StatementKind};
use crate::sequence::effects::{reachable, CallSite};

use super::keys::{storage_accesses, AccessKind, StorageAccess};
use super::Tag;

/// The failures a clause catches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CatchKind {
    /// `catch Error(string memory reason)`: a revert with a reason string.
    Error,
    /// `catch Panic(uint256 code)`.
    Panic,
    /// `catch (bytes memory data)`: any failure, with its revert data.
    #[serde(rename = "bytes")]
    Bytes,
    /// `catch { ... }`: any failure, without its data.
    #[serde(rename = "all")]
    All,
}

/// Why a try fact is AMBIGUOUS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TryReason {
    /// No try statement of the code has the call the CFG guards, so its clauses cannot be told apart.
    StatementNotFound,
    /// A clause names an error the language does not catch by name.
    UnknownCatchKind,
}

/// One effect inside a clause, as the source writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectSite {
    /// The raw path of a write, keys included; the call text; the event name.
    pub text: String,
    pub span: SourceSpan,
}

/// What a clause's own code does; effects of the functions it calls are theirs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatchEffects {
    pub writes: Vec<EffectSite>,
    pub calls: Vec<EffectSite>,
    pub emits: Vec<EffectSite>,
    /// Some path through the clause reverts.
    pub reverts: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatchClauseFact {
    /// None when the clause names an error the language does not catch by name.
    pub kind: Option<CatchKind>,
    pub span: SourceSpan,
    /// The body has no statement: the failure is swallowed.
    pub empty: bool,
    pub effects: CatchEffects,
    /// The CFG block the clause starts at.
    pub block: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardedCall {
    /// `<target>.<function>` as the source writes it; `<Contract>.constructor` for `new`.
    pub text: String,
    pub kind: CallKind,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TryCatchFact {
    /// `<function>#<n>`, `n` counting the function's try statements in source order.
    pub id: String,
    /// `<contract key>::<function key>` declaring the code.
    pub function: String,
    /// Key of the contract that runs the function.
    pub in_context: String,
    /// The modifier whose code holds the try statement.
    pub modifier: Option<String>,
    pub span: SourceSpan,
    pub guarded_call: Option<GuardedCall>,
    pub success_span: Option<SourceSpan>,
    pub catch_clauses: Vec<CatchClauseFact>,
    pub tag: Tag,
    pub reason: Option<TryReason>,
    /// The CFG block ending in the guarded call, and the one its success branch starts at.
    pub block: usize,
    pub success_block: usize,
}

/// Every try statement of `func`'s reachable code, as `context` runs it, in source order.
pub fn try_catches(project: &Project, context: &ContractDef, owner: &ContractDef, func: &FunctionDef, cfg: &CfgGraph) -> Vec<TryCatchFact> {
    let statements = try_statements(project, context, func);
    let accesses = storage_accesses(project, context, owner, func, cfg);
    let function = format!("{}::{}", owner.key(), func.key);
    let mut facts = Vec::new();
    for n in reachable(cfg) {
        // the builder adds the success branch first, then one catch branch per clause, in block order
        let mut branches: Vec<(bool, NodeIndex)> = cfg
            .edges(n)
            .filter_map(|e| match e.weight() {
                BranchEdge::ExternalCallSuccess => Some((true, e.target())),
                BranchEdge::CatchClause { .. } => Some((false, e.target())),
                _ => None,
            })
            .collect();
        branches.sort_by_key(|(success, target)| (!success, cfg[*target].id));
        let Some(&(true, success)) = branches.first() else { continue };
        let catches = &branches[1..];
        let block = &cfg[n];
        let last = block.statements.last();
        let guarded_call = last.and_then(|stmt| guarded(stmt, &project.decl_table));
        let statement = guarded_call
            .as_ref()
            .and_then(|call| statements.get(&call.span))
            .filter(|(_, clauses)| clauses.len() == branches.len());
        let mut fact = TryCatchFact {
            id: String::new(),
            function: function.clone(),
            in_context: context.key().to_string(),
            modifier: last.and_then(|stmt| stmt.origin().1).map(str::to_string),
            span: guarded_call.as_ref().map(|c| c.span).or(block.span).unwrap_or(func.span),
            guarded_call,
            success_span: None,
            catch_clauses: Vec::new(),
            tag: Tag::Ambiguous,
            reason: Some(TryReason::StatementNotFound),
            block: block.id,
            success_block: cfg[success].id,
        };
        if let Some((span, clauses)) = statement {
            fact.span = *span;
            fact.success_span = Some(clauses[0].span);
            fact.catch_clauses = clauses[1..]
                .iter()
                .zip(catches)
                .map(|(clause, &(_, start))| CatchClauseFact {
                    kind: catch_kind(clause),
                    span: clause.span,
                    empty: clause.body.is_empty(),
                    effects: effects(cfg, n, start, clause.span, &accesses, &project.decl_table),
                    block: cfg[start].id,
                })
                .collect();
            let known = fact.catch_clauses.iter().all(|c| c.kind.is_some());
            fact.tag = if known { Tag::Extracted } else { Tag::Ambiguous };
            fact.reason = (!known).then_some(TryReason::UnknownCatchKind);
        }
        facts.push(fact);
    }
    facts.sort_by_key(|f| (f.span, f.block));
    for (n, fact) in facts.iter_mut().enumerate() {
        fact.id = format!("{function}#{n}");
    }
    facts
}

/// The try statements of the function's code and modifiers, by the span of the call each guards.
fn try_statements<'a>(project: &'a Project, context: &'a ContractDef, func: &'a FunctionDef) -> HashMap<SourceSpan, (SourceSpan, &'a [CatchClause])> {
    let mut found = HashMap::new();
    let modifiers = func.modifiers.iter().filter_map(|m| modifier_code(Some(project), context, &m.name)).map(|m| m.body.as_slice());
    for body in std::iter::once(func.body.as_deref().unwrap_or_default()).chain(modifiers) {
        walk_all(
            body,
            &mut |stmt: &'a Statement| {
                if let StatementKind::TryCatch { expression, clauses } = &stmt.kind {
                    found.insert(expression.span, (stmt.span, clauses.as_slice()));
                }
            },
            &mut |_| {},
        );
    }
    found
}

/// `<target>.<function>` of a call, as the effect summaries name it.
fn call_text(stmt: &CfgStatement, decls: &DeclTable) -> Option<String> {
    CallSite::of(stmt, decls).map(|c| c.call).or_else(|| match stmt {
        CfgStatement::ExternalCall { target, function, .. } => Some(format!("{target}.{function}")),
        _ => None,
    })
}

fn guarded(stmt: &CfgStatement, decls: &DeclTable) -> Option<GuardedCall> {
    let CfgStatement::ExternalCall { kind, span: Some(span), .. } = stmt else { return None };
    Some(GuardedCall { text: call_text(stmt, decls)?, kind: *kind, span: *span })
}

fn catch_kind(clause: &CatchClause) -> Option<CatchKind> {
    match (clause.name.as_deref(), clause.params.len()) {
        (Some("Error"), _) => Some(CatchKind::Error),
        (Some("Panic"), _) => Some(CatchKind::Panic),
        (None, 0) => Some(CatchKind::All),
        (None, _) => Some(CatchKind::Bytes),
        _ => None,
    }
}

/// What the code inside `span` does on the blocks reachable from `start` without re-running the try in `try_block`.
fn effects(cfg: &CfgGraph, try_block: NodeIndex, start: NodeIndex, span: SourceSpan, accesses: &[StorageAccess], decls: &DeclTable) -> CatchEffects {
    let mut seen = HashSet::from([try_block, start]);
    let mut queue = VecDeque::from([start]);
    let mut blocks = HashSet::new();
    let mut out = CatchEffects::default();
    // the walk leaves the clause through returns, loops and modifier code; only code inside its span is its own
    let inside = |s: Option<SourceSpan>| s.is_some_and(|s| span.contains(&s));
    while let Some(n) = queue.pop_front() {
        let block = &cfg[n];
        blocks.insert(block.id);
        out.reverts |= block.kind == BlockKind::Revert && inside(block.span);
        for (stmt, at) in block.statements.iter().filter_map(|s| Some((s, s.origin().0?))).filter(|(_, at)| span.contains(at)) {
            let site = |text: String| EffectSite { text, span: at };
            match stmt {
                CfgStatement::EmitEvent { event, .. } => out.emits.push(site(event.clone())),
                _ => out.calls.extend(call_text(stmt, decls).map(site)),
            }
        }
        queue.extend(cfg.neighbors(n).filter(|next| seen.insert(*next)));
    }
    let writes = accesses.iter().filter(|a| a.kind == AccessKind::Write && blocks.contains(&a.block) && inside(Some(a.span)));
    out.writes = writes.map(|a| EffectSite { text: a.path.clone(), span: a.span }).collect();
    for sites in [&mut out.writes, &mut out.calls, &mut out.emits] {
        sites.sort_by(|a, b| (a.span, &a.text).cmp(&(b.span, &b.text)));
        sites.dedup();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::common::Param;

    fn clause(name: Option<&str>, params: usize) -> CatchClause {
        let param = Param { name: String::new(), type_name: "bytes".into(), is_storage: false, id: None };
        let span = SourceSpan { file_index: 0, start_line: 1, start_col: 1, end_line: 1, end_col: 9 };
        CatchClause { name: name.map(str::to_string), params: vec![param; params], body: Vec::new(), span }
    }

    #[test]
    fn the_clause_kind_comes_from_its_name_and_parameters() {
        assert_eq!(catch_kind(&clause(Some("Error"), 1)), Some(CatchKind::Error));
        assert_eq!(catch_kind(&clause(Some("Panic"), 1)), Some(CatchKind::Panic));
        assert_eq!(catch_kind(&clause(None, 1)), Some(CatchKind::Bytes));
        assert_eq!(catch_kind(&clause(None, 0)), Some(CatchKind::All));
        assert_eq!(catch_kind(&clause(Some("Custom"), 1)), None, "only Error and Panic are caught by name");
    }
}
