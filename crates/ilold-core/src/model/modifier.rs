use serde::{Deserialize, Serialize};

use super::common::{Param, SourceSpan};
use super::expression::Expression;
use super::statement::Statement;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifierDef {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Vec<Statement>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifierRef {
    pub name: String,
    pub arguments: Vec<Expression>,
}
