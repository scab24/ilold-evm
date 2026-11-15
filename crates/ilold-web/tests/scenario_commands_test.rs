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

async fn start() -> (reqwest::Client, u16) {
    let paths = vec![fixture("staking")];
    let (_state, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();
    (reqwest::Client::new(), port)
}

async fn cmd(
    client: &reqwest::Client,
    port: u16,
    contract: &str,
    command: serde_json::Value,
) -> serde_json::Value {
    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({ "contract": contract, "command": command }))
        .send()
        .await
        .expect("POST /api/cmd failed");
    assert!(
        res.status().is_success(),
        "POST /api/cmd returned {}",
        res.status()
    );
    res.json().await.expect("response was not JSON")
}

fn scenario_new(name: &str) -> serde_json::Value {
    serde_json::json!({ "Scenario": { "sub": { "New": { "name": name } } } })
}

fn scenario_list() -> serde_json::Value {
    serde_json::json!({ "Scenario": { "sub": "List" } })
}

fn scenario_switch(name: &str) -> serde_json::Value {
    serde_json::json!({ "Scenario": { "sub": { "Switch": { "name": name } } } })
}

fn scenario_fork(name: &str) -> serde_json::Value {
    serde_json::json!({ "Scenario": { "sub": { "Fork": { "name": name } } } })
}

fn scenario_fork_at(name: &str, at_step: usize) -> serde_json::Value {
    serde_json::json!({ "Scenario": { "sub": { "Fork": { "name": name, "at_step": at_step } } } })
}

fn scenario_delete(name: &str) -> serde_json::Value {
    serde_json::json!({ "Scenario": { "sub": { "Delete": { "name": name } } } })
}

fn call(func: &str) -> serde_json::Value {
    serde_json::json!({ "Call": { "func": func } })
}

fn list_items(r: &serde_json::Value) -> &Vec<serde_json::Value> {
    r.get("ScenarioList")
        .and_then(|v| v.get("items"))
        .and_then(|v| v.as_array())
        .expect("ScenarioList.items array")
}

fn find_scenario<'a>(items: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
    items
        .iter()
        .find(|it| it.get("name").and_then(|n| n.as_str()) == Some(name))
        .unwrap_or_else(|| panic!("scenario '{name}' not in list: {items:?}"))
}

#[tokio::test]
async fn scenario_new_creates_empty_scenario() {
    let (client, port) = start().await;

    let r = cmd(&client, port, "Staking", scenario_new("alt1")).await;
    assert_eq!(
        r.get("ScenarioCreated")
            .and_then(|v| v.get("name"))
            .and_then(|n| n.as_str()),
        Some("alt1"),
        "expected ScenarioCreated{{name: alt1}}, got: {r}"
    );

    let r = cmd(&client, port, "Staking", scenario_list()).await;
    let items = list_items(&r);
    assert_eq!(items.len(), 2, "expected 2 scenarios, got: {items:?}");
    let main = find_scenario(items, "main");
    assert_eq!(main.get("active").and_then(|v| v.as_bool()), Some(true));
    let alt1 = find_scenario(items, "alt1");
    assert_eq!(alt1.get("active").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(
        alt1.get("step_count").and_then(|v| v.as_u64()),
        Some(0),
        "alt1 should start empty"
    );
}

#[tokio::test]
async fn scenario_list_shows_all_with_active_marker() {
    let (client, port) = start().await;

    let r = cmd(&client, port, "Staking", scenario_list()).await;
    let items = list_items(&r);
    assert_eq!(items.len(), 1);
    let main = find_scenario(items, "main");
    assert_eq!(main.get("active").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(main.get("step_count").and_then(|v| v.as_u64()), Some(0));

    cmd(&client, port, "Staking", call("deposit")).await;

    let r = cmd(&client, port, "Staking", scenario_list()).await;
    let items = list_items(&r);
    let main = find_scenario(items, "main");
    assert_eq!(
        main.get("step_count").and_then(|v| v.as_u64()),
        Some(1),
        "main should have 1 step after deposit"
    );
}

#[tokio::test]
async fn scenario_switch_changes_active() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", scenario_new("alt1")).await;

    let r = cmd(&client, port, "Staking", scenario_switch("alt1")).await;
    let switched = r.get("ScenarioSwitched").expect("ScenarioSwitched variant");
    assert_eq!(
        switched.get("from").and_then(|v| v.as_str()),
        Some("main")
    );
    assert_eq!(switched.get("to").and_then(|v| v.as_str()), Some("alt1"));

    cmd(&client, port, "Staking", call("deposit")).await;

    let r = cmd(&client, port, "Staking", scenario_list()).await;
    let items = list_items(&r);
    let alt1 = find_scenario(items, "alt1");
    assert_eq!(alt1.get("active").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(alt1.get("step_count").and_then(|v| v.as_u64()), Some(1));
    let main = find_scenario(items, "main");
    assert_eq!(main.get("active").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(main.get("step_count").and_then(|v| v.as_u64()), Some(0));
}

#[tokio::test]
async fn scenario_fork_clones_prefix_at_current_step() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", call("deposit")).await;

    let r = cmd(&client, port, "Staking", scenario_fork("alt1")).await;
    let forked = r.get("ScenarioForked").expect("ScenarioForked variant");
    assert_eq!(forked.get("from").and_then(|v| v.as_str()), Some("main"));
    assert_eq!(forked.get("to").and_then(|v| v.as_str()), Some("alt1"));
    assert_eq!(forked.get("at_step").and_then(|v| v.as_u64()), Some(1));

    let r = cmd(&client, port, "Staking", scenario_list()).await;
    let items = list_items(&r);
    assert_eq!(
        find_scenario(items, "main")
            .get("step_count")
            .and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        find_scenario(items, "alt1")
            .get("step_count")
            .and_then(|v| v.as_u64()),
        Some(1),
        "fork should copy the prefix"
    );
}

#[tokio::test]
async fn scenario_fork_emits_branch_created_journal_entry() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", call("deposit")).await;
    cmd(&client, port, "Staking", scenario_fork("alt1")).await;
    cmd(&client, port, "Staking", scenario_switch("alt1")).await;

    let r = cmd(
        &client,
        port,
        "Staking",
        serde_json::json!("SaveSession"),
    )
    .await;
    let json_str = r
        .get("SessionSaved")
        .and_then(|v| v.get("json"))
        .and_then(|v| v.as_str())
        .expect("SessionSaved.json");

    let saved: serde_json::Value =
        serde_json::from_str(json_str).expect("SaveSession payload was not JSON");
    let active = saved
        .get("active")
        .and_then(|v| v.as_str())
        .expect("active scenario name");
    let entries = saved
        .pointer(&format!("/scenarios/{active}/journal/entries"))
        .and_then(|e| e.as_array())
        .expect("scenarios.<active>.journal.entries array");

    let branch = entries
        .iter()
        .find_map(|e| e.get("BranchCreated"))
        .unwrap_or_else(|| panic!("no BranchCreated entry in {entries:?}"));
    assert_eq!(
        branch.get("from_function").and_then(|v| v.as_str()),
        Some("main")
    );
    assert_eq!(
        branch.get("branch_function").and_then(|v| v.as_str()),
        Some("alt1")
    );
}

#[tokio::test]
async fn scenario_delete_refuses_when_active() {
    let (client, port) = start().await;

    let r = cmd(&client, port, "Staking", scenario_delete("main")).await;
    let msg = r
        .get("Error")
        .and_then(|v| v.get("message"))
        .and_then(|v| v.as_str())
        .expect("Error.message");
    assert!(
        msg.contains("Cannot delete active scenario"),
        "unexpected error message: {msg}"
    );
}

#[tokio::test]
async fn scenario_delete_only_remaining_is_guarded_by_active_check() {
    let (client, port) = start().await;

    let r = cmd(&client, port, "Staking", scenario_delete("main")).await;
    let msg = r
        .get("Error")
        .and_then(|v| v.get("message"))
        .and_then(|v| v.as_str())
        .expect("Error.message");
    assert!(
        msg.contains("Cannot delete active scenario"),
        "expected active-guard error, got: {msg}"
    );
}

#[tokio::test]
async fn scenario_delete_removes_other_scenario() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", scenario_new("alt1")).await;
    cmd(&client, port, "Staking", scenario_new("alt2")).await;
    cmd(&client, port, "Staking", scenario_switch("alt1")).await;

    let r = cmd(&client, port, "Staking", scenario_delete("alt2")).await;
    assert_eq!(
        r.get("ScenarioDeleted")
            .and_then(|v| v.get("name"))
            .and_then(|n| n.as_str()),
        Some("alt2"),
        "expected ScenarioDeleted{{name: alt2}}, got: {r}"
    );

    let r = cmd(&client, port, "Staking", scenario_list()).await;
    let items = list_items(&r);
    assert_eq!(items.len(), 2);
    assert_eq!(
        find_scenario(items, "alt1")
            .get("active")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        find_scenario(items, "main")
            .get("active")
            .and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[tokio::test]
async fn scenario_name_validation_rejects_invalid() {
    let (client, port) = start().await;

    for bad in [
        "InvalidName!",
        "1starts-with-digit",
        "",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        let r = cmd(&client, port, "Staking", scenario_new(bad)).await;
        let msg = r
            .get("Error")
            .and_then(|v| v.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected Error for name {bad:?}, got: {r}"));
        assert!(
            msg.contains("Invalid scenario name"),
            "unexpected error for {bad:?}: {msg}"
        );
    }
}

#[tokio::test]
async fn scenario_fork_at_step_truncates_to_n() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", call("deposit")).await;
    cmd(&client, port, "Staking", call("deposit")).await;
    cmd(&client, port, "Staking", call("withdraw")).await;

    let r = cmd(&client, port, "Staking", scenario_fork_at("alt1", 2)).await;
    let forked = r.get("ScenarioForked").expect("ScenarioForked variant");
    assert_eq!(forked.get("at_step").and_then(|v| v.as_u64()), Some(2));

    let r = cmd(&client, port, "Staking", scenario_list()).await;
    let items = list_items(&r);
    assert_eq!(
        find_scenario(items, "main")
            .get("step_count")
            .and_then(|v| v.as_u64()),
        Some(3),
        "main must be unchanged"
    );
    assert_eq!(
        find_scenario(items, "alt1")
            .get("step_count")
            .and_then(|v| v.as_u64()),
        Some(2),
        "alt1 must be truncated to first 2 steps"
    );
}

#[tokio::test]
async fn scenario_fork_at_step_zero_creates_empty() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", call("deposit")).await;
    cmd(&client, port, "Staking", call("withdraw")).await;

    let r = cmd(&client, port, "Staking", scenario_fork_at("alt1", 0)).await;
    assert_eq!(
        r.get("ScenarioForked")
            .and_then(|v| v.get("at_step"))
            .and_then(|v| v.as_u64()),
        Some(0)
    );

    let r = cmd(&client, port, "Staking", scenario_list()).await;
    let items = list_items(&r);
    assert_eq!(
        find_scenario(items, "alt1")
            .get("step_count")
            .and_then(|v| v.as_u64()),
        Some(0),
        "fork at 0 must yield an empty scenario"
    );
}

#[tokio::test]
async fn scenario_fork_at_step_greater_than_length_errors() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", call("deposit")).await;

    let r = cmd(&client, port, "Staking", scenario_fork_at("alt1", 5)).await;
    let msg = r
        .get("Error")
        .and_then(|v| v.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected Error, got: {r}"));
    assert!(
        msg.contains("Cannot fork at step 5") && msg.contains("only 1 step"),
        "unexpected error: {msg}"
    );

    let r = cmd(&client, port, "Staking", scenario_list()).await;
    let items = list_items(&r);
    assert_eq!(items.len(), 1, "failed fork must not create a scenario");
}

#[tokio::test]
async fn scenario_name_reserved_main_cannot_be_recreated() {
    let (client, port) = start().await;

    let r = cmd(&client, port, "Staking", scenario_new("main")).await;
    let msg = r
        .get("Error")
        .and_then(|v| v.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected Error, got: {r}"));
    assert!(
        msg.contains("already exists"),
        "unexpected error message: {msg}"
    );
}

fn save_session() -> serde_json::Value {
    serde_json::json!("SaveSession")
}

fn load_session(json: &str) -> serde_json::Value {
    serde_json::json!({ "LoadSession": { "json": json } })
}

#[tokio::test]
async fn save_produces_v2_json_with_all_scenarios_and_active() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", call("deposit")).await;
    cmd(&client, port, "Staking", call("deposit")).await;
    cmd(&client, port, "Staking", scenario_fork_at("alt1", 1)).await;
    cmd(&client, port, "Staking", scenario_switch("alt1")).await;

    let r = cmd(&client, port, "Staking", save_session()).await;
    let json_str = r
        .get("SessionSaved")
        .and_then(|v| v.get("json"))
        .and_then(|v| v.as_str())
        .expect("SessionSaved.json");
    let parsed: serde_json::Value =
        serde_json::from_str(json_str).expect("v2 save JSON must parse");

    assert_eq!(parsed["version"], 2, "v2 save must declare version 2");
    assert_eq!(parsed["contract"], "Staking");
    assert_eq!(
        parsed["active"], "alt1",
        "active scenario at save time must be persisted"
    );
    let scenarios = parsed["scenarios"]
        .as_object()
        .expect("scenarios must be an object");
    assert!(scenarios.contains_key("main") && scenarios.contains_key("alt1"));
    assert_eq!(
        parsed["order"]
            .as_array()
            .expect("order array")
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["main", "alt1"],
    );
    let alt1 = &scenarios["alt1"];
    assert_eq!(alt1["forked_from"]["scenario"], "main");
    assert_eq!(alt1["forked_from"]["at_step"], 1);
}

#[tokio::test]
async fn load_v2_json_restores_all_scenarios_active_and_order() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", call("deposit")).await;
    cmd(&client, port, "Staking", scenario_fork_at("alt1", 1)).await;
    let saved = cmd(&client, port, "Staking", save_session()).await;
    let json = saved["SessionSaved"]["json"].as_str().unwrap().to_string();

    cmd(&client, port, "Staking", call("withdraw")).await;
    cmd(&client, port, "Staking", scenario_new("ghost")).await;

    cmd(&client, port, "Staking", load_session(&json)).await;

    let r = cmd(&client, port, "Staking", scenario_list()).await;
    let items = list_items(&r);
    let names: Vec<&str> = items
        .iter()
        .map(|it| it.get("name").and_then(|n| n.as_str()).unwrap())
        .collect();
    assert_eq!(names, vec!["main", "alt1"], "ghost scenario must be gone");
    assert_eq!(
        find_scenario(items, "main")
            .get("step_count")
            .and_then(|v| v.as_u64()),
        Some(1),
        "main step count must reflect the snapshot, not post-save mutations"
    );
    assert_eq!(
        find_scenario(items, "alt1")
            .get("step_count")
            .and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        find_scenario(items, "main")
            .get("active")
            .and_then(|v| v.as_bool()),
        Some(true),
        "saved active was 'main'"
    );
}

#[tokio::test]
async fn load_v1_json_wraps_as_single_main_scenario() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", call("deposit")).await;
    let saved = cmd(&client, port, "Staking", save_session()).await;
    let v2_json = saved["SessionSaved"]["json"].as_str().unwrap();
    let v2: serde_json::Value = serde_json::from_str(v2_json).unwrap();
    let bare = v2["scenarios"]["main"].clone();
    let v1_json = serde_json::to_string(&bare).unwrap();

    cmd(&client, port, "Staking", scenario_new("alt1")).await;
    cmd(&client, port, "Staking", load_session(&v1_json)).await;

    let r = cmd(&client, port, "Staking", scenario_list()).await;
    let items = list_items(&r);
    assert_eq!(
        items.len(),
        1,
        "v1 load must collapse the store to a single scenario, got: {items:?}"
    );
    let only = &items[0];
    assert_eq!(only.get("name").and_then(|v| v.as_str()), Some("main"));
    assert_eq!(only.get("step_count").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(only.get("active").and_then(|v| v.as_bool()), Some(true));
}

#[tokio::test]
async fn load_replaces_existing_store_destructively() {
    let (client, port) = start().await;

    let saved = cmd(&client, port, "Staking", save_session()).await;
    let json = saved["SessionSaved"]["json"].as_str().unwrap().to_string();

    cmd(&client, port, "Staking", call("deposit")).await;
    cmd(&client, port, "Staking", call("withdraw")).await;
    cmd(&client, port, "Staking", scenario_new("alt1")).await;
    cmd(&client, port, "Staking", scenario_new("alt2")).await;

    cmd(&client, port, "Staking", load_session(&json)).await;

    let r = cmd(&client, port, "Staking", scenario_list()).await;
    let items = list_items(&r);
    assert_eq!(items.len(), 1, "load must drop alt1/alt2");
    assert_eq!(
        find_scenario(items, "main")
            .get("step_count")
            .and_then(|v| v.as_u64()),
        Some(0),
        "main must be empty per snapshot"
    );
}
