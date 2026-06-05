use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub file_index: usize,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub type_name: String,
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
    pub span: SourceSpan,
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
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDef {
    pub name: String,
    pub params: Vec<Param>,
    pub span: SourceSpan,
}
