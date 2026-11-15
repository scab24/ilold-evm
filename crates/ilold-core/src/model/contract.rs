use serde::{Deserialize, Serialize};

use super::common::{EnumDef, ErrorDef, EventDef, SourceSpan, StateVar, StructDef};
use super::decl_id::DeclId;
use super::function::FunctionDef;
use super::modifier::ModifierDef;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDef {
    pub name: String,
    #[serde(default)]
    pub qualified_id: String,
    #[serde(default)]
    pub key: String,
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
    #[serde(default)]
    pub base_ids: Vec<DeclId>,
    #[serde(default)]
    pub id: Option<DeclId>,
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
    pub fn key(&self) -> &str {
        if self.key.is_empty() { &self.name } else { &self.key }
    }

    pub fn is_free_functions(&self) -> bool {
        self.id.is_none() && self.name == FREE_FUNCTIONS
    }

    pub fn is_listed(&self) -> bool {
        !self.is_dependency && !self.is_free_functions()
    }

    pub fn is_deployable(&self) -> bool {
        self.kind == ContractKind::Contract && !self.is_free_functions()
    }

    pub fn is_project_deployable(&self) -> bool {
        !self.is_dependency && self.is_deployable()
    }
}
