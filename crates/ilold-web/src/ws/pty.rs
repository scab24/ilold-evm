use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use tokio::sync::mpsc;

use crate::state::AppState;

pub async fn ws_pty_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let port = state.port;
    let contract_path = state.contract_path.clone();
    ws.on_upgrade(move |socket| handle_pty_session(socket, port, contract_path))
}

async fn handle_pty_session(mut socket: WebSocket, port: u16, contract_path: PathBuf) {
    let pty_system = NativePtySystem::default();
    let pair = match pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => pair,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("PTY error: {e}").into()))
                .await;
            return;
        }
    };

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            let _ = socket
                .send(Message::Text(
                    format!("Cannot find ilold binary: {e}").into(),
                ))
                .await;
            return;
        }
    };

    let mut cmd = CommandBuilder::new(&exe);
    cmd.arg("explore");
    cmd.arg("--attach");
    cmd.arg(format!("http://localhost:{port}"));
    cmd.arg(contract_path.to_string_lossy().as_ref());
    if let Some(parent) = contract_path.parent() {
        cmd.cwd(parent);
    }

    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(child) => child,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("Spawn error: {e}").into()))
                .await;
            return;
        }
    };
    drop(pair.slave);

    let reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("PTY reader error: {e}").into()))
                .await;
            let _ = child.kill();
            return;
        }
    };
    let writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("PTY writer error: {e}").into()))
                .await;
            let _ = child.kill();
            return;
        }
    };

    // PTY master is not Send; its IO runs on blocking threads
    let (stdout_tx, mut stdout_rx) = mpsc::channel::<Vec<u8>>(64);
    tokio::task::spawn_blocking(move || {
        let mut reader = reader;
        let mut buf = [0u8; 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if stdout_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let (stdin_tx, stdin_rx) = mpsc::channel::<Vec<u8>>(64);
    tokio::task::spawn_blocking(move || {
        let mut writer = writer;
        let mut stdin_rx = stdin_rx;
        while let Some(data) = stdin_rx.blocking_recv() {
            if writer.write_all(&data).is_err() {
                break;
            }
        }
    });

    let (resize_tx, resize_rx) = mpsc::channel::<(u16, u16)>(4);
    tokio::task::spawn_blocking(move || {
        let master = pair.master;
        let mut resize_rx = resize_rx;
        while let Some((cols, rows)) = resize_rx.blocking_recv() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    });

    loop {
        tokio::select! {
            chunk = stdout_rx.recv() => {
                match chunk {
                    Some(data) => {
                        if socket.send(Message::Binary(data.into())).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        let _ = socket.send(Message::Close(None)).await;
                        break;
                    }
                }
            }

            ws_msg = socket.recv() => {
                match ws_msg {
                    Some(Ok(Message::Binary(data))) => {
                        if stdin_tx.send(data.to_vec()).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                            if val["type"].as_str() == Some("resize") {
                                if let (Some(cols), Some(rows)) = (
                                    val["cols"].as_u64().map(|v| v as u16),
                                    val["rows"].as_u64().map(|v| v as u16),
                                ) {
                                    let _ = resize_tx.send((cols, rows)).await;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => continue,
                }
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
}
