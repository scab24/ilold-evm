use crate::model::contract::ContractDef;
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::model::statement::{Statement, StatementKind};

use super::types::{StatementOrigin, StatementPath};

pub struct FlatStatement<'a> {
    pub path: StatementPath,
    pub statement: &'a Statement,
    pub depth: usize,
    pub parent: Option<usize>,
    pub origin: StatementOrigin,
}

pub fn flatten_function<'a>(
    project: &'a Project,
    contract: &'a ContractDef,
    function: &'a FunctionDef,
) -> Vec<FlatStatement<'a>> {
    let mut out: Vec<FlatStatement<'a>> = Vec::new();

    let body: &'a [Statement] = function.body.as_deref().unwrap_or(&[]);

    struct ModifierParts<'a> {
        name: &'a str,
        before: &'a [Statement],
        after: &'a [Statement],
    }
    let mut modifier_parts: Vec<ModifierParts<'a>> = Vec::new();
    for m_ref in &function.modifiers {
        let Some(m_def) = project.resolve_modifier(contract, &m_ref.name) else {
            continue;
        };
        let split = m_def
            .body
            .iter()
            .position(|s| matches!(s.kind, StatementKind::Placeholder));
        let (before, after) = match split {
            Some(idx) => (&m_def.body[..idx], &m_def.body[idx + 1..]),
            None => (m_def.body.as_slice(), &[][..]),
        };
        modifier_parts.push(ModifierParts {
            name: m_def.name.as_str(),
            before,
            after,
        });
    }

    for parts in &modifier_parts {
        let origin = StatementOrigin::Modifier(parts.name.to_string());
        for stmt in parts.before {
            walk(stmt, 0, None, &origin, &mut out);
        }
    }

    for stmt in body {
        walk(stmt, 0, None, &StatementOrigin::FunctionBody, &mut out);
    }

    for parts in modifier_parts.iter().rev() {
        let origin = StatementOrigin::Modifier(parts.name.to_string());
        for stmt in parts.after {
            walk(stmt, 0, None, &origin, &mut out);
        }
    }

    out
}

fn walk<'a>(
    stmt: &'a Statement,
    depth: usize,
    parent: Option<usize>,
    origin: &StatementOrigin,
    out: &mut Vec<FlatStatement<'a>>,
) {
    let idx = out.len();
    out.push(FlatStatement {
        path: StatementPath(vec![idx]),
        statement: stmt,
        depth,
        parent,
        origin: origin.clone(),
    });

    let child_parent = Some(idx);
    let child_depth = depth + 1;

    match &stmt.kind {
        StatementKind::If { then_body, else_body, .. } => {
            for s in then_body {
                walk(s, child_depth, child_parent, origin, out);
            }
            if let Some(eb) = else_body {
                for s in eb {
                    walk(s, child_depth, child_parent, origin, out);
                }
            }
        }
        StatementKind::For { init, body, .. } => {
            if let Some(init_stmt) = init {
                walk(init_stmt, child_depth, child_parent, origin, out);
            }
            for s in body {
                walk(s, child_depth, child_parent, origin, out);
            }
        }
        StatementKind::While { body, .. } | StatementKind::DoWhile { body, .. } => {
            for s in body {
                walk(s, child_depth, child_parent, origin, out);
            }
        }
        StatementKind::Block { statements } | StatementKind::UncheckedBlock { statements } => {
            for s in statements {
                walk(s, child_depth, child_parent, origin, out);
            }
        }
        StatementKind::TryCatch { clauses, .. } => {
            for clause in clauses {
                for s in &clause.body {
                    walk(s, child_depth, child_parent, origin, out);
                }
            }
        }
        StatementKind::ExpressionStmt { .. }
        | StatementKind::VariableDeclaration { .. }
        | StatementKind::Return { .. }
        | StatementKind::Emit { .. }
        | StatementKind::Revert { .. }
        | StatementKind::Assembly { .. }
        | StatementKind::Placeholder
        | StatementKind::Continue
        | StatementKind::Break => {}
    }
}
