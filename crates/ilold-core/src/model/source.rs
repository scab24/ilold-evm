//! Where a value comes from: what a call targets or what a guard compares the caller with.
use serde::{Deserialize, Serialize};

use super::common::StateVar;
use super::decl_id::{DeclId, DeclKind};
use super::expression::{Expression, ExpressionKind};
use super::project::Project;
use crate::cfg::builder::expr_to_string;
use crate::util::is_type_cast;

/// `key` is the declaration's readable key, absent without solc ids.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ValueSource {
    StateVar { name: String, key: Option<String> },
    Immutable { name: String, key: Option<String> },
    Constant { name: String, key: Option<String> },
    Param { index: usize, name: String },
    Literal { text: String },
    MsgSender,
    TxOrigin,
    Expression { text: String },
}

/// The operand of a type conversion, typed or by name.
pub fn cast_operand(expr: &Expression) -> Option<&Expression> {
    match &expr.kind {
        ExpressionKind::TypeCast { expression, .. } => Some(expression),
        ExpressionKind::FunctionCall { callee, arguments, .. } => {
            let cast = match &callee.kind {
                ExpressionKind::TypeMeta { .. } => true,
                ExpressionKind::Identifier { name, .. } => is_type_cast(name),
                _ => false,
            };
            match arguments.as_slice() {
                [inner] if cast => Some(inner),
                _ => None,
            }
        }
        _ => None,
    }
}

pub fn uncast(mut expr: &Expression) -> &Expression {
    while let Some(inner) = cast_operand(expr) {
        expr = inner;
    }
    expr
}

/// `bound` names the parameters in scope; `vars` stands in for solc ids in hand-built models.
pub fn value_source(
    project: Option<&Project>,
    vars: &[StateVar],
    expr: &Expression,
    bound: impl Fn(&str, Option<DeclId>) -> Option<ValueSource>,
) -> ValueSource {
    let expr = uncast(expr);
    match &expr.kind {
        ExpressionKind::Identifier { name, resolved } => bound(name, *resolved)
            .or_else(|| state_source(project, vars, name, *resolved))
            .unwrap_or_else(|| ValueSource::Expression { text: name.clone() }),
        ExpressionKind::MemberAccess { object, member, .. }
            if member == "sender" && matches!(&object.kind, ExpressionKind::Identifier { name, .. } if name == "msg") =>
        {
            ValueSource::MsgSender
        }
        ExpressionKind::Literal { value, .. } => ValueSource::Literal { text: value.clone() },
        _ => ValueSource::Expression { text: expr_to_string(expr) },
    }
}

fn state_source(project: Option<&Project>, vars: &[StateVar], name: &str, resolved: Option<DeclId>) -> Option<ValueSource> {
    let (var, key) = match (project, resolved) {
        (Some(project), Some(id)) => {
            let target = project.decl_table.lookup(id).filter(|t| t.kind == DeclKind::StateVariable)?;
            let owner = &project.contracts[*project.contract_index.get(&target.contract)?];
            (owner.state_vars.iter().find(|v| v.name == target.name)?, Some(target.key.clone()))
        }
        (_, Some(id)) => (vars.iter().find(|v| v.id == Some(id))?, None),
        (_, None) => (vars.iter().find(|v| v.name == name)?, None),
    };
    let name = var.name.clone();
    Some(if var.is_constant {
        ValueSource::Constant { name, key }
    } else if var.is_immutable {
        ValueSource::Immutable { name, key }
    } else {
        ValueSource::StateVar { name, key }
    })
}
