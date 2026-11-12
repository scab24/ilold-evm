//! Mapping keys of reads and writes on the API. Fixture: tests/fixtures/solc/keys.
use std::path::PathBuf;

use serde_json::{json, Value};

async fn start() -> (reqwest::Client, String) {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/keys");
    let (_state, port) = ilold_web::start_server(vec![fixture], 0, 2).await.unwrap();
    (reqwest::Client::new(), format!("http://127.0.0.1:{port}/api"))
}

async fn get(client: &reqwest::Client, url: String) -> Value {
    let res = client.get(&url).send().await.unwrap();
    assert!(res.status().is_success(), "{url}: {}", res.status());
    res.json().await.unwrap()
}

fn function<'a>(contract: &'a Value, name: &str) -> &'a Value {
    contract["functions"].as_array().unwrap().iter().find(|f| f["name"] == name).unwrap()
}

#[tokio::test]
async fn function_summaries_list_their_accesses_with_keys() {
    let (client, base) = start().await;
    let keys = get(&client, format!("{base}/contract/Keys")).await;
    let credit = &function(&keys, "credit")["accesses"];
    let writes: Vec<&Value> = credit.as_array().unwrap().iter().filter(|a| a["kind"] == "write").collect();
    assert_eq!(writes.len(), 1, "{credit}");
    assert_eq!(writes[0]["path"], "balances[account]");
    assert_eq!(writes[0]["keys"], json!([{"text": "account", "kind": "parameter", "ref": "Keys::credit"}]));

    let balances = keys["state_vars"].as_array().unwrap().iter().find(|v| v["name"] == "balances").unwrap();
    assert_eq!(balances["key_kinds"], json!(["msg_sender", "parameter", "state_var", "this"]));
    let total = keys["state_vars"].as_array().unwrap().iter().find(|v| v["name"] == "total").unwrap();
    assert_eq!(total["key_kinds"], json!([]));
}

#[tokio::test]
async fn cfg_blocks_list_the_accesses_they_make() {
    let (client, base) = start().await;
    let cfg = get(&client, format!("{base}/contract/Keys/move/cfg")).await;
    let reads: Vec<String> = cfg["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|n| n["data"]["accesses"].as_array().cloned().unwrap_or_default())
        .filter(|a| a["kind"] == "read")
        .map(|a| a["keys"][0]["text"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(reads, ["from", "to"]);
}

#[tokio::test]
async fn who_touches_reports_keys_per_function_and_transitive_writes() {
    let (client, base) = start().await;
    let res = client
        .post(format!("{base}/cmd"))
        .json(&json!({"contract": "Keys", "command": {"Who": {"variable": "balances"}}}))
        .send()
        .await
        .unwrap();
    let body: Value = res.json().await.unwrap();
    let accesses = body["VariableInfo"]["accesses"].as_array().unwrap_or_else(|| panic!("{body}"));
    let entry = |function: &str, kind: &str| {
        accesses.iter().find(|a| a["function"] == function && a["kind"] == kind).unwrap_or_else(|| panic!("{function} {kind}: {accesses:?}"))
    };
    assert_eq!(entry("deposit", "write")["keys"], json!([{"text": "msg.sender", "kind": "msg_sender", "ref": null}]));
    let later = entry("creditLater", "write");
    assert_eq!((&later["keys"], &later["keys_reason"], &later["key_sites"]), (&Value::Null, &json!("transitive"), &json!([])));
    assert_eq!(entry("move", "read")["keys_reason"], "several");
    assert_eq!(body["VariableInfo"]["writers"].as_array().unwrap().len(), 8, "the 1.0 lists stay");
}
