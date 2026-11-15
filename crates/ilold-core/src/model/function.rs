use serde::{Deserialize, Serialize};

use super::common::{Param, SourceSpan};
use super::decl_id::DeclId;
use super::modifier::ModifierRef;
use super::expression::{AssignOperator, Expression, ExpressionKind};
use super::statement::{walk_all, Statement, StatementKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub id: Option<DeclId>,
    pub kind: FunctionKind,
    pub visibility: Visibility,
    pub mutability: Mutability,
    pub modifiers: Vec<ModifierRef>,
    #[serde(default)]
    pub base_constructor_calls: Vec<ModifierRef>,
    pub params: Vec<Param>,
    pub returns: Vec<Param>,
    pub body: Option<Vec<Statement>>,
    pub is_virtual: bool,
    pub is_override: bool,
    pub span: SourceSpan,
}

impl FunctionDef {
    pub fn match_signature(&self) -> &str {
        if self.signature.is_empty() { &self.name } else { &self.signature }
    }

    pub fn is_entry_point(&self) -> bool {
        self.kind != FunctionKind::Constructor && matches!(self.visibility, Visibility::Public | Visibility::External)
    }

    pub fn returned_values<'a>(&'a self) -> Vec<&'a Expression> {
        let names_return = |name: &str, resolved: Option<DeclId>| {
            self.returns.iter().any(|r| match (resolved, r.id) {
                (Some(a), Some(b)) => a == b,
                _ => !r.name.is_empty() && r.name == name,
            })
        };
        let mut out = Vec::new();
        let on_stmt = &mut |s: &'a Statement| match &s.kind {
            StatementKind::Return { value: Some(value) } => out.push(value),
            StatementKind::ExpressionStmt { expression } => {
                if let ExpressionKind::Assignment { target, operator: AssignOperator::Assign, value } = &expression.kind {
                    if matches!(&target.kind, ExpressionKind::Identifier { name, resolved } if names_return(name, *resolved)) {
                        out.push(value);
                    }
                }
            }
            _ => {}
        };
        walk_all(self.body.as_deref().unwrap_or_default(), on_stmt, &mut |_| {});
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FunctionKind {
    Function,
    Constructor,
    Fallback,
    Receive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    External,
    Internal,
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mutability {
    Pure,
    View,
    Payable,
    NonPayable,
}

impl Mutability {
    pub fn is_view_or_pure(self) -> bool {
        matches!(self, Mutability::View | Mutability::Pure)
    }
}
