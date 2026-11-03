use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap().join("tests/fixtures/staking")
}

#[tokio::test(flavor = "current_thread")]
async fn embedded_ui_and_api_share_dynamic_port_on_single_worker() {
    let (_, port) = ilold_web::start_server(vec![fixture()], 0, 2).await.unwrap();
    assert_ne!(port, 0);
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");
    let root = client.get(&base).send().await.unwrap();
    assert_eq!(root.status(), 200);
    assert!(root.headers()["content-type"].to_str().unwrap().contains("text/html"));
    let html = root.text().await.unwrap();
    assert!(html.contains("/_app/immutable/"), "expected real Svelte entry: {html}");
    let js = html.split(['\"', '\'']).find(|part| part.starts_with("/_app/") && part.ends_with(".js"))
        .expect("HTML must reference built JS");
    let response = client.get(format!("{base}{js}")).send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.headers()["content-type"].to_str().unwrap().contains("javascript"));
    assert!(!response.bytes().await.unwrap().is_empty());
    let css_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("frontend/build/_app/immutable/assets");
    let css = std::fs::read_dir(css_dir).unwrap().filter_map(Result::ok)
        .find(|entry| entry.path().extension().is_some_and(|ext| ext == "css"))
        .expect("built CSS asset");
    let response = client.get(format!("{base}/_app/immutable/assets/{}", css.file_name().to_string_lossy()))
        .send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.headers()["content-type"].to_str().unwrap().contains("text/css"));
    assert!(!response.bytes().await.unwrap().is_empty());
    for route in ["/contract/Staking", "/contract/Staking/"] {
        let page = client.get(format!("{base}{route}")).send().await.unwrap();
        assert_eq!(page.status(), 200, "{route}");
        assert!(page.text().await.unwrap().contains("/_app/immutable/"), "{route}");
    }
    for path in [
        "/api/no-such-route", "/api/no-such-route/", "/ws/no-such-route", "/_app/immutable/missing.js",
        "/contract/", "/contract/Staking/extra", "/contract/missing.js",
    ] {
        let response = client.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(response.status(), 404, "{path}");
        assert!(!response.text().await.unwrap().contains("/_app/immutable/"));
    }
    let response = client.get(format!("{base}/api/project")).send().await.unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn occupied_port_is_reported() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let result = ilold_web::start_server(vec![fixture()], port, 2).await;
    match result {
        Err(error) => assert_eq!(
            error.downcast_ref::<std::io::Error>().map(std::io::Error::kind),
            Some(std::io::ErrorKind::AddrInUse),
            "expected a port binding error, got: {error:#}"
        ),
        Ok(_) => panic!("server unexpectedly started on an occupied port"),
    }
    drop(listener);
}

#[tokio::test]
async fn binds_loopback_by_default_and_all_interfaces_on_request() {
    let (state, port) = ilold_web::start_server(vec![fixture()], 0, 2).await.unwrap();
    assert!(state.addr.ip().is_loopback(), "{}", state.addr);
    assert_eq!(state.addr.port(), port);
    assert_eq!(ilold_web::local_url(state.addr), format!("http://127.0.0.1:{port}"));

    let all = std::net::SocketAddr::from(([0, 0, 0, 0], 0));
    let (state, port, _server) = ilold_web::start_server_with_handle(vec![fixture()], all, 2).await.unwrap();
    assert!(state.addr.ip().is_unspecified(), "{}", state.addr);
    let response = reqwest::get(format!("{}/api/project", ilold_web::local_url(state.addr))).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(ilold_web::local_url(state.addr), format!("http://127.0.0.1:{port}"));
}
