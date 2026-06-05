use serde::{Deserialize, Serialize};

use super::common::SourceSpan;
use super::decl_id::DeclId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExpressionKind {
    FunctionCall {
        callee: Box<Expression>,
        arguments: Vec<Expression>,
    },
    MemberAccess {
        object: Box<Expression>,
        member: String,
        #[serde(default)]
        resolved: Option<DeclId>,
    },
    BinaryOp {
        left: Box<Expression>,
        operator: BinaryOperator,
        right: Box<Expression>,
    },
    UnaryOp {
        operator: UnaryOperator,
        operand: Box<Expression>,
    },
    Identifier {
        name: String,
        #[serde(default)]
        resolved: Option<DeclId>,
    },
    Literal {
        value: String,
        literal_type: LiteralType,
    },
    IndexAccess {
        base: Box<Expression>,
        index: Option<Box<Expression>>,
    },
    Assignment {
        target: Box<Expression>,
        operator: AssignOperator,
        value: Box<Expression>,
    },
    Ternary {
        condition: Box<Expression>,
        true_expr: Box<Expression>,
        false_expr: Box<Expression>,
    },
    TypeCast {
        type_name: String,
        expression: Box<Expression>,
    },
    TypeMeta {
        type_name: String,
    },
    New {
        type_name: String,
        arguments: Vec<Expression>,
    },
    Tuple {
        elements: Vec<Option<Expression>>,
    },
    IndexRange {
        base: Box<Expression>,
        start: Option<Box<Expression>>,
        end: Option<Box<Expression>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOperator {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

impl BinaryOperator {
    pub fn as_str(self) -> &'static str {
        match self {
            BinaryOperator::Add => "+",
            BinaryOperator::Sub => "-",
            BinaryOperator::Mul => "*",
            BinaryOperator::Div => "/",
            BinaryOperator::Mod => "%",
            BinaryOperator::Pow => "**",
            BinaryOperator::Eq => "==",
            BinaryOperator::Neq => "!=",
            BinaryOperator::Lt => "<",
            BinaryOperator::Gt => ">",
            BinaryOperator::Lte => "<=",
            BinaryOperator::Gte => ">=",
            BinaryOperator::And => "&&",
            BinaryOperator::Or => "||",
            BinaryOperator::BitAnd => "&",
            BinaryOperator::BitOr => "|",
            BinaryOperator::BitXor => "^",
            BinaryOperator::Shl => "<<",
            BinaryOperator::Shr => ">>",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOperator {
    Not,
    Neg,
    BitNot,
    PreIncrement,
    PreDecrement,
    PostIncrement,
    PostDecrement,
    Delete,
}

impl UnaryOperator {
    pub fn format_parts(self) -> (&'static str, bool) {
        match self {
            UnaryOperator::Not => ("!", false),
            UnaryOperator::Neg => ("-", false),
            UnaryOperator::BitNot => ("~", false),
            UnaryOperator::PreIncrement => ("++", false),
            UnaryOperator::PreDecrement => ("--", false),
            UnaryOperator::PostIncrement => ("++", true),
            UnaryOperator::PostDecrement => ("--", true),
            UnaryOperator::Delete => ("delete ", false),
        }
    }

    pub fn mutates_operand(self) -> bool {
        matches!(
            self,
            UnaryOperator::PreIncrement
                | UnaryOperator::PostIncrement
                | UnaryOperator::PreDecrement
                | UnaryOperator::PostDecrement
                | UnaryOperator::Delete
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssignOperator {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    BitAndAssign,
    BitOrAssign,
    BitXorAssign,
    ShlAssign,
    ShrAssign,
}

impl AssignOperator {
    pub fn as_str(self) -> &'static str {
        match self {
            AssignOperator::Assign => "=",
            AssignOperator::AddAssign => "+=",
            AssignOperator::SubAssign => "-=",
            AssignOperator::MulAssign => "*=",
            AssignOperator::DivAssign => "/=",
            AssignOperator::ModAssign => "%=",
            AssignOperator::BitAndAssign => "&=",
            AssignOperator::BitOrAssign => "|=",
            AssignOperator::BitXorAssign => "^=",
            AssignOperator::ShlAssign => "<<=",
            AssignOperator::ShrAssign => ">>=",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiteralType {
    Number,
    String,
    Bool,
    HexString,
    Address,
}
