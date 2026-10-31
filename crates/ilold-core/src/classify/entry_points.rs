use std::fmt;

use serde::{Deserialize, Serialize};

use crate::classify::guards::{function_guards, GuardFact};
use crate::model::contract::ContractDef;
use crate::model::function::{FunctionDef, FunctionKind, Visibility};
use crate::model::project::Project;

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

/// Access level of `func` read from its own contract only (no inherited modifiers or helpers).
/// Use [`classify_in`] when the project is at hand.
pub fn classify_function(func: &FunctionDef, contract: &ContractDef) -> AccessLevel {
    access_level(func, &function_guards(None, contract, contract, func))
}

/// Access level and guards of `func` (declared in `owner`) as `context` runs it.
pub fn classify_in(
    project: &Project,
    context: &ContractDef,
    owner: &ContractDef,
    func: &FunctionDef,
) -> (AccessLevel, Vec<GuardFact>) {
    let guards = function_guards(Some(project), context, owner, func);
    (access_level(func, &guards), guards)
}

/// Access level of the function `name` (bare, key or signature) as `context` runs it.
pub fn classify_named(project: &Project, context: &ContractDef, name: &str) -> Option<AccessLevel> {
    let (owner, func) = project.resolve_function(context, name)?;
    Some(classify_in(project, context, owner, func).0)
}

/// `Restricted` when an access guard exists; its role names the modifiers that carry the
/// guards (the first one on the chain, also when reached through a called function), or the
/// checks themselves when no modifier is on the chain.
pub fn access_level(func: &FunctionDef, guards: &[GuardFact]) -> AccessLevel {
    match func.kind {
        FunctionKind::Constructor => return AccessLevel::Special { kind: "constructor".into() },
        FunctionKind::Fallback => return AccessLevel::Special { kind: "fallback".into() },
        FunctionKind::Receive => return AccessLevel::Special { kind: "receive".into() },
        FunctionKind::Function => {}
    }
    if matches!(func.visibility, Visibility::Internal | Visibility::Private) {
        return AccessLevel::Internal;
    }
    let mut sources: Vec<String> = Vec::new();
    for g in guards.iter().filter(|g| g.kind.is_access()) {
        let source = match g.via.iter().find_map(|v| v.strip_prefix("modifier ")) {
            Some(modifier) => modifier.to_string(),
            None => g.condition.clone(),
        };
        if !sources.contains(&source) {
            sources.push(source);
        }
    }
    if sources.is_empty() {
        AccessLevel::Public
    } else {
        AccessLevel::Restricted { role: sources.join(", ") }
    }
}

/// Own functions of `contract`, read from the contract alone.
pub fn classify_all(contract: &ContractDef) -> Vec<(String, AccessLevel)> {
    contract
        .functions
        .iter()
        .map(|f| (f.name.clone(), classify_function(f, contract)))
        .collect()
}

/// Own and inherited functions of `contract` as the most derived contract runs them.
pub fn classify_contract(project: &Project, contract: &ContractDef) -> Vec<(String, AccessLevel)> {
    project
        .accessible_functions(contract)
        .into_iter()
        .map(|af| (af.function.name.clone(), classify_in(project, contract, af.owner, af.function).0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::guards::GuardKind;
    use crate::model::expression::{BinaryOperator, Expression, ExpressionKind};
    use crate::model::statement::{Statement, StatementKind};
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
            qualified_id: String::new(), key: String::new(), is_dependency: false, base_ids: vec![],
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

    fn kinds(f: &FunctionDef) -> Vec<(GuardKind, bool)> {
        function_guards(None, &contract(), &contract(), f).iter().map(|g| (g.kind, g.heuristic)).collect()
    }

    #[test]
    fn when_not_paused_is_a_state_guard_not_access() {
        let f = make_func("deposit", Visibility::External, FunctionKind::Function, vec!["whenNotPaused"], Some(vec![]));
        assert_eq!(classify_function(&f, &contract()), AccessLevel::Public);
        assert_eq!(kinds(&f), [(GuardKind::Pause, true)]);
    }

    #[test]
    fn nonreentrant_is_a_state_guard_not_access() {
        let f = make_func("withdraw", Visibility::External, FunctionKind::Function, vec!["nonReentrant"], Some(vec![]));
        assert_eq!(classify_function(&f, &contract()), AccessLevel::Public);
        assert_eq!(kinds(&f), [(GuardKind::Reentrancy, true)]);
    }

    #[test]
    fn initializer_is_a_state_guard_not_access() {
        let f = make_func("initialize", Visibility::External, FunctionKind::Function, vec!["initializer"], Some(vec![]));
        assert_eq!(classify_function(&f, &contract()), AccessLevel::Public);
        assert_eq!(kinds(&f), [(GuardKind::Initializer, true)]);
    }

    #[test]
    fn unresolved_access_modifier_is_a_heuristic_fact() {
        let f = make_func("setRate", Visibility::External, FunctionKind::Function, vec!["onlyOwner"], Some(vec![]));
        assert_eq!(kinds(&f), [(GuardKind::AccessByName, true)]);
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
            AccessLevel::Restricted { role: "require(msg.sender == owner)".into() }
        );
        assert_eq!(kinds(&f), [(GuardKind::CallerEquals, false)]);
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
    fn tx_sender_is_not_the_caller() {
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
        let mut stmt = require_sender_eq_owner();
        if let StatementKind::ExpressionStmt { expression } = &mut stmt.kind {
            if let ExpressionKind::FunctionCall { arguments, .. } = &mut expression.kind {
                if let ExpressionKind::BinaryOp { left, .. } = &mut arguments[0].kind {
                    **left = not_msg;
                }
            }
        }
        let f = make_func("f", Visibility::External, FunctionKind::Function, vec![], Some(vec![stmt]));
        assert_eq!(classify_function(&f, &contract()), AccessLevel::Public);
        assert_eq!(kinds(&f), []);
    }
}
