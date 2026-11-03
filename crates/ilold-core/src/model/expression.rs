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
        #[serde(default)]
        options: Vec<CallOption>,
        #[serde(default)]
        call: CallType,
    },
    MemberAccess {
        object: Box<Expression>,
        member: String,
        #[serde(default)]
        resolved: Option<DeclId>,
        // solc type string of the object
        #[serde(default)]
        object_type: Option<String>,
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
        #[serde(default)]
        options: Vec<CallOption>,
        // `new C(...)` deploys a contract; `new uint256[](n)` only allocates
        #[serde(default)]
        creates_contract: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallOption {
    pub name: String,
    pub value: Expression,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallType {
    /// No compiler type information; consumers fall back to syntax and names.
    #[default]
    Unknown,
    /// Compiler builtin, event, error or array push/pop: no contract code runs.
    Builtin,
    Call(CallKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CallKind {
    Internal,
    InternalLibrary,
    LibraryDelegatecall,
    External,
    ExternalSelf,
    LowLevelCall,
    LowLevelDelegatecall,
    LowLevelStaticcall,
    Transfer,
    Send,
    Creation,
}

impl CallKind {
    pub fn is_internal(self) -> bool {
        matches!(self, CallKind::Internal | CallKind::InternalLibrary)
    }

    pub fn label(self) -> &'static str {
        match self {
            CallKind::Internal => "internal",
            CallKind::InternalLibrary => "library internal",
            CallKind::LibraryDelegatecall => "library delegatecall",
            CallKind::External => "external",
            CallKind::ExternalSelf => "external self",
            CallKind::LowLevelCall => "low-level call",
            CallKind::LowLevelDelegatecall => "low-level delegatecall",
            CallKind::LowLevelStaticcall => "low-level staticcall",
            CallKind::Transfer => "transfer",
            CallKind::Send => "send",
            CallKind::Creation => "creation",
        }
    }

    /// `None` means a builtin that runs no contract code.
    pub fn from_solc_function_kind(kind: &str) -> Option<CallKind> {
        Some(match kind {
            "internal" => CallKind::Internal,
            "external" => CallKind::External,
            "delegatecall" => CallKind::LibraryDelegatecall,
            "barecall" => CallKind::LowLevelCall,
            "baredelegatecall" | "barecallcode" => CallKind::LowLevelDelegatecall,
            "barestaticcall" => CallKind::LowLevelStaticcall,
            "transfer" => CallKind::Transfer,
            "send" => CallKind::Send,
            "creation" => CallKind::Creation,
            _ => return None,
        })
    }
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
    /// Never silently mapped to another operator.
    Unknown,
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
            BinaryOperator::Unknown => "<?>",
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
    Unknown,
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
            UnaryOperator::Unknown => ("<?>", false),
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
    Unknown,
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
            AssignOperator::Unknown => "<?>=",
        }
    }

    /// Writes made through a call have no value expression to show.
    pub fn describe_change(self, value_expr: &str) -> String {
        if value_expr.is_empty() { "written".to_string() } else { format!("{} {}", self.as_str(), value_expr) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiteralType {
    Number,
    String,
    Bool,
    HexString,
    UnicodeString,
    Address,
    Unknown,
}
