//! What was analysed: the git revision and every Solidity file under the project with its status.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use crate::model::project::{CompileFailure, Project};
use crate::parse::cache::{digest, NON_SOURCE_DIRS};
use crate::parse::solc_frontend::{is_dependency_path, ProjectLayout};

use super::ids::Paths;

pub(super) struct Repo {
    pub root: Option<PathBuf>,
    pub commit: Option<String>,
    pub dirty: Option<bool>,
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    // optional locks would let `status` rewrite the index of the analysed repository
    let out = Command::new("git").arg("--no-optional-locks").arg("-C").arg(dir).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

impl Repo {
    pub fn locate(dir: &Path) -> Self {
        let Some(root) = git(dir, &["rev-parse", "--show-toplevel"]).and_then(|top| Path::new(&top).canonicalize().ok()) else {
            return Self { root: None, commit: None, dirty: None };
        };
        let commit = git(dir, &["rev-parse", "--verify", "--quiet", "HEAD"]);
        let dirty = git(dir, &["status", "--porcelain", "--untracked-files=all", "--", "."]).map(|s| !s.is_empty());
        Self { root: Some(root), commit, dirty }
    }
}

/// Analysed sources in path order: each path, then the digest of its content.
pub(super) fn sources_sha256(project: Option<&Project>, paths: &Paths) -> String {
    let files: BTreeMap<String, String> = project
        .map(|p| p.source_files.iter().map(|f| (paths.relative(Path::new(&f.path)), digest(f.content.as_bytes()))).collect())
        .unwrap_or_default();
    let material: String = files.iter().map(|(path, hash)| format!("{path}\0{hash}\n")).collect();
    digest(material.as_bytes())
}

pub(super) fn remappings_sha256(layout: &ProjectLayout) -> String {
    digest(layout.remappings.join("\n").as_bytes())
}

fn solidity_files(dir: &Path, out: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.filter_map(Result::ok) {
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        if kind.is_dir() && !entry.file_name().to_str().is_some_and(|name| NON_SOURCE_DIRS.contains(&name)) {
            solidity_files(&path, out);
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "sol") {
            out.insert(path);
        }
    }
}

const UNIMPORTED: &str = "no project source imports it";
const SOURCES_FAILED: &str = "a compile error inside the sources left no facts";

/// `project` is `None` when the sources failed to compile; `failures` then name the failing files.
pub(super) fn files(layout: &ProjectLayout, paths: &Paths, project: Option<&Project>, failures: &[CompileFailure]) -> Vec<Value> {
    let mut files = BTreeSet::new();
    solidity_files(&layout.root, &mut files);
    let modelled: BTreeSet<PathBuf> =
        project.map(|p| p.source_files.iter().map(|f| PathBuf::from(&f.path)).collect()).unwrap_or_default();
    files.extend(modelled.iter().cloned());
    let mut failed: BTreeMap<PathBuf, Vec<&CompileFailure>> = BTreeMap::new();
    for failure in failures {
        if let Some(path) = &failure.path {
            failed.entry(layout.root.join(path)).or_default().push(failure);
        }
    }
    files.extend(failed.keys().cloned());

    let mut entries: Vec<Value> = files
        .iter()
        .map(|file| {
            let (status, reason) = if failed.contains_key(file) {
                ("failed_compile", None)
            } else if project.is_none() {
                ("excluded", Some(SOURCES_FAILED))
            } else if modelled.contains(file) {
                (if is_dependency_path(file, &layout.sources) { "dependency" } else { "analysed" }, None)
            } else if file.starts_with(&layout.tests) || file.starts_with(&layout.scripts) {
                ("test_or_script", Some(UNIMPORTED))
            } else {
                ("excluded", Some(UNIMPORTED))
            };
            let errors = failed.get(file);
            json!({
                "path": paths.relative(file),
                "status": status,
                "reason": reason,
                "error": errors.map(|e| paths.relative_text(&e.iter().map(|f| f.message.as_str()).collect::<Vec<_>>().join("; "))),
                "line": errors.and_then(|e| e.iter().find_map(|f| f.line)),
            })
        })
        .collect();
    entries.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    entries
}

/// Compile errors without a file, the note that a partial compile left files out, and empty dependency directories.
pub(super) fn warnings(layout: &ProjectLayout, paths: &Paths, failures: &[CompileFailure], partial: bool) -> Vec<Value> {
    let partial = partial.then(|| {
        json!({"kind": "partial_compile", "message": "files outside the sources failed to compile; facts cover the sources and their imports"})
    });
    let empty = empty_dependencies(layout, paths).into_iter()
        .map(|dir| json!({"kind": "empty_dependency", "message": format!("{dir} is empty (git submodule not initialised?)")}));
    partial
        .into_iter()
        .chain(failures.iter().filter(|f| f.path.is_none()).map(|f| json!({"kind": "compile_error", "message": paths.relative_text(&f.message)})))
        .chain(empty)
        .collect()
}

/// The first empty directory on the way to each remapping target, and declared submodules that are empty or missing.
fn empty_dependencies(layout: &ProjectLayout, paths: &Paths) -> BTreeSet<String> {
    let is_empty = |dir: &Path| std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none());
    // a missing directory on the way is a stale remapping, not an uninitialised submodule
    let remapped = layout.remappings.iter().filter_map(|r| r.split_once('=')).filter_map(|(_, target)| {
        let target = layout.root.join(target);
        let mut below: Vec<&Path> =
            target.ancestors().take_while(|dir| *dir != paths.base && dir.starts_with(&paths.base)).collect();
        below.reverse();
        below.into_iter().take_while(|dir| dir.exists()).find(|dir| is_empty(dir)).map(Path::to_path_buf)
    });
    let modules = git(&paths.base, &["config", "--null", "--file", ".gitmodules", "--get-regexp", r"^submodule\..*\.path$"]);
    let declared = modules.iter().flat_map(|m| m.split('\0')).filter_map(|entry| entry.split_once('\n'))
        .map(|(_, path)| paths.base.join(path))
        .filter(|dir| !dir.exists() || is_empty(dir));
    remapped.chain(declared).map(|dir| paths.relative(&dir)).collect()
}
