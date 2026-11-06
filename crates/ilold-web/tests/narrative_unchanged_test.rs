//! Value facts are added beside the call statements, so narratives and path trees of calls that move value stay as they were.
//! Golden files: tests/golden/call_resolution, recorded before value facts existed. Fixture: tests/fixtures/solc/call_resolution.
use std::path::PathBuf;

use serde_json::Value;

/// Fields added to the function narrative since the golden files were recorded.
const ADDED: &[&str] = &["value_transfers", "balance_reads"];

#[tokio::test]
async fn narratives_and_paths_of_value_moving_calls_are_unchanged() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let (_state, port) = ilold_web::start_server(vec![root.join("../../tests/fixtures/solc/call_resolution")], 0, 2).await.unwrap();
    let client = reqwest::Client::new();
    for func in ["pay", "lowLevel", "viaCast", "viaState"] {
        for (file, route) in [("function", format!("/session/function/Caller/{func}")), ("paths", format!("/contract/Caller/{func}/paths"))] {
            let res = client.get(format!("http://127.0.0.1:{port}/api{route}")).send().await.unwrap();
            assert!(res.status().is_success(), "{route}: {}", res.status());
            let mut body: Value = res.json().await.unwrap();
            if let Some(object) = body.as_object_mut() {
                ADDED.iter().for_each(|field| drop(object.remove(*field)));
            }
            let golden = root.join("tests/golden/call_resolution").join(format!("{func}_{file}.json"));
            let expected: Value = serde_json::from_str(&std::fs::read_to_string(&golden).unwrap()).unwrap();
            assert_eq!(body, expected, "{route} differs from {}", golden.display());
        }
    }
}
