//! Contract links and dispatch candidates on the API. Fixture: tests/fixtures/solc/dispatch.
use std::path::PathBuf;

use serde_json::{json, Value};

async fn start() -> (reqwest::Client, String) {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/dispatch");
    let (_state, port) = ilold_web::start_server(vec![fixture], 0, 2).await.unwrap();
    (reqwest::Client::new(), format!("http://127.0.0.1:{port}/api"))
}

async fn get(client: &reqwest::Client, url: String) -> Value {
    let res = client.get(&url).send().await.unwrap();
    assert!(res.status().is_success(), "{url}: {}", res.status());
    res.json().await.unwrap()
}

fn pairs(links: &Value) -> Vec<(String, String, String, String)> {
    let field = |l: &Value, f: &str| l[f].as_str().unwrap_or_default().to_string();
    links.as_array().unwrap().iter().map(|l| (field(l, "from_contract"), field(l, "to_contract"), field(l, "basis"), field(l, "tag"))).collect()
}

#[tokio::test]
async fn a_contract_lists_the_contracts_it_links_to_and_from() {
    let (client, base) = start().await;
    let router = get(&client, format!("{base}/contract/Router")).await;
    let out: Vec<(String, String)> = pairs(&router["outgoing_links"]).into_iter().map(|(_, to, basis, _)| (to, basis)).collect();
    assert_eq!(
        out,
        [("HookA", "inherits_interface"), ("HookB", "inherits_interface"), ("Registry", "selector_match"), ("Router", "self"), ("Vault", "inherits_interface")]
            .map(|(a, b)| (a.to_string(), b.to_string()))
    );
    assert_eq!(router["outgoing_links"][4]["via_state_vars"], json!(["vault"]));
    assert_eq!(router["outgoing_links"][4]["calls"][0]["function"], "Router::run");

    let vault = get(&client, format!("{base}/contract/Vault")).await;
    assert_eq!(pairs(&vault["incoming_links"]), [("Router".into(), "Vault".into(), "inherits_interface".into(), "INFERRED".into())]);
    assert_eq!(vault["outgoing_links"], json!([]));
}

#[tokio::test]
async fn call_graph_edges_carry_candidates_and_the_address_assignments() {
    let (client, base) = start().await;
    let graph = get(&client, format!("{base}/contract/Router/callgraph")).await;
    let edge = |target: &str| -> Value {
        graph["edges"].as_array().unwrap().iter().find(|e| e["data"]["source"] == "Router::run" && e["data"]["target"] == target).unwrap()["data"].clone()
    };
    let hook = edge("IHook::onCall");
    let contracts: Vec<&str> = hook["dispatch_candidates"].as_array().unwrap().iter().map(|c| c["contract"].as_str().unwrap()).collect();
    assert_eq!(contracts, ["HookA", "HookB"]);
    assert_eq!(hook["dispatch_candidates"][0]["tag"], "AMBIGUOUS");
    let kinds: Vec<&str> = hook["assigned_at"].as_array().unwrap().iter().map(|a| a["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["constructor_param", "setter"]);
    assert_eq!(hook["assigned_at"][1]["function"], "Router::setHook");
    assert_eq!(edge("IOrphan::ping")["unresolved_reason"], "no_implementation");
    assert_eq!(edge("Router::selfValue")["dispatch_candidates"][0]["basis"], "self");
    let internal = graph["edges"].as_array().unwrap().iter().find(|e| e["data"]["kind"] == "Internal");
    assert!(internal.is_none_or(|e| e["data"].get("dispatch_candidates").is_none()), "{internal:?}");
}

#[tokio::test]
async fn the_project_map_and_the_dependency_graph_read_the_same_links() {
    let (client, base) = start().await;
    let map = get(&client, format!("{base}/project/map")).await;
    let rows: Vec<&Value> = map["relationships"].as_array().unwrap().iter().filter(|r| r["kind"] == "ContractLink").collect();
    let from_map = pairs(&Value::from(rows.into_iter().cloned().collect::<Vec<_>>()));
    let call = map["relationships"].as_array().unwrap().iter().find(|r| r["to_function"] == "deposit" && r["kind"] == "External").unwrap();
    assert_eq!(call["dispatch_candidates"][0]["function"], "Vault::deposit");

    let deps = get(&client, format!("{base}/project/depgraph")).await;
    let edges: Vec<Value> = deps["edges"].as_array().unwrap().iter().filter(|e| !e["data"]["link"].is_null()).map(|e| e["data"]["link"].clone()).collect();
    assert_eq!(pairs(&Value::from(edges.clone())), from_map);
    assert_eq!(from_map.len(), 5);
    let vault = deps["edges"].as_array().unwrap().iter().find(|e| e["data"]["target"] == "Vault").unwrap();
    assert!(vault["data"]["kinds"].as_array().unwrap().contains(&json!("dispatches")), "{vault}");
}

const OVERRIDES: &str = "pragma solidity 0.8.28;
interface IHook { function ping() external; function pong() external; }
contract Hook is IHook { function ping() external {} function pong() external {} }
abstract contract Base {
    IHook public hook;
    constructor(IHook h) { hook = h; hook.pong(); }
    modifier gate() virtual { hook.ping(); _; }
    function run() public gate { hook.pong(); }
}
contract Child is Base {
    constructor(IHook h) Base(h) {}
    modifier gate() override { _; }
}
";

#[tokio::test]
async fn call_graph_assignments_follow_the_code_the_contract_runs() {
    let root = std::env::temp_dir().canonicalize().unwrap().join(format!("ilold-web-overrides-{}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("foundry.toml"), "[profile.default]\nsrc = 'src'\nlibs = []\nsolc_version = '0.8.28'\nauto_detect_remappings = false\n").unwrap();
    std::fs::write(root.join("src/Overrides.sol"), OVERRIDES).unwrap();
    let (_state, port) = ilold_web::start_server(vec![root.clone()], 0, 2).await.unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    let (client, base) = (reqwest::Client::new(), format!("http://127.0.0.1:{port}/api"));
    let sites = |graph: &Value, source: &str, target: &str| -> Vec<String> {
        let edge = graph["edges"].as_array().unwrap().iter().find(|e| e["data"]["source"] == source && e["data"]["target"] == target).unwrap();
        edge["data"]["assigned_at"].as_array().unwrap().iter().map(|a| a["function"].as_str().unwrap().to_string()).collect()
    };
    let child = get(&client, format!("{base}/contract/Child/callgraph")).await;
    assert_eq!(sites(&child, "Base::constructor", "IHook::pong"), ["Base::constructor"], "a base constructor runs in the deploying contract");
    assert_eq!(sites(&child, "Base::run", "IHook::pong"), ["Base::constructor"]);
    assert!(sites(&child, "Base::run", "IHook::ping").is_empty(), "the overridden modifier's call does not run in Child");
    let own = get(&client, format!("{base}/contract/Base/callgraph")).await;
    assert_eq!(sites(&own, "Base::run", "IHook::ping"), ["Base::constructor"]);
}
