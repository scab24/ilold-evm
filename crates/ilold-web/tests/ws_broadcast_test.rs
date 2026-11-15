use std::path::PathBuf;
use std::time::Duration;

use futures_util::StreamExt;
use tokio_tungstenite::connect_async;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("tests/fixtures")
        .join(name)
}

#[tokio::test]
async fn broadcast_add_node_on_call_command() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
        .await
        .expect("WS connection failed");

    let client = reqwest::Client::new();
    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({
            "contract": "Staking",
            "command": { "Call": { "func": "deposit" } }
        }))
        .send()
        .await
        .expect("POST failed");

    assert!(res.status().is_success(), "POST returned {}", res.status());

    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("Timed out waiting for WS message")
        .expect("WS stream ended")
        .expect("WS error");

    let text = msg.into_text().expect("Not a text message");
    let payload: serde_json::Value = serde_json::from_str(&text).unwrap();

    assert_eq!(payload["type"], "session_add_node");
    assert_eq!(payload["function"], "deposit");
    assert_eq!(payload["scenario"], "main");
    assert!(payload["step_index"].is_number());
    assert!(payload["access"].is_string() || payload["access"].is_object());
}

#[tokio::test]
async fn broadcast_clear_after_call_and_clear() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
        .await
        .expect("WS connection failed");

    let client = reqwest::Client::new();

    client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({
            "contract": "Staking",
            "command": { "Call": { "func": "deposit" } }
        }))
        .send()
        .await
        .unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await.unwrap().unwrap().unwrap();
    let payload: serde_json::Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
    assert_eq!(payload["type"], "session_add_node");

    client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({
            "contract": "Staking",
            "command": "Clear"
        }))
        .send()
        .await
        .unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await.unwrap().unwrap().unwrap();
    let payload: serde_json::Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
    assert_eq!(payload["type"], "session_clear");
    assert_eq!(payload["scenario"], "main");
}

#[tokio::test]
async fn no_broadcast_on_non_mutating_command() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
        .await
        .expect("WS connection failed");

    let client = reqwest::Client::new();

    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({
            "contract": "Staking",
            "command": "Functions"
        }))
        .send()
        .await
        .unwrap();

    assert!(res.status().is_success());

    let result = tokio::time::timeout(Duration::from_millis(500), ws.next()).await;
    assert!(result.is_err(), "Should have timed out — no broadcast expected");
}

#[tokio::test]
async fn who_command_returns_variable_info() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();
    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({
            "contract": "Staking",
            "command": { "Who": { "variable": "balances" } }
        }))
        .send()
        .await
        .unwrap();

    assert!(res.status().is_success());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["VariableInfo"]["variable"], "balances");
    assert!(body["VariableInfo"]["writers"].is_array());
}
