//! Guard facts on the web API: `/api/contract/{c}` function summaries (own and inherited),
//! the `f`/`fa` session commands and the function narrative carry the same facts.
//! Fixture: tests/fixtures/solc/access.

use std::path::PathBuf;

use serde_json::{json, Value};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/access")
}

async fn start() -> (reqwest::Client, String) {
    let (_state, port) = ilold_web::start_server(vec![fixture()], 0, 1).await.unwrap();
    (reqwest::Client::new(), format!("http://127.0.0.1:{port}"))
}

fn entry<'a>(list: &'a Value, name: &str) -> &'a Value {
    list.as_array().unwrap().iter().find(|f| f["name"] == name).unwrap_or_else(|| panic!("{name}: {list}"))
}

/// (kind, condition, via) of each guard.
fn facts(entry: &Value) -> Vec<(String, String, Vec<String>)> {
    entry["guards"]
        .as_array()
        .unwrap_or_else(|| panic!("guards: {entry}"))
        .iter()
        .map(|g| {
            let via = g["via"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
            (g["kind"].as_str().unwrap().to_string(), g["condition"].as_str().unwrap().to_string(), via)
        })
        .collect()
}

fn owner_fact() -> Vec<(String, String, Vec<String>)> {
    vec![(
        "CallerEquals".into(),
        "if (owner() != _msgSender()) revert OwnableUnauthorizedAccount(_msgSender())".into(),
        vec!["modifier onlyOwner".into(), "Ownable::_checkOwner".into()],
    )]
}

#[tokio::test]
async fn contract_detail_lists_access_and_guards() {
    let (client, base) = start().await;
    let body: Value = client.get(format!("{base}/api/contract/Guards")).send().await.unwrap().json().await.unwrap();

    let own = entry(&body["functions"], "setByOwner");
    assert_eq!(own["access"], json!({"Restricted": {"role": "onlyOwner"}}));
    assert_eq!(facts(own), owner_fact());

    let paused = entry(&body["functions"], "pausedSet");
    assert_eq!(paused["access"], "Public");
    assert_eq!(facts(paused)[0].0, "Pause");
    assert_eq!(paused["guards"][0]["heuristic"], false);

    let inherited = entry(&body["inherited_functions"], "transferOwnership");
    assert_eq!(inherited["access"], json!({"Restricted": {"role": "onlyOwner"}}));
    assert_eq!(facts(inherited), owner_fact());
}

#[tokio::test]
async fn session_lists_and_narrative_carry_guards() {
    let (client, base) = start().await;
    let cmd = |command: Value| {
        let client = client.clone();
        let url = format!("{base}/api/cmd");
        async move {
            let res = client.post(url).json(&json!({"contract": "Guards", "command": command})).send().await.unwrap();
            res.json::<Value>().await.unwrap()
        }
    };

    let f = cmd(json!("Functions")).await;
    let list = &f["FunctionList"]["functions"];
    assert_eq!(facts(entry(list, "setByOwner")), owner_fact());
    assert_eq!(facts(entry(list, "deposit")), []);

    let fa = cmd(json!("FunctionsAll")).await;
    let all = &fa["FunctionListAll"]["functions"];
    let inherited = entry(all, "transferOwnership");
    assert_eq!(inherited["is_inherited"], true);
    assert_eq!(inherited["access"], json!({"Restricted": {"role": "onlyOwner"}}));
    assert_eq!(facts(inherited), owner_fact());

    let narrative: Value = client
        .get(format!("{base}/api/session/function/Guards/setByOwner"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(facts(&narrative), owner_fact());
    assert!(
        narrative["observations"].as_array().unwrap().iter().all(|o| o["kind"] != "NoAccessControl"),
        "{narrative}"
    );
}
