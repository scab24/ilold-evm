// The restart test uses a private ILOLD_PROJECT_CACHE_DIR, so it never touches the real cache.
use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::analysis_cache::load_or_build_with;
use ilold_core::model::common::SourceSpan;
use ilold_core::model::contract::{ContractDef, ContractKind};
use ilold_core::model::decl_id::DeclTable;
use ilold_core::model::function::{FunctionDef, FunctionKind, Mutability, Visibility};
use ilold_core::model::project::{Project, SourceFile};
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::parse::ProjectKey;
use sha2::{Digest, Sha256};

static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "ilold-analysis-{label}-{}-{}", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&dir).unwrap();
    dir
}

fn sorted<K: Ord + Debug, V: Debug>(map: &HashMap<K, V>) -> String {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    format!("{entries:?}")
}

/// Graph indices, edge order and the dependency index included.
fn canonical(analysis: &ProjectAnalysis) -> String {
    let deps = &analysis.dep_graph;
    let index: Vec<_> = deps.graph.node_indices()
        .map(|n| (deps.graph[n].name.clone(), deps.node(&deps.graph[n].name)))
        .collect();
    [sorted(&analysis.cfgs), sorted(&analysis.path_trees), sorted(&analysis.call_graphs),
        format!("{:?}", deps.graph), format!("{index:?}"), sorted(&analysis.sequence_trees),
        sorted(&analysis.sequence_analyses), sorted(&analysis.classifications)].join("\n")
}

fn small_project() -> Project {
    let span = SourceSpan { file_index: 0, start_line: 1, start_col: 1, end_line: 1, end_col: 1 };
    let function = |name: &str| FunctionDef { name: name.into(), key: name.into(),
        signature: format!("{name}()"), selector: None, id: None, kind: FunctionKind::Function,
        visibility: Visibility::Public, mutability: Mutability::NonPayable, modifiers: vec![],
        base_constructor_calls: vec![],
        params: vec![], returns: vec![], body: Some(vec![]), is_virtual: false, is_override: false, span };
    let base = ContractDef { name: "Base".into(), kind: ContractKind::Contract,
        functions: vec![function("ping"), function("pong")], modifiers: vec![], state_vars: vec![],
        structs: vec![], enums: vec![], events: vec![], errors: vec![], inherits: vec![],
        id: None, linearized_bases: vec![], span, qualified_id: "src/C.sol:Base".into(),
        key: "Base".into(), is_dependency: false, base_ids: vec![] };
    let child = ContractDef { name: "Child".into(), key: "Child".into(), qualified_id: "src/C.sol:Child".into(),
        inherits: vec!["Base".into()], functions: vec![function("run")], ..base.clone() };
    Project::new(vec![SourceFile { path: "src/C.sol".into(), content: String::new() }],
        vec![base, child], DeclTable::default())
}

fn key(root: &Path, seed: &str) -> ProjectKey {
    ProjectKey { root: root.to_path_buf(), key: format!("{:x}", Sha256::digest(seed.as_bytes())) }
}

fn analysis_files(base: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in fs::read_dir(base).into_iter().flatten().flatten() {
        for entry in fs::read_dir(dir.path()).into_iter().flatten().flatten() {
            if entry.file_name().to_string_lossy().starts_with("analysis-") { files.push(entry.path()); }
        }
    }
    files
}

#[test]
fn hit_skips_the_builder_and_equals_the_fresh_build() {
    let dir = temp_dir("hit");
    let base = dir.join("store");
    let project = small_project();
    let params = AnalysisParams::new(3);
    let key = key(&dir, "project");
    let builds = Cell::new(0);
    let build = || { builds.set(builds.get() + 1); ProjectAnalysis::build(&project, &params) };
    let cold = load_or_build_with(Some(&base), Some(&key), &params, build);
    let warm = load_or_build_with(Some(&base), Some(&key), &params, || {
        builds.set(builds.get() + 1);
        panic!("a validated hit must not rebuild the analysis");
    });
    assert_eq!(builds.get(), 1);
    assert_eq!(canonical(&cold), canonical(&warm));
    assert_eq!(canonical(&warm), canonical(&ProjectAnalysis::build(&project, &params)));
    assert!(warm.dep_graph.dependencies("Child").iter().any(|d| d.node.name == "Base" && d.edge.inherits));
}

#[test]
fn parameter_or_project_key_change_misses() {
    let dir = temp_dir("params");
    let base = dir.join("store");
    let project = small_project();
    let params = AnalysisParams::new(3);
    let key = key(&dir, "project");
    load_or_build_with(Some(&base), Some(&key), &params, || ProjectAnalysis::build(&project, &params));
    let changed = [AnalysisParams::new(2), {
        let mut pruning = AnalysisParams::new(3);
        pruning.pruning.max_paths = 7;
        pruning
    }];
    for other in &changed {
        let builds = Cell::new(0);
        load_or_build_with(Some(&base), Some(&key), other, || {
            builds.set(builds.get() + 1);
            ProjectAnalysis::build(&project, other)
        });
        assert_eq!(builds.get(), 1, "a parameter change must rebuild");
    }
    let builds = Cell::new(0);
    load_or_build_with(Some(&base), Some(&self::key(&dir, "edited")), &params, || {
        builds.set(builds.get() + 1);
        ProjectAnalysis::build(&project, &params)
    });
    assert_eq!(builds.get(), 1, "a project key change must rebuild");
}

#[test]
fn corrupt_or_truncated_snapshot_falls_back_to_a_fresh_build() {
    let dir = temp_dir("corrupt");
    let base = dir.join("store");
    let project = small_project();
    let params = AnalysisParams::new(3);
    let key = key(&dir, "project");
    let fresh = canonical(&ProjectAnalysis::build(&project, &params));
    for damage in [0usize, 1, 2] {
        load_or_build_with(Some(&base), Some(&key), &params, || ProjectAnalysis::build(&project, &params));
        let files = analysis_files(&base);
        assert_eq!(files.len(), 1, "one snapshot per parameter set");
        let mut bytes = fs::read(&files[0]).unwrap();
        match damage {
            0 => { bytes.pop(); }
            1 => { let last = bytes.len() - 1; bytes[last] ^= 0xff; }
            _ => bytes = b"broken".to_vec(),
        }
        fs::write(&files[0], bytes).unwrap();
        let builds = Cell::new(0);
        let recovered = load_or_build_with(Some(&base), Some(&key), &params, || {
            builds.set(builds.get() + 1);
            ProjectAnalysis::build(&project, &params)
        });
        assert_eq!(builds.get(), 1);
        assert_eq!(canonical(&recovered), fresh);
    }
}

#[test]
fn missing_parse_key_or_store_bypasses() {
    let dir = temp_dir("bypass");
    let base = dir.join("store");
    let project = small_project();
    let params = AnalysisParams::new(3);
    let builds = Cell::new(0);
    for _ in 0..2 {
        load_or_build_with(Some(&base), None, &params, || {
            builds.set(builds.get() + 1);
            ProjectAnalysis::build(&project, &params)
        });
    }
    load_or_build_with(None, Some(&key(&dir, "project")), &params, || {
        builds.set(builds.get() + 1);
        ProjectAnalysis::build(&project, &params)
    });
    assert_eq!(builds.get(), 3);
    assert!(!base.exists(), "a bypass must not publish");
}

const CHILD_ROOT: &str = "ILOLD_ANALYSIS_TEST_CHILD";

#[test]
fn analysis_child() {
    let Some(root) = std::env::var_os(CHILD_ROOT) else { return };
    let cache = std::env::var_os("ILOLD_PROJECT_CACHE_DIR")
        .expect("analysis child requires an explicit isolated ILOLD_PROJECT_CACHE_DIR");
    assert!(!cache.is_empty());
    let depth = std::env::var("ILOLD_ANALYSIS_TEST_DEPTH").unwrap().parse().unwrap();
    let mut params = AnalysisParams::new(depth);
    params.pruning.max_paths = std::env::var("ILOLD_ANALYSIS_TEST_MAX_PATHS").unwrap().parse().unwrap();
    let (project, key) = SolcFrontend.parse_project_keyed(Path::new(&root)).unwrap();
    let analysis = ProjectAnalysis::load_or_build(&project, key.as_ref(), &params);
    assert!(analysis.sequence_analyses.contains_key("Staking"));
    println!("ANALYSIS_DIGEST={:x}", Sha256::digest(canonical(&analysis).as_bytes()));
}

fn child(root: &Path, cache: &Path, depth: usize, max_paths: usize, disable: bool) -> (String, String) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.arg("--exact").arg("analysis_child").arg("--nocapture")
        .env(CHILD_ROOT, root).env("ILOLD_PROJECT_CACHE_DIR", cache)
        .env("ILOLD_ANALYSIS_TEST_DEPTH", depth.to_string())
        .env("ILOLD_ANALYSIS_TEST_MAX_PATHS", max_paths.to_string())
        .env_remove("FOUNDRY_PROFILE").env_remove("ILOLD_DISABLE_PROJECT_CACHE");
    if disable { command.env("ILOLD_DISABLE_PROJECT_CACHE", "1"); }
    let output = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{stdout}\n{stderr}");
    let digest = stdout.lines().find_map(|l| l.strip_prefix("ANALYSIS_DIGEST=")).unwrap_or_default();
    // libtest prints the test name on the same line under --nocapture.
    let digest = digest.split_whitespace().next().unwrap_or_default().to_string();
    assert_eq!(digest.len(), 64, "{stdout}");
    (digest, stderr)
}

#[test]
fn restart_reuses_analysis_and_parameters_invalidate_only_the_analysis_layer() {
    let root = temp_dir("restart");
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("foundry.toml"),
        "[profile.default]\nsrc = 'src'\nlibs = []\nsolc_version = '0.8.28'\nauto_detect_remappings = false\n").unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/staking/src/staking.sol");
    fs::copy(fixture, root.join("src/staking.sol")).unwrap();
    fs::write(root.join("src/Vault.sol"), "pragma solidity ^0.8.20; import './staking.sol';\n\
        contract Vault is Staking { Staking public peer; constructor() Staking(address(0), address(0)) {}\n\
        function poke() external { peer.claimRewards(); } }\n").unwrap();
    let cache = root.join("private-cache");

    let (cold, log) = child(&root, &cache, 3, 10_000, false);
    assert!(log.contains("ilold project cache miss") && log.contains("ilold analysis cache miss"), "{log}");
    let (warm, log) = child(&root, &cache, 3, 10_000, false);
    assert!(log.contains("ilold project cache hit") && log.contains("ilold analysis cache hit"), "{log}");
    assert_eq!(cold, warm, "loaded analysis must equal the cold build");

    let (depth, log) = child(&root, &cache, 2, 10_000, false);
    assert!(log.contains("ilold project cache hit") && log.contains("ilold analysis cache miss"), "{log}");
    assert_ne!(depth, cold, "sequence depth changes the derived analysis");
    let (depth_warm, log) = child(&root, &cache, 2, 10_000, false);
    assert!(log.contains("ilold analysis cache hit"), "{log}");
    assert_eq!(depth, depth_warm);

    let (pruned, log) = child(&root, &cache, 3, 2, false);
    assert!(log.contains("ilold project cache hit") && log.contains("ilold analysis cache miss"), "{log}");
    assert_ne!(pruned, cold, "path pruning changes the derived analysis");

    for file in analysis_files(&cache) {
        let mut bytes = fs::read(&file).unwrap();
        bytes.truncate(bytes.len() / 2);
        fs::write(&file, bytes).unwrap();
    }
    let (recovered, log) = child(&root, &cache, 3, 10_000, false);
    assert!(log.contains("ilold project cache hit") && log.contains("ilold analysis cache miss"), "{log}");
    assert_eq!(recovered, cold, "a corrupt snapshot falls back to an equal fresh build");

    let (bypassed, log) = child(&root, &cache, 3, 10_000, true);
    assert!(log.contains("ilold project cache bypass") && log.contains("ilold analysis cache bypass"), "{log}");
    assert!(!log.contains("ilold analysis cache hit"), "{log}");
    assert_eq!(bypassed, cold, "a fresh build without caches equals the cached analysis");
}
