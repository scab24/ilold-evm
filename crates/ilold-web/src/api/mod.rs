pub mod project;
pub mod contract;
pub mod depgraph;
pub mod annotations;
pub mod session;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub struct ApiError(Box<Response>);

impl From<(StatusCode, String)> for ApiError {
    fn from(error: (StatusCode, String)) -> Self {
        Self(Box::new(error.into_response()))
    }
}

impl From<Response> for ApiError {
    fn from(response: Response) -> Self {
        Self(Box::new(response))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        *self.0
    }
}
