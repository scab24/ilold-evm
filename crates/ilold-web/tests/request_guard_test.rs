use std::path::PathBuf;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap().join("tests/fixtures/staking")
}

const FOREIGN: &str = "http://evil.example";
const COMMAND: &str = r#"{"contract":"Staking","command":{"Call":{"func":"deposit"}}}"#;

// Raw requests, because HTTP clients do not let a test forge Host the way a rebinding browser does.
async fn status(port: u16, method: &str, path: &str, host: &str, extra: &[(&str, &str)], body: &str) -> u16 {
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\n", body.len());
    if !body.is_empty() {
        request.push_str("Content-Type: application/json\r\n");
    }
    for (name, value) in extra {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n") {
        let read = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut byte)).await.unwrap().unwrap();
        assert_eq!(read, 1, "connection closed before the status line");
        head.push(byte[0]);
    }
    String::from_utf8(head).unwrap().split_whitespace().nth(1).unwrap().parse().unwrap()
}

async fn upgrade(port: u16, path: &str, origin: Option<&str>) -> u16 {
    let mut headers = vec![
        ("Connection", "Upgrade"),
        ("Upgrade", "websocket"),
        ("Sec-WebSocket-Version", "13"),
        ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
    ];
    if let Some(origin) = origin {
        headers.push(("Origin", origin));
    }
    status(port, "GET", path, &format!("127.0.0.1:{port}"), &headers, "").await
}

fn attached_children(port: u16) -> String {
    let output = std::process::Command::new("pgrep")
        .args(["-f", &format!("explore --attach http://127.0.0.1:{port}")])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[tokio::test]
async fn foreign_origin_websocket_upgrades_are_refused_before_any_child_starts() {
    let (_, port) = ilold_web::start_server(vec![fixture()], 0, 2).await.unwrap();
    for path in ["/ws", "/ws/pty", "/ws/pty?contract=Staking"] {
        assert_eq!(upgrade(port, path, Some(FOREIGN)).await, 403, "{path}");
        assert_eq!(upgrade(port, path, Some("null")).await, 403, "{path}");
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(attached_children(port), "", "a refused upgrade must not spawn a REPL");
}

#[tokio::test]
async fn same_origin_and_originless_upgrades_are_accepted() {
    let (_, port) = ilold_web::start_server(vec![fixture()], 0, 2).await.unwrap();
    let same = format!("http://127.0.0.1:{port}");
    assert_eq!(upgrade(port, "/ws", None).await, 101);
    assert_eq!(upgrade(port, "/ws", Some(&same)).await, 101);
    assert_eq!(upgrade(port, "/ws/pty", Some(&same)).await, 101);
}

#[tokio::test]
async fn foreign_origin_cannot_change_state() {
    let (_, port) = ilold_web::start_server(vec![fixture()], 0, 2).await.unwrap();
    let host = format!("127.0.0.1:{port}");
    let same = format!("http://{host}");
    assert_eq!(status(port, "POST", "/api/cmd", &host, &[("Origin", FOREIGN)], COMMAND).await, 403);
    assert_eq!(status(port, "POST", "/api/annotations", &host, &[("Origin", FOREIGN)], "{}").await, 403);
    assert_eq!(status(port, "DELETE", "/api/annotations/x", &host, &[("Origin", FOREIGN)], "").await, 403);
    // same host, other port: a different origin
    let other_port = format!("http://127.0.0.1:{}", port.wrapping_add(1));
    assert_eq!(status(port, "POST", "/api/cmd", &host, &[("Origin", &other_port)], COMMAND).await, 403);
    assert_eq!(status(port, "POST", "/api/cmd", &host, &[("Origin", &same)], COMMAND).await, 200);
    assert_eq!(status(port, "POST", "/api/cmd", &host, &[], COMMAND).await, 200);
}

#[tokio::test]
async fn only_this_servers_host_names_are_served() {
    let (_, port) = ilold_web::start_server(vec![fixture()], 0, 2).await.unwrap();
    for host in [format!("evil.example:{port}"), "evil.example".to_string(), format!("127.0.0.1:{}", port.wrapping_add(1))] {
        assert_eq!(status(port, "GET", "/api/project", &host, &[], "").await, 403, "{host}");
        assert_eq!(status(port, "GET", "/", &host, &[], "").await, 403, "{host}");
        assert_eq!(status(port, "POST", "/api/cmd", &host, &[], COMMAND).await, 403, "{host}");
    }
    let rebound = format!("evil.example:{port}");
    let headers = [("Connection", "Upgrade"), ("Upgrade", "websocket"), ("Sec-WebSocket-Version", "13"),
        ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="), ("Origin", "http://evil.example")];
    assert_eq!(status(port, "GET", "/ws/pty", &rebound, &headers, "").await, 403);
    for host in [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("LocalHost:{port}"), format!("[::1]:{port}")] {
        assert_eq!(status(port, "GET", "/api/project", &host, &[], "").await, 200, "{host}");
    }
}

#[tokio::test]
async fn all_interface_binds_serve_loopback_names() {
    let all = std::net::SocketAddr::from(([0, 0, 0, 0], 0));
    let (_, port, _server) = ilold_web::start_server_with_handle(vec![fixture()], all, 2).await.unwrap();
    assert_eq!(status(port, "GET", "/api/project", &format!("127.0.0.1:{port}"), &[], "").await, 200);
    assert_eq!(status(port, "GET", "/api/project", &format!("localhost:{port}"), &[], "").await, 200);
    assert_eq!(status(port, "GET", "/api/project", &format!("evil.example:{port}"), &[], "").await, 403);
}

#[tokio::test]
async fn cross_site_fetch_metadata_is_refused_even_without_origin() {
    let (_, port) = ilold_web::start_server(vec![fixture()], 0, 2).await.unwrap();
    let host = format!("127.0.0.1:{port}");
    for site in ["cross-site", "same-site"] {
        for path in ["/api/project", "/api/session/function/Staking/deposit", "/"] {
            assert_eq!(status(port, "GET", path, &host, &[("Sec-Fetch-Site", site)], "").await, 403, "{site} {path}");
        }
    }
    for site in ["same-origin", "none"] {
        assert_eq!(status(port, "GET", "/api/project", &host, &[("Sec-Fetch-Site", site)], "").await, 200, "{site}");
    }
    assert_eq!(status(port, "GET", "/api/project", &host, &[], "").await, 200);
}

#[tokio::test]
async fn favicon_is_answered_without_content() {
    let (_, port) = ilold_web::start_server(vec![fixture()], 0, 2).await.unwrap();
    assert_eq!(status(port, "GET", "/favicon.ico", &format!("127.0.0.1:{port}"), &[], "").await, 204);
}
