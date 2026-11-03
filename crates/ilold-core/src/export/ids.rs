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

    /// Absolute paths inside free text, such as compiler messages: relative under the base, `<outside-root>/<file name>` elsewhere.
    pub fn relative_text(&self, text: &str) -> String {
        let base = self.base.to_string_lossy();
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        while let Some(found) = text[at..].find('/') {
            let start = at + found;
            out.push_str(&text[at..start]);
            let opens = text[..start].chars().next_back().is_none_or(|c| c.is_whitespace() || "\"'`([<{=,:".contains(c));
            // the base may contain characters that end a path elsewhere, such as spaces
            let from = if text[start..].starts_with(&*base) { start + base.len() } else { start };
            let end = text[from..].find(|c: char| c.is_whitespace() || "\"'`)]>},;".contains(c)).map_or(text.len(), |i| from + i);
            let path = text[start..end].trim_end_matches(['.', ':']);
            if opens && Path::new(path).file_name().is_some() {
                out.push_str(&self.render(Path::new(path)));
                at = start + path.len();
            } else {
                out.push('/');
                at = start + 1;
            }
        }
        out.push_str(&text[at..]);
        out
    }

    fn render(&self, path: &Path) -> String {
        let Some(inside) = [path.to_path_buf(), resolved(path)].into_iter().find(|p| p.starts_with(&self.base)) else {
            return format!("<outside-root>/{}", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
        };
        let relative = self.relative(&inside);
        if relative.is_empty() { ".".to_string() } else { relative }
    }
}

/// Symlinks resolved in the longest existing prefix, e.g. `/tmp` to `/private/tmp` on macOS.
fn resolved(path: &Path) -> PathBuf {
    path.ancestors()
        .find_map(|dir| Some(dir.canonicalize().ok()?.join(path.strip_prefix(dir).ok()?)))
        .unwrap_or_else(|| path.to_path_buf())
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

    #[test]
    fn absolute_paths_in_text_become_relative_or_lose_their_location() {
        let paths = Paths { base: PathBuf::from("/repo/my project") };
        let text = r#"Source "lib/a/B.sol" not found. Searched the following locations: "/repo/my project". See /repo/my project/src/A.sol:3, (/opt/x/C.sol) and /repo/my project2/D.sol."#;
        assert_eq!(
            paths.relative_text(text),
            r#"Source "lib/a/B.sol" not found. Searched the following locations: ".". See src/A.sol:3, (<outside-root>/C.sol) and <outside-root>/D.sol."#
        );
        assert_eq!(paths.relative_text("a / b, x/y, //, 3/4"), "a / b, x/y, //, 3/4");
    }

    #[cfg(unix)]
    #[test]
    fn paths_in_text_through_a_symlink_resolve_to_the_base() {
        let dir = std::env::temp_dir().join(format!("ilold-relative-text-{}", std::process::id()));
        let real = dir.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, dir.join("link")).unwrap();
        let paths = Paths { base: real.canonicalize().unwrap() };
        let text = format!("\"{}/src/Missing.sol\"", dir.join("link").display());
        let rendered = paths.relative_text(&text);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(rendered, "\"src/Missing.sol\"");
    }
}
