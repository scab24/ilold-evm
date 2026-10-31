//! Overloaded functions on `/api/contract/{c}/{func}/...`: a bare name works when it is
//! unambiguous, a full signature selects one overload, and an ambiguous bare name is a 409
//! listing the candidate signatures. /cfg, /paths and /source must agree on the overload.

use std::path::PathBuf;

use serde_json::Value;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/solc/call_resolution")
}

async fn start() -> (reqwest::Client, String) {
    let (_state, port) = ilold_web::start_server(vec![fixture()], 0, 2).await.unwrap();
    (reqwest::Client::new(), format!("http://127.0.0.1:{port}/api/contract"))
}

async fn get(client: &reqwest::Client, url: String) -> (reqwest::StatusCode, Value) {
    let res = client.get(url).send().await.unwrap();
    let status = res.status();
    let text = res.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

fn cfg_statements(cfg: &Value) -> Vec<String> {
    cfg["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|n| n["data"]["statements"].as_array().cloned().unwrap_or_default())
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn ambiguous_bare_name_is_409_with_candidates() {
    let (client, base) = start().await;
    for route in ["cfg", "paths", "source"] {
        let (status, body) = get(&client, format!("{base}/Over/put/{route}")).await;
        assert_eq!(status, reqwest::StatusCode::CONFLICT, "{route}: {body}");
        assert_eq!(
            body["candidates"],
            serde_json::json!(["put(uint256)", "put(address)", "put(uint256,bool)"]),
            "{route}"
        );
    }
}

#[tokio::test]
async fn signature_selects_the_same_overload_on_every_route() {
    let (client, base) = start().await;

    let (status, source) = get(&client, format!("{base}/Over/put(address)/source")).await;
    assert!(status.is_success(), "{source}");
    assert!(source["source"].as_str().unwrap().contains("function put(address who)"));
    let (status, cfg) = get(&client, format!("{base}/Over/put(address)/cfg")).await;
    assert!(status.is_success(), "{cfg}");
    let stmts = cfg_statements(&cfg);
    assert!(stmts.iter().any(|s| s == "put()"), "{stmts:?}");
    assert!(!stmts.iter().any(|s| s.starts_with("last")), "{stmts:?}");
    let (status, paths) = get(&client, format!("{base}/Over/put(address)/paths")).await;
    assert!(status.is_success(), "{paths}");
    assert_eq!(paths["function"], "put(address)");

    let (_, source) = get(&client, format!("{base}/Over/put(uint256)/source")).await;
    assert!(source["source"].as_str().unwrap().contains("function put(uint256 a)"));
    let (_, cfg) = get(&client, format!("{base}/Over/put(uint256)/cfg")).await;
    assert!(cfg_statements(&cfg).iter().any(|s| s.starts_with("last")));

    let (status, source) = get(&client, format!("{base}/Over/put(uint256,bool)/source")).await;
    assert!(status.is_success(), "{source}");
    assert!(source["source"].as_str().unwrap().contains("bool flag"));
    let (_, paths) = get(&client, format!("{base}/Over/put(uint256,bool)/paths")).await;
    assert_eq!(paths["contract"], "OverBase");
}

#[tokio::test]
async fn unambiguous_bare_name_and_unknown_function() {
    let (client, base) = start().await;
    let (status, source) = get(&client, format!("{base}/OverBase/put/source")).await;
    assert!(status.is_success(), "{source}");
    let (status, _) = get(&client, format!("{base}/Over/receive/cfg")).await;
    assert!(status.is_success());
    let (status, _) = get(&client, format!("{base}/Over/nope/cfg")).await;
    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn contract_detail_and_callgraph_expose_overload_keys_and_kinds() {
    let (client, base) = start().await;
    let (_, detail) = get(&client, format!("{base}/Over")).await;
    let keys: Vec<&str> = detail["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["constructor", "put(uint256)", "put(address)", "receive", "fallback"]);
    let inherited: Vec<&str> = detail["inherited_functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["signature"].as_str().unwrap())
        .collect();
    assert_eq!(inherited, ["put(uint256,bool)"]);

    let (_, cg) = get(&client, format!("{base}/Over/callgraph")).await;
    let edges: Vec<(String, String, String)> = cg["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            let d = &e["data"];
            (d["source"].as_str().unwrap().into(), d["target"].as_str().unwrap().into(),
                d["kind"].as_str().unwrap().into())
        })
        .collect();
    assert!(edges.contains(&("Over::put(address)".into(), "Over::put(uint256)".into(), "Internal".into())), "{edges:?}");
    assert!(edges.contains(&("Over::put(uint256)".into(), "OverBase::put".into(), "Internal".into())), "{edges:?}");

    let (_, cg) = get(&client, format!("{base}/UsesLib/callgraph")).await;
    let kinds: Vec<&str> = cg["edges"].as_array().unwrap().iter()
        .map(|e| e["data"]["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"InternalLibrary") && kinds.contains(&"LibraryDelegatecall"), "{kinds:?}");
}

#[tokio::test]
async fn scenario_steps_of_functions_sharing_a_name_keep_their_own_access() {
    let (client, base) = start().await;
    let api = base.trim_end_matches("/contract");
    for func in ["put(uint256)", "put(address)"] {
        let res = client
            .post(format!("{api}/cmd"))
            .json(&serde_json::json!({ "contract": "Clash", "command": { "Call": { "func": func } } }))
            .send()
            .await
            .unwrap();
        assert!(res.status().is_success());
    }
    let (status, all) = get(&client, format!("{api}/scenarios/all?contract=Clash")).await;
    assert!(status.is_success(), "{all}");
    let steps: Vec<(String, String)> = all["scenarios"][0]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["function"].as_str().unwrap().to_string(), s["access"].to_string()))
        .collect();
    assert_eq!(
        steps,
        [
            ("put(uint256)".to_string(), r#"{"Restricted":{"role":"onlyOwner"}}"#.to_string()),
            ("put(address)".to_string(), r#""Public""#.to_string()),
        ]
    );
}
