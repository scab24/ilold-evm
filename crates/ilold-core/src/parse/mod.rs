pub(crate) mod cache;
pub mod error;
pub mod solc_frontend;
pub mod span;

use std::path::PathBuf;

use crate::model::project::Project;
use error::ParseError;

pub trait ProjectParser {
    fn parse(&self, paths: &[PathBuf]) -> Result<Project, ParseError>;
}

/// Identity of a parsed project whose inputs were validated against the persistent cache.
/// Present only when the project cache was usable; derived caches must bypass without it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectKey {
    pub root: PathBuf,
    pub key: String,
}
