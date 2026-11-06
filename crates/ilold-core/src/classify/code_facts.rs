//! Builtins and unchecked arithmetic a function runs, modifiers included.
use serde::{Deserialize, Serialize};

use crate::cfg::builder::modifier_code;
use crate::model::common::SourceSpan;
use crate::model::contract::ContractDef;
use crate::model::expression::{CallType, Expression, ExpressionKind};
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::model::statement::{walk_all, Statement, StatementKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Builtin {
    Ecrecover,
    Selfdestruct,
    TxOrigin,
    BlockTimestamp,
    BlockNumber,
    Blockhash,
    CodeLength,
    Gasleft,
    MsgValue,
    Balance,
    BlockChainid,
    AbiEncodePacked,
}

impl Builtin {
    pub fn name(self) -> &'static str {
        match self {
            Builtin::Ecrecover => "ecrecover",
            Builtin::Selfdestruct => "selfdestruct",
            Builtin::TxOrigin => "tx.origin",
            Builtin::BlockTimestamp => "block.timestamp",
            Builtin::BlockNumber => "block.number",
            Builtin::Blockhash => "blockhash",
            Builtin::CodeLength => "code.length",
            Builtin::Gasleft => "gasleft",
            Builtin::MsgValue => "msg.value",
            Builtin::Balance => "balance",
            Builtin::BlockChainid => "block.chainid",
            Builtin::AbiEncodePacked => "abi.encodePacked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BuiltinUse {
    pub builtin: Builtin,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct CodeFacts {
    pub builtin_uses: Vec<BuiltinUse>,
    pub unchecked_blocks: Vec<SourceSpan>,
}

/// In source order: each modifier with its arguments as `context` resolves it, then the body.
pub fn code_facts(project: Option<&Project>, context: &ContractDef, func: &FunctionDef) -> CodeFacts {
    let mut builtin_uses = Vec::new();
    let mut unchecked_blocks = Vec::new();
    let mut on_expr = |_: Option<&str>, e: &Expression| {
        if let Some(builtin) = builtin(e) {
            builtin_uses.push(BuiltinUse { builtin, span: e.span });
        }
    };
    let mut on_stmt = |s: &Statement| {
        if matches!(s.kind, StatementKind::UncheckedBlock { .. }) {
            unchecked_blocks.push(s.span);
        }
    };
    walk_function(project, context, func, &mut on_stmt, &mut on_expr);
    CodeFacts { builtin_uses, unchecked_blocks }
}

/// In source order: each modifier's arguments and code as `context` resolves it, then the body; expressions come with the modifier whose code holds them.
pub(crate) fn walk_function<'a>(
    project: Option<&'a Project>,
    context: &'a ContractDef,
    func: &'a FunctionDef,
    on_stmt: &mut impl FnMut(&'a Statement),
    on_expr: &mut impl FnMut(Option<&'a str>, &'a Expression),
) {
    for mref in &func.modifiers {
        mref.arguments.iter().for_each(|a| a.walk(&mut |e| on_expr(None, e)));
        if let Some(def) = modifier_code(project, context, &mref.name) {
            walk_all(&def.body, on_stmt, &mut |e| on_expr(Some(mref.name.as_str()), e));
        }
    }
    walk_all(func.body.as_deref().unwrap_or_default(), on_stmt, &mut |e| on_expr(None, e));
}

pub(crate) fn builtin(expr: &Expression) -> Option<Builtin> {
    match &expr.kind {
        // a user function may shadow a builtin name
        ExpressionKind::FunctionCall { callee, call, .. } if !matches!(call, CallType::Call(_)) => match &callee.kind {
            ExpressionKind::Identifier { name, .. } => match name.as_str() {
                "ecrecover" => Some(Builtin::Ecrecover),
                "selfdestruct" => Some(Builtin::Selfdestruct),
                "blockhash" => Some(Builtin::Blockhash),
                "gasleft" => Some(Builtin::Gasleft),
                _ => None,
            },
            _ => None,
        },
        ExpressionKind::MemberAccess { object, member, object_type, .. } => {
            let global = |namespace: &str| matches!(&object.kind, ExpressionKind::Identifier { name, .. } if name == namespace);
            match member.as_str() {
                "origin" if global("tx") => Some(Builtin::TxOrigin),
                "timestamp" if global("block") => Some(Builtin::BlockTimestamp),
                "number" if global("block") => Some(Builtin::BlockNumber),
                "chainid" if global("block") => Some(Builtin::BlockChainid),
                "value" if global("msg") => Some(Builtin::MsgValue),
                "encodePacked" if global("abi") => Some(Builtin::AbiEncodePacked),
                "balance" if is_address(object, object_type.as_deref()) => Some(Builtin::Balance),
                "length" => match &object.kind {
                    ExpressionKind::MemberAccess { object, member, object_type, .. }
                        if member == "code" && is_address(object, object_type.as_deref()) =>
                    {
                        Some(Builtin::CodeLength)
                    }
                    _ => None,
                },
                _ => None,
            }
        }
        _ => None,
    }
}

// without solc types only an explicit conversion tells an address from a struct
fn is_address(object: &Expression, object_type: Option<&str>) -> bool {
    match object_type {
        Some(t) => t == "address" || t == "address payable",
        None => matches!(&object.kind, ExpressionKind::TypeCast { type_name, .. } if type_name.starts_with("address")),
    }
}
