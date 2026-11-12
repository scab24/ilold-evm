use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceSpan {
    pub file_index: usize,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

impl SourceSpan {
    pub fn contains(&self, other: &SourceSpan) -> bool {
        self.file_index == other.file_index
            && (self.start_line, self.start_col) <= (other.start_line, other.start_col)
            && (other.end_line, other.end_col) <= (self.end_line, self.end_col)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub type_name: String,
    /// A pointer into the caller's storage.
    #[serde(default)]
    pub is_storage: bool,
    #[serde(default)]
    pub id: Option<super::decl_id::DeclId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateVar {
    pub name: String,
    pub type_name: String,
    pub visibility: super::function::Visibility,
    pub is_constant: bool,
    pub is_immutable: bool,
    pub initial_value: Option<super::expression::Expression>,
    #[serde(default)]
    pub resolved_types: Vec<super::decl_id::DeclId>,
    #[serde(default)]
    pub id: Option<super::decl_id::DeclId>,
    /// It implements an interface function with this selector.
    #[serde(default)]
    pub selector: Option<String>,
    pub span: SourceSpan,
}

impl StateVar {
    /// Constants and immutables live in the code.
    pub fn is_storage(&self) -> bool {
        !self.is_constant && !self.is_immutable
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<Param>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<String>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDef {
    pub name: String,
    pub params: Vec<Param>,
    #[serde(default)]
    pub id: Option<super::decl_id::DeclId>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDef {
    pub name: String,
    pub params: Vec<Param>,
    #[serde(default)]
    pub id: Option<super::decl_id::DeclId>,
    pub span: SourceSpan,
}
