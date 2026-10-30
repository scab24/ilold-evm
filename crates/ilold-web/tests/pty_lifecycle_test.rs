//! The PTY child (an attached REPL in production) must end with its socket.

use std::time::{Duration, Instant};

use axum::extract::ws::WebSocketUpgrade;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use ilold_web::ws::pty::bridge_pty;
use portable_pty::CommandBuilder;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

type Client = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A bridge whose child prints its pid and then waits forever.
async fn serve() -> u16 {
    let app = Router::new().route(
        "/pty",
        get(|ws: WebSocketUpgrade| async move {
            ws.on_upgrade(|socket| async move {
                let mut cmd = CommandBuilder::new("/bin/sh");
                cmd.args(["-c", "echo PID=$$; exec sleep 600"]);
                bridge_pty(socket, cmd).await;
            })
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}

async fn connect_and_read_pid(port: u16) -> (Client, u32) {
    let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/pty")).await.unwrap();
    let mut out = String::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let msg = tokio::time::timeout(deadline - Instant::now(), ws.next())
            .await
            .expect("timed out waiting for the child pid")
            .expect("socket ended before the pid")
            .unwrap();
        if let Message::Binary(data) = msg {
            out.push_str(&String::from_utf8_lossy(&data));
        }
        if let Some(pid) = out.split("PID=").nth(1).and_then(|rest| rest.split_whitespace().next()) {
            if out.contains('\n') {
                return (ws, pid.parse().unwrap());
            }
        }
    }
}

fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

async fn assert_gone(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while alive(pid) {
        assert!(Instant::now() < deadline, "child {pid} still running after its socket closed");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn child_ends_when_the_socket_closes() {
    let port = serve().await;
    let (mut ws, pid) = connect_and_read_pid(port).await;
    assert!(alive(pid));
    ws.send(Message::Close(None)).await.unwrap();
    assert_gone(pid).await;
}

#[tokio::test]
async fn child_ends_when_the_connection_drops_without_close() {
    let port = serve().await;
    let (ws, pid) = connect_and_read_pid(port).await;
    assert!(alive(pid));
    drop(ws);
    assert_gone(pid).await;
}
