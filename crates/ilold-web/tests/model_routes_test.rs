//! Contract identity on the API. Fixture: tests/fixtures/solc/model/project.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::Value;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/model/project")
}

async fn start() -> (reqwest::Client, String) {
    let (_state, port) = ilold_web::start_server(vec![fixture()], 0, 2).await.unwrap();
    (reqwest::Client::new(), format!("http://127.0.0.1:{port}/api"))
}

async fn get(client: &reqwest::Client, url: String) -> (reqwest::StatusCode, Value) {
    let res = client.get(url).send().await.unwrap();
    let status = res.status();
    let text = res.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

fn encode(segment: &str) -> String {
    segment.replace('/', "%2F").replace(':', "%3A")
}

fn strings(values: &Value, field: &str) -> BTreeSet<String> {
    values.as_array().unwrap().iter().map(|v| v[field].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn ambiguous_contract_name_is_409_and_qualified_ids_select_one() {
    let (client, base) = start().await;
    for route in ["", "/callgraph", "/depgraph", "/sequences", "/suggestions", "/mint/paths"] {
        let (status, body) = get(&client, format!("{base}/contract/Token{route}")).await;
        assert_eq!(status, reqwest::StatusCode::CONFLICT, "{route}: {body}");
        let candidates: BTreeSet<String> =
            body["candidates"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_string()).collect();
        assert_eq!(
            candidates,
            BTreeSet::from(["src/core/Token.sol:Token".to_string(), "src/ext/Token.sol:Token".to_string()]),
            "{route}"
        );
    }

    let (status, core) = get(&client, format!("{base}/contract/{}", encode("src/core/IToken.sol:IToken"))).await;
    assert!(status.is_success(), "{core}");
    assert_eq!(core["functions"].as_array().unwrap().len(), 3);
    assert_eq!(core["qualified_id"], "src/core/IToken.sol:IToken");
    assert_eq!(core["display_name"], "IToken");
    let (_, ext) = get(&client, format!("{base}/contract/{}", encode("src/ext/IToken.sol:IToken"))).await;
    assert_eq!(ext["functions"].as_array().unwrap().len(), 1);

    let ext_token = encode("src/ext/Token.sol:Token");
    let (status, paths) = get(&client, format!("{base}/contract/{ext_token}/mint/paths")).await;
    assert!(status.is_success(), "{paths}");
    assert_eq!(paths["contract"], "src/ext/Token.sol:Token");

    let (status, deps) = get(&client, format!("{base}/contract/{ext_token}/depgraph")).await;
    assert!(status.is_success(), "{deps}");
    let edges: BTreeSet<(String, String)> = deps["edges"].as_array().unwrap().iter()
        .map(|e| (e["data"]["source"].as_str().unwrap().into(), e["data"]["target"].as_str().unwrap().into()))
        .collect();
    assert_eq!(edges, BTreeSet::from([("src/ext/Token.sol:Token".into(), "src/core/IToken.sol:IToken".into())]));

    let (status, _) = get(&client, format!("{base}/contract/Nope")).await;
    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn dependencies_are_hidden_by_default_and_listed_on_request() {
    let (client, base) = start().await;
    let (_, project) = get(&client, format!("{base}/project")).await;
    let names = strings(&project["contracts"], "name");
    assert!(names.contains("Vault") && names.contains("src/core/IToken.sol:IToken"), "{names:?}");
    assert!(!names.contains("Multicall") && !names.contains("Context"), "{names:?}");
    assert!(project["contracts"].as_array().unwrap().iter().all(|c| c["is_dependency"] == false));

    let (_, all) = get(&client, format!("{base}/project?include_dependencies=true")).await;
    let all_names = strings(&all["contracts"], "name");
    for dep in ["Multicall", "Context", "Ownable", "Math"] {
        assert!(all_names.contains(dep), "{dep} in {all_names:?}");
    }
    assert_eq!(all_names.len(), names.len() + 4);

    let (_, graph) = get(&client, format!("{base}/project/depgraph")).await;
    let ids: BTreeSet<String> = graph["nodes"].as_array().unwrap().iter()
        .map(|n| n["data"]["id"].as_str().unwrap().to_string()).collect();
    assert!(!ids.contains("Multicall") && ids.contains("Vault"), "{ids:?}");
    for e in graph["edges"].as_array().unwrap() {
        assert!(ids.contains(e["data"]["source"].as_str().unwrap()) && ids.contains(e["data"]["target"].as_str().unwrap()));
    }
    let (_, full) = get(&client, format!("{base}/project/depgraph?include_dependencies=true")).await;
    assert!(full["edges"].as_array().unwrap().iter().any(|e| e["data"]["source"] == "Vault"
        && e["data"]["target"] == "Multicall" && e["data"]["kinds"].as_array().unwrap().contains(&"inherits".into())));
    assert!(full["nodes"].as_array().unwrap().iter().any(|n| n["data"]["id"] == "Multicall" && n["data"]["is_dependency"] == true));
}

#[tokio::test]
async fn derived_contract_lists_inherited_members_from_dependencies() {
    let (client, base) = start().await;
    let (status, vault) = get(&client, format!("{base}/contract/Vault")).await;
    assert!(status.is_success(), "{vault}");
    let inherited = strings(&vault["inherited_functions"], "name");
    assert_eq!(
        inherited,
        ["multicall", "owner", "_transferOwnership", "_msgSender", "_helper", "useSecret"].map(String::from).into()
    );
    let vars: Vec<&str> = vault["inherited_state_vars"].as_array().unwrap().iter()
        .map(|v| v["name"].as_str().unwrap()).collect();
    assert_eq!(vars, ["baseVar", "_owner"]);
    let multicall = vault["inherited_functions"].as_array().unwrap().iter().find(|f| f["name"] == "multicall").unwrap();
    assert!(multicall["path_count"].as_u64().unwrap() > 0, "{multicall}");
}

#[tokio::test]
async fn session_routes_tell_ambiguous_from_unknown_contracts() {
    let (client, base) = start().await;
    let candidates = serde_json::json!(["src/core/Token.sol:Token", "src/ext/Token.sol:Token"]);
    for route in ["session/function/Token/mint", "session/trace/Token/mint", "session/slice/mint/held?contract=Token"] {
        let (status, body) = get(&client, format!("{base}/{route}")).await;
        assert_eq!(status, reqwest::StatusCode::CONFLICT, "{route}: {body}");
        assert_eq!(body["candidates"], candidates, "{route}");
    }
    let call = serde_json::json!({ "contract": "Token", "command": "Session" });
    let res = client.post(format!("{base}/cmd")).json(&call).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::CONFLICT);
    assert_eq!(res.json::<Value>().await.unwrap()["candidates"], candidates);
    let (status, body) = get(&client, format!("{base}/session/function/Nope/mint")).await;
    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
    assert_eq!(body, "Contract 'Nope' not found");
}

#[tokio::test]
async fn slice_follows_inherited_functions() {
    let (client, base) = start().await;
    let (status, body) = get(&client, format!("{base}/session/slice/_transferOwnership/_owner?contract=Vault")).await;
    assert!(status.is_success(), "{body}");
    assert!(!body["backward"].as_array().unwrap().is_empty() || !body["forward"].as_array().unwrap().is_empty(), "{body}");
}

#[tokio::test]
async fn bases_are_listed_with_their_contract_keys() {
    let (client, base) = start().await;
    let ext_token = encode("src/ext/Token.sol:Token");
    let (_, detail) = get(&client, format!("{base}/contract/{ext_token}")).await;
    assert_eq!(detail["inherits"], serde_json::json!(["CoreToken"]));
    assert_eq!(detail["inherit_keys"], serde_json::json!(["src/core/IToken.sol:IToken"]));

    let (_, project) = get(&client, format!("{base}/project")).await;
    let (_, map) = get(&client, format!("{base}/project/map")).await;
    for listing in [&project["contracts"], &map["contracts"]] {
        let token = listing.as_array().unwrap().iter().find(|c| c["name"] == "src/ext/Token.sol:Token").unwrap();
        assert_eq!(token["inherit_keys"], serde_json::json!(["src/core/IToken.sol:IToken"]), "{token}");
    }
    let vault = project["contracts"].as_array().unwrap().iter().find(|c| c["name"] == "Vault").unwrap();
    assert_eq!(vault["inherit_keys"], serde_json::json!(["VaultBase", "Ownable", "Multicall", "IVault"]));
}

#[tokio::test]
async fn inherited_functions_can_be_drawn_expanded_and_searched() {
    let (state, port) = ilold_web::start_server(vec![fixture()], 0, 2).await.unwrap();
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}/api");

    let (_, graph) = get(&client, format!("{base}/contract/Vault/callgraph")).await;
    let owner = graph["nodes"].as_array().unwrap().iter()
        .find(|n| n["data"]["label"] == "owner").unwrap_or_else(|| panic!("{graph}"));
    assert_eq!(owner["data"]["is_external"], false);

    let (status, cfg) = get(&client, format!("{base}/contract/Vault/owner/cfg")).await;
    assert!(status.is_success(), "{cfg}");

    use ilold_web::ws::search::{search_paths, SearchQuery};
    let query = SearchQuery { query: "owner".into(), contract: Some("Vault".into()), function: None };
    let (results, total) = search_paths(&state, &query, usize::MAX).unwrap();
    assert!(total > 0 && results.iter().any(|r| r.function == "owner" && r.contract == "Vault"),
        "{:?}", results.iter().map(|r| (&r.contract, &r.function)).collect::<Vec<_>>());
    let query = SearchQuery { query: "owner".into(), contract: Some("Vault".into()), function: Some("owner".into()) };
    let (results, _) = search_paths(&state, &query, usize::MAX).unwrap();
    assert!(!results.is_empty() && results.iter().all(|r| r.function == "owner"));

    for (name, error) in [("Nope", "Contract 'Nope' not found"), ("Token", "is ambiguous")] {
        let query = SearchQuery { query: "mint".into(), contract: Some(name.into()), function: None };
        let err = search_paths(&state, &query, usize::MAX).unwrap_err();
        assert!(err.contains(error), "{err}");
    }
}

#[tokio::test]
async fn cfg_blocks_carry_source_lines() {
    let (client, base) = start().await;
    let (_, cfg) = get(&client, format!("{base}/contract/Vault/deposit/cfg")).await;
    let lines: Vec<(u64, &str)> = cfg["nodes"].as_array().unwrap().iter()
        .filter_map(|n| Some((n["data"]["span"]["start_line"].as_u64()?, n["data"]["file"].as_str()?))).collect();
    assert!(lines.iter().any(|&(line, file)| line == 20 && file.ends_with("/src/Vault.sol")), "{cfg}");
}

#[tokio::test]
async fn reads_never_highlight_and_highlight_is_a_same_origin_post() {
    use ilold_core::exploration::commands::CanvasPatch;
    let (state, port) = ilold_web::start_server(vec![fixture()], 0, 2).await.unwrap();
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}/api");
    let call = serde_json::json!({ "contract": "Vault", "command": { "Call": { "func": "deposit" } } });
    assert!(client.post(format!("{base}/cmd")).json(&call).send().await.unwrap().status().is_success());
    let mut rx = state.session_tx.subscribe();
    let highlighted = |rx: &mut tokio::sync::broadcast::Receiver<ilold_web::state::SessionBroadcast>| {
        let mut seen = Vec::new();
        while let Ok(update) = rx.try_recv() {
            if let CanvasPatch::Highlight { scenario, function } = update.patch {
                seen.push((update.contract, scenario, function));
            }
        }
        seen
    };

    for route in [
        "session/function/Vault/deposit",
        "session/function/Vault/deposit?highlight=true",
        "session/trace/Vault/owner?highlight=true",
        "session/step/0/narrative?contract=Vault&highlight=true",
        "session/step/0/trace?contract=Vault&highlight=true",
    ] {
        let (status, body) = get(&client, format!("{base}/{route}")).await;
        assert!(status.is_success(), "{route}: {body}");
    }
    assert_eq!(highlighted(&mut rx), Vec::new(), "reads affect no other client");

    let post = |body: Value| client.post(format!("{base}/session/highlight")).json(&body).send();
    for body in [
        serde_json::json!({ "contract": "Vault", "function": "deposit" }),
        serde_json::json!({ "contract": "Vault", "function": "owner" }),
        serde_json::json!({ "contract": "Vault", "step": 0 }),
    ] {
        let response = post(body.clone()).await.unwrap();
        assert!(response.status().is_success(), "{body}: {}", response.status());
    }
    let expected = |f: &str| ("Vault".to_string(), "main".to_string(), f.to_string());
    assert_eq!(highlighted(&mut rx), vec![expected("deposit"), expected("owner"), expected("deposit")]);

    for (body, status) in [
        (serde_json::json!({ "contract": "Vault", "function": "nope" }), 404),
        (serde_json::json!({ "contract": "Vault", "step": 9 }), 404),
        (serde_json::json!({ "contract": "Vault" }), 400),
        (serde_json::json!({ "contract": "Vault", "function": "deposit", "step": 0 }), 400),
    ] {
        assert_eq!(post(body.clone()).await.unwrap().status().as_u16(), status, "{body}");
    }
    let foreign = client.post(format!("{base}/session/highlight"))
        .header("Origin", "http://evil.example")
        .json(&serde_json::json!({ "contract": "Vault", "function": "deposit" }))
        .send().await.unwrap();
    assert_eq!(foreign.status().as_u16(), 403);
    assert_eq!(highlighted(&mut rx), Vec::new());
}
