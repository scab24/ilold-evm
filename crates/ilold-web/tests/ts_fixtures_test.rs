//! Writes endpoint responses where `frontend/src/lib/api/fixtures/fixtures.ts` checks them against the frontend types.
use std::path::PathBuf;

const RESPONSES: &[(&str, &str)] = &[
    ("project_map", "/project/map"),
    ("project_depgraph", "/project/depgraph"),
    ("callgraph", "/contract/Vault/callgraph"),
];

#[tokio::test]
async fn responses_are_written_for_the_frontend_type_check() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let (_state, port) = ilold_web::start_server(vec![root.join("../../tests/fixtures/solc/cross")], 0, 2).await.unwrap();
    let client = reqwest::Client::new();
    for (name, route) in RESPONSES {
        let res = client.get(format!("http://127.0.0.1:{port}/api{route}")).send().await.unwrap();
        assert!(res.status().is_success(), "{route}: {}", res.status());
        let body: serde_json::Value = res.json().await.unwrap();
        let text = format!("{}\n", serde_json::to_string_pretty(&body).unwrap());
        let path = root.join("frontend/src/lib/api/fixtures").join(format!("{name}.json"));
        // rewriting only on change keeps a clean checkout clean
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            std::fs::write(&path, text).unwrap();
        }
    }
}
