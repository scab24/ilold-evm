//! Value transfers, balance reads and value flows on the API. Fixture: tests/fixtures/solc/value.
use std::path::PathBuf;

use serde_json::{json, Value};

async fn contract() -> Value {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/value");
    let (_state, port) = ilold_web::start_server(vec![fixture], 0, 2).await.unwrap();
    let res = reqwest::get(format!("http://127.0.0.1:{port}/api/contract/Payments")).await.unwrap();
    assert!(res.status().is_success(), "{}", res.status());
    res.json().await.unwrap()
}

fn function<'a>(contract: &'a Value, name: &str) -> &'a Value {
    contract["functions"].as_array().unwrap().iter().find(|f| f["name"] == name).unwrap()
}

#[tokio::test]
async fn function_summaries_carry_their_transfers_and_balance_reads() {
    let contract = contract().await;
    let sent = &function(&contract, "sendEth")["value_transfers"];
    assert_eq!(sent.as_array().map(Vec::len), Some(1), "{sent}");
    assert_eq!((&sent[0]["via_library"], &sent[0]["to_expr"], &sent[0]["asset_kind"]), (&json!("Address::sendValue"), &json!("to"), &json!("native")));
    assert_eq!(function(&contract, "balances")["balance_reads"].as_array().map(Vec::len), Some(8));
    let unrecognized = function(&contract, "unrecognized");
    assert!(unrecognized.get("value_transfers").is_none() && unrecognized.get("balance_reads").is_none(), "{unrecognized}");
}

#[tokio::test]
async fn the_contract_sums_its_flows_by_direction_asset_and_counterparty() {
    let contract = contract().await;
    let flows = contract["value_flows"].as_array().unwrap();
    let withdraw = flows
        .iter()
        .find(|f| f["direction"] == "out" && f["asset_kind"] == "erc20" && f["token_expr"] == "asset" && f["counterparty_expr"] == "msg.sender")
        .unwrap();
    assert_eq!((&withdraw["functions"], &withdraw["transfers"]), (&json!(["Payments::withdraw"]), &json!(["Payments::withdraw#0"])));
    let transfers: usize = contract["functions"].as_array().unwrap().iter().filter_map(|f| f["value_transfers"].as_array()).map(Vec::len).sum();
    assert_eq!(flows.iter().map(|f| f["transfers"].as_array().unwrap().len()).sum::<usize>(), transfers);
}
