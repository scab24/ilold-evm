#[derive(Debug, thiserror::Error)]
pub enum McpClientError {
    #[error("ilold backend unreachable at {url}: {reason}")]
    Unreachable { url: String, reason: String },
    #[error("ilold backend returned HTTP {status}: {body}")]
    HttpError { status: u16, body: String },
    #[error("invalid response from ilold backend: {0}")]
    InvalidResponse(String),
}
