use serde::{Deserialize, Serialize};

use super::common::{Param, SourceSpan};
use super::modifier::ModifierRef;
use super::statement::Statement;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub kind: FunctionKind,
    pub visibility: Visibility,
    pub mutability: Mutability,
    pub modifiers: Vec<ModifierRef>,
    // constructor `Base(args)` specifiers; not inlined like modifiers
    #[serde(default)]
    pub base_constructor_calls: Vec<ModifierRef>,
    pub params: Vec<Param>,
    pub returns: Vec<Param>,
    pub body: Option<Vec<Statement>>,
    pub is_virtual: bool,
    pub is_override: bool,
    pub span: SourceSpan,
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
