use std::path::PathBuf;

use serde_json::{json, Value};

#[tokio::test]
async fn the_false_edge_of_a_branch_reads_as_the_negated_condition() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/control_flow");
    let (_state, port) = ilold_web::start_server(vec![fixture], 0, 2).await.unwrap();
    let url = format!("http://127.0.0.1:{port}/api/contract/ControlFlow/ifRevert/cfg");
    let res = reqwest::get(&url).await.unwrap();
    assert!(res.status().is_success(), "{url}: {}", res.status());
    let cfg: Value = res.json().await.unwrap();
    let edges: Vec<Value> = cfg["edges"].as_array().unwrap().iter().map(|e| e["data"].clone()).collect();
    assert_eq!(
        Value::from(edges),
        json!([
            {"id": "e0", "source": "b0", "target": "b1", "kind": "ConditionalTrue { condition: \"x < 10\" }", "kind_label": "x < 10", "call_count": 0},
            {"id": "e1", "source": "b1", "target": "b2", "kind": "Unconditional", "call_count": 0},
            {"id": "e2", "source": "b0", "target": "b3", "kind": "ConditionalFalse { condition: \"x < 10\" }", "kind_label": "!(x < 10)", "call_count": 0},
            {"id": "e3", "source": "b3", "target": "b4", "kind": "Unconditional", "call_count": 0},
        ]),
    );
}
