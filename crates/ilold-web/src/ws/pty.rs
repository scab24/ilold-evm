use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use portable_pty::{Child, CommandBuilder, NativePtySystem, PtySize, PtySystem};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct PtyQuery {
    /// Contract the attached REPL starts on (the page the terminal belongs to).
    #[serde(default)]
    pub contract: Option<String>,
}

pub async fn ws_pty_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(query): Query<PtyQuery>,
) -> impl IntoResponse {
    let port = state.port;
    let contract_path = state.contract_path.clone();
    // Only names of analyzed contracts reach the child's argv.
    let contract = match query.contract.as_deref().filter(|name| !name.is_empty()) {
        Some(name) => match state.project.find_contract(Some(name)) {
            Ok(c) => Ok(Some(c.key().to_string())),
            Err(e) => Err(e),
        },
        None => Ok(None),
    };
    ws.on_upgrade(move |mut socket| async move {
        match contract {
            Ok(contract) => handle_pty_session(socket, port, contract_path, contract).await,
            Err(e) => {
                let _ = socket.send(Message::Text(format!("PTY error: {e}\r\n").into())).await;
            }
        }
    })
}

/// PATH positional for `ilold explore --attach`; clap rejects an empty value.
pub fn attach_path(contract_path: &Path) -> PathBuf {
    if contract_path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        contract_path.to_path_buf()
    }
}

async fn handle_pty_session(
    mut socket: WebSocket,
    port: u16,
    contract_path: PathBuf,
    contract: Option<String>,
) {
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
    if let Some(contract) = &contract {
        cmd.arg("--contract");
        cmd.arg(contract);
    }
    let attach_path = attach_path(&contract_path);
    cmd.arg(attach_path.as_os_str());
    let cwd = if attach_path.is_dir() { Some(attach_path.as_path()) } else { attach_path.parent() };
    if let Some(cwd) = cwd.filter(|dir| !dir.as_os_str().is_empty()) {
        cmd.cwd(cwd);
    }

    bridge_pty(socket, cmd).await;
}

/// Kills and reaps the PTY child however the bridge ends, including when the
/// connection task itself is dropped.
struct ChildGuard(Option<Box<dyn Child + Send + Sync>>);

impl ChildGuard {
    async fn reap(mut self) {
        if let Some(child) = self.0.take() {
            let _ = tokio::task::spawn_blocking(move || kill_and_wait(child)).await;
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.0.take() {
            kill_and_wait(child);
        }
    }
}

fn kill_and_wait(mut child: Box<dyn Child + Send + Sync>) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs `cmd` in a fresh PTY wired to `socket` (binary frames both ways, JSON
/// `{"type":"resize"}` text frames). The child never outlives the socket: a
/// close frame, a dropped connection or a receive error ends it.
pub async fn bridge_pty(mut socket: WebSocket, cmd: CommandBuilder) {
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

    let child = match pair.slave.spawn_command(cmd) {
        Ok(child) => ChildGuard(Some(child)),
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
            child.reap().await;
            return;
        }
    };
    let writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("PTY writer error: {e}").into()))
                .await;
            child.reap().await;
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
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => continue,
                }
            }
        }
    }

    child.reap().await;
}
