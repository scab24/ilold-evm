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

async fn start_with_staking() -> (reqwest::Client, u16) {
    let paths = vec![fixture("staking")];
    let (_state, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();
    let client = reqwest::Client::new();

    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({
            "contract": "Staking",
            "command": { "Call": { "func": "deposit" } }
        }))
        .send()
        .await
        .expect("POST /api/cmd failed");
    assert!(
        res.status().is_success(),
        "seed POST /api/cmd returned {}",
        res.status()
    );

    (client, port)
}

#[tokio::test]
async fn existing_state_endpoint_targets_active_scenario() {
    let (client, port) = start_with_staking().await;

    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/state"))
        .send()
        .await
        .unwrap();

    assert!(
        res.status().is_success(),
        "GET /api/session/state returned {}",
        res.status()
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body.is_array(),
        "response should be a JSON array of VariableSummary, got: {body}"
    );
    let arr = body.as_array().unwrap();
    assert!(
        !arr.is_empty(),
        "active scenario state should have at least one variable summary after `c deposit`"
    );
}

#[tokio::test]
async fn existing_sequence_endpoint_targets_active_scenario() {
    let (client, port) = start_with_staking().await;

    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({
            "contract": "Staking",
            "command": { "Call": { "func": "withdraw" } }
        }))
        .send()
        .await
        .unwrap();
    assert!(
        res.status().is_success(),
        "seed second step POST returned {}",
        res.status()
    );

    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/sequence"))
        .send()
        .await
        .unwrap();

    assert!(
        res.status().is_success(),
        "GET /api/session/sequence returned {}",
        res.status()
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body.is_object(),
        "response should be a SequenceNarrative object, got: {body}"
    );
}

#[tokio::test]
async fn existing_timeline_endpoint_targets_active_scenario() {
    let (client, port) = start_with_staking().await;

    let res = client
        .get(format!(
            "http://127.0.0.1:{port}/api/session/timeline/balances"
        ))
        .send()
        .await
        .unwrap();

    assert!(
        res.status().is_success(),
        "GET /api/session/timeline/balances returned {}",
        res.status()
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body.is_object(),
        "response should be a VariableTimeline object, got: {body}"
    );
}

#[tokio::test]
async fn existing_narrative_endpoint_targets_active_scenario() {
    let (client, port) = start_with_staking().await;

    let res = client
        .get(format!(
            "http://127.0.0.1:{port}/api/session/step/0/narrative"
        ))
        .send()
        .await
        .unwrap();

    assert!(
        res.status().is_success(),
        "GET /api/session/step/0/narrative returned {}",
        res.status()
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body.is_object(),
        "response should be a FunctionNarrative object, got: {body}"
    );
}

#[tokio::test]
async fn existing_trace_endpoint_targets_active_scenario() {
    let (client, port) = start_with_staking().await;

    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/step/0/trace"))
        .send()
        .await
        .unwrap();

    assert!(
        res.status().is_success(),
        "GET /api/session/step/0/trace returned {}",
        res.status()
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body.is_object(),
        "response should be a FlowTree object, got: {body}"
    );
}

fn scenario_new(name: &str) -> serde_json::Value {
    serde_json::json!({ "Scenario": { "sub": { "New": { "name": name } } } })
}

fn scenario_switch(name: &str) -> serde_json::Value {
    serde_json::json!({ "Scenario": { "sub": { "Switch": { "name": name } } } })
}

fn call(func: &str) -> serde_json::Value {
    serde_json::json!({ "Call": { "func": func } })
}

async fn get_scenarios(client: &reqwest::Client, port: u16) -> serde_json::Value {
    let res = client
        .get(format!("http://127.0.0.1:{port}/api/scenarios"))
        .send()
        .await
        .expect("GET /api/scenarios failed");
    assert!(
        res.status().is_success(),
        "GET /api/scenarios returned {}",
        res.status()
    );
    res.json().await.expect("response was not JSON")
}

async fn get_scenarios_all(client: &reqwest::Client, port: u16) -> serde_json::Value {
    let res = client
        .get(format!("http://127.0.0.1:{port}/api/scenarios/all"))
        .send()
        .await
        .expect("GET /api/scenarios/all failed");
    assert!(
        res.status().is_success(),
        "GET /api/scenarios/all returned {}",
        res.status()
    );
    res.json().await.expect("response was not JSON")
}

#[tokio::test]
async fn get_scenarios_returns_main_on_fresh_server() {
    let (client, port) = start().await;

    let body = get_scenarios(&client, port).await;
    let arr = body
        .as_array()
        .unwrap_or_else(|| panic!("expected array, got: {body}"));
    assert_eq!(arr.len(), 1, "expected single auto-seeded main scenario");
    assert_eq!(arr[0].get("name").and_then(|v| v.as_str()), Some("main"));
    assert_eq!(arr[0].get("active").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(arr[0].get("step_count").and_then(|v| v.as_u64()), Some(0));
}

#[tokio::test]
async fn get_scenarios_reflects_new_and_step_count() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", scenario_new("alt1")).await;
    cmd(&client, port, "Staking", call("deposit")).await;

    let body = get_scenarios(&client, port).await;
    let arr = body
        .as_array()
        .unwrap_or_else(|| panic!("expected array, got: {body}"));
    assert_eq!(arr.len(), 2, "expected main + alt1, got: {arr:?}");

    let main = arr
        .iter()
        .find(|it| it.get("name").and_then(|n| n.as_str()) == Some("main"))
        .expect("main entry missing");
    assert_eq!(main.get("active").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(main.get("step_count").and_then(|v| v.as_u64()), Some(1));

    let alt1 = arr
        .iter()
        .find(|it| it.get("name").and_then(|n| n.as_str()) == Some("alt1"))
        .expect("alt1 entry missing");
    assert_eq!(alt1.get("active").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(alt1.get("step_count").and_then(|v| v.as_u64()), Some(0));
}

#[tokio::test]
async fn get_scenarios_all_returns_ordered_snapshot() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", scenario_new("alt1")).await;
    cmd(&client, port, "Staking", scenario_new("alt2")).await;
    cmd(&client, port, "Staking", call("deposit")).await;

    let body = get_scenarios_all(&client, port).await;
    assert_eq!(body.get("active").and_then(|v| v.as_str()), Some("main"));

    let scenarios = body
        .get("scenarios")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("scenarios should be an array, got: {body}"));
    assert_eq!(scenarios.len(), 3, "expected 3 scenarios, got: {scenarios:?}");

    let names: Vec<&str> = scenarios
        .iter()
        .map(|entry| entry.get("name").and_then(|v| v.as_str()).expect("name"))
        .collect();
    assert_eq!(names, vec!["main", "alt1", "alt2"], "insertion order broken");

    let main_steps = scenarios[0]
        .get("steps")
        .and_then(|v| v.as_array())
        .expect("main steps array");
    assert_eq!(main_steps.len(), 1, "main should have 1 step");
    let step0 = &main_steps[0];
    assert_eq!(
        step0.get("function").and_then(|v| v.as_str()),
        Some("deposit")
    );
    assert_eq!(step0.get("step_index").and_then(|v| v.as_u64()), Some(0));
    assert!(
        step0.get("access").is_some(),
        "SessionStepView must serialize an `access` field, got: {step0}"
    );

    for idx in 0..3 {
        assert!(
            scenarios[idx].get("forked_from").map(|v| v.is_null()).unwrap_or(true),
            "scenarios[{idx}] ({}) should have forked_from = null, got: {}",
            names[idx],
            scenarios[idx]
        );
    }

    for idx in [1usize, 2] {
        let steps = scenarios[idx]
            .get("steps")
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| panic!("scenarios[{idx}] missing steps array"));
        assert!(
            steps.is_empty(),
            "scenarios[{idx}] ({}) should be empty, got: {steps:?}",
            names[idx]
        );
    }
}

#[tokio::test]
async fn get_scenarios_all_tracks_active_after_switch() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", scenario_new("alt1")).await;
    cmd(&client, port, "Staking", scenario_switch("alt1")).await;

    let body = get_scenarios_all(&client, port).await;
    assert_eq!(
        body.get("active").and_then(|v| v.as_str()),
        Some("alt1"),
        "active should reflect post-switch scenario, got: {body}"
    );
}

#[tokio::test]
async fn get_scenarios_all_exposes_fork_origin() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", call("deposit")).await;
    cmd(&client, port, "Staking", call("deposit")).await;
    cmd(&client, port, "Staking", call("withdraw")).await;
    cmd(
        &client,
        port,
        "Staking",
        serde_json::json!({ "Scenario": { "sub": { "Fork": { "name": "alt1", "at_step": 2 } } } }),
    )
    .await;

    let body = get_scenarios_all(&client, port).await;
    let scenarios = body
        .get("scenarios")
        .and_then(|v| v.as_array())
        .expect("scenarios array");

    let main = scenarios
        .iter()
        .find(|s| s.get("name").and_then(|v| v.as_str()) == Some("main"))
        .expect("main snapshot");
    assert!(
        main.get("forked_from").map(|v| v.is_null()).unwrap_or(true),
        "main must have forked_from = null, got: {main}"
    );

    let alt1 = scenarios
        .iter()
        .find(|s| s.get("name").and_then(|v| v.as_str()) == Some("alt1"))
        .expect("alt1 snapshot");
    let fork = alt1
        .get("forked_from")
        .unwrap_or_else(|| panic!("alt1.forked_from missing: {alt1}"));
    assert_eq!(
        fork.get("scenario").and_then(|v| v.as_str()),
        Some("main"),
        "fork origin must be main, got: {fork}"
    );
    assert_eq!(
        fork.get("at_step").and_then(|v| v.as_u64()),
        Some(2),
        "fork at_step must be 2, got: {fork}"
    );
}

#[tokio::test]
async fn get_scenarios_access_level_resolves_correctly() {
    let (client, port) = start().await;

    cmd(&client, port, "Staking", call("deposit")).await;

    let body = get_scenarios_all(&client, port).await;
    let scenarios = body
        .get("scenarios")
        .and_then(|v| v.as_array())
        .expect("scenarios array");
    let main_steps = scenarios[0]
        .get("steps")
        .and_then(|v| v.as_array())
        .expect("main steps array");
    let access = main_steps[0]
        .get("access")
        .expect("access field");
    assert_eq!(
        access.as_str(),
        Some("Public"),
        "deposit should be Public, got: {access}"
    );
}
