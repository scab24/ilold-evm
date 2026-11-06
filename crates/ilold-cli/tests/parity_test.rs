//! One fixture through the export, the web API and the MCP tools: each fact must read the same everywhere.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use ilold_evm_mcp::{dispatch, IloldClient};
use serde_json::Value;
use tokio::sync::Mutex;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc").join(name).canonicalize().unwrap()
}

/// The export's records, envelope first.
fn export(project: &Path) -> Vec<Value> {
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "ilold-parity-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("graph.jsonl");
    let run = Command::new(env!("CARGO_BIN_EXE_ilold"))
        .args(["graph", "--json"])
        .arg(&out)
        .arg(project)
        .env("ILOLD_PROJECT_CACHE_DIR", dir.join("cache"))
        .env_remove("ILOLD_DISABLE_PROJECT_CACHE")
        .env_remove("FOUNDRY_PROFILE")
        .output()
        .expect("run ilold graph");
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    let text = std::fs::read_to_string(&out).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

struct Surfaces {
    client: IloldClient,
    records: Vec<Value>,
}

impl Surfaces {
    async fn of(name: &str) -> Self {
        let project = fixture(name);
        let (_state, port) = ilold_web::start_server(vec![project.clone()], 0, 2).await.unwrap();
        Self { client: IloldClient::new(format!("http://127.0.0.1:{port}")), records: export(&project) }
    }

    fn records<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Value> {
        self.records.iter().filter(move |r| r["kind"] == kind)
    }

    async fn web(&self, path: &str) -> Value {
        self.client.get(path).await.unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    async fn mcp(&self, tool: &str, arguments: Value) -> Value {
        let result = dispatch(&self.client, &Mutex::new(None), tool, Some(&arguments)).await;
        assert!(!result.is_error.unwrap_or(false), "{tool}: {:?}", result.content);
        result.structured_content.unwrap_or_else(|| panic!("{tool} returned no JSON"))
    }
}

fn names(values: &Value, field: &str) -> BTreeSet<String> {
    values.as_array().unwrap().iter().map(|v| v[field].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn contracts_are_the_same_in_every_surface() {
    let s = Surfaces::of("cross").await;
    let exported: BTreeSet<String> =
        s.records("contract").filter(|c| c["is_dependency"] == false).map(|c| c["name"].as_str().unwrap().to_string()).collect();
    let web = names(&s.web("/api/project").await["contracts"], "name");
    let mcp = names(&s.mcp("ilold_project_overview", serde_json::json!({})).await["contracts"], "name");
    assert!(!exported.is_empty());
    assert_eq!(exported, web);
    assert_eq!(web, mcp);
}
