use serde::{Deserialize, Serialize};

use super::common::{Param, SourceSpan};
use super::decl_id::DeclId;
use super::modifier::ModifierRef;
use super::statement::Statement;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    /// Unique within the defining contract: the name, or the signature when overloaded.
    /// Analysis maps and call-graph ids use `(contract, key)`.
    #[serde(default)]
    pub key: String,
    /// `name(type,...)` from solc parameter types, without data locations.
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

impl FunctionDef {
    /// Signature used to match overrides and overloads; hand-built models fall back to the name.
    pub fn match_signature(&self) -> &str {
        if self.signature.is_empty() { &self.name } else { &self.signature }
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
