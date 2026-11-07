//! Writes endpoint responses where `frontend/src/lib/api/fixtures/fixtures.ts` checks them against the frontend types.
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// Per fixture project, the file name and route of each response.
const RESPONSES: &[(&str, &[(&str, &str)])] = &[
    (
        "dispatch",
        &[
            ("project_map", "/project/map"),
            ("project_depgraph", "/project/depgraph"),
            ("callgraph", "/contract/Router/callgraph"),
            ("contract_detail", "/contract/Router"),
        ],
    ),
    (
        "keys",
        &[("keys_contract_detail", "/contract/Keys"), ("keys_cfg", "/contract/Keys/move/cfg"), ("keys_function", "/session/function/Keys/move")],
    ),
    (
        "trycatch",
        &[
            ("trycatch_contract_detail", "/contract/TryCatch"),
            ("trycatch_cfg", "/contract/TryCatch/clauses/cfg"),
            ("trycatch_function", "/session/function/TryCatch/clauses"),
        ],
    ),
    (
        "value",
        &[
            ("value_contract_detail", "/contract/Payments"),
            ("value_function", "/session/function/Payments/buy"),
            ("value_balances_function", "/session/function/Payments/balances"),
        ],
    ),
    ("phases", &[("phases_contract_detail", "/contract/Sale")]),
];

fn write(root: &Path, name: &str, body: &Value) {
    let text = format!("{}\n", serde_json::to_string_pretty(body).unwrap());
    let path = root.join("frontend/src/lib/api/fixtures").join(format!("{name}.json"));
    // rewriting only on change keeps a clean checkout clean
    if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
        std::fs::write(&path, text).unwrap();
    }
}

#[tokio::test]
async fn responses_are_written_for_the_frontend_type_check() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let client = reqwest::Client::new();
    for (fixture, responses) in RESPONSES {
        let (_state, port) = ilold_web::start_server(vec![root.join("../../tests/fixtures/solc").join(fixture)], 0, 2).await.unwrap();
        for (name, route) in *responses {
            let res = client.get(format!("http://127.0.0.1:{port}/api{route}")).send().await.unwrap();
            assert!(res.status().is_success(), "{route}: {}", res.status());
            write(&root, name, &res.json().await.unwrap());
        }
        if *fixture == "keys" {
            let who = json!({"contract": "Keys", "command": {"Who": {"variable": "balances"}}});
            let res = client.post(format!("http://127.0.0.1:{port}/api/cmd")).json(&who).send().await.unwrap();
            assert!(res.status().is_success(), "who: {}", res.status());
            write(&root, "keys_who", &res.json().await.unwrap());
        }
    }
}
