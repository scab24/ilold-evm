//! Per-contract session isolation: every contract owns its own ScenarioStore,
//! so commands naming another contract never drop scenarios, forks or steps.
//! Uses tests/fixtures/solc/cross (LendingPool, Vault, BasePool, IPool, SafeMath).

use std::path::PathBuf;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio_tungstenite::connect_async;

fn cross() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("tests/fixtures/solc/cross")
}

async fn start() -> (reqwest::Client, u16) {
    let (_state, port) = ilold_web::start_server(vec![cross()], 0, 2).await.unwrap();
    (reqwest::Client::new(), port)
}

async fn cmd(client: &reqwest::Client, port: u16, contract: &str, command: Value) -> Value {
    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&json!({ "contract": contract, "command": command }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success(), "POST /api/cmd returned {}", res.status());
    res.json().await.unwrap()
}

async fn get(client: &reqwest::Client, port: u16, path: &str) -> (u16, String) {
    let res = client.get(format!("http://127.0.0.1:{port}{path}")).send().await.unwrap();
    let status = res.status().as_u16();
    (status, res.text().await.unwrap())
}

async fn get_json(client: &reqwest::Client, port: u16, path: &str) -> Value {
    let (status, body) = get(client, port, path).await;
    assert_eq!(status, 200, "GET {path}: {body}");
    serde_json::from_str(&body).unwrap()
}

fn call(func: &str) -> Value {
    json!({ "Call": { "func": func } })
}

fn steps_of(all: &Value, scenario: &str) -> Vec<String> {
    all["scenarios"].as_array().unwrap().iter()
        .find(|s| s["name"] == scenario)
        .unwrap_or_else(|| panic!("scenario {scenario} missing in {all}"))["steps"]
        .as_array().unwrap().iter()
        .map(|s| s["function"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn switching_contract_keeps_scenarios_of_both_contracts() {
    let (client, port) = start().await;

    cmd(&client, port, "LendingPool", call("supply")).await;
    cmd(&client, port, "LendingPool", json!({ "Scenario": { "sub": { "Fork": { "name": "alt" } } } })).await;
    cmd(&client, port, "LendingPool", json!({ "Scenario": { "sub": { "Switch": { "name": "alt" } } } })).await;
    cmd(&client, port, "LendingPool", call("withdraw")).await;

    // Another contract gets its own store instead of replacing LendingPool's.
    let added = cmd(&client, port, "Vault", call("depositVia")).await;
    assert_eq!(added["StepAdded"]["step_index"], 0, "{added}");

    let pool = get_json(&client, port, "/api/scenarios/all?contract=LendingPool").await;
    assert_eq!(pool["contract"], "LendingPool");
    assert_eq!(pool["active"], "alt");
    assert_eq!(steps_of(&pool, "main"), ["supply"]);
    assert_eq!(steps_of(&pool, "alt"), ["supply", "withdraw"]);
    let alt = pool["scenarios"].as_array().unwrap().iter().find(|s| s["name"] == "alt").unwrap();
    assert_eq!(alt["forked_from"]["scenario"], "main");

    let vault = get_json(&client, port, "/api/scenarios/all?contract=Vault").await;
    assert_eq!(vault["contract"], "Vault");
    assert_eq!(vault["active"], "main");
    assert_eq!(steps_of(&vault, "main"), ["depositVia"]);

    // Switching back restores the previous contract untouched.
    let view = cmd(&client, port, "LendingPool", json!("Session")).await;
    assert_eq!(view["SessionView"]["contract"], "LendingPool");
    assert_eq!(view["SessionView"]["steps"], json!(["supply", "withdraw"]));
    let added = cmd(&client, port, "LendingPool", call("push")).await;
    assert_eq!(added["StepAdded"]["step_index"], 2, "{added}");

    let vault = get_json(&client, port, "/api/scenarios/all?contract=Vault").await;
    assert_eq!(steps_of(&vault, "main"), ["depositVia"]);
}

#[tokio::test]
async fn session_gets_resolve_explicit_contract_and_default_to_last_used() {
    let (client, port) = start().await;

    cmd(&client, port, "LendingPool", call("supply")).await;
    cmd(&client, port, "Vault", call("depositVia")).await;

    // Explicit contract wins over the last used one.
    let scenarios = get_json(&client, port, "/api/scenarios?contract=LendingPool").await;
    assert_eq!(scenarios[0]["step_count"], 1);
    let narrative = get_json(&client, port, "/api/session/step/0/narrative?contract=LendingPool").await;
    assert_eq!(narrative["name"], "supply", "{narrative}");
    let trace = get(&client, port, "/api/session/step/0/trace?contract=LendingPool").await;
    assert_eq!(trace.0, 200, "{}", trace.1);
    let state = get_json(&client, port, "/api/session/state?contract=LendingPool").await;
    assert!(state.to_string().contains("balances"), "{state}");
    let timeline = get_json(&client, port, "/api/session/timeline/balances?contract=LendingPool").await;
    assert!(!timeline["state_entries"].as_array().unwrap().is_empty(), "{timeline}");
    let slice = get(&client, port, "/api/session/slice/supply/balances?contract=LendingPool").await;
    assert_eq!(slice.0, 200, "{}", slice.1);

    // Without a contract, the last contract named by /api/cmd is used (Vault).
    let narrative = get_json(&client, port, "/api/session/step/0/narrative").await;
    assert_eq!(narrative["name"], "depositVia", "{narrative}");
    let all = get_json(&client, port, "/api/scenarios/all").await;
    assert_eq!(all["contract"], "Vault");

    // Reads never change ownership: Vault stays the last used contract.
    get_json(&client, port, "/api/session/state?contract=LendingPool").await;
    let all = get_json(&client, port, "/api/scenarios/all").await;
    assert_eq!(all["contract"], "Vault");

    // Unknown contracts are rejected instead of silently reading another session.
    let (status, body) = get(&client, port, "/api/session/state?contract=Nope").await;
    assert_eq!(status, 404, "{body}");
    assert!(body.contains("Nope"), "{body}");
}

#[tokio::test]
async fn broadcasts_carry_the_contract() {
    let (client, port) = start().await;
    let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws")).await.unwrap();

    cmd(&client, port, "Vault", call("depositVia")).await;
    cmd(&client, port, "LendingPool", json!({ "Scenario": { "sub": { "New": { "name": "alt" } } } })).await;

    let mut seen = Vec::new();
    for _ in 0..2 {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await.unwrap().unwrap().unwrap();
        let payload: Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
        seen.push((payload["type"].as_str().unwrap().to_string(), payload["contract"].clone()));
    }
    assert_eq!(seen[0], ("session_add_node".to_string(), json!("Vault")));
    assert_eq!(seen[1], ("scenario_created".to_string(), json!("LendingPool")));

    // Switching contracts no longer emits a destructive clear for the previous one.
    let next = tokio::time::timeout(Duration::from_millis(300), ws.next()).await;
    assert!(next.is_err(), "unexpected extra message: {next:?}");
}

#[tokio::test]
async fn default_contract_is_shared_and_never_unknown() {
    let (state, port) = ilold_web::start_server(vec![cross()], 0, 2).await.unwrap();
    let client = reqwest::Client::new();

    let expected = state.project.default_contract().expect("cross has contracts").name.clone();
    assert_eq!(expected, "LendingPool");

    let all = get_json(&client, port, "/api/scenarios/all").await;
    assert_eq!(all["contract"], expected.as_str());

    // The attached REPL derives the same rule from /api/project.
    let project = get_json(&client, port, "/api/project").await;
    let candidates: Vec<(String, ilold_core::model::contract::ContractKind, Vec<String>)> = project["contracts"]
        .as_array().unwrap().iter()
        .map(|c| (
            c["name"].as_str().unwrap().to_string(),
            serde_json::from_value(c["kind"].clone()).unwrap(),
            serde_json::from_value(c["inherits"].clone()).unwrap(),
        ))
        .collect();
    let picked = ilold_core::model::project::pick_default_contract(
        candidates.iter().map(|(n, k, i)| (n.as_str(), *k, i.as_slice())),
    );
    assert_eq!(picked, Some(expected.as_str()));
}

#[tokio::test]
async fn lagged_client_gets_a_resync_message() {
    use ilold_core::exploration::commands::CanvasPatch;
    use ilold_web::state::SessionBroadcast;

    let (tx, mut rx) = tokio::sync::broadcast::channel::<SessionBroadcast>(2);
    for _ in 0..5 {
        tx.send(SessionBroadcast {
            contract: "Vault".into(),
            patch: CanvasPatch::ClearAll { scenario: "main".into() },
        }).unwrap();
    }

    let first = ilold_web::ws::handler::next_session_message(&mut rx).await.unwrap();
    let first: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(first["type"], "session_resync", "{first}");

    let next = ilold_web::ws::handler::next_session_message(&mut rx).await.unwrap();
    let next: Value = serde_json::from_str(&next).unwrap();
    assert_eq!(next["type"], "session_clear");
    assert_eq!(next["contract"], "Vault");

    drop(tx);
    while ilold_web::ws::handler::next_session_message(&mut rx).await.is_some() {}
}

#[tokio::test]
async fn pty_rejects_unknown_contract_before_spawning() {
    let (_client, port) = start().await;
    let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/pty?contract=Nope")).await.unwrap();
    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await.unwrap().unwrap().unwrap();
    let text = msg.into_text().unwrap();
    assert!(text.contains("Contract 'Nope' not found"), "{text}");
}
