//! Stable ids: `evm:<git-root-relative file>:<Contract>.<member>`.
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::model::common::{Param, SourceSpan};
use crate::model::contract::ContractDef;
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::parse::solc_frontend::canonical_type;

/// Paths relative to the git root, or to the Foundry root outside a repository.
pub(super) struct Paths {
    pub base: PathBuf,
}

impl Paths {
    pub fn relative(&self, path: &Path) -> String {
        if let Ok(rest) = path.strip_prefix(&self.base) {
            return rest.to_string_lossy().into_owned();
        }
        let common = path.components().zip(self.base.components()).take_while(|(a, b)| a == b).count();
        let mut out: PathBuf = std::iter::repeat_n("..", self.base.components().count() - common).collect();
        out.extend(path.components().skip(common));
        out.to_string_lossy().into_owned()
    }
}

pub(super) struct Ids<'p> {
    pub project: &'p Project,
    files: Vec<String>,
}

impl<'p> Ids<'p> {
    pub fn new(project: &'p Project, paths: &Paths) -> Self {
        let files = project.source_files.iter().map(|f| paths.relative(Path::new(&f.path))).collect();
        Self { project, files }
    }

    fn file(&self, index: usize) -> &str {
        self.files.get(index).map_or("", String::as_str)
    }

    pub fn span(&self, span: SourceSpan) -> Value {
        json!({
            "file": self.file(span.file_index),
            "start_line": span.start_line,
            "start_col": span.start_col,
            "end_line": span.end_line,
            "end_col": span.end_col,
        })
    }

    // free functions belong to their file, not to a contract
    fn scope(&self, c: &ContractDef) -> String {
        let file = self.file(c.span.file_index);
        if c.is_free_functions() { format!("evm:{file}:") } else { format!("evm:{file}:{}.", c.name) }
    }

    pub fn contract(&self, c: &ContractDef) -> Option<String> {
        (!c.is_free_functions()).then(|| format!("evm:{}:{}", self.file(c.span.file_index), c.name))
    }

    pub fn function(&self, c: &ContractDef, f: &FunctionDef) -> String {
        let signature = f.match_signature();
        let parens = if signature.contains('(') { "" } else { "()" };
        format!("{}{signature}{parens}", self.scope(c))
    }

    pub fn member(&self, c: &ContractDef, name: &str) -> String {
        format!("{}{name}", self.scope(c))
    }

    /// Events and errors carry their parameter types only when overloaded.
    pub fn overloadable(&self, c: &ContractDef, name: &str, params: &[Param], overloaded: bool) -> String {
        if overloaded {
            format!("{}{name}({})", self.scope(c), param_types(params).join(","))
        } else {
            self.member(c, name)
        }
    }

    pub fn function_by_key(&self, contract: &str, key: &str) -> Option<String> {
        let c = &self.project.contracts[*self.project.contract_index.get(contract)?];
        c.functions.iter().find(|f| f.key == key).map(|f| self.function(c, f))
    }

    /// From a declaration's readable key, `<contract qualified id>::<name>`.
    pub fn member_by_key(&self, key: &str) -> Option<String> {
        let (contract, name) = key.rsplit_once("::")?;
        Some(self.member(&self.project.contracts[*self.project.contract_index.get(contract)?], name))
    }
}

pub(super) fn param_types(params: &[Param]) -> Vec<String> {
    params.iter().map(|p| canonical_type(&p.type_name)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_outside_the_base_climb_instead_of_turning_absolute() {
        let paths = Paths { base: PathBuf::from("/repo/project") };
        assert_eq!(paths.relative(Path::new("/repo/project/src/A.sol")), "src/A.sol");
        assert_eq!(paths.relative(Path::new("/repo/project")), "");
        assert_eq!(paths.relative(Path::new("/repo/shared/lib/B.sol")), "../shared/lib/B.sol");
    }
}
