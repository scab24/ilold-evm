//! Phase candidates on the contract route. Fixture: tests/fixtures/solc/phases.
use std::path::PathBuf;

use serde_json::{json, Value};

async fn contract(port: u16, name: &str) -> Value {
    let res = reqwest::get(format!("http://127.0.0.1:{port}/api/contract/{name}")).await.unwrap();
    assert!(res.status().is_success(), "{}", res.status());
    res.json().await.unwrap()
}

#[tokio::test]
async fn the_contract_lists_its_phase_candidates_with_inherited_functions_counted() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/phases");
    let (_state, port) = ilold_web::start_server(vec![fixture], 0, 2).await.unwrap();
    let sale = contract(port, "Sale").await;
    let vars: Vec<&Value> = sale["phase_candidates"].as_array().unwrap().iter().map(|p| &p["state_var"]).collect();
    assert_eq!(vars, [&json!("Pausable::paused"), &json!("Sale::round"), &json!("Sale::state")]);
    let state = &sale["phase_candidates"][2];
    assert_eq!((&state["writers"], &state["tag"], &state["in_context"]), (&json!(["Sale::close", "Sale::open"]), &json!("INFERRED"), &json!("Sale")));
    let vault = contract(port, "Vault").await;
    assert_eq!(vault["phase_candidates"][0]["guarded_functions"], json!(["Vault::deposit", "Vault::withdraw"]));
    assert_eq!(contract(port, "Pausable").await["phase_candidates"], json!([]));
}

#[tokio::test]
async fn the_function_narrative_lists_the_phases_that_gate_it_or_that_it_sets() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/phases");
    let (_state, port) = ilold_web::start_server(vec![fixture], 0, 2).await.unwrap();
    let narrative = |func: &'static str| async move {
        let res = reqwest::get(format!("http://127.0.0.1:{port}/api/session/function/Sale/{func}")).await.unwrap();
        assert!(res.status().is_success(), "{}", res.status());
        res.json::<Value>().await.unwrap()
    };
    let vars = |narrative: &Value| -> Vec<Value> { narrative["phase_candidates"].as_array().unwrap().iter().map(|p| p["state_var"].clone()).collect() };
    assert_eq!(vars(&narrative("buy").await), [json!("Pausable::paused"), json!("Sale::round"), json!("Sale::state")]);
    assert_eq!(vars(&narrative("setPaused").await), [json!("Pausable::paused")], "the writer");
    assert!(narrative("audit").await.get("phase_candidates").is_none());
}
