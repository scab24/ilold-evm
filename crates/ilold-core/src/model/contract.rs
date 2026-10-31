use serde::{Deserialize, Serialize};

use super::common::{EnumDef, ErrorDef, EventDef, SourceSpan, StateVar, StructDef};
use super::decl_id::DeclId;
use super::function::FunctionDef;
use super::modifier::ModifierDef;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDef {
    /// Solidity name; not unique across a project (see `key` and `qualified_id`).
    pub name: String,
    /// Stable id `<source path relative to the project root>:<Name>`.
    #[serde(default)]
    pub qualified_id: String,
    /// Unique within the model: the bare name when no other contract can claim it, otherwise
    /// `qualified_id`. Analysis maps, call-graph and dependency-graph ids use it.
    #[serde(default)]
    pub key: String,
    /// Declared outside the project sources (libraries, vendored code). Loaded so that
    /// inheritance and calls resolve; hidden from default listings.
    #[serde(default)]
    pub is_dependency: bool,
    pub kind: ContractKind,
    pub functions: Vec<FunctionDef>,
    pub modifiers: Vec<ModifierDef>,
    pub state_vars: Vec<StateVar>,
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
    pub events: Vec<EventDef>,
    pub errors: Vec<ErrorDef>,
    /// Base names as written; `base_ids` holds the declarations they resolve to.
    pub inherits: Vec<String>,
    /// solc `baseContracts[].baseName.referencedDeclaration`, in declaration order.
    #[serde(default)]
    pub base_ids: Vec<DeclId>,
    #[serde(default)]
    pub id: Option<DeclId>,
    /// solc `linearizedBaseContracts`: C3 order, the contract itself first.
    #[serde(default)]
    pub linearized_bases: Vec<DeclId>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContractKind {
    Contract,
    Interface,
    Library,
    Abstract,
}

/// Name of the library-like container that holds a source file's free functions.
pub const FREE_FUNCTIONS: &str = "FreeFunctions";

impl ContractDef {
    /// `key`, or the name for hand-built models without one.
    pub fn key(&self) -> &str {
        if self.key.is_empty() { &self.name } else { &self.key }
    }

    pub fn is_free_functions(&self) -> bool {
        self.id.is_none() && self.name == FREE_FUNCTIONS
    }

    /// Shown by default listings; hidden contracts still resolve.
    pub fn is_listed(&self) -> bool {
        !self.is_dependency && !self.is_free_functions()
    }
}
