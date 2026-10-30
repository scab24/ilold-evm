use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::contract::ContractDef;
use crate::model::expression::{BinaryOperator, Expression, ExpressionKind};
use crate::model::function::{FunctionDef, FunctionKind, Visibility};
use crate::model::statement::{Statement, StatementKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessLevel {
    Public,
    Restricted { role: String },
    Internal,
    Special { kind: String },
}

impl fmt::Display for AccessLevel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AccessLevel::Public => write!(f, "Public"),
            AccessLevel::Restricted { role } => write!(f, "Restricted({})", role),
            AccessLevel::Internal => write!(f, "Internal"),
            AccessLevel::Special { kind } => write!(f, "Special({})", kind),
        }
    }
}

impl AccessLevel {
    pub fn short_label(&self) -> &str {
        match self {
            AccessLevel::Public => "P",
            AccessLevel::Restricted { .. } => "R",
            AccessLevel::Internal => "I",
            AccessLevel::Special { .. } => "S",
        }
    }

    pub fn is_unrestricted(&self) -> bool {
        matches!(self, AccessLevel::Public)
    }
}

const ACCESS_MODIFIERS: &[&str] = &[
    "onlyowner", "onlyadmin", "onlyrole", "onlygovernance",
    "onlyminter", "onlypauser", "onlyoperator", "onlyguardian",
    "onlyauthorized", "onlymanager", "onlycontroller",
    "onlybridge", "onlyrelayer", "onlyvault",
    "auth", "restricted",
];

#[allow(dead_code)]
const STATE_GUARD_MODIFIERS: &[&str] = &[
    "whennotpaused", "whenpaused",
    "nonreentrant", "noreentrant",
    "initializer", "reinitializer",
];

pub fn classify_function(func: &FunctionDef, _contract: &ContractDef) -> AccessLevel {
    match func.kind {
        FunctionKind::Constructor => {
            return AccessLevel::Special { kind: "constructor".into() };
        }
        FunctionKind::Fallback => {
            return AccessLevel::Special { kind: "fallback".into() };
        }
        FunctionKind::Receive => {
            return AccessLevel::Special { kind: "receive".into() };
        }
        FunctionKind::Function => {}
    }

    if matches!(func.visibility, Visibility::Internal | Visibility::Private) {
        return AccessLevel::Internal;
    }

    for modifier in &func.modifiers {
        let lower = modifier.name.to_lowercase();
        for pattern in ACCESS_MODIFIERS {
            if lower.contains(pattern) {
                return AccessLevel::Restricted {
                    role: modifier.name.clone(),
                };
            }
        }
    }

    if let Some(body) = &func.body {
        if let Some(role) = find_sender_access_check(body) {
            return AccessLevel::Restricted { role };
        }
    }

    AccessLevel::Public
}

pub fn classify_all(contract: &ContractDef) -> Vec<(String, AccessLevel)> {
    contract
        .functions
        .iter()
        .map(|f| (f.name.clone(), classify_function(f, contract)))
        .collect()
}

// matches msg.sender compared with ==/!=
fn find_sender_access_check(stmts: &[Statement]) -> Option<String> {
    for stmt in stmts {
        if let Some(role) = check_statement_for_access(stmt) {
            return Some(role);
        }
    }
    None
}

fn check_statement_for_access(stmt: &Statement) -> Option<String> {
    match &stmt.kind {
        StatementKind::If { condition, then_body, else_body } => {
            if expr_is_sender_comparison(condition) {
                return Some("msg.sender check".into());
            }
            if let Some(role) = find_sender_access_check(then_body) {
                return Some(role);
            }
            if let Some(else_stmts) = else_body {
                if let Some(role) = find_sender_access_check(else_stmts) {
                    return Some(role);
                }
            }
        }
        StatementKind::ExpressionStmt { expression } => {
            if is_require_with_sender_comparison(expression) {
                return Some("msg.sender check".into());
            }
        }
        StatementKind::Block { statements } | StatementKind::UncheckedBlock { statements } => {
            if let Some(role) = find_sender_access_check(statements) {
                return Some(role);
            }
        }
        _ => {}
    }
    None
}

fn is_require_with_sender_comparison(expr: &Expression) -> bool {
    if let ExpressionKind::FunctionCall { callee, arguments, .. } = &expr.kind {
        let is_require = matches!(&callee.kind,
            ExpressionKind::Identifier { name, .. } if name == "require" || name == "assert"
        );
        if is_require {
            return arguments
                .first()
                .map_or(false, |arg| expr_is_sender_comparison(arg));
        }
    }
    false
}

fn expr_is_sender_comparison(expr: &Expression) -> bool {
    match &expr.kind {
        ExpressionKind::BinaryOp { left, operator, right } => {
            match operator {
                BinaryOperator::Eq | BinaryOperator::Neq => {
                    expr_is_msg_sender(left) || expr_is_msg_sender(right)
                }
                BinaryOperator::And | BinaryOperator::Or => {
                    expr_is_sender_comparison(left) || expr_is_sender_comparison(right)
                }
                _ => false,
            }
        }
        ExpressionKind::UnaryOp { operand, .. } => expr_is_sender_comparison(operand),
        _ => false,
    }
}

fn expr_is_msg_sender(expr: &Expression) -> bool {
    if let ExpressionKind::MemberAccess { object, member, .. } = &expr.kind {
        if member == "sender" {
            if let ExpressionKind::Identifier { name, .. } = &object.kind {
                return name == "msg";
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::common::SourceSpan;
    use crate::model::function::Mutability;
    use crate::model::modifier::ModifierRef;

    fn span() -> SourceSpan {
        SourceSpan { file_index: 0, start_line: 0, start_col: 0, end_line: 0, end_col: 0 }
    }

    fn make_func(name: &str, vis: Visibility, kind: FunctionKind, modifiers: Vec<&str>, body: Option<Vec<Statement>>) -> FunctionDef {
        FunctionDef {
            name: name.into(),
            key: name.into(),
            signature: format!("{name}()"),
            selector: None,
            id: None,
            kind,
            visibility: vis,
            mutability: Mutability::NonPayable,
            modifiers: modifiers
                .into_iter()
                .map(|m| ModifierRef { name: m.into(), arguments: vec![] })
                .collect(),
            base_constructor_calls: vec![],
            params: vec![],
            returns: vec![],
            body,
            is_virtual: false,
            is_override: false,
            span: span(),
        }
    }

    fn contract() -> ContractDef {
        ContractDef {
            name: "Test".into(),
            kind: crate::model::contract::ContractKind::Contract,
            functions: vec![], modifiers: vec![], state_vars: vec![],
            structs: vec![], enums: vec![], events: vec![], errors: vec![],
            inherits: vec![], id: None, linearized_bases: vec![], span: span(),
        }
    }

    fn msg_sender_expr() -> Expression {
        Expression {
            kind: ExpressionKind::MemberAccess {
                object: Box::new(Expression {
                    kind: ExpressionKind::Identifier { name: "msg".into(), resolved: None },
                    span: span(),
                }),
                member: "sender".into(),
                resolved: None,
                object_type: None,
            },
            span: span(),
        }
    }

    fn owner_expr() -> Expression {
        Expression {
            kind: ExpressionKind::Identifier { name: "owner".into(), resolved: None },
            span: span(),
        }
    }

    fn require_sender_eq_owner() -> Statement {
        Statement {
            kind: StatementKind::ExpressionStmt {
                expression: Expression {
                    kind: ExpressionKind::FunctionCall {
                        callee: Box::new(Expression {
                            kind: ExpressionKind::Identifier { name: "require".into(), resolved: None },
                            span: span(),
                        }),
                        arguments: vec![
                            Expression {
                                kind: ExpressionKind::BinaryOp {
                                    left: Box::new(msg_sender_expr()),
                                    operator: BinaryOperator::Eq,
                                    right: Box::new(owner_expr()),
                                },
                                span: span(),
                            },
                        ],
                        options: vec![],
                        call: Default::default(),
                    },
                    span: span(),
                },
            },
            span: span(),
        }
    }

    #[test]
    fn internal_function() {
        let f = make_func("_update", Visibility::Internal, FunctionKind::Function, vec![], Some(vec![]));
        assert_eq!(classify_function(&f, &contract()), AccessLevel::Internal);
    }

    #[test]
    fn private_function() {
        let f = make_func("_helper", Visibility::Private, FunctionKind::Function, vec![], Some(vec![]));
        assert_eq!(classify_function(&f, &contract()), AccessLevel::Internal);
    }

    #[test]
    fn public_no_modifier() {
        let f = make_func("deposit", Visibility::External, FunctionKind::Function, vec![], Some(vec![]));
        assert_eq!(classify_function(&f, &contract()), AccessLevel::Public);
    }

    #[test]
    fn restricted_only_owner() {
        let f = make_func("setRate", Visibility::External, FunctionKind::Function, vec!["onlyOwner"], Some(vec![]));
        assert_eq!(
            classify_function(&f, &contract()),
            AccessLevel::Restricted { role: "onlyOwner".into() }
        );
    }

    #[test]
    fn when_not_paused_is_still_public() {
        let f = make_func("deposit", Visibility::External, FunctionKind::Function, vec!["whenNotPaused"], Some(vec![]));
        assert_eq!(classify_function(&f, &contract()), AccessLevel::Public);
    }

    #[test]
    fn nonreentrant_is_still_public() {
        let f = make_func("withdraw", Visibility::External, FunctionKind::Function, vec!["nonReentrant"], Some(vec![]));
        assert_eq!(classify_function(&f, &contract()), AccessLevel::Public);
    }

    #[test]
    fn constructor_is_special() {
        let f = make_func("", Visibility::Public, FunctionKind::Constructor, vec![], Some(vec![]));
        assert_eq!(
            classify_function(&f, &contract()),
            AccessLevel::Special { kind: "constructor".into() }
        );
    }

    #[test]
    fn receive_is_special() {
        let f = make_func("", Visibility::External, FunctionKind::Receive, vec![], Some(vec![]));
        assert_eq!(
            classify_function(&f, &contract()),
            AccessLevel::Special { kind: "receive".into() }
        );
    }

    #[test]
    fn require_msg_sender_eq_owner() {
        let body = vec![require_sender_eq_owner()];
        let f = make_func("admin_func", Visibility::External, FunctionKind::Function, vec![], Some(body));
        assert_eq!(
            classify_function(&f, &contract()),
            AccessLevel::Restricted { role: "msg.sender check".into() }
        );
    }

    #[test]
    fn auth_modifier() {
        let f = make_func("mint", Visibility::External, FunctionKind::Function, vec!["auth"], Some(vec![]));
        assert_eq!(
            classify_function(&f, &contract()),
            AccessLevel::Restricted { role: "auth".into() }
        );
    }

    #[test]
    fn msg_sender_is_exact() {
        assert!(expr_is_msg_sender(&msg_sender_expr()));

        let not_msg = Expression {
            kind: ExpressionKind::MemberAccess {
                object: Box::new(Expression {
                    kind: ExpressionKind::Identifier { name: "tx".into(), resolved: None },
                    span: span(),
                }),
                member: "sender".into(),
                resolved: None,
                object_type: None,
            },
            span: span(),
        };
        assert!(!expr_is_msg_sender(&not_msg));
    }
}
