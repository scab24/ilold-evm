//! Conservative, content-addressed parsed-project snapshot. Never stores compiler artifacts.
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use foundry_compilers::ProjectPathsConfig;
use foundry_compilers::resolver::parse::SolData;
use foundry_compilers::solc::{Solc, SolcLanguage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::contract::ContractDef;
use crate::model::decl_id::DeclTable;
use crate::model::project::{Project, SourceFile};

const VERSION: &str = "ilold-project-v2-foundry-0.21-sha256";
const MAGIC: &[u8; 8] = b"ILOLDV02";
const HEADER: usize = 8 + 32 + 64 + 32 + 8;
const MAX_FILES: usize = 4096;
const MAX_INPUT: u64 = 64 * 1024 * 1024;
const MAX_COMPILER: u64 = 128 * 1024 * 1024;
const MAX_SNAPSHOT: u64 = 64 * 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

fn digest(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }

fn read_bounded(path: &Path, max: u64) -> Result<String, String> {
    if fs::symlink_metadata(path).map_err(|e| e.to_string())?.file_type().is_symlink() {
        return Err("symlinked input".into());
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > max { return Err("input size limit".into()); }
    let mut content = Vec::new();
    file.take(max + 1).read_to_end(&mut content).map_err(|e| e.to_string())?;
    if content.len() as u64 > max { return Err("input size limit".into()); }
    String::from_utf8(content).map_err(|e| e.to_string())
}

fn hash_file(path: &Path, max: u64) -> Result<String, String> {
    if fs::symlink_metadata(path).map_err(|e| e.to_string())?.file_type().is_symlink() {
        return Err("symlinked input".into());
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    if len > max { return Err("input size limit".into()); }
    let mut bytes = Vec::with_capacity(len as usize);
    file.take(max + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max { return Err("input size limit".into()); }
    Ok(digest(&bytes))
}

fn walk(path: &Path, files: &mut BTreeSet<PathBuf>, visited: &mut usize) -> Result<(), String> {
    *visited += 1;
    if *visited > MAX_FILES { return Err("file count limit".into()); }
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    if meta.file_type().is_symlink() { return Err("symlinked source tree".into()); }
    if meta.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if matches!(entry.file_name().to_str(), Some(".git" | ".codegraph" | "target" | "out" | "cache")) {
                continue;
            }
            walk(&entry.path(), files, visited)?;
        }
    } else if meta.is_file() && path.extension().is_some_and(|ext| ext == "sol") {
        files.insert(path.to_path_buf());
    }
    Ok(())
}

/// Full roots, library and remapping trees are included, not just prior compiled files.
/// A bounded import preflight proves every transitive import is within the enumerated closure.
pub(super) fn fingerprint(
    root: &Path, paths: &ProjectPathsConfig<SolcLanguage>,
    settings: &impl Serialize, profile: &str, solc: &Solc,
) -> Result<String, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    if root.to_str().is_none() { return Err("non-UTF8 root".into()); }
    if paths.root != root { return Err("noncanonical project paths".into()); }
    if fs::symlink_metadata(&solc.solc).map_err(|e| e.to_string())?.file_type().is_symlink() {
        return Err("symlinked compiler".into());
    }
    let compiler_path = solc.solc.canonicalize().map_err(|e| e.to_string())?;
    let compiler = hash_file(&compiler_path, MAX_COMPILER)?;
    let mut files = BTreeSet::new();
    let mut visited = 0;
    for dir in [&paths.sources, &paths.tests, &paths.scripts].into_iter()
        .map(PathBuf::as_path)
        .chain(paths.libraries.iter().map(PathBuf::as_path))
        .chain(paths.remappings.iter().map(|r| Path::new(&r.path))) {
        walk(dir, &mut files, &mut visited)?;
    }
    if files.len() > MAX_FILES { return Err("file count limit".into()); }
    // Parse only already-bounded bytes. Foundry's graph resolver reads dependencies
    // before returning its graph, so it cannot enforce a pre-read byte budget here.
    // The same Foundry path resolver checks import targets without opening source data.
    let mut total = 0u64;
    let mut sources = Vec::new();
    for file in &files {
        let len = fs::symlink_metadata(file).map_err(|e| e.to_string())?.len();
        total = total.checked_add(len).ok_or("input size overflow")?;
        if total > MAX_INPUT { return Err("input size limit".into()); }
        let content = read_bounded(file, MAX_INPUT - (total - len))?;
        sources.push((file.clone(), content));
    }
    for (file, content) in &sources {
        let parsed = SolData::parse(content, file);
        parsed.parse_result().map_err(|_| "unprovable import parse".to_string())?;
        for import in parsed.imports {
            let target = paths.resolve_import(file.parent().ok_or("missing import parent")?, import.data.path())
                .map_err(|_| "unprovable import resolution".to_string())?;
            if !files.contains(&target) { return Err("import outside enumerated roots".into()); }
        }
    }
    let mut material = serde_json::to_vec(&(VERSION, &root, profile, settings, &paths.root,
        &paths.sources, &paths.tests, &paths.scripts, &paths.libraries,
        &paths.remappings, &paths.include_paths, &paths.allowed_paths,
        &solc.version.to_string(), &compiler_path, compiler)).map_err(|e| e.to_string())?;
    for config in ["foundry.toml", "remappings.txt"] {
        let path = root.join(config);
        material.extend_from_slice(config.as_bytes());
        match fs::symlink_metadata(&path) {
            Ok(_) => material.extend_from_slice(hash_file(&path, MAX_INPUT)?.as_bytes()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => material.extend_from_slice(b"absent"),
            Err(e) => return Err(e.to_string()),
        }
    }
    for (file, content) in sources {
        material.extend_from_slice(file.to_str().ok_or("non-UTF8 input path")?.as_bytes());
        material.extend_from_slice(digest(content.as_bytes()).as_bytes());
    }
    Ok(digest(&material))
}

#[derive(Serialize)]
struct SnapshotRef<'a> {
    sources: &'a [SourceFile],
    contracts: &'a [ContractDef],
    declarations: Vec<(isize, String, String)>,
}
#[derive(Deserialize)]
struct Snapshot {
    sources: Vec<SourceFile>,
    contracts: Vec<ContractDef>,
    declarations: Vec<(isize, String, String)>,
}
impl Snapshot {
    fn into_project(self) -> Option<Project> {
        let mut project = Project { source_files: self.sources, contracts: self.contracts,
            contract_index: Default::default(),
            decl_table: DeclTable::from_snapshot(self.declarations)? };
        if project.contracts.iter().any(|c| c.span.file_index >= project.source_files.len()) {
            return None;
        }
        project.rebuild_index();
        Some(project)
    }
}
pub(crate) struct Store { base: PathBuf }
impl Store {
    pub(crate) fn from_env() -> Result<Self, String> {
        if std::env::var_os("ILOLD_DISABLE_PROJECT_CACHE").is_some() { return Err("disabled".into()); }
        let base = match std::env::var_os("ILOLD_PROJECT_CACHE_DIR") {
            Some(path) => PathBuf::from(path),
            None => PathBuf::from(std::env::var_os("HOME").ok_or("no home")?)
                .join(".cache").join("ilold").join("projects"),
        };
        Ok(Self { base })
    }

    /// Explicit store location, used for injection in tests.
    pub(crate) fn at(base: &Path) -> Self { Self { base: base.to_path_buf() } }

    /// Private per-project directory; derived snapshots live beside `project.json`.
    pub(crate) fn cache_dir(&self, root: &Path) -> Result<PathBuf, String> {
        let base = &self.base;
    // Reject symlinked cache ancestors (including override) instead of following them.
    for ancestor in base.ancestors().take_while(|p| p != &Path::new("")) {
        if let Ok(meta) = fs::symlink_metadata(ancestor) {
            if meta.file_type().is_symlink() { return Err("symlinked cache path".into()); }
        }
    }
    let dir = base.join(digest(root.to_string_lossy().as_bytes()));
    #[cfg(unix)] {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir)
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(&dir).map_err(|e| e.to_string())?.permissions().mode() & 0o077 != 0 {
            return Err("unsafe cache permissions".into());
        }
    }
        Ok(dir)
    }

    fn snapshot_path(&self, root: &Path) -> Result<PathBuf, String> {
        Ok(self.cache_dir(root)?.join("project.json"))
    }

    pub(super) fn load(&self, root: &Path, key: &str) -> Result<Project, String> {
        let bytes = read_envelope(&self.snapshot_path(root)?, VERSION, key, MAX_SNAPSHOT)?;
        serde_json::from_slice::<Snapshot>(&bytes).map_err(|e| e.to_string())?
            .into_project().ok_or_else(|| "invalid declaration or source index".into())
    }

    pub(super) fn publish(&self, root: &Path, key: &str, project: &Project) -> Result<(), String> {
        if project.decl_table.len() > MAX_FILES * 8 { return Err("declaration count limit".into()); }
        let snapshot = SnapshotRef { sources: &project.source_files, contracts: &project.contracts,
            declarations: project.decl_table.snapshot() };
        write_envelope(&self.snapshot_path(root)?, VERSION, key, MAX_SNAPSHOT, &snapshot)
    }
}

/// Reads a whole envelope payload after checking magic, format version, key, length and checksum.
pub(crate) fn read_envelope(path: &Path, version: &str, key: &str, max: u64) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > max { return Err("invalid snapshot size/type".into()); }
    if meta.len() < HEADER as u64 || key.len() != 64 { return Err("invalid snapshot header".into()); }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut header = [0u8; HEADER];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    let len = u64::from_be_bytes(header[136..144].try_into().map_err(|_| "invalid header")?);
    if &header[..8] != MAGIC || header[8..40] != Sha256::digest(version.as_bytes())[..]
        || &header[40..104] != key.as_bytes() || len != meta.len() - HEADER as u64 {
        return Err("stale or corrupt snapshot".into());
    }
    let mut bytes = Vec::with_capacity(len as usize);
    file.take(len + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 != len || header[104..136] != Sha256::digest(&bytes)[..] {
        return Err("truncated or corrupt snapshot".into());
    }
    Ok(bytes)
}

/// Serializes into a private temp file within `max` bytes, then atomically renames it over `path`.
pub(crate) fn write_envelope(path: &Path, version: &str, key: &str, max: u64,
    payload: &impl Serialize) -> Result<(), String> {
    if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid snapshot key".into());
    }
    let dir = path.parent().ok_or("missing cache dir")?;
    let temp = dir.join(format!(".snapshot-{}-{}.tmp", std::process::id(), NEXT_TEMP.fetch_add(1, Ordering::Relaxed)));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        file.write_all(&[0u8; HEADER]).map_err(|e| e.to_string())?;
        let (len, checksum) = {
            let mut writer = BoundedWriter { file: &mut file, count: 0, max, hash: Sha256::new() };
            serde_json::to_writer(&mut writer, payload).map_err(|e| e.to_string())?;
            (writer.count, writer.hash.finalize())
        };
        let mut header = [0u8; HEADER];
        header[..8].copy_from_slice(MAGIC);
        header[8..40].copy_from_slice(&Sha256::digest(version.as_bytes()));
        header[40..104].copy_from_slice(key.as_bytes());
        header[104..136].copy_from_slice(&checksum);
        header[136..144].copy_from_slice(&len.to_be_bytes());
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        file.write_all(&header).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temp, path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&temp); }
    result
}

/// The compile closure is never called on a validated hit. Errors are returned unchanged.
/// The flag reports whether the returned project is known to match `key`.
pub(super) fn reuse_or_compile<E>(
    store: Option<&Store>, root: Option<&Path>, key: Option<&str>,
    mut unchanged: impl FnMut() -> bool, compile: impl FnOnce() -> Result<Project, E>,
) -> Result<(Project, bool), E> {
    if let (Some(store), Some(root), Some(key)) = (store, root, key) {
        match store.load(root, key) {
            Ok(project) if unchanged() => {
                eprintln!("ilold project cache hit");
                return Ok((project, true));
            }
            Ok(_) => eprintln!("ilold project cache miss: inputs changed during lookup"),
            Err(reason) => eprintln!("ilold project cache miss: {reason}"),
        }
    } else { eprintln!("ilold project cache bypass: compiler, input or store unavailable"); }
    let project = compile()?;
    let mut validated = false;
    if let (Some(store), Some(root), Some(key)) = (store, root, key) {
        if unchanged() {
            validated = true;
            if let Err(reason) = store.publish(root, key, &project) {
                eprintln!("ilold project cache publish skipped: {reason}");
            }
        } else { eprintln!("ilold project cache publish skipped: inputs changed"); }
    }
    Ok((project, validated))
}

struct BoundedWriter<'a> { file: &'a mut File, count: u64, max: u64, hash: Sha256 }
impl Write for BoundedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if (bytes.len() as u64) > self.max - HEADER as u64 - self.count {
            return Err(std::io::Error::other("snapshot size limit"));
        }
        self.file.write_all(bytes)?;
        self.count += bytes.len() as u64;
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { self.file.flush() }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Store, PathBuf, Project) {
        let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("ilold-cache-test-{}-{}", std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&dir).unwrap();
        let mut declarations = DeclTable::default();
        declarations.insert(crate::model::decl_id::DeclId(7), "C".into(), "f".into());
        use crate::model::common::SourceSpan;
        use crate::model::contract::ContractKind;
        use crate::model::function::{FunctionDef, FunctionKind, Mutability, Visibility};
        let span = SourceSpan { file_index: 0, start_line: 1, start_col: 1, end_line: 1, end_col: 1 };
        let parent = ContractDef { name: "Parent".into(), kind: ContractKind::Contract,
            functions: vec![FunctionDef { name: "f".into(), kind: FunctionKind::Function,
                visibility: Visibility::Public, mutability: Mutability::Pure, modifiers: vec![],
                params: vec![], returns: vec![], body: None, is_virtual: false,
                is_override: false, span }], modifiers: vec![], state_vars: vec![],
            structs: vec![], enums: vec![], events: vec![], errors: vec![], inherits: vec![], span };
        let child = ContractDef { name: "Child".into(), inherits: vec!["Parent".into()],
            functions: vec![], ..parent.clone() };
        let mut project = Project { source_files: vec![SourceFile { path: "src/C.sol".into(), content: "contract C {}".into() }],
            contracts: vec![parent, child], contract_index: Default::default(), decl_table: declarations };
        project.rebuild_index();
        (Store { base: dir.join("private-cache") }, dir, project)
    }

    #[test]
    fn snapshot_roundtrip_corruption_size_and_new_key() {
        let (store, dir, project) = fixture();
        let key = digest(b"first");
        store.publish(&dir, &key, &project).unwrap();
        assert_eq!(store.load(&dir, &key).unwrap().decl_table.lookup(crate::model::decl_id::DeclId(7)).unwrap().function, "f");
        assert!(store.load(&dir, &digest(b"second")).is_err());
        let path = store.snapshot_path(&dir).unwrap();
        fs::write(&path, b"broken").unwrap();
        assert!(store.load(&dir, &key).is_err());
        store.publish(&dir, &key, &project).unwrap();
        let mut truncated = fs::read(&path).unwrap();
        truncated.pop();
        fs::write(&path, truncated).unwrap();
        assert!(store.load(&dir, &key).is_err());
        store.publish(&dir, &key, &project).unwrap();
        File::create(&path).unwrap().set_len(MAX_SNAPSHOT + 1).unwrap();
        assert!(store.load(&dir, &key).is_err());
    }

    #[test]
    fn injected_compiler_is_skipped_on_hit_and_errors_propagate() {
        let (store, dir, project) = fixture();
        let key = digest(b"compile");
        let calls = std::cell::Cell::new(0);
        let compile = || { calls.set(calls.get() + 1); Ok::<_, &'static str>(project.clone()) };
        assert!(reuse_or_compile(Some(&store), Some(&dir), Some(&key), || true, compile).unwrap().1);
        let (warm, validated) = reuse_or_compile(Some(&store), Some(&dir), Some(&key), || true, || {
            calls.set(calls.get() + 1);
            Err::<Project, _>("must not compile")
        }).unwrap();
        assert!(validated);
        assert_eq!(calls.get(), 1);
        assert!(!reuse_or_compile(None, Some(&dir), Some(&key), || true, || Ok::<_, ()>(project.clone())).unwrap().1);
        assert!(!reuse_or_compile(Some(&store), Some(&dir), Some(&digest(b"moving")), || false,
            || Ok::<_, ()>(project.clone())).unwrap().1);
        assert_eq!(warm.decl_table.lookup(crate::model::decl_id::DeclId(7)).unwrap().function,
            project.decl_table.lookup(crate::model::decl_id::DeclId(7)).unwrap().function);
        let cold_child = project.find_contract(Some("Child")).unwrap();
        let warm_child = warm.find_contract(Some("Child")).unwrap();
        assert_eq!(warm.resolve_function(warm_child, "f").unwrap().0.name,
            project.resolve_function(cold_child, "f").unwrap().0.name);
        let failure = reuse_or_compile(Some(&store), Some(&dir), Some(&digest(b"changed")),
            || true, || Err::<Project, _>("compile failed"));
        assert_eq!(failure.unwrap_err(), "compile failed");
    }

    #[test]
    fn concurrent_publications_are_whole_snapshots() {
        let (store, dir, project) = fixture();
        let mut workers = Vec::new();
        for i in 0..8 {
            let dir = dir.clone();
            let project = project.clone();
            let store = Store { base: store.base.clone() };
            workers.push(std::thread::spawn(move || store.publish(&dir, &digest(format!("key-{i}").as_bytes()), &project)));
        }
        for worker in workers { worker.join().unwrap().unwrap(); }
        let bytes = fs::read(store.snapshot_path(&dir).unwrap()).unwrap();
        assert!(bytes.starts_with(MAGIC));
        assert_eq!(bytes[104..136], Sha256::digest(&bytes[HEADER..])[..]);
        assert_eq!(serde_json::from_slice::<Snapshot>(&bytes[HEADER..]).unwrap().declarations.len(), 1);
    }

    #[test]
    fn bounded_fingerprint_tracks_sources_imports_settings_compiler_and_limits() {
        let (_, root, _) = fixture();
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join("vendor")).unwrap();
        fs::write(root.join("src/Main.sol"), "pragma solidity ^0.8.0; import 'ext/External.sol'; contract Main {}\n").unwrap();
        fs::write(root.join("vendor/External.sol"), "pragma solidity ^0.8.0; contract External {}\n").unwrap();
        let compiler_path = root.join("solc-fixture");
        fs::write(&compiler_path, b"compiler-one").unwrap();
        let compiler = Solc::new_with_version(&compiler_path, "0.8.20".parse().unwrap());
        let remapping = format!("ext/={}/", root.join("vendor").display()).parse().unwrap();
        let paths = ProjectPathsConfig::builder().root(&root).no_libs()
            .remappings(vec![remapping]).build::<SolcLanguage>().unwrap();
        let settings = serde_json::json!({"optimizer": false});
        let original = fingerprint(&root, &paths, &settings, "default", &compiler).unwrap();
        assert_eq!(original, fingerprint(&root, &paths, &settings, "default", &compiler).unwrap());
        assert_ne!(original, fingerprint(&root, &paths, &serde_json::json!({"optimizer": true}), "default", &compiler).unwrap());
        assert_ne!(original, fingerprint(&root, &paths, &settings, "other", &compiler).unwrap());
        fs::write(&compiler_path, b"compiler-two").unwrap();
        assert_ne!(original, fingerprint(&root, &paths, &settings, "default", &compiler).unwrap());
        fs::write(root.join("vendor/External.sol"), "pragma solidity ^0.8.0; contract External { uint value; }\n").unwrap();
        let changed = fingerprint(&root, &paths, &settings, "default", &compiler).unwrap();
        fs::write(root.join("src/Extra.sol"), "pragma solidity ^0.8.0; contract Extra {}\n").unwrap();
        assert_ne!(changed, fingerprint(&root, &paths, &settings, "default", &compiler).unwrap());
        fs::write(root.join("src/Main.sol"), "pragma solidity ^0.8.0; import '../outside.sol'; contract Main {}\n").unwrap();
        fs::write(root.join("outside.sol"), "pragma solidity ^0.8.0; contract Outside {}\n").unwrap();
        assert!(fingerprint(&root, &paths, &settings, "default", &compiler).is_err());
        fs::write(root.join("src/Main.sol"), "pragma solidity ^0.8.0; import 'ext/External.sol'; contract Main {}\n").unwrap();
        File::create(root.join("src/Large.sol")).unwrap().set_len(MAX_INPUT + 1).unwrap();
        assert!(fingerprint(&root, &paths, &settings, "default", &compiler).is_err());
    }

    #[test]
    fn declaration_pairs_roundtrip_and_reject_duplicates() {
        let mut declarations = DeclTable::default();
        declarations.insert(crate::model::decl_id::DeclId(12), "Parent".into(), "run".into());
        let pairs = declarations.snapshot();
        let encoded = serde_json::to_vec(&pairs).unwrap();
        let decoded = serde_json::from_slice(&encoded).unwrap();
        let restored = DeclTable::from_snapshot(decoded).unwrap();
        assert_eq!(restored.lookup(crate::model::decl_id::DeclId(12)).unwrap().function, "run");
        assert!(DeclTable::from_snapshot(vec![(1, "a".into(), "".into()), (1, "b".into(), "".into())]).is_none());
    }
}
