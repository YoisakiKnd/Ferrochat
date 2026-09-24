use super::{apply_headers, model_from_id, ChatChunk, ChatProvider, ChatStream, Conn, RemoteModel};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use ferrochat_core::AppError;
use futures::StreamExt;
use serde_json::{json, Value};

pub struct Anthropic(pub Conn);

#[async_trait]
impl ChatProvider for Anthropic {
    async fn list_models(&self) -> Result<Vec<RemoteModel>, AppError> {
        let url = format!("{}/v1/models", self.0.base_url.trim_end_matches('/'));
        let req = apply_headers(
            self.0
                .client
                .get(url)
                .header("x-api-key", &self.0.api_key)
                .header("anthropic-version", "2023-06-01"),
            &self.0.headers,
        );
        let res = req
            .send()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        let body: Value = res
            .json()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        let data = body
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(data
            .iter()
            .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(model_from_id))
            .collect())
    }

    async fn chat_stream(&self, model: &str, body: &Value) -> Result<ChatStream, AppError> {
        let url = format!("{}/v1/messages", self.0.base_url.trim_end_matches('/'));
        let payload = to_anthropic(model, body);
        let req = apply_headers(
            self.0
                .client
                .post(url)
                .header("x-api-key", &self.0.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&payload),
            &self.0.headers,
        );
        let res = req
            .send()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        if !res.status().is_success() {
            return Err(AppError::BadRequest(res.text().await.unwrap_or_default()));
        }
        let stream = res.bytes_stream().eventsource().map(|event| match event {
            Ok(ev) => {
                let v: Value = serde_json::from_str(&ev.data).unwrap_or(json!({}));
                let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if kind == "content_block_delta" {
                    let delta = v.get("delta").cloned().unwrap_or(json!({}));
                    let dtype = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if dtype == "thinking_delta" {
                        return Ok(ChatChunk {
                            reasoning: delta
                                .get("thinking")
                                .and_then(|t| t.as_str())
                                .map(|s| s.to_string()),
                            ..Default::default()
                        });
                    }
                    return Ok(ChatChunk {
                        content: delta
                            .get("text")
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string()),
                        ..Default::default()
                    });
                }
                if kind == "message_stop" {
                    return Ok(ChatChunk {
                        done: true,
                        ..Default::default()
                    });
                }
                if kind == "error" {
                    return Ok(ChatChunk {
                        error: Some(v.to_string()),
                        done: true,
                        ..Default::default()
                    });
                }
                Ok(ChatChunk::default())
            }
            Err(e) => Err(AppError::Internal(e.to_string())),
        });
        Ok(Box::pin(stream))
    }

    async fn check(&self, model: Option<&str>) -> Result<(), AppError> {
        if model.is_some() {
            let body = json!({"messages":[{"role":"user","content":"ping"}]});
            let mut stream = self.chat_stream(model.unwrap(), &body).await?;
            if let Some(item) = stream.next().await {
                item?;
            }
            Ok(())
        } else {
            self.list_models().await.map(|_| ())
        }
    }
}

fn to_anthropic(model: &str, incoming: &Value) -> Value {
    let messages = incoming
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut system = String::new();
    let mut out = Vec::new();
    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content = msg.get("content").cloned().unwrap_or(json!(""));
        let text = content_to_text(&content);
        if role == "system" {
            if !system.is_empty() {
                system.push('\n');
            }
            system.push_str(&text);
        } else {
            out.push(json!({"role": if role == "assistant" { "assistant" } else { "user" }, "content": super::provider_content("anthropic", &content).unwrap_or(json!(text))}));
        }
    }
    let mut body = json!({
        "model": model,
        "max_tokens": 4096,
        "messages": out,
        "stream": true,
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    body
}

fn content_to_text(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        return arr
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
    }
    content.to_string()
}
