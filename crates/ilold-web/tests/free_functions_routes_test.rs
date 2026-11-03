//! Free-function containers are hidden from listings but resolvable. Fixture: solc/state_flow.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::Value;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/state_flow")
}

async fn get(client: &reqwest::Client, url: String) -> (reqwest::StatusCode, Value) {
    let res = client.get(url).send().await.unwrap();
    let status = res.status();
    let text = res.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

fn names(contracts: &Value) -> BTreeSet<String> {
    contracts.as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect()
}

fn node_ids(graph: &Value) -> BTreeSet<String> {
    graph["nodes"].as_array().unwrap().iter().map(|n| n["data"]["id"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn free_function_containers_are_hidden_but_resolvable() {
    let (_state, port) = ilold_web::start_server(vec![fixture()], 0, 2).await.unwrap();
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}/api");

    let (_, project) = get(&client, format!("{base}/project")).await;
    let listed = names(&project["contracts"]);
    assert!(listed.contains("FreeUser") && !listed.contains("FreeFunctions"), "{listed:?}");
    let (_, all) = get(&client, format!("{base}/project?include_dependencies=true")).await;
    assert!(names(&all["contracts"]).contains("FreeFunctions"));

    let (_, graph) = get(&client, format!("{base}/project/depgraph")).await;
    let ids = node_ids(&graph);
    assert!(ids.contains("FreeUser") && !ids.contains("FreeFunctions"), "{ids:?}");
    let (_, full) = get(&client, format!("{base}/project/depgraph?include_dependencies=true")).await;
    assert!(node_ids(&full).contains("FreeFunctions"));

    let (status, free) = get(&client, format!("{base}/contract/FreeFunctions")).await;
    assert!(status.is_success(), "{free}");
    assert_eq!(free["functions"].as_array().unwrap().len(), 1);
}
