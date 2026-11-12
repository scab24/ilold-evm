//! Try statements on the API. Fixture: tests/fixtures/solc/trycatch.
use std::path::PathBuf;

use serde_json::Value;

async fn start() -> (reqwest::Client, String) {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/trycatch");
    let (_state, port) = ilold_web::start_server(vec![fixture], 0, 2).await.unwrap();
    (reqwest::Client::new(), format!("http://127.0.0.1:{port}/api"))
}

async fn get(client: &reqwest::Client, url: String) -> Value {
    let res = client.get(&url).send().await.unwrap();
    assert!(res.status().is_success(), "{url}: {}", res.status());
    res.json().await.unwrap()
}

#[tokio::test]
async fn the_cfg_returns_each_try_with_the_blocks_of_its_branches() {
    let (client, base) = start().await;
    let cfg = get(&client, format!("{base}/contract/TryCatch/swallow/cfg")).await;
    let tries = cfg["try_catches"].as_array().unwrap();
    assert_eq!(tries.len(), 1, "{cfg}");
    let fact = &tries[0];
    assert_eq!((&fact["guarded_call"]["text"], &fact["catch_clauses"][0]["kind"]), (&Value::from("this.step"), &Value::from("all")));
    assert_eq!(fact["catch_clauses"][0]["empty"], true);

    let block = |id: &Value| format!("b{id}");
    let edge = |target: &Value| {
        cfg["edges"].as_array().unwrap().iter().find(|e| e["data"]["source"] == block(&fact["block"]) && e["data"]["target"] == block(target)).cloned()
    };
    assert!(edge(&fact["success_block"]).is_some_and(|e| e["data"]["kind"] == "ExternalCallSuccess"), "{cfg}");
    assert!(edge(&fact["catch_clauses"][0]["block"]).is_some(), "{cfg}");

    let clauses = get(&client, format!("{base}/contract/TryCatch/clauses/cfg")).await;
    let kinds: Vec<&str> = clauses["try_catches"][0]["catch_clauses"].as_array().unwrap().iter().map(|c| c["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["Error", "Panic", "bytes"]);

    let step = get(&client, format!("{base}/contract/TryCatch/step/cfg")).await;
    assert!(step.get("try_catches").is_none(), "a function without try keeps the old shape: {step}");
}

#[tokio::test]
async fn function_summaries_carry_their_try_statements() {
    let (client, base) = start().await;
    let contract = get(&client, format!("{base}/contract/TryCatch")).await;
    let function = |name: &str| contract["functions"].as_array().unwrap().iter().find(|f| f["name"] == name).unwrap().clone();
    assert_eq!(function("swallow")["try_catches"].as_array().map(Vec::len), Some(1));
    assert_eq!(function("guarded")["try_catches"][0]["modifier"], "pinged");
    assert!(function("step").get("try_catches").is_none());
}
