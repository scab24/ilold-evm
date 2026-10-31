use serde_json::Value;

use crate::error::McpClientError;

/// `value` as one URL path segment or query value: a contract id such as
/// `src/core/IToken.sol:IToken` or a signature such as `f(uint256[] storage)`.
pub fn encode_component(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b':' | b'(' | b')' | b',' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub struct IloldClient {
    base_url: String,
    http: reqwest::Client,
}

impl IloldClient {
    pub fn new(base_url: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn get(&self, path: &str) -> Result<Value, McpClientError> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| McpClientError::Unreachable {
                url: url.clone(),
                reason: e.to_string(),
            })?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(McpClientError::HttpError { status, body });
        }
        resp.json::<Value>()
            .await
            .map_err(|e| McpClientError::InvalidResponse(e.to_string()))
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<Value, McpClientError> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| McpClientError::Unreachable {
                url: url.clone(),
                reason: e.to_string(),
            })?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(McpClientError::HttpError { status, body });
        }
        resp.json::<Value>()
            .await
            .map_err(|e| McpClientError::InvalidResponse(e.to_string()))
    }

    pub async fn health_check(&self) -> Result<(), McpClientError> {
        self.get("/api/project").await.map(|_| ())
    }
}
