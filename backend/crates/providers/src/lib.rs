mod anthropic;
mod gemini;
mod mock;
mod openai;

use async_trait::async_trait;
use ferrochat_core::{group_name, infer_capabilities, AppError};
use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::pin::Pin;

pub type ChatStream = Pin<Box<dyn Stream<Item = Result<ChatChunk, AppError>> + Send>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteModel {
    pub id: String,
    pub name: String,
    pub group: String,
    pub capabilities: ferrochat_core::Capabilities,
}

#[derive(Debug, Clone, Default)]
pub struct ChatChunk {
    pub content: Option<String>,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<ToolCallDelta>,
    pub done: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Clone)]
pub struct Conn {
    pub kind: String,
    pub base_url: String,
    pub api_key: String,
    pub headers: Value,
    pub client: reqwest::Client,
}

#[async_trait]
pub trait ChatProvider: Send + Sync {
    async fn list_models(&self) -> Result<Vec<RemoteModel>, AppError>;
    async fn chat_stream(&self, model: &str, body: &Value) -> Result<ChatStream, AppError>;
    async fn check(&self, model: Option<&str>) -> Result<(), AppError>;
}

pub fn build(conn: Conn) -> Box<dyn ChatProvider> {
    match conn.kind.as_str() {
        "anthropic" => Box::new(anthropic::Anthropic(conn)),
        "gemini" => Box::new(gemini::Gemini(conn)),
        "mock" => Box::new(mock::Mock),
        _ => Box::new(openai::OpenAi(conn)),
    }
}

pub fn provider_content(kind: &str, content: &Value) -> Option<Value> {
    let parts = content.as_array()?;
    let mut out = Vec::new();
    for part in parts {
        let ptype = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if ptype == "text" || part.get("text").is_some() {
            let text = part.get("text").and_then(|v| v.as_str()).unwrap_or("");
            if kind == "gemini" {
                out.push(json!({"text": text}));
            } else {
                out.push(json!({"type": "text", "text": text}));
            }
        } else if ptype == "image_url" {
            let url = part
                .pointer("/image_url/url")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if let Some((mime, data)) = data_url(url) {
                if kind == "gemini" {
                    out.push(json!({"inline_data": {"mime_type": mime, "data": data}}));
                } else {
                    out.push(json!({"type": "image", "source": {"type": "base64", "media_type": mime, "data": data}}));
                }
            }
        }
    }
    Some(json!(out))
}

fn data_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("data:")?;
    let (mime, data) = rest.split_once(";base64,")?;
    Some((mime, data))
}

pub fn next_key(keys: &str, tick: usize) -> String {
    let parts: Vec<&str> = keys
        .split([',', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return String::new();
    }
    parts[tick % parts.len()].to_string()
}

pub(crate) fn model_from_id(id: &str) -> RemoteModel {
    RemoteModel {
        id: id.to_string(),
        name: id.to_string(),
        group: group_name(id),
        capabilities: infer_capabilities(id),
    }
}

pub(crate) fn apply_headers(
    req: reqwest::RequestBuilder,
    headers: &Value,
) -> reqwest::RequestBuilder {
    let mut req = req;
    if let Some(map) = headers.as_object() {
        for (k, v) in map {
            if let Some(s) = v.as_str() {
                req = req.header(k, s);
            }
        }
    }
    req
}

pub fn openai_body(model: &str, incoming: &Value) -> Value {
    let mut body = json!({
        "model": model,
        "messages": incoming.get("messages").cloned().unwrap_or(json!([])),
        "stream": true,
    });
    if let Some(params) = incoming.get("params").and_then(|v| v.as_object()) {
        for key in [
            "temperature",
            "top_p",
            "max_tokens",
            "presence_penalty",
            "frequency_penalty",
            "seed",
            "reasoning_effort",
        ] {
            if let Some(v) = params.get(key) {
                body[key] = v.clone();
            }
        }
    }
    if let Some(tools) = incoming.get("tools") {
        body["tools"] = tools.clone();
    }
    body
}
