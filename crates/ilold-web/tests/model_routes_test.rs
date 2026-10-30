//! Contract identity on the API: qualified ids, 409 for ambiguous bare names, dependency
//! contracts hidden from default listings, and inherited members of a derived contract.
//! Fixture: tests/fixtures/solc/model/project.

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
