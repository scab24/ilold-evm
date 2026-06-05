use httpmock::Method::{GET, POST};
use httpmock::MockServer;
use ilold_evm_mcp::{dispatch, IloldClient};
use serde_json::json;
use tokio::sync::Mutex;

#[tokio::test]
async fn each_tool_routes_to_its_endpoint() {
    let server = MockServer::start();
    let overview = server.mock(|w, t| {
        w.method(GET).path("/api/project");
        t.status(200).json_body(json!({ "files": 1, "contracts": [] }));
    });
    let map = server.mock(|w, t| {
        w.method(GET).path("/api/project/map");
        t.status(200).json_body(json!({ "contracts": [], "relationships": [] }));
    });
    let project_dep = server.mock(|w, t| {
        w.method(GET).path("/api/project/depgraph");
        t.status(200).json_body(json!({ "nodes": [], "edges": [] }));
    });
    let contract_dep = server.mock(|w, t| {
        w.method(GET).path("/api/contract/Vault/depgraph");
        t.status(200).json_body(json!({ "nodes": [], "edges": [] }));
    });

    let client = IloldClient::new(server.base_url());
    let current = Mutex::new(None::<String>);

    assert!(!dispatch(&client, &current, "ilold_project_overview", None).await.is_error.unwrap_or(false));
    overview.assert();

    assert!(!dispatch(&client, &current, "ilold_project_map", None).await.is_error.unwrap_or(false));
    map.assert();

    assert!(!dispatch(&client, &current, "ilold_dependency_graph", None).await.is_error.unwrap_or(false));
    project_dep.assert();

    let args = json!({ "contract": "Vault" });
    assert!(!dispatch(&client, &current, "ilold_contract_dependencies", Some(&args)).await.is_error.unwrap_or(false));
    contract_dep.assert();
}

#[tokio::test]
async fn contract_dependencies_requires_contract() {
    let server = MockServer::start();
    let dep = server.mock(|w, t| {
        w.method(GET).path("/api/contract/Vault/depgraph");
        t.status(200).json_body(json!({}));
    });
    let client = IloldClient::new(server.base_url());
    let current = Mutex::new(None::<String>);

    let res = dispatch(&client, &current, "ilold_contract_dependencies", None).await;
    assert_eq!(res.is_error, Some(true), "missing contract must error");
    assert_eq!(dep.hits(), 0, "no HTTP call when the required arg is absent");
}

#[tokio::test]
async fn unknown_tool_errors() {
    let server = MockServer::start();
    let client = IloldClient::new(server.base_url());
    let current = Mutex::new(None::<String>);
    let res = dispatch(&client, &current, "ilold_does_not_exist", None).await;
    assert_eq!(res.is_error, Some(true));
}

#[tokio::test]
async fn p2_get_tools_route_to_endpoints() {
    let server = MockServer::start();
    let detail = server.mock(|w, t| {
        w.method(GET).path("/api/contract/Vault");
        t.status(200).json_body(json!({}));
    });
    let narrative = server.mock(|w, t| {
        w.method(GET).path("/api/session/function/Vault/deposit");
        t.status(200).json_body(json!({}));
    });
    let trace = server.mock(|w, t| {
        w.method(GET)
            .path("/api/session/trace/Vault/deposit")
            .query_param("depth", "3")
            .query_param("reverts", "true");
        t.status(200).json_body(json!({}));
    });
    let client = IloldClient::new(server.base_url());
    let current = Mutex::new(None::<String>);

    dispatch(&client, &current, "ilold_contract_detail", Some(&json!({"contract":"Vault"}))).await;
    detail.assert();
    dispatch(&client, &current, "ilold_function_analysis", Some(&json!({"contract":"Vault","function":"deposit"}))).await;
    narrative.assert();
    dispatch(&client, &current, "ilold_trace", Some(&json!({"contract":"Vault","function":"deposit","depth":3,"reverts":true}))).await;
    trace.assert();
}

#[tokio::test]
async fn p2_post_tools_send_exact_command() {
    let server = MockServer::start();
    let entry = server.mock(|w, t| {
        w.method(POST)
            .path("/api/cmd")
            .json_body(json!({ "contract": "Vault", "command": "Functions" }));
        t.status(200).json_body(json!({ "FunctionList": { "functions": [] } }));
    });
    let who = server.mock(|w, t| {
        w.method(POST)
            .path("/api/cmd")
            .json_body(json!({ "contract": "Vault", "command": { "Who": { "variable": "total" } } }));
        t.status(200).json_body(json!({ "VariableInfo": {} }));
    });
    let client = IloldClient::new(server.base_url());
    let current = Mutex::new(None::<String>);

    dispatch(&client, &current, "ilold_entry_points", Some(&json!({"contract":"Vault"}))).await;
    entry.assert();
    dispatch(&client, &current, "ilold_who_touches", Some(&json!({"contract":"Vault","variable":"total"}))).await;
    who.assert();
}

#[tokio::test]
async fn p2_missing_required_args_error() {
    let server = MockServer::start();
    let client = IloldClient::new(server.base_url());
    let current = Mutex::new(None::<String>);
    let no_func = dispatch(&client, &current, "ilold_function_analysis", Some(&json!({"contract":"Vault"}))).await;
    assert_eq!(no_func.is_error, Some(true), "function_analysis needs function");
    let no_var = dispatch(&client, &current, "ilold_who_touches", Some(&json!({"contract":"Vault"}))).await;
    assert_eq!(no_var.is_error, Some(true), "who_touches needs variable");
}

#[tokio::test]
async fn p3_use_validates_then_sets_active() {
    let server = MockServer::start();
    let detail = server.mock(|w, t| {
        w.method(GET).path("/api/contract/Vault");
        t.status(200).json_body(json!({ "name": "Vault" }));
    });
    let session = server.mock(|w, t| {
        w.method(POST)
            .path("/api/cmd")
            .json_body(json!({ "contract": "Vault", "command": "Session" }));
        t.status(200).json_body(json!({ "SessionView": {} }));
    });
    let client = IloldClient::new(server.base_url());
    let current = Mutex::new(None::<String>);

    let res = dispatch(&client, &current, "ilold_use", Some(&json!({"contract":"Vault"}))).await;
    assert_ne!(res.is_error, Some(true), "use must succeed");
    detail.assert();
    session.assert();
    assert_eq!(*current.lock().await, Some("Vault".to_string()), "active contract set");
}

#[tokio::test]
async fn p3_use_rejects_unknown_contract() {
    let server = MockServer::start();
    let detail = server.mock(|w, t| {
        w.method(GET).path("/api/contract/Ghost");
        t.status(404).body("not found");
    });
    let session = server.mock(|w, t| {
        w.method(POST).path("/api/cmd");
        t.status(200).json_body(json!({}));
    });
    let client = IloldClient::new(server.base_url());
    let current = Mutex::new(None::<String>);

    let res = dispatch(&client, &current, "ilold_use", Some(&json!({"contract":"Ghost"}))).await;
    assert_eq!(res.is_error, Some(true), "unknown contract must error");
    detail.assert();
    assert_eq!(session.hits(), 0, "no Session command for an invalid contract");
    assert_eq!(*current.lock().await, None, "active contract unchanged on error");
}

#[tokio::test]
async fn p3_stateful_tools_require_active_contract() {
    let server = MockServer::start();
    let any = server.mock(|w, t| {
        w.method(POST).path("/api/cmd");
        t.status(200).json_body(json!({}));
    });
    let client = IloldClient::new(server.base_url());
    let current = Mutex::new(None::<String>);

    for tool in ["ilold_session_state", "ilold_session_back", "ilold_session_clear", "ilold_export"] {
        let res = dispatch(&client, &current, tool, None).await;
        assert_eq!(res.is_error, Some(true), "{tool} must require an active contract");
    }
    let call = dispatch(&client, &current, "ilold_session_call", Some(&json!({"function":"deposit"}))).await;
    assert_eq!(call.is_error, Some(true), "session_call must require an active contract");
    assert_eq!(any.hits(), 0, "no HTTP call without an active contract");
}

#[tokio::test]
async fn p3_session_and_findings_send_exact_commands() {
    let server = MockServer::start();
    let call = server.mock(|w, t| {
        w.method(POST).path("/api/cmd")
            .json_body(json!({ "contract": "Vault", "command": { "Call": { "func": "deposit" } } }));
        t.status(200).json_body(json!({ "StepAdded": {} }));
    });
    let finding = server.mock(|w, t| {
        w.method(POST).path("/api/cmd")
            .json_body(json!({ "contract": "Vault", "command": { "Finding": { "severity": "High", "title": "reentrancy", "description": "in withdraw" } } }));
        t.status(200).json_body(json!({ "FindingAdded": { "id": "1" } }));
    });
    let note = server.mock(|w, t| {
        w.method(POST).path("/api/cmd")
            .json_body(json!({ "contract": "Vault", "command": { "Note": { "text": "check guard" } } }));
        t.status(200).json_body(json!({ "NoteAdded": null }));
    });
    let status = server.mock(|w, t| {
        w.method(POST).path("/api/cmd")
            .json_body(json!({ "contract": "Vault", "command": { "Status": { "func": "withdraw", "status": "Vulnerable" } } }));
        t.status(200).json_body(json!({ "StatusUpdated": null }));
    });
    let client = IloldClient::new(server.base_url());
    let current = Mutex::new(Some("Vault".to_string()));

    dispatch(&client, &current, "ilold_session_call", Some(&json!({"function":"deposit"}))).await;
    call.assert();
    dispatch(&client, &current, "ilold_record_finding", Some(&json!({"severity":"High","title":"reentrancy","description":"in withdraw"}))).await;
    finding.assert();
    dispatch(&client, &current, "ilold_note", Some(&json!({"text":"check guard"}))).await;
    note.assert();
    dispatch(&client, &current, "ilold_set_status", Some(&json!({"function":"withdraw","status":"Vulnerable"}))).await;
    status.assert();
}

#[tokio::test]
async fn p3_timeline_and_slice_route() {
    let server = MockServer::start();
    let timeline = server.mock(|w, t| {
        w.method(GET).path("/api/session/timeline/total");
        t.status(200).json_body(json!({ "mutations": [] }));
    });
    let slice = server.mock(|w, t| {
        w.method(GET).path("/api/session/slice/deposit/total")
            .query_param("direction", "forward");
        t.status(200).json_body(json!({ "lines": [] }));
    });
    let client = IloldClient::new(server.base_url());
    let current = Mutex::new(Some("Vault".to_string()));

    dispatch(&client, &current, "ilold_timeline", Some(&json!({"variable":"total"}))).await;
    timeline.assert();
    dispatch(&client, &current, "ilold_slice", Some(&json!({"function":"deposit","variable":"total","direction":"forward"}))).await;
    slice.assert();
}

#[tokio::test]
async fn p3_slice_requires_active_contract() {
    let server = MockServer::start();
    let slice = server.mock(|w, t| {
        w.method(GET).path("/api/session/slice/deposit/total");
        t.status(200).json_body(json!({}));
    });
    let client = IloldClient::new(server.base_url());
    let current = Mutex::new(None::<String>);

    let res = dispatch(&client, &current, "ilold_slice", Some(&json!({"function":"deposit","variable":"total"}))).await;
    assert_eq!(res.is_error, Some(true), "slice must require an active contract");
    assert_eq!(slice.hits(), 0, "no slice request without an active contract");
}

#[tokio::test]
async fn p3_command_result_error_surfaces_as_mcp_error() {
    let server = MockServer::start();
    let cmd = server.mock(|w, t| {
        w.method(POST).path("/api/cmd");
        t.status(200).json_body(json!({ "Error": { "message": "Function 'foo' not found" } }));
    });
    let client = IloldClient::new(server.base_url());
    let current = Mutex::new(Some("Vault".to_string()));

    let res = dispatch(&client, &current, "ilold_session_call", Some(&json!({"function":"foo"}))).await;
    cmd.assert();
    assert_eq!(res.is_error, Some(true), "backend CommandResult::Error (HTTP 200) must surface as an MCP error");
}

#[tokio::test]
async fn trace_forwards_expand_step_ids() {
    let server = MockServer::start();
    let trace = server.mock(|w, t| {
        w.method(GET)
            .path("/api/session/trace/Vault/deposit")
            .query_param("expand", "17,24");
        t.status(200).json_body(json!({}));
    });
    let client = IloldClient::new(server.base_url());
    let current = Mutex::new(None::<String>);

    dispatch(&client, &current, "ilold_trace", Some(&json!({"contract":"Vault","function":"deposit","expand":"17,24"}))).await;
    trace.assert();
}
