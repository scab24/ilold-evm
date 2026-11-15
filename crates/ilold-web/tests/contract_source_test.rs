use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures")
        .join(name)
}

async fn start_with(fixture_name: &str) -> (reqwest::Client, u16) {
    let paths = vec![fixture(fixture_name)];
    let (_state, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();
    (reqwest::Client::new(), port)
}

#[tokio::test]
async fn get_function_source_returns_source_for_deposit() {
    let (client, port) = start_with("staking").await;

    let res = client
        .get(format!(
            "http://127.0.0.1:{port}/api/contract/Staking/deposit/source"
        ))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success(), "status: {}", res.status());

    let body: serde_json::Value = res.json().await.unwrap();
    let source = body.get("source").and_then(|v| v.as_str()).unwrap();
    assert!(
        source.contains("function deposit"),
        "expected signature line, got:\n{source}"
    );
    assert!(
        source.contains("balances[msg.sender]"),
        "expected deposit body, got:\n{source}"
    );
    assert!(
        source.contains("emit Staked"),
        "expected deposit emit statement, got:\n{source}"
    );
    let start_line = body.pointer("/span/start_line").and_then(|v| v.as_u64()).unwrap();
    let end_line = body.pointer("/span/end_line").and_then(|v| v.as_u64()).unwrap();
    assert!(end_line > start_line, "full-body span must span >1 lines, got: {start_line}..{end_line}");
    let file_path = body.get("file_path").and_then(|v| v.as_str()).unwrap();
    assert!(file_path.ends_with("staking.sol"), "unexpected file_path: {file_path}");
}

#[tokio::test]
async fn get_function_source_returns_404_for_missing_function() {
    let (client, port) = start_with("staking").await;

    let res = client
        .get(format!(
            "http://127.0.0.1:{port}/api/contract/Staking/doesNotExist/source"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        reqwest::StatusCode::NOT_FOUND,
        "unknown function must 404, got: {}",
        res.status(),
    );
}

#[tokio::test]
async fn get_function_source_resolves_inherited_function() {
    let (client, port) = start_with("governor").await;

    let res = client
        .get(format!(
            "http://127.0.0.1:{port}/api/contract/Governor/schedule/source"
        ))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success(), "status: {}", res.status());

    let body: serde_json::Value = res.json().await.unwrap();
    let source = body.get("source").and_then(|v| v.as_str()).unwrap();
    assert!(
        source.contains("function schedule"),
        "inherited function source missing, got:\n{source}"
    );
}
