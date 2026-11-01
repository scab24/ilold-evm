// Each startup runs in a child process with a private ILOLD_PROJECT_CACHE_DIR.
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

use ilold_core::exploration::session::ExplorationSession;
use ilold_web::state::{AppState, Annotation, AnnotationSeverity, AnnotationStatus};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap().join("tests/fixtures/staking")
}

fn sorted<K: Ord + Debug, V: Debug>(map: &HashMap<K, V>) -> String {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    format!("{entries:?}")
}

fn derived(state: &AppState) -> String {
    [sorted(&state.cfgs), sorted(&state.path_trees), sorted(&state.call_graphs),
        format!("{:?}", state.dep_graph.graph), sorted(&state.sequence_trees),
        sorted(&state.sequence_analyses), sorted(&state.classifications)].join("\n")
}

#[test]
fn startup_child() {
    if std::env::var_os("ILOLD_WEB_CACHE_TEST_CHILD").is_none() { return; }
    assert!(std::env::var_os("ILOLD_PROJECT_CACHE_DIR").is_some_and(|dir| !dir.is_empty()));
    let state = AppState::from_paths(&[fixture()], 2, ([127, 0, 0, 1], 0).into(), fixture()).unwrap();
    let scenarios = state.scenarios.read().unwrap().names().to_vec();
    let annotations = state.annotations.read().unwrap().len();
    let derived = derived(&state);
    let mut hasher = DefaultHasher::new();
    derived.hash(&mut hasher);
    println!("STARTUP scenarios={scenarios:?} annotations={annotations} derived={}:{:x} end",
        derived.len(), hasher.finish());
    // Mutate runtime state before exit; a later startup must not observe it.
    state.annotations.write().unwrap().push(Annotation { id: "a1".into(), contract: "Staking".into(),
        function: None, path_id: None, block_id: None, text: "note".into(),
        severity: AnnotationSeverity::Info, status: AnnotationStatus::Open });
    state.scenarios.write().unwrap().insert("extra", ExplorationSession::new("Staking", "ilold"));
}

fn startup(cache: &Path) -> (String, String) {
    let output = Command::new(std::env::current_exe().unwrap())
        .arg("--exact").arg("startup_child").arg("--nocapture")
        .env("ILOLD_WEB_CACHE_TEST_CHILD", "1").env("ILOLD_PROJECT_CACHE_DIR", cache)
        .env_remove("FOUNDRY_PROFILE").env_remove("ILOLD_DISABLE_PROJECT_CACHE")
        .output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{stdout}\n{stderr}");
    let line = stdout.lines().find_map(|l| l.split_once("STARTUP ").map(|(_, rest)| rest))
        .and_then(|rest| rest.split_once(" end").map(|(line, _)| line.to_string()))
        .unwrap_or_else(|| panic!("{stdout}"));
    (line, stderr)
}

#[test]
fn restart_reuses_analysis_with_fresh_sessions() {
    let cache = std::env::temp_dir().canonicalize().unwrap()
        .join(format!("ilold-web-cache-{}", std::process::id())).join("projects");
    let (cold, log) = startup(&cache);
    assert!(log.contains("ilold analysis cache miss"), "{log}");
    let (warm, log) = startup(&cache);
    assert!(log.contains("ilold project cache hit") && log.contains("ilold analysis cache hit"), "{log}");
    assert_eq!(cold, warm);
    assert!(warm.starts_with("scenarios=[\"main\"] annotations=0 "), "{warm}");
}
