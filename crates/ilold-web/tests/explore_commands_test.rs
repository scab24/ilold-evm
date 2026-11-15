use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("tests/fixtures")
        .join(name)
}

#[derive(Debug, Clone)]
struct FlowEvent {
    variant: String,
    payload: serde_json::Value,
}

fn flatten_flow_tree(node: &serde_json::Value, out: &mut Vec<FlowEvent>) {
    if let Some(kind) = node.get("kind") {
        match kind {
            serde_json::Value::String(s) => {
                out.push(FlowEvent { variant: s.clone(), payload: serde_json::Value::Null });
            }
            serde_json::Value::Object(map) => {
                if let Some((variant, payload)) = map.iter().next() {
                    out.push(FlowEvent { variant: variant.clone(), payload: payload.clone() });
                }
            }
            _ => {}
        }
    }
    if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
        for child in children {
            flatten_flow_tree(child, out);
        }
    }
}

#[tokio::test]
async fn vars_endpoint_returns_state_variables() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();
    let res = client
        .get(format!("http://127.0.0.1:{port}/api/contract/Staking"))
        .send().await.unwrap();

    assert!(res.status().is_success());
    let body: serde_json::Value = res.json().await.unwrap();

    let vars = body["state_vars"].as_array().unwrap();
    assert!(!vars.is_empty(), "Should have state variables");

    let names: Vec<&str> = vars.iter()
        .filter_map(|v| v["name"].as_str())
        .collect();
    assert!(names.contains(&"balances"), "Should contain balances: {:?}", names);
    assert!(names.contains(&"totalStaked"), "Should contain totalStaked: {:?}", names);
    assert!(names.contains(&"rewardRate"), "Should contain rewardRate: {:?}", names);

    let balances = vars.iter().find(|v| v["name"] == "balances").unwrap();
    assert_eq!(balances["is_constant"], false);
    assert_eq!(balances["is_immutable"], false);
    assert!(balances["type_name"].as_str().is_some());
}

#[tokio::test]
async fn info_endpoint_returns_narrative_with_tree_data() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();

    client.post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({"contract": "Staking", "command": {"Call": {"func": "deposit"}}}))
        .send().await.unwrap();

    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/function/Staking/deposit"))
        .send().await.unwrap();

    assert!(res.status().is_success());
    let body: serde_json::Value = res.json().await.unwrap();

    assert_eq!(body["name"], "deposit");
    assert!(body["total_paths"].as_u64().unwrap() > 0);
    assert!(body["state_writes"].as_array().unwrap().len() > 0);
    assert!(body["modifiers"].as_array().unwrap().len() > 0);
    assert!(body["external_calls"].as_array().unwrap().len() > 0);
}

#[tokio::test]
async fn sequence_endpoint_with_two_steps() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();

    client.post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({"contract": "Staking", "command": {"Call": {"func": "deposit"}}}))
        .send().await.unwrap();
    client.post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({"contract": "Staking", "command": {"Call": {"func": "withdraw"}}}))
        .send().await.unwrap();

    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/sequence"))
        .send().await.unwrap();

    assert!(res.status().is_success());
    let body: serde_json::Value = res.json().await.unwrap();

    assert!(body["steps"].as_array().unwrap().len() >= 2);
    assert!(body["observations"].as_array().is_some());
}

#[tokio::test]
async fn sequence_with_one_step_returns_error() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();

    client.post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({"contract": "Staking", "command": {"Call": {"func": "deposit"}}}))
        .send().await.unwrap();

    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/sequence"))
        .send().await.unwrap();

    assert_eq!(res.status(), 400);
    let body = res.text().await.unwrap();
    assert!(body.contains("at least 2 steps"));
}

#[tokio::test]
async fn full_explore_workflow() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();
    let cmd = |body: serde_json::Value| {
        let c = client.clone();
        let p = port;
        async move {
            c.post(format!("http://127.0.0.1:{p}/api/cmd"))
                .json(&body)
                .send().await.unwrap()
                .json::<serde_json::Value>().await.unwrap()
        }
    };

    let res = cmd(serde_json::json!({"contract": "Staking", "command": "Functions"})).await;
    let funcs = res["FunctionList"]["functions"].as_array().unwrap();
    assert!(funcs.len() >= 8);

    let res = cmd(serde_json::json!({"contract": "Staking", "command": {"Call": {"func": "deposit"}}})).await;
    assert_eq!(res["StepAdded"]["function"], "deposit");
    assert_eq!(res["StepAdded"]["step_index"], 0);

    let res = cmd(serde_json::json!({"contract": "Staking", "command": "State"})).await;
    let summary = res["StateView"]["summary"].as_array().unwrap();
    assert!(!summary.is_empty());

    let res = cmd(serde_json::json!({"contract": "Staking", "command": {"Who": {"variable": "balances"}}})).await;
    let writers = res["VariableInfo"]["writers"].as_array().unwrap();
    assert!(writers.len() >= 2, "balances should have at least 2 writers");

    let res = cmd(serde_json::json!({"contract": "Staking", "command": {"Call": {"func": "withdraw"}}})).await;
    assert_eq!(res["StepAdded"]["function"], "withdraw");
    assert_eq!(res["StepAdded"]["step_index"], 1);

    let res = cmd(serde_json::json!({"contract": "Staking", "command": "Back"})).await;
    assert_eq!(res["StepRemoved"]["remaining"], 1);

    let res = cmd(serde_json::json!({"contract": "Staking", "command": "Session"})).await;
    assert_eq!(res["SessionView"]["contract"], "Staking");
    let steps = res["SessionView"]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1);

    let res = cmd(serde_json::json!({"contract": "Staking", "command": "Clear"})).await;
    assert_eq!(res, serde_json::json!("Cleared"));

    let res = cmd(serde_json::json!({"contract": "Staking", "command": "Session"})).await;
    assert_eq!(res["SessionView"]["steps"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn trace_swap_shows_external_call_before_update_writes() {
    let paths = vec![fixture("uniswap_v2_pair")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();
    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/trace/UniswapV2Pair/swap?depth=4"))
        .send().await.unwrap();

    assert!(res.status().is_success(), "trace endpoint failed: {}", res.status());
    let tree: serde_json::Value = res.json().await.unwrap();

    assert_eq!(tree["contract"], "UniswapV2Pair");
    assert_eq!(tree["function"], "swap");
    assert_eq!(tree["max_depth"], 4);
    let modifiers = tree["modifiers"].as_array().unwrap();
    assert!(modifiers.iter().any(|m| m == "lock"));

    let mut events: Vec<FlowEvent> = Vec::new();
    flatten_flow_tree(&tree["root"], &mut events);

    let first_external = events.iter().position(|e| {
        e.variant == "ExternalCall"
            && e.payload.get("function").and_then(|v| v.as_str()) == Some("transfer")
    });
    assert!(first_external.is_some(), "Expected IERC20.transfer ExternalCall");

    assert!(
        events.iter().any(|e| {
            e.variant == "InternalCall"
                && e.payload.get("function").and_then(|v| v.as_str()) == Some("_update")
        }),
        "Expected _update as InternalCall",
    );

    assert!(
        events.iter().any(|e| {
            e.variant == "Write"
                && e.payload.get("target").and_then(|v| v.as_str()) == Some("reserve0")
        }),
        "Expected inlined write to reserve0",
    );

    let ext_idx = first_external.unwrap();
    assert!(
        events[ext_idx..].iter().any(|e| {
            e.variant == "Write"
                && e.payload.get("target").and_then(|v| v.as_str()) == Some("reserve0")
        }),
        "Expected reserve0 write AFTER first transfer (CEI). first_external={}",
        ext_idx,
    );
}

#[tokio::test]
async fn trace_getreserves_has_no_empty_target_writes() {
    let paths = vec![fixture("uniswap_v2_pair")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();
    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/trace/UniswapV2Pair/getReserves"))
        .send().await.unwrap();

    assert!(res.status().is_success());
    let tree: serde_json::Value = res.json().await.unwrap();

    let mut events: Vec<FlowEvent> = Vec::new();
    flatten_flow_tree(&tree["root"], &mut events);

    let write_count = events.iter().filter(|e| e.variant == "Write").count();
    assert_eq!(write_count, 0, "getReserves should emit no Write events");
}


#[tokio::test]
async fn trace_update_has_no_internal_calls_and_shows_state_writes() {
    let paths = vec![fixture("uniswap_v2_pair")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();
    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/trace/UniswapV2Pair/_update"))
        .send().await.unwrap();

    assert!(res.status().is_success());
    let tree: serde_json::Value = res.json().await.unwrap();

    let mut events: Vec<FlowEvent> = Vec::new();
    flatten_flow_tree(&tree["root"], &mut events);

    let has_write_to = |name: &str| {
        events.iter().any(|e| {
            e.variant == "Write"
                && e.payload.get("target").and_then(|v| v.as_str()) == Some(name)
        })
    };

    assert!(has_write_to("reserve0"), "expected write to reserve0");
    assert!(has_write_to("reserve1"), "expected write to reserve1");
    assert!(events.iter().any(|e| e.variant == "EmitEvent"));
    assert!(!events.iter().any(|e| e.variant == "InternalCall"));
}

fn collect_step_ids(node: &serde_json::Value, out: &mut Vec<(u64, String)>) {
    let step_id = node.get("step_id").and_then(|v| v.as_u64()).unwrap_or(0);
    let variant = match node.get("kind") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Object(map)) => {
            map.keys().next().cloned().unwrap_or_default()
        }
        _ => String::new(),
    };
    out.push((step_id, variant));
    if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
        for child in children {
            collect_step_ids(child, out);
        }
    }
}

#[tokio::test]
async fn step_ids_are_stable_across_max_depth_configs() {
    let paths = vec![fixture("uniswap_v2_pair")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();
    let get_tree = |depth: usize| {
        let client = client.clone();
        async move {
            let res = client
                .get(format!(
                    "http://127.0.0.1:{port}/api/session/trace/UniswapV2Pair/swap?depth={depth}"
                ))
                .send().await.unwrap();
            assert!(res.status().is_success());
            res.json::<serde_json::Value>().await.unwrap()
        }
    };

    let tree_shallow = get_tree(2).await;
    let tree_deep = get_tree(4).await;

    let mut shallow: Vec<(u64, String)> = Vec::new();
    collect_step_ids(&tree_shallow["root"], &mut shallow);
    let mut deep: Vec<(u64, String)> = Vec::new();
    collect_step_ids(&tree_deep["root"], &mut deep);

    assert!(shallow.len() > 5, "shallow tree too small: {}", shallow.len());
    assert!(deep.len() > shallow.len(),
        "deep tree should have at least as many nodes as shallow (deep={}, shallow={})",
        deep.len(), shallow.len());

    let shallow_map: std::collections::HashMap<u64, &str> =
        shallow.iter().map(|(id, v)| (*id, v.as_str())).collect();
    let deep_map: std::collections::HashMap<u64, &str> =
        deep.iter().map(|(id, v)| (*id, v.as_str())).collect();

    let mut common_ids = 0usize;
    for (id, shallow_variant) in &shallow_map {
        if let Some(deep_variant) = deep_map.get(id) {
            assert_eq!(
                shallow_variant, deep_variant,
                "step_id {} has different variants: shallow={:?}, deep={:?}",
                id, shallow_variant, deep_variant,
            );
            common_ids += 1;
        }
    }
    assert!(common_ids > 5, "expected meaningful overlap, got {}", common_ids);
}

#[tokio::test]
async fn session_step_persists_flow_tree_with_populated_flow_step_ids() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();

    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({"contract": "Staking", "command": {"Call": {"func": "deposit"}}}))
        .send().await.unwrap();
    assert!(res.status().is_success());

    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({"contract": "Staking", "command": "SaveSession"}))
        .send().await.unwrap();
    assert!(res.status().is_success());
    let body: serde_json::Value = res.json().await.unwrap();
    let json_str = body["SessionSaved"]["json"].as_str()
        .expect("SaveSession should return a JSON string");
    let session: serde_json::Value = serde_json::from_str(json_str).unwrap();

    assert_eq!(session["version"], 2);
    let main = &session["scenarios"]["main"];
    assert!(!main.is_null(), "scenarios.main must be present in v2 save");
    let steps = main["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1);
    let step = &steps[0];

    assert!(
        !step["flow_tree"].is_null(),
        "step.flow_tree must be persisted (not null) after c <func>"
    );
    let flow_tree = &step["flow_tree"];
    assert_eq!(flow_tree["function"], "deposit");
    let root = &flow_tree["root"];
    assert!(root["children"].as_array().unwrap().len() > 0,
        "persisted flow_tree must have non-empty root.children");

    assert_eq!(step["trace_config"]["depth"], 2);

    let mutations = step["mutations"].as_array().unwrap();
    assert!(!mutations.is_empty(), "deposit should produce at least one mutation");
    for m in mutations {
        assert!(
            !m["flow_step_id"].is_null(),
            "mutation {:?} must have flow_step_id populated",
            m
        );
    }

    let mut tree_step_ids: std::collections::HashMap<u64, String> = std::collections::HashMap::new();
    collect_step_id_kinds(root, &mut tree_step_ids);
    for m in mutations {
        let id = m["flow_step_id"].as_u64().unwrap();
        let variant = tree_step_ids.get(&id)
            .unwrap_or_else(|| panic!("flow_step_id {} not found in persisted tree", id));
        assert!(
            variant == "Write" || variant == "StateWrite",
            "flow_step_id {} resolves to {}, expected Write or StateWrite",
            id, variant,
        );
    }
}

#[tokio::test]
async fn call_rejects_internal_function_as_session_entry() {
    let paths = vec![fixture("uniswap_v2_pair")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();
    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({
            "contract": "UniswapV2Pair",
            "command": {"Call": {"func": "_update"}}
        }))
        .send().await.unwrap();
    assert!(res.status().is_success(), "request should succeed — the error lives in the payload");

    let body: serde_json::Value = res.json().await.unwrap();
    let msg = body["Error"]["message"].as_str()
        .expect("expected CommandResult::Error for internal function");
    assert!(msg.contains("_update"), "error should name the function: {}", msg);
    assert!(msg.contains("internal") || msg.contains("private"),
        "error should explain why: {}", msg);
    assert!(msg.contains("tr") || msg.contains("view"),
        "error should suggest an alternative: {}", msg);

    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({
            "contract": "UniswapV2Pair",
            "command": {"Call": {"func": "swap"}}
        }))
        .send().await.unwrap();
    assert!(res.status().is_success());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["StepAdded"]["function"], "swap");
}

#[tokio::test]
async fn tr_swap_expand_inlines_update() {
    let paths = vec![fixture("uniswap_v2_pair")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();

    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/trace/UniswapV2Pair/swap?depth=2"))
        .send().await.unwrap();
    assert!(res.status().is_success());
    let tree: serde_json::Value = res.json().await.unwrap();

    let mut update_step_id: Option<u64> = None;
    fn find_depth_limited_update(node: &serde_json::Value, out: &mut Option<u64>) {
        if out.is_some() { return; }
        if let Some(obj) = node.get("kind").and_then(|v| v.as_object()) {
            if let Some(ic) = obj.get("InternalCall") {
                let is_update = ic.get("function").and_then(|v| v.as_str()) == Some("_update");
                let is_limited = ic.get("depth_limited").and_then(|v| v.as_bool()) == Some(true);
                if is_update && is_limited {
                    if let Some(id) = node.get("step_id").and_then(|v| v.as_u64()) {
                        *out = Some(id);
                        return;
                    }
                }
            }
        }
        if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
            for child in children {
                find_depth_limited_update(child, out);
                if out.is_some() { return; }
            }
        }
    }
    find_depth_limited_update(&tree["root"], &mut update_step_id);
    let target_id = update_step_id
        .expect("expected at least one depth_limited _update at depth=2");

    let res = client
        .get(format!(
            "http://127.0.0.1:{port}/api/session/trace/UniswapV2Pair/swap?depth=2&expand={target_id}"
        ))
        .send().await.unwrap();
    assert!(res.status().is_success());
    let tree2: serde_json::Value = res.json().await.unwrap();

    fn find_node_by_id<'a>(
        node: &'a serde_json::Value,
        target: u64,
    ) -> Option<&'a serde_json::Value> {
        if node.get("step_id").and_then(|v| v.as_u64()) == Some(target) {
            return Some(node);
        }
        if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
            for child in children {
                if let Some(found) = find_node_by_id(child, target) {
                    return Some(found);
                }
            }
        }
        None
    }
    let expanded_node = find_node_by_id(&tree2["root"], target_id)
        .expect("step_id should still exist after expand (canonical step_ids are stable)");

    let kind = expanded_node["kind"].as_object().unwrap();
    let ic = kind.get("InternalCall")
        .expect("expanded node should still be an InternalCall");
    assert_eq!(
        ic.get("depth_limited").and_then(|v| v.as_bool()),
        Some(false),
        "step {} should be expanded (not depth_limited) after ?expand={}",
        target_id, target_id
    );

    let children = expanded_node["children"].as_array().unwrap();
    assert!(!children.is_empty(),
        "expanded _update must have inlined children");

    let has_reserve_write = children.iter().any(|c| {
        c.get("kind")
            .and_then(|k| k.as_object())
            .and_then(|m| m.get("Write"))
            .and_then(|w| w.get("target"))
            .and_then(|t| t.as_str())
            .map(|s| s.starts_with("reserve"))
            .unwrap_or(false)
    });
    assert!(has_reserve_write,
        "expanded _update children should include reserve* writes");
}

#[tokio::test]
async fn timeline_balances_across_multiple_steps() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();
    let post_call = |func: &'static str| {
        let client = client.clone();
        async move {
            client
                .post(format!("http://127.0.0.1:{port}/api/cmd"))
                .json(&serde_json::json!({"contract": "Staking", "command": {"Call": {"func": func}}}))
                .send().await.unwrap()
        }
    };

    assert!(post_call("deposit").await.status().is_success());
    assert!(post_call("withdraw").await.status().is_success());

    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/timeline/balances"))
        .send().await.unwrap();
    assert!(res.status().is_success(), "endpoint failed: {}", res.status());

    let tl: serde_json::Value = res.json().await.unwrap();
    assert_eq!(tl["variable"], "balances");

    let state = tl["state_entries"].as_array().unwrap();
    assert!(state.len() >= 2,
        "expected at least 2 entries (deposit + withdraw), got {}", state.len());

    let locals = tl["local_entries"].as_array().unwrap();
    assert!(locals.is_empty());

    let mut prev_key: Option<(u64, u64)> = None;
    for entry in state {
        let session_idx = entry["session_step_index"].as_u64().unwrap();
        let flow_id = entry["flow_step_id"].as_u64().unwrap_or(u64::MAX);
        let key = (session_idx, flow_id);
        if let Some(p) = prev_key {
            assert!(key >= p, "timeline entries not sorted: {:?} after {:?}", key, p);
        }
        prev_key = Some(key);

        assert!(!entry["function"].as_str().unwrap().is_empty());
        assert!(entry["flow_step_id"].as_u64().is_some(),
            "non-legacy mutation should have flow_step_id");
        let target = entry["target"].as_str().unwrap();
        assert!(target.starts_with("balances"),
            "target should be a balances variant, got {:?}", target);
    }
}

#[tokio::test]
async fn legacy_session_loads_and_degrades_gracefully() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();

    for func in ["deposit", "withdraw"] {
        let res = client
            .post(format!("http://127.0.0.1:{port}/api/cmd"))
            .json(&serde_json::json!({"contract": "Staking", "command": {"Call": {"func": func}}}))
            .send().await.unwrap();
        assert!(res.status().is_success());
    }
    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({"contract": "Staking", "command": "SaveSession"}))
        .send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    let json_str = body["SessionSaved"]["json"].as_str().unwrap().to_string();

    let v2: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    let mut session = v2["scenarios"]["main"].clone();
    assert!(!session.is_null(), "v2 save must contain scenarios.main");
    {
        let steps = session["steps"].as_array_mut().unwrap();
        for step in steps {
            let obj = step.as_object_mut().unwrap();
            obj.remove("flow_tree");
            obj.remove("trace_config");
            if let Some(muts) = obj.get_mut("mutations").and_then(|v| v.as_array_mut()) {
                for m in muts {
                    if let Some(m_obj) = m.as_object_mut() {
                        m_obj.remove("flow_step_id");
                        m_obj.remove("scope");
                    }
                }
            }
        }
    }
    let stripped = serde_json::to_string(&session).unwrap();

    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({
            "contract": "Staking",
            "command": {"LoadSession": {"json": stripped}}
        }))
        .send().await.unwrap();
    assert!(res.status().is_success(), "load failed: {}", res.status());

    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({"contract": "Staking", "command": "State"}))
        .send().await.unwrap();
    assert!(res.status().is_success());
    let body: serde_json::Value = res.json().await.unwrap();
    let summary = body["StateView"]["summary"].as_array().unwrap();
    assert!(!summary.is_empty());
    for var in summary {
        for change in var["changes"].as_array().unwrap() {
            let s = change.as_str().unwrap();
            assert!(s.contains("(step "), "missing step ref: {:?}", s);
            assert!(
                !s.contains(":") || !s.split("step ").nth(1).map(|r| r.starts_with(|c: char| c.is_ascii_digit())).unwrap_or(false)
                || s.split("(step ").nth(1).and_then(|r| r.split(',').next())
                    .map(|n| !n.contains(':')).unwrap_or(true),
                "legacy mutation should not have flow_step ref: {:?}", s
            );
        }
    }

    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/sequence"))
        .send().await.unwrap();
    assert!(res.status().is_success());
    let narrative: serde_json::Value = res.json().await.unwrap();
    let steps = narrative["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);
    for step in steps {
        assert!(step["flow_summary"].is_null(),
            "legacy step should have null flow_summary: {:?}", step);
    }

    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/step/0/trace"))
        .send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::NOT_FOUND);
    let err = res.text().await.unwrap();
    assert!(
        err.contains("no persisted trace") || err.contains("pre-Phase-2a"),
        "expected legacy-session error message, got: {:?}", err
    );
}

#[tokio::test]
async fn sequence_narrative_includes_flow_summary_per_step() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();
    let post_call = |func: &'static str| {
        let client = client.clone();
        async move {
            client
                .post(format!("http://127.0.0.1:{port}/api/cmd"))
                .json(&serde_json::json!({"contract": "Staking", "command": {"Call": {"func": func}}}))
                .send().await.unwrap()
        }
    };

    assert!(post_call("deposit").await.status().is_success());
    assert!(post_call("withdraw").await.status().is_success());

    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/sequence"))
        .send().await.unwrap();
    assert!(res.status().is_success());
    let narrative: serde_json::Value = res.json().await.unwrap();

    let steps = narrative["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);

    for step in steps {
        let summary = &step["flow_summary"];
        assert!(!summary.is_null(),
            "step {:?} has null flow_summary; should have been populated by get_sequence_narrative",
            step["function"]);
        assert!(summary["total_steps"].as_u64().unwrap() > 0);
        assert!(summary["mutation_count"].as_u64().unwrap() > 0,
            "expected at least one mutation in {:?}", step["function"]);
        let refs = summary["mutation_refs"].as_array().unwrap();
        assert_eq!(refs.len() as u64, summary["mutation_count"].as_u64().unwrap());
        for r in refs {
            assert!(!r["variable"].as_str().unwrap().is_empty());
            assert!(r["flow_step_id"].as_u64().is_some());
        }
    }
}

#[tokio::test]
async fn tr_step_endpoint_returns_persisted_flow_tree() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();

    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({"contract": "Staking", "command": {"Call": {"func": "deposit"}}}))
        .send().await.unwrap();
    assert!(res.status().is_success());

    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/step/0/trace"))
        .send().await.unwrap();
    assert!(res.status().is_success(), "endpoint failed: {}", res.status());

    let tree: serde_json::Value = res.json().await.unwrap();
    assert_eq!(tree["function"], "deposit");
    assert_eq!(tree["max_depth"], 2);
    let root = &tree["root"];
    assert!(root["children"].as_array().unwrap().len() > 0);
}

#[tokio::test]
async fn tr_step_endpoint_404_on_unknown_step() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();

    let res = client
        .get(format!("http://127.0.0.1:{port}/api/session/step/0/trace"))
        .send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn state_view_renders_flow_step_refs() {
    let paths = vec![fixture("staking")];
    let (_, port) = ilold_web::start_server(paths, 0, 2).await.unwrap();

    let client = reqwest::Client::new();

    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({"contract": "Staking", "command": {"Call": {"func": "deposit"}}}))
        .send().await.unwrap();
    assert!(res.status().is_success());

    let res = client
        .post(format!("http://127.0.0.1:{port}/api/cmd"))
        .json(&serde_json::json!({"contract": "Staking", "command": "State"}))
        .send().await.unwrap();
    assert!(res.status().is_success());
    let body: serde_json::Value = res.json().await.unwrap();
    let summary = body["StateView"]["summary"].as_array().unwrap();
    assert!(!summary.is_empty(), "deposit should produce state changes");

    for var in summary {
        let changes = var["changes"].as_array().unwrap();
        for change in changes {
            let s = change.as_str().unwrap();
            let has_ref = s.split("step 0:").nth(1)
                .map(|rest| rest.chars().next().is_some_and(|c| c.is_ascii_digit()))
                .unwrap_or(false);
            assert!(
                has_ref,
                "change line missing 'step 0:N' flow ref: {:?}",
                s
            );
        }
    }
}

fn collect_step_id_kinds(
    node: &serde_json::Value,
    out: &mut std::collections::HashMap<u64, String>,
) {
    if let Some(id) = node.get("step_id").and_then(|v| v.as_u64()) {
        let variant = match node.get("kind") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Object(map)) => {
                map.keys().next().cloned().unwrap_or_default()
            }
            _ => String::new(),
        };
        out.insert(id, variant);
    }
    if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
        for child in children {
            collect_step_id_kinds(child, out);
        }
    }
}
