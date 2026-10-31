use axum::{http::{header, StatusCode, Uri}, response::{IntoResponse, Response}};

include!(concat!(env!("OUT_DIR"), "/ui_assets.rs"));

pub async fn root() -> Response {
    asset("index.html")
}

pub async fn ui_path(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    // never return HTML for a missing API/WS endpoint or asset
    if is_contract_route(path) {
        return asset("index.html");
    }
    asset(path)
}

/// The name must not be a file.
fn is_contract_route(path: &str) -> bool {
    let route = path.strip_suffix('/').unwrap_or(path);
    match route.split_once('/') {
        Some(("contract", name)) => !name.is_empty() && !name.contains('/') && !name.contains('.'),
        _ => false,
    }
}

fn asset(path: &str) -> Response {
    let Some((_, bytes)) = ASSETS.iter().find(|(name, _)| *name == path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };
    ([(header::CONTENT_TYPE, mime)], bytes.to_vec()).into_response()
}
