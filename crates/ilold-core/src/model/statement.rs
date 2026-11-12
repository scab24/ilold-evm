use serde::{Deserialize, Serialize};

use super::common::{Param, SourceSpan};
use super::expression::Expression;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatchClause {
    pub name: Option<String>,
    pub params: Vec<Param>,
    pub body: Vec<Statement>,
    /// From `catch` or `returns` through the closing brace; the block alone for a success clause without returns.
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: SourceSpan,
}

impl Statement {
    /// Itself, then its expressions and nested statements in source order.
    pub fn walk<'a>(&'a self, on_stmt: &mut impl FnMut(&'a Statement), on_expr: &mut impl FnMut(&'a Expression)) {
        on_stmt(self);
        match &self.kind {
            StatementKind::If { condition, then_body, else_body } => {
                condition.walk(on_expr);
                walk_all(then_body, on_stmt, on_expr);
                walk_all(else_body.as_deref().unwrap_or_default(), on_stmt, on_expr);
            }
            StatementKind::For { init, condition, increment, body: stmts } => {
                if let Some(init) = init {
                    init.walk(on_stmt, on_expr);
                }
                for e in condition.iter().chain(increment) {
                    e.walk(on_expr);
                }
                walk_all(stmts, on_stmt, on_expr);
            }
            StatementKind::While { condition, body: stmts } => {
                condition.walk(on_expr);
                walk_all(stmts, on_stmt, on_expr);
            }
            StatementKind::DoWhile { body: stmts, condition } => {
                walk_all(stmts, on_stmt, on_expr);
                condition.walk(on_expr);
            }
            StatementKind::Block { statements } | StatementKind::UncheckedBlock { statements } => {
                walk_all(statements, on_stmt, on_expr);
            }
            StatementKind::Return { value } => value.iter().for_each(|e| e.walk(on_expr)),
            StatementKind::Emit { arguments, .. } | StatementKind::Revert { arguments, .. } => {
                arguments.iter().for_each(|e| e.walk(on_expr));
            }
            StatementKind::ExpressionStmt { expression } => expression.walk(on_expr),
            StatementKind::VariableDeclaration { initial_value, .. } => initial_value.iter().for_each(|e| e.walk(on_expr)),
            StatementKind::TryCatch { expression, clauses } => {
                expression.walk(on_expr);
                for clause in clauses {
                    walk_all(&clause.body, on_stmt, on_expr);
                }
            }
            StatementKind::Assembly { .. } | StatementKind::Break | StatementKind::Continue | StatementKind::Placeholder => {}
        }
    }
}

pub fn walk_all<'a>(stmts: &'a [Statement], on_stmt: &mut impl FnMut(&'a Statement), on_expr: &mut impl FnMut(&'a Expression)) {
    for s in stmts {
        s.walk(on_stmt, on_expr);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StatementKind {
    If {
        condition: Expression,
        then_body: Vec<Statement>,
        else_body: Option<Vec<Statement>>,
    },
    For {
        init: Option<Box<Statement>>,
        condition: Option<Expression>,
        increment: Option<Expression>,
        body: Vec<Statement>,
    },
    While {
        condition: Expression,
        body: Vec<Statement>,
    },
    DoWhile {
        body: Vec<Statement>,
        condition: Expression,
    },
    Block {
        statements: Vec<Statement>,
    },
    UncheckedBlock {
        statements: Vec<Statement>,
    },
    Return {
        value: Option<Expression>,
    },
    Emit {
        event_name: String,
        arguments: Vec<Expression>,
        #[serde(default)]
        event_id: Option<super::decl_id::DeclId>,
    },
    Revert {
        error_name: Option<String>,
        arguments: Vec<Expression>,
    },
    ExpressionStmt {
        expression: Expression,
    },
    VariableDeclaration {
        name: String,
        type_name: String,
        initial_value: Option<Expression>,
        #[serde(default)]
        is_storage_ref: bool,
        /// Identifiers referring to the variable carry the same id.
        #[serde(default)]
        id: Option<super::decl_id::DeclId>,
    },
    TryCatch {
        expression: Expression,
        clauses: Vec<CatchClause>,
    },
    Assembly {
        span: SourceSpan,
    },
    Break,
    Continue,
    Placeholder,
}
