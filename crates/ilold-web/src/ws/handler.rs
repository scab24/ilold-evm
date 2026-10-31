use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use ilold_core::classify::entry_points::AccessLevel;
use ilold_core::exploration::commands::{CanvasPatch, ScenarioEvent};

use crate::state::{AppState, SessionBroadcast};
use super::search::{self, SearchQuery};

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "search")]
    Search(SearchQuery),
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum ServerMessage {
    #[serde(rename = "search_result")]
    SearchResult(search::SearchResult),
    #[serde(rename = "search_complete")]
    SearchComplete { total: usize, truncated: bool },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "session_add_node")]
    SessionAddNode { contract: String, scenario: String, function: String, access: AccessLevel, step_index: usize },
    #[serde(rename = "session_remove_node")]
    SessionRemoveNode { contract: String, scenario: String },
    #[serde(rename = "session_clear")]
    SessionClear { contract: String, scenario: String },
    #[serde(rename = "session_highlight")]
    SessionHighlight { contract: String, scenario: String, function: String },
    #[serde(rename = "scenario_created")]
    ScenarioCreated { contract: String, name: String },
    #[serde(rename = "scenario_switched")]
    ScenarioSwitched { contract: String, from: String, to: String },
    #[serde(rename = "scenario_deleted")]
    ScenarioDeleted { contract: String, name: String },
    #[serde(rename = "scenario_forked")]
    ScenarioForked { contract: String, from: String, to: String, at_step: usize },
    #[serde(rename = "scenario_store_reloaded")]
    ScenarioStoreReloaded { contract: String, active: String },
    /// Patches were dropped for this client (broadcast lag): refetch session state.
    #[serde(rename = "session_resync")]
    SessionResync,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

fn server_message_from_patch(SessionBroadcast { contract, patch }: SessionBroadcast) -> ServerMessage {
    match patch {
        CanvasPatch::AddNode { scenario, function, access, step_index } => {
            ServerMessage::SessionAddNode { contract, scenario, function, access, step_index }
        }
        CanvasPatch::RemoveLastNode { scenario } => ServerMessage::SessionRemoveNode { contract, scenario },
        CanvasPatch::ClearAll { scenario } => ServerMessage::SessionClear { contract, scenario },
        CanvasPatch::Highlight { scenario, function } => {
            ServerMessage::SessionHighlight { contract, scenario, function }
        }
        CanvasPatch::ScenarioEvent(evt) => match evt {
            ScenarioEvent::Created { name } => ServerMessage::ScenarioCreated { contract, name },
            ScenarioEvent::Switched { from, to } => ServerMessage::ScenarioSwitched { contract, from, to },
            ScenarioEvent::Deleted { name } => ServerMessage::ScenarioDeleted { contract, name },
            ScenarioEvent::Forked { from, to, at_step } => {
                ServerMessage::ScenarioForked { contract, from, to, at_step }
            }
            ScenarioEvent::Reloaded { active } => {
                ServerMessage::ScenarioStoreReloaded { contract, active }
            }
        },
    }
}

/// Next session message for a WebSocket client as JSON text. A lagged receiver
/// gets `session_resync` instead of silently losing patches; `None` once closed.
pub async fn next_session_message(rx: &mut broadcast::Receiver<SessionBroadcast>) -> Option<String> {
    let msg = match rx.recv().await {
        Ok(update) => server_message_from_patch(update),
        Err(broadcast::error::RecvError::Lagged(_)) => ServerMessage::SessionResync,
        Err(broadcast::error::RecvError::Closed) => return None,
    };
    Some(serde_json::to_string(&msg).unwrap())
}

async fn send_json(socket: &mut WebSocket, msg: &ServerMessage) -> bool {
    let json = serde_json::to_string(msg).unwrap();
    socket.send(Message::Text(json.into())).await.is_ok()
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.session_tx.subscribe();

    loop {
        tokio::select! {
            client_msg = socket.recv() => {
                let msg = match client_msg {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => continue,
                };

                let parsed: ClientMessage = match serde_json::from_str(&msg) {
                    Ok(m) => m,
                    Err(e) => {
                        let err = ServerMessage::Error { message: format!("Invalid message: {e}") };
                        if !send_json(&mut socket, &err).await { break; }
                        continue;
                    }
                };

                match parsed {
                    ClientMessage::Search(query) => {
                        let (results, total) = search::search_paths(&state, &query, search::MAX_SEARCH_RESULTS);
                        let truncated = results.len() < total;

                        for result in results {
                            let msg = ServerMessage::SearchResult(result);
                            if !send_json(&mut socket, &msg).await { break; }
                        }

                        let complete = ServerMessage::SearchComplete { total, truncated };
                        if !send_json(&mut socket, &complete).await { break; }
                    }
                }
            }

            outbound = next_session_message(&mut rx) => {
                let Some(json) = outbound else { break };
                if socket.send(Message::Text(json.into())).await.is_err() { break; }
            }
        }
    }
}
