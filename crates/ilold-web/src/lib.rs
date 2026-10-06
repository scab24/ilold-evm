pub mod api;
pub mod state;
pub mod ui;
pub mod ws;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::routing::{delete, get, post, put};
use axum::Router;
use tower_http::cors::CorsLayer;

use state::AppState;

fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/project", get(api::project::get_project))
        .route("/api/project/map", get(api::project::get_project_map))
        .route("/api/project/depgraph", get(api::depgraph::get_project_depgraph))
        .route("/api/contract/{name}/depgraph", get(api::depgraph::get_contract_depgraph))
        .route("/api/contract/{name}", get(api::contract::get_contract))
        .route("/api/contract/{name}/callgraph", get(api::contract::get_callgraph))
        .route("/api/contract/{name}/{func}/cfg", get(api::contract::get_cfg))
        .route("/api/contract/{name}/{func}/paths", get(api::contract::get_paths))
        .route("/api/contract/{name}/{func}/source", get(api::contract::get_function_source))
        .route("/api/contract/{name}/sequences", get(api::contract::get_sequences))
        .route("/api/contract/{name}/analysis", get(api::contract::get_sequence_analysis))
        .route("/api/contract/{name}/suggestions", get(api::contract::get_search_suggestions))
        .route("/api/annotations", get(api::annotations::list_annotations))
        .route("/api/annotations", post(api::annotations::create_annotation))
        .route("/api/annotations/{id}", put(api::annotations::update_annotation))
        .route("/api/annotations/{id}", delete(api::annotations::delete_annotation))
        .route("/api/cmd", post(api::session::handle_command))
        .route("/api/session/step/{index}/narrative", get(api::session::get_step_detail))
        .route("/api/session/step/{index}/trace", get(api::session::get_session_step_trace))
        .route("/api/session/timeline/{variable}", get(api::session::get_variable_timeline_handler))
        .route("/api/session/state", get(api::session::get_state_detail))
        .route("/api/session/sequence", get(api::session::get_sequence_detail))
        .route("/api/session/function/{contract}/{func}", get(api::session::get_function_detail))
        .route("/api/session/trace/{contract}/{func}", get(api::session::get_flow_trace))
        .route("/api/session/slice/{func}/{variable}", get(api::session::get_function_slice))
        .route("/api/scenarios", get(api::session::get_scenarios))
        .route("/api/scenarios/all", get(api::session::get_all_scenarios))
        .route("/ws", get(ws::handler::ws_handler))
        .route("/ws/pty", get(ws::pty::ws_pty_handler))
        .route("/", get(ui::root))
        .fallback(get(ui::ui_path))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Absolute project directory the embedded terminal attaches to: the path itself
/// when it is a directory, otherwise its parent. Never empty, even for `.`.
pub fn contract_path_for(path: &Path) -> PathBuf {
    let absolute = std::fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf());
    if absolute.is_dir() {
        return absolute;
    }
    match absolute.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

pub async fn serve(paths: Vec<PathBuf>, port: u16, max_seq_depth: usize) -> anyhow::Result<()> {
    println!("Analyzing {} file(s)...", paths.len());
    let contract_path = paths.first().map(|p| contract_path_for(p)).unwrap_or_else(|| contract_path_for(Path::new(".")));
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    let actual_port = listener.local_addr()?.port();
    let state = Arc::new(AppState::from_paths(&paths, max_seq_depth, actual_port, contract_path)?);
    println!(
        "Ready: {} contracts, {} functions analyzed\n",
        state.project.contracts.len(),
        state.cfgs.len(),
    );

    let app = build_router(state);
    println!("Server running at http://localhost:{actual_port}");
    axum::serve(listener, app).await?;
    Ok(())
}

pub async fn start_server(
    paths: Vec<PathBuf>, port: u16, max_seq_depth: usize,
) -> anyhow::Result<(Arc<AppState>, u16)> {
    let (state, port, _server) = start_server_with_handle(paths, port, max_seq_depth).await?;
    Ok((state, port))
}

pub async fn start_server_with_handle(
    paths: Vec<PathBuf>, port: u16, max_seq_depth: usize,
) -> anyhow::Result<(Arc<AppState>, u16, tokio::task::JoinHandle<anyhow::Result<()>>)> {
    let contract_path = paths.first().map(|p| contract_path_for(p)).unwrap_or_else(|| contract_path_for(Path::new(".")));
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    let actual_port = listener.local_addr()?.port();
    let state = Arc::new(AppState::from_paths(&paths, max_seq_depth, actual_port, contract_path)?);
    let app = build_router(state.clone());
    let server = tokio::spawn(async move {
        let result = axum::serve(listener, app).await.map_err(anyhow::Error::from);
        if let Err(ref error) = result { eprintln!("Explore server stopped: {error:#}"); }
        result
    });
    Ok((state, actual_port, server))
}
