use serde::{Deserialize, Serialize};

use super::common::{EnumDef, ErrorDef, EventDef, SourceSpan, StateVar, StructDef};
use super::decl_id::DeclId;
use super::function::FunctionDef;
use super::modifier::ModifierDef;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDef {
    /// Not unique across a project; see `key` and `qualified_id`.
    pub name: String,
    /// `<source path relative to the project root>:<Name>`
    #[serde(default)]
    pub qualified_id: String,
    /// The bare name when no other contract can claim it, otherwise `qualified_id`.
    #[serde(default)]
    pub key: String,
    /// Loaded so that inheritance and calls resolve; hidden from default listings.
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
    pub inherits: Vec<String>,
    /// solc `baseContracts[].baseName.referencedDeclaration`
    #[serde(default)]
    pub base_ids: Vec<DeclId>,
    #[serde(default)]
    pub id: Option<DeclId>,
    /// solc `linearizedBaseContracts`, the contract itself first.
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

pub const FREE_FUNCTIONS: &str = "FreeFunctions";

impl ContractDef {
    /// The name for hand-built models without a key.
    pub fn key(&self) -> &str {
        if self.key.is_empty() { &self.name } else { &self.key }
    }

    pub fn is_free_functions(&self) -> bool {
        self.id.is_none() && self.name == FREE_FUNCTIONS
    }

    /// Hidden contracts still resolve.
    pub fn is_listed(&self) -> bool {
        !self.is_dependency && !self.is_free_functions()
    }
}
