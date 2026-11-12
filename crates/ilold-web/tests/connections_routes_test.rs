//! Connections, routes and value flow on the API, as the running contract sees its inherited code. Fixture: tests/fixtures/connections.
use std::path::PathBuf;

use serde_json::{json, Value};

async fn server() -> u16 {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/connections");
    ilold_web::start_server(vec![fixture], 0, 2).await.unwrap().1
}

async fn get(port: u16, route: &str) -> (u16, Value) {
    let res = reqwest::get(format!("http://127.0.0.1:{port}/api{route}")).await.unwrap();
    let status = res.status().as_u16();
    (status, res.json().await.unwrap_or(Value::Null))
}

async fn ok(port: u16, route: &str) -> Value {
    let (status, body) = get(port, route).await;
    assert_eq!(status, 200, "{route}");
    body
}

fn groups(list: &Value) -> Vec<(String, usize)> {
    list.as_array().unwrap().iter().map(|g| (g["contract"].as_str().unwrap().to_string(), g["calls"].as_array().unwrap().len())).collect()
}

fn pairs(list: &[(&str, usize)]) -> Vec<(String, usize)> {
    list.iter().map(|(c, n)| (c.to_string(), *n)).collect()
}

/// Every field of the connections of Hook; `ROOT` is the fixture directory.
const HOOK: &str = r#"{
  "contract": "Hook",
  "outgoing": [],
  "incoming": [{"contract": "Curve", "calls": [
        {"in_context": "Curve", "function": "CurveBase::_graduate", "span": {"file_index": 1, "start_line": 27, "start_col": 9, "end_line": 27, "end_col": 63}, "kind": "call", "target": "IHook::migrate", "callees": [{"function": "Hook::migrate", "contract": "Hook", "basis": "inherits_interface", "tag": "INFERRED"}],
         "unresolved_reason": null, "via": "CurveBase::hook", "in_try": false, "transfers": ["CurveBase::_graduate#0"], "file": "ROOT/src/Curve.sol"},
        {"in_context": "Curve", "function": "CurveBase::_graduate", "span": {"file_index": 1, "start_line": 27, "start_col": 52, "end_line": 27, "end_col": 62}, "kind": "read", "target": "IHook::fee", "callees": [{"function": "Hook::fee", "contract": "Hook", "basis": "inherits_interface", "tag": "INFERRED"}],
         "unresolved_reason": null, "via": "CurveBase::hook", "in_try": false, "transfers": [], "file": "ROOT/src/Curve.sol"}]}],
  "unresolved": [
        {"in_context": "Hook", "function": "Hook::migrate", "span": {"file_index": 3, "start_line": 35, "start_col": 9, "end_line": 35, "end_col": 43}, "kind": "call", "target": "IPoolManager::unlock", "callees": [],
         "unresolved_reason": "out_of_scope_interface", "via": "Hook::manager", "in_try": false, "transfers": [], "file": "ROOT/src/Parts.sol"},
        {"in_context": "Hook", "function": "Hook::migrate", "span": {"file_index": 3, "start_line": 36, "start_col": 9, "end_line": 36, "end_col": 40}, "kind": "call", "target": "IPoolManager::settle", "callees": [],
         "unresolved_reason": "out_of_scope_interface", "via": "Hook::manager", "in_try": false, "transfers": ["Hook::migrate#0"], "file": "ROOT/src/Parts.sol"}],
  "assignments": [{"state_var": "Hook::manager", "function": "Hook::constructor", "span": {"file_index": 3, "start_line": 31, "start_col": 9, "end_line": 31, "end_col": 27}, "expr_text": "manager_", "kind": "constructor_param"}],
  "transfers": [
        {"id": "CurveBase::_graduate#0", "function": "CurveBase::_graduate", "in_context": "Curve", "modifier": null, "span": {"file_index": 1, "start_line": 27, "start_col": 9, "end_line": 27, "end_col": 63},
         "asset_kind": "native", "asset_candidates": [], "operation": "call", "direction": "out", "token_expr": null, "from_expr": null, "to_expr": "hook",
         "amount_expr": "address(this).balance", "id_expr": null, "call": "hook.migrate", "via_library": null, "tag": "EXTRACTED", "reason": null},
        {"id": "Hook::migrate#0", "function": "Hook::migrate", "in_context": "Hook", "modifier": null, "span": {"file_index": 3, "start_line": 36, "start_col": 9, "end_line": 36, "end_col": 40},
         "asset_kind": "native", "asset_candidates": [], "operation": "call", "direction": "out", "token_expr": null, "from_expr": null, "to_expr": "manager",
         "amount_expr": "amount", "id_expr": null, "call": "manager.settle", "via_library": null, "tag": "EXTRACTED", "reason": null}],
  "outgoing_calls": 0,
  "incoming_calls": 2
}"#;

#[tokio::test]
async fn connections_list_the_calls_of_the_running_contract_and_into_it() {
    let port = server().await;
    let curve = ok(port, "/contract/Curve/connections").await;
    assert_eq!(curve["contract"], "Curve");
    assert_eq!(groups(&curve["outgoing"]), pairs(&[("DenA", 1), ("DenB", 1), ("Hook", 2), ("Payout", 1), ("Token", 2)]));
    let token: Vec<(&str, &str)> = curve["outgoing"][4]["calls"].as_array().unwrap().iter().map(|c| (c["function"].as_str().unwrap(), c["kind"].as_str().unwrap())).collect();
    assert_eq!(token, vec![("Curve::buy", "call"), ("Payout::pay", "call")], "the project library runs in Curve");
    assert_eq!(groups(&curve["incoming"]), pairs(&[("Router", 2)]), "buyWith and the price read");
    let deliver = &curve["outgoing"][0]["calls"][0];
    assert_eq!((&deliver["function"], &deliver["kind"], deliver["callees"].as_array().unwrap().len()), (&json!("Curve::buy"), &json!("call"), 2));
    assert_eq!(deliver["callees"][0]["tag"], "AMBIGUOUS");
    let hook: Vec<(&str, &str, &str)> = curve["outgoing"][2]["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["function"].as_str().unwrap(), c["kind"].as_str().unwrap(), c["via"].as_str().unwrap()))
        .collect();
    assert_eq!(hook, vec![("CurveBase::_graduate", "call", "CurveBase::hook"), ("CurveBase::_graduate", "read", "CurveBase::hook")], "inherited code runs in Curve");
    let sites: Vec<(&str, &str)> = curve["assignments"].as_array().unwrap().iter().map(|a| (a["state_var"].as_str().unwrap(), a["kind"].as_str().unwrap())).collect();
    assert_eq!(sites, vec![("CurveBase::den", "constructor_param"), ("CurveBase::hook", "constructor_param"), ("CurveBase::token", "constructor_param")]);
    assert_eq!(curve["unresolved"], json!([]));

    let hook = ok(port, "/contract/Hook/connections").await;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/connections").canonicalize().unwrap();
    let expected: Value = serde_json::from_str(&HOOK.replace("ROOT", &root.display().to_string())).unwrap();
    assert_eq!(hook, expected, "the abstract CurveBase calls Hook only inside Curve");
    assert_eq!(get(port, "/contract/Nope/connections").await.0, 404);
}

/// Per route, per step: `caller -> callee` and whether the previous step's code reaches the caller.
fn hops(routes: &Value) -> Vec<Vec<Vec<(String, String, bool)>>> {
    let call = |c: &Value| (c["call"]["function"].as_str().unwrap().to_string(), c["callee"]["function"].as_str().unwrap().to_string(), c["recorded"] == true);
    let steps = |r: &Value| r["steps"].as_array().unwrap().iter().map(|s| s["calls"].as_array().unwrap().iter().map(call).collect()).collect();
    routes["routes"].as_array().unwrap().iter().map(steps).collect()
}

fn hop(caller: &str, callee: &str) -> (String, String, bool) {
    (caller.into(), callee.into(), true)
}

#[tokio::test]
async fn a_route_follows_calls_and_reads_only_when_asked() {
    let port = server().await;
    let route = ok(port, "/project/route?from=Router&to=Hook").await;
    assert_eq!(hops(&route), vec![vec![vec![hop("Router::buyWith", "Curve::buy")], vec![hop("CurveBase::_graduate", "Hook::migrate")]]]);
    assert_eq!((&route["from"], &route["to"], &route["truncated"]), (&json!("Router"), &json!("Hook"), &json!(false)));
    let reads = ok(port, "/project/route?from=Router&to=Hook&reads=true").await;
    assert_eq!(
        hops(&reads),
        vec![vec![
            vec![hop("Router::buyWith", "Curve::buy"), hop("Router::quote", "CurveBase::price")],
            vec![hop("CurveBase::_graduate", "Hook::migrate"), hop("CurveBase::_graduate", "Hook::fee")],
        ]]
    );
    assert_eq!(ok(port, "/project/route?from=Hook&to=Router").await, json!({"from": "Hook", "to": "Router", "routes": [], "truncated": false}), "calls run one way");
    assert_eq!(get(port, "/project/route?from=Router&to=Nope").await.0, 404);
    assert_eq!(get(port, "/project/route?from=Router").await.0, 400);
}

fn party(p: &Value) -> String {
    match p["kind"].as_str().unwrap() {
        "contract" => p["name"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect::<Vec<_>>().join("|"),
        "declared" => format!("declared {}", p["name"].as_str().unwrap()),
        _ => format!("`{}`", p["name"].as_str().unwrap_or("")),
    }
}

fn flow(body: &Value) -> Vec<(String, String, String, String, usize)> {
    let pair = |p: &Value| (p["in_context"].as_str().unwrap().to_string(), party(&p["from"]), party(&p["to"]), p["asset_kind"].as_str().unwrap().to_string(), p["transfers"].as_array().unwrap().len());
    body["pairs"].as_array().unwrap().iter().map(pair).collect()
}

#[tokio::test]
async fn value_flow_pairs_the_parties_the_code_resolves() {
    let port = server().await;
    let row = |c: &str, from: &str, to: &str, asset: &str| (c.to_string(), from.to_string(), to.to_string(), asset.to_string(), 1);
    let hook = ok(port, "/project/value-flow?contract=Hook").await;
    assert_eq!(hook["contract"], "Hook");
    assert_eq!(flow(&hook), vec![row("Curve", "Curve", "Hook", "native"), row("Hook", "Hook", "declared IPoolManager", "native")]);
    let transfers: Vec<(&str, &str)> = hook["transfers"].as_array().unwrap().iter().map(|t| (t["in_context"].as_str().unwrap(), t["call"].as_str().unwrap())).collect();
    assert_eq!(transfers, vec![("Curve", "hook.migrate"), ("Hook", "manager.settle")]);

    let all = ok(port, "/project/value-flow").await;
    assert_eq!(all["contract"], Value::Null);
    assert_eq!(
        flow(&all),
        vec![
            row("Curve", "Curve", "Hook", "native"),
            row("Curve", "Curve", "`payable(to)`", "native"),
            row("Curve", "Curve", "`to`", "erc20"),
            row("Curve", "Token", "`to`", "erc20"),
            row("Hook", "Hook", "declared IPoolManager", "native"),
            row("Router", "Router", "Curve", "native"),
        ],
        "every pair of every deployed contract, no CurveBase or Payout context"
    );
    assert_eq!(get(port, "/project/value-flow?contract=Nope").await.0, 404);
}

#[tokio::test]
async fn the_contract_detail_reads_inherited_code_as_the_contract_runs_it() {
    let port = server().await;
    let curve = ok(port, "/contract/Curve").await;
    let graduate = curve["inherited_functions"].as_array().unwrap().iter().find(|f| f["name"] == "_graduate").unwrap();
    assert_eq!(graduate["value_transfers"][0]["in_context"], "Curve");
    let flow = |asset: &str, token: Value, counterparty: &str, functions: &[&str], ids: &[&str]| {
        json!({"direction": "out", "asset_kind": asset, "token_expr": token, "counterparty_expr": counterparty, "functions": functions, "transfers": ids})
    };
    assert_eq!(
        curve["value_flows"],
        json!([
            flow("native", Value::Null, "hook", &["CurveBase::_graduate"], &["CurveBase::_graduate#0"]),
            flow("native", Value::Null, "payable(to)", &["Curve::buy"], &["Curve::buy#1"]),
            flow("erc20", json!("token"), "to", &["Curve::buy", "Payout::pay"], &["Curve::buy#0", "Payout::pay#0"]),
        ]),
        "every transfer the code Curve runs makes, its library code included"
    );
    let stored = ok(port, "/project/value-flow?contract=Curve").await;
    let mut ids: Vec<&str> = stored["transfers"].as_array().unwrap().iter().filter(|t| t["in_context"] == "Curve").map(|t| t["id"].as_str().unwrap()).collect();
    let mut summed: Vec<&str> = curve["value_flows"].as_array().unwrap().iter().flat_map(|f| f["transfers"].as_array().unwrap()).map(|t| t.as_str().unwrap()).collect();
    ids.sort_unstable();
    summed.sort_unstable();
    assert_eq!(summed, ids, "the same transfers as the value flow of the running contract");
    let to: Vec<&str> = curve["outgoing_links"].as_array().unwrap().iter().map(|l| l["to_contract"].as_str().unwrap()).collect();
    assert_eq!(to, vec!["DenA", "DenB", "Hook", "Token"]);
    let hook = ok(port, "/contract/Hook").await;
    let from: Vec<&str> = hook["incoming_links"].as_array().unwrap().iter().map(|l| l["from_contract"].as_str().unwrap()).collect();
    assert_eq!(from, vec!["Curve"]);
    let narrative = ok(port, "/session/function/Curve/_graduate").await;
    assert_eq!(narrative["value_transfers"][0]["in_context"], "Curve");
}
