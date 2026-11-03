//! Paged lists. Fixture: tests/fixtures/staking (server sequence depth 2).

use std::path::PathBuf;

use serde_json::Value;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/staking")
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

fn page(body: &Value) -> (u64, u64, u64, bool) {
    let p = &body["page"];
    (p["offset"].as_u64().unwrap(), p["limit"].as_u64().unwrap(), p["total"].as_u64().unwrap(), p["truncated"].as_bool().unwrap())
}

#[tokio::test]
async fn paths_are_paged_with_stats_and_a_marker() {
    let (client, base) = start().await;
    let (status, all) = get(&client, format!("{base}/Staking/withdraw/paths")).await;
    assert!(status.is_success(), "{all}");
    let total = all["stats"]["total_paths"].as_u64().unwrap();
    assert!(total >= 3, "{all}");
    assert_eq!(page(&all), (0, 100, total, false));
    assert_eq!(all["paths"].as_array().unwrap().len() as u64, total);
    assert_eq!(all["stats"]["truncated"], false);
    for field in ["loop_cutoff", "depth_cutoff", "paths_dropped"] {
        assert_eq!(all["stats"][field], 0, "{field}");
    }

    let (_, second) = get(&client, format!("{base}/Staking/withdraw/paths?offset=1&limit=1")).await;
    assert_eq!(page(&second), (1, 1, total, total > 2));
    assert_eq!(second["paths"].as_array().unwrap().len(), 1);
    assert_eq!(second["paths"][0], all["paths"][1]);
    assert!(second["paths"][0]["annotations"]["require_checks"].is_array());

    let (_, clamped) = get(&client, format!("{base}/Staking/withdraw/paths?limit=5000")).await;
    assert_eq!(page(&clamped).1, 1000, "the applied limit is reported");
}

#[tokio::test]
async fn sequences_honour_depth_and_pages() {
    let (client, base) = start().await;
    let (status, default) = get(&client, format!("{base}/Staking/sequences")).await;
    assert!(status.is_success(), "{default}");
    let n = default["functions"].as_array().unwrap().len() as u64;
    assert_eq!(n, 8);
    assert_eq!(default["max_depth"], 2);
    assert_eq!(page(&default), (0, 100, n + n * n, false));

    let (_, shallow) = get(&client, format!("{base}/Staking/sequences?depth=1")).await;
    assert_eq!(page(&shallow).2, n);
    assert!(shallow["sequences"].as_array().unwrap().iter().all(|s| s["depth"] == 1));

    let (_, deep) = get(&client, format!("{base}/Staking/sequences?depth=3&offset=8&limit=10")).await;
    assert_eq!(page(&deep), (8, 10, n + n * n + n * n * n, true));
    assert_eq!(deep["sequences"][0]["steps"], serde_json::json!([0, 0]));

    let (_, light) = get(&client, format!("{base}/Staking/sequences?limit=0")).await;
    assert!(light["sequences"].as_array().unwrap().is_empty());
    assert_eq!(light["functions"].as_array().unwrap().len() as u64, n);

    for depth in [0, 9] {
        let (status, body) = get(&client, format!("{base}/Staking/sequences?depth={depth}")).await;
        assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{depth}: {body}");
    }

    let (_, interface) = get(&client, format!("{base}/IERC20/sequences?depth=3")).await;
    assert!(interface["functions"].as_array().unwrap().is_empty(), "{interface}");
    assert_eq!(page(&interface).2, 0);
}

#[tokio::test]
async fn contract_summaries_flag_truncated_paths() {
    let (client, base) = start().await;
    let (_, detail) = get(&client, format!("{base}/Staking")).await;
    let functions = detail["functions"].as_array().unwrap();
    assert!(!functions.is_empty());
    assert!(functions.iter().all(|f| f["paths_truncated"] == false), "{functions:?}");
}

#[test]
fn path_search_reports_every_match_but_returns_a_bounded_list() {
    use ilold_web::ws::search::{search_paths, SearchQuery};
    let state = ilold_web::state::AppState::from_paths(&[fixture()], 2, ([127, 0, 0, 1], 0).into(), fixture()).unwrap();
    let query = SearchQuery { query: "revert".into(), contract: None, function: None };
    let (all, total) = search_paths(&state, &query, usize::MAX).unwrap();
    assert!(total > 1 && all.len() == total, "{total}");
    let (first, counted) = search_paths(&state, &query, 1).unwrap();
    assert_eq!((first.len(), counted), (1, total));
}
